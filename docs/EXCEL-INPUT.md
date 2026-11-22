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

A workbook has no pages, like a Word document: `Document::pages` is empty; the HTML modes that
show pages give the friendly page, and the PDF is laid out from the document (see
[The PDF](#the-pdf)).

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
4. The pictures **placed in cells** (Excel 365's "Place in cell": a cell's `vm` attribute, through
   `xl/metadata.xml` and `xl/richData/` to the picture's file) follow the table of their cell, in
   cell order, as figures with their alternative text; their cell, which holds `#VALUE!` for older
   readers, is left empty.
5. The **drawn objects** of the sheet's drawing (`xl/drawings/`), after the cells, in the order of
   their anchors (row, then column), those of a group (`xdr:grpSp`) included:
   - the text of **shapes and text boxes** (Japanese workbooks often put text, flows and org
     charts in shapes) becomes paragraphs, one per line;
   - a **picture** becomes a figure with its alternative text;
   - a **chart** (a `graphicFrame` of a `c:chart`, or of an Office 2016 `cx:chart`) becomes a
     figure of the chart drawn as SVG by kakemono (`src/chart/`), captioned with its title, else
     "Chart N" (the Nth chart of the sheet), **followed by a table of its data**: the categories as
     rows, a column per series, the series names as the header row, with the same caption. Only the
     values the chart part caches are read (formulas are not computed): a chart without them gives
     its caption and a note. A chart of a type that is not drawn (stock, bubble, surface, the
     Office 2016 types such as waterfall or treemap) gives its data table alone. The chart is drawn
     at the size of its anchor: the widths of its columns and the heights of its rows
     (`col width`, `row ht`, `sheetFormatPr`), or its `ext`, within 72 × 54 pt to 1440 × 1440 pt.
     Its series take the colours of the file, else the accent colours of the workbook's theme
     (`xl/theme/theme1.xml`); the values or shares of `c:dLbls` are shown on bars, points and
     slices.

   Connectors and form controls are not read.

A region with fewer than two columns of data (no row with two cells or more), such as a cover or a
list of notes, becomes **paragraphs**, one per row. (When `table_min_fill_ratio` is raised above 0,
a sparse region becomes paragraphs too, one per row, the text of its cells joined by a space.)

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
  1904 date systems are both read. A time format without hours (`mm:ss`, `mm:ss.0`) gives the
  minutes and seconds, `05:08` or `05:08.4`; a date cell stored as ISO text (`t="d"`) gives
  `2024-01-31 14:05` too, without the time at midnight.
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
memory. At most `max_rows` (100,000) rows and `max_cols` (1,000) columns are read from a sheet,
and at most `max_cells` (2,500,000) cells: rows are read whole, and once a row takes the count of
cells to the limit, the rows after it are left out. What is left out is said in a paragraph at the
end of the sheet: `N more rows are not shown.`, `N more columns are not shown.` The cell limit
bounds the memory (a cell takes a few hundred bytes from the reading to the outputs; the largest
sheet of the corpus, 2.12 million cells, is read whole). The declared size of a sheet is not
trusted, and the ZIP and XML limits are those of the Word reader. A part that is damaged but
optional (the styles, whose cells then get the General format; a drawing; a table part) is skipped
rather than failing the whole file. Damaged shared strings fail it: the text of every string cell
would be lost without a word.

## Outputs

| Output | A workbook gives |
| --- | --- |
| friendly HTML (`-m friendly`, the default) | headings, captioned tables, notes and paragraphs |
| `-m hard`, `hard-95`, `medium` (`--all`) | the friendly page: there are no pages to copy or place, as for a Word document without layout |
| Word, Markdown, LaTeX, EPUB | the same document; with `-m medium`, Markdown keeps HTML tables |
| editable JSON (`--content` applies it back) | the blocks and tables; `source.format` is `xlsx` |
| chunks (`.jsonl`) | the text cut along the sheets; a table is cut only between rows, and **every chunk of a table repeats its caption and header rows**, unless together they would take more than half of a chunk: then no chunk of the table has them |
| PDF (`-o x.pdf`) | the document laid out in pages, without TeX (see [The PDF](#the-pdf)) |

`kakemono inspect book.xlsx` prints the blocks found; it takes `--header-rows` too.

## The PDF

`kakemono convert book.xlsx -o book.pdf` (`Paper::to_pdf`) lays the document out in pages with the
Word layout engine (`src/layout/`: line breaking, fonts, tables, pages, krilla), not with TeX:
`src/layout/workbook/` turns the blocks into the engine's formatted document (`FDoc`), as the Word
reader does for a `.docx`. The look is that of the LaTeX output compiled with LuaLaTeX:

- one sheet after another, each starting with its heading (kept with what follows), page numbers
  at the bottom;
- tables centred, with a heavy rule above and below and a light one under the header (the
  rules the document gives, as in Word and LaTeX); a caption above its table is kept with it;
- the columns are as wide as their text when the table fits the width of the text; otherwise each
  column keeps at least its typical cell (the mean width of its body cells, at most
  `typical_cell_max_em`, and its widest word, and its header on at most `header_max_lines`
  lines), the columns that need little more get it, and the long ones share the rest and wrap;
- a table that does not fit is set smaller (from `table_size_pt` down to `min_table_size_pt`),
  then on landscape pages (from `table_size_pt` down again), then in groups of columns that each
  repeat the first column, at `min_table_size_pt`: no column is ever cut off; the text after a
  turned table stays on its pages until the next heading or float;
- a long table breaks between rows, with its header rows repeated at the top of each page (when
  they take at most `max_head_ratio` of the page); a table of at most `max_unbroken_body_rows`
  body rows moves whole to the next page; a row is kept whole unless it is taller than
  `max_unsplit_row_ratio` of the page, then it breaks between lines;
- at most `max_cells` (500,000) cells of tables are laid out in one PDF, some 3,000 pages: the
  rows past them are left out, with a paragraph saying how many (laying out takes 2 to 3 KB a
  cell at its peak; the 18,035-row Toronto budget, 144,000 cells, is whole: 420 pages, 4.4 s,
  0.6 GB);
- figures (PNG, JPEG, and SVG pictures and charts drawn as vector) at their size, made smaller to
  fit the text width and `figure_max_height_ratio` of its height, with their caption;
- the text in a serif font (Liberation Serif, else Times New Roman or another installed serif),
  CJK text in a CJK font (Noto Serif CJK JP, else IPAex/IPA Mincho, Yu Mincho, MS Mincho, Hiragino,
  Noto Sans CJK...), links in blue.

The page is A4 with 2.5 cm margins, unless the first visible sheet whose print settings
(`pageSetup paperSize`) name a known paper (Letter, Legal, Tabloid, Executive, A3, A4, A5, B4, B5)
asks for another. Every threshold is a field of `WorkbookPdfConfig` (`config.rs`):
`Paper::to_pdf_with` takes one. The PDF is the same from one run to the next on one computer, but
depends on the fonts installed, so it has no snapshot: `tests/excel.rs` checks that every cell and
the last row of each table are in its text.

Not done: `--content` does not apply to the PDF; the sheet's own column widths, fonts, colours,
borders, number alignment of a cell, and print areas are not used (the look is the LaTeX output's).

## In the other interfaces

- Rust: `Paper::from_bytes_with(data, &ExcelConfig { header_rows: Some(1), ..Default::default() })`;
  `Paper::from_bytes` reads a workbook with the default `ExcelConfig`. `InputFormat::Xlsx` is
  `"xlsx"` by `name()` and has no pages; `Paper::to_pdf` (or `to_pdf_with` and a
  `WorkbookPdfConfig`) lays it out as a PDF.
- Python: `header_rows=` on `convert`, `convert_all`, `to_docx`, `to_markdown`, `to_json`,
  `to_latex`, `to_epub`, `to_chunks` and `analyze`.

## Tests and corpus

- `tests/excel.rs`: `tests/data/excel/sample.xlsx`, saved by LibreOffice from the hand-written
  `sample.fods` (sheets with a captioned table and a note, a merged header, dates and currency, two
  tables, a form, links, frozen header rows, a hidden sheet), is read and written to every
  format; the snapshot is `tests/data/snapshots/excel-sample.txt`. To make the workbook again:
  `soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods`.
- `tests/data/excel-corpus.tsv`: public workbooks (statistics offices, regulators, companies,
  Japanese government forms, templates) to try the reader on; downloaded
  (`scripts/fetch-excel-corpus.sh`), never committed.
- `scripts/excel-report.py DIR... [--out FILE]` converts every workbook found to JSON and writes a
  TSV, one line per sheet: the file, the sheet, its tables, the header rows of each table and its
  paragraphs. Run it before and after a change of the header or region rules, on the corpus, and
  compare the two reports. It does not say yet where each header came from (the file's
  declaration, the types of the columns, or the default of one row): that needs the reader to
  record it, a follow-up.

## Left out

Excel 97–2003 (`.xls`), `.ods` and `.csv` files; pivot tables, comments, conditional formatting,
cell colours and form controls; recomputing formulas (a chart shows the values its part caches);
rebuilding the look of a form drawn on a grid; pictures placed in cells shown inside their table
cell (they follow the table); the data labels of area, radar and scatter charts.
