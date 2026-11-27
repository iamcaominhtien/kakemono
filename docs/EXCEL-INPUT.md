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

A workbook has no pages, like a Word document: `Document::pages` is empty. The HTML modes other
than friendly show the sheets as Excel does (see [The sheet view](#the-sheet-view)), and the PDF is
laid out from the document (see [The PDF](#the-pdf)).

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
- Strings keep bold, italic, subscript, superscript and the colour of their rich-text runs; a run
  with its own properties (`rPr`) sets its own font, so it is plain in a bold cell unless it is
  bold itself, while a run without them takes the cell's font. **Furigana**
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
| `-m medium`, `hard`, `hard-95` (`--all`) | the sheet view: the sheets as Excel shows them (see [The sheet view](#the-sheet-view)); it does not show text edited with `--content` |
| Word, Markdown, LaTeX, EPUB | the same document; with `-m medium`, Markdown keeps HTML tables |
| editable JSON (`--content` applies it back) | the blocks and tables; `source.format` is `xlsx` |
| chunks (`.jsonl`) | the text cut along the sheets; a table is cut only between rows, and **every chunk of a table repeats its caption and header rows**, unless together they would take more than half of a chunk: then no chunk of the table has them |
| PDF (`-o x.pdf`) | the document laid out in pages, without TeX (see [The PDF](#the-pdf)) |

`kakemono inspect book.xlsx` prints the blocks found; it takes `--header-rows` too.


## The sheet view

`-m medium` (also `hard` and `hard-95`, which have no pages to copy) reads the workbook a second
time, with the look of every cell (`src/excel/view.rs`), as the medium mode of a Word document reads
its formatting:

- a tab per visible sheet (without scripts, the sheets follow each other, each under its name);
- the grid from A1 to the last cell with a value, a format, a merge or a drawn object: the column
  widths (in digits of the workbook's default font, as Excel measures them: a Western font's
  digit rounded to whole pixels, 7 for Calibri 11 or Arial 10, 8 for Calibri or Corbel 12, half
  the font's size in whole pixels, rounded up, for an East Asian font, 8 for MS P Mincho
  11, 0.55 of it, rounded, for Yu Gothic, 9 at 12 points) and row heights of the file, on the screen the workbook was saved on: with an East Asian
  default font, the widths written as whole numbers of pixels of a larger screen (9 for an 11-point
  Mincho at 120 dots per inch, 125 %, 12 for an 11-point Yu Gothic at 144) are that screen's, whose pixels are smaller, and its rows
  are whole numbers of its pixels; Excel prints them so, and a sheet set to fit a page at a given
  scale there fits it here too, hidden rows and columns left out, merged cells spanning their
  area, the column letters and row numbers kept in view when scrolling, the gridlines unless the
  sheet hides them;
- each cell's font (family with fallbacks of its kind, Japanese ones included: the MS fonts of
  Windows fall back on the IPA fonts of their metrics, the P fonts' Latin letters and digits on
  Arial or Times New Roman, as in the PDF; size, colour, bold,
  italic, underline, strike), fill (solid and shaded patterns; a gradient's first colour), borders
  (thin, medium, thick, dashed, dotted, double), alignment (numbers right, text left unless the
  cell says otherwise; distributed, 均等割付, spread over the cell's width; vertical; indent;
  wrapped text; text turned or stacked, a stacked text that wraps going on in columns to the left; a
  number format's fill, `*x`, sets the text before it at the cell's left edge and the text after it
  at the right, as an accounting format sets `$` apart from the amount), with theme colours
  and tints and the legacy palette; formats of whole rows and columns too;
- text wider than its cell runs over the empty cells beside it, as in Excel, and is cut at the
  next cell with a value; a text centred across the selection (`centerContinuous`) is centred over
  the blank cells after it that are centred so too, and wrapped in their width when it wraps; a merged range takes its right and bottom borders from its right column
  and bottom row;
- links: to a web page (http, https, mailto, ftp), and to a place in the workbook
  (`'Sheet'!A1`), which shows that sheet's tab;
- charts (SVG; dashed and smoothed lines, a legend placed by hand on its side), pictures, pictures
  placed in cells, embedded objects (a Word document, ...) by the picture the file keeps of them,
  shapes (preset outlines such as arrows, bent arrows, triangles, brackets, braces, cylinders,
  callouts with their wedge to the tip they point at; arrow heads and the corners of brackets and
  braces sized by the shape's shorter side, as Excel draws them; drawn outlines, `custGeom`, of
  lines and curves whose points are numbers; fill, outline, text with
  the look of each run, its insets (within its outline's text box: a cylinder's below its lid),
  cut at the bottom edge when the shape says so, on lines as
  long as they are when it is not wrapped, `wrap="none"`) and connectors (straight and elbow, with arrow heads, at least 5 points long on thin lines) at the place and size
  of their anchors, groups included, turned and mirrored with their groups; a hidden shape
  (`cNvPr hidden`) only as the form control the sheet's VML drawing shows (`visibility`), a check
  box or option button with its box or circle (ticked or filled when `x:Checked`) before its
  label;
- conditional formats (`src/excel/conditional.rs`), tested on the values read: a cell compared
  with numbers, texts or another cell (`cellIs`), containing, beginning or ending with a text,
  empty or not, a duplicate or unique value, each with the font colour and style, fill and
  borders of its format, the first rule over the others, a rule that stops (`stopIfTrue`) ending
  them; and data bars, from their colour to white, as long as the value between their bounds.

Not drawn: conditional formats on a formula of their own (`expression`), colour scales, icon
sets, top or bottom values, above or below average, sparklines, comments, number colours of a
format (`[Red]`), text turned by an angle other than ±90°, and the curves of curved connectors
(drawn straight). A sheet is drawn up to 200 000 cells read and 600 000 positions (rows × columns)
and 300 columns, the rows past them left out with a note.

## The PDF

`kakemono convert book.xlsx -o book.pdf` (`Paper::to_pdf`) has two styles, both drawn with the Word
layout engine's fonts and PDF writer (krilla), not with TeX: `--pdf-style print`, the default, and
`--pdf-style document` (`WorkbookPdfConfig::style`).

### Printed as Excel prints it (`--pdf-style print`)

`src/layout/workbook/print.rs` prints each visible sheet from the same reading as the sheet view
(`excel::read_book`):

- what prints: the sheet's print area (`_xlnm.Print_Area`), else from A1 to the last cell with a
  value, a fill or a border, a merge or a drawn object; hidden rows and columns left out; sheet
  names are not printed (Excel prints none);
- the pages: the paper of `pageSetup paperSize` (Letter when the sheet names none, as Excel's
  default), turned for `orientation="landscape"`, within `pageMargins`; at `scale`, or fitted to
  `fitToWidth` × `fitToHeight` pages (`pageSetUpPr fitToPage`, never larger than 100 %, at least
  10 %); cut between columns, then rows, where a page is full and at the manual breaks
  (`rowBreaks`, `colBreaks`; those of a side fitted to a number of pages are left out, as Excel
  does, those of a side not fitted kept, as LibreOffice does); the rows repeated at the top
  and the columns at the left (`_xlnm.Print_Titles`) on every later page; a text that runs over
  empty cells past a page's columns going on on the page of the next columns, the letter on the
  edge kept on the first; pages down then over, or over then down
  (`pageOrder`); centred when `printOptions` says so; gridlines only when it asks for them;
- row heights as on the screen the workbook was saved on (see the sheet view); column widths as
  the printer draws them: Excel keeps a width in digits of the default font, which the screen
  measures in whole pixels and the printer nearly exactly (Microsoft's KB 214394), so a digit of
  an 11-point MS P Gothic prints 5.5 points wide, not the screen's 8 pixels. The digit is half an
  em in MS Gothic and MS Mincho, P or not, and measured in a Western default font when it (or a
  font of its metrics) is installed; other fonts keep the screen's widths. The scales Excel
  stores for sheets fitted to a page agree with this for Calibri, Arial and MS P Gothic, and with
  the screen for Yu Gothic and MS P Mincho;
  row heights as Excel draws them, a whole number of pixels at 96 per inch (`ht="12.95"` is 17
  pixels, 12.75 points; a workbook saved on a screen of another resolution prints a little longer
  or shorter than Excel printed it there); a row whose height is not set by hand (no
  `customHeight`), which Excel fits to its text, grows by the lines its wrapped text lacks in the
  font set in its place, so that no line is cut, and the drawn objects below it move down with
  their cells (it never shrinks: LibreOffice, which fits such rows both ways, can print fewer
  pages);
- on each page: cell fills, borders (thin, medium, thick, dashed, dotted, double; a hairline, dotted
  on the screen, as a thin solid line, as Excel prints it), the text in the
  cell's font (its family or a font with its metrics: Calibri as Carlito, Century Gothic as URW
  Gothic...; MS Mincho and MS Gothic, P or not, as the IPA fonts, their Latin letters and digits
  in Times New Roman or Arial when they are not installed, since the IPA P fonts' digits are wider
  than half an em; Japanese text of a Western font in a Japanese font), size, colour, bold, italic, underline, strike,
  aligned as Excel aligns it (centred across the selection too, wrapped in its width), wrapped when the cell wraps,
  running over the empty cells beside it and cut at the next value or the page's edge; a distributed
  text (均等割付) spread over the cell, its words, spaces and Japanese letters apart by even gaps;
  lines broken by Word's Japanese rules (no line ends with an opening bracket, none starts with a
  closing one or 、。); text turned 90° or stacked is set a letter a line, a stacked text that
  wraps going on in columns to the left; a number format's fill (`*x`) setting the text before it
  at the cell's left edge and the text after it at the right;
  links kept (http, https, mailto, ftp);
- charts (vector), pictures, pictures in cells, embedded objects by their picture, shapes (preset
  outlines as vector, their text as text, run by run) and connectors with their arrow heads, where they are anchored; one across a page break
  is cut, its parts on their pages;
- a merged range cut by a page break shows its part on each page, its borders and fill cut at
  the page's edge;
- the headers and footers of the sheet (left, centre and right sections; the page number and
  count, the sheet's name, the file's name, the date (ISO) and time of printing; font, size, bold,
  italic, underline, strike and colour codes; their own first and even pages), at their distances
  from the paper's edges, scaled with the sheet unless they are set not to be; pictures in them are
  left out;
- a page with nothing to print is left out, as Excel does;
- at most `print_max_pages` (2,000) pages.

Conditional formats print as the sheet view draws them, data bars in one colour.

Not printed: pictures of headers and footers, the conditional formats the sheet view leaves out, comments, the curves of curved connectors
(drawn straight), text turned by another angle. The text stays text: it can be selected and
searched.

### As a document (`--pdf-style document`)

`src/layout/workbook/` lays the document out in pages with the
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

Not done: `--content` does not apply to either PDF; in the document style, the sheet's own column
widths, fonts, colours, borders, number alignment of a cell, and print areas are not used (the look
is the LaTeX output's).

## In the other interfaces

- Rust: `Paper::from_bytes_with(data, &ExcelConfig { header_rows: Some(1), ..Default::default() })`;
  `Paper::from_bytes` reads a workbook with the default `ExcelConfig`. `InputFormat::Xlsx` is
  `"xlsx"` by `name()` and has no pages; `Paper::to_pdf` prints it as Excel does, `to_pdf_with` and a
  `WorkbookPdfConfig` with `style: WorkbookPdfStyle::Document` lays it out as a document.
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
