#!/usr/bin/env python3
"""Reports how kakemono reads every Excel workbook (.xlsx, .xlsm) under the
given directories: one TSV line per sheet with the file, the sheet's name,
its number of tables, the header rows of each table (comma-separated, in
order) and its number of paragraphs. Run it before and after a change of the
header or region rules (`src/excel/build.rs`, `src/excel/regions.rs`) and
compare the two reports with `diff`.

A workbook that kakemono cannot read gives one line with `error` and the
message in place of the sheet. Where each header came from (the file's
declaration, the types of the columns, the default of one row) is not in the
JSON yet, so not in the report.

    python3 -I scripts/excel-report.py DIR... [--out FILE] [--jobs 8] [--timeout 120]
"""

import argparse
import concurrent.futures as cf
import json
import os
import subprocess
import sys
import tempfile

here = os.path.dirname(os.path.abspath(__file__))
parser = argparse.ArgumentParser()
parser.add_argument('dirs', nargs='+')
parser.add_argument('--out', help='write the report to this file instead of the standard output')
parser.add_argument('--kakemono', default=os.path.join(here, '..', 'target', 'release', 'kakemono'))
parser.add_argument('--jobs', type=int, default=os.cpu_count() or 4)
parser.add_argument('--timeout', type=int, default=120)
args = parser.parse_args()

WORKBOOKS = ('.xlsx', '.xlsm')
COLUMNS = ['file', 'sheet', 'tables', 'header_rows', 'paragraphs']


def workbooks():
    """The workbooks under the given directories, sorted."""
    found = []
    for root_dir in args.dirs:
        for root, _, names in os.walk(root_dir):
            found.extend(os.path.join(root, name) for name in names if name.lower().endswith(WORKBOOKS))
    return sorted(found)


def clean(text):
    """`text` on one TSV field: no tab, no line break."""
    return ' '.join(str(text).split())


def sheets(blocks):
    """The report lines of the blocks of a workbook's JSON: one per sheet (a
    level 1 heading and the blocks after it)."""
    rows = []
    for block in blocks:
        if block.get('type') == 'heading' and block.get('level') == 1:
            rows.append({'sheet': block.get('plain', ''), 'header_rows': [], 'paragraphs': 0})
        elif rows:
            if block.get('table') is not None:
                rows[-1]['header_rows'].append(block['table'].get('header_rows', 0))
            elif block.get('type') == 'paragraph':
                rows[-1]['paragraphs'] += 1
    return rows


def report(path):
    """The TSV lines of the workbook `path`."""
    with tempfile.TemporaryDirectory() as out:
        target = os.path.join(out, 'x.json')
        try:
            run = subprocess.run([args.kakemono, 'convert', path, '-o', target],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=args.timeout)
        except subprocess.TimeoutExpired:
            return [[path, 'error', 'timeout', '', '']]
        if run.returncode != 0 or not os.path.exists(target):
            message = run.stderr.decode('utf-8', 'replace').strip().splitlines()
            return [[path, 'error', clean(message[-1] if message else f'exit {run.returncode}'), '', '']]
        with open(target, encoding='utf-8') as file:
            blocks = json.load(file).get('blocks', [])
    return [[path, clean(row['sheet']), str(len(row['header_rows'])),
             ','.join(str(rows) for rows in row['header_rows']), str(row['paragraphs'])]
            for row in sheets(blocks)]


def main():
    if not os.path.exists(args.kakemono):
        sys.exit(f'{args.kakemono} not found: run `cargo build --release` first')
    paths = workbooks()
    with cf.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        lines = [line for lines in pool.map(report, paths) for line in lines]
    text = '\n'.join('\t'.join(line) for line in [COLUMNS] + lines) + '\n'
    if args.out:
        with open(args.out, 'w', encoding='utf-8') as file:
            file.write(text)
    else:
        sys.stdout.write(text)
    sheet_lines = [line for line in lines if line[1] != 'error']
    errors = len(lines) - len(sheet_lines)
    tables = sum(int(line[2]) for line in sheet_lines)
    print(f'{len(paths)} workbooks, {len(sheet_lines)} sheets, {tables} tables, {errors} errors', file=sys.stderr)


if __name__ == '__main__':
    main()
