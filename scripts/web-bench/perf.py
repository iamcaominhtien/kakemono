#!/usr/bin/env python3
"""Speed and memory of kakemono on web pages: the pages of the benchmark, and pages
written here that push the limits (a page of 50 MB, 20,000 items of a list, ...).

    python3 scripts/web-bench/perf.py sweep          # every benchmark page to Markdown, one at a time
    python3 scripts/web-bench/perf.py parallel 4     # all of them, 4 at a time: pages a second
    python3 scripts/web-bench/perf.py formats        # every output of the 5 largest pages
    python3 scripts/web-bench/perf.py stress [--pdf] # the written pages (also to PDF)

Each conversion is timed by GNU time (`/usr/bin/time`, Linux): wall time and peak
memory. A conversion that takes more than STRESS_SLOW_S on a written page is marked
SLOW: the time of a rule that compares each node with its parent, or each item with
the ones before it, grows with the square of the page. The outputs and the written
pages go to out/web-bench/perf/.
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "tests" / "data" / "web-bench"
OUT = ROOT / "out" / "web-bench" / "perf"
KAKEMONO = ROOT / "target" / "release" / "kakemono"
GNU_TIME = "/usr/bin/time"
# Seconds one conversion may take.
TIMEOUT_S = 300
# Seconds over which a written page is marked slow.
STRESS_SLOW_S = 5.0
# Items of the written pages that repeat one thing.
ITEMS = 20_000
# The pages, by their name.
HTML_FILES = "*.html"
FORMATS = ["md", "html", "docx", "tex", "typ", "epub", "json", "jsonl", "pdf"]
WORDS = "the quick brown fox jumps over lazy dog again near river bank under bright moon while birds sing".split()


def run(src: Path, out_name: str, extra: tuple[str, ...] = ()) -> dict:
    """Converts `src` to OUT/out_name under GNU time: seconds, peak MB, exit code."""
    out = OUT / out_name
    stats = OUT / f"{out_name}.time"
    command = [GNU_TIME, "-o", str(stats), "-f", "%e %M", str(KAKEMONO), "convert", str(src), "-o", str(out), *extra]
    try:
        proc = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, timeout=TIMEOUT_S)
    except subprocess.TimeoutExpired:
        return {"wall": float(TIMEOUT_S), "mb": 0.0, "code": -9, "err": "timeout"}
    wall, kb = stats.read_text().strip().splitlines()[-1].split()
    return {"wall": float(wall), "mb": int(kb) / 1024, "code": proc.returncode,
            "err": proc.stderr.decode(errors="replace")[-300:]}


def pages() -> list[Path]:
    """The pages of the benchmark (fetch.py)."""
    return sorted((DATA / "pages").glob(HTML_FILES)) + sorted((DATA / "aeb").glob(HTML_FILES))


def percentile(values: list[float], share: float) -> float:
    values = sorted(values)
    return values[min(len(values) - 1, round(share * (len(values) - 1)))]


def summary(rows: list[dict], label: str) -> None:
    walls = [row["wall"] for row in rows]
    mems = [row["mb"] for row in rows]
    print(f"{label}: {len(rows)} pages, failures {sum(row['code'] != 0 for row in rows)}")
    print(f"  time   median {percentile(walls, 0.5) * 1000:.0f} ms, p95 {percentile(walls, 0.95) * 1000:.0f} ms, "
          f"max {max(walls) * 1000:.0f} ms, total {sum(walls):.1f} s")
    print(f"  memory median {percentile(mems, 0.5):.0f} MB, p95 {percentile(mems, 0.95):.0f} MB, max {max(mems):.0f} MB")


def sweep() -> None:
    rows = []
    for at, src in enumerate(pages()):
        row = run(src, f"sweep-{at}.md")
        rows.append({"name": src.stem, "kb": src.stat().st_size // 1024, **row})
    summary(rows, "sweep (Markdown)")
    print("  slowest:")
    for row in sorted(rows, key=lambda row: -row["wall"])[:6]:
        print(f"    {row['wall'] * 1000:6.0f} ms {row['mb']:5.0f} MB {row['kb']:6} KB  {row['name'][:60]}")
    for row in rows:
        if row["code"]:
            print("  FAILED", row["name"], row["err"])
    (OUT / "sweep.json").write_text(json.dumps(rows, indent=1))


def parallel(jobs: int) -> None:
    files = pages()
    start = time.perf_counter()
    with ThreadPoolExecutor(jobs) as pool:
        rows = list(pool.map(lambda item: run(item[1], f"parallel-{item[0]}.md"), enumerate(files)))
    wall = time.perf_counter() - start
    print(f"{jobs} at a time: {len(files)} pages in {wall:.1f} s, {len(files) / wall:.0f} pages a second, "
          f"most memory of one {max(row['mb'] for row in rows):.0f} MB, failures {sum(row['code'] != 0 for row in rows)}")


def formats() -> None:
    largest = sorted(pages(), key=lambda page: -page.stat().st_size)[:5]
    print(f"{'page':38} {'KB':>6} " + " ".join(f"{fmt:>12}" for fmt in FORMATS))
    for src in largest:
        cells = []
        for fmt in FORMATS:
            extra = ("-m", "friendly") if fmt == "html" else ()
            row = run(src, f"{src.stem}.{fmt}", extra)
            cells.append(f"{row['wall'] * 1000:5.0f}ms/{row['mb']:4.0f}M" if row["code"] == 0 else f"exit {row['code']}")
        print(f"{src.stem[:38]:38} {src.stat().st_size // 1024:6} " + " ".join(f"{cell:>12}" for cell in cells))


def paragraph(number: int) -> str:
    """A paragraph of 60 words, different for each number (the same paragraphs are
    folded into one on purpose)."""
    words = (WORDS[(number * 7 + k) % len(WORDS)] + (str(number % 97) if k % 5 == 0 else "") for k in range(60))
    return f"<p>{' '.join(words)} Paragraph {number}.</p>\n"


def article(body: str) -> str:
    """A page whose article holds `body` and some paragraphs."""
    text = "".join(paragraph(number) for number in range(20))
    return ("<!doctype html><html><head><meta charset='utf-8'><title>Stress - Site</title>"
            "<meta property='og:site_name' content='Site'></head>"
            f"<body><main><article><h1>Stress</h1>{body}{text}</article></main></body></html>")


def repeat(template: str, count: int = ITEMS) -> str:
    return "".join(template.format(i=i) for i in range(count))


def written_pages() -> dict[str, str]:
    """The pages that push the limits, by name."""
    picture = base64.b64encode((ROOT / "tests" / "data" / "word" / "curve.png").read_bytes()).decode()
    return {
        "big-48mb": article("".join(paragraph(number) for number in range(130_000))),
        "one-line-20mb": article("<p>" + "word " * 4_000_000 + "</p>"),
        "sections": article(repeat("<h2>Section {i}</h2><p>Text of section {i}.</p>")),
        "nested-5000": article("<div>" * 5_000 + "<p>deep</p>" + "</div>" * 5_000),
        "unclosed-tags": article("<p><b><i><span>" * 5_000),
        "table-5000-rows": article("<table>" + repeat("<tr>" + "<td>r{i}</td>" * 20 + "</tr>", 5_000) + "</table>"),
        "links": article("<p>" + repeat("<a href='/p{i}'>link {i}</a> ", 100_000) + "</p>"),
        "pictures": article(repeat(f"<figure><img src='data:image/png;base64,{picture}'><figcaption>Figure {{i}}</figcaption></figure>", 2_000)),
        "formulas": article(repeat("<p>Formula <math><mfrac><mi>a{i}</mi><msqrt><mi>b</mi></msqrt></mfrac></math>.</p>", 10_000)),
        "footnotes": article(repeat("<p>Claim {i}<sup id='c{i}'><a href='#n{i}'>[{i}]</a></sup></p>")
                             + "<ol>" + repeat("<li id='n{i}'><a href='#c{i}'>^</a> Note {i}</li>") + "</ol>"),
        "one-note-cited": article(repeat("<p>Claim {i}<sup><a href='#n0'>[0]</a></sup></p>") + "<ol><li id='n0'>Note</li></ol>"),
        "clutter-boxes": article(repeat("<div class='ad sidebar share'><a href='/x{i}'>x</a></div>", 200_000)),
        "list-items": article("<ul>" + repeat("<li>item {i} <a href='/i{i}'>x</a></li>") + "</ul>"),
        "menu-spans": article("<p>" + repeat("<span class='menu'>w{i}</span> ") + "</p>"),
        "number-paragraphs": article("<div>" + repeat("<p>{i}</p>") + "</div>"),
        "line-breaks": article("<p>" + repeat("line {i}<br><br>") + "</p>"),
        "code-lines": article("<pre>" + repeat("line {i}\n", 5 * ITEMS) + "</pre>"),
        "same-headings": article(repeat("<h3>This is a heading that reads like a whole sentence number {i}.</h3>")),
        "teasers": article(repeat("<div><a href='/post{i}'><h3>Story {i}</h3></a></div>")),
        "garbage-bytes": "",
    }


def write_pages() -> Path:
    """Writes the pages of written_pages() that are not written yet; returns their folder."""
    written = OUT / "written"
    written.mkdir(exist_ok=True)
    for name, html in written_pages().items():
        path = written / f"{name}.html"
        if not path.exists():
            path.write_bytes(html.encode() if html else os.urandom(5_000_000))
    return written


def stress(pdf: bool) -> None:
    written = write_pages()
    outputs = ["md", "pdf"] if pdf else ["md"]
    print(f"{'page':20} {'MB':>5}  " + "  ".join(f"{fmt:>16}" for fmt in outputs))
    for path in sorted(written.glob(HTML_FILES)):
        cells = []
        for fmt in outputs:
            row = run(path, f"{path.stem}.{fmt}")
            mark = " SLOW" if row["wall"] > STRESS_SLOW_S else ""
            failed = f" exit {row['code']}" if row["code"] else ""
            cells.append(f"{row['wall']:6.2f} s {row['mb']:5.0f} MB{mark}{failed}")
        print(f"{path.stem:20} {path.stat().st_size / 1e6:5.1f}  " + "  ".join(f"{cell:>16}" for cell in cells), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="what", required=True)
    sub.add_parser("sweep")
    sub.add_parser("formats")
    jobs = sub.add_parser("parallel")
    jobs.add_argument("jobs", type=int)
    written = sub.add_parser("stress")
    written.add_argument("--pdf", action="store_true", help="also write each page as a PDF")
    args = parser.parse_args()
    OUT.mkdir(parents=True, exist_ok=True)
    if args.what == "sweep":
        sweep()
    elif args.what == "formats":
        formats()
    elif args.what == "parallel":
        parallel(args.jobs)
    else:
        stress(args.pdf)


if __name__ == "__main__":
    main()
