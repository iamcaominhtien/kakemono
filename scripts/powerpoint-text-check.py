#!/usr/bin/env python3
"""Compares kakemono's reading of PowerPoint decks with python-pptx's, an
independent reader: what share of each deck's words kakemono keeps, whether
each slide's title is a heading, and whether text that must not reach the
document (placeholder prompts, footers, slide numbers) does.

Usage: powerpoint-text-check.py [DECK.pptx | DIR ...] [--kakemono PATH] [--out FILE.tsv]
(default: the decks of tests/data/powerpoint-corpus/). Run it with a Python
that has python-pptx (`pip install python-pptx`), with `python3 -I`, since
the decks are untrusted files.

Per deck (a TSV row): visible slides; words of python-pptx's text of the
visible slides (shapes, groups, tables; not charts, SmartArt or notes) and
the share of them found in kakemono's text (recall, counted as multisets of
words, a CJK character being a word); words of the notes and their recall;
slide titles and the share found as headings; prompt texts ("Click to ...")
and footer texts found in kakemono's text (each should be 0). A last row
gives the totals. Two runs' files can be diffed before and after a change.
"""

import argparse
import collections
import json
import os
import re
import subprocess
import sys
import tempfile

from pptx import Presentation
from pptx.enum.shapes import MSO_SHAPE_TYPE, PP_PLACEHOLDER

here = os.path.dirname(os.path.abspath(__file__))
parser = argparse.ArgumentParser()
parser.add_argument('paths', nargs='*', default=[os.path.join(here, '..', 'tests', 'data', 'powerpoint-corpus')])
parser.add_argument('--kakemono', default=os.path.join(here, '..', 'target', 'release', 'kakemono'))
parser.add_argument('--out', help='TSV file to write (default: standard output)')
args = parser.parse_args()

# Placeholders whose text is page furniture, left out of the document.
FURNITURE = {PP_PLACEHOLDER.DATE, PP_PLACEHOLDER.FOOTER, PP_PLACEHOLDER.SLIDE_NUMBER, PP_PLACEHOLDER.HEADER}
TITLES = {PP_PLACEHOLDER.TITLE, PP_PLACEHOLDER.CENTER_TITLE, PP_PLACEHOLDER.VERTICAL_TITLE}
# A word: a run of letters or digits, or one CJK character (they are not
# separated by spaces).
WORD = re.compile(r'[⺀-鿿豈-﫿ｦ-ﾟ]|[^\W_]+', re.UNICODE)
PROMPT = re.compile(r'\bClick to (add|edit)\b', re.IGNORECASE)


def words(text):
    return [word.lower() for word in WORD.findall(text)]


def shape_texts(shapes, out, furniture):
    """The text of `shapes` (groups and tables included) into `out`; the
    text of furniture placeholders into `furniture`."""
    for shape in shapes:
        if shape.shape_type == MSO_SHAPE_TYPE.GROUP:
            shape_texts(shape.shapes, out, furniture)
            continue
        placeholder = shape.placeholder_format.type if shape.is_placeholder else None
        target = furniture if placeholder in FURNITURE else out
        if shape.has_text_frame:
            target.append(shape.text_frame.text)
        if getattr(shape, 'has_table', False) and shape.has_table:
            for row in shape.table.rows:
                for cell in row.cells:
                    if not cell.is_spanned:
                        target.append(cell.text)


def reference(path):
    """What python-pptx reads of the visible slides of `path`."""
    deck = Presentation(path)
    slides = []
    for slide in deck.slides:
        if slide._element.get('show') == '0':
            continue
        texts, furniture = [], []
        shape_texts(slide.shapes, texts, furniture)
        title = ''
        for shape in slide.placeholders:
            if shape.placeholder_format.type in TITLES and shape.has_text_frame:
                title = ' '.join(shape.text_frame.text.split())
                break
        notes = ''
        if slide.has_notes_slide and slide.notes_slide.notes_text_frame is not None:
            notes = slide.notes_slide.notes_text_frame.text
        slides.append({'texts': texts, 'furniture': furniture, 'title': title, 'notes': notes})
    return slides


def strings(value, out):
    """Every `plain` text of kakemono's editable JSON, in order."""
    if isinstance(value, dict):
        for key, item in value.items():
            if key == 'plain' and isinstance(item, str):
                out.append(item)
            else:
                strings(item, out)
    elif isinstance(value, list):
        for item in value:
            strings(item, out)


def ours(path, workdir):
    """kakemono's text of `path` (all of it) and its headings."""
    out = os.path.join(workdir, 'deck.json')
    result = subprocess.run([args.kakemono, 'convert', path, '-o', out], capture_output=True, text=True, timeout=300)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.strip().splitlines()[-1] if result.stderr.strip() else 'failed')
    with open(out, encoding='utf-8') as file:
        doc = json.load(file)
    texts = []
    strings(doc.get('blocks', []), texts)
    headings = [' '.join(block.get('plain', '').split()) for block in doc.get('blocks', [])
                if block.get('type') in ('heading', 'title')]
    return '\n'.join(texts), headings


def recall(expected, found):
    """Words of `expected` found in `found` (multisets), and how many."""
    expected, found = collections.Counter(expected), collections.Counter(found)
    kept = sum(min(count, found[word]) for word, count in expected.items())
    return kept, sum(expected.values())


def decks(paths):
    for path in paths:
        if os.path.isdir(path):
            for name in sorted(os.listdir(path)):
                if name.lower().endswith(('.pptx', '.pptm', '.ppsx', '.potx')):
                    yield os.path.join(path, name)
        else:
            yield path


def main():
    columns = ['deck', 'slides', 'words', 'recall', 'notes_words', 'notes_recall', 'titles', 'titles_found',
               'prompts', 'footers', 'error']
    rows = []
    totals = collections.Counter()
    with tempfile.TemporaryDirectory() as workdir:
        for path in decks(args.paths):
            name = os.path.splitext(os.path.basename(path))[0]
            try:
                slides = reference(path)
                text, headings = ours(path, workdir)
            except Exception as error:  # a deck either reader fails on is reported, not fatal
                rows.append([name] + [''] * (len(columns) - 2) + [str(error)[:120]])
                totals['errors'] += 1
                continue
            found = words(text)
            kept, count = recall(words('\n'.join(t for slide in slides for t in slide['texts'])), found)
            notes_kept, notes_count = recall(words('\n'.join(slide['notes'] for slide in slides)), found)
            titles = [slide['title'] for slide in slides if slide['title']]
            heading_set = set(headings)
            titles_found = sum(1 for title in titles if title in heading_set)
            prompts = len(PROMPT.findall(text))
            # A footer's text counts when it is not also in the slides' own
            # text (spaces compared as one).
            flat = lambda value: ' '.join(value.split())
            body = flat('\n'.join(t for slide in slides for t in slide['texts']))
            ours_flat = flat(text)
            footers = sum(1 for slide in slides for t in slide['furniture']
                          if len(flat(t)) > 3 and flat(t) not in body and flat(t) in ours_flat)
            rows.append([name, len(slides), count, f'{kept / count:.3f}' if count else '',
                         notes_count, f'{notes_kept / notes_count:.3f}' if notes_count else '',
                         len(titles), titles_found, prompts, footers, ''])
            totals.update({'slides': len(slides), 'words': count, 'kept': kept, 'notes_words': notes_count,
                           'notes_kept': notes_kept, 'titles': len(titles), 'titles_found': titles_found,
                           'prompts': prompts, 'footers': footers})
    ratio = lambda part, whole: f'{totals[part] / totals[whole]:.3f}' if totals[whole] else ''
    rows.append(['TOTAL', totals['slides'], totals['words'], ratio('kept', 'words'), totals['notes_words'],
                 ratio('notes_kept', 'notes_words'), totals['titles'], totals['titles_found'], totals['prompts'],
                 totals['footers'], f"{totals['errors']} errors"])
    lines = ['\t'.join(columns)] + ['\t'.join(str(value) for value in row) for row in rows]
    output = '\n'.join(lines) + '\n'
    if args.out:
        with open(args.out, 'w', encoding='utf-8') as file:
            file.write(output)
    sys.stdout.write(output if not args.out else lines[-1] + '\n')


if __name__ == '__main__':
    main()
