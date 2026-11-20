"""Builds the metric table kakemono embeds (src/layout/metrics.txt) from
harvested faces: fonts Word documents use that have no installed or
metric-compatible stand-in."""
import json, os, sys, tempfile


def confined(path):
    """The path, resolved; it must lie inside the working directory or the
    temporary directory, where corpora and outputs are kept."""
    real = os.path.realpath(path)
    for base in (os.getcwd(), tempfile.gettempdir()):
        base = os.path.realpath(base)
        if real == base or real.startswith(base + os.sep):
            return real
    sys.exit(f'{path}: not inside the working or temporary directory')


SEGOE_UI = 'Segoe UI'
TREBUCHET = 'Trebuchet MS'
ARIAL_BLACK = 'Arial Black'
ARIAL_UNICODE = 'Arial Unicode MS'
CALIBRI_LIGHT = 'Calibri Light'
with open(confined(sys.argv[1])) as source:
    faces = json.load(source)
NAMES = {
    'Tahoma': 'Tahoma', 'Verdana': 'Verdana', 'SegoeUI': SEGOE_UI, 'Segoe UI': SEGOE_UI,
    'SegoeUISymbol': 'Segoe UI Symbol', 'TrebuchetMS': TREBUCHET, 'Trebuchet MS': TREBUCHET,
    'Georgia': 'Georgia', 'BookAntiqua': 'Book Antiqua', 'ArialBlack': ARIAL_BLACK, 'Arial Black': ARIAL_BLACK,
    'LucidaConsole': 'Lucida Console', 'MonotypeCorsiva': 'Monotype Corsiva', 'ArialUnicodeMS': ARIAL_UNICODE,
    'Arial Unicode MS': ARIAL_UNICODE, 'Lucida Sans Unicode': 'Lucida Sans Unicode', 'GillSansMT': 'Gill Sans MT',
    'CenturyGothic': 'Century Gothic', 'Garamond': 'Garamond', 'FranklinGothic': 'Franklin Gothic', 'CalibriLight': CALIBRI_LIGHT,
    'Calibri Light': CALIBRI_LIGHT, 'Aptos': 'Aptos', 'Constantia': 'Constantia', 'Candara': 'Candara', 'Corbel': 'Corbel',
    'Consolas': 'Consolas',
}
best = {}
for f in faces:
    name = NAMES.get(f['family'])
    if not name or len(f['widths']) < 90:
        continue
    key = (name, f['bold'], f['italic'])
    if key not in best or len(f['widths']) > len(best[key]['widths']):
        best[key] = f
out = ['# Advance widths and vertical metrics of fonts Word documents use, read from',
       '# the fonts embedded in PDFs that Word made (metrics only, no outlines).',
       '# face <family> <bold> <italic> <upem> <winAscent> <winDescent> <hheaAscent> <hheaDescent> <lineGap>',
       '# <first code> <width> <width> ... : consecutive characters, widths in font units']
for (name, bold, italic), f in sorted(best.items()):
    out.append(f"face\t{name}\t{int(bold)}\t{int(italic)}\t{f['upem']}\t{f['win_ascent']}\t{f['win_descent']}\t{f['hhea_ascent']}\t{f['hhea_descent']}\t{f['line_gap']}")
    codes = sorted(int(k) for k in f['widths'] if int(k) < 0x2600 or 0xFB00 <= int(k) <= 0xFB06)
    run = []
    def flush():
        if run:
            out.append(f"{run[0][0]:x}\t" + ' '.join(str(w) for _, w in run))
    for c in codes:
        w = f['widths'][str(c)]
        if run and c != run[-1][0] + 1:
            flush(); run = []
        run.append((c, w))
    flush()
with open(confined(sys.argv[2]), 'w') as target:
    target.write('\n'.join(out) + '\n')
print(len(best), 'faces', sorted(best))
