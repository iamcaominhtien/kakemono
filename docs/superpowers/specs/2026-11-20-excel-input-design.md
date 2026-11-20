# Excel input: design

Date: 2026-11-20. Status: draft, for review.

## Intent

kakemono reads Word documents and produces every output from them. The next input is
Excel: `.xlsx` (Office Open XML) and `.xls` (Excel 97–2003, BIFF8). The owner wants a
**full reading**, not a data dump:

- every value as Excel shows it, so that the structure outputs (Markdown, JSON, chunks,
  friendly HTML, Word, LaTeX, EPUB) show what a person sees in Excel;
- every formatting and print property, so that the PDF and the page modes (`hard`, `hard-95`,
  `medium`) look like what Excel prints.

This follows the Word work (PR #16), where the same split exists: `src/word/` reads the
structure, `src/word/fmt/` + `src/layout/` read the formatting and lay out pages, checked
against Word's own PDFs.

## Goals

1. `kakemono convert book.xlsx|book.xls -o out.<any format>` works for every output format
   and every HTML mode, like a Word file does today.
2. Cell text is the text Excel displays: number formats applied (sections, conditions,
   colours, dates in both date systems, fractions, scientific, `@`, `_` padding, `*` fill,
   `[$-…]` locale tags), with the decimal and grouping separators of a chosen locale.
3. The PDF output is the workbook as Excel prints it ("Save as PDF", entire workbook): same
   page size and orientation, same page breaks, same cells on each page at the same places,
   headers and footers, borders, fills, fonts, merged cells, text overflow and clipping,
   hidden rows/columns left out, conditional formatting drawn, pictures.
4. Fidelity is measured against PDFs made by Excel, with a script and minimums in CI, like
   `scripts/word-fidelity.py`.
5. No panic and no timeout on any input file (crash test on a large public corpus).

## Non-goals

- Recalculating formulas. Excel stores the last computed value of each formula; kakemono shows
  that value. A file written by a tool that stores no value shows the formula's cached value
  if any, else nothing (the formula text goes to the JSON output only).
- Macros/VBA, pivot table refresh, external links, data connections, Power Query.
- Writing or editing spreadsheets (no `.xlsx` output).
- Other spreadsheet formats in this work: `.xlsb`, `.ods`, `.csv`, Excel 5/95 (BIFF5) and
  older `.xls`. They may come later; the design should not block them.
- Encrypted or password-protected files: refused with a clear error (as `.doc` does).
- Charts in the first phases (see Phases): a chart is a later addition; until then the PDF
  leaves its area empty and the structure outputs omit it.

## Anti-goals

- Tuning the layout to LibreOffice. LibreOffice's page breaks differ from Excel's on 13 of
  the 30 samples (for example 8 pages against Excel's 14); its output is never a reference.
- A second dependency stack: no `calamine`, no `umya-spreadsheet`. The readers reuse what the
  Word reader already has (`src/word/zip.rs`, `roxmltree`, `cfb`) and the layout reuses
  `src/layout/` (fonts, shaping, display list, PDF writer).
- Treating a sheet as a Word table and reusing the Word flow (`src/layout/flow.rs`): Excel's
  rules (no wrapping by default, text spilling into empty neighbours, clipping, scaling to
  fit, row/column page breaks, print titles) are not Word's.

## Constraints

- Rust only, no network, no new heavy dependency.
- Public API: `InputFormat` (`src/paper.rs`) is a public enum without `#[non_exhaustive]`;
  adding `Xlsx` and `Xls` is a breaking change, so the commit that adds them is `feat!:`.
  New public items follow the rules of CLAUDE.md ("Public API and packages").
- Format detection: `.xlsx` and `.docx` are both ZIP packages; `.xls` and `.doc` are both
  compound files, and `word::doc::is_doc` accepts any compound file today. Detection must look
  inside (`xl/workbook.xml` through the package relationships; a `Workbook` or `Book` stream)
  and check Excel before Word.
- Repository conventions: thresholds of the layout are documented constants or config fields
  with units in their names; snapshots for every output; Conventional Commits; SonarQube
  issues fixed in each PR.
- Licences: only files we made may be committed. The 30 samples and the PDFs Excel made from
  them are ours. Public corpus files are listed by URL and sha256, downloaded, never committed.

## Approaches considered

| | Approach | Trade-off |
|---|---|---|
| **A (chosen)** | Own readers for `.xlsx` (ZIP + XML) and `.xls` (BIFF8 on `cfb`) into one intermediate form with every property; two branches from it: structure (`Document`) and layout (a grid paginator on top of `src/layout/`). | Most work; no new dependency; full control of every property, as for Word. |
| B | `calamine` for values, own code for formatting. | Fast start on the structure outputs, but `calamine` gives no formatting for `.xls`, so a BIFF8 reader is needed anyway: two readings of the same file for little gain. |
| C | Turn each sheet into a Word table and reuse the Word layout. | Most reuse, but Excel's print rules differ from Word's (see Anti-goals); fidelity would stay low. |

The owner chose A.

## Design

### Modules

A new top-level module `src/sheet/`, parallel to `src/word/`:

- `mod.rs`: `SheetFormat { Xlsx, Xls }`, `detect(data)`, `read(data, format, source) ->
  Document`, `read_book(data, format) -> Book` (the formatted workbook for the layout).
- `xlsx/`: the OOXML reader. Parts: `workbook.xml` (sheets, states, date system, defined
  names `_xlnm.Print_Area` / `_xlnm.Print_Titles`), `sharedStrings.xml` (with rich runs),
  `styles.xml` (number formats, fonts, fills, borders, cell XFs, cell styles, dxfs for
  conditional formats, indexed/theme colours with tint), `theme1.xml` (theme colours and
  fonts), each `worksheets/sheetN.xml` (sheetFormatPr, cols, rows with heights and hidden/outline
  flags, cells with type/style/value/formula, mergeCells, conditionalFormatting, pageMargins,
  pageSetup, printOptions, headerFooter, rowBreaks/colBreaks, sheetView RTL, drawing and
  legacy-drawing links, tableParts), drawings (pictures with one-cell/two-cell/absolute
  anchors; chart anchors recorded for later), tables (style, header row, banding), comments.
- `xls/`: the BIFF8 reader on the `Workbook` (or `Book`) stream: BOF/EOF, CODEPAGE,
  DATEMODE (1904), SST + CONTINUE, FONT, FORMAT, XF, STYLE, PALETTE, BOUNDSHEET, ROW,
  COLINFO, DEFCOLWIDTH, DEFAULTROWHEIGHT, LABELSST, LABEL, NUMBER, RK, MULRK, BOOLERR,
  FORMULA (+ STRING for text results), BLANK/MULBLANK, MERGEDCELLS, NAME (print area and
  titles), HPAGEBREAKS/VPAGEBREAKS, SETUP, margins, HCENTER/VCENTER, PRINTGRIDLINES,
  PRINTHEADERS, HEADER/FOOTER, WSBOOL (fit to page), CF/CFRULE, MSODRAWING/OBJ for pictures
  (reusing the OfficeArt code of `src/word/doc/` where it fits), FILEPASS (refuse).
- `ir.rs`: the intermediate form both readers fill (below).
- `numfmt.rs`: the number-format engine (format code + value + locale → displayed text and
  colour), with the built-in formats 0–49 and the locale-dependent ones (14, 22, ...).
- `build.rs`: intermediate form → `Document` (structure outputs).
- `layout/` (or `src/layout/sheet/`; builder's choice): the print layout → `display::Page`
  list → `layout::pdf::write`.

### Intermediate form (`ir.rs`)

One `Book` with: date system; styles (fonts, fills, borders, number formats, alignment,
protection ignored) indexed by XF; theme colours and fonts; default (Normal) font and the
workbook's maximum digit width (computed from the font, see Layout); sheets. Each sheet:
name, state (visible/hidden/veryHidden), kind (worksheet; chart sheets and dialog sheets
noted and skipped), default column width and row height, columns (width, hidden, style,
outline level), rows (height, custom height flag, hidden, style, outline level), cells
(value: number, text, rich text, bool, error, empty-with-style; the formula text when there
is one; style index), merged ranges, conditional formats (rules in priority order with
their dxf or colour scale / data bar / icon set), print setup (paper size id or explicit
size, orientation, scale, fit-to width/height, first page number, page order, margins,
centering, gridlines, headings, print area(s), print title rows/columns, manual row/column
breaks), header/footer strings (odd, even, first, with the flags), right-to-left view,
pictures (bytes, anchor), charts (anchor only for now), comments, table parts.

Values keep full precision; nothing is formatted in the readers.

### Number formats and locale

`numfmt.rs` formats a value with an Excel format code as Excel does, given a `Locale`
(decimal separator, grouping separator, date order and month/day names, currency for
locale-dependent built-ins). The locale comes from a new option (`--locale`, default
`en-US`; also in the library and Python options). The reference PDFs of the samples were made
on a machine with comma decimals and dot grouping (Vietnamese-style), so the fidelity script
passes that locale for them; the corpus manifests record each reference's locale.

`General` depends on the column width (Excel shortens numbers to fit, then shows `#####` /
scientific); the engine takes an optional available width to do the same in the layout.

The golden tests for this engine come from the samples: `L2-05-number-formats` prints 30
formats × 7 values in Excel's PDF, so each displayed string is a test case.

### Structure outputs (`build.rs`)

- Each visible worksheet becomes a section: a level-1 heading with the sheet name, then its
  used range as one `Table` (header rows: the print-title rows if any, else none; merged cells
  as row/col spans), with empty leading/trailing rows and columns trimmed. Hidden sheets,
  hidden rows and hidden columns are left out.
- A sheet whose used range is a few text cells over many columns (a title block, notes)
  stays a table; splitting a sheet into several tables (blank-row-separated blocks) is left to
  the builder as a later refinement, behind a documented threshold.
- Cell text is the displayed text (number formats + locale). Rich text keeps bold/italic and
  super/subscript spans where the model has them.
- Pictures become `Document::pictures` (as for Word, `ImageSpec::embedded`); comments become
  notes attached to their cell's table, or are dropped in formats that cannot hold them.
- The editable JSON gains what a workbook needs (sheet, cell reference, formula text) as new
  fields only, so no `FORMAT_VERSION` change.

### Print layout

A grid paginator, per visible worksheet in workbook order (a hidden or empty sheet prints
nothing; an all-empty workbook gives one blank page, as Excel refuses to print it — the builder
picks the behaviour and documents it):

1. **Sizes.** Column widths in Excel's units are converted to pixels with the maximum digit
   width of the Normal font (`pixels = trunc((256 × width + trunc(128 / mdw)) / 256 × mdw)`,
   the Office Open XML rule), then to points. Row heights come from the file when stored;
   otherwise from the largest font in the row and, for wrapped cells, the number of wrapped
   lines. The mdw is measured from the font with the existing `FontBook`; whether Excel for Mac
   (the reference) and Excel for Windows round to the same pixels is checked on the samples.
2. **Printable area.** Paper size minus margins; the print area or the used range (including
   cells that only have a fill or border, and spilled text); scale from `scale` or computed
   from fit-to-width/height (largest scale ≤ 100% that fits, Excel's 10% floor).
3. **Breaks.** Rows then columns cut where the next row/column would not fit, manual breaks
   first; print-title rows/columns repeated on each page and counted in the space; page order
   down-then-over or over-then-down; centering.
4. **Drawing.** Fills, then gridlines (if printed), then borders (Excel's border precedence
   between neighbours), then text: alignment (horizontal incl. fill, justify, distributed,
   centre-across-selection; vertical; indent; rotation; wrap; shrink-to-fit), overflow into empty
   neighbours (left, right, both for centred), clipping at non-empty neighbours, `####` for
   numbers that do not fit; merged cells drawn once; conditional formats applied (fills, fonts,
   colour scales, data bars, icon sets); pictures at their anchors; row/column headings if
   printed.
5. **Headers and footers.** Excel's codes (`&L &C &R`, `&P &N &D &T &F &A &Z`, `&"font,style"`,
   size, `&B &I &U &S &X &Y`, `&&`), odd/even/first, scaled with the sheet or not per
   `scaleWithDoc`, aligned with margins per `alignWithMargins`.
6. **Output.** `display::Page` items → `layout::pdf::write`, so text stays selectable and fonts
   are embedded as for Word. The page modes reuse the Word path: `hard`/`hard-95` analyse the
   PDF, `medium` shows each sheet as one continuous HTML grid with its formatting.

### Integration

- `InputFormat::{Xlsx, Xls}`; `Paper::from_bytes` detects them (before Word); a
  `Source::Sheet(..)` variant mirrors `Source::Word`; `to_pdf`, `word_layout_html` and the
  CLI handle it; the Python package exposes it through the same calls.
- `inspect` prints the workbook summary (sheets, used ranges, print setup) for debugging.
- Docs: a `docs/EXCEL-INPUT.md` like `docs/WORD-INPUT.md`; README and ARCHITECTURE updated;
  CLAUDE.md gains an "Excel input" section like "Word input".

## Test data

1. **Samples (committed):** `tests/data/excel/` holds the 30 graded samples as saved by Excel
   for Mac 16.115 (`*.xlsx`), 8 of them also saved as `.xls`, the 30 PDFs Excel made of them,
   the generator script (`make_samples.py`, openpyxl) and a note of the environment (Excel
   16.115 on macOS 26.5.1, no default printer, comma-decimal locale; every sample sets its
   paper size). About 4.5 MB; `.gitignore` gets an exception for these PDFs. They give:
   snapshots of every output, the number-format golden tests, and the fidelity minimums.
   The originals are also in Drive (`kakemono/kakemono-excel-corpus/samples`).
2. **Excel-made PDFs found online (downloaded):** pairs of a public workbook and a PDF whose
   producer is Excel, from the open-data portals (gov.uk, data.gov.au, open.canada.ca), kept
   only when the PDF's text matches the cells; listed in `tests/data/excel-corpus.tsv` with
   URLs, sha256 and the PDF's locale. Expected: about 100–200 pairs (not yet verified).
3. **Crash corpus (downloaded):** the 694 public workbooks of
   `kakemono/kakemono-excel-corpus/manifest.tsv` (576 `.xlsx`, 118 `.xls`, open licences), plus
   more `.xls` from public corpora if needed; every output of every file, no panic, no timeout.

### Fidelity measure

`scripts/sheet-fidelity.py REF.pdf OURS.pdf`: per workbook, page count equal; per page, page
size equal and the set of words with their positions (tolerance a fraction of the font size)
matched. Totals: share of workbooks with the same page count, share of words on the same page,
share of words placed within tolerance. CI minimums start at what the first working layout
reaches and only go up.

## Phases

1. `.xlsx` reader + intermediate form + number formats + structure outputs (every non-PDF
   format, `friendly` mode), with the samples' snapshots and the number-format tests.
2. Print layout: PDF output and `hard`/`hard-95`/`medium` modes; fidelity script and CI
   minimums on the samples, then on the online pairs.
3. `.xls` reader (BIFF8) into the same intermediate form; the 8 `.xls` samples must give the
   same outputs as their `.xlsx` twins (except what BIFF8 cannot store).
4. Crash test on the corpus; charts (drawn from their cached values), and what the fidelity
   numbers show is missing.

Each phase is its own PR.

## Decided by the owner

- Full reading (values and formatting), both branches (structure and print layout).
- Approach A.
- `.xlsx` and `.xls`.
- Reference PDFs from Excel itself (the owner's Excel for Mac), never LibreOffice.
- Test samples graded from simple to very complex, made by us, saved and exported by Excel.

## Calls made in this design (the owner may overrule)

- Default locale `en-US`, with a `--locale` option.
- Charts after the first three phases.
- One table per sheet in the structure outputs to start with.
- `.xlsb`, `.ods`, `.csv` out of scope for now.
- The sample PDFs are committed (they are ours), with a `.gitignore` exception.

## Left to the builder

Module layout below `src/sheet/`, the names of types and options, how the BIFF8 code shares
OfficeArt parsing with `src/word/doc/`, the exact word-matching tolerance of the fidelity
script, the order of work inside a phase, and every threshold (as documented constants).
