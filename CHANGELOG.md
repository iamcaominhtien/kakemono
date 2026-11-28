# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.19.1](https://github.com/iamcaominhtien/kakemono/compare/v0.19.0...v0.19.1) - 2026-11-27

### Performance

- Convert long documents and unusual web pages in linear time, with less memory

Long documents and unusual web pages now convert in time in proportion to their size, and a document laid out as a PDF holds about 40% less memory. Every output stays the same.

#### PDF of a long document

The pages of a document are held until the PDF is written. They now take less room:

- The runs of a line in one face are joined as they are placed on the page: one run per line instead of one per word. The PDF writer already drew them as one.
- A glyph takes 28 bytes instead of 40, the spare room of each page's vectors is freed once the page is drawn, and the writer drops each page once drawn.
- The pieces of a run share its format instead of holding a copy each.
- Each font family's face and built-in metrics are resolved once, not for each run.

| Web page to PDF | Before | After |
| --- | --- | --- |
| 130,000 paragraphs (48 MB, 13,000 pages) | 40 s, 4.6 GB | 25 s, 2.6 GB |
| One line of 4 million words (20 MB) | 18 s, 7.3 GB | 13 s, 3.6 GB |
| 4 MB of paragraphs (1,084 pages) | 2.8 s, 412 MB | 2.1 s, 248 MB |
| The 483 documents of the Word corpus | 106 s | 101 s |

#### Pages whose time grew with the square of their size

Each of these was found with pages written to repeat one thing 20,000 times:

| Page | Where | Before | After |
| --- | --- | --- | --- |
| 20,000 list items with a link | `html/clean.rs`, counts | 52 s | 0.2 s |
| 20,000 spans of class `menu` in a paragraph | `html/clean.rs`, clutter | 28 s | 0.1 s |
| 20,000 number paragraphs in one box | `html/clean.rs`, counts | 7 s | 0.1 s |
| 20,000 double line breaks | `html/clean.rs` | 1.8 s | 0.1 s |
| 100,000 lines of code | `word/build.rs`, lines | 28 s | 0.1 s |
| 20,000 notes cited from one mark, or headings with the same words | `word/build.rs`, anchors | 16–20 s | 0.2–0.3 s |
| 100,000 lines of code, as chunks | `render/chunks` | 9 s | 0.2 s |
| 20,000 headings in a row, as a PDF | `layout/flow.rs`, kept with next | over 300 s | 3.6 s |
| 100,000 links in a paragraph, as a PDF | `layout/para.rs`, words and runs | 124 s | 1.8 s |

The fixes:

- A parent holds more text than a child: the check stops at the first other text, instead of comparing their whole texts for each child. The counts go out at once.
- Each parent's line breaks are walked once.
- Anchors of the same base are numbered on from the last one given.
- A span is split into lines without copying its text for each line.
- The chunker counts heads of a long text that double in length, not the whole text for each piece it cuts.
- A chain of paragraphs kept with the next is measured until it is taller than a page.
- Each word looks only at the runs it overlaps.

Elements nested tens of thousands deep are still slow (11 s for 50,000): almost all of it is html5ever building the tree.

#### Tools

- `scripts/web-bench/perf.py` measures speed and memory:
  - `sweep`: every benchmark page, one at a time;
  - `parallel N`: all of them, N at a time;
  - `formats`: every output of the largest pages;
  - `stress [--pdf]`: pages written to push the limits.
- `docs/GUIDE.md` has the numbers for web pages.

## [0.19.0](https://github.com/iamcaominhtien/kakemono/compare/v0.18.0...v0.19.0) - 2026-11-27

### Added

- Read web pages (HTML files and http(s) addresses) as input

kakemono now reads web pages. You can give it a saved `.html` file or an `http(s)` address, and convert it to any output: the four HTML modes, Word, Markdown, LaTeX, Typst, EPUB, JSON and chunks. It finds the article in the page, leaves out what is around it, and fetches its pictures.

```bash
kakemono convert https://example.org/post -o post.md
kakemono convert saved.html --base-url https://example.org/post -o post.docx
```

#### Reading a page

- The page is parsed with html5ever. The article is the part of the page that holds most of its text outside links.
- Around and inside the article, kakemono leaves out what a site puts there:
  - navigation, headers and footers, ads, cookie banners and pop-ups;
  - share buttons, newsletter and feedback forms, tooltips;
  - lists of related or recommended articles, teasers, tags;
  - author boxes and author photos, comment links, view counts;
  - the icons of the site, and drawings that only point to the site's sprite.
- The article keeps its title, authors, summary, headings, lists, tables, code, formulas (MathML) and pictures.
  - A picture that fails to download is retried, then fetched in another size.
  - When an article has no picture of its own, the photo shown above it (`og:image`) is kept.
- Notes and citations that link to a list become footnotes. The heading of a list that is left empty this way goes too.
- Some sites get their own handling:
  - Wikipedia: notes, hatnotes, portals, sister projects and succession tables.
  - arXiv (LaTeXML): authors, notes and equations.
  - Journals (PLOS, PMC, Nature): references without their links to Google Scholar or PubMed, and without a number written twice.
- A page that is a table (Hacker News) reads as a table without its empty columns. A home page takes its title from its metadata, not from its first story. A page with no article (a video) reads its description.
- `--base-url` reads a saved page as if it were at its address, so its links and pictures resolve.

#### Web benchmark

`scripts/web-bench/` scores kakemono on about 330 pages, the way the Scrapinghub article extraction benchmark does: F1 of 4-word shingles. 181 of the pages have a ground truth. The others are, by category:

- Vietnamese and international news;
- tech news, blogs and documentation;
- Wikipedia in seven languages;
- science, government sites, and pages that are not articles.

It also writes every page to every output to find crashes. For each category it makes a review PDF and a zip of the conversions.

| Category | Pages | F1 |
| --- | --- | --- |
| Ground truth (Scrapinghub) | 181 | 0.928 |
| Government | 13 | 0.970 |
| Blogs | 21 | 0.956 |
| Documentation | 19 | 0.956 |
| International news | 20 | 0.913 |
| Science | 7 | 0.905 |
| Vietnamese news | 31 | 0.900 |
| Tech news | 12 | 0.866 |
| Wikipedia | 15 | 0.849 |
| Not articles | 9 | 0.803 |
| All | 328 | 0.920 |

On the pages with a ground truth, trafilatura and readability-lxml are scored as well. Outside the ground truth, the reference is trafilatura's own extraction, so its mistakes count against kakemono. For example:

- trafilatura leaves out formulas and image captions;
- Chinese and Japanese paragraphs are one shingle each, so a different note number fails the whole paragraph.

#### Other fixes

- The friendly view lets the article take the whole width when it has no table of contents.
- The LaTeX output writes the Greek math alphabets as commands.
- A table that only lays out an equation without a number (from Word, a book or a web page) reads as a displayed equation.

#### Performance

The 330 pages of the benchmark convert to Markdown in 10 ms each (median), 310 ms for the slowest (arXiv's Llama 3 paper), with 55 MB of memory at most. On 4 cores that is about 200 pages a second.

Larger generated pages convert too:

| Page | Size | Time | Memory |
| --- | --- | --- | --- |
| 130,000 paragraphs | 48 MB | 2.0 s | 460 MB |
| 200,000 boxes of clutter | 12 MB | 0.8 s | 390 MB |
| A single 20 MB line | 20 MB | 0.8 s | 175 MB |
| A table of 5,000 rows | 1.7 MB | 0.4 s | 150 MB |
| Elements nested 20,000 deep | 0.2 MB | 1.5 s | 21 MB |

The clutter is removed in linear time. Elements nested 50,000 deep take 11 s, most of it in html5ever's building of so deep a tree.

## [0.18.0](https://github.com/iamcaominhtien/kakemono/compare/v0.17.1...v0.18.0) - 2026-11-24

### Added

- *(typst)* Add Typst output with a printed-book layout ([#34](https://github.com/iamcaominhtien/kakemono/pull/34))

Closes #19.

#### What

A new output format, `-f typst` (or `-o x.typ`): a Typst document that compiles with `typst compile`, with no TeX installation. Typst breaks each paragraph as a whole (no wide gaps in justified lines) and keeps every footnote on the page of its call, which is what a translated book needs.

- **The file** starts with its settings (layout, page size, fonts, language, cover), then the template (the `kakemono-…` functions that lay the document out), then the text. A reader changes the page size or the fonts there, so the CLI gains only `--layout`.
- **Two layouts**, chosen from the document (`--layout auto`, the default) or forced with `--layout book|article`:
  - `book`, for a document with chapters and no abstract, references or arXiv id: A5, a title page, chapters that open on a right-hand page and number their notes from 1, mirrored margins, running heads (book title on the left, chapter on the right, none on chapter openings or blank pages), opening words in small capitals (not in Vietnamese);
  - `article`: A4, the title block at the top, page numbers at the foot.
- **Content**:
  - headings, lists with the document's markers, and tables as `table`s with spanning cells and booktabs-like rules;
  - formulas as Typst math, built from the same `MathNode` structure as LaTeX and MathML;
  - footnotes and added notes (`<note>`) as `#footnote` (see below);
  - links, and pictures as files;
  - a table of contents becomes Typst's `outline`, with the new page numbers.
- **Language**: `lang` is found from the text (Vietnamese), for hyphenation and quotes. The default font, Libertinus Serif, ships with Typst and has the Vietnamese letters.
- **Library**: `Paper::to_typst`, `TypstOptions`, `TypstLayout`. **Python**: `to_typst`.
- **EPUB books** (now an input, #35) go to Typst too: a novel gets the book layout, with its title page and chapters.
- Compiling to PDF inside kakemono (the optional point 3 of the issue) is left out: the `typst` crate would add a large dependency to the published crate, and the `typst` CLI is a single binary.

#### Fixed: line breaks in every output

A `<br>` of an EPUB book, or a manual line break of a Word document, is a `\n` in the text, and every output wrote it as a space, so the lines of a poem ran together (*The Divine Comedy* came out as prose). Each line of a run is now styled on its own, and the lines are joined by the format's line break: `<br>` in HTML, EPUB and Markdown, `\newline{}` in LaTeX (a space inside a table), `#linebreak()` in Typst. Word already wrote `<w:br/>`. The PDF papers have line breaks only in code, which is written apart, so their outputs are unchanged.

#### Footnotes at the foot of the page (Word, LaTeX, Typst)

A footnote reached the outputs as a block after its paragraph, linked from a raised mark: Word, LaTeX and Typst wrote it as a small paragraph there, not as a footnote. The EPUB books do the same: their notes, which e-readers show in a popup, are moved after the paragraph that calls them by the EPUB reader.

These three outputs now attach each footnote to its call (`render/notes.rs`, `footnotes_attached`), so that it becomes a Word footnote, a `\footnote` or a `#footnote`, at the foot of the page of its call. A note is attached when its mark is called once, from a paragraph or a list item, and the note follows the call (after only other notes, or on the same page of the PDF or the next). Endnotes of Word, notes called from a heading, a caption, a table or the authors, and notes called from more than one place stay as they were. The document model, the JSON and the other outputs do not change.

- *The Divine Comedy*: 2,089 of its 2,103 notes become footnotes (the other 14 are also referred to by other notes); *War and Peace*: all 137.
- The PDF papers: 69 of their 94 footnotes (the rest are author notes or affiliations).
- The Word corpus: 1,666 footnotes in 148 documents.

#### Fixed: SonarCloud security rating of `main`

The quality gate of `main` is red since #32: SonarCloud reports a command argument injection (`pythonsecurity:S8705`) in `scripts/ci_changes.py`, whose `BASE` and `HEAD` went from the command line to `git`, which would take an argument starting with `-` for an option. Each is now a commit hash or a branch or tag name, resolved with `git rev-parse --verify --end-of-options`, and only the full hash it prints is used; anything else exits with code 2. The workflows pass the SHAs of the pull request, so CI is unchanged.

## [0.17.1](https://github.com/iamcaominhtien/kakemono/compare/v0.17.0...v0.17.1) - 2026-11-24

### Performance

- *(layout)* Draw each line's text of one font as one text object

The PDF that the page layout writes (Word and EPUB input laid out as a PDF, and the `hard` and `hard-95` modes of Word input, which read that PDF) draws the text of a line in one font as one text object, instead of one per word and one per space. The PDF is faster to write and smaller, and looks the same.

Before, every text run was its own text object with its own text matrix, font and colour, and a run in a stand-in face (a metric-compatible font standing in for an Office font) was also wrapped in a transformation for its horizontal scale, even when that scale was 1 to within rounding (1.0000001). Now consecutive runs of one face, size, colour and faked styles on one baseline are drawn as one, each glyph still at its own position, and a scale within 1e-4 of 1 is none.

| Document | Instructions to write the PDF (callgrind) | PDF size |
| --- | --- | --- |
| Ulysses (EPUB, 4,485 pages) | 23.8 → 19.1 billion (−20%) | 9.7 → 7.8 MB (−19%) |
| A government report (Word, 48 pages) | 2.07 → 1.78 billion (−14%) | 516 → 324 KB (−37%) |
| Another one (Word, 33 pages) | 1.36 → 1.14 billion (−16%) | 380 → 238 KB (−38%) |
| Moby-Dick (EPUB, 323 pages) | | 4.9 → 3.2 MB (−35%) |
| The Mahabharata, 150 chapters (EPUB, Devanagari) | 7.46 → 7.37 billion (−1%) | 3.52 → 3.60 MB (+2%) |

A book in Devanagari gains nothing: its words are set in a fallback face and the spaces between them in the run's own font, so no two neighbouring runs share a face.

## [0.17.0](https://github.com/iamcaominhtien/kakemono/compare/v0.16.1...v0.17.0) - 2026-11-23

### Added

- [**breaking**] Read EPUB books as input

An EPUB book (`.epub`) is now an input like a PDF or a Word document, for every output: HTML, Word, Markdown, LaTeX, EPUB, editable JSON, chunks for retrieval, and PDF (the book is laid out in pages through a Word document, as for Word input).

```bash
kakemono convert book.epub -o book.md
```

The content documents of the book (XHTML), in the order of its spine, are read without layout analysis into the same intermediate form as a Word document, so the same rules give each paragraph its kind:

- headings (raised together so that the highest is of level 1, since many books keep `<h1>` for the title); the title and authors from the title page, or else from the book's metadata (`dc:title`, `dc:creator`); `<pre>` as code; `<figcaption>` and a table's `<caption>` as captions; ordered and bullet lists with their markers (`start`, `type`, `value`) and nesting;
- MathML formulas keep their structure (MathML, LaTeX and Office Math in the outputs, in running text too);
- tables with `colspan`, `rowspan` and header rows; pictures (`<img>`, an `<svg>` that shows one, or an SVG drawing) at the size the book gives them; SVG pages of the spine (comics, picture books) as figures;
- a note called by an `epub:type="noteref"` (or `role="doc-noteref"`) link follows the paragraph that calls it, without its link back; links between the documents of the book keep working;
- bold, italics, monospace, scripts and small capitals from the elements and from simple style sheet rules (`.italic { font-style: italic }`); `display: none`, page break markers and the navigation document are left out; HTML named entities (`&nbsp;`) are understood.

A book has no pages: all four HTML modes give the `friendly` article. Books protected by DRM cannot be read (the error says so). The outputs name the source in their colophon ("Converted from EPUB by kakemono").

The Python package reads EPUB books too (`kakemono.Document("book.epub")`).

Reading is fast: on 172 public books (141 from Standard Ebooks, the 31 IDPF EPUB 3 samples; 294 MB), a book is written as Markdown in 0.03 s (median; 0.11 s at the 95th percentile) with 28 MB of memory, about 1,300 books a minute in one process and 5,300 on 4 cores.

##### Performance

- The page layout (PDF output of Word and EPUB input, and the `hard` and `hard-95` modes of Word input) is faster, with the same PDF:
  - each font is read once, instead of once per new piece of text (rustybuzz parses the font's substitution and positioning tables each time it reads it);
  - a shaping plan is made once per font, script and direction, instead of once per piece of text;
  - the font of a run is resolved once per piece of text, instead of once per character.

  A book in Devanagari (the Mahabharata sample, 3,767 pages) is laid out in about 16 s instead of 35 s; measured in instructions, the layout of its first 150 chapters takes 35% fewer after the font fix. Word documents are 4–30% faster.

##### Fixed

- Code listings of Word documents came out empty in every output (HTML, Word, Markdown, LaTeX, EPUB): a listing was rebuilt only from the lines of PDF pages, which a Word document does not have.

##### Breaking changes

- `InputFormat` has a new variant, `Epub`, and is now `#[non_exhaustive]`: a `match` on it needs a `_` arm.
- `Meta` has a new public field, `source_format` (`Some("epub")` for a book). It is left out of the `--json` output when empty, so the output for PDF and Word input is unchanged.

## [0.16.1](https://github.com/iamcaominhtien/kakemono/compare/v0.16.0...v0.16.1) - 2026-11-23

### Documentation

- Streamline README and skip CI for documentation-only PRs ([#32](https://github.com/iamcaominhtien/kakemono/pull/32))

- Shorten the README and preserve detailed documentation in a linked guide.
- Clarify how edited JSON applies to Word page-layout outputs.
- Update crate and Python package descriptions and PyPI keywords to reflect supported formats. Published package metadata will update with the next release.
- Skip the builds and tests on pull requests that only change the documentation. `scripts/ci_changes.py` decides it: Markdown at the root, `docs/`, `python/README.md` or `LICENSE`, plus the release such a pull request carries (the version and metadata of `Cargo.toml` and `python/pyproject.toml`, the kakemono version in `Cargo.lock`). Lint, tests, Word fidelity, SonarQube and the Python tests and wheels are then skipped; the title and version check still runs.

## [0.16.0](https://github.com/iamcaominhtien/kakemono/compare/v0.15.3...v0.16.0) - 2026-11-23

### Added

- *(python)* Add Document, render_svg, read_formula and to_pdf ([#31](https://github.com/iamcaominhtien/kakemono/pull/31))

The Python package can now do everything the command line does, and it writes several outputs of one document up to twice as fast. Closes #30.

##### `render_svg`: draw an SVG as a PNG

`kakemono.render_svg(svg, path=None, *, scale=1.0, resources_dir=None)` does what `kakemono render-svg` does, with the same renderer and fonts (the system's and those of `KAKEMONO_FONT_DIRS`). `svg` is a path, or the SVG as text or bytes. The function returns the PNG's bytes, and also writes them to `path` when it is given. Pictures named by relative paths are read from the SVG's folder, or from `resources_dir`. An SVG it cannot draw raises a `ValueError` that says what is wrong (`cannot draw the SVG: ...`). The fonts are loaded once per process, so drawing many previews from Python costs less than running the program each time.

##### `to_pdf` and `read_formula`

- `kakemono.to_pdf("report.docx", "report.pdf")` lays out a Word document in pages as Word does, like `convert -o x.pdf`. With `content`, it lays out the edited document.
- `kakemono.read_formula(image)` reads a picture of a formula (PNG or JPEG, black on white, as LaTeX renders it) and returns its MathML, like `kakemono formula`. It returns `None` when the formula cannot be read with confidence.

##### `Document`: analyse once, write every format

Each module function reads and analyses its input again. Translating a paper this way (`to_json`, then six outputs with `content`) analyses it seven times. A `Document` is analysed once, and its methods (`convert`, `convert_all`, `to_docx`, `to_markdown`, `to_json`, `to_latex`, `to_epub`, `to_chunks`, `to_pdf`, `analyze`) all reuse that analysis. They take the same options as the functions, except the options of the analysis (`tables`, `jobs`, `png_compression`, `content`), which are given when the document is opened:

```python
doc = kakemono.Document("paper.pdf")
content = doc.to_json("paper.json")
### ... translate content ...
vi = kakemono.Document("paper.pdf", content=content)
vi.convert("hard-95"); vi.to_docx("paper.vi.docx"); vi.to_epub("paper.vi.epub")
```

Six outputs (friendly, hard-95, Word, Markdown, LaTeX, EPUB) with edited content, measured on 4 cores:

| Paper | One function per output | One `Document` |
| --- | --- | --- |
| LIGO (16 pages) | 2.07 s | 1.07 s |
| Attention (15 pages) | 2.48 s | 1.79 s |
| GPT-4 (100 pages) | 6.66 s | 3.37 s |

The module functions are unchanged and give the same output: each one now opens a `Document` and writes one output from it. A `Document` can be used from several threads at once, and the GIL is released while it works.

##### `replace_image` next to the edited JSON

When `content` is a path, a figure's `replace_image` given as a relative path is now read from the folder of that JSON file, as `--content` does on the command line. Before, the picture was not replaced, and a warning said so. When `content` is a dict or JSON text, relative paths are read from the current folder.

## [0.15.3](https://github.com/iamcaominhtien/kakemono/compare/v0.15.2...v0.15.3) - 2026-11-23

### Build and CI

- Speed up the SonarQube coverage job

The SonarQube job took about 16 minutes: 10 for the Rust coverage, 4 for the Python coverage. Two causes, both fixed here.

##### Each real paper is analysed once

Each test of `tests/real_papers.rs` loaded and analysed its paper again: the LIGO paper 12 times, Adam 6, GPT-4 at least 4. Under `cargo llvm-cov` the analysis is about 7 times slower than in `cargo test`, so this one file took 7 minutes of the job (419 s, against 62 s in the CI test job).

The papers are now loaded and analysed once (a `OnceLock` per paper) and shared by every test; different papers are still analysed in parallel, and a test that changes the analysis works on a copy. Measured on 4 cores:

| `tests/real_papers.rs` | Before | After |
| --- | --- | --- |
| with coverage (`cargo llvm-cov`) | 589 s | 85 s |
| without (`cargo test`) | 63 s | 56 s |

The coverage is the same (21 917 of 40 666 lines) and the snapshots are unchanged.

##### The Python binding's build is cached

`python/` is a separate crate with its own target directory, which `rust-cache` did not keep: the binding and all its dependencies were built from scratch on every run (4 min). `python/target` is now cached too; with the dependencies cached the release build takes about 1 minute. (A debug build was tried and is slower: 442 s from scratch and 82 s cached, against 238 s and 56 s, because of its debug info.) The first run after this change fills the new cache, so the gain shows from the second run.

## [0.15.2](https://github.com/iamcaominhtien/kakemono/compare/v0.15.1...v0.15.2) - 2026-11-23

### Fixed

- *(hard-95)* Fit every page before printing ([#28](https://github.com/iamcaominhtien/kakemono/pull/28))

A hard-95 page fits its text when it comes near the screen, so printing or saving as PDF from the browser laid out every page that had never been scrolled to without fitting it. Edited or translated text longer than the original then wrapped onto extra lines (headings above all) and overlapped the text below.

The page now fits every page just before printing (`beforeprint`), so Ctrl+P / "Save as PDF" gives the same pages as the screen. An HTML file written by an earlier version can be fixed the same way by running `kakemonoFitAll()` in the browser console before printing.

#### Release script

`scripts/release.py` now checks what it is given: the PR number must be a positive integer, and `--body-file` must be a file in the repository or in a temporary directory (or `-` for the standard input). SonarCloud counted both as vulnerabilities (3 issues), which failed the quality gate of `main`. The SonarQube job now also runs the tests of the release script, which it measured at 0% coverage.

## [0.15.1](https://github.com/iamcaominhtien/kakemono/compare/v0.15.0...v0.15.1) - 2026-11-22

### Fixed

- Keep a novel's chapters, drop caps and paragraphs whole

Novels, as calibre and Word make them, now keep their chapters and their paragraphs. On the two books of the issues, every chapter is found and the paragraphs are no longer cut into lines.

##### Paragraphs (#26)

- **Paragraphs separated by space.** In a document that sets its paragraphs apart with a blank line (and often ragged right, with no first-line indent), prose set close together is one paragraph: a line that ends early no longer ends it. The document is recognised by the regular spacing of its paragraphs, which the varied gaps of a paper do not have.
- **Across a page break**, an unfinished sentence that goes on in lowercase is one paragraph, even after a short last line.
- **A stray blank line inside a sentence** ("… his class put" / "together, but …") no longer cuts it, when the part before it stops on a bare word; a line of verse ending in a comma, or a short item of a list, stays apart.
- **Shouted lines in capitals** ("YOU WON'T BELIEVE / WHAT IT'S DOING!") stay one paragraph.
- **JSON:** `layout.bbox` of a paragraph that goes on onto the next page is now the box of its lines on its first page, as documented, instead of a union of boxes from two pages.

On *Harry Potter and the Sorcerer's Stone* (250 pages, made in Word), the paragraphs cut mid-sentence drop from 374 to 6, all of them items of a list or lines of a song; the paragraphs cut line by line on a page from 399 to 1.

##### Chapter headings (#25)

- **A chapter headed by its number alone** ("2" between "1 ROGER" and "3 ANDOVER") is a level-1 heading when it is styled like the numbered chapters and in sequence with them. A number out of sequence (a page number, a label) stays text.
- **Chapter labels in words** ("CHAPTER TWO", "Chapter Twenty-One", "PART THREE") are level-1 headings, with the short line in capitals under them as their title. On the first page, such a label is no longer taken for the authors' names.
- **A prologue headed like the chapters without a number**, marked by the chapter ornament under it, is a heading.
- **The ornament under a chapter heading** ("◄○►", "* * *", "❦") joins the heading instead of being a paragraph.

##### Drop caps (#25)

- A drop cap set beside a single line, as calibre sets them, opens its paragraph: the rest of the line no longer comes out as a superscript (`<b>T</b><sup>rong…</sup>`), the paragraph is no longer cut where the lines beside the cap end, and a line of dialogue opening with a drop cap is no longer a heading.
- No stray space after a drop cap ("T he letter").

On *Những vụ án mạng ABC* (126 pages, calibre), all 35 chapters (8 of them headed by their number alone) and the prologue are level-1 headings, against 27 before; on *Harry Potter*, all 17 chapters, against none, and the book has no authors any more.

Fixes #25
Fixes #26

## [0.15.0](https://github.com/iamcaominhtien/kakemono/compare/v0.14.0...v0.15.0) - 2026-11-22

### Added

- Replace a figure's picture and edit the text inside figures with --content

Lets a tool that edits kakemono's JSON (utsushi, for translation) change what a figure shows, not only the text around it:

- **`replace_image`** on a figure block in `--content`: a PNG, JPEG or SVG replaces the figure's picture in every output, fitted into its box; hard-95 draws it over the figure's box without its old text layer.
- **`figure_lines`**: the runs of text inside a figure (labels, rotated runs), editable in `--content`; the figure is redrawn with the new text in place.
- **`kakemono render-svg`**: rasterizes an SVG as kakemono does.
- Stored SVG pictures are drawn as vectors where the format allows.
- Fix: a Word document converted to PDF ignored `--content`.

## [0.14.0](https://github.com/iamcaominhtien/kakemono/compare/v0.13.2...v0.14.0) - 2026-11-22

### Added

- [**breaking**] Added notes in the editable JSON and every output ([#23](https://github.com/iamcaominhtien/kakemono/pull/23))

A `<note>…</note>` tag in the text of the editable JSON is a note added to the text when it is edited: a translator's note, an editor's or a reviewer's, for example. `--content` carries these added notes into every output that shows edited text, numbered in reading order:

- **friendly and medium**: a raised number links to the note. The notes are listed in a small aside after their block (after the next block, for a heading), each with a link back.
- **hard-95**: the raised number opens the note in a popup, and hovering shows it as a tooltip.
- **Word**: real footnotes, in `word/footnotes.xml`.
- **LaTeX**: `\footnote`, or `\footnotemark` with `\footnotetext` inside tables.
- **Markdown**: `[^n1]` footnotes after the block.
- **EPUB**: noteref links, and EPUB 3 popup footnotes at the end of each chapter, numbered again in each chapter.

What a note may contain:

- It may hold style tags and links, but no formula and no other note. An unclosed `<note>`, a formula or a note inside a note is read with warnings.
- Its text is printed exactly as given.

The tag is unrelated to a block of type `note`. The JSON written back gives the `<note>` tag again. `plain` and the chunks leave the notes out. Outputs without added notes are unchanged: the style and script for notes are added only to pages that have notes.

**Breaking change:** `model::Span` has a new public field, `note: Option<Vec<Span>>`, which holds the content of an added note; the span itself is the note's call. Code that builds a `Span` with a struct literal must set it.

## [0.13.2](https://github.com/iamcaominhtien/kakemono/compare/v0.13.1...v0.13.2) - 2026-11-22

### Fixed

- *(book)* No page under the cover while the book opens or closes ([#21](https://github.com/iamcaominhtien/kakemono/pull/21))

Fixes #20.

In the book view with two pages per spread, turning the cover showed the destination's left page at once, while the cover was still lying on the right. Now the left side shows only the desk until the leaf lands, both when the book opens and when it closes.

- `begin()` changes in two ways when a turn starts from the closed cover:
  - it hides the destination's `.verso`;
  - it clips `.bk-window` to the right of the spine, measured from the `.recto` box so the right page's first letters are not cut.
- `end()` clears the clip.

##### Releases: the merged commit of a PR is the release

Until now, the Release workflow committed `chore: release vX.Y.Z` to `main` after each merge and tagged that commit, so a GitHub release pointed at a bot commit, not at the merged PR. Now each PR carries its own version (`Cargo.toml`, `Cargo.lock`) and its own `CHANGELOG.md` entry.

- **`scripts/release.py bump --pr <N>`** writes both:
  - the version comes from the last tag and the PR title: `feat` gives a minor release, a breaking change a major one (minor while 0.x), and anything else a patch;
  - the entry is the title followed by the description, cleaned as `release-plz.toml` cleaned it.
- **`.github/workflows/pr.yml`** (new) checks the title, the version and the entry.
- **On merge,** the Release workflow tags the pushed commit and creates the release from that entry. It no longer commits to `main`.
- `release-plz.toml` is removed.

## [0.13.1](https://github.com/iamcaominhtien/kakemono/compare/v0.13.0...v0.13.1) - 2026-11-20

### Added

- Feat!(markdown): adapt tables to pipe tables in friendly mode

### Fixed

- *(markdown)* Write a header row the analysis could not cut once

### Changed

- *(markdown)* Split the flattening of a table into header and rows
## [0.13.0](https://github.com/iamcaominhtien/kakemono/compare/v0.12.0...v0.13.0) - 2026-11-20

### Added

- Chunks output for retrieval (RAG), with a Qasper evaluation ([#17](https://github.com/iamcaominhtien/kakemono/pull/17))

kakemono now writes the text of a paper as chunks ready to embed for retrieval (RAG): `kakemono convert paper.pdf -o paper.jsonl` (or `--format chunks`), one JSON object per line.

## [0.12.0](https://github.com/iamcaominhtien/kakemono/compare/v0.11.0...v0.12.0) - 2026-11-20

### Added

- [**breaking**] Read Word documents (.docx, .doc) as input ([#16](https://github.com/iamcaominhtien/kakemono/pull/16))

kakemono reads Word documents (.docx and .doc) and writes every output from them: friendly HTML, Word, Markdown, LaTeX, EPUB and JSON from their structure (src/word/), and a PDF and the hard, hard-95 and medium modes from a page layout made as Word makes it (src/word/fmt/, src/layout/), in Rust only.

- .doc read on cfb (FIB, piece table, sprms, styles, lists, notes, fields, tables, pictures, OfficeArt); Office Math and MathType (MTEF 3 and 5) as formulas; EMF and WMF drawn.
- On 524 public Word documents with Word's own PDFs: 95.9% of lines broken at the same words and 89.1% of page counts as Word's (scripts/word-fidelity.py, checked in CI with minimums).
- Against LibreOffice 24.2, Word to PDF on the same 524 documents: 209 s against 790 s, median 35 MB against 192 MB, no crash against 5 (scripts/word-benchmark.py).
- No panic or timeout on 4,317 public Word files.

## [0.11.0](https://github.com/iamcaominhtien/kakemono/compare/v0.10.0...v0.11.0) - 2026-11-19

### Added

- Editable JSON output, --content, bibliographic metadata and analysis fixes ([#13](https://github.com/iamcaominhtien/kakemono/pull/13))

A new output format, editable JSON (`-f json` / `.json`), that holds the content of a document independently of its source format. `--content FILE.json` applies an edited copy back, so every output (hard-95, medium, friendly, docx, md, tex, epub) shows the edited text, for example a translation. The PR also adds read-only fields for search and data extraction, and fixes several analysis bugs found while testing on varied papers and books.

##### Editable JSON (`src/render/json/`)
- File header: `format: "kakemono-content"`, `version` 1, and `source` (format, name, sha256). Each block has its id, type, section/parent, and its `layout` (page and bbox) in a separate field, so formats without pages can leave it out.
- `text` is a single string with HTML-like tags (`<b> <i> <code> <sup> <sub> <sf> <sc> <mf> <a href> <font color>`). `<m id="n">LaTeX</m>` stands in for an inline formula; on apply, the formula's original spans are put back in its place.
- `apply`: only text that differs from what would be exported is applied, so an unedited file changes nothing. The format, version, sha256 and block types are checked. Edited blocks lose their per-line text (`hlines`, `line_texts`, `inner_spans`).
- hard-95: an edited block reflows inside its box, with the font shrunk down to 62% at most. Paragraphs that run over several columns or pages keep running over them.
- `--content` works with every output except `-m hard`, which has no text to replace.

##### Read-only fields
- `plain` on every block and table cell: the text without tags, formulas as `$LaTeX$`. Editing `plain` alone gives a warning.
- `citation` on each reference: authors (given names first), title, venue, year, volume, pages, DOI, arXiv id, URL. Covered styles: IEEE, ACL/APA, astronomy, physics, books, glued initials.
- `meta` fields: authors (with affiliations, e-mail addresses and notes), arXiv category, date, DOI, keywords, abstract.
  - Names printed side by side are separated by the gaps between glyphs.
  - Braced e-mails (`{a, b}@x`) are expanded.
  - Numbered affiliation marks are resolved.
  - Loose e-mails are matched to authors by name.

##### Other changes
- Python: `to_json()`, plus a `content=` option on every converter.
- LaTeX: Vietnamese letters print with pdfLaTeX (T5 encoding for those letters only, T1 stays the main encoding).
- Equations: a matrix cut into columns is joined back into one formula.

##### Analysis fixes
- An old arXiv id kept its version apart from the date (`gr-qc/9712019v1` used to be read as `v13`).
- A bibliography without a heading (PRL style): a run of ≥5 paragraphs labelled `[1]`, `[2]`, … in order is references. LIGO now has 118 references.
- Small marked text is a footnote area only at the foot of the page, or when no text follows it. The affiliations, abstract and keywords under an A&A title are no longer footnotes.
- Headings:
  - a bold heading with an italic word still counts as bold;
  - `Appendix A.1:` set in italics is a heading;
- italic section numbers of several parts (`2.1. 2-point …`) are not list items;
  - `4.1.1. …` set like body text is a heading when `4.1` is one.
  - All 31 headings of astro-ph 1905.06454 are now found.
- References with a hanging indent and no labels are cut into one block per entry.

## [0.10.0](https://github.com/iamcaominhtien/kakemono/compare/v0.9.0...v0.10.0) - 2026-11-18

### Added

- Twice as fast and half the memory on long documents, HTML written as it is drawn ([#12](https://github.com/iamcaominhtien/kakemono/pull/12))

Long documents now convert about twice as fast, with about half the memory, in every mode. Every output stays byte for byte the same. A new option, `--png-compression`, trades file size for drawing time.

| PDF, mode (4 threads) | Before | After |
| --- | --- | --- |
| Dive into Deep Learning (1185 pages), `friendly` | 10.6 s / 1.20 GB | 6.3 s / 580 MB |
| The same book twice (2370 pages), `friendly` | 25.3 s / 2.32 GB | 13.1 s / 1.1 GB |
| Dive into Deep Learning, `hard` | 13.6 s / 1.73 GB | 8.6 s / 640 MB |
| Dive into Deep Learning, `friendly`, `-j 1` | 18.8 s / 919 MB | 16.0 s / 506 MB |
| GPT-4 report (100 pages), `friendly` | 1.33 s / 245 MB | 0.93 s / 208 MB |

The full benchmark against v0.9.0 is in `docs/IMPROVEMENTS.md` §10. It covers every format, the thread sweep, several files at once, and a 1-core machine with 512 MB. Across all 80 conversions (10 PDFs × 8 formats), the time goes from 400 s to 227 s. On that small machine, a 1185-page book in `friendly` now fits where v0.9.0 was killed.

##### Faster analysis
- **Time now grows linearly with the page count.** Fitting equation pictures (`analysis/crops.rs`) went through every line of the document for each page. The blocks of each page are now indexed once: 3.3 s → 0.08 s on the 1185-page book.
- **Lines are built on the `-j` threads.** Before, they were built on one thread: 3.1 s → 0.9 s on four threads.
- **The lines of a page are dropped once it is laid out.** The blocks hold their own copies.

##### Less memory
- **The command line uses mimalloc** (version 2, freed memory purged at once). With glibc, memory freed by another thread stayed reserved. mimalloc gives it back and is also faster. Small papers take about the same memory as before. The library does not set an allocator.
- **The HTML is written as its pictures are drawn.** Before, every picture was drawn into memory, then the whole page was built as one more string: `hard` held 348 MB of page pictures and a 472 MB page at once. Now the workers draw at most 4 pictures per thread ahead of the HTML (`parallel::map_ordered`), and the page goes straight to the file.
- **New API: `Paper::write_html`** writes the HTML to any `io::Write` and gives each picture file to a callback. It produces the same bytes as `Paper::to_html`.

##### PNG compression level
- **New option:** `--png-compression fast|balanced|high` on the command line, `Paper::with_png_compression` with the new `PngCompression` type in the library, and `png_compression` in every Python conversion function.
- **The default stays `balanced`**, and its output does not change.
- **`fast`** is for converting many documents on a slow machine. On the 1185-page textbook, on one thread, it takes 14.8 s instead of 18.3 s, for an HTML file of 70.7 MB instead of 55.3 MB.
- **`high`** makes that file 49.9 MB, for twice the time.
- **The pixels are the same at every level.** A few more photographs become JPEG with `fast`, because each JPEG is compared with a larger PNG.

The remaining leads are in `docs/IMPROVEMENTS.md` §10:
- drawing the pictures is now all of the rendering time;
- EPUB and Word are still built whole in memory (EPUB takes the most memory now);
- glyphs kept in the `Document` are now most of the memory;
- a memory limit for small machines.

## [0.9.0](https://github.com/iamcaominhtien/kakemono/compare/v0.8.0...v0.9.0) - 2026-11-17

### Added

- EPUB output, inline fractions, books, and a benchmark ([#11](https://github.com/iamcaominhtien/kakemono/pull/11))

##### EPUB output (`-f epub`, `.epub`)
- New EPUB 3 output for e-readers, made from the same analysis as the other formats (renderer in `src/render/epub/`). The book has:
  - XHTML chapters and a table of contents (`nav.xhtml` + NCX);
  - a cover;
  - formulas as MathML.
- Formulas wider than `max_math_width_em` (20 em) are shown as pictures, so readers no longer cut them off on the right.
- Links stay valid in EPUB readers: web addresses are percent-encoded, and broken ones become plain text.
- Available from the CLI, from Rust (`Paper::to_epub`) and from Python (`kakemono.to_epub`).
- Every test paper and book passes `epubcheck`.

##### Inline fractions (every format)
- Fractions stacked on a bar inside a line, such as `β₂/√β₁` or `1/√d_k`, are now read whole. This includes terms with scripts and fractions set in a superscript.

##### Books and novels
- Drop caps are joined back to their word.
- Short running feet and page numbers at the bottom of the page are removed.
- Lines of ragged-right prose no longer split paragraphs.
- Chapter titles become level-1 headings.
- Results:
  - Pride and Prejudice: broken paragraphs go from 335 to 2;
  - The Great Gatsby: from 107 to 1;
  - the 8 test papers: unchanged.

##### Benchmark
- `scripts/benchmark.py` measures time, memory and CPU with GNU time, in three experiments:
  - every format on every file;
  - a thread sweep (`-j`);
  - N conversions at once.
- On 4 cores:
  - a 1151-page book: 10 s and 1.2 GB;
  - a 2302-page book: 24 s and 2.4 GB;
  - about 300 papers a minute, with one process per core and `-j 1`.
- Results are in the README ("Performance") and, in full, in `docs/IMPROVEMENTS.md` §10, with leads for later work.

##### Release notes
- The changelog and the GitHub release of a merged PR now show the PR description, not just its title.
- The parts written only for reviewers are left out (Summary heading, testing sections, footers), and lines GitHub wrapped are joined again.
- Past entries of `CHANGELOG.md` were filled in the same way. CONTRIBUTING.md explains how to write a description that reads well as a release note.

## [0.8.0](https://github.com/iamcaominhtien/kakemono/compare/v0.7.0...v0.8.0) - 2026-11-17

### Added

- LaTeX output ([#10](https://github.com/iamcaominhtien/kakemono/pull/10))

  #### LaTeX output

  New `tex` format (`-o x.tex` or `-f tex`, alias `latex`): the analysed document as a LaTeX source that **compiles as it is with pdfLaTeX, XeLaTeX or LuaLaTeX**, using only packages every TeX distribution has (`amsmath`, `amssymb`, `mathtools`, `graphicx`, `booktabs`, `multirow`, `hyperref`).

  - Title, authors (cards side by side, like `\and`) and the `abstract` environment as a title page; headings as `\section*`, `\subsection*`, … (their numbers are part of their text); lists keep the markers of the paper (`\item[(a)]`).
  - Rebuilt tables as `tabular`s with `booktabs` rules, `\multicolumn` and `\multirow`, shrunk to the text width only when wider.
  - Formulas with a known structure as LaTeX math (`equation*` with `\tag{n}`), the others as pictures; formulas inside the text as `$…$`; symbols of running text (Greek, operators, math alphabets) in math mode.
  - Figures as pictures (`\includegraphics`), always written to an image folder (`<name>_assets/`, or `--image-dir`): LaTeX cannot embed them, so `--images inline` is refused. Figures and tables stay in place (no floats) and keep their caption labels.
  - Citations and cross-references as `\hyperref`, web addresses as `\href` (breakable after `/`).
  - Characters pdfLaTeX has no glyph for are declared in the preamble and print as **?** there; XeLaTeX/LuaLaTeX print them.

  Library: `Paper::to_latex`, `LatexOptions`, `LatexOutput` (additive, no breaking change). Python: `kakemono.to_latex(source, path, ...)`.

  #### Refactoring

  The `MathNode` → LaTeX writer moves from `render/markdown/latex.rs` to `render/latex/math.rs` and is shared by Markdown and LaTeX. **The Markdown output is unchanged.**

## [0.7.0](https://github.com/iamcaominhtien/kakemono/compare/v0.6.0...v0.7.0) - 2026-11-16

### Added

- Markdown output and image folders for Word documents ([#9](https://github.com/iamcaominhtien/kakemono/pull/9))

  #### Markdown output and image folders

  - New `md` format (`-o x.md` or `-f md`): headings, lists, footnotes, references and fenced code listings; pipe tables (HTML tables when cells span); formulas whose structure is clear as LaTeX between `$$` with `\tag{n}`, the others pictures. Pictures are written to an image folder by default, or embedded with `--images inline`.
  - `--images files` now also works for Word documents: the pictures stay embedded and are also written to a folder.
  - New `--image-dir <dir>` names that folder (default `<name>_assets`); with `--all`, one subfolder per mode.
  - Library: `Paper::to_markdown`, `MarkdownOptions`, `MarkdownOutput`, `Paper::to_docx_with_images`, `DocxOutput`. Python: `to_markdown`, and `save_images`/`image_dir` for `to_docx`.

  #### Formulas inside running text

  - Formulas inside sentences are written as math in every reflowing output (MathML in HTML, OMML in Word, `$…$` in Markdown), from the same analysis.
  - A formula inside the text that is wider than the column scrolls in its own box instead of widening the page on a phone.

  #### Books (tested on *Mathematics for Machine Learning* and a deep-learning maths book)

  - Wide margins: margin notes become notes, running feet repeated at the same place are removed, running heads crowded by a figure or a margin caption are removed.
  - Matrices and braces drawn in pieces, `⋮`/`⋱` made of periods, and the parts of one displayed formula split across blocks are rebuilt.
  - Accents TeX raises over capital letters (`\tilde B`) are joined to them.
  - Tables of contents: the page-number column is no longer dropped as review line numbers, entries nest by section number, and a page number close to a long title is read.
  - Captions set in the margin find their figures and tables (pairing, panels side by side or stacked, tables without rules, diagrams of labels and arrows); the shading of worked examples is not taken for a figure. Figures without a picture in the MML book: 140 → 0.

## [0.6.0](https://github.com/iamcaominhtien/kakemono/compare/v0.5.0...v0.6.0) - 2026-11-15

### Added

- Book contents, PDF links, side-by-side pictures and condensed fonts ([#8](https://github.com/iamcaominhtien/kakemono/pull/8))

  Fixes found by converting an O'Reilly book (*AI Engineering*, 535 pages) and checked against 14 other books and the 8 test papers.

  **Tables of contents** (new `contents_entry` block)
  - Lines that end with a page number set apart from their title (leader dots or a wide gap), with the numbers in order, are entries of a table of contents.
  - Each entry is one block with its `level` and printed `page`, and links to the heading it names.
  - Entry levels come from the indents. Pages are lined up on the page with the most entries, because left and right pages have different margins. An indent shared by only a few entries, such as a hanging "10.", joins the nearest level.
  - A page number cut off as a line of its own belongs to its title. An entry title that wrapped into a block of its own is joined to the rest of the entry.
  - `medium` and `friendly` show the contents as an indented list with dotted leaders.
  - Before this change, chapter entries ("1. Introduction … 1") were read as numbered level-1 headings. Their small size then pulled every section heading up to level 1, and the other entries ran together into paragraphs.

  **Heading levels**
  - When the table of contents lists enough headings, it gives their levels.
  - A heading it leaves out takes the level of listed headings of its size, or goes one level below the deepest listed level.
  - AI Engineering: chapters are H1, sections H2, subsections H3, and the smaller run-in headings H4. Before, almost all of them were H1.
  - Chapter titles are larger in `medium` and `friendly`.

  **Running heads**
  - A page number next to a separator ("Preface | xv", "96 | Chapter 2: …") is recognised.
  - This removes 25 running heads that leaked into AI Engineering, some of them cutting paragraphs in two. The same pattern is fixed in AI & ML for Coders and other O'Reilly-style books.

  **PDF links**
  - Link annotations that open a URL now become `href` on the spans they cover. Only http(s), mailto and ftp addresses are kept.
  - AI Engineering has 1143 link annotations; 980 links are kept, against 12 found before from URLs written in the text. Most of the difference is links whose annotation is split over two lines.
  - On LIGO, the reference DOIs and arXiv links are now linked.

  **Text**
  - A space character drawn by the PDF is a word space however narrow the gap: "Vittorio Cretella", "Praise for AI Engineering", "2. Register at the front desk".
  - A book cover is shown whole instead of being cut just under its title.

  **`hard-95`**
  - A line set in a font much narrower than any fallback (condensed sans, UbuntuMono) is squeezed sideways onto its width. On AI Engineering, lines that do not fit their width go from 531 to 12 of 15,269.
  - A table whose cells overflow their columns gets a smaller type, the same in every cell.
  - Tables no longer cover the page picture with white, so filled header rows (white on black) show.

  **`medium` / `friendly`**
  - A small uncaptioned picture with the text block after it standing beside it, such as the icon of a note, now floats to its side as in the PDF. Before, it was always set above the text.
  - Only link texts that look like addresses break anywhere. Links on words no longer break inside words.

  **Docs**
  - README lists `contents_entry` and the link `href`s.
  - CLAUDE.md: test the book view only when asked.

  #### Note

  The previous branch was squash-merged. Commit `08a3475` only records its history as an ancestor (no file changes) so that the branch could move forward without a force push. Squash-merge this PR to keep `main` clean.

## [0.5.0](https://github.com/iamcaominhtien/kakemono/compare/v0.4.0...v0.5.0) - 2026-11-15

### Added

- Convert papers and books to Word documents (.docx) ([#7](https://github.com/iamcaominhtien/kakemono/pull/7))

  Adds a Word output (`.docx`) built from the same `Document` as the four HTML modes, and fixes the outline of books, whose table of contents was read as headings.

  ##### Word output (`src/render/docx/`)

  - **Styles:** title, authors, abstract, `Heading 1`–`4` (Word's navigation pane and table of contents work), lists, footnotes, code (Consolas) and references.
  - **Tables:** rebuilt tables become Word tables, with merged cells (`gridSpan`, `vMerge`), column alignment and the paper's thin and heavy rules.
  - **Formulas:** formulas whose structure is known become Office Math (OMML) equations that can be edited in Word, with fractions, roots, scripts, large operators, accents and matrices. The others are pictures. A numbered formula keeps its number at the right margin.
  - **Figures:** pictures of their area of the page, drawn on several threads, with their caption on the side it had in the paper (label in bold).
  - **Links:** citations and references to figures, tables and sections are links to bookmarks; web links are external hyperlinks.
  - **Page:** US Letter, one-inch margins.
  - **No new crates:** `flate2` and `crc32fast` were already in the dependency tree. A small ZIP writer packs the parts with fixed timestamps, so the same PDF always gives the same file.

  ##### Interfaces

  - **Library:** `Paper::to_docx(&analysis, &DocxOptions)`, with `DocxOptions` re-exported from the crate root. Additive only, no breaking change.
  - **CLI:** `--format docx` (`-f`), chosen automatically when `-o` ends in `.docx`; `--all` with docx is rejected.
  - **Python:** `kakemono.to_docx(source, path=None, *, tables, equations, scale, jobs)`.

  ##### Fix: table-of-contents entries were headings

  The chapter lines of a book's table of contents ("1 The way of the program", "Preface v") are bold, short and numbered, so they were taken for headings and filled the outline of every output: the contents column of the friendly mode, and now Word's navigation pane.

  After a "Contents" heading, a run of blocks that end in leader dots or a page number, plus the headings followed by such a block, is now plain text. The run stops at the first other block, or at a chapter label set alone ("Chapter 1"). On Think Python 2e and Think Stats 2e, only the contents pages change (22 and 16 false headings removed). The 8 test papers have no table of contents and are unchanged.

  #### Limitations

  - One flowing column: the page and column layout of the PDF is not kept.
  - Figures are pictures, not editable drawings.
  - Footnotes stay in the text flow, as in the HTML, not as Word footnotes.
  - The operand of `∑`/`∫` follows the operator rather than sitting inside its `m:nary` element. It looks right, but editing it in Word differs slightly.

## [0.4.0](https://github.com/iamcaominhtien/kakemono/compare/v0.3.0...v0.4.0) - 2026-11-15

### Added

- [**breaking**] Rename the project to kakemono and publish it on crates.io and PyPI ([#5](https://github.com/iamcaominhtien/kakemono/pull/5))

  The project outgrows its old name: `paper2html` ties it to papers and to HTML, while it should later convert to (and from) more formats. It is now **kakemono**, after the Japanese hanging scroll that is unrolled to be read, as the tool unrolls a fixed PDF page into flowing text. It is also published as a Rust crate and a Python package.

  ##### Rename (breaking)

  - Crate, library and executable: `paper2html` → `kakemono` (`kakemono convert paper.pdf`).
  - Download cache: `~/.cache/paper2html` → `~/.cache/kakemono`; `PAPER2HTML_CACHE` → `KAKEMONO_CACHE`. The old cache directory is no longer used.
  - Test variables: `P2H_UPDATE_SNAPSHOTS` / `P2H_PRINT_METRICS` → `KAKEMONO_*`.
  - Release workflow, CI, Sonar project name, ARCHITECTURE.md, CLAUDE.md and the browser checks follow.
  - The generated HTML names the new tool (`<meta name="generator">`, the friendly-mode footer), so every snapshot is updated.
  - Unchanged on purpose: the `p2h-*` localStorage keys and `window.p2h*` names in the renderer JS (internal; renaming them would reset readers' saved settings), the Sonar project key (needs the SonarCloud project renamed first), and old links in `CHANGELOG.md` (GitHub redirects them).

  ##### crates.io

  - A new `crate` job in the Release workflow runs `cargo publish
  --locked` for each new tag (secret `CARGO_REGISTRY_TOKEN`). release-plz keeps tagging and releasing, with `publish = false`.
  - `include` in `Cargo.toml`: the package holds only `src/`, `Cargo.toml`, `Cargo.lock`, `README.md`, `LICENSE` and `CHANGELOG.md`; tests, scripts, docs and the Python binding stay in the repository. `cargo package` builds it standalone.
  - Public API: what `lib.rs` re-exports (`Paper`, `convert`, `Analysis`, `PageRenderer`, the options), plus `config`, `model` and `source`. The pipeline stages (`analysis`, `backend`, `common`, `paper`, `raster`, `render`) stay `pub` for the CLI, examples and tests but are `#[doc(hidden)]` internals. `config` re-exports the config struct of each stage, so `LayoutConfig` is fully documented on docs.rs.

  ##### Python package (`python/`)

  - A separate crate (not a workspace member, so the Rust build never needs Python) that binds the library with PyO3, abi3 for Python 3.9+, built with maturin.
  - `kakemono.convert(source, mode="friendly", ...)`, `convert_all(source, modes)` (one analysis for several modes) and `analyze(source)` (the `--json` document as a dict). `source` is bytes, a path, or an arXiv id/URL. The GIL is released during a conversion. Typed (`py.typed`, `.pyi`).
  - New workflow `python.yml`:
  - on every pull request: `cargo fmt`/`clippy` of the binding, `pytest`, wheels for Linux x86_64/aarch64, macOS x86_64/arm64 and Windows x64 (each native wheel is imported after the build), and the sdist;
  - after each Release run (`workflow_run`, since tags pushed by the release do not start workflows): builds the newest tag with its version (`scripts/set-python-version.py`) and publishes it to PyPI through a trusted publisher (environment `pypi`) if PyPI does not have that version yet.

  ##### README

  - Centered header: new scroll logo (`docs/assets/logo.svg`), name, tagline, badges (release, CI, crates.io, PyPI, docs.rs, hayro, platforms, MIT) and a quick navigation row.
  - Feature table; install with `cargo install kakemono`, prebuilt binaries, `cargo add` / `pip install`; a Python section; Python tests under Testing.

  #### Before the first release

  - crates.io: `CARGO_REGISTRY_TOKEN` is set.
  - PyPI: a pending trusted publisher for project `kakemono`, owner `iamcaominhtien`, repository `kakemono`, workflow `python.yml`, environment `pypi`, and a `pypi` environment in the repository settings.


### Build and CI

- Release a version set by hand in Cargo.toml as it is (the first release after the rename)
## [0.3.0](https://github.com/iamcaominhtien/rust-pdf-parser/compare/v0.2.1...v0.3.0) - 2026-11-15

### Added

- Convert books faithfully in all four modes ([#4](https://github.com/iamcaominhtien/rust-pdf-parser/pull/4))

  Extends paper2html from arXiv papers to whole books (textbooks, novels, a Japanese course). Every change was checked against 14 books and the 8 test papers before it was committed.

  **`hard` / `hard-95`**
  - Book pages render faithfully. Very large pages are drawn as PNG.
  - TeX papers are set in Latin Modern, with inline math placed exactly.
  - Small capitals show as small capitals, and tabs are kept.

  **Text extraction**
  - Glyphs that PDFs leave without Unicode are read from their font's known encoding: Linux Libertine ligatures and old-style figures, `lasy`, `cmsy`/`cmex`, `txsy`, MathDesign, OpenSymbol bullets and Adobe expert sets.
  - Blank unmapped glyphs are dropped.
  - Underscores and fill-in blanks drawn as rules become `_`.
  - Inline fractions are read as `1/2`, `(n−1)/n`. This covers both terms stacked on a bar and terms left in rows of their own.

  **Structure**
  - Removed from the text:
    - running heads that carry a page number (arabic or roman),
    - printer's marks,
    - furigana.
  - Kept as structure rather than prose:
  - code listings, line by line with their indentation (new `code` block),
    - a list item's further paragraphs (new `list_continuation` block),
    - "Chapter N" joined with the chapter title,
    - novel scene breaks, centred.
  - Text boxes (shaded, framed, or with a side bar) stay text instead of becoming pictures. A box with unreadable characters stays a picture.
  - Axis tick labels at the ends of an axis join their figure.
  - Displays set in text letters (`P(A) = P(A and B) + …`) are kept together as one equation.
  - A language course's example sentences and dialogue are text, not headings.

  **`friendly` book view**
  - Covers are shown as full pages.
  - A long book is laid out once per step: OpenStax goes from 334 s to 30 s.
  - Column drift is fixed.
  - A code listing left stranded runs on to the next page.
  - A heading stranded by a chapter outline moves on to the next page.

  **Images**
  - Photographs are cut out as JPEG. Vector figures stay SVG; JPEG is used only for raster crops with at least 4096 colours, whose PNG is at least 64 KB, and only when the JPEG is less than half the PNG's size.

  **Docs**
  - README cut from 415 to 150 lines; the pipeline details stay in ARCHITECTURE.md.

  #### Baseline: `main` vs this branch

  The table shows `inspect` counts and the `friendly` output of each document. When a cell shows a single number, the value did not change.

  | Document | `�` | Headings | Code blocks | Paragraphs | MathML formulas | friendly MB | friendly s |
  | --- | --- | --- | --- | --- | --- | --- | --- |
  | AI &amp; ML for Coders | 0 | 401 → 378 | 0 → 512 | 4412 → 2581 | 23 | 33.4 → 27.9 | 2.1 → 3.1 |
  | Irodori (Japanese) | 0 | 1310 → 921 | 0 | 4284 → 3212 | 0 | 168.7 → 115.2 | 11.8 → 13.2 |
  | Foundations of Data Science | 0 | 541 → 463 | 0 → 3 | 3453 → 3433 | 1051 → 1090 | 11.1 → 10.7 | 2.4 → 2.5 |
  | Grinstead, Probability | 369 → 0 | 539 → 251 | 0 → 3 | 4357 → 4040 | 1340 → 1437 | 11.4 → 10.6 | 2.5 → 2.6 |
  | Erickson, Algorithms | 456 → 29 | 235 → 233 | 0 → 20 | 2859 → 2801 | 91 → 139 | 29.2 → 26.8 | 4.6 → 4.8 |
  | Little Brother | 0 | 36 | 0 → 6 | 3167 → 3021 | 0 | 1.3 | 1.1 → 1.3 |
  | OpenIntro Statistics | 0 | 479 → 629 | 0 → 8 | 3155 → 3125 | 224 → 275 | 59.8 → 42.4 | 4.2 |
  | ProbML Intro | 4 → 2 | 1245 → 1159 | 0 → 12 | 8882 → 8542 | 2157 → 2206 | 65.0 → 54.4 | 9.5 → 10.2 |
  | The Public Domain | 0 | 85 → 77 | 0 | 1933 → 1558 | 0 | 3.4 → 2.7 | 1.6 → 1.8 |
  | SICP | 6 → 4 | 319 → 318 | 0 → 1284 | 7961 → 3156 | 86 → 87 | 9.3 → 8.6 | 2.7 → 2.9 |
  | Think Python | 8 → 0 | 412 → 303 | 0 → 531 | 4928 → 3114 | 31 → 32 | 2.5 | 0.9 |
  | Think Stats | 0 | 431 → 191 | 0 → 285 | 2725 → 1786 | 56 | 5.1 → 5.0 | 1.1 → 1.2 |
  | OBP (open access) | 0 | 16 | 0 | 596 → 595 | 0 | 8.7 → 5.0 | 0.9 → 1.2 |
  | OpenStax Statistics | 0 | 1226 → 1285 | 0 → 7 | 9661 → 9887 | 8 | 92.4 → 68.4 | 6.2 → 6.5 |
  | 8 test papers | 2 | unchanged | 0 → 6 | 1198 → 1139 | 57 | 38.0 → 27.6 | 3.0 → 3.4 |
  | **Total** | **845 → 37** | | **0 → 2677** | | **5124 → 5410** | **539 → 409** | **54.5 → 60.0** |

  How to read the table:
  - **Paragraphs:** fewer is mostly better. Code lines that were one paragraph each are now listings, and paragraphs that were split by leaked running heads or tick labels are joined again.
  - **Headings:** fewer means running heads, numbered exercises and example sentences are no longer read as headings. OpenIntro and OpenStax have more because the text inside boxes is now extracted, including the headings in those boxes.
  - **Size:** `friendly` output is 24% smaller, mainly from JPEG.
  - **Time:** the total over all 22 documents, one `friendly` conversion each, on 4 cores.

  The test metrics in `tests/real_papers.rs` are unchanged. Snapshots were updated for every intended output change.

  ##### Cost

  Conversion is about 10% slower in total: 54.5 s → 60.0 s for these 22 documents. Before the `perf` commit it was 18% slower (66.5 s). That commit removed these costs without changing any output (`inspect` is identical on all 22 documents):
  - code listings were split by removing and inserting in the middle of the node list, which is quadratic on books with thousands of listings (SICP, AI &amp; ML);
  - every thin rule was compared with every glyph of its page when looking for blanks;
  - every glyph was hashed with SipHash and compared with all same-letter glyphs in its baseline band when looking for drawn-twice copies;
  - colours of photographs were counted with a hash set per pixel.

  What remains:
  - JPEG encoding, about 1.4 s on Irodori and 0.7 s on AI &amp; ML. It makes those books 32% and 16% smaller.
  - The new analysis passes (code listings, fractions, blanks, furigana, boxes), about 5% on the other documents.

  #### Known remaining issues

  - `check-friendly.js` still reports:
    - SICP: 5 pages more than 60% empty.
    - AI &amp; ML for Coders: 3 pages more than 60% empty.
  - Grinstead and SICP: "longer text was not paginated again". The check waits only 0.7 s after the text grows, and books of this size take longer to paginate again. Grinstead already failed this check before these changes.
  - Grinstead's appendix normal table is partly taken as a picture, as it was before; the tick-label change takes a few more columns into the picture.
  - One fraction split across two layout blocks on OpenIntro p105 is still read as two paragraphs.

## [0.2.1](https://github.com/iamcaominhtien/rust-pdf-parser/compare/v0.2.0...v0.2.1) - 2026-11-14

### Other

- Auto-compact context at 70%
- Auto-compact context at 68%
## [0.2.0](https://github.com/iamcaominhtien/rust-pdf-parser/compare/v0.1.0...v0.2.0) - 2026-11-14

### Added

- Display formulas as MathML, and formulas read from pictures ([#3](https://github.com/iamcaominhtien/rust-pdf-parser/pull/3))
## [0.1.0](https://github.com/iamcaominhtien/rust-pdf-parser/releases/tag/v0.1.0) - 2026-11-14

### Build and CI

- Release every merge into main without a release PR

### Other

- Initial commit
