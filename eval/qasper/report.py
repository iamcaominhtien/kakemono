"""Summarise retrieve.py's results: mean evidence recall per pipeline and budget,
and paired differences against a reference pipeline with 95% confidence
intervals from a bootstrap over papers (questions of one paper are not
independent, so papers are resampled, not questions).

    python report.py <model> [reference] [pipeline ...]
"""

import json
import sys

import numpy as np

from common import RESULTS

KEYS = ["ceiling", "recall@512", "recall@1024", "recall@2048"]
ORDER = ["pypdf-fixed", "pypdf-fixed256", "unstructured-title", "docling-hybrid", "kakemono-fixed", "kakemono-fixed256", "kakemono-struct"]


def main():
    model = sys.argv[1]
    ref = sys.argv[2] if len(sys.argv) > 2 else "kakemono-struct"
    names = sys.argv[3:] or [p for p in ORDER if (RESULTS / model / f"{p}.json").exists()]
    res = {p: {r["qid"]: r for r in json.loads((RESULTS / model / f"{p}.json").read_text())} for p in names}
    qids = sorted(res[ref])
    papers = sorted({res[ref][q]["pid"] for q in qids})
    by_paper = {p: [q for q in qids if res[ref][q]["pid"] == p] for p in papers}
    rng = np.random.default_rng(0)
    samples = [[q for p in rng.choice(papers, len(papers)) for q in by_paper[p]] for _ in range(2000)]

    print(f"{model}: {len(qids)} questions, {len(papers)} papers\n")
    print(f"{'pipeline':20s}" + "".join(f"{k:>13s}" for k in KEYS))
    for p in names:
        print(f"{p:20s}" + "".join(f"{np.mean([res[p][q][k] for q in qids]):13.3f}" for k in KEYS))
    print(f"\ndifference {ref} - pipeline, 95% CI (bootstrap over papers):")
    for p in names:
        if p == ref:
            continue
        cells = []
        for k in KEYS[1:]:
            d = {q: res[ref][q][k] - res[p][q][k] for q in qids}
            boots = [np.mean([d[q] for q in s]) for s in samples]
            lo, hi = np.percentile(boots, [2.5, 97.5])
            cells.append(f"{np.mean(list(d.values())):+.3f} [{lo:+.3f},{hi:+.3f}]")
        print(f"  vs {p:18s}" + "  ".join(cells))


if __name__ == "__main__":
    main()
