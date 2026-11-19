"""Chunks from the other converters, each with its own recommended chunker.

    .venv-tools/bin/python tools_chunk.py [pipeline ...]

Runs in its own virtual environment (.venv-tools), so that the converters'
dependencies do not change the one that embeds.

  docling-hybrid      docling (default options) + its HybridChunker, contextualized
                      (headings prepended), at most MAX_CHUNK_TOKENS tokens
  unstructured-title  unstructured partition_pdf (fast strategy) + chunk_by_title,
                      pieces still too long cut at MAX_CHUNK_TOKENS tokens
"""

import sys
import time

from chunk import fixed_chunks
from common import CHUNKS, MAX_CHUNK_TOKENS, PDF, TOKENIZER, count_tokens, sample_ids, write_jsonl


def docling_chunker():
    from docling.chunking import HybridChunker
    from docling.document_converter import DocumentConverter
    from docling_core.transforms.chunker.tokenizer.huggingface import HuggingFaceTokenizer
    from transformers import AutoTokenizer

    conv = DocumentConverter()
    tok = HuggingFaceTokenizer(tokenizer=AutoTokenizer.from_pretrained(TOKENIZER), max_tokens=MAX_CHUNK_TOKENS)
    chunker = HybridChunker(tokenizer=tok)

    def run(pdf):
        doc = conv.convert(str(pdf)).document
        return [chunker.contextualize(c) for c in chunker.chunk(dl_doc=doc)]

    run.conv, run.chunker = conv, chunker  # timing.py times the two steps apart
    return run


def unstructured_chunker():
    from unstructured.chunking.title import chunk_by_title
    from unstructured.partition.pdf import partition_pdf

    def run(pdf):
        elements = partition_pdf(filename=str(pdf), strategy="fast")
        # about 4 characters per token: aim under the token limit, then enforce it
        chunks = [c.text for c in chunk_by_title(elements, max_characters=MAX_CHUNK_TOKENS * 4)]
        return [p for c in chunks for p in (fixed_chunks(c) if count_tokens(c) > MAX_CHUNK_TOKENS else [c])]

    return run


PIPELINES = {"docling-hybrid": docling_chunker, "unstructured-title": unstructured_chunker}


def main():
    for pipeline in sys.argv[1:] or list(PIPELINES):
        run, n, t = PIPELINES[pipeline](), 0, time.time()
        for pid in sample_ids():
            out = CHUNKS / pipeline / f"{pid}.jsonl"
            if out.exists():
                continue
            try:
                chunks = [c for c in run(PDF / f"{pid}.pdf") if c.strip()]
            except Exception as e:  # a failed paper counts as no text, not as left out
                print(f"{pipeline} {pid}: FAILED {e}", file=sys.stderr)
                chunks = []
            write_jsonl(out, [{"i": i, "text": c, "tokens": count_tokens(c)} for i, c in enumerate(chunks)])
            n += len(chunks)
        print(f"{pipeline}: {n} chunks in {time.time() - t:.0f}s", flush=True)


if __name__ == "__main__":
    main()
