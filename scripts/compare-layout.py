#!/usr/bin/env python3
"""Compares the layout of a PDF made by kakemono from a Word document with
the PDF Word itself made: how many lines break at the same words, and
whether they land on the same page.

Usage: compare-layout.py REFERENCE.pdf OURS.pdf [--json | --diff]

Lines are read with `kakemono lines` (glyph baselines). A line of the
reference is matched when a line of ours has exactly the same words (in
any order of reading, so that tables and text boxes do not matter); each
line of ours matches once, the one on the nearest page first.
"""
import collections, html, json, os, re, subprocess, sys, unicodedata

QUOTES = str.maketrans({'’': "'", '‘': "'", '“': '"', '”': '"', ' ': ' '})


def norm(text):
    """A word as compared: NFKC, plain quotes."""
    return unicodedata.normalize('NFKC', html.unescape(text)).translate(QUOTES)


KAKEMONO = os.environ.get('KAKEMONO', os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'kakemono'))


def lines(pdf):
    """The lines of a PDF, as `kakemono lines` reads them: (page, baseline,
    left, text), and the page count."""
    out = subprocess.run([KAKEMONO, 'lines', pdf], capture_output=True, text=True).stdout.splitlines()
    pages = int(out[0].split('\t')[1]) if out and out[0].startswith('pages') else 0
    result = []
    for row in out[1:]:
        page, base, left, right, text = row.split('\t', 4)
        text = ' '.join(norm(word) for word in text.split())
        if text:
            result.append((int(page), float(base), float(left), text))
    return result, pages


def compare(ref_pdf, our_pdf, detail=False):
    ref, ref_pages = lines(ref_pdf)
    ours, our_pages = lines(our_pdf)
    available = collections.defaultdict(list)
    for page, top, left, text in ours:
        available[text].append(page)
    matched = same_page = 0
    missing = []
    for page, top, left, text in ref:
        pages = available.get(text)
        if pages:
            best = min(range(len(pages)), key=lambda i: abs(pages[i] - page))
            our_page = pages.pop(best)
            matched += 1
            same_page += our_page == page
        else:
            missing.append((page, text))
    result = {
        'ref_pages': ref_pages, 'our_pages': our_pages,
        'ref_lines': len(ref), 'our_lines': len(ours), 'lines_matched': matched,
        'line_ratio': matched / max(1, len(ref)),
        'same_page_ratio': same_page / max(1, len(ref)),
    }
    if detail:
        extra = [(page, text) for text, pages in available.items() for page in pages]
        return result, missing, sorted(extra)
    return result


def diff(ref_pdf, our_pdf, limit=60):
    """Prints, page by page, the reference lines no line of ours matches, and
    our lines that match none."""
    _, missing, extra = compare(ref_pdf, our_pdf, detail=True)
    pages = sorted({page for page, _ in missing} | {page for page, _ in extra})
    shown = 0
    for page in pages:
        print(f'--- page {page + 1}')
        for p, text in missing:
            if p == page:
                print(f'  REF : {text}')
        for p, text in extra:
            if p == page:
                print(f'  OURS: {text}')
        shown += 1
        if shown >= limit:
            break


if __name__ == '__main__':
    if '--diff' in sys.argv:
        diff(sys.argv[1], sys.argv[2])
        sys.exit()
    result = compare(sys.argv[1], sys.argv[2])
    if '--json' in sys.argv:
        print(json.dumps(result))
    else:
        for k, v in result.items():
            print(f'{k:16} {v:.3f}' if isinstance(v, float) else f'{k:16} {v}')
