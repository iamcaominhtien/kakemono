#!/usr/bin/env python3
"""Downloads the pages of the web benchmark into tests/data/web-bench/ (not committed:
the pages belong to their sites):

- aeb/: the 181 pages of the Scrapinghub article extraction benchmark, with their
  ground truth (the text of each article, marked by people);
- pages/: the pages of scripts/web-bench/pages.tsv, as their sites serve them now.

A page already downloaded is kept (delete it to download it again), so that the
benchmark compares versions of kakemono on the same pages. index.json lists every
page: its set, category, address, and where its reference text comes from.

    python3 scripts/web-bench/fetch.py [--jobs 8]
"""

from __future__ import annotations

import argparse
import gzip
import json
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
DATA = ROOT / "tests" / "data" / "web-bench"
AEB_RAW = "https://raw.githubusercontent.com/scrapinghub/article-extraction-benchmark/master"
USER_AGENT = "Mozilla/5.0 (compatible; kakemono-bench; +https://github.com/iamcaominhtien/kakemono)"


def curl(url: str, dest: Path) -> bool:
    """Downloads `url` to `dest`; False when it fails (the file is then absent)."""
    part = dest.with_suffix(dest.suffix + ".part")
    run = subprocess.run(
        ["curl", "-fsSL", "--retry", "2", "-m", "60", "-A", USER_AGENT, "-o", str(part), url],
        capture_output=True,
        check=False,
    )
    if run.returncode != 0 or not part.exists() or part.stat().st_size == 0:
        part.unlink(missing_ok=True)
        return False
    part.rename(dest)
    return True


def fetch_aeb(jobs: int) -> list[dict]:
    """The Scrapinghub benchmark: its ground truth and its pages."""
    folder = DATA / "aeb"
    folder.mkdir(parents=True, exist_ok=True)
    truth_file = folder / "ground-truth.json"
    if not truth_file.exists() and not curl(f"{AEB_RAW}/ground-truth.json", truth_file):
        print("could not download the ground truth of the Scrapinghub benchmark", file=sys.stderr)
        return []
    truth = json.loads(truth_file.read_text(encoding="utf-8"))

    def one(key: str) -> dict | None:
        html = folder / f"{key}.html"
        if not html.exists():
            gz = folder / f"{key}.html.gz"
            if not curl(f"{AEB_RAW}/html/{key}.html.gz", gz):
                return None
            html.write_bytes(gzip.decompress(gz.read_bytes()))
            gz.unlink()
        return {
            "name": f"aeb-{key[:12]}",
            "set": "aeb",
            "category": "aeb",
            "url": truth[key].get("url", ""),
            "file": str(html.relative_to(DATA)),
            "reference": "ground-truth",
            "truth": truth[key].get("articleBody", ""),
        }

    with ThreadPoolExecutor(jobs) as pool:
        pages = [page for page in pool.map(one, sorted(truth)) if page]
    print(f"aeb: {len(pages)} of {len(truth)} pages")
    return pages


def fetch_pages(jobs: int) -> list[dict]:
    """The pages of pages.tsv."""
    folder = DATA / "pages"
    folder.mkdir(parents=True, exist_ok=True)
    rows = []
    for line in (HERE / "pages.tsv").read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        category, name, url = line.split("\t")
        rows.append((category, name, url))

    def one(row: tuple[str, str, str]) -> dict:
        category, name, url = row
        html = folder / f"{name}.html"
        ok = html.exists() or curl(url, html)
        return {
            "name": name,
            "set": "pages",
            "category": category,
            "url": url,
            "file": str(html.relative_to(DATA)) if ok else None,
            "reference": "trafilatura",
        }

    with ThreadPoolExecutor(jobs) as pool:
        pages = list(pool.map(one, rows))
    missing = [page["name"] for page in pages if not page["file"]]
    print(f"pages: {len(pages) - len(missing)} of {len(pages)} downloaded")
    for name in missing:
        print(f"  not downloaded: {name}")
    return pages


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--jobs", type=int, default=8)
    args = parser.parse_args()
    DATA.mkdir(parents=True, exist_ok=True)
    pages = fetch_aeb(args.jobs) + fetch_pages(args.jobs)
    (DATA / "index.json").write_text(json.dumps(pages, ensure_ascii=False, indent=1), encoding="utf-8")
    print(f"{sum(1 for page in pages if page['file'])} pages in {DATA}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
