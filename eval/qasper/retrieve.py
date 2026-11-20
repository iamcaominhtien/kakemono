"""Embed every pipeline's chunks, retrieve for each question, measure evidence recall.

    python retrieve.py <model> [pipeline ...]

For each question, the chunks of its own paper are ranked by cosine similarity
and taken in rank order until a token budget is spent (the last one cut). The
score is the share of the gold evidence that the retrieved text contains,
measured as word trigrams, so that it does not depend on how a pipeline cut
its chunks. Qasper has several annotators per question: the best one counts.
Evidence that only points at a figure or a table ("FLOAT SELECTED") is left
out, as in Qasper's --text_evidence_only.

The ceiling column is the same measure against the whole text of the paper:
what a pipeline could reach at best, i.e. how much of the text it extracted.
"""

import json
import sys

import numpy as np

from common import CHUNKS, RESULTS, cut_tokens, ngrams, qasper, read_jsonl, sample_ids, words

BUDGETS = [512, 1024, 2048]

MODELS = {
    # name: (Hugging Face id, query prompt, document prompt)
    "qwen3": (
        "Qwen/Qwen3-Embedding-0.6B",
        "Instruct: Given a question about a research paper, retrieve passages of the paper that answer it\nQuery: ",
        "",
    ),
    "bge-base": ("BAAI/bge-base-en-v1.5", "Represent this sentence for searching relevant passages: ", ""),
    "nomic": ("nomic-ai/nomic-embed-text-v1.5", "search_query: ", "search_document: "),
}

# nomic's model code is not in transformers: it is downloaded from the Hub and
# run (trust_remote_code). Both its repositories are pinned to the commits
# whose code was read, so that what runs cannot change.
REMOTE_CODE = {
    "nomic": {
        "trust_remote_code": True,
        "revision": "e9b6763023c676ca8431644204f50c2b100d9aab",
        "config_kwargs": {"code_revision": "7710840340a098cfb869c4f65e87cf2b1b70caca"},
        "model_kwargs": {"code_revision": "7710840340a098cfb869c4f65e87cf2b1b70caca"},
    },
}


def questions(papers: dict, ids: list[str]) -> list[dict]:
    out = []
    for pid in ids:
        for q in papers[pid]["qas"]:
            evidences = []
            for a in q["answers"]:
                ev = [e for e in a["answer"]["evidence"] if e and not e.startswith("FLOAT SELECTED")]
                if ev and not a["answer"]["unanswerable"]:
                    evidences.append(ev)
            if evidences:
                out.append({"pid": pid, "qid": q["question_id"], "question": q["question"], "evidences": evidences})
    return out


def coverage(evidences: list[list[str]], text_grams: set) -> float:
    best = 0.0
    for ev in evidences:
        grams = ngrams(words(" ".join(ev)))
        if grams:
            best = max(best, len(grams & text_grams) / len(grams))
    return best


def embedder(model: str):
    import torch
    from sentence_transformers import SentenceTransformer

    hf, qp, dp = MODELS[model]
    extra = dict(REMOTE_CODE.get(model, {}))
    model_kwargs = {"dtype": torch.bfloat16 if model == "qwen3" else torch.float32, **extra.pop("model_kwargs", {})}
    m = SentenceTransformer(hf, device="cpu", model_kwargs=model_kwargs, **extra)
    m.max_seq_length = 640  # chunks are at most 512 tokens (+ the prompt)
    return lambda texts, query: m.encode([(qp if query else dp) + t for t in texts], batch_size=8, normalize_embeddings=True)


def cached(file, compute, *args):
    """The array in `file`, computed by `compute(*args)` and saved there the first time."""
    if file.exists():
        return np.load(file)
    arr = compute(*args)
    file.parent.mkdir(parents=True, exist_ok=True)
    np.save(file, arr)
    return arr


def embed_chunks(embed, chunks: list[dict], dim: int):
    """The embeddings of the chunks of one paper (none for a paper without text)."""
    return embed([c["text"] for c in chunks], False) if chunks else np.zeros((0, dim))


def retrieved(chunks: list[dict], order, budget: int) -> list[str]:
    """The texts of the chunks in rank order until `budget` tokens, the last one cut."""
    left, parts = budget, []
    for j in order:
        if left <= 0:
            break
        c = chunks[j]
        parts.append(c["text"] if c["tokens"] <= left else cut_tokens(c["text"], left))
        left -= c["tokens"]
    return parts


def score(q: dict, pid: str, chunks: list[dict], emb, qvec, whole: set) -> dict:
    """The row of one question: its ceiling, and recall and ranks at each budget."""
    order = np.argsort(-(emb @ qvec)) if len(chunks) else []
    row = {"qid": q["qid"], "pid": pid, "ceiling": coverage(q["evidences"], whole)}
    for budget in BUDGETS:
        parts = retrieved(chunks, order, budget)
        row[f"recall@{budget}"] = coverage(q["evidences"], ngrams(words("\n".join(parts))))
        row[f"top@{budget}"] = [int(j) for j in order[: len(parts)]]
    return row


def run_pipeline(pipeline: str, ids: list[str], qs: list[dict], qemb, embed, cache) -> list[dict]:
    """The rows of every question for one pipeline, embedding its chunks when not cached."""
    rows = []
    for pid in ids:
        chunks = read_jsonl(CHUNKS / pipeline / f"{pid}.jsonl")
        emb = cached(cache / pipeline / f"{pid}.npy", embed_chunks, embed, chunks, qemb.shape[1])
        whole = ngrams(words(" ".join(c["text"] for c in chunks)))
        rows += [score(q, pid, chunks, emb, qemb[qi], whole) for qi, q in enumerate(qs) if q["pid"] == pid]
        print(f"{pipeline} {pid} done", file=sys.stderr, flush=True)
    return rows


def main():
    model, pipelines = sys.argv[1], sys.argv[2:] or ["pypdf-fixed", "kakemono-fixed", "kakemono-struct"]
    papers, ids = qasper(), sample_ids()
    qs = questions(papers, ids)
    embed = embedder(model)
    cache = RESULTS / "emb" / model
    cache.mkdir(parents=True, exist_ok=True)
    qemb = cached(cache / "questions.npy", embed, [q["question"] for q in qs], True)
    summary = {}
    for pipeline in pipelines:
        rows = run_pipeline(pipeline, ids, qs, qemb, embed, cache)
        (RESULTS / model).mkdir(parents=True, exist_ok=True)
        (RESULTS / model / f"{pipeline}.json").write_text(json.dumps(rows))
        keys = ["ceiling"] + [f"recall@{b}" for b in BUDGETS]
        summary[pipeline] = {k: round(float(np.mean([r[k] for r in rows])), 3) for k in keys}
    print(f"\n{len(qs)} questions, {len(ids)} papers, model {model}")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
