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

## Excel input (`src/excel`)

The workbooks named are those of `tests/data/excel-corpus.tsv`
(`scripts/fetch-excel-corpus.sh`). Found by reviewing every output of the
corpus against the workbooks' XML.

### Decisions still open

- [ ] Dates: show them as the cell's format shows them in the sheet view and
      the Excel-style PDF (keeping ISO in Markdown, JSON and chunks)? ISO
      dates overflow or are clipped in narrow cells whose format is short
      (`d`, `d-mmm`): ms-project-schedule (the day row), poi-excelpivottablesample.
- [ ] The cap of cells read per sheet, trimming empty columns, a benchmark of
      large workbooks.

### Regions and header rows (`regions.rs`, `build.rs`; compare with `scripts/excel-report.py`)

- [ ] Header rows taken wrongly:
      - ss-simple-gantt: the merged info block B3:H6 ("Project Title",
        "Project Manager"...) is the header; the real one (PHASE, WEEK n, Task)
        is body.
      - hellowork-kyujin-moushikomi: 11 of the 14 rows of the table at rows
        104-113 are header rows.
      - boe-inflation-attitudes: the notes in B1:B3 and the title block A1:A5
        are glued into the first column headers.
      - abs-cpi-640101, Data1: 9 header rows (Unit, Series Type, Series ID...)
        glued into one cell per column.
      - statjp-nenkan-age-sex: the unit rows ("（単位　1,000人）") are in the
        header cells; they are a note.
      - mhlw-kokumin-seikatsu-r6-06: a section row ("推計数（単位：千世帯）")
        in the column headers; a mid-table section row repeated in every
        column.
- [ ] Row labels parted from their numbers: boe-inflation-attitudes, about 16
      questions have their labels in A and their data from AM, so the labels
      become paragraphs and the numbers a table without labels or dates.
      citizens-financial-supplement, "Average Balance Sheets": column AD
      ("Proof") is cut off the table into one-number paragraphs.
- [ ] Two tables side by side read as one: statjp-nenkan-age-sex (ages 0-49 in
      A:G, 50-100+ in I:O).
- [ ] A form is still one wide table (hellowork-kyujin-moushikomi, 31 columns;
      ur-kinkyu-renraku-taisei): a list of fields and values would read
      better.
- [ ] ss-kpi-performance-dashboard, DASHBOARD DATA: four small tables in one
      12-column table under an empty header.

### Text outputs

- [ ] A chart without a title of its own is "Chart N"; its title is the cell
      above it (ss-kpi-performance-dashboard: PROJECTS / TASKS, MONTHLY
      REVENUE...), written apart as a paragraph. The chart type is not said.
- [ ] Line breaks inside a cell become spaces (digital-glossary bullet lists,
      citizens KPM text; statjp "All \nages" gives a double space).
- [ ] Bold of part of a cell: `**100**%` where the whole cell is bold; a
      closing `)` moved out of the bold.
- [ ] The data-validation list of ss-campaign-budget-roi (O8:O20) comes out as
      12 paragraphs after the table.
- [ ] Rich-text runs keep no size of their own (`sz`): a footnote mark set
      smaller in the cell is as large as its text.

### Document outputs

- [ ] Workbook PDF in document style: a table kept whole that does not fit
      under its heading leaves the heading alone on a page
      (ss-simple-gantt); the fix is in the shared `flow.rs` (keep chains),
      to check with `scripts/word-fidelity.py`.
- [ ] docx and EPUB: a table of 100 columns and more (boe-inflation-attitudes,
      109) still runs off the page or the screen; the PDF cuts it into groups
      of columns, they do not.

### Sheet view and Excel-style PDF

- [ ] Conditional formats on a formula of their own (`expression`) are not
      applied (github-wbs-template's Gantt bars, ms-project-schedule).
- [ ] Visible comments are not shown in the sheet view.
- [ ] The dark table styles (`TableStyleDark*`) and a workbook's own table
      styles (`tableStyles`) are not drawn.
- [ ] WordArt with a gradient or outline fill is drawn plain
      (poi-excelwithattachments); the icon in a Word object's preview
      metafile is missing.
- [ ] The `&N` code of a footer (number of pages) counts the sheet's pages;
      check whether Excel counts those of the whole print
      (worldbank-commodity-pinksheet).
- [ ] census-state-pop: a merged note crossing the page cut beside repeated
      title columns prints fragments (LibreOffice does too).
- [ ] digital-common-spec-gyomuflow: the gateway "X" (`mathMultiply`) looks
      bolder and larger than LibreOffice draws it; not checked against Excel.

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
