# Word input (`.docx`, `.doc`): research

Status: research only, nothing implemented yet. This note looks at how kakemono
could read Word documents next to PDFs, what was measured, and what is
recommended.

## The question

Today every input is a PDF: `Paper` holds a `PdfFile`, `backend/` extracts
glyphs, and `analysis/` rebuilds the structure (lines, columns, headings,
formulas, tables) from their positions. A Word file already *has* that
structure written down (paragraph styles, list numbering, tables, Office Math),
but it has no pages: page layout only exists once a word processor lays it out.

So there are two ways in:

| | A. Word → PDF → existing pipeline | B. Read the Word file directly |
| --- | --- | --- |
| How | `soffice --headless --convert-to pdf`, then the PDF as today | Unzip, read `word/document.xml` and friends, build a `Document` |
| Outputs | All, including `hard`, `hard-95`, `medium` | The flowing ones: `friendly`, Word, Markdown, LaTeX, EPUB, JSON. Not the page modes (no page geometry) |
| Structure | Guessed again from glyph positions, by heuristics tuned on arXiv papers | Exact: headings, lists, tables, footnotes, links are written in the file |
| Formulas | See the measurements: lost here | Office Math (OMML) maps almost one to one onto `MathNode` |
| Dependencies | LibreOffice at run time (hundreds of MB), its fonts | None new: `flate2` is already a dependency, `roxmltree` already in `Cargo.lock` (through `resvg`) |
| `.doc` (Word 97–2003) | Same command | No: the binary format is too large to read by hand; convert to `.docx` first |

## Measurements (2026-11, LibreOffice 24.2, this container)

Two test files, both free of licence problems:

- `note.docx`: written by pandoc from Markdown (title, author, abstract,
  headings, inline and display formulas, a numbered and a bulleted list, a
  table without rules, a footnote, a reference). `note.doc` is the same file
  saved by LibreOffice as Word 97.
- `ligo.docx`: kakemono's own Word output of the committed LIGO paper
  (`tests/data/1602.03837v1.pdf`): 29 formulas, 5 pictures, 1 table, 118
  references, 669 links.

| Check | Result |
| --- | --- |
| Conversion time (`soffice --headless`) | 0.6 s (`note.docx` → PDF), 1.7 s (`ligo.docx` → PDF), 1.4 s (`.docx` → `.doc`, including start-up) |
| Formulas through route A | **All lost.** `pdftotext` gives "Let be convex and -smooth": LibreOffice without its Math module (`libreoffice-math`, not installed here and the package mirror was unreachable) drops Office Math silently. With the module, formulas come out as OpenSymbol glyphs, which `analysis::math` is not tuned for (not verified) |
| Table without rules through route A | Broken: the header row became paragraphs, two columns became references |
| Lists through route A, from `.doc` | The second number became "1." again; bullets became "�" |
| Headings, title, author, abstract through route A | Correct on `note` |
| What a `.docx` holds (route B) | `note.docx`: 4 `m:oMath`, 2 display formulas, 1 table, 1 real footnote, 4 numbered paragraphs, styles `Title`, `Author`, `Heading1/2`, `TableCaption`. `ligo.docx`: 29 `m:oMath`, 5 PNG pictures, 1 table, styles `Title`, `Abstract`, `Heading1`, `Caption`, `Bibliography` |
| `.doc` → `.docx` (LibreOffice) | Paragraph styles and numbering kept; formulas lost (same missing module) |

Conclusion of the measurements: route A turns a well-structured file into a
less structured one and then guesses the structure back, and loses formulas
unless the installation is complete. It is the only way to the page modes,
though.

## Recommendation

Route B as the main path, route A only as an optional bridge.

1. **Phase 1: `.docx`, flowing outputs.** A `docx` input module that builds a
   `Document` directly. `friendly`, Word, Markdown, LaTeX, EPUB and JSON work;
   `hard`, `hard-95` and `medium` refuse a Word input with a clear error (or
   fall back to `friendly`, to be decided).
2. **Phase 2: `.doc`.** Recognised by its signature (`D0 CF 11 E0`, a
   compound file) and converted to `.docx` with LibreOffice when `soffice` is
   on the `PATH`; otherwise an error that says what to install. Formulas
   need `libreoffice-math`.
3. **Phase 3 (optional): page modes for Word.** `--via-pdf`: LibreOffice
   writes a PDF and the existing pipeline runs on it, with the known losses.

### Where it fits

```
 PDF bytes ──► backend/ ──► analysis/ ─┐
                                       ├─► Document ──► render/ (unchanged, except pictures)
 DOCX bytes ─► docx input ─────────────┘
 DOC bytes ──► soffice ──► DOCX bytes
```

- `Paper::from_bytes` already takes bytes: it can tell the formats apart by
  their first bytes (`%PDF`, `PK\3\4` with a `word/document.xml`,
  `D0 CF 11 E0`) and keep a private `enum` of sources. No new constructor
  needed.
- The JSON output already records `source.format` (now always `"pdf"`) and
  the SHA-256: `"docx"` fits there, and `--content` works the same way.
- Where the input module lives: next to `backend/` (it replaces `backend/`
  *and* `analysis/` for Word), for example `src/docx_input/` with `zip`
  (reader), `styles`, `numbering`, `body`, `omml` (OMML → `MathNode`, the
  reverse of `render/docx/omml.rs`), `media`.

### What the reader maps

| Word | `Document` |
| --- | --- |
| Style `Title`, `Subtitle` | `Title` |
| Styles with `w:outlineLvl` / `Heading1..9` (following `w:basedOn`) | `Heading { level }` |
| Style `Author`, first paragraphs before the first heading | `Authors`, `Note` (with the existing front-matter rules where they apply) |
| `Abstract` style, or the paragraphs after a heading "Abstract" | `Paragraph { in_abstract: true }` |
| `w:numPr` + `numbering.xml` (counters computed from `lvlText`) | `ListItem { marker }` |
| `w:tbl` (`gridSpan`, `vMerge`, borders) | `Figure { kind: Table }` + `Table` |
| `w:drawing` → `a:blip r:embed` → `word/media/*` | `Figure` with an embedded picture |
| `Caption` style, `SEQ Figure` / `SEQ Table` fields | the caption and `label` of the nearest float |
| `m:oMathPara` / `m:oMath` alone in a paragraph | `Equation` with `math: Some(..)` |
| `m:oMath` inside a run | `MATH` spans (or a new inline-math carrier) |
| `w:footnoteReference` + `footnotes.xml`, endnotes | `Footnote { marker }` |
| `w:hyperlink` (relationship or `w:anchor`), bookmarks, `REF` fields | `Span::href`, `Block::anchor` |
| `Bibliography` style, paragraphs after "References" | `Reference { label }` |
| Run properties `b`, `i`, `vertAlign`, monospace fonts, `color` | `Span::style`, `Span::color` |
| `docProps/core.xml` `dc:title` | `Meta::title` when there is no title paragraph |

### What has to change outside the new module

- **Pictures.** `ImageSpec` is "a rectangle of a PDF page", drawn by
  `PageRenderer`; equations even require one (`Equation { image: ImageSpec }`).
  Word pictures are files inside the package. The `assets` store (and the Word
  writer's own picture placeholders) need a second kind of picture, the bytes
  of an embedded file, and an equation needs to be possible without a
  picture. `model` is public API, so this is a breaking change (`feat!:`,
  version 0.12).
- **`Paper::renderer`** returns a `PageRenderer`, which only exists for a PDF:
  it has to become fallible for a Word input (also a breaking change), or
  stay PDF-only with a documented panic.
- **Picture formats.** PNG and JPEG go through unchanged. Word also stores
  EMF/WMF (charts, MathType previews), for which there is no good Rust
  renderer: phase 1 keeps them as files where the output can (Word output) and
  otherwise writes the caption with a warning. SVG (with a PNG fallback in
  newer Office) can go through `resvg`. GIF/TIFF need the `gif`/`tiff`
  features of `image`.
- **Renderers that look at geometry**: the caption-above-or-below test
  (`line.bbox.cy() < spec.rect.cy()`) in `flow.rs`, `docx/body.rs`,
  `latex/mod.rs`, `markdown/mod.rs` should read the order from the document
  instead; `friendly` shows "N pages" and EPUB draws a cover from page 1,
  both meaningless for Word.
- **CLI and Python**: accept `.docx`/`.doc` paths; `inspect` works as is on
  the `Document`.

### Known limits of route B

- MathType formulas (OLE objects with a WMF preview), common in older
  papers, are not Office Math: only their preview picture is there.
- Text boxes, frames, columns and floating shapes have no reading order in the
  file other than their anchor: they are read where they are anchored.
- Tracked changes: read the document as accepted (`w:ins` kept, `w:del`
  skipped). Comments are skipped.
- Fields: their last computed result is read (`w:fldSimple`, the runs between
  `fldChar separate` and `end`), which is what Word shows.

### Tests

- Fixtures that may be committed: small `.docx` files written for the tests
  (by pandoc or by hand) and `ligo.docx`, which kakemono writes itself from
  the committed LIGO PDF. The last one gives a round trip: PDF → analysis →
  Word → Word input → `Document`; the text, headings, references and formulas
  should come back the same as the PDF analysis.
- Snapshots of each output, as for the PDFs.
- `.doc` tests only when `soffice` is on the `PATH`, like the LaTeX
  compilation test with `pdflatex`.

### Size

A rough estimate, from the existing modules of the same kind: ZIP reader
~150 lines, OOXML reader (styles, numbering, body, tables, footnotes, links,
media) ~1,500–2,500 lines, OMML → `MathNode` ~400–600 lines, pictures without
a page in `model`/`assets` and the renderers ~300 lines, plus tests. Several
pull requests: the picture change in `model` first, then the reader, then
`.doc`.

## Decisions to take before phase 1

1. Page modes with a Word input: refuse, or fall back to `friendly`?
2. The model change for pictures (breaking, 0.12) is acceptable?
3. `.doc`: depend on LibreOffice at run time (optional), or leave `.doc` out
   and document "save as `.docx`"?
