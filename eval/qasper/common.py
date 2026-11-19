"""Shared paths, text normalisation and token counting for the Qasper evaluation."""

import json
import re
from functools import lru_cache
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
QASPER = DATA / "qasper-test-v0.3.json"
SAMPLE = DATA / "sample.txt"
PDF = DATA / "pdf"
CHUNKS = DATA / "chunks"
RESULTS = DATA / "results"
KAKEMONO = ROOT.parent.parent / "target" / "release" / "kakemono"

# Every chunk is at most this long, so that no embedding model truncates one.
MAX_CHUNK_TOKENS = 512
# The tokenizer that counts tokens for every pipeline (chunk sizes and budgets).
TOKENIZER = "Qwen/Qwen3-Embedding-0.6B"


@lru_cache(maxsize=1)
def tokenizer():
    from transformers import AutoTokenizer

    return AutoTokenizer.from_pretrained(TOKENIZER)


def count_tokens(text: str) -> int:
    return len(tokenizer()(text, add_special_tokens=False)["input_ids"])


def cut_tokens(text: str, n: int) -> str:
    """The first `n` tokens of `text`."""
    ids = tokenizer()(text, add_special_tokens=False)["input_ids"]
    return text if len(ids) <= n else tokenizer().decode(ids[:n])


def sample_ids() -> list[str]:
    return [line.strip() for line in SAMPLE.read_text().splitlines() if line.strip()]


def qasper() -> dict:
    return json.loads(QASPER.read_text())


# S2ORC, which Qasper's text comes from, replaces citations, references to
# figures and formulas by placeholders; they have no counterpart in a PDF.
_PLACEHOLDER = re.compile(r"\b(?:BIBREF|FIGREF|TABREF|SECREF|EQREF|INLINEFORM|DISPLAYFORM)\d*\b")
_WORD = re.compile(r"[a-z0-9]+")


def words(text: str) -> list[str]:
    """Lower-case alphanumeric words, placeholders dropped, hyphenation joined."""
    text = _PLACEHOLDER.sub(" ", text)
    text = re.sub(r"(\w)-\s*\n\s*(\w)", r"\1\2", text)  # "seg-\nment" -> "segment"
    return _WORD.findall(text.lower())


def ngrams(ws: list[str], n: int = 3) -> set[tuple[str, ...]]:
    return {tuple(ws[i : i + n]) for i in range(len(ws) - n + 1)}


def write_jsonl(path: Path, rows) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w") as f:
        for r in rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")


def read_jsonl(path: Path) -> list[dict]:
    # split on "\n" only: str.splitlines() also splits on U+2028, which the text can hold
    return [json.loads(line) for line in path.read_text().split("\n") if line.strip()]
