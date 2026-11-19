#!/usr/bin/env python3
"""Measures vertical layout locally: for each pair of consecutive lines of
the reference (on one page) that ours has too, on one page, compares the
distance between their baselines.

Usage: compare-steps.py CORPUS_DIR IDS_FILE OUT_DIR
Prints the share of steps within 0.25 pt and the mean error per document
and in total.
"""
import importlib.util, os, sys, tempfile
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


def steps(ref_pdf, our_pdf):
    ref, _ = c.lines(ref_pdf); ours, _ = c.lines(our_pdf)
    count = {}
    for l in ref: count[l[3]] = count.get(l[3], 0) + 1
    mine = {}
    for l in ours: mine.setdefault(l[3], []).append(l)
    good = total = 0; err = 0.0
    ref = sorted(ref, key=lambda l: (l[0], l[1], l[2]))
    for a, b in zip(ref, ref[1:]):
        if a[0] != b[0] or count[a[3]] > 1 or count[b[3]] > 1 or b[1] - a[1] < 1: continue
        oa, ob = mine.get(a[3]), mine.get(b[3])
        if not oa or not ob or len(oa) > 1 or len(ob) > 1 or oa[0][0] != ob[0][0]: continue
        d = abs((ob[0][1] - oa[0][1]) - (b[1] - a[1]))
        total += 1; good += d < 0.25; err += min(d, 20)
    return good, total, err

if __name__ == '__main__':
    corpus, ids, out = (confined(arg) for arg in sys.argv[1:4])
    G = T = 0; E = 0.0
    with open(ids) as listing:
        names = listing.read().split()
    for name in names:
        g, t, e = steps(os.path.join(corpus, name + '.pdf'), os.path.join(out, name + '.pdf'))
        G += g; T += t; E += e
        print(f'{name} steps ok {g}/{t} mean err {e / max(1, t):.2f}')
    print(f'TOTAL steps ok {G}/{T} = {G / max(1, T):.3f}, mean err {E / max(1, T):.3f}')
