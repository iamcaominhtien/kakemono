#!/usr/bin/env python3
"""Compares the layout of a PDF made by kakemono from a Word document with
the PDF Word itself made: how many lines break at the same words, and
whether they land on the same page.

Usage: compare-layout.py REFERENCE.pdf OURS.pdf [--json | --diff]

Lines are read with `kakemono lines` (glyph baselines). A line of the
reference is matched when a line of ours has exactly the same words (in
any order of reading, so that tables and text boxes do not matter); each
line of ours matches once, the one on the nearest page first. Spaces are
not compared: Word's PDFs leave some out.
"""
import collections, html, json, os, re, subprocess, sys, unicodedata

# Quotes compared as plain ones. Word's PDFs of some documents give the
# curly apostrophe and opening quote the Unicode of '‟' and '„' (a wrong
# ToUnicode map), so these are folded too, on both sides alike.
QUOTES = str.maketrans({'’': "'", '‘': "'", '“': '"', '”': '"', '‟': "'", '„': "'", ' ': ' '})


LEADER = re.compile(r' ?([.\-_·…]{3,}) ?')
BULLETS = set('•·▪■●◆○□◦‣➢➤✓✔')


def norm(text):
    """A word as compared: NFKC, plain quotes; a lone symbol (a bullet,
    which Word's PDFs often draw with no Unicode for it) is one mark."""
    text = unicodedata.normalize('NFKC', html.unescape(text)).translate(QUOTES)
    if len(text) == 1 and (text in BULLETS or text == '\ufffd' or unicodedata.category(text) in ('So', 'Co')):
        return '•'
    return text


def mark(ch):
    """A character as compared: a symbol (a bullet, ©) is one mark."""
    if ch in BULLETS or ch == '\ufffd' or unicodedata.category(ch) in ('So', 'Co'):
        return '•'
    return ch


KAKEMONO = os.environ.get('KAKEMONO', os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'kakemono'))


def lines(pdf):
    """The lines of a PDF, as `kakemono lines` reads them: (page, baseline,
    left, text), and the page count."""
    out = subprocess.run([KAKEMONO, 'lines', '--', os.path.realpath(pdf)], capture_output=True, text=True).stdout.splitlines()
    pages = int(out[0].split('\t')[1]) if out and out[0].startswith('pages') else 0
    result = []
    for row in out[1:]:
        page, base, left, _, text = row.split('\t', 4)
        text = ' '.join(norm(word) for word in text.split())
        # A tab leader: whether a space precedes it depends on how the PDF
        # writes the tab, not on the layout.
        text = LEADER.sub(lambda match: match.group(1), text)
        # Without its spaces, and with every symbol as one mark: Word's PDFs
        # leave out spaces typed in runs of their own ("householdsrepresentative")
        # and the one after a symbol ("©2013"). Where a line breaks does
        # not depend on them.
        text = ''.join(mark(ch) for ch in text if not ch.isspace())
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
            best = min(range(len(pages)), key=lambda i, pages=pages, page=page: abs(pages[i] - page))
            our_page = pages.pop(best)
            matched += 1
            same_page += our_page == page
        else:
            missing.append((page, text))
    ref_words = collections.Counter(word for *_, text in ref for word in text.split())
    our_words = collections.Counter(word for *_, text in ours for word in text.split())
    result = {
        'word_ratio': sum((ref_words & our_words).values()) / max(1, sum(ref_words.values())),
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


# A path as given on the command line: letters, digits and `_ . / + -`, not
# starting with `-` (which `kakemono lines` would read as an option).
SAFE_PATH = re.compile(r'[\w./+][\w./+-]*')


if __name__ == '__main__':
    ref_pdf, our_pdf = sys.argv[1], sys.argv[2]
    if not (SAFE_PATH.fullmatch(ref_pdf) and SAFE_PATH.fullmatch(our_pdf)):
        sys.exit('compare-layout.py: unexpected characters in a path')
    if '--diff' in sys.argv:
        diff(ref_pdf, our_pdf)
        sys.exit()
    result = compare(ref_pdf, our_pdf)
    if '--json' in sys.argv:
        print(json.dumps(result))
    else:
        for k, v in result.items():
            print(f'{k:16} {v:.3f}' if isinstance(v, float) else f'{k:16} {v}')
