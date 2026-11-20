#!/usr/bin/env python3
"""For each document, the first page whose last line does not end the same
page in ours, with the lines around it: where pagination first goes wrong.

Usage: first-divergence.py CORPUS_DIR IDS_FILE OUT_DIR
"""
import collections, importlib.util, os, sys, tempfile
spec = importlib.util.spec_from_file_location('c', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'compare-layout.py'))
c = importlib.util.module_from_spec(spec); spec.loader.exec_module(c)


def confined(path):
    """The path, resolved; it must lie inside the working directory or the
    temporary directory, where corpora and outputs are kept."""
    real = os.path.realpath(path)
    for base in (os.getcwd(), tempfile.gettempdir()):
        base = os.path.realpath(base)
        if real == base or real.startswith(base + os.sep):
            return real
    sys.exit(f'{path}: not inside the working or temporary directory')


corpus, ids, out = (confined(arg) for arg in sys.argv[1:4])
with open(ids) as listing:
    names = listing.read().split()
for doc_id in names:
    ref, rp = c.lines(os.path.join(corpus, doc_id + '.pdf')); ours, op = c.lines(os.path.join(out, doc_id + '.pdf'))
    count = collections.Counter(l[3] for l in ref)
    where = collections.defaultdict(list)
    for l in ours: where[l[3]].append(l)
    by_page = collections.defaultdict(list)
    for l in ref: by_page[l[0]].append(l)
    found = None
    for page in sorted(by_page):
        body = sorted(by_page[page], key=lambda l: l[1], reverse=True)
        # The last line of the page that is unique in both and is not a
        # page number.
        for l in body:
            if count[l[3]] == 1 and len(where[l[3]]) == 1 and not l[3].isdigit():
                ol = where[l[3]][0]
                if ol[0] != page:
                    found = (page, l, ol)
                break
        if found: break
    if not found:
        print(f'{doc_id}: pages agree ({rp}/{op})'); continue
    page, l, ol = found
    print(f'{doc_id}: ref page {page + 1} ends with "{l[3][:50]}" at {l[1]:.0f}; ours has it on page {ol[0] + 1} at {ol[1]:.0f}')
