# Detailed guide

This guide covers output formats, the CLI, APIs, benchmarks and testing. For a quick start, see the [README](../README.md). Benchmark results describe the stated test environment, not a performance guarantee.

## Word documents as input

A `.docx` or `.doc` file is an input like a PDF (`kakemono convert report.docx -o report.md`): the
format is recognised from the file's bytes. A Word document says what each paragraph is, so it is
read directly, without the layout analysis a PDF needs, and turned into the same structure:

- paragraph styles give the title, authors, abstract, headings (`Heading 1`–`9`, or any style with
  an outline level), captions, footnotes written as paragraphs, references (`Bibliography`, or the
  paragraphs after a "References" heading), code (a code style, or text all in a monospaced font)
  and table of contents entries; list numbering gives list items with their markers ("1.", "(a)");
- Word equations (Office Math) keep their structure: MathML, LaTeX and Office Math in the outputs,
  in running text too;
- tables become rebuilt tables (merged cells kept), pictures (PNG, JPEG, GIF, BMP, TIFF, WebP, SVG) figures,
  with the caption before or after them; footnotes and endnotes follow their paragraph;
- hyperlinks, cross-references to bookmarks, citations and references to figures and tables become
  links, as for a PDF;
- pictures stored as Windows metafiles (EMF, WMF: charts, older drawings) are drawn by kakemono
  as SVG; they and SVG pictures (the SVG Word keeps beside a PNG too) stay vector in every output:
  an `<img>` of the SVG in HTML, an `.svg` file in Markdown, JSON and EPUB, a PDF picture in LaTeX,
  an SVG with a PNG beside it in Word, vector drawing in the PDF. An SVG is written again by
  kakemono first, with only its drawing (no script, link or style sheet);
- charts of `.docx` files are drawn by kakemono as SVG (vector in the PDF too), each followed by a
  table of its data; an Office 2016 chart (waterfall, treemap, ...) shows the picture Word keeps
  beside it, else its data; the charts of `.doc` files are left out;
- formulas made with the Equation Editor 3.0 or MathType (MTEF 3 and 5, in `.doc` and `.docx`
  files) become real formulas, floating shapes and text boxes of `.doc` files are read,
  and nested lists keep their level.

**Pages as Word lays them out.** kakemono also lays a Word document out in pages itself, the way
Word does: fonts with Word's metrics (a table of metrics for the usual Office fonts is built in),
line breaking, line spacing, widow and orphan control, keep-with-next, tables (row heights, merged
cells, borders, shading, repeated header rows), headers and footers, footnotes, sections and
columns, frames, floating pictures, shapes and text boxes. No Word, LibreOffice or other program is
used. That layout gives:

- `-o report.pdf` (or `-f pdf`): a PDF of the document, with selectable text and links;
- `hard` and `hard-95`: its pages, as for a PDF;
- `medium`: Word's formatting (fonts, colours, indents, spacing, tables, pictures) in one
  continuous page, without page breaks;
- `friendly` stays the single-column article, from the document's structure.

How close it is to Word is measured in CI on 524 documents with the PDF Word made of them (government
reports, forms, letters, guidance, templates; `.docx` and `.doc`; see
[Testing](#testing)): about 96% of lines break at the same words as in Word, and 89% of documents
have the same number of pages. Text edited with `--content` reaches every output except the page
layout (PDF, `hard`, `hard-95`, `medium`). `.doc` files (Word 97–2003) are read by kakemono
itself; Word 6/95 files, RTF files and password-protected files are not. Comments, tracked
changes (read as accepted), macros and form fields are not shown. See
[docs/WORD-INPUT.md](WORD-INPUT.md) for the design.

## Excel workbooks as input

A `.xlsx` file is an input like a PDF or a Word document (`kakemono convert sales.xlsx -o sales.md`);
the format is recognised from the file's bytes. The cells say what the content is, so the workbook
is read directly into the same structure, to read it comfortably outside Excel's grid or to feed
it to an AI:

- each visible sheet is a section headed by its name; hidden sheets, rows and columns are left out;
- the cells of a sheet are cut into tables where empty rows or columns separate them; a title or
  unit line above a table is its caption, single-cell rows below it are notes;
- the header rows of a table come from `--header-rows N` if given, else from what the file declares
  (Excel Table, auto-filter, frozen rows, print titles), else from the types of the columns (the
  header is the rows above the first one whose cells have the type of their column), else the
  first row;
- values are shown as Excel shows them: percentages, thousands separators, decimals; dates and
  times always in ISO form (`2024-01-31`, `14:05`); furigana never reaches the text;
- the text of shapes and text boxes is read; forms drawn on a grid keep their text, not their look;
- pictures become figures (also those of groups, and those placed in cells, after their table);
  charts are drawn by kakemono as SVG, sharp at any zoom (bar, column, line, area, pie, doughnut,
  scatter, radar, and bar and line combined; the colours and data labels of the file), each
  followed by a table of its data, which is what the chunks and the JSON give;
- in the chunks (`.jsonl`), a table is cut between rows and every chunk repeats its header rows.

```bash
kakemono convert sales.xlsx -o sales.md                  # Markdown, a pipe table per region
kakemono convert sales.xlsx -o sales.jsonl               # chunks for retrieval (RAG)
kakemono convert sales.xlsx --header-rows 2 -o sales.md  # two header rows in every table (0: none)
```

`-m medium` (and `hard`, `hard-95`) shows the workbook as Excel does: a tab per sheet, and on each
the grid of its cells with their widths, heights, merges, fonts, fills, borders and alignment, the
column letters and row numbers, and its charts, pictures, shapes and connectors drawn where they
are anchored. `friendly` is the workbook as a document, the form every other output takes.
`-o sales.pdf` prints it as Excel prints it, without TeX: each sheet's print area (or the cells it
uses) cut into pages by its print settings (paper, orientation, margins, scale or fit to pages,
manual breaks, rows repeated at the top), with the look of every cell and its charts, pictures and
shapes where they are anchored, the text kept as text. `--pdf-style document` lays it out as a
document instead: each sheet after its heading,
tables with rules and their cells wrapped in their columns, a long table broken between rows with
its header rows repeated on each page, a wide one set smaller, then on landscape pages, then in
groups of columns (no column is ever cut off), charts drawn as vector, CJK text in CJK fonts; on A4
unless the workbook's print settings name another paper. Details:
[docs/EXCEL-INPUT.md](EXCEL-INPUT.md).

## EPUB books as input

An `.epub` file is an input like a PDF or a Word document (`kakemono convert book.epub -o
book.md`). The content documents of the book (XHTML), in the order of its spine, are read
directly, without layout analysis, into the same intermediate form as a Word document, so the same
rules give each paragraph its kind:

- `<h1>`–`<h6>` are headings (raised together so that the highest is of level 1, since many books
  keep `<h1>` for the title or start their chapters at `<h2>`); the title and authors come from the
  title page (`class="title"`, `class="author"`) or else from the book's metadata (`dc:title`,
  `dc:creator`); `<pre>` is code; `<figcaption>` and a table's `<caption>` are captions; ordered
  and bullet lists keep their markers (`start`, `type`, `value`) and nesting;
- MathML formulas keep their structure (MathML, LaTeX and Office Math in the outputs, in running
  text too);
- tables become rebuilt tables (`colspan`, `rowspan` and header rows kept); pictures (`<img>`, an
  `<svg>` showing one, or a drawing in SVG) become figures at the size the book gives them;
- a note called by a link marked `epub:type="noteref"` (or `role="doc-noteref"`) follows the
  paragraph that calls it, without its link back (Word, LaTeX and Typst set it as a footnote of
  its call); links between the documents of the book keep
  working, and citations and references to figures and tables become links, as for a PDF;
- bold, italics, monospace, scripts and small capitals come from the elements (`<em>`, `<b>`,
  `<code>`, `<sup>`, ...) and from simple rules of the book's style sheets (`.italic { font-style:
  italic }`); elements with `display: none`, page break markers and the navigation document are
  left out;
- line breaks (`<br>`: the lines of a poem, of an address) stay line breaks in every output, as
  the manual line breaks of a Word document do: `<br>` in HTML, EPUB and Markdown, `<w:br/>` in
  Word, `\newline` in LaTeX (a space inside a table, whose cells hold one line), `#linebreak()`
  in Typst.

A book has no pages: all four HTML modes give the `friendly` article, and `-o book.pdf` lays the
book out in pages through a Word document, as for Word input. Books protected by DRM (encrypted
content documents) cannot be read.

## Word documents

`kakemono convert paper.pdf -o paper.docx` (or `--format docx`) writes a Word document instead of
HTML. It is built from the same analysis as the HTML modes, as one flowing column:

- the title, authors, abstract, headings (`Heading 1`–`4`, so Word's navigation pane and table of
  contents work), lists, code and references get Word styles;
- footnotes become Word footnotes (see [Footnotes](#footnotes));
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
- rebuilt tables become pipe tables. With `-m friendly` (the default) every table is adapted to
  one, its meaning kept rather than its layout: the header rows become one (`BLEU: EN-DE`), and a
  cell spanning rows, or columns under one group header, is repeated in each, so that every row
  reads on its own. With `-m medium` the layout is kept: a table whose cells span rows or columns
  stays an HTML table;
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
- footnotes become `\footnote`s (see [Footnotes](#footnotes));
- figures are pictures (`\includegraphics`), always written to `<name>_assets/` next to the file
  (`--image-dir` to choose it): LaTeX cannot embed them, so `--images inline` is refused;
- figures and tables stay where they were in the text (they are not floats) and keep the label of
  their caption ("Figure 3:"); citations and references to figures, tables and sections are
  links (`\hyperref`), web addresses `\href`.

A character that the fonts of pdfLaTeX lack (Chinese, say) is printed as **?** by pdfLaTeX;
XeLaTeX and LuaLaTeX print it when their font has it. Vietnamese letters (ơ, ư, ặ, ữ, ...)
are set in the T5 encoding of the `vntex` package by pdfLaTeX, the rest of the text staying in
T1. `--scale` and `--equations` apply.

## Typst

`kakemono convert book.pdf -o book.typ` (or `--format typst`) writes a [Typst](https://typst.app)
document, from the same analysis, that compiles as it is with `typst compile book.typ` (no TeX
installation; the fonts it uses by default come with Typst). Typst breaks each paragraph as a
whole, so justified lines have no wide gaps, and keeps every footnote on the page of its call: it
is the output for printing a book, a translated one above all.

The file starts with its settings, which can be changed there: the layout (`doc-layout`), the page
size (`paper`: `"a4"`, `"a5"`, `(width: 110mm, height: 180mm)` for a phone, ...), the fonts
(`fonts`: each character is taken from the first font that has it; Libertinus Serif, the default,
has the Vietnamese letters), the size of the text, the language (`lang`: hyphenation and quotation
marks; Vietnamese, which is not hyphenated, is found from the text) and a cover picture
(`cover`). The template comes next: the Typst functions (`kakemono-…`) that lay the document
out, which can be changed too.

There are two layouts, chosen with `--layout` or from the document (`auto`, the default):

- `book`, for a document with chapters and nothing of a paper (no abstract, references or arXiv
  identifier): A5 pages, a title page, chapters that open on a right-hand page a third of the way
  down, their first words in small capitals (except in Vietnamese, which most fonts lack small
  capitals for) and their notes numbered from 1; mirrored margins; running heads with the title of
  the book on the left-hand pages and that of the chapter on the right-hand ones, none on the title
  page, on the opening page of a chapter (which has its number at the foot) or on a blank page;
- `article`: A4 pages, the title, authors and abstract at the top of the first page, page numbers
  at the foot.

In both:

- headings become Typst headings, without numbers of their own (theirs are part of their text);
  the outermost level is `=`; lists keep the markers of the document;
- a table of contents is Typst's own (`outline`), with the page numbers of the new layout and
  links to the headings;
- rebuilt tables become `table`s with rules above, below and under the header and their spanning
  cells; text is set in a smaller size and wraps when the table is wider than the text;
- formulas whose structure is clear become Typst math (with their number at the right), the others
  pictures; formulas inside the text become `$…$`;
- footnotes and added notes (`<note>` in the editable JSON) become Typst footnotes (see
  [Footnotes](#footnotes));
- line breaks (the lines of a poem) are kept;
- figures are pictures, always written to `<name>_assets/` next to the file (`--image-dir` to
  choose it): Typst cannot embed them in its source, so `--images inline` is refused; a picture is
  as wide as in the PDF, at most as wide as the text, and stays on one page with its caption;
- figures and tables stay where they were in the text (they are not floats) and keep the label of
  their caption ("Figure 3:"); citations and references to figures, tables and sections are links,
  as are web addresses.

`--scale` and `--equations` apply. The tests compile the Typst output with Typst 0.15.

## Footnotes

Word, LaTeX and Typst set a footnote at the foot of the page of its call, numbered by the format: a
footnote of a Word document, a note of an EPUB book (which e-readers show in a popup) and a
footnote of a PDF page. A note becomes a footnote when its mark is called once, from a paragraph or
a list item, and the note follows the call (after only other notes, or on the same page of the PDF
or the next). The others stay where they were, small, with their mark: the endnotes of a Word
document (at the end), a note called from a heading, a caption, a table or the authors, or from
more than one place (another note referring to it, say). The HTML modes, Markdown, EPUB and JSON
keep every note as it was, after its paragraph.

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
  formula of the block, with its LaTeX inside. `<note>…</note>` is a note added to the text when it is edited (a translator's note,
  an editor's or a reviewer's, say), attached at that point of the text, for example `“<i>Mon ami</i><note>French: <i>my friend</i>.</note>
  Hastings!”`: it may hold the style tags and links, but no formula and no other note. (This tag is
  unrelated to a block of type `note`.)
- `plain` is the same text without tags, formulas as `$LaTeX$` (`$$…$$` for a displayed formula),
  for search indexes and language models; table cells have one too.

**Bibliographic fields.** For data extraction the file also has what could be read with
confidence from the front matter and the reference list (a field that could not be read is left
out):

```json
"meta": {
  "title": "Attention Is All You Need", "arxiv_id": "1706.03762v7",
  "arxiv_category": "cs.CL", "date": "2023-08-02", "doi": "…", "keywords": ["…"],
  "authors": [{ "name": "Ashish Vaswani", "affiliations": [0], "emails": ["avaswani@google.com"], "notes": [0] }, …],
  "affiliations": ["Google Brain", "Google Research", "University of Toronto"],
  "author_notes": ["Equal contribution. Listing order is random. …", …],
  "abstract": "The dominant sequence transduction models are …"
}
```

```json
{ "id": 160, "type": "reference", "label": "32", "text": "[32] Noam Shazeer, … <i>arXiv preprint </i>…",
  "citation": { "authors": ["Noam Shazeer", "Azalia Mirhoseini", …, "Jeff Dean"],
                "title": "Outrageously large neural networks: The sparsely-gated mixture-of-experts layer",
                "venue": "arXiv", "year": 2017, "arxiv": "1701.06538" } }
```

- `authors` come from the author block: names (told apart by commas, "and", or the gaps between
  names printed side by side), the affiliations below them or pointed to by a raised mark
  (`Jane Doe¹`), the e-mail addresses (`{a, b}@example.com` gives two) and notes (`∗` Equal
  contribution). `date` is that of the arXiv version (from its stamp), else the date printed with
  the authors.
- `citation` gives `authors` (given names first, `et_al` when the list is cut), `title`, `venue`,
  `year`, `volume`, `pages`, `doi`, `arxiv` and `url`. It knows the usual styles (IEEE with a quoted
  title, ACL/APA with the year after the authors, the astronomy and physics styles without titles,
  books in italics); on the papers of the test suite most references give authors, title and year.
  A reference with no year written gets the year of its arXiv identifier.
- These fields, like `plain`, are read-only: they are derived from the text and not read back.

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

Added notes (`<note>`) are printed as they are written and numbered in reading order: in
friendly and medium a raised number links to the note, listed in a small aside after its block;
in hard-95 the number opens the note in a popup; Word gets real footnotes, LaTeX `\footnote`s,
Typst footnotes (numbered again in each chapter of a book),
Markdown `[^n1]` footnotes and EPUB popup footnotes at the end of each chapter (numbered again in
each chapter). `plain` and the chunks leave the notes out. A `<note>` that is not closed, or that
holds a formula or another note, is read with a warning.

**Translating the text inside a figure.** A figure whose picture has text in the PDF (labels,
legends, the boxes of a diagram) lists it in `figure_lines`, as runs: a label, or the lines of one
paragraph inside a box, each `{"text": "…", "box": [x0, y0, x1, y1], "size": pt}`, the `text`
editable like a block's. An edited run is written in place: the figure is drawn again without the
glyphs of the old text, and the new text is written in its box, in a sans-serif (or monospaced)
font of the run's size and weight, broken into lines that fit the box (made smaller, down to 75%,
when it needs more lines than the run had; a one-line label may grow to 1.6 times its width). The
figure then becomes an SVG, as with `replace_image` below, which wins when both are given. Rotated
text (an axis title written upwards) comes after the other runs, with its `rotation` in degrees,
and is written back along the same line, centred on the old text. A tick label never joins the
words of a label next to it.

**Replacing a figure's picture.** A `figure` block may also get `"replace_image"`: a PNG, JPEG or
SVG file, named by a path relative to the JSON file, or a data URI. Every output then draws it in
place of the figure's picture (a translated or redrawn figure, say). An SVG stays an SVG in the
HTML modes, EPUB, Markdown and JSON, with its text turned into paths so that it looks the same
without its fonts; Word, LaTeX and Typst get it as a PNG at `--scale`. In `hard-95` the picture covers the
figure's box on the page, and the figure's old text layer is left out. In every output a picture
of another shape than the figure is fitted into its box and centred, never stretched, and the
pictures an SVG names (relative to the SVG) are kept inside it.
A picture whose shape differs from the figure's box, a file that cannot be read, or
`replace_image` on a block that is not a figure with a picture is a warning; the figure then keeps
its picture (except for the shape, where the new picture is still used). A JSON written again after
`--content` has the new picture in `image` and no `replace_image`. With the library,
`Paper::apply_content_in(&mut analysis, &json, folder)` reads relative paths from the JSON's folder;
`Paper::apply_content`, which has no folder, takes only data URIs and absolute paths.

In `hard-95` an edited block flows inside the box of its lines, its type made smaller when the new
text is longer (down to 62%), and a paragraph that runs over columns or pages runs over them
again. The `hard` mode shows the pages themselves and cannot show edited text. In LaTeX output,
pdfLaTeX prints Vietnamese (with `vntex`); for scripts its fonts lack (Chinese, say), use XeLaTeX
or LuaLaTeX.

## Chunks for retrieval (RAG)

`kakemono convert paper.pdf -o paper.jsonl` (or `--format chunks`) cuts the text into chunks
ready to embed, one JSON object per line:

```json
{"id":3,"text":"I. INTRODUCTION\n\nThe discovery of the binary pulsar system PSR B1913+16 by Hulse and Taylor ...",
 "tokens":439,"title":"Observation of Gravitational Waves from a Binary Black Hole Merger",
 "section":["I. INTRODUCTION"],"blocks":[12,13],"pages":[0]}
```

- **Along the structure.** A chunk holds the blocks of one section, up to `--chunk-tokens`
  (default 512); a block is cut only when it does not fit in one chunk, at sentence ends. Tables
  give their caption and rows (cells joined by `|`), formulas their LaTeX (`$$...$$`).
- **Headed by the section path** (`3 Model > 3.2 Attention`), not by the paper title: within one
  paper a title on every chunk only blurs the embeddings. The title is in each chunk's `title`,
  to filter or prefix when searching several papers. `blocks` are the ids of the
  [editable JSON](#editable-json) (and `--json`), `pages` count from 0.
- **Sized in tokens.** By default an estimate that needs no tokenizer: 15 to 20% above the count
  of real subword tokenizers, so that a chunk seldom passes a model's limit. To count exactly,
  give `--tokenizer` your embedding model's `tokenizer.json` (from its Hugging Face repository) or
  `cl100k_base` / `o200k_base` for OpenAI's models. `tokens` then counts without special tokens
  (`[CLS]`, `<s>`), which most models add.

Measured on [Qasper](https://allenai.org/data/qasper) (questions on 100 NLP papers; share of the
gold evidence in the retrieved text, two embedding models; see `eval/qasper/`):

| Chunks from | nomic-embed v1.5, 1024 / 2048 tokens retrieved | Qwen3-Embedding 0.6B, 1024 / 2048 | Time, 100 papers (CPU) |
| --- | --- | --- | --- |
| **kakemono** (`-f chunks`) | **0.611** / **0.738** | **0.581** / **0.753** | **9.5 s** |
| docling + its `HybridChunker` | 0.608 / 0.734 | 0.536 / 0.705 | 2032 s |
| unstructured (`fast`) + `chunk_by_title` | 0.461 / 0.596 | 0.446 / 0.626 | 153 s |
| pypdf, cut every 512 tokens | 0.388 / 0.559 | 0.450 / 0.621 | 38 s |

docling ties with nomic and is 4 to 5 points behind with Qwen3 (a difference outside the 95%
interval of a bootstrap over papers); pypdf and unstructured are 13 to 22 points behind with both. Times are conversion and chunking,
without embedding, one paper after another on 4 cores.

```bash
kakemono convert paper.pdf -o paper.jsonl --chunk-tokens 256
kakemono convert paper.pdf -o paper.jsonl --tokenizer path/to/bge-m3/tokenizer.json
```

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
kakemono convert book.pdf -o book.typ && typst compile book.typ
```

```bash
kakemono convert paper.pdf -o paper.epub
```

```bash
kakemono convert paper.pdf -o paper.json
kakemono convert paper.pdf --content paper.edited.json -m hard-95
```

The input is a PDF file, a Word document (`.docx`, `.doc`), an Excel workbook (`.xlsx`), an EPUB book (`.epub`), an arXiv ID (`1706.03762`,
`arXiv:1706.03762v7`, `hep-th/9901001`) or an arXiv URL. Downloaded PDFs are cached in `~/.cache/kakemono` (`KAKEMONO_CACHE` changes it).

| Option | Meaning |
| --- | --- |
| `-m, --mode <hard\|hard-95\|medium\|friendly>` | Mode (default `friendly`); for Markdown, `friendly` or `medium` (how tables are written) |
| `--all` | All four modes: `<name>.<mode>.html` |
| `-f, --format <html\|docx\|md\|tex\|typst\|epub\|json\|chunks\|pdf>` | HTML (default), a Word document, Markdown, LaTeX, Typst, an EPUB book, editable JSON, chunks for retrieval, or a PDF (Word and EPUB input: the document laid out in pages as Word lays it out; a workbook: printed as Excel prints it); chosen from `--output` when it ends in `.docx`, `.md`, `.tex`, `.typ`, `.epub`, `.json`, `.jsonl` or `.pdf` |
| `--header-rows <N>` | Header rows of every table of an Excel workbook, 0 for none (default: found from the cells); ignored for other inputs |
| `--pdf-style <excel\|document>` | The PDF of an Excel workbook: printed as Excel prints it (default), or laid out as a document |
| `-o, --output <path>` | HTML, `.docx`, `.md`, `.tex`, `.typ`, `.epub`, `.json`, `.jsonl` or `.pdf` file, or directory |
| `--content <file.json>` | Write the output with the text of an edited JSON file (from `--format json`, for the same PDF); every format but `hard` mode for PDF input; Word page-layout outputs do not use edited text |
| `--images <inline\|files>` | Embed images (default for HTML and Word) or write them to a folder next to the file (default for Markdown and JSON, always for LaTeX and Typst; Word keeps them embedded too; never for EPUB) |
| `--image-dir <dir>` | That folder, relative to the output file (default `<name>_assets`); with `--all`, one subfolder per mode |
| `--scale <number>` | Image resolution in pixels per PDF point (default 3 ≈ 216 dpi), HTML, Word and EPUB |
| `--page-image <svg\|svg-only\|png>` | Page background of `hard`/`hard-95` (default `svg`; PNG for a page whose SVG would pass 1.5 MB, when smaller) |
| `--png-compression <fast\|balanced\|high>` | How hard PNG pictures are compressed (default `balanced`); `fast` draws them several times faster for files about a quarter larger, `high` makes them a tenth smaller for ten times the time. The pixels are the same |
| `--tables <auto\|image>` | Rebuild tables when their grid is clear (default), or keep pictures |
| `--layout <auto\|article\|book>` | Layout of the Typst output: a printed book or an article, chosen from the document by default (it is also a setting at the top of the `.typ` file) |
| `--equations <mathml\|image>` | Formulas as MathML (Word equations in `.docx`, LaTeX in `.md` and `.tex`, Typst math in `.typ`, MathML in `.epub`) where the structure is clear, also inside the text (default); or display formulas as pictures and inline ones as styled text |
| `--fonts <embed\|system>` | Embed the reading and math fonts (default, works offline) or use the reader's fonts |
| `--json` | Also write `<name>.json`, the document structure with every field of the analysis (see below; to edit the text, use `--format json`) |
| `--chunk-tokens <N>` | Largest chunk, in tokens (default 512), for `--format chunks` |
| `--tokenizer <file.json\|name>` | Count the tokens of chunks with a real tokenizer: a Hugging Face `tokenizer.json`, or `cl100k_base` / `o200k_base` (OpenAI); the default is an estimate |
| `-j, --jobs <N>` | Threads (default one per core); the result is the same for every value |

Other commands: `kakemono fetch <id> -o paper.pdf` only downloads; `kakemono inspect paper.pdf`
prints the detected structure (headings, paragraphs, figures, formulas; `<math>` marks a formula
rebuilt as MathML); `kakemono formula picture.png` reads pictures of LaTeX formulas and prints
their MathML (experimental); `kakemono render-svg picture.svg -o picture.png [--scale 2]` draws an
SVG as a PNG the way kakemono draws the SVG pictures it is given (`replace_image`), with the
pictures it names relative to the SVG.

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
`paper.to_typst(&analysis, &TypstOptions::default())` a Typst document with its picture files,
`paper.to_epub(&analysis, &EpubOptions::default())` the bytes of an EPUB book, and
`paper.to_json(&analysis, &JsonOptions::default())` the editable JSON with its picture files, and
`paper.to_chunks(&analysis, &ChunkOptions::default())` the chunks for retrieval
(`kakemono::chunks_jsonl` writes them as JSON Lines; `TokenCounter::load` reads a tokenizer,
`TokenCounter::custom` takes any counting function). The real tokenizers are the default cargo
features `tokenizers` (Hugging Face) and `openai-tokenizers`; without them, chunks are sized by the
estimate.
`paper.apply_content(&mut analysis, &json)` applies an edited JSON file to the analysis, and supported outputs written from it afterwards show the edited text. Word page-layout outputs do not use edited text. `analysis.doc` is the
document structure that `--json` writes. The API reference is on [docs.rs](https://docs.rs/kakemono);
how the pipeline works is described in [ARCHITECTURE.md](../ARCHITECTURE.md).

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
content = kakemono.to_json("paper.pdf", "paper.json")  # the editable JSON (a dict), pictures in paper_assets/
html = kakemono.convert("paper.pdf", "hard-95", content=content)  # any output with the edited text
doc = kakemono.analyze("paper.pdf")                    # the structure, as with --json
kakemono.to_pdf("report.docx", "report.pdf")           # a Word document laid out in pages
kakemono.render_svg("figure.svg", "figure.png")        # an SVG drawn as a PNG, as render-svg does
doc = kakemono.Document("paper.pdf", content=content)  # analysed once, for several outputs:
doc.to_docx("paper.vi.docx"); doc.to_epub("paper.vi.epub")
```

The wheels (Linux, macOS, Windows; Python 3.9+) contain the Rust library: nothing else to install.
The options are those of the command line (`tables`, `equations`, `page_image`, `embed_fonts`,
`scale`, `jobs`, `png_compression`, `content`: an edited JSON as a dict, its text or its path);
see [python/README.md](../python/README.md).

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

**Word documents** (`scripts/word-benchmark.py`, same machine, against LibreOffice 24.2 on the
524 documents of the Word corpus, one at a time):

| Word → PDF | kakemono | LibreOffice 24.2 |
| --- | --- | --- |
| All 524 documents (11,657 pages) | 209 s (56 pages/s) | 790 s (14 pages/s) |
| Median per document | 0.12 s, 35 MB | 0.47 s, 192 MB |
| Largest memory | 526 MB | 822 MB |
| Could not convert | 0 | 5 (crashes) |
| Lines / page counts as in Word's PDF | 95.9% / 89.1% | 79.6% / 72.3% |

The flowing outputs of a Word file (friendly HTML, Word, Markdown, LaTeX, EPUB, JSON) skip the
page layout: 0.01–0.4 s per document, 15–23× faster than LibreOffice's Word and HTML exports
(`.doc` → `.docx` included). One process per core converts about 870 short documents a minute on 4
cores. By size, every format, threads and memory:
[docs/WORD-INPUT.md](WORD-INPUT.md#speed-and-resources).

**Web pages** (`scripts/web-bench/perf.py`, same machine): the 330 pages of the web benchmark
convert to Markdown in 10 ms each (median), 310 ms for the slowest, with 55 MB at most; one
process per core converts about 200 pages a second on 4 cores. Pages written to push the limits:

| Page | Size | Markdown | PDF |
| --- | ---: | ---: | ---: |
| 130,000 paragraphs | 48 MB | 1.9 s, 458 MB | 25 s, 2.6 GB |
| One line of 4 million words | 20 MB | 0.7 s, 177 MB | 13 s, 3.6 GB |
| 200,000 boxes of clutter | 12 MB | 0.7 s, 392 MB | 0.6 s, 392 MB |
| 100,000 links in one paragraph | 3.3 MB | 0.6 s, 160 MB | 1.8 s, 400 MB |
| 20,000 headings in a row | 1.5 MB | 0.2 s, 64 MB | 3.6 s, 156 MB |
| 100,000 lines of code | 1.1 MB | 0.1 s, 107 MB | 1.5 s, 390 MB |

Time grows in proportion to the page (`tests/scaling.rs` checks it), except for elements nested
tens of thousands deep, which html5ever builds in a time that grows with the square of the depth.
A laid-out page holds its text until the PDF is written: the 13,000 pages of the 48 MB page take
about 200 MB of memory per 1,000 pages.

All the numbers, and leads for making it faster, are in
[docs/IMPROVEMENTS.md](IMPROVEMENTS.md#10-benchmark-november-2026-and-leads-for-speed-and-memory).

## Web pages as input

A web page is an input like a book: an `.html` (`.htm`, `.xhtml`) file, or the `http(s)` address of
a page (`kakemono convert https://en.wikipedia.org/wiki/Fourier_transform -o ft.md`). The page is
parsed as a browser parses it (html5ever), its article is kept and written as one XHTML document,
which is read as an EPUB book is (above): headings, lists, tables, MathML formulas, code, figures
with their captions, notes and links come out the same way.

- **The article.** Scripts, forms, navigation (`<nav>`, `<aside>`, the header and footer of the
  page), elements with the roles of navigation, banners, dialogs and search, hidden elements
  (`hidden`, `display: none`), and elements whose class or `id` names clutter (menus, sidebars,
  sharing buttons, advertisements, cookie notices, newsletters, "related" lists, comments) are
  removed, unless they hold the title or most of the text. The article is then the element where
  the text stops being in one child, text in links not counting (as Firefox's reader view finds
  it). `<br><br>` ends a paragraph.
- **Title and authors.** The title is the first `<h1>` of the article, else the first of the page
  when it is in the same `<article>` or `<main>` or is the page's `<title>`, else `og:title` or
  the `<title>` without the name of the site. The authors come from `citation_author`,
  `author` or `article:author`, unless the page marks its own (`class="author"`).
- **Pictures.** The best version of each picture is taken (the smallest of a `srcset` at least
  1600 pixels wide, a `<picture>`'s sources but AVIF, the address of a lazy picture in `data-src`
  or in `<noscript>`), fitted to the page; icons and tracking pixels (under 48 pixels) are left
  out. A page at an address has its pictures fetched with it (8 at a time, 300 at most); a file
  has them read next to it; `data:` pictures are inside the page. PNG, JPEG, GIF, BMP, WebP and SVG
  are drawn.
- **Notes.** A short link in a superscript to an item of a list (Wikipedia's `[1]`, most blogs'
  notes) calls a note, which follows its paragraph as a footnote, without its link back; so does
  pandoc's `role="doc-noteref"`.
- **Sites.** arXiv's HTML papers (LaTeXML): notes written in place, each author with their
  addresses, the abstract. Wikipedia: its notes, edit links, maintenance boxes and navigation
  boxes. Sphinx and mdBook: the `¶` and self-links of headings.
- **Encoding.** The byte order mark, the `charset` of the HTTP response, then a `<meta>` near the
  start; otherwise UTF-8, or Windows-1252 for bytes that are not UTF-8.

A page saved as a file is read as at its address with `--base-url` (`kakemono convert
saved.html --base-url https://example.org/a/page.html -o page.md`): its links point there,
and its pictures are fetched from there.

arXiv addresses (`/abs/`, `/pdf/`, `/html/`) still give the paper's PDF, which the analysis reads
with its pages; another address of a PDF, a Word document or an EPUB book gives that document. A
page has no pages: all four HTML modes give the `friendly` article, and `-o page.pdf` lays it out
through a Word document. Pages built by scripts in the browser (an empty `<body>` filled by
JavaScript) cannot be read, nor pages behind a login or a paywall; the pictures of a local file
are not fetched from the Web.

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
- Word input: formulas made with MathType 2 (MTEF 2, in very old `.doc` files) stay pictures;
  floating shapes and text boxes are read where they are anchored in the flowing outputs; the page
  layout has no true transparency (a transparent fill is blended over white) and draws rounded
  and freeform shapes as their bounding rectangle.
- Excel input: `.xls`, `.ods` and `.csv` are not read; pivot tables and comments are left out;
  formulas show the value saved in the file; forms drawn on a grid stay sparse tables (or their
  text) in the document outputs, not rebuilt forms.
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
- **Snapshots**: a hash of each mode's HTML, of the Word, Markdown, LaTeX, Typst and EPUB documents,
  of the editable JSON, of the chunks and of the `--json` document per paper, in `tests/data/snapshots/`.
  When the output changes on purpose, update them and commit them with the code:
  `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers`.

- **Word input** (`tests/word.rs`): `tests/data/word/sample.docx`, written by pandoc from
  `sample.md`, `sample.doc`, the same saved by LibreOffice, and `lists.docx` (nested lists) are
  read and written to every format (snapshot in `tests/data/snapshots/word-sample.txt`); the LIGO
  paper goes through a round trip PDF → Word → kakemono. To make the samples again:
  `cd tests/data/word && pandoc sample.md -o sample.docx && soffice --headless --convert-to doc sample.docx`.
- **Excel input** (`tests/excel.rs`): `tests/data/excel/sample.xlsx`, saved by LibreOffice from the
  hand-written `sample.fods`, and `charts.xlsx` (charts, pictures), written by
  `tests/data/excel/make-chart-xlsx.py`, are read and written to every format (snapshot in
  `tests/data/snapshots/excel-sample.txt`). To make the sample again:
  `soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods`.
- **EPUB input** (`tests/epub.rs`): `tests/data/epub/sample.epub`, written by pandoc from the same
  `tests/data/word/sample.md`, is read like the Word sample and written to every format (snapshot
  in `tests/data/snapshots/epub-sample.txt`); small books built in the test cover style sheets,
  lists, merged cells, notes and pictures, and the LIGO paper goes through a round trip PDF → EPUB
  → kakemono. To make the sample again:
  `cd tests/data/word && pandoc sample.md --mathml -o ../epub/sample.epub`.
- **Web pages** (`tests/html.rs`): `tests/data/html/sample.html`, written by pandoc from the same
  `tests/data/word/sample.md`, reads like the Word sample and the EPUB sample and is written to
  every format (snapshot in `tests/data/snapshots/html-sample.txt`); small pages built in the
  test cover clutter, lazy pictures, links, Wikipedia's and LaTeXML's notes and encodings, and a
  page at an address is read with its pictures fetched by the test. Real pages
  (`tests/web_pages.rs`: Wikipedia in English and Vietnamese, arXiv HTML papers, a news article, a
  research blog, documentation): `scripts/fetch-test-pages.sh` downloads those of
  `tests/data/web-pages.tsv` (not committed); the test checks the title, text that must be there,
  clutter that must not, and lower bounds on words and notes. To make the sample again:
  `cd tests/data/word && pandoc -s --mathml sample.md -o ../html/sample.html`, then point its
  picture to `../word/curve.png`.
- **Web benchmark** ([`scripts/web-bench/`](../scripts/web-bench/README.md)): some 330 pages,
  181 of them with a ground truth (the Scrapinghub article extraction benchmark), scored as
  that benchmark scores article extraction and against trafilatura and readability, written
  to every output, and gathered in review PDFs (the page on its site beside kakemono's
  output) to look through.
- **Word layout fidelity** (CI job "Word layout fidelity"): `scripts/fetch-word-corpus.sh`
  downloads the Word documents of `tests/data/word-corpus.tsv` with the PDF Word made of each
  (public documents: gov.uk, data.gov.uk, data.gov.au, public GitHub repositories; not committed);
  `scripts/word-fidelity.py` lays each out and compares it with Word's PDF line by line
  (`scripts/compare-layout.py`): the share of lines broken at the same words, and of documents
  with the same page count. The job fails below its minimums. Pairs left out, and why, are in
  `tests/data/word-corpus-excluded.tsv`.
- **Word layout fixtures** (`tests/word_layout.rs`): seven documents of that corpus, chosen for
  what they hold (columns, text boxes, floating shapes, frames, EMF pictures, rows of exact height),
  are laid out and compared with Word's PDFs on every CI run, with a minimum per document; they
  are downloaded first (`scripts/fetch-word-corpus.sh 8 $(cat tests/data/word-layout-fixtures.txt)`)
  and skipped when missing.
- **Crash test**: `scripts/word-crash-test.py DIR...` converts every Word file found to every
  output and reports panics and timeouts (run on 4,300 public Word files: none).

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

