#!/usr/bin/env python3
"""Benchmarks kakemono on Word documents (.docx, .doc), against LibreOffice
where it can do the same, on the corpus of tests/data/word-corpus.tsv
(downloaded by scripts/fetch-word-corpus.sh).

Experiments:

  compare     Word → PDF for every document of the corpus, kakemono and
              LibreOffice: time, CPU time, peak memory, errors, and how close
              each PDF is to the PDF Word made of the same file
              (scripts/compare-layout.py), with the time and memory by
              document size.
  formats     The given documents to every output (PDF, the four HTML modes,
              Word, Markdown, LaTeX, EPUB, JSON) and, with LibreOffice, to
              PDF, Word (.docx), HTML and ODT.
  threads     One document to PDF with -j 1, 2, 4... up to the number of cores.
  concurrent  Several conversions of one document to PDF at once: total time,
              throughput and memory.

Usage:
  word-benchmark.py compare [--limit N] [--no-libreoffice] [--json FILE]
  word-benchmark.py formats DOC... [--no-libreoffice] [--json FILE]
  word-benchmark.py threads DOC [--json FILE]
  word-benchmark.py concurrent DOC [--max N] [--json FILE]
  word-benchmark.py sizes FILE        (the size table of a saved compare run)

Common options: --corpus DIR, --kakemono PATH, --soffice PATH. FILE is
written in the current directory. Conversions run one at a time unless the
experiment is about running them together; LibreOffice starts once per
conversion, as a command line conversion would (its first start, which
creates its profile, is not counted).
"""
import argparse, concurrent.futures as cf, importlib.util, json, os, shutil, statistics, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('compare', os.path.join(here, 'compare-layout.py'))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)

TIMEOUT_S = 300
POLL_S = 0.005
# Outputs of kakemono: (name, extension, extra arguments).
KAKEMONO_OUTPUTS = [
    ('pdf', 'pdf', []),
    ('friendly', 'html', ['-m', 'friendly']),
    ('medium', 'html', ['-m', 'medium']),
    ('hard', 'html', ['-m', 'hard']),
    ('hard-95', 'html', ['-m', 'hard-95']),
    ('docx', 'docx', []),
    ('md', 'md', []),
    ('tex', 'tex', []),
    ('epub', 'epub', []),
    ('json', 'json', []),
]
# Outputs of LibreOffice: (name, --convert-to filter, extension written).
LIBREOFFICE_OUTPUTS = [
    ('pdf', 'pdf', 'pdf'),
    ('docx', 'docx:MS Word 2007 XML', 'docx'),
    ('html', 'html', 'html'),
    ('odt', 'odt', 'odt'),
]
# Document sizes, in pages of Word's PDF: (label, first, last).
SIZE_BINS = [('1–4 pages', 1, 4), ('5–19 pages', 5, 19), ('20–49 pages', 20, 49),
             ('50–149 pages', 50, 149), ('150+ pages', 150, 10**9)]


def confined(path):
    """The absolute path, `..` resolved; it must lie inside the working
    directory or the temporary directory, where corpora and outputs are kept
    (the corpus may hold links to the files)."""
    real = os.path.normpath(os.path.abspath(path))
    for base in (os.getcwd(), tempfile.gettempdir()):
        base = os.path.realpath(base)
        if real == base or real.startswith(base + os.sep):
            return real
    sys.exit(f'{path}: not inside the working or temporary directory')


def start(command):
    """Starts `command`, its output discarded."""
    return subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def wait(process, started):
    """Waits for `process`: (exit code, wall seconds, CPU seconds, peak
    memory in MB), of it and the processes it waited for."""
    deadline = started + TIMEOUT_S
    while True:
        pid, status, usage = os.wait4(process.pid, os.WNOHANG)
        if pid:
            break
        if time.time() > deadline:
            process.kill()
            pid, status, usage = os.wait4(process.pid, 0)
            break
        time.sleep(POLL_S)
    process.returncode = os.waitstatus_to_exitcode(status)
    return process.returncode, time.time() - started, usage.ru_utime + usage.ru_stime, usage.ru_maxrss / 1024


def timed(command, output):
    """Runs `command`, which writes `output`: its time, CPU time, peak memory
    and the size of the output, or the error."""
    began = time.time()
    code, wall, cpu, peak = wait(start(command), began)
    result = {'seconds': round(wall, 3), 'cpu_seconds': round(cpu, 3), 'peak_mb': round(peak, 1)}
    if code != 0 or not os.path.exists(output):
        result['error'] = 'timeout' if wall >= TIMEOUT_S else f'exit {code}'
    else:
        result['output_kb'] = round(os.path.getsize(output) / 1024, 1)
    return result


class Tools:
    """The two converters, with the folder their outputs go to."""

    def __init__(self, args):
        self.kakemono = args.kakemono
        self.soffice = None if args.no_libreoffice else shutil.which(args.soffice)
        self.out_dir = tempfile.mkdtemp(prefix='word-benchmark-')
        self.profile = f'-env:UserInstallation=file://{self.out_dir}/lo-profile'
        if self.soffice:
            # The first start creates the profile: not counted.
            subprocess.run([self.soffice, self.profile, '--headless', '--norestore', '--version'],
                           capture_output=True, timeout=TIMEOUT_S, check=False)

    def kakemono_command(self, src, name, ext, extra, jobs=None, tag=''):
        """The command converting `src` to output `name`, and the file it writes."""
        doc_id = os.path.splitext(os.path.basename(src))[0]
        out = os.path.join(self.out_dir, 'kakemono', f'{doc_id}{tag}.{name}.{ext}')
        os.makedirs(os.path.dirname(out), exist_ok=True)
        command = [self.kakemono, 'convert', src, '-o', out, *extra]
        if jobs is not None:
            command += ['-j', str(jobs)]
        return command, out

    def libreoffice_command(self, src, name, target, ext):
        """The command converting `src` with LibreOffice, and the file it writes."""
        doc_id = os.path.splitext(os.path.basename(src))[0]
        folder = os.path.join(self.out_dir, 'libreoffice', name)
        os.makedirs(folder, exist_ok=True)
        command = [self.soffice, self.profile, '--headless', '--norestore', '--convert-to', target, '--outdir', folder, src]
        return command, os.path.join(folder, f'{doc_id}.{ext}')

    def close(self):
        shutil.rmtree(self.out_dir, ignore_errors=True)


def documents(corpus, limit=0):
    """The Word files of the corpus that have Word's PDF next to them."""
    names = sorted(name for name in os.listdir(corpus)
                   if name.endswith(('.docx', '.doc')) and os.path.exists(os.path.join(corpus, os.path.splitext(name)[0] + '.pdf')))
    return names[:limit] if limit else names


def fidelity_summary(rows):
    ok = [r for r in rows if 'error' not in r]
    lines = sum(r['ref_lines'] for r in ok)
    seconds = [r['seconds'] for r in ok]
    memory = [r['peak_mb'] for r in ok]
    pages = sum(r['ref_pages'] for r in ok)
    return {
        'documents': len(rows),
        'errors': len(rows) - len(ok),
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


def size_table(results, corpus):
    """Time and memory of each tool by document size (pages of Word's PDF),
    over the documents both converted."""
    pages = {}
    for rows in results.values():
        for r in rows:
            if 'ref_pages' in r:
                pages[r['id']] = r['ref_pages']
    both = set.intersection(*({r['id'] for r in rows if 'error' not in r} for rows in results.values()))
    table = []
    for label, first, last in SIZE_BINS:
        ids = {i for i in both if first <= pages.get(i, 0) <= last}
        if not ids:
            continue
        row = {'size': label, 'documents': len(ids), 'pages': sum(pages[i] for i in ids)}
        sizes = []
        for doc_id in ids:
            for ext in ('.docx', '.doc'):
                path = os.path.join(corpus, doc_id + ext)
                if os.path.exists(path):
                    sizes.append(os.path.getsize(path) / 1024)
        row['median_file_kb'] = round(statistics.median(sizes))
        for tool, rows in results.items():
            mine = [r for r in rows if r['id'] in ids]
            seconds = sum(r['seconds'] for r in mine)
            row[tool] = {
                'median_seconds': round(statistics.median(r['seconds'] for r in mine), 3),
                'pages_per_second': round(row['pages'] / max(seconds, 1e-9), 1),
                'median_peak_mb': round(statistics.median(r['peak_mb'] for r in mine), 1),
                'max_peak_mb': round(max(r['peak_mb'] for r in mine), 1),
            }
        table.append(row)
    return table


def print_sizes(table):
    tools = [key for key in table[0] if isinstance(table[0][key], dict)] if table else []
    print('\n| Size | Documents | Median file | ' + ' | '.join(
        f'{t}: median time | {t}: pages/s | {t}: median / max memory' for t in tools) + ' |')
    print('| --- | ---: | ---: |' + ' ---: | ---: | ---: |' * len(tools))
    for row in table:
        cells = [row['size'], str(row['documents']), f"{row['median_file_kb']} KB"]
        for t in tools:
            r = row[t]
            cells += [f"{r['median_seconds']} s", str(r['pages_per_second']), f"{r['median_peak_mb']:.0f} / {r['max_peak_mb']:.0f} MB"]
        print('| ' + ' | '.join(cells) + ' |')


def run_compare(args, tools):
    corpus = args.corpus
    compare.KAKEMONO = args.kakemono
    names = documents(corpus, args.limit)
    results = {'kakemono': []} | ({'libreoffice': []} if tools.soffice else {})
    for at, name in enumerate(names):
        src = os.path.join(corpus, name)
        ref = os.path.join(corpus, os.path.splitext(name)[0] + '.pdf')
        for tool in results:
            if tool == 'kakemono':
                command, out = tools.kakemono_command(src, 'pdf', 'pdf', [])
            else:
                command, out = tools.libreoffice_command(src, 'pdf', 'pdf', 'pdf')
            result = timed(command, out)
            result['id'] = os.path.splitext(name)[0]
            if 'error' not in result:
                result.update(compare.compare(ref, out))
            results[tool].append(result)
        print(f'{at + 1}/{len(names)} {name}', file=sys.stderr)
    report = {tool: fidelity_summary(rows) for tool, rows in results.items()}
    for tool, row in report.items():
        print(f"\n{tool}: {row['documents']} documents, {row['errors']} errors")
        print(f"  time: {row['total_seconds']} s in all, median {row['median_seconds']} s, longest {row['max_seconds']} s, "
              f"{row['pages_per_second']} pages/s; CPU {row['cpu_seconds']} s")
        print(f"  peak memory: median {row['median_peak_mb']} MB, largest {row['max_peak_mb']} MB")
        print(f"  lines broken as Word: {row['line_ratio']:.3f}; same page count: {row['same_page_count_ratio']:.3f}; "
              f"lines on the same page: {row['same_page_ratio']:.3f}")
    sizes = size_table(results, corpus)
    print_sizes(sizes)
    return {'summary': report, 'sizes': sizes, 'documents': results}


def word_pages(pdf):
    """The page count of Word's PDF of a document."""
    report = compare.compare(pdf, pdf)
    return report['ref_pages']


def run_formats(args, tools):
    rows = []
    for doc in args.docs:
        src = confined(doc)
        doc_id = os.path.splitext(os.path.basename(src))[0]
        info = {'id': doc_id, 'file_kb': round(os.path.getsize(src) / 1024, 1)}
        ref = os.path.splitext(src)[0] + '.pdf'
        if os.path.exists(ref):
            info['pages'] = word_pages(ref)
        for name, ext, extra in KAKEMONO_OUTPUTS:
            command, out = tools.kakemono_command(src, name, ext, extra)
            rows.append(info | {'tool': 'kakemono', 'output': name} | timed(command, out))
        if tools.soffice:
            for name, target, ext in LIBREOFFICE_OUTPUTS:
                command, out = tools.libreoffice_command(src, name, target, ext)
                rows.append(info | {'tool': 'libreoffice', 'output': name} | timed(command, out))
        print(f'{doc_id} done', file=sys.stderr)
    print('\n| Document | Tool | Output | Time | CPU | Peak memory | Output size |')
    print('| --- | --- | --- | ---: | ---: | ---: | ---: |')
    for r in rows:
        if 'error' in r:
            print(f"| {r['id']} | {r['tool']} | {r['output']} | {r['error']} | | | |")
        else:
            print(f"| {r['id']} | {r['tool']} | {r['output']} | {r['seconds']} s | {r['cpu_seconds']} s | {r['peak_mb']:.0f} MB | {r['output_kb']:.0f} KB |")
    return {'formats': rows}


def run_threads(args, tools):
    src = confined(args.docs[0])
    rows = []
    jobs = 1
    while jobs <= (os.cpu_count() or 1):
        command, out = tools.kakemono_command(src, 'pdf', 'pdf', [], jobs=jobs)
        rows.append({'jobs': jobs} | timed(command, out))
        jobs *= 2
    print('\n| -j | Time | CPU | Peak memory |\n| ---: | ---: | ---: | ---: |')
    for r in rows:
        print(f"| {r['jobs']} | {r['seconds']} s | {r['cpu_seconds']} s | {r['peak_mb']:.0f} MB |")
    return {'threads': rows}


def run_concurrent(args, tools):
    src = confined(args.docs[0])
    rows = []
    count = 1
    while count <= args.max:
        began = time.time()
        launched = []
        for at in range(count):
            command, out = tools.kakemono_command(src, 'pdf', 'pdf', [], jobs=1, tag=f'-{at}')
            launched.append(start(command))
        with cf.ThreadPoolExecutor(count) as pool:
            waited = list(pool.map(lambda process: wait(process, began), launched))
        total = time.time() - began
        rows.append({
            'at_once': count,
            'seconds': round(total, 2),
            'files_per_minute': round(count * 60 / total, 1),
            'errors': sum(1 for code, *_ in waited if code != 0),
            'memory_mb': round(sum(peak for *_, peak in waited)),
        })
        count *= 2
    print('\n| At once | Total | Throughput | Memory (sum of peaks) |\n| ---: | ---: | ---: | ---: |')
    for r in rows:
        print(f"| {r['at_once']} | {r['seconds']} s | {r['files_per_minute']} files/min | {r['memory_mb']} MB |")
    return {'concurrent': rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('experiment', choices=['compare', 'formats', 'threads', 'concurrent', 'sizes'])
    parser.add_argument('docs', nargs='*', help='Word documents (formats, threads, concurrent), or a saved compare run (sizes)')
    parser.add_argument('--corpus', default=os.path.join(here, '..', 'tests', 'data', 'word-corpus'))
    parser.add_argument('--kakemono', default=os.path.join(here, '..', 'target', 'release', 'kakemono'))
    parser.add_argument('--soffice', default='soffice')
    parser.add_argument('--limit', type=int, default=0, help='compare: only the first N documents')
    parser.add_argument('--max', type=int, default=16, help='concurrent: the most conversions at once')
    parser.add_argument('--no-libreoffice', action='store_true')
    parser.add_argument('--json', help='file written in the current directory')
    args = parser.parse_args()
    args.corpus = confined(args.corpus)
    if args.experiment == 'sizes':
        with open(confined(args.docs[0])) as saved:
            print_sizes(size_table(json.load(saved)['documents'], args.corpus))
        return
    if args.experiment != 'compare' and not args.docs:
        parser.error(f'{args.experiment} needs at least one document')
    tools = Tools(args)
    try:
        run = {'compare': run_compare, 'formats': run_formats, 'threads': run_threads, 'concurrent': run_concurrent}
        report = run[args.experiment](args, tools)
    finally:
        tools.close()
    if args.json:
        with open(os.path.basename(args.json), 'w') as out:
            json.dump(report, out, indent=1)


main()
