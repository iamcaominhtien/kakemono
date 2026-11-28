# Architecture

`kakemono` is a **pipeline**: a PDF passes through a chain of stages. Each stage takes one clearly
defined form of data and returns the next. The analysis side (PDF → `Document`) and the output side
(`Document` → HTML, Word, Markdown, LaTeX, Typst, EPUB, JSON or chunks) meet at exactly one place: the `Document` model in `model`.
A Word document (`.docx`, `.doc`) or an Excel workbook (`.xlsx`) skips the analysis: `word/` or `excel/` reads it straight into a `Document`.
So does an EPUB book: `epub/` reads its XHTML into the intermediate form of `word/`, which builds the `Document`.
So does a web page: `html/` parses it as a browser does, keeps its article and writes it as XHTML for the reader of `epub/`.
An edited JSON file is applied to the `Document` (`render/json/apply.rs`; a figure's `replace_image` by `render/json/replace.rs`) before any output is written.

```
 PDF bytes
    │
    ▼  backend/          Adapter around the hayro library: the only code that knows about hayro
 RawPage[] + FontInfo[]  (characters, images, strokes of each page; font table)
    │
    ▼  analysis/pipeline.rs   one function; read it top to bottom to see the whole flow
    │   page        PageData, document-wide font size / line spacing statistics
    │   lines       characters → lines (superscript/subscript, inline fractions, drop caps, accents, rotated text)
    │   furniture   remove repeated headers/footers/page numbers (by band, or at the same place)
    │   floats      figures, tables, algorithms + captions
    │   tables      row/column grid of tables
    │   order       reading order (columns, regions)
    │   blocks      lines → raw blocks (paragraphs, equations, footnotes, margin notes)
    │   semantics/  classification (title, authors, contents, headings, code listings, …), paragraph joining,
    │               reference linking, building the Document
    │   crops       equation pictures that do not cut glyphs
    │   math        structure of display formulas (MathNode trees: matrices, cases, dots)
    ▼
 Document (model)       fixed boundary between analysis and output
    ▲
    │  word/          DOCX/DOC bytes → the same Document: docx.rs (OOXML) or doc/ (Word 97–2003)
    │                 → ir.rs (paragraphs, runs, tables, notes) → build.rs (styles → block kinds)
    │  epub/          EPUB bytes → the package and spine → html.rs (XHTML → word's ir.rs) → build.rs
    │  html/          a web page (file, bytes or address) → html5ever → the article (clutter removed,
    │                 notes marked) → XHTML → epub's html.rs → build.rs
    │
    │  excel/         XLSX bytes → the same Document: sheet.rs (a streamed sheet → Grid) →
    │                 regions.rs (the cells cut into tables) → build.rs (captions, header rows, notes)
    │
    │  chart/         a chart part of either (c:chartSpace, cx:chartSpace) → Chart → an SVG
    │                 picture and a data table, placed by word/ and excel/ (and word/fmt/ for the layout)
    │
    │  word/fmt/      DOCX/DOC bytes → FDoc, every formatting property resolved
    │   └► layout/    FDoc laid out in pages as Word does → a PDF (`-f pdf`); for the hard and
    │                 hard-95 modes that PDF goes through the PDF pipeline above; medium is
    │                 layout/html.rs (Word's formatting in one continuous page)
    │      layout/workbook/  a workbook's Document → FDoc (tables fitted to the page) → the
    │                 same engine → a PDF
    │
    ▼  render/        trait Renderer: one implementation per mode
 hard │ hard-95 │ medium │ friendly        shared: assets, escape, flow, mathml, table
    │
    ├► render/docx/      the same Document as a Word file (.docx): no Renderer, its own writer
    ├► render/markdown/  the same Document as Markdown (.md): pictures through the assets store
    ├► render/latex/     the same Document as LaTeX (.tex): pictures through the assets store
    ├► render/typst/     the same Document as Typst (.typ), an article or a book: pictures through the assets store
    ├► render/epub/      the same Document as an EPUB book (.epub): the flowing HTML as XHTML
    ├► render/chunks/    the text cut into chunks for retrieval (.jsonl)
    └► render/json/      the content as editable JSON (.json) ─┐
                                                               │ edited, applied back to the Document
 Document ◄────────────────────────── render/json/apply.rs ◄───┘ (--content), then any output above
```

## Modules

| Module | Role |
| --- | --- |
| `lib.rs` | Public API (the semver promise of the crate on crates.io): `Paper`, `convert`, `Analysis`, the Options types, `config`, `model`, `source`. The other modules are `#[doc(hidden)]` internals, public only for the CLI, examples and tests |
| `main.rs` | CLI: only reads the arguments, then calls the API |
| `paper.rs` | `Paper`: loads the PDF, Word document, workbook, book or web page (`from_bytes`, `from_bytes_with`, `from_path`, `from_url`; and its SHA-256), runs the analysis, outputs HTML (`to_html`), Word (`to_docx`, `to_docx_with_images`), Markdown (`to_markdown`), LaTeX (`to_latex`), Typst (`to_typst`), EPUB (`to_epub`), editable JSON (`to_json`) chunks (`to_chunks`) or, for a Word document, a book or a workbook, a PDF of its pages (`to_pdf`, `to_pdf_with`), and applies an edited JSON file (`apply_content`) |
| `config.rs` | `AnalyzeOptions` and `LayoutConfig` (every threshold of the algorithm), `ExcelConfig` (the thresholds of the Excel reader, re-exported from `excel/`) |
| `source/` | Where documents come from: `arxiv` (ID recognition, download, cache), `web` (a document at an `http(s)` address, and the pictures of a page, fetched several at a time, with limits on size, number and time) |
| `word/` | Word documents as input, read without layout analysis: `zip` (a minimal ZIP reader), `docx` (the parts of a `.docx`: document, styles, numbering, notes, relationships, pictures, fields), `doc/` (the binary `.doc` in its compound file, with `cfb`: `fib` the File Information Block and the piece table, `props` sprms, property pages, style sheet, fonts, lists, bookmarks, `picture` pictures in the `Data` stream), `styles` (style inheritance and list markers, shared by both), `omml` (Office Math → `MathNode`), `pictures` (pictures made ready: PNG and JPEG as they are, other bitmaps made PNG, SVG and metafiles kept as SVG written again by usvg, made ready once for the text and the page layout; a `asvg:svgBlip` is taken over the PNG beside it; a chart's SVG stored as it is), charts of a `.docx` (`w:drawing` of a chart part, through `chart/`: a figure and its data table; an Office 2016 chart shows the fallback picture of its `mc:AlternateContent` when there is one; `.doc` charts are left out), `ir` (what both readers fill in), `build` (`ir` → `Document`: block kinds from styles, captions given to floats, notes after their paragraph, links) |
| `excel/` | Excel workbooks as input, read without layout analysis (a workbook has no pages; `Source::Sheet` keeps the file for the sheet view): `workbook` (sheets in order, date system, print titles, title), `strings` (shared strings with rich-text runs, furigana left out), `styles` (cell formats: number format, bold and italic, and the look the sheet view draws: font, fill, borders, alignment), `numfmt` (a number or date as Excel shows it; dates always ISO), `pull` (a pull reader of XML, so a sheet is never held as a tree), `sheet` (a worksheet part → `Grid`: cells, merges, hyperlinks, hidden rows and columns, declared tables), `grid`, `regions` (blocks of cells cut by empty rows and columns), `drawing` (text and look of shapes, text boxes and connectors, pictures and charts of the sheet's drawing, with their anchors and frames), `richdata` (pictures placed in cells: `vm` → `xl/metadata.xml` → `xl/richData/`), `build` (`Grid` → blocks: sheet heading, captioned tables with their header rows, notes, or paragraphs for a form; pictures; charts as figure and data table), `view` (the sheet view, the medium mode: the sheets as Excel shows them, read again with every cell's look); the thresholds are `ExcelConfig` (`config.rs`) |
| `chart/` | Charts of workbooks and Word documents (`pub(crate)`): `read` (`c:chartSpace` → `Chart`: plot groups, series with their cached values, colours, data labels, axes, legend; formulas are not computed), `chartex` (Office 2016 `cx:chartSpace`: title and data, not drawn), `theme` (accent colours of the theme part), `table` (`data_table`: the categories as rows, a column per series), `svg/` (our renderer: `plane` bars, lines, areas, scatter; `round` pies, doughnuts, radars; `axis` nice ticks; `labels` data labels; `write` escaped SVG text), `place` (`Charts`: a chart part read and drawn at a size, within `ChartConfig`'s limits; `blocks`: the figure, then the data table with the same caption, or a note). A chart SVG is ours, made of numbers and escaped text: it is stored as an SVG picture without usvg's rewriting (`word::pictures::store_chart`), within the SVG byte cap |
| `epub/` | EPUB books as input, read without layout analysis: `mod.rs` (the package: `META-INF/container.xml`, the package document's metadata and spine, paths inside the archive, read with `word/zip`), `html` (XHTML → the `word/ir` form, with Word style names: `<h2>` → "heading 2", `<pre>` → "Source Code", `<figcaption>` → "Caption"; lists, tables with merged cells, pictures, notes called by `noteref` links, bookmarks for every `id`), `css` (simple rules of the style sheets that give runs their style), `mathml` (MathML → `MathNode`), `entities` (HTML's named character references for the XML parser). `word/build` then makes the `Document` |
| `html/` | Web pages as input (`.html`, `.htm`, `.xhtml`, or an address): `decode` (the encoding: byte order mark, HTTP `charset`, `<meta>`, else UTF-8 or Windows-1252), `dom` (html5ever's tree copied into a vector of nodes, edited, then written as well-formed XHTML with the SVG and MathML namespaces), `clean` (the article: elements, roles, hidden elements and classes or `id`s of clutter removed, the element that holds the text found as Readability does, the title (`<h1>`, else the metadata), authors from `<meta>`, lazy pictures and `srcset`s resolved, icons removed, links made absolute, `<br><br>` made a paragraph break), `notes` (superscript links to a list item become notes; LaTeXML's notes, authors and abstract; MediaWiki's links back). The XHTML is read by `epub/html` as a book of one document, its pictures from `file:` URLs, `data:` URLs or fetched (`source/web`) |
| `word/fmt/` | A Word document with all its formatting, for the layout engine (`FDoc`, `model`): `docx` and `doc` read sections, headers and footers, resolved paragraph, run, table and cell properties (`props`, `styles`; `sprm` for `.doc`), lists, notes, fields, pictures, shapes and embedded fonts; `drawing` the floating shapes and text boxes of a `.doc` (OfficeArt), `frames` frames placed like text boxes, `contrast` automatic text colour |
| `word/metafile/` | EMF and WMF pictures replayed on a small model of a Windows device context (`dc`) and written as SVG |
| `word/mtef.rs` | MathType and Equation Editor formulas (MTEF 3 and 5, the `Equation Native` stream of an OLE object) → `MathNode` |
| `layout/` | The Word layout engine (SVG pictures drawn as vector): `fonts` (embedded, installed, metric-compatible open fonts, and the built-in metrics of `metrics.txt` for fonts with no open match), `shape` (glyphs with Word's defaults), `para` (line breaking, tabs, justification, line heights), `flow` (sections, columns, pages: spacing, widows and orphans, keep-with-next, tables, laid out a few rows at a time as they are placed, headers and footers, footnotes, floating objects; a long document, of `STREAM_MIN_CHARS` or more, hands each page to a `PageSink` as soon as it is drawn, after a pass that only counts when its headers show the page count or its links need the bookmarks of later pages), `display` (what is drawn where: the runs of a line in one face are joined as they are placed, and each glyph takes 28 bytes, since a shorter document's pages are held until the PDF is written), `pdf` (written with krilla page by page, `Writer`), `html` (the medium mode for Word), `lines` (text lines of a PDF with their baselines, to compare a layout with Word's), `workbook` (a workbook's `Document` as an `FDoc`: headings, paragraphs, figures, and tables whose columns are measured with the engine's fonts and fitted to the page, smaller, on landscape pages or in groups of columns; `WorkbookPdfConfig`), `workbook/print` (the default PDF of a workbook: each sheet printed as Excel prints it, from the sheet view's reading, as display pages) |
| `backend/` | Adapter around hayro: reads pages (`extract`), recognizes fonts (`fonts`), draws pages/regions (`render`; resvg turns a very large SVG page into a PNG; a photograph is cut out as a JPEG); `svg`: a foreign SVG picture parsed by usvg and written back by usvg's writer (no script, event handler, `foreignObject`, style sheet or link survives; only data pictures within the pixel cap), once: what it asks for (text, elements, path data, with the copies `<use>` makes) is counted before it is read and capped, and one too large to write as SVG (nodes, points, filters, bytes, or not well-formed once written) is kept as a PNG; drawn into a PDF as vector by krilla-svg (as a capped PNG when it has filters). The caps are constants next to `MAX_RASTER_PIXELS` in `render` |
| `common/` | Shared code with no dependency on any other module: geometry, text patterns (caption labels), parallel execution |
| `analysis/` | The analysis stages, orchestrated by `analysis/pipeline.rs` |
| `model/` | `Document` and its sub-types: the output of analysis, the input of HTML output, and also JSON |
| `render/` | HTML output: a `Renderer` for each mode, the `assets` image store (pictures drawn from the pages of the PDF, or stored in the document for a Word input: `Pictures`, PNG, JPEG or SVG — an SVG is always an `<img>`, never inline SVG), shared HTML parts |
| `render/docx/` | Word output: `body` (blocks → `word/document.xml`), `omml` (`MathNode` → Office Math), `table` (Word tables), `parts` (styles, relationships, page); the package is written by `render/zip.rs` (a minimal ZIP writer shared with EPUB). Pictures are placeholders drawn afterwards on several threads, as in `assets`; an SVG picture is written as Word writes one, a PNG drawn of it with the SVG beside it (`asvg:svgBlip`); `to_docx_with_images` also returns them as files |
| `render/inline_math.rs` | Formulas inside running text: runs of math-font spans, their scripts and the symbols between them, rebuilt as `MathNode` trees; shared by the medium and friendly modes (MathML), Word (Office Math), Markdown, LaTeX and Typst (`$…$`) |
| `render/notes.rs` | Added notes (`Span::note`, the `<note>` tag of the editable JSON) in the outputs: the HTML modes, Markdown and EPUB number a copy of the document (the call's text becomes its number, its link the note's anchor); the call and the asides of the flowing modes and EPUB. Word, LaTeX and Typst let the format number them (footnotes). `footnotes_attached` makes the footnote blocks with a single call (Word, EPUB, PDF) notes of their calls, for Word, LaTeX and Typst |
| `render/markdown/` | Markdown output: `inline` (spans → emphasis, code, links; escaping), tables as pipe tables (friendly: header rows joined, spanning cells repeated) or, in medium, HTML tables when cells span. Pictures go through the `assets` store, as files or data URIs |
| `render/formula.rs` | What the writers of formulas as text (LaTeX, Typst) read alike from a `MathNode`: accents, primes, stretchy delimiters, big operators, upright function names, and the matrices and `cases` a table between tall delimiters is; each writer names them in its own syntax |
| `render/runs.rs` | Spans as the markup of a text format (LaTeX, Typst): cut at the calls of notes, neighbouring spans of the same style joined, the spaces around a link kept outside it (`RunMarkup`), and the names of labels |
| `render/sequence.rs` | The order in which LaTeX and Typst write the blocks (`steps`: code runs as one listing, title and author runs, the abstract's heading, an equation with its stray number, contents runs, the arXiv link after the front matter), and the author cards they set alike (`write_authors`) |
| `render/latex/` | LaTeX output: `math` (`MathNode` → LaTeX, also used by Markdown for `$$`), `text` (spans → escaped text, `\textbf`, links, symbols in math mode), `preamble.tex` (packages and the commands for pictures and wide tables), tables as `tabular`. Pictures go through the `assets` store, always as files; characters pdfLaTeX has no glyph for are declared in the preamble |
| `render/typst/` | Typst output: `template.typ` (the layout: the functions `kakemono-…` that the text calls, the `kakemono` show rule with the article and the book layouts), `mod.rs` (the settings at the top of the file, the layout chosen from the document, blocks as calls of the template, tables as `table`s with each cell at its place), `math` (`MathNode` → Typst math: tokens apart, delimiters and separators escaped, words as strings), `text` (spans → escaped markup, `#emph`, links, notes as `#footnote`; a mark after each call becomes `;` where the text would continue it). Pictures go through the `assets` store, always as files |
| `render/epub/` | EPUB output: the blocks written by the shared flow (`render_flow`, with the friendly-mode markup) and turned into XHTML (`xhtml`: closed void elements, MathML namespace, links between chapters), one content document per top-level section; `package` writes the package document, the navigation document and the NCX; `epub.css` is the style sheet. Pictures go through the `assets` store and are stored in the book |
| `render/json/` | Editable JSON output: `markup` (spans ↔ one string with HTML-like tags; a formula in running text becomes `<m id>` and comes back as its original spans), `mod.rs` (the file: source, blocks with their section, layout, LaTeX/MathML, pictures through the `assets` store), `apply` (an edited file back into the `Document`: only text that differs from what would be written is applied; edited blocks lose their per-line text — `hlines`, `line_texts`, a table's `inner_spans` — which is how hard-95 knows to flow them in their box); read-only fields derived from the text: `plain_text` (`plain`: no tags, formulas as `$LaTeX$`), `citation` (a reference's authors, title, venue, year, identifiers, by the shapes of the usual citation styles), `front` (`meta`: authors with affiliations, e-mail addresses and notes, arXiv category, date, DOI, keywords, abstract) |
| `render/chunks/` | Chunks for retrieval (RAG): `mod.rs` groups the blocks by section (text as the JSON's `plain`, tables as rows), cuts a block that does not fit at sentence ends, heads each chunk with its section path; `tokens` counts tokens, by an estimate fitted on two tokenizers or by a real tokenizer (Hugging Face `tokenizers`, OpenAI's through `tiktoken-rs`; optional cargo features) |
| `python/` | The Python package (separate crate, built with maturin): `src/lib.rs` binds `Paper` and its analysis as one `Document` class with PyO3 (analysed once, written in any format) and releases the GIL, and binds `render-svg` and `formula`; `kakemono/__init__.py` adds paths and arXiv ids |
| `raster/` | Formulas from pictures (`kakemono formula`): pieces of ink recognised against the TeX fonts (`tex`, `templates`, `shape`, `recognize`), then rebuilt by `analysis::math` |

Dependencies run in one direction only:

```
main → paper → analysis → backend → common
              → word    → analysis (the linking of `semantics`)
              → excel   → word (`zip`, `docx` for parts and relationships, `xml`), chart, model
              → chart   → word (`docx::parse`, `xml`), excel (`numfmt`), model
              → epub    → word (`ir`, `build`, `zip`, pictures)
              → html    → epub (`html`), source (`web`)
              → layout  → word (`fmt`), backend (reading PDF lines), render (HTML escaping), model (a workbook)
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
| Adapter | `backend/` | Only one place depends on hayro (and resvg, usvg, krilla-svg for SVG pictures) |
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
hashes of 8 real papers, in all four modes, in Word, in Markdown, in LaTeX, in Typst, in EPUB, in editable JSON, in chunks and in `--json`, with `tests/data/snapshots`; one character of
difference turns the test red. When you change the output on purpose, run
`KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --test real_papers` and commit the snapshots with the change.
