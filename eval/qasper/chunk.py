"""Turn each sample paper into chunks, one JSONL file per pipeline and paper.

    python chunk.py [pipeline ...]

Pipelines:
  pypdf-fixed      pypdf's text, cut every MAX_CHUNK_TOKENS tokens (the usual baseline)
  kakemono-fixed   kakemono's text in reading order, cut the same way (isolates the text)
  kakemono-struct  kakemono's blocks grouped by section, never cut inside a block
                   unless it is too long, each chunk headed by its section path
"""

import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from common import CHUNKS, KAKEMONO, MAX_CHUNK_TOKENS, PDF, count_tokens, cut_tokens, sample_ids, tokenizer, write_jsonl


# ---------------------------------------------------------------- text sources


def pypdf_text(pdf: Path) -> str:
    import logging

    from pypdf import PdfReader

    logging.getLogger("pypdf").setLevel(logging.ERROR)

    return "\n".join(page.extract_text() or "" for page in PdfReader(pdf).pages)


def kakemono_json(pdf: Path) -> dict:
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "paper.json"
        subprocess.run([str(KAKEMONO), "convert", str(pdf), "-o", str(out)], check=True, capture_output=True)
        return json.loads(out.read_text())


def block_text(b: dict) -> str:
    """The text of one block, as a language model should read it."""
    if b["type"] == "figure":
        parts = [b.get("plain", "")]
        table = b.get("table")
        if table:
            for row in table["rows"]:
                parts.append(" | ".join(c.get("plain", "") for c in row))
        elif b.get("figure_text"):
            parts.append(b["figure_text"])
        return "\n".join(p for p in parts if p)
    return b.get("plain") or ""


# ---------------------------------------------------------------- chunkers


def fixed_chunks(text: str, size: int = MAX_CHUNK_TOKENS) -> list[str]:
    ids = tokenizer()(text, add_special_tokens=False)["input_ids"]
    out = [tokenizer().decode(ids[i : i + size]) for i in range(0, len(ids), size)]
    return [c for c in out if c.strip()]


_SENTENCE_END = re.compile(r"(?<=[.!?])\s+(?=[A-Z0-9(\[])")


def split_long(text: str, budget: int) -> list[str]:
    """Split a block too long for one chunk at sentence ends (else hard cuts)."""
    pieces, cur = [], ""
    for s in _SENTENCE_END.split(text):
        cand = f"{cur} {s}".strip()
        if count_tokens(cand) <= budget:
            cur = cand
            continue
        if cur:
            pieces.append(cur)
        while count_tokens(s) > budget:
            head = cut_tokens(s, budget)
            pieces.append(head)
            s = s[len(head) :].strip()
        cur = s
    if cur:
        pieces.append(cur)
    return pieces


def struct_chunks(doc: dict, size: int = MAX_CHUNK_TOKENS) -> list[str]:
    blocks = doc["blocks"]
    by_id = {b["id"]: b for b in blocks}

    def path(section_id):
        names = []
        while section_id is not None and section_id in by_id:
            h = by_id[section_id]
            names.append(h.get("plain", ""))
            section_id = h.get("parent")
        return " > ".join(reversed(names))

    title = doc.get("meta", {}).get("title", "")
    chunks: list[str] = []
    head, body = "", []

    def flush():
        if body:
            chunks.append((head + "\n\n" if head else "") + "\n\n".join(body))
            body.clear()

    for b in blocks:
        if b["type"] == "heading":
            flush()
            continue
        text = block_text(b).strip()
        if not text:
            continue
        new_head = " | ".join(p for p in (title, path(b.get("section"))) if p)
        if new_head != head:
            flush()
            head = new_head
        room = size - count_tokens(head) - 2
        for piece in split_long(text, room) if count_tokens(text) > room else [text]:
            if body and count_tokens(head + "\n\n" + "\n\n".join(body + [piece])) > size:
                flush()
            body.append(piece)
    flush()
    return chunks


# ---------------------------------------------------------------- main


def build(pipeline: str, pid: str) -> list[str]:
    pdf = PDF / f"{pid}.pdf"
    if pipeline == "pypdf-fixed":
        return fixed_chunks(pypdf_text(pdf))
    doc = kakemono_json(pdf)
    if pipeline == "kakemono-fixed":
        return fixed_chunks("\n\n".join(t for t in map(block_text, doc["blocks"]) if t))
    if pipeline == "kakemono-struct":
        return struct_chunks(doc)
    raise SystemExit(f"unknown pipeline {pipeline}")


PIPELINES = ["pypdf-fixed", "kakemono-fixed", "kakemono-struct"]


def main():
    pipelines = sys.argv[1:] or PIPELINES
    for pipeline in pipelines:
        n = 0
        for pid in sample_ids():
            out = CHUNKS / pipeline / f"{pid}.jsonl"
            try:
                chunks = build(pipeline, pid)
            except Exception as e:  # a failed paper counts as no text, not as left out
                print(f"{pipeline} {pid}: FAILED {e}", file=sys.stderr)
                chunks = []
            write_jsonl(out, [{"i": i, "text": c, "tokens": count_tokens(c)} for i, c in enumerate(chunks)])
            n += len(chunks)
        print(f"{pipeline}: {n} chunks")


if __name__ == "__main__":
    main()
