#!/usr/bin/env python3
"""Benchmarks kakemono's Word → PDF conversion against LibreOffice's, on the
corpus of tests/data/word-corpus.tsv (downloaded by
scripts/fetch-word-corpus.sh): time, CPU time, peak memory, and how close
each PDF is to the PDF Word made of the same file (scripts/compare-layout.py).

Usage: word-benchmark.py [--corpus DIR] [--kakemono PATH] [--soffice PATH]
                         [--limit N] [--no-libreoffice] [--json FILE]  (FILE: in the current directory)

The documents are converted one at a time, so that the times do not depend
on what else runs. LibreOffice is started once per document, as a command
line conversion would; its first start (which creates its profile) is not
counted.
"""
import argparse, importlib.util, json, os, shutil, statistics, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('compare', os.path.join(here, 'compare-layout.py'))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)

parser = argparse.ArgumentParser()
parser.add_argument('--corpus', default=os.path.join(here, '..', 'tests', 'data', 'word-corpus'))
parser.add_argument('--kakemono', default=os.path.join(here, '..', 'target', 'release', 'kakemono'))
parser.add_argument('--soffice', default='soffice')
parser.add_argument('--limit', type=int, default=0, help='only the first N documents')
parser.add_argument('--no-libreoffice', action='store_true')
parser.add_argument('--json', help='file written in the current directory')
args = parser.parse_args()

TIMEOUT_S = 300


def confined(path):
    """The path, resolved; it must lie inside the working directory or the
    temporary directory, where corpora and outputs are kept."""
    real = os.path.realpath(path)
    for base in (os.getcwd(), tempfile.gettempdir()):
        base = os.path.realpath(base)
        if real == base or real.startswith(base + os.sep):
            return real
    sys.exit(f'{path}: not inside the working or temporary directory')


def measure(tool, command, out_pdf, ref_pdf):
    """Runs `command` and compares the PDF it writes with Word's: its time,
    CPU time and peak memory (of it and the processes it waited for)."""
    start = time.time()
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    deadline = start + TIMEOUT_S
    while True:
        pid, status, usage = os.wait4(process.pid, os.WNOHANG)
        if pid:
            break
        if time.time() > deadline:
            process.kill()
            pid, status, usage = os.wait4(process.pid, 0)
            break
        time.sleep(0.005)
    process.returncode = os.waitstatus_to_exitcode(status)
    result = {
        'tool': tool,
        'seconds': round(time.time() - start, 3),
        'cpu_seconds': round(usage.ru_utime + usage.ru_stime, 3),
        'peak_mb': round(usage.ru_maxrss / 1024, 1),
    }
    if process.returncode != 0 or not os.path.exists(out_pdf):
        result['error'] = f'exit {process.returncode}'
        return result
    result.update(compare.compare(ref_pdf, out_pdf))
    return result


def summary(results):
    ok = [r for r in results if 'error' not in r]
    lines = sum(r['ref_lines'] for r in ok)
    pages = sum(r['our_pages'] for r in ok)
    seconds = [r['seconds'] for r in ok]
    memory = [r['peak_mb'] for r in ok]
    return {
        'documents': len(results),
        'errors': len(results) - len(ok),
        'total_seconds': round(sum(seconds), 1),
        'median_seconds': round(statistics.median(seconds), 3) if seconds else None,
        'max_seconds': round(max(seconds), 2) if seconds else None,
        'cpu_seconds': round(sum(r['cpu_seconds'] for r in ok), 1),
        'pages_per_second': round(pages / max(sum(seconds), 1e-9), 1),
        'median_peak_mb': round(statistics.median(memory), 1) if memory else None,
        'max_peak_mb': round(max(memory), 1) if memory else None,
        'line_ratio': sum(r['lines_matched'] for r in ok) / max(1, lines),
        'same_page_count_ratio': sum(1 for r in ok if r['our_pages'] == r['ref_pages']) / max(1, len(ok)),
        'same_page_ratio': sum(r['same_page_ratio'] * r['ref_lines'] for r in ok) / max(1, lines),
    }


def main():
    corpus = confined(args.corpus)
    compare.KAKEMONO = args.kakemono
    docs = sorted(name for name in os.listdir(corpus)
                  if name.endswith(('.docx', '.doc')) and os.path.exists(os.path.join(corpus, os.path.splitext(name)[0] + '.pdf')))
    if args.limit:
        docs = docs[:args.limit]
    tools = ['kakemono'] + ([] if args.no_libreoffice or not shutil.which(args.soffice) else ['libreoffice'])
    out_dir = tempfile.mkdtemp(prefix='word-benchmark-')
    profile = f'-env:UserInstallation=file://{out_dir}/lo-profile'
    if 'libreoffice' in tools:
        # The first start creates the profile: not counted.
        subprocess.run([args.soffice, profile, '--headless', '--version'], capture_output=True, timeout=TIMEOUT_S, check=False)
    results = {tool: [] for tool in tools}
    for at, name in enumerate(docs):
        doc_id = os.path.splitext(name)[0]
        src = os.path.join(corpus, name)
        ref = os.path.join(corpus, doc_id + '.pdf')
        for tool in tools:
            dst_dir = os.path.join(out_dir, tool)
            os.makedirs(dst_dir, exist_ok=True)
            dst = os.path.join(dst_dir, doc_id + '.pdf')
            if tool == 'kakemono':
                command = [args.kakemono, 'convert', src, '-o', dst]
            else:
                command = [args.soffice, profile, '--headless', '--norestore', '--convert-to', 'pdf', '--outdir', dst_dir, src]
            result = measure(tool, command, dst, ref)
            result['id'] = doc_id
            results[tool].append(result)
        print(f'{at + 1}/{len(docs)} {doc_id}', file=sys.stderr)
    shutil.rmtree(out_dir, ignore_errors=True)
    report = {tool: summary(rows) for tool, rows in results.items()}
    for tool, row in report.items():
        print(f"\n{tool}: {row['documents']} documents, {row['errors']} errors")
        print(f"  time: {row['total_seconds']} s in all, median {row['median_seconds']} s, longest {row['max_seconds']} s, "
              f"{row['pages_per_second']} pages/s; CPU {row['cpu_seconds']} s")
        print(f"  peak memory: median {row['median_peak_mb']} MB, largest {row['max_peak_mb']} MB")
        print(f"  lines broken as Word: {row['line_ratio']:.3f}; same page count: {row['same_page_count_ratio']:.3f}; "
              f"lines on the same page: {row['same_page_ratio']:.3f}")
    if args.json:
        with open(os.path.basename(args.json), 'w') as out:
            json.dump({'summary': report, 'documents': results}, out, indent=1)


main()
