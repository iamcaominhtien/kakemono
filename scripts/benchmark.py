#!/usr/bin/env python3
"""Benchmark the kakemono CLI: time, memory and CPU of conversions.

Three experiments, each printed as a Markdown table (and written as CSV
with --csv DIR):

  formats     every PDF converted to every output format, one at a time
  threads     one PDF analysed with -j 1, 2, 4, ... (up to the CPU count)
  concurrent  N copies of one PDF converted at the same time, N = 1, 2, 4, ...

Every run is measured with GNU time (`/usr/bin/time -v`: wall time, peak
resident memory, CPU share); the analysis time is read from the CLI's own
report. The concurrent runs also sample the memory used by the whole
machine every 50 ms, which is what limits how many files fit at once.

Usage:
  scripts/benchmark.py formats    PDF... [--formats friendly,epub,...]
  scripts/benchmark.py threads    PDF
  scripts/benchmark.py concurrent PDF [--max N] [--jobs J]

Needs a release build (cargo build --release) and GNU time.
"""

from __future__ import annotations

import argparse
import csv
import os
import re
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIN = ROOT / "target" / "release" / "kakemono"
GNU_TIME = "/usr/bin/time"

# Output format -> (extra CLI arguments, file extension).
FORMATS = {
    "friendly": (["-m", "friendly"], "html"),
    "medium": (["-m", "medium"], "html"),
    "hard": (["-m", "hard"], "html"),
    "hard-95": (["-m", "hard-95"], "html"),
    "epub": ([], "epub"),
    "docx": ([], "docx"),
    "md": ([], "md"),
    "tex": ([], "tex"),
}


def pages(pdf: Path) -> int:
    """Number of pages, from pdfinfo (0 when it is not installed)."""
    try:
        out = subprocess.run(["pdfinfo", str(pdf)], capture_output=True, text=True).stdout
    except FileNotFoundError:
        return 0
    found = re.search(r"^Pages:\s+(\d+)", out, re.M)
    return int(found.group(1)) if found else 0


def output_size(path: Path) -> int:
    """Bytes of an output file plus its picture folder, if any."""
    total = path.stat().st_size if path.exists() else 0
    assets = path.with_name(path.stem + "_assets")
    if assets.is_dir():
        total += sum(f.stat().st_size for f in assets.rglob("*") if f.is_file())
    return total


def convert(pdf: Path, fmt: str, out_dir: Path, jobs: int | None = None) -> dict:
    """Runs one conversion under GNU time and returns its measures."""
    args, ext = FORMATS[fmt]
    out = out_dir / f"{pdf.stem}.{fmt}.{ext}"
    cmd = [GNU_TIME, "-v", str(BIN), "convert", str(pdf), *args, "-o", str(out)]
    if jobs is not None:
        cmd += ["-j", str(jobs)]
    proc = subprocess.run(cmd, capture_output=True, text=True)
    err = proc.stderr
    if proc.returncode != 0:
        raise RuntimeError(f"{pdf.name} {fmt} failed:\n{err[-2000:]}")

    def field(name: str) -> str:
        found = re.search(rf"^\s*{re.escape(name)}: (.+)$", err, re.M)
        return found.group(1).strip() if found else ""

    wall = field("Elapsed (wall clock) time (h:mm:ss or m:ss)")
    parts = [float(p) for p in wall.split(":")]
    seconds = sum(p * 60 ** i for i, p in enumerate(reversed(parts)))
    analysed = re.search(r"tables rebuilt in ([\d.]+)s", err)
    return {
        "file": pdf.stem,
        "format": fmt,
        "pages": pages(pdf),
        "wall_s": round(seconds, 2),
        "analysis_s": float(analysed.group(1)) if analysed else 0.0,
        "peak_rss_mb": round(int(field("Maximum resident set size (kbytes)")) / 1024, 1),
        "cpu_pct": int(field("Percent of CPU this job got").rstrip("%") or 0),
        "output_mb": round(output_size(out) / 1e6, 2),
    }


def table(rows: list[dict], columns: list[str]) -> str:
    """A Markdown table of `rows` with these columns."""
    lines = ["| " + " | ".join(columns) + " |", "|" + "---|" * len(columns)]
    lines += ["| " + " | ".join(str(row[c]) for c in columns) + " |" for row in rows]
    return "\n".join(lines)


def save(rows: list[dict], name: str, csv_dir: str | None) -> None:
    """Writes `rows` to `csv_dir/name.csv` when a CSV folder was given."""
    if not csv_dir or not rows:
        return
    Path(csv_dir).mkdir(parents=True, exist_ok=True)
    with open(Path(csv_dir) / f"{name}.csv", "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=list(rows[0]))
        writer.writeheader()
        writer.writerows(rows)


def bench_formats(pdfs: list[Path], formats: list[str], out_dir: Path) -> list[dict]:
    rows = []
    for pdf in pdfs:
        for fmt in formats:
            row = convert(pdf, fmt, out_dir)
            row["pages_per_s"] = round(row["pages"] / row["wall_s"], 1) if row["wall_s"] else 0
            rows.append(row)
            print(f"  {pdf.stem:<14} {fmt:<9} {row['wall_s']:>7.2f}s {row['peak_rss_mb']:>8.1f} MB", file=sys.stderr)
    return rows


def bench_threads(pdf: Path, out_dir: Path) -> list[dict]:
    rows = []
    counts = [1]
    while counts[-1] * 2 <= (os.cpu_count() or 1):
        counts.append(counts[-1] * 2)
    for jobs in counts:
        row = convert(pdf, "friendly", out_dir, jobs)
        row["jobs"] = jobs
        rows.append(row)
        print(f"  -j {jobs}: {row['wall_s']}s", file=sys.stderr)
    base = rows[0]["wall_s"]
    for row in rows:
        row["speedup"] = round(base / row["wall_s"], 2)
    return rows


class MemorySampler(threading.Thread):
    """Samples the memory in use on the machine (MemTotal - MemAvailable)."""

    def __init__(self) -> None:
        super().__init__(daemon=True)
        self.peak_mb = 0.0
        self.stop = threading.Event()

    @staticmethod
    def used_mb() -> float:
        info = dict(line.split(":", 1) for line in open("/proc/meminfo"))
        kb = lambda key: int(info[key].split()[0])
        return (kb("MemTotal") - kb("MemAvailable")) / 1024

    def run(self) -> None:
        base = self.used_mb()
        while not self.stop.is_set():
            self.peak_mb = max(self.peak_mb, self.used_mb() - base)
            time.sleep(0.05)


def bench_concurrent(pdf: Path, max_n: int, jobs: int | None, out_dir: Path) -> list[dict]:
    rows = []
    n_pages = pages(pdf)
    n = 1
    while n <= max_n:
        sampler = MemorySampler()
        sampler.start()
        start = time.monotonic()
        results: list[dict] = [{}] * n

        def work(i: int) -> None:
            sub = out_dir / f"c{i}"
            sub.mkdir(exist_ok=True)
            results[i] = convert(pdf, "friendly", sub, jobs)

        threads = [threading.Thread(target=work, args=(i,)) for i in range(n)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
        total = time.monotonic() - start
        sampler.stop.set()
        sampler.join()
        walls = [r["wall_s"] for r in results]
        rows.append({
            "file": pdf.stem,
            "concurrent": n,
            "jobs_each": jobs if jobs is not None else "auto",
            "total_s": round(total, 2),
            "mean_wall_s": round(sum(walls) / n, 2),
            "max_wall_s": max(walls),
            "files_per_min": round(60 * n / total, 1),
            "pages_per_s": round(n * n_pages / total, 1),
            "max_rss_each_mb": max(r["peak_rss_mb"] for r in results),
            "machine_peak_mb": round(sampler.peak_mb),
        })
        print(f"  {n} at once: {total:.2f}s", file=sys.stderr)
        n *= 2
    return rows


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("experiment", choices=["formats", "threads", "concurrent"])
    parser.add_argument("pdfs", nargs="+", type=Path)
    parser.add_argument("--formats", default="friendly,epub,docx,md,tex")
    parser.add_argument("--max", type=int, default=16, help="largest number of concurrent conversions")
    parser.add_argument("--jobs", type=int, default=None, help="-j of each concurrent conversion")
    parser.add_argument("--csv", default=None, help="folder to write the results as CSV")
    args = parser.parse_args()
    if not BIN.exists():
        sys.exit(f"{BIN} not found: run cargo build --release first")
    with tempfile.TemporaryDirectory(prefix="kakemono-bench-") as tmp:
        out_dir = Path(tmp)
        if args.experiment == "formats":
            rows = bench_formats(args.pdfs, args.formats.split(","), out_dir)
            cols = ["file", "pages", "format", "wall_s", "analysis_s", "pages_per_s", "peak_rss_mb", "cpu_pct", "output_mb"]
        elif args.experiment == "threads":
            rows = bench_threads(args.pdfs[0], out_dir)
            cols = ["file", "pages", "jobs", "wall_s", "analysis_s", "speedup", "peak_rss_mb", "cpu_pct"]
        else:
            rows = bench_concurrent(args.pdfs[0], args.max, args.jobs, out_dir)
            cols = list(rows[0])
    print(table(rows, cols))
    save(rows, args.experiment, args.csv)


if __name__ == "__main__":
    main()
