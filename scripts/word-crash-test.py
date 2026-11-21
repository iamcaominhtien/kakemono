#!/usr/bin/env python3
"""Converts every Word file (.doc, .docx) and every Excel workbook (.xlsx,
.xlsm) under the given directories to every output (the 4 HTML modes, PDF,
Word, Markdown, LaTeX, EPUB, JSON) and reports the files that make kakemono
crash (a panic, a signal) or hang (a timeout). A workbook has no page layout:
its PDF output is not tried.

A clean error (a damaged or password-protected file) is not a crash: it is
counted apart.

    python3 scripts/word-crash-test.py DIR... [--jobs 8] [--timeout 120] [--json out.json]
"""

import argparse
import concurrent.futures as cf
import json
import os
import subprocess
import sys
import tempfile
import time

here = os.path.dirname(os.path.abspath(__file__))
parser = argparse.ArgumentParser()
parser.add_argument('dirs', nargs='+')
parser.add_argument('--kakemono', default=os.path.join(here, '..', 'target', 'release', 'kakemono'))
parser.add_argument('--jobs', type=int, default=os.cpu_count() or 4)
parser.add_argument('--timeout', type=int, default=120)
parser.add_argument('--json')
args = parser.parse_args()

# Each output: the arguments after the input file.
OUTPUTS = [
    ('html', ['--all', '-o', '{out}/html']),
    ('pdf', ['-o', '{out}/x.pdf']),
    ('docx', ['-o', '{out}/x.docx']),
    ('md', ['-o', '{out}/x.md']),
    ('tex', ['-o', '{out}/x.tex']),
    ('epub', ['-o', '{out}/x.epub']),
    ('json', ['-o', '{out}/x.json']),
]
WORKBOOKS = ('.xlsx', '.xlsm')
timings = {}  # path -> seconds over all its outputs


def files():
    for root_dir in args.dirs:
        for root, _, names in os.walk(root_dir):
            for name in sorted(names):
                if name.lower().endswith(('.doc', '.docx') + WORKBOOKS):
                    yield os.path.join(root, name)


def check(path):
    """The failures of `path`: (output, kind, message), kind being
    'crash', 'timeout' or 'error'."""
    failures = []
    began = time.time()
    with tempfile.TemporaryDirectory() as out:
        workbook = path.lower().endswith(WORKBOOKS)
        for name, rest in OUTPUTS:
            if workbook and name == 'pdf':
                continue
            command = [args.kakemono, 'convert', path] + [part.format(out=out) for part in rest]
            try:
                done = subprocess.run(command, capture_output=True, text=True, timeout=args.timeout,
                                      errors='replace')
            except subprocess.TimeoutExpired:
                failures.append((name, 'timeout', f'over {args.timeout} s'))
                continue
            if done.returncode == 0:
                continue
            message = (done.stderr.strip().splitlines() or [''])[-1][:300]
            crashed = done.returncode < 0 or done.returncode == 101 or 'panicked' in done.stderr
            failures.append((name, 'crash' if crashed else 'error', message))
            if not crashed:
                # A file that cannot be read fails every output the same way.
                break
    timings[path] = time.time() - began
    return failures


def main():
    paths = list(files())
    started = time.time()
    results = {}
    with cf.ThreadPoolExecutor(args.jobs) as pool:
        for count, (path, failures) in enumerate(
                zip(paths, pool.map(check, paths)), start=1):
            results[path] = failures
            for name, kind, message in failures:
                if kind != 'error':
                    print(f'{kind.upper()}\t{name}\t{path}\t{message}', flush=True)
            if count % 200 == 0:
                print(f'... {count}/{len(paths)} in {time.time() - started:.0f} s', file=sys.stderr, flush=True)
    kinds = {kind: sum(any(f[1] == kind for f in fails) for fails in results.values())
             for kind in ('crash', 'timeout', 'error')}
    print(f'files {len(paths)}, crashes {kinds["crash"]}, timeouts {kinds["timeout"]}, '
          f'clean errors {kinds["error"]}, {time.time() - started:.0f} s')
    slowest = sorted(timings.items(), key=lambda item: -item[1])[:3]
    print('slowest: ' + ', '.join(f'{os.path.basename(path)} {secs:.1f} s' for path, secs in slowest))
    if args.json:
        with open(args.json, 'w') as out:
            json.dump({path: fails for path, fails in results.items() if fails}, out, indent=1)
    sys.exit(1 if kinds['crash'] or kinds['timeout'] else 0)


main()
