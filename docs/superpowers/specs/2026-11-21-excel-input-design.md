# Excel input (`.xlsx`) — design

Date: 2026-11-21. Status: approved by the user (2026-11-21).

## Intent

Accept an Excel workbook as input to `kakemono convert`, next to PDF and Word,
so that a spreadsheet can be:

- **read comfortably** as a document (HTML `friendly`, Markdown, docx, EPUB,
  LaTeX) by people who do not like reading it in Excel's grid;
- **fed to AI**: chunks (`.jsonl`) for RAG and the editable JSON, with the
  context a model needs (sheet name, column headers) in every piece.

## Goals

- Read `.xlsx` (Office Open XML spreadsheet; also `.xlsm`, same format)
  directly into a `model::Document`, with a reader of our own: full control of
  what each sheet becomes, no shared code path with the Word builder.
- Every output that does not need pages works: `friendly` HTML, md, docx, tex,
  epub, json (+ `--content`), chunks.
- Cells show the value **as Excel displays it** (number formats, dates,
  percentages), not the raw stored number.
- Chunks of a big table stay usable on their own: a table is cut only between
  rows, and every chunk repeats the table's header rows. (This also helps
  tables of PDFs and Word documents.)

## Non-goals (for now)

- `.xls` (BIFF8 binary, Excel 97–2003). Possible later on top of `cfb`; a
  separate piece of work.
- `.ods`, `.csv`.
- Page layout of a spreadsheet: `hard`, `hard-95`, `medium` keep the existing
  rule for documents without pages (`InputFormat::has_pages` is false → the
  `friendly` page is given). No `-o x.pdf` for a workbook.
- Charts, pictures, pivot tables, comments, conditional formatting, cell
  colours, form controls. Charts/pictures may come later.
- The look of forms (Japanese 方眼紙 sheets, 様式, flows drawn with shapes):
  v1 extracts their **text** in reading order (cells and text boxes), it does
  not rebuild the form (decided with the user).
- Recomputing formulas: the value cached in the file is used.

## Anti-goals (failure even if it "works")

- A chunk of table rows without the column names: useless for retrieval.
- Raw serial numbers where Excel shows a date (`45291` instead of
  `2024-01-01`), or `0.125` where Excel shows `12.5%`.
- Huge HTML/Markdown full of empty rows and columns (the used range of a sheet
  is often much bigger than its data).
- A crash, hang or memory blow-up on a large or hostile workbook (zip bomb,
  million-row sheet): same limits spirit as the Word reader.

## Constraints

- Public API: `InputFormat` is a public, exhaustive enum. Adding `Xlsx` is a
  breaking change → commit `feat!:`, and the Python package must handle the
  new name. (Alternative left to the builder: mark it `#[non_exhaustive]` in
  the same breaking release.)
- JSON is a published format: a new `input format` value (`"xlsx"`) is a new
  value, not a changed field; no `FORMAT_VERSION` bump unless a field changes
  meaning.
- New thresholds go in a documented config struct (`src/config.rs`), with unit
  suffixes; no magic numbers.
- No new heavy dependency: reuse `src/word/zip.rs` (zip reading with limits)
  and `roxmltree` (as `src/word/xml.rs` / `docx::parse` do).
- Test data: a small `.xlsx` made by us (LibreOffice or a script), committed
  under `tests/data/excel/`, with a snapshot like `word-sample.txt`.

## Approaches considered

1. **Through the Word intermediate form** (`word::ir` → `word::build`): least
   code, every output at once; but the Word builder decides what blocks look
   like, with Word-specific rules (one-cell tables unwrapped, numbered-equation
   tables, captions).
2. **Directly into `Document`** — **chosen by the user**: a reader of its own
   that builds blocks itself. More code (the grid → `Table` with spans), but
   full control: splitting a sheet into several tables, header detection,
   what a sheet heading says, all decided for spreadsheets only.
3. Each row as a text record (`Name: An, Age: 30`): best for RAG alone, poor
   for display — rejected, goes against the "read it as a document" goal.

## Design

### Module

New `src/excel/` (sibling of `src/word/`, public like `word` with
`detect`/`read`):

- `mod.rs`: `detect(data) -> bool` (zip with `xl/workbook.xml`, found via
  `_rels/.rels`), `read(data, source) -> Result<Document>`.
- `workbook.rs`: sheets in order (name, state hidden/visible, part path via
  `xl/_rels/workbook.xml.rels`), `date1904`, defined title from
  `docProps/core.xml`.
- `strings.rs`: shared strings, including rich text runs (bold/italic kept as
  `Span` style bits).
- `styles.rs`: `cellXfs` → number format id and font; custom `numFmts`;
  `fonts` → bold/italic, so that a cell bold by its style shows bold (and
  bold can mark a header row).
- `numfmt.rs`: formatting a number with an Excel format code: built-in ids
  (0–49 as Excel defines them), custom codes — sections `pos;neg;zero;text`,
  `0`, `#`, `,` thousands, decimals, `%`, scientific, literal text and quoted
  strings, `[colour]`/`[condition]` ignored, dates/times (`yyyy mm dd hh mm
  ss AM/PM`, `m` as minute after `h`). Unknown parts → fall back to the
  general format. This is the piece that most needs unit tests.
  **Dates and times are always written ISO** (decided by the user):
  `2024-01-31`, `14:05`, `2024-01-31 14:05`, whatever the built-in or custom
  date code (built-in ids 14–22, 45–47, and custom codes made of date/time
  tokens). Numbers follow the cell's code (`12.5%`, `1,234.50`, with `.` as
  decimal and `,` as thousands separator); a currency symbol is kept only when
  it is written in a custom code (`"¥"#,##0`, `[$€-407]`); built-in currency
  ids 5–8 give the number without a symbol.
- `sheet.rs`: a streaming read of `xl/worksheets/sheetN.xml`: cells
  (`r`, `t` = `s`/`str`/`inlineStr`/`b`/`e`/`n`/`d`, `s` style), `mergeCells`,
  hyperlinks (via the sheet's rels → `Span::href`), hidden rows/columns.
- `build.rs`: grid → blocks.

### What a workbook becomes

- `meta.title`: core properties title, else none (the CLI's source name is
  already recorded in `meta.source`).
- Each **visible** sheet, in workbook order (hidden and very hidden skipped):
  - a `Heading { level: 1 }` with the sheet name;
  - its data as one or more tables: the used cells are trimmed of empty
    outer rows/columns; then the sheet is split into **regions** separated by
    runs of fully empty rows at least `region_gap_rows` long (config, default
    2), **and** by fully empty columns at least `region_gap_cols` wide
    (default 1) — a 2-D split (connected blocks of cells, then their bounding
    boxes; *builder's call made by the designer*: side-by-side tables are
    common) — regions in reading order (top to bottom, then left to right);
  - each region is read top to bottom into, in this order:
    - **caption rows**: the leading rows with a single non-empty cell (a
      title, a unit line such as `(in millions of dollars)`) → the table's
      caption (`Block::spans`, one line each);
    - **header rows** (see below);
    - **body rows**;
    - **note rows**: trailing rows with a single non-empty cell (`Source:`,
      `注:`) → paragraphs after the table;
  - a region with fewer than two columns of data (only single-cell rows: a
    form, a notes or cover sheet) becomes **paragraphs** instead of a table,
    one per row, cells of a row joined `label: value` style when a row has
    two cells;
  - text in shapes and text boxes of the sheet's drawing (`xl/drawings/`,
    `a:t` in `xdr:sp`) becomes paragraphs after the sheet's cells, in their
    anchor order (top-left cell, row then column). Pictures, charts,
    connectors and form controls (checkboxes) are left out in v1;
  - an empty sheet gives nothing (no heading either).
- Hidden rows and columns are left out.
- Merged cells → `colspan`/`rowspan` on the top-left cell; the others dropped.
  `centerContinuous` (centre across selection, common in Japanese statistics
  instead of a merge) → the text's cell spans the empty cells to its right
  that have the same alignment.
- **Header rows** — decided in this order, first that applies wins:
  1. `--header-rows N` (CLI) / `ExcelOptions::header_rows` given by the user.
  2. What the file declares, mapped onto the region: an Excel Table's
     `headerRowCount`; the first row of an `autoFilter` range; frozen rows
     (`pane ySplit`); print titles (`_xlnm.Print_Titles` rows). The rows of the
     region down to the declared last header row, minus the caption rows.
  3. Column types: the first row (after the captions) whose non-empty cells
     have, in at least `header_type_match_ratio` (config, prototype: 0.75) of
     them, the majority type (number / date / text / boolean) of their column
     in the next `header_lookahead_rows` (config, prototype: 20) rows is the
     first body row; the rows before it are the header (0 to many). A merge
     reaching into the header extends it down to the merge's last row.
  4. Otherwise (a table of text only, nothing declared): the first row.
  A region of one row has no header.
  The prototype of this rule on the 99-file corpus got the stats tables right
  (multi-row headers of e-Stat-style Japanese tables, ONS, Destatis), and the
  all-text Japanese spec sheets are covered by their declared freeze/print
  titles.
- Column alignment: right for a column whose non-header cells are mostly
  numbers (`numeric_column_ratio`, config), left otherwise.
- Cell text: formatted value; shared and inline strings **without furigana**
  (`rPh` runs are skipped, nearly every Japanese workbook has them; `phoneticPr`
  ignored); rich-text runs keep bold/italic/sub/superscript; booleans `TRUE`/`FALSE`; errors as written
  (`#N/A`); formula cells use their cached value (`<v>`), empty when absent.
- Limits (config): maximum rows and columns read per sheet; past them the
  reader stops that sheet and adds a paragraph saying how many rows were left
  out. Zip/XML limits as the Word reader.

### Wiring

- `InputFormat::Xlsx` (`name() = "xlsx"`, `has_pages() = false`).
- `Paper::from_bytes`: check `excel::detect` before `word::detect` (both are
  zips; each looks for its own main part). New `Source::Sheet(Box<Document>)`
  (no file kept: there is no layout to redo). `analyze_with`, `to_html`,
  `to_pdf` (error: "a workbook has no page layout") handled.
- CLI: help texts list `.xlsx`; `inspect` works on the document.
- Python: the format name passes through; README/ARCHITECTURE/CLAUDE.md/
  `docs/` updated.

### Chunks: tables cut between rows

(Shipped as its own commit, before the Excel reader, with its own snapshot
review. A table with no header row and no caption is still cut between rows,
with only the section path repeated.)

In `src/render/chunks/`: a table block that does not fit in one chunk is cut
between rows (never inside a row unless one row alone is too big — then
between words as today). Each piece starts with the section path, the caption
if any, and the header rows (` | `-joined), then as many body rows as fit.
Applies to every input format, so PDF/Word chunk snapshots will change where a
table was cut: intended, to be reviewed and updated.

## Corpus

`tests/data/excel-corpus.tsv` (URL, publisher, category, note), like
`tests/data/word-corpus.tsv`: about 100 public workbooks (UK ONS, US Census,
BEA, OMB, Destatis, INSEE, Vietnam NSO, SEC EDGAR, Smartsheet templates; e-Stat,
stat.go.jp, 厚労省, デジタル庁, IPA, 日本政策金融公庫, city 様式), downloaded by a
script, never committed. A crash/timeout run over all of it, and a report
(sheets → tables, header rows found, by declaration or by types) to compare
before/after a change of the rules.

## Tests

- Unit tests: number formats (dates incl. 1904 system, percent, thousands,
  negative sections, text section), grid trimming, region split, header rule,
  merges.
- `tests/data/excel/sample.xlsx` (several sheets: a plain table, a merged
  header, dates/currency, two regions, a hidden sheet, a hyperlink) +
  snapshot `excel-sample.txt` of every output.
- Chunks: a long table is cut at rows and every chunk carries the header.
- Malformed/huge input: does not panic (`scripts/word-crash-test.py`-style).

## Left to the builder

- Exact file split inside `src/excel/` and names of config fields.
- Streaming XML vs. `roxmltree` per sheet, within the memory limits.
- How the sample `.xlsx` is produced (LibreOffice, or written by a small
  script), as long as it is reproducible and committed with its source.
