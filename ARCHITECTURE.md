# Architecture

`kakemono` is a **pipeline**: a PDF passes through a chain of stages. Each stage takes one clearly
defined form of data and returns the next. The analysis side (PDF → `Document`) and the output side
(`Document` → HTML or Word) meet at exactly one place: the `Document` model in `model`.

```
 PDF bytes
    │
    ▼  backend/          Adapter around the hayro library: the only code that knows about hayro
 RawPage[] + FontInfo[]  (characters, images, strokes of each page; font table)
    │
    ▼  analysis/pipeline.rs   one function; read it top to bottom to see the whole flow
    │   page        PageData, document-wide font size / line spacing statistics
    │   lines       characters → lines (superscript/subscript, accents, rotated text)
    │   furniture   remove repeated headers/footers/page numbers
    │   floats      figures, tables, algorithms + captions
    │   tables      row/column grid of tables
    │   order       reading order (columns, regions)
    │   blocks      lines → raw blocks (paragraphs, equations, footnotes)
    │   semantics/  classification (title, authors, headings, code listings, …), paragraph joining,
    │               reference linking, building the Document
    │   crops       equation pictures that do not cut glyphs
    │   math        structure of display formulas (MathNode trees, written as MathML)
    ▼
 Document (model)       fixed boundary between analysis and output
    │
    ▼  render/        trait Renderer: one implementation per mode
 hard │ hard-95 │ medium │ friendly        shared: assets, escape, flow, mathml, table
    │
    └► render/docx/   the same Document as a Word file (.docx): no Renderer, its own writer
```

## Modules

| Module | Role |
| --- | --- |
| `lib.rs` | Public API (the semver promise of the crate on crates.io): `Paper`, `convert`, `Analysis`, the Options types, `config`, `model`, `source`. The other modules are `#[doc(hidden)]` internals, public only for the CLI, examples and tests |
| `main.rs` | CLI: only reads the arguments, then calls the API |
| `paper.rs` | `Paper`: loads the PDF, runs the analysis, outputs HTML (`to_html`) or Word (`to_docx`) |
| `config.rs` | `AnalyzeOptions` and `LayoutConfig` (every threshold of the algorithm) |
| `source/` | PDF sources: `arxiv` (ID recognition, download, cache) |
| `backend/` | Adapter around hayro: reads pages (`extract`), recognizes fonts (`fonts`), draws pages/regions (`render`; resvg turns a very large SVG page into a PNG; a photograph is cut out as a JPEG) |
| `common/` | Shared code with no dependency on any other module: geometry, text styles, text handling, parallel execution |
| `analysis/` | The analysis stages, orchestrated by `analysis/pipeline.rs` |
| `model/` | `Document` and its sub-types: the output of analysis, the input of HTML output, and also JSON |
| `render/` | HTML output: a `Renderer` for each mode, the `assets` image store, shared HTML parts |
| `render/docx/` | Word output: `body` (blocks → `word/document.xml`), `omml` (`MathNode` → Office Math), `table` (Word tables), `parts` (styles, relationships, page), `zip` (a minimal ZIP writer). Pictures are placeholders drawn afterwards on several threads, as in `assets` |
| `python/` | The Python package (separate crate, built with maturin): `src/lib.rs` binds `Paper` with PyO3 and releases the GIL; `kakemono/__init__.py` adds paths and arXiv ids |
| `raster/` | Formulas from pictures (`kakemono formula`): pieces of ink recognised against the TeX fonts (`tex`, `templates`, `shape`, `recognize`), then rebuilt by `analysis::math` |

Dependencies run in one direction only:

```
main → paper → analysis → backend → common
              → render  → model   → common
```

`render` knows nothing about `analysis` except `Analysis`/`Document`, and `analysis` knows nothing
about HTML.

## Patterns

| Pattern | Where | Purpose |
| --- | --- | --- |
| Pipeline | `analysis/pipeline.rs` | See the whole flow in one place; test each stage on its own |
| Configuration Object | `config::LayoutConfig`, one sub-struct per stage | Every threshold has a name, a doc, a unit, and lives in one place |
| Rules (chain of rules) | Block classification in `analysis/blocks` and `analysis/semantics` | Each rule is a small function; adding or changing a rule does not touch the others |
| Strategy | `render::Renderer` | The four modes share one interface; adding a mode = adding a module |
| Adapter | `backend/` | Only one place depends on hayro (and resvg) |
| Builder | `analysis/semantics/text.rs` (`TextBuilder`), builds the `Document` | Build complex data step by step |

No inheritance trees and no Visitor: `enum` + `match` is the natural Rust way to handle block kinds.

## Code conventions

**Order within a file:** module doc (`//!`) → `use` → constants and regexes → data types (with
`impl`) → public functions → private helper functions → `#[cfg(test)] mod tests`.

**Thresholds and constants.**
- A threshold of the algorithm (a number that exists by choice, for example "split when the gap is
  more than 1.45 times the font size") is a documented field in that stage's config struct, with a
  name that carries its unit:
  `_em` (multiple of the font size), `_pt` (PDF point), `_ratio` (ratio 0–1).
- A constant that is not a threshold (a unit, a buffer size) is a documented `const` at the top of
  the file.
- Numbers with an obvious mathematical meaning (`0.5` for the midpoint, `2.0` for doubling, `100.0`
  for percent) stay as they are.
- Anything used in several modules (text style flags, caption-matching regexes) lives in `common/`.

**Functions.** At most about 80 lines; split longer ones into named steps. Each function does one
thing that you can say in one sentence.

**Names.** Use meaningful names (`line`, `block`, `line_idx`). Do not use single letters, except
short loop indices (`i`, `j`, `k`), coordinates (`x`, `y`), counts (`n`) and the comparison pair
`|a, b|` in `sort_by`.

**Docs.** Every item (module, type, field, function, constant) has a `///` doc. The doc says *what*
it does and *why*, and does not repeat the code. For a non-trivial function, state the meaning of the
parameters, the return value and the special cases (when it returns `None`, when it panics).

**Visibility.** Only what belongs to the public API is `pub`; internals are `pub(crate)` or private.

CI checks these conventions (`clippy.toml`, lints in `lib.rs`).

## Guarantee of unchanged behavior

Every structural change must keep the output the same. `tests/real_papers.rs` compares the output
hashes of 8 real papers, in all four modes, in Word and in JSON, with `tests/data/snapshots`; one character of
difference turns the test red. When you change the output on purpose, run
`KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --test real_papers` and commit the snapshots with the change.
