# TODO

Things known to be missing or wrong, to come back to. Each item names a paper
and what goes wrong, so it can be reproduced.

## What the editable JSON is for

The JSON (`-f json`, `--content`) is one content format for several uses; new
uses get new fields, not new formats.

| Use | Fields it uses | Still missing |
| --- | --- | --- |
| 1. Feeding language models (RAG, question answering) | `plain` (no tags, formulas as `$LaTeX$`), `section`, `parent`, reading order | No ready-made chunks for an index: users cut by `section` themselves |
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

## hard-95

- [ ] Edited blocks get uneven font sizes: each one shrinks on its own to fit
      its box, so neighbouring paragraphs can end up at different sizes.
