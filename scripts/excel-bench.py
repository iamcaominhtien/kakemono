#!/usr/bin/env python3
"""Benchmark and stress test of kakemono's Excel input.

    python3 -I scripts/excel-bench.py corpus DIR      # every workbook of DIR to each output, and LibreOffice to PDF
    python3 -I scripts/excel-bench.py synth OUTDIR    # synthetic workbooks of growing size and odd shapes (openpyxl)
    python3 -I scripts/excel-bench.py concurrent F    # F to PDF, 1, 2, 4, 8 at once

Time is wall time; memory is the peak resident size of the process
(os.wait4). Results as JSON lines on stdout. `KAKEMONO_BIN` names the
binary (default: target/release/kakemono), `BENCH_TIMEOUT` the seconds a
conversion may take (default 600). Run it under a memory limit
(`ulimit -v`): the long-text case takes more than 10 GB in the
document-style PDF.
"""
import json
import os
import subprocess
import sys
import tempfile
import time

BIN = os.environ.get('KAKEMONO_BIN', os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'target', 'release', 'kakemono'))
TIMEOUT_S = int(os.environ.get("BENCH_TIMEOUT", "600"))

OUTPUTS = {
    'sheet-html': ['-m', 'medium', '-o', '{out}/x.medium.html'],
    'pdf-excel': ['-o', '{out}/x.pdf'],
    'pdf-doc': ['--pdf-style', 'document', '-o', '{out}/x.pdf'],
    'md': ['-o', '{out}/x.md'],
    'docx': ['-o', '{out}/x.docx'],
    'json': ['-o', '{out}/x.json'],
    'chunks': ['-o', '{out}/x.jsonl'],
    'epub': ['-o', '{out}/x.epub'],
    'tex': ['-o', '{out}/x.tex'],
    'friendly': ['-m', 'friendly', '-o', '{out}/x.friendly.html'],
}


def run(cmd, timeout=TIMEOUT_S, cwd=None):
    """(seconds, peak MB, exit code or 'timeout')."""
    start = time.monotonic()
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=cwd)
    deadline = start + timeout
    while True:
        pid, status, usage = os.wait4(proc.pid, os.WNOHANG)
        if pid:
            secs = time.monotonic() - start
            code = os.waitstatus_to_exitcode(status)
            return round(secs, 2), round(usage.ru_maxrss / 1024, 1), code
        if time.monotonic() > deadline:
            proc.kill()
            _, _, usage = os.wait4(proc.pid, 0)
            return round(time.monotonic() - start, 2), round(usage.ru_maxrss / 1024, 1), 'timeout'
        time.sleep(0.02)


def convert(path, outputs):
    res = {}
    for name in outputs:
        with tempfile.TemporaryDirectory() as out:
            args = [a.replace('{out}', out) for a in OUTPUTS[name]]
            secs, mb, code = run([BIN, 'convert', path] + args)
            size = sum(os.path.getsize(os.path.join(d, f)) for d, _, fs in os.walk(out) for f in fs)
            res[name] = {'s': secs, 'mb': mb, 'code': code, 'out_mb': round(size / 1e6, 2)}
    return res


def libreoffice(path):
    with tempfile.TemporaryDirectory() as out, tempfile.TemporaryDirectory() as profile:
        # A first start creates the profile; not counted.
        run(['soffice', f'-env:UserInstallation=file://{profile}', '--headless', '--terminate_after_init'], 120)
        secs, mb, code = run(['soffice', f'-env:UserInstallation=file://{profile}', '--headless',
                              '--convert-to', 'pdf', '--outdir', out, path])
        ok = any(f.endswith('.pdf') for f in os.listdir(out))
        return {'s': secs, 'mb': mb, 'code': code if ok else 'no-pdf'}


def corpus(directory):
    for name in sorted(os.listdir(directory)):
        if not name.endswith(('.xlsx', '.xlsm', '.xltx')):
            continue
        path = os.path.join(directory, name)
        row = {'file': name, 'kb': os.path.getsize(path) // 1024, 'kakemono': convert(path, OUTPUTS)}
        row['libreoffice'] = libreoffice(path)
        print(json.dumps(row), flush=True)


def synth(outdir):
    import openpyxl
    from openpyxl.utils import get_column_letter
    os.makedirs(outdir, exist_ok=True)

    def grid(name, rows, cols, text_every=0, long_text=0):
        path = os.path.join(outdir, name)
        if os.path.exists(path):
            return path
        wb = openpyxl.Workbook(write_only=True)
        ws = wb.create_sheet('Data')
        ws.append([f'Col {c}' for c in range(cols)])
        for r in range(rows):
            line = []
            for c in range(cols):
                if long_text:
                    line.append(('word ' * (long_text // 5)) + str(r))
                elif text_every and c % text_every == 0:
                    line.append(f'item {r} {c}')
                else:
                    line.append(r * cols + c + 0.5)
            ws.append(line)
        wb.save(path)
        return path

    def many_sheets(name, sheets, rows, cols):
        path = os.path.join(outdir, name)
        if os.path.exists(path):
            return path
        wb = openpyxl.Workbook(write_only=True)
        for s in range(sheets):
            ws = wb.create_sheet(f'S{s}')
            ws.append([f'h{c}' for c in range(cols)])
            for r in range(rows):
                ws.append([r + c for c in range(cols)])
        wb.save(path)
        return path

    def merges(name, count):
        path = os.path.join(outdir, name)
        if os.path.exists(path):
            return path
        wb = openpyxl.Workbook()
        ws = wb.active
        for i in range(count):
            r = 1 + (i // 100) * 2
            c = 1 + (i % 100) * 2
            ws.cell(r, c, f'm{i}')
            ws.merge_cells(start_row=r, start_column=c, end_row=r + 1, end_column=c + 1)
        wb.save(path)
        return path

    def widest(name):
        path = os.path.join(outdir, name)
        if os.path.exists(path):
            return path
        wb = openpyxl.Workbook(write_only=True)
        ws = wb.create_sheet('Wide')
        for r in range(5):
            ws.append([r * 16384 + c for c in range(16384)])
        wb.save(path)
        return path

    cases = [
        ('grid-10k-x-10', lambda: grid('g1.xlsx', 10_000, 10, text_every=3)),
        ('grid-50k-x-20 (1M cells)', lambda: grid('g2.xlsx', 50_000, 20, text_every=4)),
        ('grid-100k-x-25 (2.5M, at the cap)', lambda: grid('g3.xlsx', 100_000, 25, text_every=5)),
        ('grid-150k-x-30 (4.5M, over the caps)', lambda: grid('g4.xlsx', 150_000, 30, text_every=5)),
        ('wide-1000-x-1000 (1M cells)', lambda: grid('w1.xlsx', 1_000, 1_000)),
        ('widest-5-x-16384', lambda: widest('w2.xlsx')),
        ('sheets-500-x-100', lambda: many_sheets('s1.xlsx', 500, 10, 10)),
        ('long-text-2000-x-5 (32k chars)', lambda: grid('t1.xlsx', 2_000, 5, long_text=32_000)),
        ('merges-20k', lambda: merges('m1.xlsx', 20_000)),
    ]
    outputs = ['sheet-html', 'pdf-excel', 'pdf-doc', 'md', 'chunks', 'docx']
    for label, make in cases:
        start = time.monotonic()
        path = make()
        row = {'case': label, 'mb_file': round(os.path.getsize(path) / 1e6, 1),
               'gen_s': round(time.monotonic() - start, 1), 'kakemono': convert(path, outputs)}
        print(json.dumps(row), flush=True)


def concurrent(path):
    for n in (1, 2, 4, 8):
        with tempfile.TemporaryDirectory() as out:
            start = time.monotonic()
            procs = [subprocess.Popen([BIN, 'convert', path, '-o', f'{out}/x{i}.pdf'],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) for i in range(n)]
            peaks, codes = [], []
            for proc in procs:
                _, status, usage = os.wait4(proc.pid, 0)
                peaks.append(usage.ru_maxrss / 1024)
                codes.append(os.waitstatus_to_exitcode(status))
            secs = time.monotonic() - start
            print(json.dumps({'parallel': n, 's': round(secs, 2), 'per_s': round(n / secs, 2),
                              'sum_peak_mb': round(sum(peaks)), 'codes': codes}), flush=True)


if __name__ == '__main__':
    {'corpus': corpus, 'synth': synth, 'concurrent': concurrent}[sys.argv[1]](sys.argv[2])
