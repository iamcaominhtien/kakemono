#!/usr/bin/env python3
"""Scores kakemono's reading of the pages of the web benchmark (fetch.py first).

For each page, the text kakemono keeps (the blocks of `--json`, without the title and
the authors) is compared with a reference text, as the Scrapinghub benchmark compares
article bodies: shingles of 4 words, precision (how much of what was kept belongs to
the article), recall (how much of the article was kept) and F1, averaged over pages.
The reference is the ground truth for the Scrapinghub pages, and trafilatura's
extraction for the others (a reference, not the truth: its own mistakes count
against kakemono). trafilatura and readability-lxml are scored on the Scrapinghub
pages too, for comparison.

Every page is also written to every output (Word, LaTeX, Typst, EPUB, JSON, chunks,
PDF): a failure, a panic or a timeout is reported.

    /root/.venvs/web-bench/bin/python scripts/web-bench/evaluate.py [--jobs 8] [--no-outputs]

Writes out/web-bench/results.json and prints a summary per category.
"""

from __future__ import annotations

import argparse
import json
import re
import statistics
import subprocess
import sys
import time
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "tests" / "data" / "web-bench"
OUT = ROOT / "out" / "web-bench"
KAKEMONO = ROOT / "target" / "release" / "kakemono"
# Seconds one conversion may take.
TIMEOUT_S = 120
# Outputs written for the crash test.
OUTPUTS = ["docx", "tex", "typ", "epub", "json", "jsonl", "pdf"]
# Blocks that are not the body of the article.
NOT_BODY = {"title", "authors"}
# Share of the shingles of a sentence that must be found for it to count as kept.
KEPT_SHARE = 0.5
# Shortest sentence, in words, shown as missing or extra.
SNIPPET_MIN_WORDS = 6
# Snippets kept per page and kind.
SNIPPETS = 6

TOKEN_RE = re.compile(r"\w+", re.UNICODE)


def tokens(text: str) -> list[str]:
    return TOKEN_RE.findall(text or "")


def shingles(words: list[str], n: int = 4) -> Counter:
    return Counter(tuple(words[i : i + n]) for i in range(max(1, len(words) - n + 1)) if words[i : i + n])


def match(true: str, pred: str) -> tuple[float, float, float]:
    """(tp, fp, fn) of the shingles, normalised as the Scrapinghub benchmark does."""
    t, p = shingles(tokens(true)), shingles(tokens(pred))
    tp = fp = fn = 0.0
    for key in set(t) | set(p):
        tc, pc = t.get(key, 0), p.get(key, 0)
        tp += min(tc, pc)
        fp += max(0, pc - tc)
        fn += max(0, tc - pc)
    total = tp + fp + fn
    return (tp / total, fp / total, fn / total) if total else (0.0, 0.0, 0.0)


def scores(tp: float, fp: float, fn: float) -> dict:
    precision = 1.0 if fp == fn == 0 else (0.0 if tp == fp == 0 else tp / (tp + fp))
    recall = 1.0 if fp == fn == 0 else (0.0 if tp == fn == 0 else tp / (tp + fn))
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return {"precision": precision, "recall": recall, "f1": f1}


def summary(rows: list[dict], key: str = "kakemono") -> dict:
    """The benchmark's averages: precision and recall averaged over pages, F1 of them."""
    triples = [row[key]["tp_fp_fn"] for row in rows if row.get(key)]
    if not triples:
        return {}
    precision = statistics.mean(scores(*t)["precision"] for t in triples if t[0] + t[1] > 0) if any(
        t[0] + t[1] > 0 for t in triples
    ) else 0.0
    recall = statistics.mean(scores(*t)["recall"] for t in triples if t[0] + t[2] > 0)
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return {"pages": len(triples), "precision": precision, "recall": recall, "f1": f1}


def sentences(text: str) -> list[str]:
    parts = re.split(r"(?<=[.!?。！？])\s+|\n+", text or "")
    return [part.strip() for part in parts if len(tokens(part)) >= SNIPPET_MIN_WORDS]


def uncovered(text: str, other: str) -> list[str]:
    """The sentences of `text` most of whose shingles are not in `other`, longest first."""
    seen = shingles(tokens(other))
    out = []
    for sentence in sentences(text):
        own = shingles(tokens(sentence))
        found = sum(1 for key in own if key in seen)
        if own and found / len(own) < KEPT_SHARE:
            out.append(sentence)
    out.sort(key=len, reverse=True)
    return [s if len(s) <= 220 else s[:217] + "…" for s in out[:SNIPPETS]]


def spans_text(spans: list[dict]) -> str:
    return "".join(span.get("text", "") for span in spans or [])


def block_text(block: dict) -> str:
    parts = [spans_text(block.get("spans", []))]
    table = block.get("table")
    if isinstance(table, dict):
        for row in table.get("rows", []):
            parts.append(" ".join(spans_text(cell.get("spans", [])) for cell in row))
    return "\n".join(part for part in parts if part)


def kakemono_read(page: dict, work: Path) -> dict:
    """kakemono's text of the page, from `--json`."""
    html = DATA / page["file"]
    md = work / f"{page['name']}.md"
    started = time.time()
    try:
        run = subprocess.run(
            [str(KAKEMONO), "convert", str(html), "-o", str(md), "--json"],
            capture_output=True,
            timeout=TIMEOUT_S,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"error": f"timeout after {TIMEOUT_S}s"}
    elapsed = time.time() - started
    if run.returncode != 0:
        return {"error": run.stderr.decode("utf-8", "replace").strip()[-400:], "seconds": elapsed}
    # `--json` names its file after the input.
    doc = json.loads((work / f"{Path(page['file']).stem}.json").read_text(encoding="utf-8"))
    blocks = doc.get("blocks", [])
    body = "\n".join(block_text(block) for block in blocks if block.get("type") not in NOT_BODY)
    kinds = Counter(block.get("type") for block in blocks)
    return {
        "seconds": elapsed,
        "title": (doc.get("meta") or {}).get("title"),
        "text": body,
        "words": len(tokens(body)),
        "blocks": len(blocks),
        "footnotes": kinds.get("footnote", 0),
        "figures": kinds.get("figure", 0),
        "headings": kinds.get("heading", 0),
        "first_is_title": bool(blocks) and blocks[0].get("type") == "title",
    }


def reference(page: dict, html: str) -> str:
    if page["reference"] == "ground-truth":
        return page["truth"]
    import trafilatura

    return trafilatura.extract(html, url=page["url"], include_tables=True, include_comments=False) or ""


def baselines(page: dict, html: str) -> dict:
    """trafilatura and readability-lxml on a page with a ground truth."""
    import trafilatura
    from readability import Document as Readability

    out = {}
    try:
        out["trafilatura"] = trafilatura.extract(html, url=page["url"], include_tables=True) or ""
    except Exception:  # noqa: BLE001 - a baseline that fails reads nothing
        out["trafilatura"] = ""
    try:
        summary_html = Readability(html).summary(html_partial=True)
        out["readability"] = re.sub(r"<[^>]+>", " ", summary_html)
    except Exception:  # noqa: BLE001
        out["readability"] = ""
    return out


def outputs(page: dict, work: Path) -> list[str]:
    """The outputs that could not be written (with why)."""
    failures = []
    for ext in OUTPUTS:
        dest = work / "outputs" / f"{page['name']}.{ext}"
        dest.parent.mkdir(parents=True, exist_ok=True)
        try:
            run = subprocess.run(
                [str(KAKEMONO), "convert", str(DATA / page["file"]), "-o", str(dest)],
                capture_output=True,
                timeout=TIMEOUT_S,
                check=False,
            )
        except subprocess.TimeoutExpired:
            failures.append(f"{ext}: timeout")
            continue
        if run.returncode != 0:
            message = run.stderr.decode("utf-8", "replace").strip().splitlines()
            failures.append(f"{ext}: {message[-1] if message else run.returncode}")
    return failures


def evaluate(page: dict, work: Path, with_outputs: bool) -> dict:
    row = {key: page[key] for key in ("name", "set", "category", "url", "file", "reference")}
    if not page.get("file"):
        row["error"] = "not downloaded"
        return row
    html = (DATA / page["file"]).read_text(encoding="utf-8", errors="replace")
    read = kakemono_read(page, work)
    ref = reference(page, html)
    row["reference_words"] = len(tokens(ref))
    if "error" in read:
        row["error"] = read["error"]
        row["kakemono"] = {"tp_fp_fn": (0.0, 0.0, 1.0), **scores(0.0, 0.0, 1.0)}
    else:
        text = read.pop("text")
        triple = match(ref, text)
        row["kakemono"] = {"tp_fp_fn": triple, **scores(*triple)}
        row.update(read)
        row["missing"] = uncovered(ref, text)
        row["extra"] = uncovered(text, ref)
    if page["set"] == "aeb":
        for name, text in baselines(page, html).items():
            triple = match(ref, text)
            row[name] = {"tp_fp_fn": triple, **scores(*triple)}
    if with_outputs and "error" not in row:
        row["output_failures"] = outputs(page, work)
    return row


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--no-outputs", action="store_true", help="skip the crash test of the outputs")
    parser.add_argument("--only", help="evaluate only the pages whose name contains this")
    args = parser.parse_args()
    if not KAKEMONO.exists():
        print("build kakemono first: cargo build --release", file=sys.stderr)
        return 1
    pages = json.loads((DATA / "index.json").read_text(encoding="utf-8"))
    if args.only:
        pages = [page for page in pages if args.only in page["name"]]
    work = OUT / "work"
    work.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(args.jobs) as pool:
        rows = list(pool.map(lambda page: evaluate(page, work, not args.no_outputs), pages))
    categories = sorted({row["category"] for row in rows})
    report = {"pages": rows, "summary": {}}
    print(f"{'category':<10} {'pages':>5} {'F1':>6} {'prec.':>6} {'recall':>6}  errors")
    for category in categories + ["all"]:
        group = [row for row in rows if category in ("all", row["category"])]
        ok = [row for row in group if "kakemono" in row]
        result = summary(ok)
        errors = sum(1 for row in group if "error" in row)
        report["summary"][category] = {**result, "errors": errors}
        if result:
            print(
                f"{category:<10} {result['pages']:>5} {result['f1']:>6.3f} {result['precision']:>6.3f}"
                f" {result['recall']:>6.3f}  {errors}"
            )
    aeb = [row for row in rows if row["set"] == "aeb" and "kakemono" in row]
    if aeb:
        print("\nScrapinghub benchmark (ground truth):")
        for name in ("kakemono", "trafilatura", "readability"):
            result = summary(aeb, name)
            report["summary"][f"aeb:{name}"] = result
            print(f"  {name:<12} F1 {result['f1']:.3f}  precision {result['precision']:.3f}  recall {result['recall']:.3f}")
    failures = [(row["name"], failure) for row in rows for failure in row.get("output_failures", [])]
    print(f"\noutputs that failed: {len(failures)}")
    for name, failure in failures[:40]:
        print(f"  {name}: {failure}")
    for row in rows:
        if "error" in row:
            print(f"  error {row['name']}: {row['error'][:200]}")
    (OUT / "results.json").write_text(json.dumps(report, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"\n{OUT / 'results.json'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
