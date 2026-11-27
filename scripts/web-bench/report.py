#!/usr/bin/env python3
"""Writes the review PDFs of the web benchmark (evaluate.py first): one PDF per category
in out/web-bench/review/, to look through for mistakes quickly.

Each tested page takes one A4 page: its name, address and scores; the page as its
site shows it (the saved file, with its style sheets and pictures from the site);
kakemono's friendly HTML at its start, middle and end; and the sentences of the
reference that kakemono left out, and those it kept that the reference does not have.
The pages with the lowest F1 come first. The first page of each PDF sums the
category up.

    /root/.venvs/web-bench/bin/python scripts/web-bench/report.py [--only CATEGORY] [--jobs 8]
"""

from __future__ import annotations

import argparse
import html
import json
import statistics
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
DATA = ROOT / "tests" / "data" / "web-bench"
OUT = ROOT / "out" / "web-bench"
KAKEMONO = ROOT / "target" / "release" / "kakemono"
# Seconds one conversion may take (pictures are fetched).
TIMEOUT_S = 180
# Most pages in one PDF: a longer category is cut into parts.
PAGES_PER_PDF = 60

CSS = """
@page { size: A4 landscape; }
body { font: 9pt/1.35 system-ui, sans-serif; margin: 0; color: #1d1d1f; }
.sheet { page-break-after: always; height: 192mm; display: grid; grid-template-columns: 1.3fr 1fr; gap: 4mm; overflow: hidden; }
.head { grid-column: 1 / 3; display: flex; gap: 6mm; align-items: baseline; border-bottom: 1px solid #ccc; padding-bottom: 1.5mm; }
.head h2 { font-size: 11pt; margin: 0; }
.head .url { color: #555; font-size: 8pt; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 120mm; }
.score { font-weight: 600; }
.bad { color: #b00020; } .mid { color: #b36b00; } .good { color: #1b7f3b; }
.left img { width: 100%; height: 100mm; object-fit: cover; object-position: top; border: 1px solid #bbb; }
.right img { width: 100%; height: 56mm; object-fit: cover; object-position: top; border: 1px solid #bbb; display: block; margin-bottom: 1.5mm; }
.label { font-size: 7.5pt; color: #666; margin: 0.5mm 0; }
.snips { display: grid; grid-template-columns: 1fr 1fr; gap: 3mm; font-size: 7.5pt; margin-top: 1.5mm; }
.snips ul { margin: 0; padding-left: 4mm; } .snips li { margin-bottom: 0.8mm; }
.snips h3 { font-size: 8pt; margin: 0 0 1mm; }
.missing h3 { color: #b00020; } .extra h3 { color: #b36b00; }
.summary { page-break-after: always; }
.summary table { border-collapse: collapse; width: 100%; font-size: 8.5pt; }
.summary td, .summary th { border-bottom: 1px solid #ddd; padding: 1mm 2mm; text-align: left; }
.summary td.n { text-align: right; font-variant-numeric: tabular-nums; }
.error { color: #b00020; }
"""


def friendly(row: dict) -> Path | None:
    """kakemono's friendly HTML of the page, read as at its address (pictures fetched)."""
    out = OUT / "review" / "html" / f"{row['name']}.html"
    out.parent.mkdir(parents=True, exist_ok=True)
    if out.exists():
        return out
    command = [str(KAKEMONO), "convert", str(DATA / row["file"]), "-o", str(out)]
    if row.get("url"):
        command += ["--base-url", row["url"]]
    try:
        run = subprocess.run(command, capture_output=True, timeout=TIMEOUT_S, check=False)
    except subprocess.TimeoutExpired:
        return None
    return out if run.returncode == 0 and out.exists() else None


def tone(f1: float) -> str:
    """The class of the colour of a score."""
    if f1 < 0.7:
        return "bad"
    return "mid" if f1 < 0.9 else "good"


def sheet(row: dict, shots: Path) -> str:
    name = row["name"]
    esc = html.escape
    kake = row.get("kakemono", {})
    f1 = kake.get("f1", 0.0)
    scores = (
        f"<span class='score {tone(f1)}'>F1 {f1:.2f}</span> "
        f"P {kake.get('precision', 0):.2f} · R {kake.get('recall', 0):.2f}"
    )
    for base in ("trafilatura", "readability"):
        if base in row:
            scores += f" · {base} F1 {row[base]['f1']:.2f}"
    facts = (
        f"{row.get('words', 0)} words (reference {row.get('reference_words', 0)}, {row['reference']}) · "
        f"{row.get('headings', 0)} headings · {row.get('figures', 0)} figures · {row.get('footnotes', 0)} notes"
    )
    if row.get("first_is_title") is False:
        facts += " · <span class='error'>first block is not the title</span>"
    if row.get("output_failures"):
        facts += f" · <span class='error'>outputs failed: {esc('; '.join(row['output_failures']))}</span>"
    if "error" in row:
        facts += f" · <span class='error'>{esc(row['error'][:200])}</span>"

    def img(kind: str) -> str:
        path = shots / f"{name}.{kind}.jpg"
        return f"<img src='{path.as_uri()}'>" if path.exists() else "<div class='label error'>(no screenshot)</div>"

    def items(key: str) -> str:
        found = row.get(key) or []
        return "".join(f"<li>{esc(text)}</li>" for text in found) or "<li>—</li>"

    return f"""
<section class='sheet'>
  <div class='head'><h2>{esc(name)}</h2><span>{scores}</span>
    <span class='url'>{esc(row.get('url', ''))}</span></div>
  <div class='left'>
    <div class='label'>The page on its site (saved copy) · title read: <b>{esc(row.get('title') or '—')}</b> · {facts}</div>
    {img('original')}
    <div class='snips'>
      <div class='missing'><h3>Left out (in the reference, not in kakemono)</h3><ul>{items('missing')}</ul></div>
      <div class='extra'><h3>Kept but not in the reference</h3><ul>{items('extra')}</ul></div>
    </div>
  </div>
  <div class='right'>
    <div class='label'>kakemono (friendly): start · middle · end</div>
    {img('top')}{img('middle')}{img('end')}
  </div>
</section>"""


def summary_page(title: str, rows: list[dict], overall: dict) -> str:
    esc = html.escape
    ok = [row for row in rows if "kakemono" in row and "error" not in row]
    f1s = [row["kakemono"]["f1"] for row in ok]
    lines = "".join(
        f"<tr><td>{esc(row['name'])}</td><td class='n {tone(row.get('kakemono', {}).get('f1', 0))}'>"
        f"{row.get('kakemono', {}).get('f1', 0):.2f}</td>"
        f"<td class='n'>{row.get('kakemono', {}).get('precision', 0):.2f}</td>"
        f"<td class='n'>{row.get('kakemono', {}).get('recall', 0):.2f}</td>"
        f"<td class='n'>{row.get('words', 0)}</td><td class='n'>{row.get('reference_words', 0)}</td>"
        f"<td class='error'>{esc(row.get('error', '')[:80])}</td></tr>"
        for row in rows
    )
    head = (
        f"<p>{len(rows)} pages · F1 {overall.get('f1', 0):.3f} · precision {overall.get('precision', 0):.3f} · "
        f"recall {overall.get('recall', 0):.3f} · median page F1 {statistics.median(f1s) if f1s else 0:.2f} · "
        f"pages under 0.7: {sum(1 for f in f1s if f < 0.7)} · errors: {overall.get('errors', 0)}</p>"
    )
    return f"""<section class='summary'><h1>{esc(title)}</h1>{head}
<table><tr><th>page</th><th>F1</th><th>precision</th><th>recall</th><th>words</th><th>reference</th><th></th></tr>{lines}</table></section>"""


def screenshot_jobs(row: dict, friendly_html: Path | None, shots: Path) -> list[dict]:
    """The screenshots of a page still to take: the saved page, and its friendly HTML at its top,
    middle and end."""
    name = row["name"]
    jobs = []
    if not (shots / f"{name}.original.jpg").exists():
        jobs.append({"url": (DATA / row["file"]).as_uri(), "base": row.get("url") or None, "out": str(shots / f"{name}.original.jpg"), "at": "top"})
    if friendly_html:
        for at in ("top", "middle", "end"):
            if not (shots / f"{name}.{at}.jpg").exists():
                jobs.append({"url": friendly_html.as_uri(), "out": str(shots / f"{name}.{at}.jpg"), "at": at})
    return jobs


def review_pages(category: str, rows: list[dict], results: dict, shots: Path) -> list[str]:
    """Writes the HTML of the review of a category, in parts of PAGES_PER_PDF pages; returns
    each part and the PDF to print it to."""
    group = sorted(
        (row for row in rows if row["category"] == category),
        key=lambda row: row.get("kakemono", {}).get("f1", 0),
    )
    parts = [group[i : i + PAGES_PER_PDF] for i in range(0, len(group), PAGES_PER_PDF)]
    to_print = []
    for number, part in enumerate(parts, 1):
        suffix = f"-{number}" if len(parts) > 1 else ""
        title = f"kakemono web benchmark — {category}{suffix}"
        body = summary_page(title, part, results["summary"].get(category, {}))
        body += "".join(sheet(row, shots) for row in part)
        page = OUT / "review" / f"{category}{suffix}.html"
        page.write_text(f"<!doctype html><meta charset='utf-8'><title>{html.escape(title)}</title><style>{CSS}</style>{body}", encoding="utf-8")
        to_print += [str(page), str(page.with_suffix(".pdf"))]
    return to_print


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--only", help="only this category")
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    results = json.loads((OUT / "results.json").read_text(encoding="utf-8"))
    rows = [row for row in results["pages"] if row.get("file")]
    if args.only:
        rows = [row for row in rows if row["category"] == args.only]
    shots = OUT / "review" / "shots"
    shots.mkdir(parents=True, exist_ok=True)

    with ThreadPoolExecutor(args.jobs) as pool:
        made = dict(zip([row["name"] for row in rows], pool.map(friendly, rows)))
    jobs = [job for row in rows for job in screenshot_jobs(row, made.get(row["name"]), shots)]
    if jobs:
        (OUT / "review" / "jobs.json").write_text(json.dumps(jobs), encoding="utf-8")
        subprocess.run(["node", str(HERE / "shoot.mjs"), str(OUT / "review" / "jobs.json")], check=False)
        taken = sum(Path(job["out"]).exists() for job in jobs)
        print(f"{taken} of {len(jobs)} screenshots")
    to_print = []
    for category in sorted({row["category"] for row in rows}):
        to_print += review_pages(category, rows, results, shots)
    subprocess.run(["node", str(HERE / "print.mjs"), *to_print], check=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
