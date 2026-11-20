"""Harvests font metrics from the fonts embedded in PDFs made by Word:
units per em, Windows ascent and descent, external leading, and the advance
width of each character. Writes one JSON object per face."""
import io, json, sys, re, collections
from pypdf import PdfReader
from fontTools.ttLib import TTFont

WIN_ANSI = {i: chr(i) for i in range(32, 127)}
WIN_ANSI.update({i: bytes([i]).decode('cp1252', 'replace') for i in range(128, 256)})
STYLE_SUFFIXES = ('BoldItalic', 'BoldOblique', 'Bold', 'Italic', 'Regular', 'Oblique')

faces = collections.defaultdict(lambda: {'widths': {}, 'vertical': None, 'sources': 0})


def strip_style(family):
    """Removes trailing style words (Bold, Italic, ...) from a family name."""
    stripped = True
    while stripped:
        stripped = False
        for suffix in STYLE_SUFFIXES:
            if family.endswith(suffix) and len(family) > len(suffix):
                family = family[:-len(suffix)]
                stripped = True
    return family


def style_of(name):
    n = name.lower().replace('-', ',').replace(' ', '')
    bold = any(k in n for k in ['bold', 'black', 'heavy', 'semibold', 'demi'])
    italic = 'italic' in n or 'oblique' in n
    family = re.split(r'[,-]', name)[0]
    for suffix in ('PSMT', 'MT'):
        if family.endswith(suffix):
            family = family[:-len(suffix)]
            break
    return strip_style(family), bold, italic


def page_fonts(page):
    """The font dictionaries of a page, or nothing."""
    try:
        fonts = page['/Resources']['/Font']
    except Exception:
        return []
    out = []
    for key in fonts:
        try:
            out.append(fonts[key].get_object())
        except Exception:
            continue
    return out


def embedded_truetype(font):
    """The embedded TrueType program of a font, as a TTFont, or None."""
    desc_font = font
    if font.get('/Subtype') == '/Type0':
        try:
            desc_font = font['/DescendantFonts'][0].get_object()
        except Exception:
            return None
    fd = desc_font.get('/FontDescriptor')
    if fd is None:
        return None
    ff = fd.get_object().get('/FontFile2')
    if ff is None:
        return None
    try:
        return TTFont(io.BytesIO(ff.get_object().get_data()), lazy=True)
    except Exception:
        return None


def vertical_of(tt):
    """The vertical metrics of a font."""
    upem = tt['head'].unitsPerEm
    os2 = tt['OS/2'] if 'OS/2' in tt else None
    hhea = tt['hhea']
    return {
        'upem': upem,
        'win_ascent': os2.usWinAscent if os2 else hhea.ascent,
        'win_descent': os2.usWinDescent if os2 else -hhea.descent,
        'hhea_ascent': hhea.ascent,
        'hhea_descent': hhea.descent,
        'line_gap': hhea.lineGap,
    }


def widths_of(font, tt, upem):
    """Character to advance width: the font's own cmap, else the PDF's encoding."""
    widths = {}
    hmtx = tt['hmtx']
    try:
        cmap = tt.getBestCmap() or {}
        for code, glyph in cmap.items():
            if glyph in hmtx.metrics:
                widths[code] = hmtx.metrics[glyph][0]
    except Exception:
        pass
    if font.get('/Subtype') == '/TrueType':
        for code, w in encoded_widths(font, upem).items():
            widths.setdefault(code, w)
    return widths


def encoded_widths(font, upem):
    """Character to advance width from a simple font's `/Widths`, read
    with the Windows encoding."""
    widths = {}
    ws = font.get('/Widths')
    if ws is None:
        return widths
    first = int(font.get('/FirstChar', 0))
    values = ws.get_object() if hasattr(ws, 'get_object') else ws
    for i, w in enumerate(values):
        ch = WIN_ANSI.get(first + i)
        if ch and float(w) > 0:
            widths[ord(ch)] = round(float(w) * upem / 1000.0)
    return widths


def harvest_font(font, seen):
    base = re.sub(r'^[A-Z]{6}\+', '', str(font.get('/BaseFont', ''))[1:])
    ident = id(font)
    if not base or (base, ident) in seen:
        return
    seen.add((base, ident))
    tt = embedded_truetype(font)
    if tt is None:
        return
    try:
        vertical = vertical_of(tt)
        widths = widths_of(font, tt, vertical['upem'])
    except Exception:
        return
    entry = faces[style_of(base)]
    entry['sources'] += 1
    if entry['vertical'] is None:
        entry['vertical'] = vertical
    # Word's subsets often map codes to the font's own Windows cmap
    # with Symbol-style codes (0xF0xx): only plain codes are kept.
    for code, w in widths.items():
        if code < 0xF000 or code > 0xF0FF:
            entry['widths'][code] = int(w)


def harvest(path):
    try:
        reader = PdfReader(path)
    except Exception:
        return
    seen = set()
    for page in reader.pages:
        for font in page_fonts(page):
            harvest_font(font, seen)


for path in sys.argv[1:]:
    harvest(path)
out = []
for (family, bold, italic), entry in sorted(faces.items()):
    if entry['vertical'] is None:
        continue
    out.append({'family': family, 'bold': bold, 'italic': italic, 'sources': entry['sources'],
                **entry['vertical'], 'widths': {str(k): v for k, v in sorted(entry['widths'].items())}})
json.dump(out, sys.stdout)
