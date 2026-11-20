# TODO

Things known to be missing or wrong, to come back to. Each item names a paper
and what goes wrong, so it can be reproduced.

## What the editable JSON is for

The JSON (`-f json`, `--content`) is one content format for several uses; new
uses get new fields, not new formats.

| Use | Fields it uses | Still missing |
| --- | --- | --- |
| 1. Feeding language models (RAG, question answering) | `plain` (no tags, formulas as `$LaTeX$`), `section`, `parent`, reading order; ready-made chunks with `-f chunks` | Chunks are measured on PDFs only: see "Chunks for retrieval" below |
| 2. Editing and translating, then rebuilding any output | `text`, `rows`, table cells, `--content` | Nothing in kakemono. A better translation workflow (glossary, context per section, review pass, bilingual field) lives outside it: see the translation kit |
| 3. Data extraction across many papers | `meta` (authors with affiliations and e-mails, date, DOI, arXiv category, keywords, abstract), `citation` on references, tables by cell | Accuracy depends on the style; see "Citations and front matter" below |
| 4. Building a reader of one's own | `layout`, `image`, `mathml`, `anchor`, links | Never tried by a real reader |

## Analysis

- [ ] BERT (1810.04805): the introduction of the appendix (a paragraph and a
      bullet list) is read as references.
- [ ] GR notes (gr-qc/9712019): the paragraph and the bullet list that
      introduce the book list are read as references.
- [ ] DL Theory (2106.10165): some references are cut in two ("pp. 318–362.
      MIT Press, Cambridge, MA, 1986." is a block of its own).
- [ ] Faster R-CNN (1506.01497): affiliations are footnotes without marks, so
      no author gets them.
- [ ] Letters split at extraction: "Ł ukasz" (2001.08361), "Castand er"
      (1905.06454).
- [ ] LIGO (1602.03837): the references have no "References" heading, so
      they stay in the last section, "ACKNOWLEDGMENTS" (their chunks are
      headed "ACKNOWLEDGMENTS" too).

## Citations and front matter (`src/render/json/citation.rs`, `front.rs`)

- [ ] A joint affiliation stays one string: "Johns Hopkins University, OpenAI"
      (2001.08361).
- [ ] No year from "(Princeton, 1996-97)" (math/0211159) or "2022/ed"
      (2303.08774 [51]).
- [ ] The publisher of a book is not its `venue`: "C. Nash and S. Sen,
      *Topology and Geometry for Physicists* (Academic Press, 1983)".
- [ ] LIGO [117]: "(LIGO Scientific Collaboration and Virgo Collaboration)"
      after the authors spoils the venue ("Relativity").

## Editable JSON

- [ ] A JSON from an older analysis is refused as soon as a block changes type
      (LIGO's references were paragraphs before v0.11). Accept it when both
      types hold plain text (paragraph, reference, list item, note), with a
      warning. Until then, the translation kit's `remap.py` carries a
      translation over by matching the original text.

## Word input (`src/word`)

- [ ] Text boxes come after the paragraph they are anchored in, in `.docx`
      and `.doc`, not where they stand on the page.

## Chunks for retrieval (`src/render/chunks`)

Everything below was measured on PDFs only (Qasper: 100 arXiv NLP papers,
`eval/qasper/`). Word input has been tried on `tests/data/word/sample.docx`
alone, which gives 5 chunks along its sections.

- [ ] Chunk the Word corpus (`scripts/fetch-word-corpus.sh`: 385 `.doc` and
      139 `.docx`, each with the PDF Word made of it) and compare the chunks
      of each document with those of its PDF: same sections, same text, no
      crash. Look for documents whose headings are bold paragraphs rather
      than heading styles, whose chunks then have no `section`.
- [ ] `pages` is always `[0]` for Word input, which has no pages: leave it
      out, or give the pages of the PDF that `Paper::to_pdf` lays out.
- [ ] The token estimate was fitted on English: on a Vietnamese sentence it
      counts 109 tokens where Qwen3 counts 36 and BERT 54, so chunks of a
      Vietnamese (or other non-English) document come out 2 to 3 times
      smaller than `--chunk-tokens`. Fit it on other languages too, or count
      a letter outside ASCII as part of its word (`NON_ASCII_TOKENS_PER_CHAR`).
- [ ] No retrieval benchmark for Word documents yet: Qasper has PDFs only.
- [ ] End-to-end answers (`eval/qasper/answer.py`: Haiku answers from the
      retrieved chunks, then judges them) have not been run: they need an
      `ANTHROPIC_API_KEY`.

## hard-95

- [ ] Edited blocks get uneven font sizes: each one shrinks on its own to fit
      its box, so neighbouring paragraphs can end up at different sizes.
