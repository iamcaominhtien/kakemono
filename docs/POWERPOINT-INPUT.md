# PowerPoint input (`.pptx`)

Status: implemented. This note describes how kakemono reads a PowerPoint presentation, what it
becomes in each output, and what it leaves out. The reader is `src/powerpoint/`; the slides are
drawn as pages by `src/layout/slides/`. The thresholds of the rules are the fields of
`PowerPointConfig` (`src/powerpoint/mod.rs`, re-exported as `kakemono::config::PowerPointConfig`).

## Why

A deck says little on each slide and much in its order, its titles and its notes. Read as a
document, its slides become headed sections with lists, tables and charts, which every output of
kakemono can show; the editable JSON and the chunks for retrieval keep each slide's title above
its bullets, and the speaker notes, which often hold what a slide only hints at, under it. Drawn
as pages, the same deck gives a PDF of its slides, and the `hard`, `hard-95` and `medium` HTML
modes.

```bash
kakemono convert talk.pptx -o talk.md        # Markdown: a section per slide, its notes after it
kakemono convert talk.pptx -o talk.jsonl     # chunks: each under its slide's title
kakemono convert talk.pptx -o talk.pdf       # the visible slides as PowerPoint draws them, a page each
kakemono convert talk.pptx -m hard-95        # the slides as pages, their text real HTML text
kakemono convert talk.pptx                   # friendly HTML
```

The format is recognised from the file's bytes (a ZIP package whose main part is a presentation),
not from its name: decks, shows and templates, with or without macros (`.pptx`, `.ppsx`, `.potx`,
`.pptm`, `.ppsm`, `.potm`), are the same format; macros are not read. PowerPoint 97–2003 files
(`.ppt`), `.odp` and Keynote files are not read (save them as `.pptx`).

## What a deck becomes (the flowing outputs)

Friendly HTML, Markdown, Word, LaTeX, Typst, EPUB, the editable JSON and the chunks are written
from one reading of the deck. A deck read so has no pages, like a Word document:
`Document::pages` is empty.

- **Hidden slides** are left out of every output, as PowerPoint's own PDF export leaves them out.
- **The document's title** is the deck's title (`docProps/core.xml`), unless it is the title
  PowerPoint gives a new deck ("PowerPoint Presentation"); else the title of its title slide;
  else the file's name.
- **The title slide.** The first slide is the title slide when its title is a centred title
  (`ctrTitle`), when its layout is a title slide's, or when it has a title and, besides it, no
  list, table or chart and at most `title_slide_max_words` (60) words. A first slide with no title
  whose text is a title slide's takes its topmost paragraph, of at most `title_max_words` (30)
  words, as its title: corporate templates set a title slide's lines in body placeholders. Its
  title is the document's title, its other lines the front matter.
- **Every other visible slide** is a section headed by its title (a level 1 heading). Its title is
  its title placeholder's text; else the title found in a text box (below); else "Slide N", `N`
  its number in the deck, so that every chunk has a section path.
- **Sections.** When a deck has PowerPoint sections, each section's name is a level 1 heading and
  its slides' titles level 2 headings; an unnamed section heads nothing, nor does a deck's only
  section (often PowerPoint's "Default Section"). A section header slide whose title repeats its
  section's name gives one heading.
- **Titles in text boxes.** A slide without a title placeholder takes as its title a text box near
  its top (within `title_top_ratio`, 0.25, of the slide's height) whose text is at least
  `title_min_size_pt` (20 pt), `title_size_ratio` (1.1) times larger than any other text of the
  slide, and at most `title_max_words` (30) words in one or two paragraphs without bullets.
- **Reading order.** The title first, then the slide's shapes by where they lie: cut into columns
  or rows by the widest empty band between them, recursively (shapes that overlap by at most
  `reading_overlap_pt`, 2 pt, still count as apart); a group's shapes the same way within the
  group; shapes that overlap more keep the order of the shape tree. The order of the shape tree
  alone is the order the shapes were drawn or pasted in, which few decks set.
- **Text.** A paragraph with a bullet or a number is a list item at its level (0 to 8), an
  automatic number keeping its marker (`a)`, `iv.`, `3.` with `startAt`); other paragraphs are
  paragraphs. Bold, italic, underline, strike, sub- and superscript, and links are kept.
  Fields show their text (`slidenum` the slide's number).
- **Tables** keep their merged cells (`gridSpan`, `rowSpan`, `hMerge`, `vMerge`); the first row is
  a header row when the table says so (`firstRow="1"`).
- **Pictures** are figures with their alternative text (`descr`); an SVG picture is kept over its
  PNG copy. A description that is only a file name or path (older PowerPoints stored
  `C:\Users\…\MC900433934[1].PNG`) is no description. Pictures marked decorative
  (`a16:decorative`), and those of the layout and master, are left out.
- **Charts** are a figure of the chart drawn by kakemono (`src/chart/`, as for Word and Excel),
  followed by a table of its data. Only the values the chart part caches are read.
- **SmartArt** gives its text as a list, each node an item at its depth.
- **Formulas** (`a14:m`) are math, not the fallback picture.
- **Page furniture is left out**: the footer, date and slide-number placeholders, the shapes of the
  layout and master, and short text boxes (at most `furniture_max_words`, 12) repeated with the
  same text near the top or bottom edge (within `furniture_edge_ratio`, 0.15, of the height) of at
  least `furniture_slides_ratio` (half) of the visible slides, and on 3 slides at least: a
  classification banner, a copyright line, a running title. Placeholder prompts ("Click to add
  title") never reach any output: an empty placeholder gives nothing.
- **Speaker notes** follow the content of their slide, under a heading "Notes" one level below
  the slide's, so that a chunk of notes has the path "Slide title › Notes". The notes page's
  slide picture and page number are left out. A slide with no content and no notes gives nothing.

`--content` applies an edited JSON file to the flowing outputs, as for Word; the page outputs
(the PDF, `hard`, `hard-95`, `medium`) show the file as it is.

## What a deck becomes (the pages)

`-o x.pdf` (`Paper::to_pdf`) draws each visible slide as one page of the slide's size, as
PowerPoint draws it; `hard`, `hard-95` and `medium` send that PDF through the PDF pipeline (a
picture of each slide; its text placed as real HTML text; its text reflowed in its columns). Each
slide is painted in this order:

1. **The background** (`p:bg`): a colour, a gradient, a picture, or the theme's background fill,
   taken from the slide, else its layout, else its master.
2. **The master's shapes, then the layout's**, unless the slide hides them (`showMasterSp="0"`);
   their placeholders are templates and are not drawn.
3. **The slide's shapes**, in painting order, groups mapped into their own space:
   - **outlines** (preset and custom geometry), with their fill (solid, see-through, linear and
     radial gradients, pictures, a pattern's foreground) and line, turned and flipped;
   - **pictures**, cropped (`a:srcRect`) and turned; SVG pictures over their PNG; WMF and EMF
     pictures drawn as vectors;
   - **text**, laid out by the Word layout engine within the frame less its insets, anchored at
     the top, middle or bottom: a line is 1.2 times its largest text, as PowerPoint sets it; space
     before and after paragraphs; bullets hung at their indent (Wingdings and Symbol bullets drawn
     with a Unicode look-alike); autofit as PowerPoint wrote it (`fontScale`, `lnSpcReduction`);
     text that is not wrapped placed by its alignment; vertical text turned; links in the theme's
     link colour; highlights;
   - **placeholders** with the position, fill, line and text properties of their layout's and
     master's;
   - **tables** with their table style (PowerPoint's default style is built in) and the cells'
     own fills and borders;
   - **charts** drawn as in the other outputs, their plot area where the file places it;
   - **SmartArt** from the drawing PowerPoint stored with it; **OLE objects** as their picture.

Colours follow the master's colour map and the slide's or layout's override; theme fonts
(`+mj-lt`, `+mn-lt`, …) resolve to the theme's. Fonts are those of the Word layout engine: an
installed font, else a metric-compatible open font (Liberation for Arial and Times New Roman,
Carlito for Calibri), else the built-in metrics of fonts with no open match, else a fallback that
covers the character (Japanese fonts included).

### How close the pages are

`scripts/word-fidelity.py --corpus tests/data/powerpoint-corpus --name PowerPoint` compares our
PDFs with the PDFs PowerPoint made of the decks of `tests/data/powerpoint-corpus.tsv` that publish
one (`scripts/fetch-powerpoint-corpus.sh`), as for Word: 91.6% of PowerPoint's lines break at the
same words in ours, and every deck has as many pages. The CI job `powerpoint-fidelity` fails
below 0.90 lines and 1.0 pages.

Most lines that differ break one word earlier than PowerPoint's: PowerPoint's lines are about
0.6% narrower. Its letters keep their widths, but its spaces (and `v`, `w`, `y`) are narrower or
wider by text size, as if rounded to whole pixels at 96 dpi with Arial's own hinting, which the
open fonts do not have; no rule found so far fits every size.

## Limits

- A deck of more than 5,000 slides is read up to that many; a slide side is at most 4,032 pt
  (56 inches, PowerPoint's largest). Groups nest at most 64 deep, SmartArt diagrams 16 deep with
  10,000 nodes; a table draws at most 10,000 cells.
- A missing part, a relationship that leaves the package or comes back to a part already read,
  and a placeholder whose layout has none to match are skipped or read with the slide's own
  properties; none fails the deck.

## Not read or not drawn

- Animations, transitions, timings and slide-show settings: a slide is drawn in its final state,
  all its shapes shown. Audio and video are not read (a video shows as its poster picture);
  nor are comments and ink.
- Effects (shadows, glow, reflection, soft edges, 3-D bevels) and artistic picture effects.
- Curved connectors and arrows are drawn straight; a bent arrow's bend is not rounded.
- Charts lack trend lines, error bars and the data labels of pies; their category labels may be
  turned or cut short where PowerPoint keeps them flat and whole, and their tick spacing may
  differ.
- Pictures are embedded at their own resolution, so a PDF of a deck with large photographs is
  larger than PowerPoint's, which downsamples them.
- In `medium`, a slide number may stay in a slide's text when a deck is too short for it to be
  recognised as a footer.

## Testing

- `tests/powerpoint.rs`: behaviour tests, named after the behaviour, on the sample and on decks
  built in the tests; the snapshot `tests/data/snapshots/powerpoint-sample.txt` of every flowing
  output of the sample. `src/layout/slides/tests.rs`: the pages.
- The sample `tests/data/powerpoint/sample.pptx` is written by
  `python3 -I tests/data/powerpoint/make-sample-pptx.py` (the standard library only).
- The corpus: `scripts/fetch-powerpoint-corpus.sh 8` downloads the 113 public decks of
  `tests/data/powerpoint-corpus.tsv` (English, Japanese and French, from government data sites and
  public repositories), with PowerPoint's PDF of five of them; it is never committed.
  `python3 scripts/word-crash-test.py tests/data/powerpoint-corpus` writes every output of every
  deck and reports panics and timeouts.
