# Excel input (`.xlsx`)

Status: implemented. This note describes how kakemono reads an Excel workbook, what it becomes in
each output, and what it leaves out. The reader is `src/excel/`; the thresholds are the fields of
`ExcelConfig` (`src/excel/mod.rs`, re-exported as `kakemono::config::ExcelConfig`).

## Why

A spreadsheet is read in a grid that few people like to read in. Read as a document, its sheets
become headed sections with tables, which any output of kakemono can show: friendly HTML,
Markdown, Word, LaTeX, EPUB. The same structure feeds an AI: the editable JSON and the chunks for
retrieval keep the sheet name and the column names with the rows.

```bash
kakemono convert sales.xlsx -o sales.md                  # Markdown, one table per region of each sheet
kakemono convert sales.xlsx -o sales.jsonl               # chunks: every chunk of a table repeats its header rows
kakemono convert sales.xlsx --header-rows 1 -o sales.md  # say how many header rows the tables have
kakemono convert sales.xlsx                              # friendly HTML
```

The format is recognised from the file's bytes (a ZIP package whose main part is a workbook), as
for Word documents; `.xlsm` files are the same format. Excel 97–2003 files (`.xls`), `.ods` and
`.csv` are not read.

## What a workbook becomes

A workbook has no pages, like a Word document: `Document::pages` is empty, and the outputs that
need pages are not written (see [Outputs](#outputs)).

Each **visible** sheet, in workbook order, becomes a section. Hidden and very hidden sheets, hidden
rows and hidden columns are left out; an empty sheet gives nothing, not even a heading.

1. A level 1 heading with the name of the sheet.
2. The cells, cut into **regions**. The used cells are trimmed of empty outer rows and columns, then
   split where at least `region_gap_rows` (2) empty rows, or `region_gap_cols` (1) empty columns,
   separate two blocks of cells. Two tables side by side are two regions. Regions come in reading
   order, top to bottom, then left to right.
3. Each region is read from top to bottom as, in this order:
   - **caption rows**: up to `max_caption_rows` rows of one text cell right above the table (a
     title, a unit line such as `(in euros)`) are its caption; single-cell rows above those are
     paragraphs before the table;
   - **header rows** (below);
   - **body rows**;
   - **note rows**: the single-cell rows after the body (`Source: ...`, `Note: ...`) are
     paragraphs after the table.
4. The text of the **shapes and text boxes** of the sheet's drawing (`xl/drawings/`): Japanese
   workbooks often put text, flows and org charts in shapes. It becomes paragraphs after the cells,
   in the order of the anchors (row, then column). Pictures, charts, connectors and form controls
   are not read.

A region with fewer than two columns of data, such as a cover, a list of notes or a form of
label and value, becomes **paragraphs**, one per row; a row of two cells reads `label: value`.

### Forms

Forms drawn on a grid (Japanese 方眼紙 sheets, application forms) are not rebuilt: only the text
is kept, in reading order. A form whose cells hold more than a label and a value stays a sparse
table. `table_min_fill_ratio` (the share of the positions of a region that must hold a cell, below
which it becomes paragraphs) exists but is 0, which keeps every region a table: on the test
corpus, the fill of forms and prose sheets (0.29 to 0.50) is the fill of many sparse financial
tables (0.11 to 0.60), so no value tells them apart.

### Header rows

They are decided in this order; the first rule that applies wins.

1. **`--header-rows N`** on the command line (`header_rows` in Python, `ExcelConfig::header_rows`
   in Rust), for every table of the workbook; `0` means no header. It only applies to workbooks.
2. **What the file declares**: the `headerRowCount` of an Excel Table, the first row of an
   auto-filter, the frozen rows of the pane, the print titles. The header is the rows of the region
   down to the last declared header row, minus its caption rows.
3. **The types of the columns**: the first row whose cells, in at least
   `header_type_match_ratio` (0.75) of them, have the commonest type (number, date, text, boolean)
   of their column in the next `header_lookahead_rows` (20) rows is the first body row; the rows
   above it are the header, from none to many. A merge that reaches into the header makes the
   header go down to the end of the merge.
4. **Otherwise**, a table of text only with nothing declared: the first row.

A region of one row has no header. A column is right-aligned when more than `numeric_column_ratio`
(0.5) of its body cells are numbers or dates, left-aligned otherwise. Merged cells become cells
with a column or row span (the other cells of the merge are dropped); a text centred across a
selection spans the empty cells to its right.

### Cell text

The text is the value **as Excel shows it**, not the stored number:

- **Dates and times are always ISO**, whatever the cell's format: `2024-01-31`, `14:05`,
  `2024-01-31 14:05` (with seconds, `14:05:09`, only when the format shows seconds). The 1900 and
  1904 date systems are both read.
- **Numbers** follow the cell's format code, with `.` as decimal separator and `,` as thousands
  separator: `1,234.50`, `12.5%`, `1.5E+3`, negative and zero sections, fractions. Colours and
  conditions in a format code (`[Red]`, `[>100]`) are ignored. A code kakemono cannot read falls
  back to the general format.
- **A currency symbol** is kept only when a custom format code writes it (`"¥"#,##0`, `[$€-407]`);
  the built-in currency formats (ids 5 to 8) give the number alone.
- Strings keep bold, italic, subscript and superscript of their rich-text runs. **Furigana**
  (phonetic runs, in nearly every Japanese workbook) never reaches the text.
- Booleans are `TRUE` and `FALSE`; errors are written as in the cell (`#N/A`); a formula gives
  the value saved in the file (kakemono does not recompute): empty when the file has none.
- A cell with a hyperlink keeps its target as a link.

### Limits

A sheet is read as a stream, not as a tree, so a large sheet does not take many times its size in
memory. At most `max_rows` (100,000) rows and `max_cols` (1,000) columns are read from a sheet: the
rest is left out, with a paragraph saying how many rows. The declared size of a sheet is not
trusted, and the ZIP and XML limits are those of the Word reader. A part that is damaged but
optional (styles, a drawing) is skipped rather than failing the whole file.

## Outputs

| Output | A workbook gives |
| --- | --- |
| friendly HTML (`-m friendly`, the default) | headings, captioned tables, notes and paragraphs |
| `-m hard`, `hard-95`, `medium` (`--all`) | the friendly page: there are no pages to copy or place, as for a Word document without layout |
| Word, Markdown, LaTeX, EPUB | the same document; with `-m medium`, Markdown keeps HTML tables |
| editable JSON (`--content` applies it back) | the blocks and tables; `source.format` is `xlsx` |
| chunks (`.jsonl`) | the text cut along the sheets; a table is cut only between rows, and **every chunk of a table repeats its caption and header rows** |
| PDF (`-o x.pdf`) | an error: a workbook has no page layout |

`kakemono inspect book.xlsx` prints the blocks found; it takes `--header-rows` too.

## In the other interfaces

- Rust: `Paper::from_bytes_with(data, &ExcelConfig { header_rows: Some(1), ..Default::default() })`;
  `Paper::from_bytes` reads a workbook with the default `ExcelConfig`. `InputFormat::Xlsx` is
  `"xlsx"` by `name()` and has no pages.
- Python: `header_rows=` on `convert`, `convert_all`, `to_docx`, `to_markdown`, `to_json`,
  `to_latex`, `to_epub`, `to_chunks` and `analyze`.

## Tests and corpus

- `tests/excel.rs`: `tests/data/excel/sample.xlsx`, saved by LibreOffice from the hand-written
  `sample.fods` (sheets with a captioned table and a note, a merged header, dates and currency, two
  tables, a form, links, frozen header rows, a hidden sheet), is read and written to every
  format; the snapshot is `tests/data/snapshots/excel-sample.txt`. To make the workbook again:
  `soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods`.
- `tests/data/excel-corpus.tsv`: public workbooks (statistics offices, regulators, companies,
  Japanese government forms, templates) to try the reader on; downloaded, never committed.

## Left out

Excel 97–2003 (`.xls`), `.ods` and `.csv` files; charts, pictures, pivot tables, comments,
conditional formatting, cell colours and form controls; recomputing formulas; rebuilding the look
of a form drawn on a grid.
