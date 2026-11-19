"""Harvests font metrics from the fonts embedded in PDFs made by Word:
units per em, Windows ascent and descent, external leading, and the advance
width of each character. Writes one JSON object per face."""
import io, json, sys, glob, re, collections
from pypdf import PdfReader
from fontTools.ttLib import TTFont

WIN_ANSI = {i: chr(i) for i in range(32, 127)}
WIN_ANSI.update({i: bytes([i]).decode('cp1252', 'replace') for i in range(128, 256)})

faces = collections.defaultdict(lambda: {'widths': {}, 'vertical': None, 'sources': 0})

def style_of(name):
    n = name.lower().replace('-', ',').replace(' ', '')
    bold = any(k in n for k in ['bold', 'black', 'heavy', 'semibold', 'demi'])
    italic = 'italic' in n or 'oblique' in n
    family = re.split(r'[,-]', name)[0]
    family = re.sub(r'(PS)?MT$', '', family)
    family = re.sub(r'(Bold|Italic|Regular|BoldItalic|BoldOblique|Oblique)+$', '', family)
    return family, bold, italic

def harvest(path):
    try:
        reader = PdfReader(path)
    except Exception:
        return
    seen = set()
    for page in reader.pages:
        try:
            fonts = page['/Resources']['/Font']
        except Exception:
            continue
        for key in fonts:
            try:
                font = fonts[key].get_object()
            except Exception:
                continue
            ident = id(font)
            base = str(font.get('/BaseFont', ''))[1:]
            base = re.sub(r'^[A-Z]{6}\+', '', base)
            if not base or (base, ident) in seen:
                continue
            seen.add((base, ident))
            subtype = font.get('/Subtype')
            desc_font = font
            if subtype == '/Type0':
                try:
                    desc_font = font['/DescendantFonts'][0].get_object()
                except Exception:
                    continue
            fd = desc_font.get('/FontDescriptor')
            if fd is None:
                continue
            fd = fd.get_object()
            ff = fd.get('/FontFile2')
            if ff is None:
                continue
            try:
                data = ff.get_object().get_data()
                tt = TTFont(io.BytesIO(data), lazy=True)
                upem = tt['head'].unitsPerEm
                os2 = tt['OS/2'] if 'OS/2' in tt else None
                hhea = tt['hhea']
                vertical = dict(upem=upem,
                                win_ascent=os2.usWinAscent if os2 else hhea.ascent,
                                win_descent=os2.usWinDescent if os2 else -hhea.descent,
                                hhea_ascent=hhea.ascent, hhea_descent=hhea.descent, line_gap=hhea.lineGap)
                hmtx = tt['hmtx']
            except Exception:
                continue
            family, bold, italic = style_of(base)
            entry = faces[(family, bold, italic)]
            entry['sources'] += 1
            if entry['vertical'] is None:
                entry['vertical'] = vertical
            # Character to glyph: the font's own cmap, else the PDF's encoding.
            widths = {}
            try:
                cmap = tt.getBestCmap() or {}
                for code, glyph in cmap.items():
                    if glyph in hmtx.metrics:
                        widths[code] = hmtx.metrics[glyph][0]
            except Exception:
                pass
            if subtype == '/TrueType':
                first = int(font.get('/FirstChar', 0)); ws = font.get('/Widths')
                if ws is not None:
                    for i, w in enumerate(ws.get_object() if hasattr(ws, 'get_object') else ws):
                        ch = WIN_ANSI.get(first + i)
                        w = float(w)
                        if ch and w > 0:
                            widths.setdefault(ord(ch), round(w * upem / 1000.0))
            # Word's subsets often map codes to the font's own Windows cmap
            # with Symbol-style codes (0xF0xx): only plain codes are kept.
            for code, w in widths.items():
                if code < 0xF000 or code > 0xF0FF:
                    entry['widths'][code] = int(w)

for path in sys.argv[1:]:
    harvest(path)
out = []
for (family, bold, italic), entry in sorted(faces.items()):
    if entry['vertical'] is None:
        continue
    out.append(dict(family=family, bold=bold, italic=italic, sources=entry['sources'], **entry['vertical'],
                    widths={str(k): v for k, v in sorted(entry['widths'].items())}))
json.dump(out, sys.stdout)
