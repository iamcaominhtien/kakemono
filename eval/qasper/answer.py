"""Answer each question with Claude Haiku from the retrieved chunks, then score the answers.

    python answer.py <model> <budget> [pipeline ...]

<model> and <budget> pick the retrieval of retrieve.py (results/<model>/<pipeline>.json,
field top@<budget>). Two scores per answer:
  answer_f1  Qasper's official token F1 against the best reference (no model involved)
  judge      Haiku's verdict against the references: 1 correct, 0.5 partly, 0 wrong
Answers and verdicts are cached in results/<model>/answers-<budget>-<pipeline>.jsonl,
so a run that stops can be started again.
"""

import json
import re
import string
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor

import anthropic

from common import CHUNKS, RESULTS, cut_tokens, qasper, read_jsonl

LLM = "claude-haiku-5-5"
WORKERS = 8

ANSWER_SYSTEM = (
    "You answer questions about a research paper using only the excerpts given. "
    "Answer in as few words as possible: a short phrase or a list, not a sentence. "
    "For a yes/no question, answer only Yes or No. "
    "If the excerpts do not contain the answer, answer only Unanswerable."
)

JUDGE_SYSTEM = (
    "You grade an answer to a question about a research paper against reference answers "
    "written by annotators. Reply with exactly one word: CORRECT if it gives the same "
    "information as one reference, PARTIAL if it gives part of it, WRONG otherwise."
)


# ------------------------------------------------ Qasper's official answer F1


def _normalize(s: str) -> str:
    s = s.lower()
    s = "".join(ch for ch in s if ch not in set(string.punctuation))
    s = re.sub(r"\b(a|an|the)\b", " ", s)
    return " ".join(s.split())


def token_f1(pred: str, gold: str) -> float:
    p, g = _normalize(pred).split(), _normalize(gold).split()
    same = sum((Counter(p) & Counter(g)).values())
    if same == 0:
        return 0.0
    precision, recall = same / len(p), same / len(g)
    return 2 * precision * recall / (precision + recall)


def references(q: dict) -> list[str]:
    out = []
    for a in q["answers"]:
        x = a["answer"]
        if x["unanswerable"]:
            out.append("Unanswerable")
        elif x["extractive_spans"]:
            out.append(", ".join(x["extractive_spans"]))
        elif x["free_form_answer"]:
            out.append(x["free_form_answer"])
        else:
            out.append("Yes" if x["yes_no"] else "No")
    return out


# ------------------------------------------------ calls


client = anthropic.Anthropic(max_retries=6)


def ask(system: str, user: str, max_tokens: int) -> str:
    r = client.messages.create(
        model=LLM,
        max_tokens=max_tokens,
        system=system,
        output_config={"effort": "low"},
        messages=[{"role": "user", "content": user}],
    )
    if r.stop_reason == "refusal":
        return ""
    return "".join(b.text for b in r.content if b.type == "text").strip()


def solve(item: dict) -> dict:
    context = "\n\n---\n\n".join(item["excerpts"])
    answer = ask(ANSWER_SYSTEM, f"Excerpts:\n\n{context}\n\nQuestion: {item['question']}", 2000)
    refs = "\n".join(f"- {r}" for r in item["refs"])
    verdict = ask(JUDGE_SYSTEM, f"Question: {item['question']}\n\nReferences:\n{refs}\n\nAnswer: {answer}", 500)
    v = verdict.upper()
    judge = 1.0 if v.startswith("CORRECT") else 0.5 if v.startswith("PARTIAL") else 0.0
    f1 = max(token_f1(answer, r) for r in item["refs"])
    return {"qid": item["qid"], "answer": answer, "verdict": verdict, "judge": judge, "answer_f1": f1}


def main():
    model, budget, pipelines = sys.argv[1], int(sys.argv[2]), sys.argv[3:] or ["pypdf-fixed", "kakemono-fixed", "kakemono-struct"]
    by_qid = {q["question_id"]: q for p in qasper().values() for q in p["qas"]}
    summary = {}
    for pipeline in pipelines:
        rows = json.loads((RESULTS / model / f"{pipeline}.json").read_text())
        out = RESULTS / model / f"answers-{budget}-{pipeline}.jsonl"
        done = {r["qid"]: r for r in read_jsonl(out)} if out.exists() else {}
        todo = []
        for r in rows:
            if r["qid"] in done:
                continue
            chunks = read_jsonl(CHUNKS / pipeline / f"{r['pid']}.jsonl")
            left, excerpts = budget, []
            for j in r[f"top@{budget}"]:
                c = chunks[j]
                excerpts.append(c["text"] if c["tokens"] <= left else cut_tokens(c["text"], left))
                left -= c["tokens"]
            q = by_qid[r["qid"]]
            todo.append({"qid": r["qid"], "question": q["question"], "excerpts": excerpts, "refs": references(q)})
        with ThreadPoolExecutor(WORKERS) as pool, out.open("a") as f:
            for res in pool.map(solve, todo):
                done[res["qid"]] = res
                f.write(json.dumps(res, ensure_ascii=False) + "\n")
                f.flush()
        scored = [done[r["qid"]] for r in rows]
        summary[pipeline] = {
            "answer_f1": round(sum(x["answer_f1"] for x in scored) / len(scored), 3),
            "judge": round(sum(x["judge"] for x in scored) / len(scored), 3),
            "n": len(scored),
        }
        print(pipeline, summary[pipeline], flush=True)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
