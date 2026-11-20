"""Time each pipeline's conversion and chunking, without embedding.

    python timing.py <pipeline> [n_papers]          # pypdf, kakemono: .venv
    .venv-tools/bin/python timing.py <pipeline> ... # docling, unstructured

Papers run one after another on the sample, in order. Model loading (docling's
layout and table models, the tokenizer) happens once before the clock starts
and is reported apart, with one warm-up run on the first paper, since some
models only load on first use. Writes data/results/timing/<pipeline>.json.
"""

import json
import sys
import time

from common import PDF, RESULTS, sample_ids


def pipelines():
    from chunk import drop_title, fixed_chunks, kakemono_json, pypdf_text, struct_chunks

    def pypdf():
        return (pypdf_text, fixed_chunks)

    def kakemono():
        return (kakemono_json, lambda doc: [drop_title(c) for c in struct_chunks(doc)])

    def docling():
        from tools_chunk import docling_chunker

        run = docling_chunker()
        conv, chunker = run.conv, run.chunker
        return (lambda pdf: conv.convert(str(pdf)).document, lambda doc: [chunker.contextualize(c) for c in chunker.chunk(dl_doc=doc)])

    def unstructured():
        from unstructured.chunking.title import chunk_by_title
        from unstructured.partition.pdf import partition_pdf

        from common import MAX_CHUNK_TOKENS, count_tokens

        def chunk(elements):
            chunks = [c.text for c in chunk_by_title(elements, max_characters=MAX_CHUNK_TOKENS * 4)]
            return [p for c in chunks for p in (fixed_chunks(c) if count_tokens(c) > MAX_CHUNK_TOKENS else [c])]

        return (lambda pdf: partition_pdf(filename=str(pdf), strategy="fast"), chunk)

    return {"pypdf-fixed": pypdf, "kakemono-struct-notitle": kakemono, "docling-hybrid": docling, "unstructured-title": unstructured}


def main():
    name = sys.argv[1]
    ids = sample_ids()[: int(sys.argv[2])] if len(sys.argv) > 2 else sample_ids()
    t = time.perf_counter()
    extract, chunk = pipelines()[name]()
    from common import tokenizer

    tokenizer()
    chunk(extract(PDF / f"{ids[0]}.pdf"))  # warm-up: some models load on first use
    load = time.perf_counter() - t
    rows = []
    for pid in ids:
        pdf = PDF / f"{pid}.pdf"
        t0 = time.perf_counter()
        try:
            doc = extract(pdf)
            t1 = time.perf_counter()
            n = len(chunk(doc))
        except Exception as e:  # noqa: BLE001 - a failed paper is timed and reported
            print(f"{pid}: FAILED {e}", file=sys.stderr)
            t1, n = time.perf_counter(), 0
        t2 = time.perf_counter()
        rows.append({"pid": pid, "extract_s": t1 - t0, "chunk_s": t2 - t1, "chunks": n})
        print(f"{name} {pid} {t2 - t0:.2f}s", file=sys.stderr, flush=True)
    out = RESULTS / "timing" / f"{name}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"load_s": load, "papers": rows}))
    ex, ch = sum(r["extract_s"] for r in rows), sum(r["chunk_s"] for r in rows)
    print(f"{name}: load {load:.1f}s, {len(rows)} papers, extract {ex:.1f}s + chunk {ch:.1f}s = {ex + ch:.1f}s")


if __name__ == "__main__":
    main()
