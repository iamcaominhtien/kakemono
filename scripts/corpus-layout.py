#!/usr/bin/env python3
"""Lays out every Word document of a corpus with kakemono and compares each
with the PDF Word made (scripts/compare-layout.py).

Usage: corpus-layout.py CORPUS_DIR IDS_FILE OUT_DIR [KAKEMONO]

CORPUS_DIR holds NNNN.docx (or .doc) with NNNN.pdf; IDS_FILE lists the ids.
Prints one line per document and the totals.
"""
import concurrent.futures as cf, json, os, resource, subprocess, sys, tempfile, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import importlib.util
spec = importlib.util.spec_from_file_location('compare', os.path.join(os.path.dirname(os.path.abspath(__file__)), 'compare-layout.py'))
compare = importlib.util.module_from_spec(spec); spec.loader.exec_module(compare)

def confined(path):
    """The path, resolved; it must lie inside the working directory or the
    temporary directory, where corpora and outputs are kept."""
    real = os.path.realpath(path)
    for base in (os.getcwd(), tempfile.gettempdir()):
        base = os.path.realpath(base)
        if real == base or real.startswith(base + os.sep):
            return real
    sys.exit(f'{path}: not inside the working or temporary directory')


corpus, ids_file, out = (confined(arg) for arg in sys.argv[1:4])
binary = sys.argv[4] if len(sys.argv) > 4 else 'target/release/kakemono'
os.makedirs(out, exist_ok=True)
with open(ids_file) as listing:
    ids = [line.strip() for line in listing if line.strip()]

def one(id):
    src = next((os.path.join(corpus, f'{id}.{ext}') for ext in ('docx', 'doc') if os.path.exists(os.path.join(corpus, f'{id}.{ext}'))), None)
    dst = os.path.join(out, f'{id}.pdf')
    start = time.time()
    limit = lambda: resource.setrlimit(resource.RLIMIT_AS, (4 << 30, 4 << 30))
    try:
        run = subprocess.run([binary, 'convert', src, '-o', dst], capture_output=True, text=True, timeout=300, preexec_fn=limit)
    except subprocess.TimeoutExpired:
        return {'id': id, 'error': 'timeout'}
    elapsed = time.time() - start
    if run.returncode != 0:
        return {'id': id, 'error': (run.stderr.strip() or f'exit {run.returncode}')[-300:]}
    result = compare.compare(os.path.join(corpus, f'{id}.pdf'), dst)
    result.update(id=id, seconds=round(elapsed, 2))
    return result

results = list(cf.ThreadPoolExecutor(max(2, (os.cpu_count() or 4) // 2)).map(one, ids))
ok = [r for r in results if 'error' not in r]
for r in results:
    if 'error' in r:
        print(f"{r['id']} ERROR {r['error']}")
    else:
        flag = ('' if r['ref_pages'] == r['our_pages'] else f"  pages {r['our_pages']}/{r['ref_pages']}") + f"  same page {r['same_page_ratio']:.2f}"
        print(f"{r['id']} lines {r['line_ratio']:.3f} ({r['lines_matched']}/{r['ref_lines']}){flag}")
lines = sum(r['lines_matched'] for r in ok); total = sum(r['ref_lines'] for r in ok)
same_pages = sum(1 for r in ok if r['ref_pages'] == r['our_pages'])
print(f"\ndocuments {len(results)}, errors {len(results) - len(ok)}")
print(f"lines broken as Word: {lines}/{total} = {lines / max(1, total):.3f}")
print(f"same page count: {same_pages}/{len(ok)} = {same_pages / max(1, len(ok)):.3f}")
on_page = sum(r['same_page_ratio'] * r['ref_lines'] for r in ok)
print(f"lines on the same page as Word: {on_page / max(1, total):.3f}")
with open(os.path.join(out, 'results.json'), 'w') as target:
    json.dump(results, target, indent=1)
