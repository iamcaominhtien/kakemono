# PowerPoint input (`.pptx`) — design

Date: 2026-11-28. Status: directions decided by the user (2026-11-28):
`.pptx` first (`.ppt` later, as its own work); the page modes (PDF, `hard`,
`hard-95`, `medium`) are part of v1, each slide a page; speaker notes come
after the content of their slide; a slide's title is a heading.

## Intent

Accept a PowerPoint presentation as input to `kakemono convert`, next to PDF,
Word, Excel, EPUB and web pages, so that a deck can be:

- **read as a document** (friendly HTML, Markdown, docx, LaTeX, Typst, EPUB):
  slide titles as headings, bullets as lists, tables as tables, charts as a
  chart and its data;
- **fed to AI**: chunks (`.jsonl`) and the editable JSON, each chunk headed by
  its slide's title, speaker notes included (they often hold what the slide
  only hints at);
- **seen as the slides**: `-o x.pdf`, `hard`, `hard-95`, `medium`, each slide
  drawn as PowerPoint draws it.

## Goals

- Read `.pptx` (and `.pptm`, `.ppsx`, `.ppsm`, `.potx`: the same package with
  another content type) with a reader of our own, into `word::ir` with Word
  style names, so that `word/build.rs` makes the `Document` (as EPUB does):
  lists, tables with merged cells, pictures, charts, formulas, notes and links
  already work there.
- Every output works, the page modes included.
- Text gets the formatting it has on the slide, through PowerPoint's
  inheritance: slide → slide layout → slide master → the presentation's
  default text style → theme (fonts `+mj-lt`/`+mn-lt`, colours through the
  master's colour map).
- A slide's PDF page matches PowerPoint's own PDF export: same page size,
  shapes where they are, text broken into the same lines in most cases,
  measured on a corpus of decks with the PDF PowerPoint made of them, as
  `scripts/word-fidelity.py` does for Word.

## Non-goals (for now)

- `.ppt` (PowerPoint 97–2003 binary): later, on top of `word/doc/cfb`.
- `.odp`, Keynote, Google Slides (exported `.pptx` works).
- Animations, transitions, timings, triggers, slide-show settings: a slide is
  drawn in its final state, all its shapes shown.
- Audio and video: their poster picture is shown, the media left out.
- Effects (shadows, glow, reflection, soft edges, 3-D bevels), pattern fills,
  artistic picture effects: not drawn. Gradient fills: drawn as gradients
  where krilla allows (linear and radial), else their first stop.
- Comments, ink annotations, hidden slides (left out of every output, as
  PowerPoint's PDF export leaves them out by default).
- Editing the slides' text in the page modes with `--content`: as for Word,
  edited text reaches the flowing outputs; the page outputs show the file.

## Anti-goals (failure even if it "works")

- Placeholder prompt text ("Click to add title") in any output: an empty
  placeholder gives nothing.
- Slide numbers, dates and footers repeated as paragraphs on every slide in
  the document (they are page furniture: drawn on the page, left out of the
  text).
- A deck whose every slide is headed "Slide N" when its slides have titles.
- A chunk of a slide's bullets without the slide's title.
- Text overflowing its box in the PDF where PowerPoint shrank it (autofit).
- A crash, hang or memory blow-up on a large or hostile deck (zip bomb,
  thousands of slides, a group nested a thousand deep, a picture of 100 000 ×
  100 000 pixels): same limits as the Word and Excel readers.

## Constraints

- Public API: `InputFormat` is `#[non_exhaustive]`, so `InputFormat::Pptx`
  (`name() = "pptx"`, `has_pages() = false`) is a new value, not a breaking
  change (`feat:`). The editable JSON gets a new `input format` value only: no
  `FORMAT_VERSION` bump.
- No new dependency: `word::zip`, `roxmltree` (as `word::docx::parse`), the
  DrawingML code of `excel/drawing.rs` and `excel/view.rs` (colours, preset
  geometry, group transforms, shape outlines as SVG), `chart/`, `word::omml`,
  `word::pictures`, and the layout engine (`layout/fonts`, `shape`, `para`,
  `display`, `pdf`).
- Dependencies keep one direction: `powerpoint → word, excel (drawing), chart,
  model`; `layout → powerpoint` for the slides' pages (as `layout → excel`
  for the workbook print).
- Thresholds in a documented config struct with unit suffixes
  (`PowerPointConfig`, re-exported by `config.rs`); no magic numbers.
- Test data: a small deck made by a script of ours, committed with its
  source, and a snapshot `powerpoint-sample.txt` of every output.

## Approaches considered

1. **Through `word::ir`** — **chosen** (recommended, approved): the slides'
   text becomes Word-style paragraphs (`Title`, `heading 1`, list paragraphs
   with levels, `Caption`), tables become `ir::Table`, and `word/build.rs`
   decides block kinds, as for EPUB and web pages. Least code, and every rule
   about lists, notes, captions and links is shared.
2. Directly into `Document`, as Excel does: full control, but it would
   rebuild lists, merged cells, notes, formulas and links that `word/build.rs`
   already does well. A slide has nothing as specific as a spreadsheet's
   header rows to justify it.
3. Through LibreOffice (`soffice --convert-to pdf`) then the PDF pipeline:
   rejected — a runtime dependency, loses structure (lists, tables, notes).

For the pages, the slides are drawn by our own code into the layout engine's
display pages (`layout/display.rs`), as the workbook print does
(`layout/workbook/print.rs`), and written by `layout/pdf.rs`. `hard`,
`hard-95` and `medium` then go through the PDF pipeline, as for Word
(`Paper::laid`).

## Design

### Module `src/powerpoint/`

- `mod.rs`: `detect(data) -> bool` (a ZIP whose `_rels/.rels` office document
  is a presentation part: `ppt/presentation.xml`), `read(data, source, config)
  -> Result<Document>`, `PowerPointConfig`.
- `package.rs`: the presentation part: slide size (`p:sldSz`, EMU → pt), the
  slide list in order (`p:sldIdLst` → relationships), hidden slides
  (`show="0"`), sections (`p14:sectionLst`: name and slide ids), the notes
  master, the default text style, `docProps/core.xml` title.
- `inherit.rs`: a slide's layout and master; placeholders matched by `type`
  and `idx` (a slide's `p:ph idx="1"` takes the layout's placeholder of the
  same `idx`, else of the same type; the layout's takes the master's of the
  same type; `body` stands for `obj` and an untyped placeholder); position and
  size (`a:xfrm`) inherited when absent; text properties merged level by level
  (`a:lvlNpPr` of the shape's `a:lstStyle` → layout placeholder → master
  placeholder → master's `p:txStyles` `titleStyle`/`bodyStyle`/`otherStyle` →
  `p:defaultTextStyle`); theme fonts and colours (`a:clrScheme`, `p:clrMap`,
  `p:clrMapOvr`) through `chart::Theme` and `chart::theme::color` (extended with the theme fonts and `alpha`).
- `text.rs`: a text body (`p:txBody`) → paragraphs with their resolved level
  properties: bullets (`a:buChar`, `a:buAutoNum` with its scheme and
  `startAt`, `a:buNone`, `a:buBlip` as a bullet), indents (`marL`, `indent`),
  alignment, spacing (`a:lnSpc`, `a:spcBef`, `a:spcAft` in percent or
  points), runs (font, size, bold, italic, underline, strike, baseline for
  sub/superscript, colour, caps, `a:hlinkClick`), `a:br`, fields (`a:fld`:
  `slidenum` gets the slide's number, `datetime*` its cached text), formulas
  (`a14:m` in `mc:AlternateContent`: the OMML through `word::omml`, else the
  fallback picture), and the body's properties (`a:bodyPr`: insets, anchor,
  wrap, columns `numCol`, vertical text `vert`, autofit `a:normAutofit
  fontScale lnSpcReduction`, `a:spAutoFit`).
- `shapes.rs`: a slide's shape tree (`p:spTree`) in painting order: shapes
  (`p:sp`, with the preset or custom geometry, fill and line of
  `excel/drawing.rs`), pictures (`p:pic`: `a:blip r:embed`, `asvg:svgBlip`
  over its PNG, `a:srcRect` crop, linked pictures left out), groups
  (`p:grpSp`, child offsets and extents mapped as `excel/drawing.rs`
  `group_map` does), connectors (`p:cxnSp`), graphic frames (`a:tbl` tables
  with `tableStyles.xml` looks; `c:chart` charts through `chart::place`;
  SmartArt `dgm` through its stored drawing `ppt/diagrams/drawingN.xml`,
  read as a group of shapes; OLE objects as their picture), and `mc:
  AlternateContent` (the choice we read, else the fallback).
- `notes.rs`: the notes slide of a slide (`ppt/notesSlides/`): the text of its
  body placeholder; the slide image placeholder and the notes page's number
  are left out.
- `build.rs`: a slide → `word::ir` items (see "What a deck becomes").
- `draw.rs` lives in `layout/slides/` (below), not here.

### What a deck becomes (the flowing outputs)

- `meta.title`: core properties title; else the title of the first slide when
  that slide is a title slide (its title placeholder is `ctrTitle`).
- A **title slide** (first slide with a `ctrTitle`): its title → `Title`
  style (the document's title), its subtitle → `Subtitle` (front-matter note),
  its other text → paragraphs.
- Each other **visible slide**, in order:
  - its title placeholder (`title`, `ctrTitle`) → `heading 1`; with
    PowerPoint sections, each section's name is a `heading 1` and its slides'
    titles `heading 2`. A slide without a title (or with an empty one) is
    headed "Slide N" (`N` its number in the deck), so that every chunk has a
    section path;
  - the rest in **reading order**, the title first. *Changed after the
    first review round (2026-11-28):* not the order of the shape tree, which
    few decks set (shapes come in the order they were drawn or pasted), but
    their position: the shapes are cut recursively into columns or rows by
    the widest empty band between them (XY-cut), a group's shapes likewise
    in the group's own space; shapes that overlap so that no band separates
    them keep their tree order;
  - a slide without a title placeholder takes its **title from a text
    box** (`PowerPointConfig`): the first paragraph of a box near the top
    clearly larger than the rest of the slide's text, else a short box near
    the top whose text is clearly the largest, else the one short, large
    text box of a slide that has no other (a section divider);
  - the **first slide is the title slide** when its title is a `ctrTitle`,
    its layout is a title slide's (`type="title"`), or it has a title and
    otherwise no list, table or chart and little text; its other text is
    front matter;
  - short text boxes repeated with the same text near the top or bottom
    edge of at least half of the slides (a classification banner, a
    copyright line, a running title) are page furniture, left out, after
    the titles are found;
  - a picture's description that is only a file name or path (older
    PowerPoints stored `C:\Users\...\MC900433934[1].PNG`) is no
    description;
  - text: each paragraph with a bullet or a number → a list paragraph of its
    level (`lvl` 0–8 → `ListMark::level`, the number's text as marker); other
    paragraphs → `Normal`; a run's style as `Span` style bits; links kept;
  - tables → `ir::Table` (`gridSpan`, `rowSpan`, `hMerge`, `vMerge`), the
    first row a header row when the table's `a:tblPr firstRow="1"`;
  - pictures → figures (alt text from `p:cNvPr descr`, caption none, unless a
    text box right after it is styled as a caption — left to the builder);
    decorative pictures (`a16:decorative`) and pictures of the layout or
    master left out;
  - charts → a figure and its data table, as in Word and Excel;
  - SmartArt → its text as a list (each node a list item at its depth), not
    a picture of it in the flowing outputs;
  - formulas → `Inline::Math`;
  - footer, date and slide-number placeholders (`ftr`, `dt`, `sldNum`)
    and shapes of the layout and master → left out (page furniture);
  - **speaker notes** (when the notes slide has text) → a `heading N+1`
    "Notes" under the slide's heading, then its paragraphs and lists, as
    above. So a chunk of notes has the path "Slide title › Notes".
- An empty slide (no text, picture, table or chart) gives its heading only
  when it has notes; otherwise nothing.

### What a deck becomes (the pages)

New `src/layout/slides/` (`mod.rs`, `text.rs`, `table.rs`): each visible slide
→ one `display::Page` of the slide's size, painted in this order:

1. the background (`p:bg`: `a:solidFill`, `a:gradFill`, `a:blipFill`
   stretched or tiled, `p:bgRef` to the theme's background fills), inherited
   slide → layout → master;
2. the master's shapes, then the layout's, when `showMasterSp` is not `0`,
   placeholders excepted (a placeholder of the master is only a template);
3. the slide's shapes in painting order. Shapes: geometry, fill and line as
   SVG pictures, drawn by `excel::view::shape_svg` (moved to a shared place
   if needed); rotation and flips. Text frames: laid out with `layout::para`
   (the fonts of `layout/fonts`: Carlito for Calibri, the built-in metrics of
   `metrics.txt` for Aptos and others), within the frame less its insets,
   anchored top, middle or bottom, with bullets hung at `marL + indent`,
   autofit applied (`fontScale` and `lnSpcReduction` as written by
   PowerPoint; `spAutoFit` grows nothing: PowerPoint wrote the size it
   grew to), columns, and vertical text (`vert`, `eaVert`) turned;
   pictures cropped; tables with their style's fills and borders (first row,
   banded rows, first column), cell margins and text laid out as above;
   charts as their SVG; footer, date and slide-number placeholders with the
   slide's number and the cached date.
4. The pages are written by `layout/pdf.rs` (`Paper::to_pdf`), with the
   document title and a bookmark per slide title.

`hard`, `hard-95` and `medium`: that PDF goes through the PDF pipeline
(`Paper::laid`, which today serves Word only, extended to presentations).

### Wiring

- `InputFormat::Pptx`; `Paper::from_bytes`: `powerpoint::detect` before
  `word::detect` (all three are zips; each looks for its own main part).
  `Source::Slides(Box<Document>, Vec<u8>, PowerPointConfig)`: the file is kept
  to draw the pages. `analyze_with`, `to_html`, `write_html`, `to_pdf`,
  `laid`, `apply_content` (edited text reaches the flowing outputs, as for
  Word), `word_format` (none) handled.
- CLI: help texts and extensions (`.pptx`, `.pptm`, `.ppsx`, `.ppsm`,
  `.potx`); `inspect` works on the document.
- Python: `format()` passes `"pptx"`; `to_pdf` accepts a presentation; README,
  ARCHITECTURE, CLAUDE.md, `docs/GUIDE.md`, a new `docs/POWERPOINT-INPUT.md`.

## Corpus and fidelity

- `tests/data/powerpoint-corpus.tsv` (id, sha256, URL of the deck, sha256 and
  URL of the PDF PowerPoint made of it, when published), downloaded by
  `scripts/fetch-powerpoint-corpus.sh` into `tests/data/powerpoint-corpus/`,
  never committed. Sources: government sites that publish a deck with its PDF
  (gov.uk, data.gov.au, open.canada.ca, data.gov.uk), and public GitHub
  repositories of talks (their licences). Target: about 100 decks, at least
  half with a PowerPoint PDF, in several languages (English, Japanese,
  Vietnamese), with charts, tables, SmartArt and formulas among them.
- `scripts/word-fidelity.py` generalised (`--corpus-file`) or a sibling
  `scripts/slides-fidelity.py`: per deck, the share of PowerPoint's lines
  broken at the same words in ours (`scripts/compare-layout.py`), and the
  page count; totals with minimums in `ci.yml`, raised as they improve.
- `scripts/word-crash-test.py` takes `.pptx` too: every output of every deck,
  no panic, no timeout.

## Tests

- Unit tests: placeholder inheritance (by `idx`, by type, `body` for `obj`),
  level properties merged, colour map and theme colours, bullets and
  auto-numbers (`arabicPeriod`, `alphaLcParenR`, `romanUcPeriod`, `startAt`),
  autofit, group transforms with `chExt` 0, a table's merges.
- `tests/data/powerpoint/sample.pptx`, written by
  `python3 -I tests/data/powerpoint/make-sample-pptx.py` (the standard library
  only, the parts written as XML, as `make-chart-xlsx.py` does), committed
  with it; checked once by opening it in LibreOffice: a title slide, a
  bullet slide with three levels and a numbered list, a table with merged
  cells, a picture, a chart, a slide without a title, a hidden slide, a slide
  with notes, two sections, a formula, a SmartArt with its stored drawing. A
  snapshot `tests/data/snapshots/powerpoint-sample.txt` of every output,
  `tests/powerpoint.rs` with behaviour tests named after the behaviour
  (`placeholder_prompts_are_not_text`, `notes_follow_their_slide`,
  `hidden_slides_are_left_out`, `slides_without_title_are_numbered`).
- `tests/cli.rs`: `-o x.pdf` of the sample has one page per visible slide of
  the slide size; Typst and LaTeX outputs of the sample compile.
- Hostile input: a deck whose slide refers to a missing layout, a cycle of
  relationships, a group nested past the limit, a huge slide size → an error
  or a best effort, never a panic.

## Left to the builder

- The exact file split, and whether `excel/drawing.rs`'s DrawingML pieces
  move to a shared `drawingml` module now or when a second user needs them.
- How a caption is recognised under a picture (if at all in v1).
- Gradient support beyond what krilla gives directly.
