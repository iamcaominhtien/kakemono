<div align="center">

<img src="docs/assets/logo.svg" alt="kakemono logo" width="120" height="120">

# kakemono

**Turn PDF papers and books — and Word documents and Excel workbooks — into clean, readable HTML, Word documents, Markdown, LaTeX or EPUB books, in pure Rust, as a single executable.**

[![Release](https://img.shields.io/github/v/release/iamcaominhtien/kakemono?label=release&color=d9822b)](https://github.com/iamcaominhtien/kakemono/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/iamcaominhtien/kakemono/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white)](https://github.com/iamcaominhtien/kakemono/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/kakemono?logo=rust&logoColor=white&color=000000)](https://crates.io/crates/kakemono)
[![PyPI](https://img.shields.io/pypi/v/kakemono?logo=python&logoColor=white&color=3776ab)](https://pypi.org/project/kakemono/)
[![docs.rs](https://img.shields.io/docsrs/kakemono?logo=docsdotrs&label=docs.rs)](https://docs.rs/kakemono)
[![PDF engine](https://img.shields.io/badge/PDF_engine-hayro-orange)](https://github.com/LaurenzV/hayro)
[![Platforms](https://img.shields.io/badge/platforms-Linux_%7C_macOS_%7C_Windows-555)](https://github.com/iamcaominhtien/kakemono/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-2f9e44)](LICENSE)

[Features](#features) · [Modes](#the-four-modes) · [Word](#word-documents) · [Excel](#excel-workbooks-as-input) · [Markdown](#markdown) · [LaTeX](#latex) · [EPUB](#epub) · [JSON](#editable-json) · [Chunks](#chunks-for-retrieval-rag) · [Install](#installation) · [Usage](#usage) ·
[Library](#use-as-a-library) · [Python](#python) · [Architecture](ARCHITECTURE.md) · [Changelog](CHANGELOG.md)

</div>

---

> *Kakemono* is a Japanese hanging scroll: a work unrolled so that it can be read at a
> glance. kakemono does the same for documents — it unrolls a fixed PDF page into text that flows.

Give it a local PDF, a Word document (`.docx`, `.doc`), an Excel workbook (`.xlsx`) or an arXiv ID; get HTML with continuous text, ready to read on any screen or
to feed to translation tools, a Word document to edit, Markdown for notes, wikis and LLMs, a
LaTeX source to edit and typeset again, an EPUB book for an e-reader, JSON for other
programs, which can be edited (translated, say) and turned back into any of these, or chunks
ready to embed for retrieval (RAG).

```bash
kakemono convert 1706.03762            # "Attention Is All You Need" → 1706.03762.friendly.html
kakemono convert 1706.03762 -o attention.docx   # the same paper as a Word document
kakemono convert 1706.03762 -o attention.md     # Markdown, pictures in attention_assets/
kakemono convert 1706.03762 -o attention.tex    # LaTeX, pictures in attention_assets/
kakemono convert 1706.03762 -o attention.epub   # an EPUB book for e-readers
kakemono convert 1706.03762 -o attention.json   # the content as editable JSON
kakemono convert 1706.03762 --content attention.vi.json -m hard-95   # the pages, with the edited text
kakemono convert 1706.03762 -o attention.jsonl  # chunks for retrieval (RAG), one per line
kakemono convert report.docx -o report.md       # a Word document (.docx or .doc) as input
kakemono convert report.docx -o report.pdf      # a Word document as a PDF, laid out as Word lays it out
kakemono convert sales.xlsx -o sales.md         # an Excel workbook (.xlsx) as input: one table per region of each sheet
```

## Features

| | |
| --- | --- |
| 🦀 **Pure Rust** | One executable: no `poppler`, `mupdf` or Python. PDF reading and rendering use [hayro](https://github.com/LaurenzV/hayro). |
| 📖 **Continuous text** | Lines joined into paragraphs, end-of-line hyphens removed, paragraphs joined across columns and pages, running heads and page numbers removed, footnotes set apart. |
| 🧱 **Real structure** | Tables rebuilt as `<table>`, formulas (displayed and inside the text) as MathML, code listings kept line by line, lists, headings and cross-links. |
| 🖼️ **Sharp images** | Figures stay vector (SVG) or render at 216 dpi; photographs are stored as JPEG. |
| 📝 **Word documents** | `.docx` with Word styles, Word tables and editable Word equations, from the same analysis. |
| 📊 **Excel workbooks** | `.xlsx` read as a document: a section per sheet, tables with their header rows, values as Excel shows them; to Markdown, HTML, Word, EPUB, LaTeX, JSON or chunks. |
| ✍️ **Markdown** | `.md` with pipe tables, LaTeX formulas (`$$`) and the pictures in a folder of their own. |
| 📐 **LaTeX** | `.tex` that compiles with pdfLaTeX, XeLaTeX or LuaLaTeX: sections, `tabular` tables, LaTeX math. |
| 📚 **EPUB** | `.epub` (EPUB 3) for e-readers: reflowing text, a table of contents, MathML formulas, tables and the first page as the cover. |
| 🌐 **Editable JSON** | `.json` with the blocks in reading order, their text with styles and formulas kept; edit it (translate it, say) and `--content` writes any output from it, the `hard-95` pages included. |
| 🔎 **Chunks for RAG** | `.jsonl` with the text cut along the sections, each chunk headed by its section path, sized in tokens (estimated, or by your embedding model's tokenizer), with the blocks and pages it comes from. |
| ⚡ **Fast** | A 15-page paper takes 1–2 s for all four modes; a 400-page book a few seconds per mode. |

## The four modes

| Mode | Layout | Text | Use when |
| --- | --- | --- | --- |
| `hard` | Each page redrawn as SVG/PNG | Hidden layer: selectable, searchable | You need an exact copy of the PDF |
| `hard-95` | Same pages, positions and line breaks | **Visible HTML text**; tables are `<table>` | You want the original look but must translate or copy |
| `medium` | Pages, columns and figure positions kept | HTML that reflows | You want the "paper feel" with flowing text |
| `friendly` | One responsive column: Paper/White/Dark themes, contents, phone layout, a page-turning book view | HTML optimised for reading | Reading on the web or a phone, translation |

In every mode a paragraph stays one element in the DOM (`<p>` in `medium`/`friendly`, one `div`
with `data-block` in `hard`/`hard-95`), so translation tools see whole paragraphs. In `hard-95`, a
block whose text changes (after translation) reflows inside its box.

## Word documents as input

A `.docx` or `.doc` file is an input like a PDF (`kakemono convert report.docx -o report.md`): the
format is recognised from the file's bytes. A Word document says what each paragraph is, so it is
read directly, without the layout analysis a PDF needs, and turned into the same structure:

- paragraph styles give the title, authors, abstract, headings (`Heading 1`–`9`, or any style with
  an outline level), captions, footnotes written as paragraphs, references (`Bibliography`, or the
  paragraphs after a "References" heading), code (a code style, or text all in a monospaced font)
  and table of contents entries; list numbering gives list items with their markers ("1.", "(a)");
- Word equations (Office Math) keep their structure: MathML, LaTeX and Office Math in the outputs,
  in running text too;
- tables become rebuilt tables (merged cells kept), pictures (PNG, JPEG, GIF, BMP, SVG) figures,
  with the caption before or after them; footnotes and endnotes follow their paragraph;
- hyperlinks, cross-references to bookmarks, citations and references to figures and tables become
  links, as for a PDF;
- pictures stored as Windows metafiles (EMF, WMF: charts, older drawings) are drawn by kakemono
  as SVG; they and SVG pictures (the SVG Word keeps beside a PNG too) stay vector in every output:
  an `<img>` of the SVG in HTML, an `.svg` file in Markdown, JSON and EPUB, a PDF picture in LaTeX,
  an SVG with a PNG beside it in Word, vector drawing in the PDF. An SVG is written again by
  kakemono first, with only its drawing (no script, link or style sheet);
- charts of `.docx` files are drawn by kakemono as SVG (vector in the PDF too), each followed by a
  table of its data; an Office 2016 chart (waterfall, treemap, ...) shows the picture Word keeps
  beside it, else its data; the charts of `.doc` files are left out;
- formulas made with the Equation Editor 3.0 or MathType (MTEF 3 and 5, in `.doc` and `.docx`
  files) become real formulas, floating shapes and text boxes of `.doc` files are read,
  and nested lists keep their level.

**Pages as Word lays them out.** kakemono also lays a Word document out in pages itself, the way
Word does: fonts with Word's metrics (a table of metrics for the usual Office fonts is built in),
line breaking, line spacing, widow and orphan control, keep-with-next, tables (row heights, merged
cells, borders, shading, repeated header rows), headers and footers, footnotes, sections and
columns, frames, floating pictures, shapes and text boxes. No Word, LibreOffice or other program is
used. That layout gives:

- `-o report.pdf` (or `-f pdf`): a PDF of the document, with selectable text and links;
- `hard` and `hard-95`: its pages, as for a PDF;
- `medium`: Word's formatting (fonts, colours, indents, spacing, tables, pictures) in one
  continuous page, without page breaks;
- `friendly` stays the single-column article, from the document's structure.

How close it is to Word is measured in CI on 524 documents with the PDF Word made of them (government
reports, forms, letters, guidance, templates; `.docx` and `.doc`; see
[Testing](#testing)): about 96% of lines break at the same words as in Word, and 89% of documents
have the same number of pages. Text edited with `--content` reaches every output except the page
layout (PDF, `hard`, `hard-95`, `medium`). `.doc` files (Word 97–2003) are read by kakemono
itself; Word 6/95 files, RTF files and password-protected files are not. Comments, tracked
changes (read as accepted), macros and form fields are not shown. See
[docs/WORD-INPUT.md](docs/WORD-INPUT.md) for the design.

## Excel workbooks as input

A `.xlsx` file is an input like a PDF or a Word document (`kakemono convert sales.xlsx -o sales.md`);
the format is recognised from the file's bytes. The cells say what the content is, so the workbook
is read directly into the same structure, to read it comfortably outside Excel's grid or to feed
it to an AI:

- each visible sheet is a section headed by its name; hidden sheets, rows and columns are left out;
- the cells of a sheet are cut into tables where empty rows or columns separate them; a title or
  unit line above a table is its caption, single-cell rows below it are notes;
- the header rows of a table come from `--header-rows N` if given, else from what the file declares
  (Excel Table, auto-filter, frozen rows, print titles), else from the types of the columns (the
  header is the rows above the first one whose cells have the type of their column), else the
  first row;
- values are shown as Excel shows them: percentages, thousands separators, decimals; dates and
  times always in ISO form (`2024-01-31`, `14:05`); furigana never reaches the text;
- the text of shapes and text boxes is read; forms drawn on a grid keep their text, not their look;
- pictures become figures (also those of groups, and those placed in cells, after their table);
  charts are drawn by kakemono as SVG, sharp at any zoom (bar, column, line, area, pie, doughnut,
  scatter, radar, and bar and line combined; the colours and data labels of the file), each
  followed by a table of its data, which is what the chunks and the JSON give;
- in the chunks (`.jsonl`), a table is cut between rows and every chunk repeats its header rows.

```bash
kakemono convert sales.xlsx -o sales.md                  # Markdown, a pipe table per region
kakemono convert sales.xlsx -o sales.jsonl               # chunks for retrieval (RAG)
kakemono convert sales.xlsx --header-rows 2 -o sales.md  # two header rows in every table (0: none)
```

A workbook has no pages: `hard`, `hard-95` and `medium` give the friendly page, as for a Word
document, and `-o sales.pdf` is an error. Details: [docs/EXCEL-INPUT.md](docs/EXCEL-INPUT.md).

## Word documents

`kakemono convert paper.pdf -o paper.docx` (or `--format docx`) writes a Word document instead of
HTML. It is built from the same analysis as the HTML modes, as one flowing column:

- the title, authors, abstract, headings (`Heading 1`–`4`, so Word's navigation pane and table of
  contents work), lists, footnotes, code and references get Word styles;
- rebuilt tables become Word tables, with merged cells and the paper's rules;
- formulas whose structure is clear become Word equations (Office Math, editable in Word), the
  others pictures; numbered formulas keep their number at the right margin; formulas inside the
  text (`fₜ(θₜ)`, `β₁ = 0.9`) are equations too;
- figures are pictures of their area of the page, with their captions; citations and references to
  figures, tables and sections are links within the document.

The page is US Letter with one-inch margins. `--scale` and `--equations` apply. With
`--images files` the pictures stay in the document and are also written to a folder
(`<name>_assets/`, or `--image-dir`). `--mode`, `--all`, `--page-image` and `--fonts` are for HTML
only.

## Markdown

`kakemono convert paper.pdf -o paper.md` (or `--format md`) writes Markdown, from the same
analysis, readable on GitHub, in Obsidian, Jupyter, Pandoc, or by an LLM:

- the title (`#`), headings (`##`–`#####`), lists, footnotes, references and code listings (fenced
  blocks) get their Markdown syntax; bold, italic, code, links and `<sup>`/`<sub>` are kept;
- rebuilt tables become pipe tables. With `-m friendly` (the default) every table is adapted to
  one, its meaning kept rather than its layout: the header rows become one (`BLEU: EN-DE`), and a
  cell spanning rows, or columns under one group header, is repeated in each, so that every row
  reads on its own. With `-m medium` the layout is kept: a table whose cells span rows or columns
  stays an HTML table;
- formulas whose structure is clear become LaTeX between `$$` (with `\tag{n}` for their number),
  the others pictures; formulas inside the text become `$…$`;
- figures are pictures, written to `<name>_assets/` next to the file (`--image-dir images` for
  `images/`); `--images inline` embeds them as data URIs instead, for a single file;
- citations and references to figures, tables and sections are links within the document.

`--scale` and `--equations` apply.

## LaTeX

`kakemono convert paper.pdf -o paper.tex` (or `--format tex`) writes a LaTeX document, from the same
analysis, that compiles as it is with pdfLaTeX, XeLaTeX or LuaLaTeX (packages of every TeX
distribution: `amsmath`, `amssymb`, `mathtools`, `graphicx`, `booktabs`, `multirow`, `hyperref`):

- the title, authors and abstract are set as on a title page; headings become `\section*`,
  `\subsection*`, … (their numbers are part of their text); lists keep the markers of the paper;
- rebuilt tables become `tabular`s with `booktabs` rules, `\multicolumn` and `\multirow`, shrunk
  to the width of the text when wider;
- formulas whose structure is clear become LaTeX math (`equation*` with `\tag{n}` for their
  number), the others pictures; formulas inside the text become `$…$`;
- figures are pictures (`\includegraphics`), always written to `<name>_assets/` next to the file
  (`--image-dir` to choose it): LaTeX cannot embed them, so `--images inline` is refused;
- figures and tables stay where they were in the text (they are not floats) and keep the label of
  their caption ("Figure 3:"); citations and references to figures, tables and sections are
  links (`\hyperref`), web addresses `\href`.

A character that the fonts of pdfLaTeX lack (Chinese, say) is printed as **?** by pdfLaTeX;
XeLaTeX and LuaLaTeX print it when their font has it. Vietnamese letters (ơ, ư, ặ, ữ, ...)
are set in the T5 encoding of the `vntex` package by pdfLaTeX, the rest of the text staying in
T1. `--scale` and `--equations` apply.

## EPUB

`kakemono convert paper.pdf -o paper.epub` (or `--format epub`) writes an EPUB 3 book, from the
same analysis and with the same markup as the friendly mode, for Apple Books, Kobo, Thorium,
calibre and other e-readers (and for Kindle through Send to Kindle):

- one chapter per top-level section, after a title page with the title, authors and abstract; a
  table of contents with every heading (and an NCX for EPUB 2 readers);
- formulas whose structure is clear become MathML (the others pictures), rebuilt tables become
  tables, code listings stay line by line; footnotes stay after their paragraph;
- figures are pictures stored in the book; the first page, drawn as a picture, is the cover that
  readers show in their library;
- the style sheet sizes everything relative to the text, so the reader's font, size and theme
  apply; citations and references to figures, tables and sections are links across chapters.

Books convert too, novels included: a drop cap is read as the first letter of its line, running
heads and feet are removed even when set far from the page edge, and each chapter is a content
document of its own.

The title, the authors, the arXiv page and the date of conversion are recorded in the book.
`--scale` and `--equations` apply; `--images files` does not (the book holds its pictures).
A displayed formula wider than a phone screen (20 times the text size) is a picture rather than
MathML: a MathML formula cannot break over lines, and most readers do not scroll sideways, so
it would be cut at the edge of the screen; a picture shrinks to the page. Readers that do not
show MathML show the text of each formula instead: `--equations image` makes every displayed
formula a picture.

## Editable JSON

`kakemono convert paper.pdf -o paper.json` (or `--format json`) writes the content of the document
for other programs: search, language models, data extraction, or your own reader. The blocks come
in reading order; formulas whose structure is clear also come as LaTeX and MathML, figures and the
other formulas as pictures in `paper_assets/`:

```json
{
  "format": "kakemono-content", "version": 1, "generator": "kakemono 0.10.0",
  "source": { "format": "pdf", "name": "1602.03837v1.pdf", "sha256": "e5e864c2…" },
  "meta": { "title": "Observation of Gravitational Waves from a Binary Black Hole Merger" },
  "pages": [[612.0, 792.0], …],
  "blocks": [
    { "id": 7, "type": "heading", "level": 1, "anchor": "sec-I", "text": "I. INTRODUCTION",
      "layout": { "page": 0, "bbox": [127.67, 308.36, 221.39, 318.86] } },
    { "id": 5, "type": "paragraph", "abstract": true, "section": 4,
      "text": "… a peak gravitational-wave strain of 1.0 × <m id=\"1\">10^{-21}</m>. It matches …" },
    { "id": 20, "type": "equation", "latex": "…", "mathml": "<math …>", "image": "paper_assets/002-eq.png" }
  ]
}
```

- `type` is `title`, `authors`, `note`, `heading` (with its `level` and the `parent` heading),
  `paragraph` (`abstract` when it is part of the abstract), `list_item`, `list_continuation`,
  `equation`, `figure` (with its `float` kind, `label`, `image`, the `figure_text` found inside it
  and a rebuilt `table`), `footnote`, `reference`, `code` (its `lines`) or `contents_entry`.
  `section` is the id of the heading a block is under; `layout` is its page (from 0) and box in
  points.
- `text` is one string, so that a paragraph can be edited (or translated) as a whole: `<b>`, `<i>`,
  `<code>`, `<sup>`, `<sub>`, `<sf>` (sans-serif), `<sc>` (small capitals), `<mf>` (math font),
  `<a href>` and `<font color>` keep the styles and links; `<m id="n">…</m>` stands for the n-th
  formula of the block, with its LaTeX inside.
- `plain` is the same text without tags, formulas as `$LaTeX$` (`$$…$$` for a displayed formula),
  for search indexes and language models; table cells have one too.

**Bibliographic fields.** For data extraction the file also has what could be read with
confidence from the front matter and the reference list (a field that could not be read is left
out):

```json
"meta": {
  "title": "Attention Is All You Need", "arxiv_id": "1706.03762v7",
  "arxiv_category": "cs.CL", "date": "2023-08-02", "doi": "…", "keywords": ["…"],
  "authors": [{ "name": "Ashish Vaswani", "affiliations": [0], "emails": ["avaswani@google.com"], "notes": [0] }, …],
  "affiliations": ["Google Brain", "Google Research", "University of Toronto"],
  "author_notes": ["Equal contribution. Listing order is random. …", …],
  "abstract": "The dominant sequence transduction models are …"
}
```

```json
{ "id": 160, "type": "reference", "label": "32", "text": "[32] Noam Shazeer, … <i>arXiv preprint </i>…",
  "citation": { "authors": ["Noam Shazeer", "Azalia Mirhoseini", …, "Jeff Dean"],
                "title": "Outrageously large neural networks: The sparsely-gated mixture-of-experts layer",
                "venue": "arXiv", "year": 2017, "arxiv": "1701.06538" } }
```

- `authors` come from the author block: names (told apart by commas, "and", or the gaps between
  names printed side by side), the affiliations below them or pointed to by a raised mark
  (`Jane Doe¹`), the e-mail addresses (`{a, b}@example.com` gives two) and notes (`∗` Equal
  contribution). `date` is that of the arXiv version (from its stamp), else the date printed with
  the authors.
- `citation` gives `authors` (given names first, `et_al` when the list is cut), `title`, `venue`,
  `year`, `volume`, `pages`, `doi`, `arxiv` and `url`. It knows the usual styles (IEEE with a quoted
  title, ACL/APA with the year after the authors, the astronomy and physics styles without titles,
  books in italics); on the papers of the test suite most references give authors, title and year.
  A reference with no year written gets the year of its arXiv identifier.
- These fields, like `plain`, are read-only: they are derived from the text and not read back.

**Applying an edited file.** `--content` writes any output with the text of an edited JSON file:

```bash
kakemono convert paper.pdf -o paper.json
# edit paper.json (translate it, ...) into paper.vi.json
kakemono convert paper.pdf --content paper.vi.json -m hard-95 -o paper.vi.html
kakemono convert paper.pdf --content paper.vi.json -o paper.vi.docx   # or .md, .tex, .epub, friendly, medium
```

The PDF provides the frame (pictures, formulas, page layout), the JSON the text. What can be edited
is the `text` of blocks and table cells and the `rows` of author groups; formulas stay what they
are wherever their `<m>` tag is put, whatever is written inside it. The rest is read-only. Blocks
left out of the file, or left unchanged, keep their text, so an unedited file changes nothing.
Extra fields are ignored, so a file can keep the original next to the edited text. The file must
come from the same PDF (its `sha256`) and the same analysis options (`--tables`). A block whose
markup cannot be read (an unknown tag) keeps its text, with a warning.

In `hard-95` an edited block flows inside the box of its lines, its type made smaller when the new
text is longer (down to 62%), and a paragraph that runs over columns or pages runs over them
again. The `hard` mode shows the pages themselves and cannot show edited text. In LaTeX output,
pdfLaTeX prints Vietnamese (with `vntex`); for scripts its fonts lack (Chinese, say), use XeLaTeX
or LuaLaTeX.

## Chunks for retrieval (RAG)

`kakemono convert paper.pdf -o paper.jsonl` (or `--format chunks`) cuts the text into chunks
ready to embed, one JSON object per line:

```json
{"id":3,"text":"I. INTRODUCTION\n\nThe discovery of the binary pulsar system PSR B1913+16 by Hulse and Taylor ...",
 "tokens":439,"title":"Observation of Gravitational Waves from a Binary Black Hole Merger",
 "section":["I. INTRODUCTION"],"blocks":[12,13],"pages":[0]}
```

- **Along the structure.** A chunk holds the blocks of one section, up to `--chunk-tokens`
  (default 512); a block is cut only when it does not fit in one chunk, at sentence ends. Tables
  give their caption and rows (cells joined by `|`), formulas their LaTeX (`$$...$$`).
- **Headed by the section path** (`3 Model > 3.2 Attention`), not by the paper title: within one
  paper a title on every chunk only blurs the embeddings. The title is in each chunk's `title`,
  to filter or prefix when searching several papers. `blocks` are the ids of the
  [editable JSON](#editable-json) (and `--json`), `pages` count from 0.
- **Sized in tokens.** By default an estimate that needs no tokenizer: 15 to 20% above the count
  of real subword tokenizers, so that a chunk seldom passes a model's limit. To count exactly,
  give `--tokenizer` your embedding model's `tokenizer.json` (from its Hugging Face repository) or
  `cl100k_base` / `o200k_base` for OpenAI's models. `tokens` then counts without special tokens
  (`[CLS]`, `<s>`), which most models add.

Measured on [Qasper](https://allenai.org/data/qasper) (questions on 100 NLP papers; share of the
gold evidence in the retrieved text, two embedding models; see `eval/qasper/`):

| Chunks from | nomic-embed v1.5, 1024 / 2048 tokens retrieved | Qwen3-Embedding 0.6B, 1024 / 2048 | Time, 100 papers (CPU) |
| --- | --- | --- | --- |
| **kakemono** (`-f chunks`) | **0.611** / **0.738** | **0.581** / **0.753** | **9.5 s** |
| docling + its `HybridChunker` | 0.608 / 0.734 | 0.536 / 0.705 | 2032 s |
| unstructured (`fast`) + `chunk_by_title` | 0.461 / 0.596 | 0.446 / 0.626 | 153 s |
| pypdf, cut every 512 tokens | 0.388 / 0.559 | 0.450 / 0.621 | 38 s |

docling ties with nomic and is 4 to 5 points behind with Qwen3 (a difference outside the 95%
interval of a bootstrap over papers); pypdf and unstructured are 13 to 22 points behind with both. Times are conversion and chunking,
without embedding, one paper after another on 4 cores.

```bash
kakemono convert paper.pdf -o paper.jsonl --chunk-tokens 256
kakemono convert paper.pdf -o paper.jsonl --tokenizer path/to/bge-m3/tokenizer.json
```

## Installation

**Prebuilt binaries** for Linux (x86_64, aarch64), macOS (Apple silicon, Intel) and Windows are
attached to every [release](https://github.com/iamcaominhtien/kakemono/releases/latest).

**With Cargo** (stable Rust):

```bash
cargo install kakemono
```

or, in a clone, `cargo build --release`; the executable is `target/release/kakemono`.

**As a library:** `cargo add kakemono` for Rust, `pip install kakemono` for Python (see
[below](#use-as-a-library)).

## Usage

```bash
kakemono convert 1706.03762
```

```bash
kakemono convert paper.pdf --mode medium -o paper.html
```

```bash
kakemono convert paper.pdf --all --json -o out/
```

```bash
kakemono convert paper.pdf -o paper.docx
```

```bash
kakemono convert paper.pdf -o paper.md --image-dir images
```

```bash
kakemono convert paper.pdf -o paper.tex
```

```bash
kakemono convert paper.pdf -o paper.epub
```

```bash
kakemono convert paper.pdf -o paper.json
kakemono convert paper.pdf --content paper.edited.json -m hard-95
```

The input is a PDF file, a Word document (`.docx`, `.doc`), an Excel workbook (`.xlsx`), an arXiv ID (`1706.03762`,
`arXiv:1706.03762v7`, `hep-th/9901001`) or an arXiv URL. Downloaded PDFs are cached in `~/.cache/kakemono` (`KAKEMONO_CACHE` changes it).

| Option | Meaning |
| --- | --- |
| `-m, --mode <hard\|hard-95\|medium\|friendly>` | Mode (default `friendly`); for Markdown, `friendly` or `medium` (how tables are written) |
| `--all` | All four modes: `<name>.<mode>.html` |
| `-f, --format <html\|docx\|md\|tex\|epub\|json\|chunks\|pdf>` | HTML (default), a Word document, Markdown, LaTeX, an EPUB book, editable JSON, chunks for retrieval, or a PDF (Word input only: the document laid out in pages as Word lays it out); chosen from `--output` when it ends in `.docx`, `.md`, `.tex`, `.epub`, `.json`, `.jsonl` or `.pdf` |
| `--header-rows <N>` | Header rows of every table of an Excel workbook, 0 for none (default: found from the cells); ignored for other inputs |
| `-o, --output <path>` | HTML, `.docx`, `.md`, `.tex`, `.epub`, `.json`, `.jsonl` or `.pdf` file, or directory |
| `--content <file.json>` | Write the output with the text of an edited JSON file (from `--format json`, for the same PDF); every format but the `hard` mode |
| `--images <inline\|files>` | Embed images (default for HTML and Word) or write them to a folder next to the file (default for Markdown and JSON, always for LaTeX; Word keeps them embedded too; never for EPUB) |
| `--image-dir <dir>` | That folder, relative to the output file (default `<name>_assets`); with `--all`, one subfolder per mode |
| `--scale <number>` | Image resolution in pixels per PDF point (default 3 ≈ 216 dpi), HTML, Word and EPUB |
| `--page-image <svg\|svg-only\|png>` | Page background of `hard`/`hard-95` (default `svg`; PNG for a page whose SVG would pass 1.5 MB, when smaller) |
| `--png-compression <fast\|balanced\|high>` | How hard PNG pictures are compressed (default `balanced`); `fast` draws them several times faster for files about a quarter larger, `high` makes them a tenth smaller for ten times the time. The pixels are the same |
| `--tables <auto\|image>` | Rebuild tables when their grid is clear (default), or keep pictures |
| `--equations <mathml\|image>` | Formulas as MathML (Word equations in `.docx`, LaTeX in `.md` and `.tex`, MathML in `.epub`) where the structure is clear, also inside the text (default); or display formulas as pictures and inline ones as styled text |
| `--fonts <embed\|system>` | Embed the reading and math fonts (default, works offline) or use the reader's fonts |
| `--json` | Also write `<name>.json`, the document structure with every field of the analysis (see below; to edit the text, use `--format json`) |
| `--chunk-tokens <N>` | Largest chunk, in tokens (default 512), for `--format chunks` |
| `--tokenizer <file.json\|name>` | Count the tokens of chunks with a real tokenizer: a Hugging Face `tokenizer.json`, or `cl100k_base` / `o200k_base` (OpenAI); the default is an estimate |
| `-j, --jobs <N>` | Threads (default one per core); the result is the same for every value |

Other commands: `kakemono fetch <id> -o paper.pdf` only downloads; `kakemono inspect paper.pdf`
prints the detected structure (headings, paragraphs, figures, formulas; `<math>` marks a formula
rebuilt as MathML); `kakemono formula picture.png` reads pictures of LaTeX formulas and prints
their MathML (experimental).

## The analysis as JSON

`--json` writes the `Document` of the analysis as it is, for debugging and tools that want every
field (the editable [JSON](#editable-json) is `--format json`). It holds the blocks in reading order. Each block has a `type` (`title`, `authors`,
`heading`, `paragraph`, `list_item`, `list_continuation`, `equation`, `figure`, `footnote`,
`reference`, `note`, `code`, `contents_entry`), its `page` (from 0), `bbox` and `spans`:

```json
{
  "id": 22, "type": "paragraph", "in_abstract": false, "page": 1,
  "bbox": { "x0": 107.7, "y0": 389.5, "x1": 505.2, "y1": 486.7 },
  "spans": [
    { "text": "The goal of reducing sequential computation ... Extended Neural GPU [", "style": 0 },
    { "text": "16", "style": 0, "href": "#ref-16" }
  ]
}
```

Joining the `text` of the spans gives the paragraph exactly. `style` is a bit set: 1 bold, 2 italic,
4 monospace, 8 math font, 16 superscript, 32 subscript, 64 sans-serif. A `code` block also has
`rows`, one per line, indented with spaces; a rebuilt table has a `table` field with its rows,
cells (`colspan`, `rowspan`) and rules. A `contents_entry` (a line of a book's table of contents)
has its `level` and printed `page`; its title links to the heading it names. Links of the PDF
(link annotations) are kept as `href` on the spans they cover.

## Use as a library

```rust
use kakemono::{HtmlOptions, Mode, Paper};

let paper = Paper::from_bytes(std::fs::read("paper.pdf")?)?;
let analysis = paper.analyze();
let out = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Friendly, ..Default::default() });
std::fs::write("paper.html", out.html)?;
```

`paper.to_html` can be called once per mode with the same analysis,
`paper.to_docx(&analysis, &DocxOptions::default())` returns the bytes of a Word document
(`to_docx_with_images` also returns its pictures as files), and
`paper.to_markdown(&analysis, &MarkdownOptions::default())` returns Markdown with its picture files,
`paper.to_latex(&analysis, &LatexOptions::default())` a LaTeX document with its picture files,
`paper.to_epub(&analysis, &EpubOptions::default())` the bytes of an EPUB book, and
`paper.to_json(&analysis, &JsonOptions::default())` the editable JSON with its picture files, and
`paper.to_chunks(&analysis, &ChunkOptions::default())` the chunks for retrieval
(`kakemono::chunks_jsonl` writes them as JSON Lines; `TokenCounter::load` reads a tokenizer,
`TokenCounter::custom` takes any counting function). The real tokenizers are the default cargo
features `tokenizers` (Hugging Face) and `openai-tokenizers`; without them, chunks are sized by the
estimate.
`paper.apply_content(&mut analysis, &json)` applies an edited JSON file to the analysis, and every
output written from it afterwards shows the edited text. `analysis.doc` is the
document structure that `--json` writes. The API reference is on [docs.rs](https://docs.rs/kakemono);
how the pipeline works is described in [ARCHITECTURE.md](ARCHITECTURE.md).

### Python

```bash
pip install kakemono
```

```python
import kakemono

html = kakemono.convert("paper.pdf")                   # friendly mode, one self-contained page
html = kakemono.convert("1706.03762", mode="medium")   # an arXiv id or URL works too
pages = kakemono.convert_all("paper.pdf")              # every mode, from one analysis
kakemono.to_docx("paper.pdf", "paper.docx")           # a Word document (also returns its bytes)
kakemono.to_markdown("paper.pdf", "paper.md")          # Markdown, pictures in paper_assets/
kakemono.to_latex("paper.pdf", "paper.tex")            # LaTeX, pictures in paper_assets/
kakemono.to_epub("paper.pdf", "paper.epub")            # an EPUB book (also returns its bytes)
content = kakemono.to_json("paper.pdf", "paper.json")  # the editable JSON (a dict), pictures in paper_assets/
html = kakemono.convert("paper.pdf", "hard-95", content=content)  # any output with the edited text
doc = kakemono.analyze("paper.pdf")                    # the structure, as with --json
```

The wheels (Linux, macOS, Windows; Python 3.9+) contain the Rust library: nothing else to install.
The options are those of the command line (`tables`, `equations`, `page_image`, `embed_fonts`,
`scale`, `jobs`, `png_compression`, `content`: an edited JSON as a dict, its text or its path);
see [python/README.md](python/README.md).

## Performance

Measured with `scripts/benchmark.py` on a 4-vCPU cloud machine with 16 GB of RAM (release build,
default `-j`, `friendly` mode; the other formats take up to a third longer, and `hard` and
`hard-95` up to 40% longer, because they draw a picture of every page):

| PDF | Pages | Size | Time | Peak memory |
| --- | ---: | ---: | ---: | ---: |
| Paper (Attention, 2017) | 15 | 2.2 MB | 0.4 s | 78 MB |
| Paper (GPT-4 report) | 100 | 5.2 MB | 0.9 s | 210 MB |
| Textbook (Dive into Deep Learning) | 1185 | 39.5 MB | 6.3 s | 580 MB |
| The same book twice | 2370 | 79 MB | 13.1 s | 1.1 GB |

- Time and memory grow linearly with the number of pages: count about 0.5 GB of memory per
  1000 pages of a book with many pictures, in every mode. The HTML is written to the file as its
  pictures are drawn, so neither the page nor its pictures are held whole in memory
  (`Paper::write_html` in the library).
- Four threads make a single long file about 2.5× faster than one; the renderers and the
  document-wide parts of the analysis still run on one thread. To convert many files, run one
  process per core with `-j 1`: on 4 cores, about 260 papers a minute, at about 53 MB each.
- Running more processes than cores only queues them: total throughput stays the same and the
  memory adds up. Four 1185-page books at once take 19 s and 3.2 GB.
- On a slow machine, `--png-compression fast` saves about a fifth of the time of a book with many
  pictures (18.3 s → 14.8 s for the textbook on one thread), for an HTML file 28% larger.
- The command line uses the mimalloc allocator; the library leaves the choice of allocator to
  the program that uses it.

**Word documents** (`scripts/word-benchmark.py`, same machine, against LibreOffice 24.2 on the
524 documents of the Word corpus, one at a time):

| Word → PDF | kakemono | LibreOffice 24.2 |
| --- | --- | --- |
| All 524 documents (11,657 pages) | 209 s (56 pages/s) | 790 s (14 pages/s) |
| Median per document | 0.12 s, 35 MB | 0.47 s, 192 MB |
| Largest memory | 526 MB | 822 MB |
| Could not convert | 0 | 5 (crashes) |
| Lines / page counts as in Word's PDF | 95.9% / 89.1% | 79.6% / 72.3% |

The flowing outputs of a Word file (friendly HTML, Word, Markdown, LaTeX, EPUB, JSON) skip the
page layout: 0.01–0.4 s per document, 15–23× faster than LibreOffice's Word and HTML exports
(`.doc` → `.docx` included). One process per core converts about 870 short documents a minute on 4
cores. By size, every format, threads and memory:
[docs/WORD-INPUT.md](docs/WORD-INPUT.md#speed-and-resources).

All the numbers, and leads for making it faster, are in
[docs/IMPROVEMENTS.md](docs/IMPROVEMENTS.md#10-benchmark-november-2026-and-leads-for-speed-and-memory).

## Limitations

- Tables whose cells hold matrices, long paragraphs or irregular rows stay pictures (`medium`,
  `friendly`) or text placed by position (`hard-95`).
- Formulas with `\underbrace`, a drawing, or symbols the PDF gives no Unicode code for stay
  pictures; a character with no code at all shows as `�` in `medium`/`friendly`.
- Formulas inside the text are found from their fonts and scripts: a fraction or a root set on
  several levels inside a line (`β₂/√β₁` stacked) is not rebuilt, and its pieces read as scripts.
- `hard-95` uses Latin Modern for TeX papers and the reader's fonts otherwise, so letter shapes may
  differ slightly; positions and line widths match.
- Author–year citations are not linked yet; scanned and encrypted PDFs are not supported.
- Word documents are one column (the page and column layout of the PDF is not kept), and figures
  are pictures.
- Word input: formulas made with MathType 2 (MTEF 2, in very old `.doc` files) stay pictures;
  floating shapes and text boxes are read where they are anchored in the flowing outputs; the page
  layout has no true transparency (a transparent fill is blended over white) and draws rounded
  and freeform shapes as their bounding rectangle.
- Excel input: `.xls`, `.ods` and `.csv` are not read; charts, pictures, pivot tables, comments and
  colours are left out; formulas show the value saved in the file; forms drawn on a grid stay
  sparse tables (or their text), not rebuilt forms.
- Unusual layouts may be read wrongly: `inspect` shows what was detected.

## Testing

```bash
scripts/fetch-test-pdfs.sh   # download the test papers, once
cargo test --release
```

- **Unit tests** and **end-to-end tests** on small generated PDFs.
- **8 real arXiv papers** (`tests/real_papers.rs`): per paper, the title, tables rebuilt,
  paragraphs broken mid-sentence, words still hyphenated, characters left in the `hard-95`
  background and formulas rebuilt as MathML. The limits are the current results: better passes,
  worse fails; tighten them after an improvement
  (`KAKEMONO_PRINT_METRICS=1 cargo test --release --test real_papers -- --nocapture`).
- **Editable JSON**: on every paper, an unedited file changes nothing and edited text comes back
  exactly as written; edited text reaches every output with its formulas and links.
- **Snapshots**: a hash of each mode's HTML, of the Word, Markdown, LaTeX and EPUB documents, of the
  editable JSON, of the chunks and of the `--json` document per paper, in `tests/data/snapshots/`.
  When the output changes on purpose, update them and commit them with the code:
  `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers`.

- **Word input** (`tests/word.rs`): `tests/data/word/sample.docx`, written by pandoc from
  `sample.md`, `sample.doc`, the same saved by LibreOffice, and `lists.docx` (nested lists) are
  read and written to every format (snapshot in `tests/data/snapshots/word-sample.txt`); the LIGO
  paper goes through a round trip PDF → Word → kakemono. To make the samples again:
  `cd tests/data/word && pandoc sample.md -o sample.docx && soffice --headless --convert-to doc sample.docx`.
- **Excel input** (`tests/excel.rs`): `tests/data/excel/sample.xlsx`, saved by LibreOffice from the
  hand-written `sample.fods`, is read and written to every format (snapshot in
  `tests/data/snapshots/excel-sample.txt`). To make it again:
  `soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods`.
- **Word layout fidelity** (CI job "Word layout fidelity"): `scripts/fetch-word-corpus.sh`
  downloads the Word documents of `tests/data/word-corpus.tsv` with the PDF Word made of each
  (public documents: gov.uk, data.gov.uk, data.gov.au, public GitHub repositories; not committed);
  `scripts/word-fidelity.py` lays each out and compares it with Word's PDF line by line
  (`scripts/compare-layout.py`): the share of lines broken at the same words, and of documents
  with the same page count. The job fails below its minimums. Pairs left out, and why, are in
  `tests/data/word-corpus-excluded.tsv`.
- **Word layout fixtures** (`tests/word_layout.rs`): seven documents of that corpus, chosen for
  what they hold (columns, text boxes, floating shapes, frames, EMF pictures, rows of exact height),
  are laid out and compared with Word's PDFs on every CI run, with a minimum per document; they
  are downloaded first (`scripts/fetch-word-corpus.sh 8 $(cat tests/data/word-layout-fixtures.txt)`)
  and skipped when missing.
- **Crash test**: `scripts/word-crash-test.py DIR...` converts every Word file and every Excel
  workbook found to every output and reports panics and timeouts (run on 4,300 public Word files
  and on the 90 public workbooks listed by `scripts/fetch-excel-corpus.sh`, of which 84 download
  from a typical network, 6 being blocked or changed at their source: none).

Only the LIGO paper (CC BY 4.0) is in the repository; tests skip papers that are not downloaded.
The JavaScript is checked in a browser (needs `playwright`):

```bash
node tests/browser/check-friendly.js out/*.friendly.html
node tests/browser/check-hard95.js out/*.hard-95.html
node tests/browser/check-stable.js out/*.medium.html out/*.friendly.html
```

The Python package is tested with `pytest` (needs [maturin](https://www.maturin.rs)):

```bash
cd python && python -m venv .venv && . .venv/bin/activate
pip install maturin pytest && maturin develop --release && pytest tests
```

CI runs `cargo fmt`, `cargo clippy`, the tests, the browser checks and the Python tests (with
wheels built for every platform) on every pull request.

## Contributing and releases

Issues and pull requests are welcome; read [ARCHITECTURE.md](ARCHITECTURE.md) and
[CONTRIBUTING.md](CONTRIBUTING.md) first. Every merge into `main` is released automatically from
Conventional Commits: version, `CHANGELOG.md`, executables for Linux, macOS and Windows, the crate
on crates.io and the wheels on PyPI.

## License

[MIT](LICENSE) © Cao Minh Tiến. The main dependencies (hayro, vello) are Apache-2.0 or MIT.
