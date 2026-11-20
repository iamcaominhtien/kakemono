#!/usr/bin/env python3
"""Prints the baselines of the lines of one page in the reference and in
ours, with their difference, to find vertical layout errors.

Usage: compare-positions.py REFERENCE.pdf OURS.pdf PAGE
"""
import importlib.util, os, sys
spec = importlib.util.spec_from_file_location('c', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'compare-layout.py'))
c = importlib.util.module_from_spec(spec); spec.loader.exec_module(c)
ref, _ = c.lines(sys.argv[1]); ours, _ = c.lines(sys.argv[2]); page = int(sys.argv[3]) - 1
mine = {}
for p, base, left, text in ours:
    if p == page: mine.setdefault(text, []).append((base, left))
prev = None
for p, base, left, text in sorted((l for l in ref if l[0] == page), key=lambda l: (l[1], l[2])):
    hit = mine.get(text)
    if hit:
        b, l = hit.pop(0)
        step = '' if prev is None else f'{base - prev[0]:7.2f} {b - prev[1]:7.2f}'
        print(f'{base:8.2f} {b:8.2f} d={b - base:6.2f} x={l - left:6.2f} {step}  {text[:60]}')
        prev = (base, b)
    else:
        print(f'{base:8.2f}     ----                         {text[:60]}')
