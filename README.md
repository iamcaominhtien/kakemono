<div align="center">

<img src="docs/assets/logo.svg" alt="kakemono logo" width="120" height="120">

# kakemono

**Turn PDF papers and books into clean, readable HTML, Word documents, Markdown, LaTeX or EPUB books — in pure Rust, as a single executable.**

[![Release](https://img.shields.io/github/v/release/iamcaominhtien/kakemono?label=release&color=d9822b)](https://github.com/iamcaominhtien/kakemono/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/iamcaominhtien/kakemono/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white)](https://github.com/iamcaominhtien/kakemono/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/kakemono?logo=rust&logoColor=white&color=000000)](https://crates.io/crates/kakemono)
[![PyPI](https://img.shields.io/pypi/v/kakemono?logo=python&logoColor=white&color=3776ab)](https://pypi.org/project/kakemono/)
[![docs.rs](https://img.shields.io/docsrs/kakemono?logo=docsdotrs&label=docs.rs)](https://docs.rs/kakemono)
[![PDF engine](https://img.shields.io/badge/PDF_engine-hayro-orange)](https://github.com/LaurenzV/hayro)
[![Platforms](https://img.shields.io/badge/platforms-Linux_%7C_macOS_%7C_Windows-555)](https://github.com/iamcaominhtien/kakemono/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-2f9e44)](LICENSE)

[Features](#features) · [Modes](#the-four-modes) · [Word](#word-documents) · [Markdown](#markdown) · [LaTeX](#latex) · [EPUB](#epub) · [JSON](#editable-json) · [Install](#installation) · [Usage](#usage) ·
[Library](#use-as-a-library) · [Python](#python) · [Architecture](ARCHITECTURE.md) · [Changelog](CHANGELOG.md)

</div>

---

> *Kakemono* is a Japanese hanging scroll: a work unrolled so that it can be read at a
> glance. kakemono does the same for documents — it unrolls a fixed PDF page into text that flows.

Give it a local PDF or an arXiv ID; get HTML with continuous text, ready to read on any screen or
to feed to translation tools, a Word document to edit, Markdown for notes, wikis and LLMs, a
LaTeX source to edit and typeset again, an EPUB book for an e-reader, or JSON for other
programs, which can be edited (translated, say) and turned back into any of these.

```bash
kakemono convert 1706.03762            # "Attention Is All You Need" → 1706.03762.friendly.html
kakemono convert 1706.03762 -o attention.docx   # the same paper as a Word document
kakemono convert 1706.03762 -o attention.md     # Markdown, pictures in attention_assets/
kakemono convert 1706.03762 -o attention.tex    # LaTeX, pictures in attention_assets/
kakemono convert 1706.03762 -o attention.epub   # an EPUB book for e-readers
kakemono convert 1706.03762 -o attention.json   # the content as editable JSON
kakemono convert 1706.03762 --content attention.vi.json -m hard-95   # the pages, with the edited text
```

## Features

| | |
| --- | --- |
| 🦀 **Pure Rust** | One executable: no `poppler`, `mupdf` or Python. PDF reading and rendering use [hayro](https://github.com/LaurenzV/hayro). |
| 📖 **Continuous text** | Lines joined into paragraphs, end-of-line hyphens removed, paragraphs joined across columns and pages, running heads and page numbers removed, footnotes set apart. |
| 🧱 **Real structure** | Tables rebuilt as `<table>`, formulas (displayed and inside the text) as MathML, code listings kept line by line, lists, headings and cross-links. |
| 🖼️ **Sharp images** | Figures stay vector (SVG) or render at 216 dpi; photographs are stored as JPEG. |
| 📝 **Word documents** | `.docx` with Word styles, Word tables and editable Word equations, from the same analysis. |
| ✍️ **Markdown** | `.md` with pipe tables, LaTeX formulas (`$$`) and the pictures in a folder of their own. |
| 📐 **LaTeX** | `.tex` that compiles with pdfLaTeX, XeLaTeX or LuaLaTeX: sections, `tabular` tables, LaTeX math. |
| 📚 **EPUB** | `.epub` (EPUB 3) for e-readers: reflowing text, a table of contents, MathML formulas, tables and the first page as the cover. |
| 🌐 **Editable JSON** | `.json` with the blocks in reading order, their text with styles and formulas kept; edit it (translate it, say) and `--content` writes any output from it, the `hard-95` pages included. |
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
- rebuilt tables become pipe tables, or HTML tables when cells span rows or columns;
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
XeLaTeX and LuaLaTeX print it when their font has it. `--scale` and `--equations` apply.

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
again. The `hard` mode shows the pages themselves and cannot show edited text. LaTeX output with
non-Latin text (Vietnamese, say) needs XeLaTeX or LuaLaTeX.

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

The input is a PDF file, an arXiv ID (`1706.03762`, `arXiv:1706.03762v7`, `hep-th/9901001`) or an
arXiv URL. Downloaded PDFs are cached in `~/.cache/kakemono` (`KAKEMONO_CACHE` changes it).

| Option | Meaning |
| --- | --- |
| `-m, --mode <hard\|hard-95\|medium\|friendly>` | Mode (default `friendly`) |
| `--all` | All four modes: `<name>.<mode>.html` |
| `-f, --format <html\|docx\|md\|tex\|epub\|json>` | HTML (default), a Word document, Markdown, LaTeX, an EPUB book or editable JSON; chosen from `--output` when it ends in `.docx`, `.md`, `.tex`, `.epub` or `.json` |
| `-o, --output <path>` | HTML, `.docx`, `.md`, `.tex`, `.epub` or `.json` file, or directory |
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
`paper.to_json(&analysis, &JsonOptions::default())` the editable JSON with its picture files.
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
doc = kakemono.analyze("paper.pdf")                    # the structure, as with --json
```

The wheels (Linux, macOS, Windows; Python 3.9+) contain the Rust library: nothing else to install.
The options are those of the command line (`tables`, `equations`, `page_image`, `embed_fonts`,
`scale`, `jobs`, `png_compression`); see [python/README.md](python/README.md).

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
  editable JSON and of the `--json` document per paper, in `tests/data/snapshots/`.
  When the output changes on purpose, update them and commit them with the code:
  `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers`.

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
