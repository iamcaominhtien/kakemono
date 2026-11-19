#!/usr/bin/env python3
"""Measures how close kakemono's PDFs of Word documents are to the PDFs Word
made of them, on the corpus of tests/data/word-corpus.tsv (downloaded by
scripts/fetch-word-corpus.sh), and fails below the thresholds.

Usage: word-fidelity.py [--corpus DIR] [--kakemono PATH] [--json FILE]
                        [--min-lines RATIO] [--min-pages RATIO]

For each document: the share of Word's lines that break at the same words
in ours (scripts/compare-layout.py), and whether the page count is the same.
Totals are over all lines and all documents. A document kakemono cannot
convert fails the run. With GITHUB_STEP_SUMMARY set, a table is added to
the summary of the CI run.
"""
import argparse, concurrent.futures as cf, importlib.util, json, os, resource, subprocess, sys, tempfile, time

here = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location('compare', os.path.join(here, 'compare-layout.py'))
compare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compare)

parser = argparse.ArgumentParser()
parser.add_argument('--corpus', default=os.path.join(here, '..', 'tests', 'data', 'word-corpus'))
parser.add_argument('--kakemono', default=os.path.join(here, '..', 'target', 'release', 'kakemono'))
parser.add_argument('--json')
parser.add_argument('--min-lines', type=float, default=0.0)
parser.add_argument('--min-pages', type=float, default=0.0)
args = parser.parse_args()
compare.KAKEMONO = args.kakemono
out_dir = tempfile.mkdtemp(prefix='word-fidelity-')
docs = sorted(name for name in os.listdir(args.corpus)
              if name.endswith(('.docx', '.doc')) and os.path.exists(os.path.join(args.corpus, os.path.splitext(name)[0] + '.pdf')))


def one(name):
    id = os.path.splitext(name)[0]
    dst = os.path.join(out_dir, id + '.pdf')
    limit = lambda: resource.setrlimit(resource.RLIMIT_AS, (4 << 30, 4 << 30))
    start = time.time()
    try:
        run = subprocess.run([args.kakemono, 'convert', os.path.join(args.corpus, name), '-o', dst],
                             capture_output=True, text=True, timeout=300, preexec_fn=limit)
    except subprocess.TimeoutExpired:
        return {'id': id, 'error': 'timeout'}
    if run.returncode != 0:
        return {'id': id, 'error': (run.stderr.strip() or f'exit {run.returncode}')[-300:]}
    result = compare.compare(os.path.join(args.corpus, id + '.pdf'), dst)
    result.update(id=id, seconds=round(time.time() - start, 2))
    return result


results = list(cf.ThreadPoolExecutor(max(2, os.cpu_count() or 2)).map(one, docs))
ok = [r for r in results if 'error' not in r]
errors = [r for r in results if 'error' in r]
lines = sum(r['lines_matched'] for r in ok)
total = sum(r['ref_lines'] for r in ok)
same_pages = sum(1 for r in ok if r['ref_pages'] == r['our_pages'])
line_ratio = lines / max(1, total)
page_ratio = same_pages / max(1, len(ok))
on_page = sum(r['same_page_ratio'] * r['ref_lines'] for r in ok) / max(1, total)
summary = {
    'documents': len(results), 'errors': len(errors), 'lines': total, 'lines_matched': lines,
    'line_ratio': line_ratio, 'same_page_count_ratio': page_ratio, 'same_page_ratio': on_page,
}
for r in sorted(ok, key=lambda r: r['line_ratio'])[:15]:
    print(f"{r['id']}: lines {r['line_ratio']:.3f}, pages {r['our_pages']}/{r['ref_pages']}")
for r in errors:
    print(f"{r['id']}: ERROR {r['error']}")
print(f"\ndocuments {len(results)}, errors {len(errors)}")
print(f"lines broken as Word: {lines}/{total} = {line_ratio:.3f} (minimum {args.min_lines})")
print(f"same page count: {same_pages}/{len(ok)} = {page_ratio:.3f} (minimum {args.min_pages})")
print(f"lines on the same page as Word: {on_page:.3f}")
if args.json:
    json.dump({'summary': summary, 'documents': results}, open(args.json, 'w'), indent=1)
if os.environ.get('GITHUB_STEP_SUMMARY'):
    with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as f:
        f.write('## Word layout fidelity\n\n| measure | value | minimum |\n| --- | --- | --- |\n')
        f.write(f'| documents | {len(results)} | |\n| conversion errors | {len(errors)} | 0 |\n')
        f.write(f'| lines broken as Word | {line_ratio:.1%} | {args.min_lines:.1%} |\n')
        f.write(f'| same page count | {page_ratio:.1%} | {args.min_pages:.1%} |\n')
        f.write(f'| lines on the same page as Word | {on_page:.1%} | |\n')
failed = errors or line_ratio < args.min_lines or page_ratio < args.min_pages
sys.exit(1 if failed else 0)
