#!/usr/bin/env python3
"""Packs kakemono's conversions of the pages of the web benchmark (fetch.py and
evaluate.py first): for each page its friendly HTML (pictures inside) and a PDF (the
Typst output compiled with `typst`, or `-o page.pdf` when Typst fails), in
out/web-bench/package/<category>/<site>/, with an index.html that lists them with
their scores, and one zip per category.

    /root/.venvs/web-bench/bin/python scripts/web-bench/package.py [--jobs 8] [--only CATEGORY]
"""

from __future__ import annotations

import argparse
import html
import json
import shutil
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / "tests" / "data" / "web-bench"
OUT = ROOT / "out" / "web-bench"
PACKAGE = OUT / "package"
# The page that lists the others.
INDEX = "index.html"
KAKEMONO = ROOT / "target" / "release" / "kakemono"
# Seconds one conversion or compilation may take.
TIMEOUT_S = 300
# Names of the categories, as the index shows them.
TITLES = {
    "news-vi": "Báo Việt Nam",
    "news": "Báo quốc tế",
    "tech": "Báo công nghệ",
    "blog": "Blog",
    "docs": "Tài liệu kỹ thuật",
    "wiki": "Wikipedia",
    "science": "Khoa học",
    "gov": "Chính phủ",
    "other": "Khác (trang chủ, bài dài)",
    "aeb": "Benchmark Scrapinghub (181 bài báo có đáp án, bản lưu 2019)",
}


def site(row: dict) -> str:
    host = urlparse(row.get("url") or "").netloc.lower()
    return host.removeprefix("www.").removeprefix("edition.") or "unknown"


def run(command: list[str], cwd: Path | None = None) -> bool:
    try:
        done = subprocess.run(command, capture_output=True, timeout=TIMEOUT_S, check=False, cwd=cwd)
    except subprocess.TimeoutExpired:
        return False
    return done.returncode == 0


def pack(row: dict) -> dict:
    folder = PACKAGE / row["category"] / site(row)
    folder.mkdir(parents=True, exist_ok=True)
    source = DATA / row["file"]
    base = ["--base-url", row["url"]] if row.get("url") else []
    page_html = folder / f"{row['name']}.html"
    page_pdf = folder / f"{row['name']}.pdf"
    made = {"html": None, "pdf": None, "pdf_by": None}
    if page_html.exists() or run([str(KAKEMONO), "convert", str(source), *base, "-o", str(page_html)]):
        made["html"] = page_html
    if page_pdf.exists():
        made["pdf"], made["pdf_by"] = page_pdf, "typst"
    else:
        with tempfile.TemporaryDirectory() as work:
            typ = Path(work) / "page.typ"
            if run([str(KAKEMONO), "convert", str(source), *base, "-o", str(typ)]) and run(
                ["typst", "compile", str(typ), str(page_pdf)], cwd=Path(work)
            ):
                made["pdf"], made["pdf_by"] = page_pdf, "typst"
            elif run([str(KAKEMONO), "convert", str(source), *base, "-o", str(page_pdf)]):
                made["pdf"], made["pdf_by"] = page_pdf, "word layout"
    return {**row, **made}


def tone(f1: float | None) -> str:
    """The class of the colour of a score."""
    if f1 is None:
        return ""
    if f1 < 0.7:
        return "bad"
    return "mid" if f1 < 0.9 else "good"


def index_row(row: dict) -> str:
    """The row of a page in the index: its title and address, its score, its files."""
    esc = html.escape
    f1 = row.get("kakemono", {}).get("f1")
    score = "" if f1 is None else f"{f1:.2f}"
    links = []
    for kind in ("html", "pdf"):
        path = row.get(kind)
        if path:
            links.append(f"<a href='{esc(str(Path(path).relative_to(PACKAGE)))}'>{kind.upper()}</a>")
    title = esc(row.get("title") or row["name"])
    url = esc(row.get("url") or "")
    return (
        f"<tr><td>{title}<br><small><a href='{url}'>{url[:90]}</a></small></td>"
        f"<td class='n {tone(f1)}'>{score}</td><td>{' · '.join(links) or '<span class=bad>lỗi</span>'}</td></tr>"
    )


def index_page(rows: list[dict]) -> str:
    esc = html.escape
    parts = [
        "<!doctype html><meta charset='utf-8'><title>kakemono — web benchmark</title>",
        "<style>body{font:15px/1.5 system-ui,sans-serif;max-width:1100px;margin:2em auto;padding:0 1em}"
        "table{border-collapse:collapse;width:100%}td,th{border-bottom:1px solid #ddd;padding:4px 8px;text-align:left}"
        "td.n{text-align:right;font-variant-numeric:tabular-nums}.bad{color:#b00020}.mid{color:#b36b00}.good{color:#1b7f3b}"
        "h2{margin-top:2em}h3{margin:1.2em 0 .3em;font-size:1em;color:#555}</style>",
        "<h1>kakemono — các trang web đã chuyển</h1>",
        "<p>Mỗi trang: bản HTML (chế độ friendly, ảnh nhúng sẵn) và bản PDF. F1 là điểm so phần nội dung "
        "kakemono giữ với đáp án (Scrapinghub) hoặc với trafilatura (các nhóm khác).</p>",
    ]
    order = [key for key in TITLES if any(row["category"] == key for row in rows)]
    for category in order:
        group = [row for row in rows if row["category"] == category]
        parts.append(f"<h2>{esc(TITLES[category])} ({len(group)})</h2>")
        for host in sorted({site(row) for row in group}):
            parts.append(f"<h3>{esc(host)}</h3><table>")
            for row in sorted((row for row in group if site(row) == host), key=lambda row: row["name"]):
                parts.append(index_row(row))
            parts.append("</table>")
    return "".join(parts)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--only", help="only this category")
    args = parser.parse_args()
    results = json.loads((OUT / "results.json").read_text(encoding="utf-8"))
    rows = [row for row in results["pages"] if row.get("file")]
    if args.only:
        rows = [row for row in rows if row["category"] == args.only]
    with ThreadPoolExecutor(args.jobs) as pool:
        done = list(pool.map(pack, rows))
    (PACKAGE / INDEX).write_text(index_page(done), encoding="utf-8")
    failed = [row["name"] for row in done if not (row["html"] and row["pdf"])]
    by_word = [row["name"] for row in done if row["pdf_by"] == "word layout"]
    print(f"{len(done) - len(failed)} of {len(done)} pages packed; PDF by the Word layout: {len(by_word)}")
    for name in failed:
        print(f"  failed: {name}")
    for category in sorted({row["category"] for row in done}):
        archive = OUT / f"kakemono-web-{category}"
        with tempfile.TemporaryDirectory() as work:
            # Each zip holds the index too, so that it opens on its own.
            staging = Path(work) / f"kakemono-web-{category}"
            shutil.copytree(PACKAGE / category, staging / category)
            shutil.copy(PACKAGE / INDEX, staging / INDEX)
            shutil.make_archive(str(archive), "zip", work)
        print(f"{archive}.zip")
    return 0


if __name__ == "__main__":
    sys.exit(main())
