<div align="center">

<img src="docs/assets/logo.svg" alt="kakemono logo" width="120" height="120">

# kakemono

**Turn PDF papers and books into clean, readable HTML — in pure Rust, as a single executable.**

[![Release](https://img.shields.io/github/v/release/iamcaominhtien/kakemono?label=release&color=d9822b)](https://github.com/iamcaominhtien/kakemono/releases/latest)
[![CI](https://img.shields.io/github/actions/workflow/status/iamcaominhtien/kakemono/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white)](https://github.com/iamcaominhtien/kakemono/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/Rust-2024_edition-000000?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![PDF engine](https://img.shields.io/badge/PDF_engine-hayro-orange)](https://github.com/LaurenzV/hayro)
[![Platforms](https://img.shields.io/badge/platforms-Linux_%7C_macOS_%7C_Windows-555)](https://github.com/iamcaominhtien/kakemono/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-2f9e44)](LICENSE)

[Features](#features) · [Modes](#the-four-modes) · [Install](#installation) · [Usage](#usage) ·
[Library](#use-as-a-library) · [Architecture](ARCHITECTURE.md) · [Changelog](CHANGELOG.md)

</div>

---

> *Kakemono* is a Japanese hanging scroll: a work unrolled so that it can be read at a
> glance. kakemono does the same for documents — it unrolls a fixed PDF page into text that flows.

Give it a local PDF or an arXiv ID; get HTML with continuous text, ready to read on any screen or
to feed to translation tools.

```bash
kakemono convert 1706.03762            # "Attention Is All You Need" → 1706.03762.friendly.html
```

## Features

| | |
| --- | --- |
| 🦀 **Pure Rust** | One executable: no `poppler`, `mupdf` or Python. PDF reading and rendering use [hayro](https://github.com/LaurenzV/hayro). |
| 📖 **Continuous text** | Lines joined into paragraphs, end-of-line hyphens removed, paragraphs joined across columns and pages, running heads and page numbers removed, footnotes set apart. |
| 🧱 **Real structure** | Tables rebuilt as `<table>`, display formulas as MathML, code listings kept line by line, lists, headings and cross-links. |
| 🖼️ **Sharp images** | Figures stay vector (SVG) or render at 216 dpi; photographs are stored as JPEG. |
| 🌐 **Translation-ready** | A paragraph is always one DOM element, and `--json` exports the blocks in reading order. |
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

## Installation

**Prebuilt binaries** for Linux (x86_64, aarch64), macOS (Apple silicon, Intel) and Windows are
attached to every [release](https://github.com/iamcaominhtien/kakemono/releases/latest).

**From source** (stable Rust):

```bash
cargo install --git https://github.com/iamcaominhtien/kakemono
```

or, in a clone, `cargo build --release`; the executable is `target/release/kakemono`.

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

The input is a PDF file, an arXiv ID (`1706.03762`, `arXiv:1706.03762v7`, `hep-th/9901001`) or an
arXiv URL. Downloaded PDFs are cached in `~/.cache/kakemono` (`KAKEMONO_CACHE` changes it).

| Option | Meaning |
| --- | --- |
| `-m, --mode <hard\|hard-95\|medium\|friendly>` | Mode (default `friendly`) |
| `--all` | All four modes: `<name>.<mode>.html` |
| `-o, --output <path>` | HTML file, or directory |
| `--images <inline\|files>` | Embed images (default) or write them to `<name>_assets/` |
| `--scale <number>` | Image resolution in pixels per PDF point (default 3 ≈ 216 dpi) |
| `--page-image <svg\|svg-only\|png>` | Page background of `hard`/`hard-95` (default `svg`; PNG for a page whose SVG would pass 1.5 MB, when smaller) |
| `--tables <auto\|image>` | Rebuild tables when their grid is clear (default), or keep pictures |
| `--equations <mathml\|image>` | Display formulas as MathML where the structure is clear (default), or pictures |
| `--fonts <embed\|system>` | Embed the reading and math fonts (default, works offline) or use the reader's fonts |
| `--json` | Also write `<name>.json`, the document structure |
| `-j, --jobs <N>` | Threads (default one per core); the result is the same for every value |

Other commands: `kakemono fetch <id> -o paper.pdf` only downloads; `kakemono inspect paper.pdf`
prints the detected structure (headings, paragraphs, figures, formulas; `<math>` marks a formula
rebuilt as MathML); `kakemono formula picture.png` reads pictures of LaTeX formulas and prints
their MathML (experimental).

## JSON for translation tools

`--json` writes the blocks in reading order. Each block has a `type` (`title`, `authors`,
`heading`, `paragraph`, `list_item`, `list_continuation`, `equation`, `figure`, `footnote`,
`reference`, `note`, `code`), its `page` (from 0), `bbox` and `spans`:

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
cells (`colspan`, `rowspan`) and rules.

## Use as a library

```rust
use kakemono::{HtmlOptions, Mode, Paper};

let paper = Paper::from_bytes(std::fs::read("paper.pdf")?)?;
let analysis = paper.analyze();
let out = paper.to_html(&analysis, &HtmlOptions { mode: Mode::Friendly, ..Default::default() });
std::fs::write("paper.html", out.html)?;
```

How the pipeline works is described in [ARCHITECTURE.md](ARCHITECTURE.md).

## Limitations

- Tables whose cells hold matrices, long paragraphs or irregular rows stay pictures (`medium`,
  `friendly`) or text placed by position (`hard-95`).
- Formulas with `\underbrace`, a drawing, or symbols the PDF gives no Unicode code for stay
  pictures; a character with no code at all shows as `�` in `medium`/`friendly`.
- `hard-95` uses Latin Modern for TeX papers and the reader's fonts otherwise, so letter shapes may
  differ slightly; positions and line widths match.
- Author–year citations are not linked yet; scanned and encrypted PDFs are not supported.
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
- **Snapshots**: a hash of each mode's HTML and of the JSON per paper, in `tests/data/snapshots/`.
  When the output changes on purpose, update them and commit them with the code:
  `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers`.

Only the LIGO paper (CC BY 4.0) is in the repository; tests skip papers that are not downloaded.
The JavaScript is checked in a browser (needs `playwright`):

```bash
node tests/browser/check-friendly.js out/*.friendly.html
node tests/browser/check-hard95.js out/*.hard-95.html
node tests/browser/check-stable.js out/*.medium.html out/*.friendly.html
```

CI runs `cargo fmt`, `cargo clippy`, the tests and the browser checks on every pull request.

## Contributing and releases

Issues and pull requests are welcome; read [ARCHITECTURE.md](ARCHITECTURE.md) and
[CONTRIBUTING.md](CONTRIBUTING.md) first. Every merge into `main` is released automatically from
Conventional Commits (version, `CHANGELOG.md`, executables for Linux, macOS and Windows).

## License

[MIT](LICENSE) © Cao Minh Tiến. The main dependencies (hayro, vello) are Apache-2.0 or MIT.
