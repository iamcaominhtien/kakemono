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
    m = SentenceTransformer(hf, device="cpu", model_kwargs={"dtype": torch.bfloat16 if model == "qwen3" else torch.float32})
    m.max_seq_length = 640  # chunks are at most 512 tokens (+ the prompt)
    return lambda texts, query: m.encode([(qp if query else dp) + t for t in texts], batch_size=8, normalize_embeddings=True)


def main():
    model, pipelines = sys.argv[1], sys.argv[2:] or ["pypdf-fixed", "kakemono-fixed", "kakemono-struct"]
    papers, ids = qasper(), sample_ids()
    qs = questions(papers, ids)
    embed = embedder(model)
    cache = RESULTS / "emb" / model
    cache.mkdir(parents=True, exist_ok=True)
    qfile = cache / "questions.npy"
    if qfile.exists():
        qemb = np.load(qfile)
    else:
        qemb = embed([q["question"] for q in qs], True)
        np.save(qfile, qemb)
    summary = {}
    for pipeline in pipelines:
        rows = []
        for pid in ids:
            chunks = read_jsonl(CHUNKS / pipeline / f"{pid}.jsonl")
            efile = cache / pipeline / f"{pid}.npy"
            if efile.exists():
                emb = np.load(efile)
            else:
                emb = embed([c["text"] for c in chunks], False) if chunks else np.zeros((0, qemb.shape[1]))
                efile.parent.mkdir(parents=True, exist_ok=True)
                np.save(efile, emb)
            whole = ngrams(words(" ".join(c["text"] for c in chunks)))
            for qi, q in enumerate(qs):
                if q["pid"] != pid:
                    continue
                order = np.argsort(-(emb @ qemb[qi])) if len(chunks) else []
                row = {"qid": q["qid"], "pid": pid, "ceiling": coverage(q["evidences"], whole)}
                for budget in BUDGETS:
                    left, parts = budget, []
                    for j in order:
                        if left <= 0:
                            break
                        c = chunks[j]
                        parts.append(c["text"] if c["tokens"] <= left else cut_tokens(c["text"], left))
                        left -= c["tokens"]
                    row[f"recall@{budget}"] = coverage(q["evidences"], ngrams(words("\n".join(parts))))
                    row[f"top@{budget}"] = [int(j) for j in order[: len(parts)]]
                rows.append(row)
            print(f"{pipeline} {pid} done", file=sys.stderr, flush=True)
        (RESULTS / model).mkdir(parents=True, exist_ok=True)
        (RESULTS / model / f"{pipeline}.json").write_text(json.dumps(rows))
        keys = ["ceiling"] + [f"recall@{b}" for b in BUDGETS]
        summary[pipeline] = {k: round(float(np.mean([r[k] for r in rows])), 3) for k in keys}
    print(f"\n{len(qs)} questions, {len(ids)} papers, model {model}")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
