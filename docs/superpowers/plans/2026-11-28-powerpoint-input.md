# PowerPoint input (`.pptx`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `kakemono convert deck.pptx -o x.md` (and every other output, `-o x.pdf`, `hard`, `hard-95` and `medium` included) works: the slides read into `word::ir` and built by `word/build.rs`, and drawn as pages by a new `layout/slides/`.

**Architecture:** A new module `src/powerpoint/` reads the package (with `word::zip` and `roxmltree`), resolves each slide's inheritance (layout, master, theme), and turns its shape tree into `word::ir` items (headings, list paragraphs, tables, pictures, charts, formulas, notes). `Paper` gets a `Source::Slides` variant. `src/layout/slides/` draws the same reading of each slide into a `display::Page`, written by `layout/pdf.rs`; `Paper::laid` sends that PDF through the PDF pipeline for `hard`, `hard-95` and `medium`.

**Tech Stack:** Rust (edition of the crate), `roxmltree`, `crate::word::{zip, docx::parse, omml, pictures, ir, build}`, `crate::excel::{drawing, view}` (DrawingML), `crate::chart`, `crate::layout::{fonts, shape, para, display, pdf}`; Python standard library for the sample generator; LibreOffice only to check the sample opens.

**Spec:** `docs/superpowers/specs/2026-11-28-powerpoint-input-design.md`

## Global Constraints

- No new dependency in `Cargo.toml`.
- `InputFormat::Pptx` (`name() == "pptx"`, `has_pages() == false`): `InputFormat` is `#[non_exhaustive]`, so this is `feat:`, not `feat!:`. No `FORMAT_VERSION` bump.
- Limits (most slides, shapes per slide, group depth, slide side) are documented constants of the module that applies them, as ARCHITECTURE.md says of constants that are not thresholds: `MAX_SLIDES = 5_000`, `MAX_SLIDE_SIDE_PT = 4_032.0` (56 inches, PowerPoint's maximum), and so on. A threshold of a rule, if one is needed, goes in a `PowerPointConfig` re-exported from `src/config.rs`.
- Hidden slides are left out of every output. Placeholder prompt text never reaches any output.
- Conventional Commits, English, imperative; each commit ends with the session's `Co-Authored-By`/`Claude-Session` lines.
- Before each commit: `cargo fmt --check && cargo clippy --all-targets -- -D warnings` and `cargo test --release`. Snapshots of the papers, Word, Excel and EPUB must not change (`git diff --stat tests/data/snapshots` shows only `powerpoint-sample.txt`), unless a task says otherwise.
- No wildcard `use x::*` outside test modules (SonarQube `rust:S2208`).
- After a change to `src/render/latex/` or `src/render/typst/` (none expected), compile as CLAUDE.md says.

## Review Focus

1. A slide placeholder whose layout has no placeholder of that `idx` or type, or a layout without a master → the slide's own properties and the defaults, no panic. Test in Task 2 (`orphan_placeholder_keeps_its_own_text`).
2. A group with `a:chExt cx="0"` (common in exports) → its children are placed with a scale of 1, not divided by zero. Test in Task 4 (`group_with_empty_child_extent_is_not_scaled`).
3. Relationships that point to a missing part, outside the package, or in a cycle (a layout whose master's relationship points back) → skipped, never followed twice. Test in Task 1 (`missing_and_external_parts_are_skipped`).
4. A `.docx` and an `.xlsx` are still read as Word and Excel; a zip that is none of them is still the same error. Test in Task 1 (`docx_and_xlsx_are_not_taken_for_a_deck`).
5. Autofit: a body with `a:normAutofit fontScale="62500" lnSpcReduction="20000"` is drawn at 62.5 % of its size, its line spacing 20 points of percent less, and its text stays inside its frame. Test in Task 7 (`autofit_text_stays_in_its_frame`).

## How the output is checked

Four layers, from cheap and certain to costly and subjective:

1. Unit tests, behaviour tests (`tests/powerpoint.rs`) and the snapshot of the sample, in CI.
2. **Text against an independent reader** (`scripts/powerpoint-text-check.py`, run with python-pptx in `/root/.venvs/kakemono-py`, not in CI): for each deck of the corpus, python-pptx's text of each visible slide is compared with our Markdown of that slide: share of its words found (recall), slide titles that are headings, prompt text leaked ("Click to add"), footer or slide-number text repeated, notes found under their slide. Totals per deck and overall, written as TSV so that two runs can be diffed before and after a change.
3. **Pages against PowerPoint's PDFs** (Task 9): lines broken at the same words, page counts, with minimums in CI.
4. **Review rounds by subagents** (after Task 4 and after Task 7; not in CI): 15–20 decks of the corpus per round.
   - A Sonnet reviewer of the *structure*: each slide's picture (PowerPoint's PDF page, else LibreOffice's, drawn with `pdftoppm` at 1×) next to our Markdown and friendly HTML of it: reading order, headings, list levels, tables, charts' data, notes.
   - A Sonnet reviewer of the *pages* (after Task 7): our PDF page next to PowerPoint's: text out of its frame, wrong fonts or sizes, backgrounds, missing pictures, shapes.
   - Haiku only to sweep many decks quickly and pick those worth a closer look.
   - Each reviewer is given the files, not our conclusions, and reports in a fixed form: deck, slide number, what is wrong, evidence (the output's text, or the area of the page), severity. Every finding is checked by hand before it is fixed; each real one is fixed with a test, so that it does not come back.

---

### Task 0 (from the start, alongside Tasks 1–5): The corpus

- [x] Collect public decks (gov.uk, data.gov.au, open.canada.ca, data.gov.uk, GitHub repositories of talks; English, Japanese, Vietnamese), preferring those published with the PDF PowerPoint made of them (the PDF's `Producer` or `Creator` names Microsoft PowerPoint). Record them in `tests/data/powerpoint-corpus.tsv` (Task 6 adds the fetch script and the crash test). Started early so that the number of decks with a PowerPoint PDF, which Task 9 depends on, is known early.
  - Result (2026-11-28): 113 decks (data.gov.uk 21, open.canada.ca 32, data.gov.au 40, Japanese ministries 18, GitHub 2; English 80, Japanese 18, French 12), only 5 with a PowerPoint PDF (uk-0002, uk-0003, uk-0007, uk-0009, uk-0010: 213 slides, all from the London Datastore and NHS Digital). No Vietnamese deck was found. Task 9 starts with those 5 and adds pairs as they are found.

### Task 1: Detection, the package, and wiring into `Paper`

**Files:**
- Create: `src/powerpoint/mod.rs` (`detect`, `read`, `PowerPointConfig`), `src/powerpoint/package.rs`
- Create: `tests/data/powerpoint/make-sample-pptx.py`, `tests/data/powerpoint/sample.pptx`, `tests/powerpoint.rs`
- Modify: `src/lib.rs` (`#[doc(hidden)] pub mod powerpoint;`), `src/config.rs` (re-export), `src/paper.rs` (`InputFormat::Pptx`, `Source::Slides`, `from_bytes`, `from_bytes_with`, `analyze_with`, `to_pdf` error until Task 7, `word_format`), `src/main.rs` (help texts, extensions) (`Cargo.toml` `include` lists only `/src`, so nothing to add there)

- [ ] **Step 1: The sample generator.** Write `make-sample-pptx.py` with `zipfile` only (as `tests/data/excel/make-chart-xlsx.py`): `[Content_Types].xml`, `_rels/.rels`, `ppt/presentation.xml` with `p:sldSz cx="12192000" cy="6858000"` (16:9), a theme, one master, layouts (Title Slide, Title and Content, Section Header, Blank), `docProps/core.xml` title. Start with three slides (title slide, a bullets slide, a hidden slide); later tasks add slides. Run `python3 -I tests/data/powerpoint/make-sample-pptx.py`, then `soffice --headless --convert-to pdf --outdir $SCRATCH tests/data/powerpoint/sample.pptx` to check it opens.
- [ ] **Step 2: Failing tests** in `tests/powerpoint.rs`: `sample_is_detected_as_pptx` (`Paper::from_bytes(..).format() == InputFormat::Pptx`), `docx_and_xlsx_are_not_taken_for_a_deck`, `hidden_slides_are_left_out`, `missing_and_external_parts_are_skipped` (a deck built in the test with a slide relationship to `../../etc/passwd` and to a missing part).
- [ ] **Step 3: Implement** `detect` (ZIP, `_rels/.rels` office document of type `.../officeDocument` whose target's content type is a presentation main part, or `ppt/presentation.xml`), `package.rs` (slide size, slide list, hidden flag, sections from `p14:sectionLst`, core title), and a `read` that gives each visible slide a heading "Slide N" for now. Wire `Source::Slides`.
- [ ] **Step 4:** `cargo test --release`; no other snapshot changes.
- [ ] **Step 5: Commit** — `feat(powerpoint): read PowerPoint presentations (.pptx) as input`

### Task 2: Inheritance: layouts, masters, text styles, theme

**Files:**
- Create: `src/powerpoint/inherit.rs`
- Modify: `src/chart/theme.rs` (theme fonts `+mj-lt`, `+mn-lt`, `+mj-ea`, ...; the colour map), `src/powerpoint/mod.rs`

**Interfaces:**
- Produces: `struct SlideParts { slide, layout: Option<Part>, master: Option<Part>, theme: Theme }`; `fn placeholder_chain(&SlideParts, &Placeholder) -> Vec<Node>` (slide's shape, then layout's, then master's); `fn level_props(chain, kind: TextKind, level: u8) -> LevelProps` (`TextKind::Title | Body | Other`).

- [ ] **Step 1: Failing unit tests:** `placeholder_by_idx_then_type`, `body_stands_for_obj_and_untyped`, `orphan_placeholder_keeps_its_own_text`, `levels_merge_from_shape_to_default_style`, `colour_map_maps_tx1_to_dk1`, `theme_fonts_resolve`.
- [ ] **Step 2: Implement.** Placeholders: `p:nvPr/p:ph` `type` (default `obj`) and `idx` (default 0); match by `idx`, else by type, else none. `a:xfrm` inherited when absent. Level properties: shape `a:lstStyle/a:lvlNpPr` → layout placeholder → master placeholder → master `p:txStyles` (`titleStyle` for `title`/`ctrTitle`, `bodyStyle` for `body`/`obj`/`subTitle`, `otherStyle` for the rest) → `ppt/presentation.xml` `p:defaultTextStyle`. Colours: `a:schemeClr` through `p:clrMap` (`p:clrMapOvr` of the slide or layout overrides), with `lumMod`/`lumOff`/`tint`/`shade` as `chart::theme::color` already applies them; `alpha` is added there (a half-transparent fill is common on slides), with a test that Excel's snapshots do not change.
- [ ] **Step 3:** `cargo test --release`.
- [ ] **Step 4: Commit** — `feat(powerpoint): resolve placeholders, text styles and theme through layouts and masters`

### Task 3: Text → `word::ir`: titles, lists, runs, reading order

**Files:**
- Create: `src/powerpoint/text.rs`, `src/powerpoint/build.rs`
- Modify: `tests/data/powerpoint/make-sample-pptx.py` (a bullets slide with three levels and a numbered list, a slide without a title, a section header, two sections), `tests/powerpoint.rs`, add `tests/data/snapshots/powerpoint-sample.txt`, `tests/real_papers.rs` or the snapshot helper if it lists the samples

- [ ] **Step 1: Failing tests:** `title_slide_gives_the_document_title`, `slide_titles_are_headings`, `sections_are_headings_above_slide_titles`, `slides_without_title_are_numbered`, `bullet_levels_become_list_levels`, `auto_numbers_keep_their_marker` (`alphaLcParenR` → `a)`, `startAt="3"` → `3.`), `placeholder_prompts_are_not_text`, `footer_date_and_number_are_left_out`, `reading_order_is_the_shape_tree_with_the_title_first`.
- [ ] **Step 2: Implement** `text.rs` (paragraphs, bullets, runs, `a:br`, `a:fld`, links) and `build.rs` (the title slide; per slide: section heading, slide heading, shapes in tree order, title first, groups in their order; furniture placeholders and layout/master shapes left out).
- [ ] **Step 3:** run every output of the sample (`--all`, md, docx, tex, typ, epub, json, jsonl) and read them; compile the `.tex` with `pdflatex` and the `.typ` with `typst compile`. Write the snapshot (`KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test powerpoint`).
- [ ] **Step 4: Commit** — `feat(powerpoint): slides as headings, lists and paragraphs in reading order`

### Task 4: Tables, pictures, charts, SmartArt, formulas, groups

**Files:**
- Create: `src/powerpoint/shapes.rs`
- Modify: `src/powerpoint/build.rs`, the sample generator (a table with `gridSpan`/`rowSpan`, a PNG and an SVG picture, a chart part reused from `make-chart-xlsx.py`'s XML, a SmartArt with `ppt/diagrams/drawing1.xml`, an `a14:m` formula with a fallback picture, a group), the snapshot

- [ ] **Step 1: Failing tests:** `table_merges_become_spans`, `first_row_is_a_header_when_the_table_says_so`, `svg_blip_is_kept_over_its_png`, `decorative_pictures_are_left_out`, `chart_gives_a_figure_and_its_data`, `smartart_text_becomes_a_list`, `formula_is_math_not_a_picture`, `group_with_empty_child_extent_is_not_scaled`, `group_nesting_past_the_limit_stops`.
- [ ] **Step 2: Implement.** Reuse `excel/drawing.rs` for geometry, fills, lines, pictures, `group_map`, `alternative`; if a piece needs `pub(crate)` or a move to a shared module, make that a separate refactoring commit first (`refactor(drawing): ...`) with unchanged Excel snapshots.
- [ ] **Step 3:** outputs of the sample read and checked; snapshot updated.
- [ ] **Step 4:** write `scripts/powerpoint-text-check.py` and run it on the corpus; fix what it finds.
- [x] **Step 5:** first review round (structure reviewer, Haiku sweep), findings checked and fixed.
  - Done 2026-11-28 with two Sonnet reviewers (16 decks) and a Haiku sweep (97 decks). Confirmed and fixed: titles in text boxes ("Slide N" in 59 of 113 decks, now 30, most of them picture-only slides), title slides without `ctrTitle`, repeated banners and footers in text boxes, the reading order of the shape tree (now by position, see the spec), file paths as descriptions. Checked and not a bug: a table's header "shifted" in gh-0008 (the file's own merges). Left as they are, shared with Word and Excel: an uncaptioned chart's "Chart N" written as the caption of both its figure and its data table; paragraphs of a table cell joined in one Markdown cell.
- [ ] **Step 6: Commit** — `feat(powerpoint): tables, pictures, charts, SmartArt and formulas of slides`

### Task 5: Speaker notes

**Files:**
- Create: `src/powerpoint/notes.rs`
- Modify: `src/powerpoint/build.rs`, the sample generator (notes on two slides, one of them with a list), the snapshot

- [ ] **Step 1: Failing tests:** `notes_follow_their_slide` (a `heading` "Notes" one level below the slide's heading, then the notes' paragraphs), `chunks_of_notes_have_the_slide_in_their_path`, `slide_number_placeholder_of_the_notes_page_is_left_out`, `empty_notes_give_nothing`.
- [ ] **Step 2: Implement**; snapshot updated.
- [ ] **Step 3: Commit** — `feat(powerpoint): speaker notes after the content of their slide`

### Task 6: Corpus, crash test and report

**Files:**
- Create: `tests/data/powerpoint-corpus.tsv`, `scripts/fetch-powerpoint-corpus.sh`
- Modify: `scripts/word-crash-test.py` (take `.pptx` and the other extensions), `.gitignore` (`tests/data/powerpoint-corpus/`)

- [ ] **Step 1:** collect about 100 public decks (gov.uk, data.gov.au, open.canada.ca, data.gov.uk, GitHub repositories of talks; English, Japanese, Vietnamese), half or more with the PDF PowerPoint made of them; record id, sha256, URLs, licence source in the header comment, as `tests/data/word-corpus.tsv`.
- [ ] **Step 2:** `python3 scripts/word-crash-test.py tests/data/powerpoint-corpus`: fix every panic and timeout found (each with a unit test), in this task or as `fix(powerpoint):` commits.
- [ ] **Step 3:** compare the Markdown of 20 decks by eye with the decks (in LibreOffice or the PDFs): titles, bullet levels, reading order, notes; note what is wrong in the commit message or fix it.
- [ ] **Step 4: Commit** — `test(powerpoint): add a corpus of public decks and crash-test it`

### Task 7: Slides drawn as pages (`-o x.pdf`)

**Files:**
- Create: `src/layout/slides/mod.rs` (a deck → `Vec<display::Page>`), `src/layout/slides/text.rs` (a text frame laid out with `layout::para`), `src/layout/slides/table.rs`
- Modify: `src/paper.rs` (`to_pdf`, `to_pdf_with` for `Source::Slides`), `src/layout/mod.rs`, `tests/cli.rs`

- [x] **Step 1: Failing tests:** `pdf_has_one_page_per_visible_slide_of_the_slide_size` (`tests/cli.rs`), `autofit_text_stays_in_its_frame`, `bullets_hang_at_their_indent`, `master_shapes_hidden_when_show_master_shapes_is_off`, `slide_number_field_shows_the_number`, `background_is_inherited_from_the_master`.
- [x] **Step 2: Implement** in the order of the spec's "What a deck becomes (the pages)": background, master and layout shapes, slide shapes (geometry through `excel::view::shape_svg`, pictures cropped, charts as SVG), text frames (insets, anchor, wrap, autofit, line spacing, space before and after, bullets, columns, vertical text), tables with `tableStyles.xml`. Fonts through `layout/fonts` as for Word; a theme font with no open match uses `metrics.txt` (Aptos is there) or the family's fallback.
- [x] **Step 3:** draw the sample and five corpus decks; look at the pages next to LibreOffice's or PowerPoint's PDFs (render both to PNG at 1×; `pdftoppm` is installed); fix what differs most.
  - Done 2026-11-28. The 113 decks are drawn without an error (the slowest in 4 s); the five with PowerPoint's PDF have the same pages and page sizes, and their words lie, on average, 0.6 to 1.9 pt from PowerPoint's vertically and 0.4 to 5.7 pt horizontally (lines broken at other words account for most of it). Found on the way and fixed: PowerPoint's single line is 1.2 times the size of the text whatever the font, and a paragraph's height ignores its mark's size when it has text; `kern="0"` turns kerning on; fills may be seen through (`a:alpha`) or gradients; Wingdings and Symbol bullets need their Unicode look-alike; a link to another slide is underlined; vertical text and the text of turned shapes are turned (`display::Item::Turn`); PowerPoint's default table style need not be in the deck (it is built in). Left for Task 9: words still run about 0.1 pt per word longer than PowerPoint's on some decks; pictures are embedded at their own resolution (PowerPoint's PDFs downsample them, so ours are larger); chart labels are cut short by the chart renderer shared with Word and Excel.
- [x] **Step 4:** second review round (pages reviewer, structure reviewer again), findings checked and fixed.
  - Done 2026-11-28 with two Sonnet reviewers of the pages (74 slides of 11 decks: five next to PowerPoint's PDFs, six next to LibreOffice's). Confirmed and fixed: links drawn in the theme's link colour (unless `ahyp:hlinkClr val="tx"`); text not wrapped and centred drawn far off the slide; turned pictures; outlines inherited from a placeholder's layout; text highlights; WMF clip art with a negative window extent drawn empty (shared with Word); tint and shade computed in sRGB instead of linear light (shared with Excel and Word charts: table bands and default outlines were too dark); half-width katakana without a font; preset outlines drawn as rectangles (stars, snipped corners, ...). `scripts/word-fidelity.py` unchanged (0.959 lines, 0.882 pages). Checked and not ours: boxes paler down a slide (LibreOffice's; the deck fills them alike). Left: chart features the chart renderer lacks (trend lines, pie data labels, label truncation), curved connectors and arrows drawn straight, a bent arrow's rounded bend.
- [x] **Step 5: Commit** — `feat(powerpoint): draw slides as PDF pages as PowerPoint does`

### Task 8: `hard`, `hard-95`, `medium` through the slides' PDF

**Files:**
- Modify: `src/paper.rs` (`laid`: any source that draws its own PDF, Word or slides; `word_layout_html` and `write_html` route the three modes of a deck through `laid`), `tests/powerpoint.rs`

- [ ] **Step 1: Failing tests:** `hard_modes_show_the_slides` (`hard` has one page picture per visible slide; `hard-95` text holds the slides' titles), `medium_keeps_slide_pages`.
- [ ] **Step 2: Implement**; snapshot of the sample updated (the three modes change from the friendly page to the slides).
- [ ] **Step 3:** `node tests/browser/check-hard95.js` and `check-stable.js` on the sample's outputs (not the book view).
- [ ] **Step 4: Commit** — `feat(powerpoint): show slides in the hard, hard-95 and medium modes`

### Task 9: Fidelity against PowerPoint's PDFs, in CI

**Files:**
- Modify: `scripts/word-fidelity.py` (`--corpus-file`, `--corpus` for a deck corpus; the comparison unchanged), `.github/workflows/ci.yml` (a `powerpoint-fidelity` job like `word-fidelity`, fetching the corpus with the decks that have a PDF), `scripts/fetch-powerpoint-corpus.sh`

- [ ] **Step 1:** run it; record the totals (share of lines broken at the same words, page counts).
- [ ] **Step 2:** fix the largest causes of difference first (fonts, autofit, line spacing, insets), each a `fix(layout):` commit that raises the totals and keeps `word-fidelity.py` where it was.
- [ ] **Step 3:** set the CI minimums at the levels reached.
- [ ] **Step 4: Commit** — `ci: measure the slides' pages against PowerPoint's PDFs`

### Task 10: Python, documents, release

**Files:**
- Modify: `python/src/lib.rs` (docs of `format`, `to_pdf` accepts a deck, its error message), `python/README.md`, `python/tests/test_kakemono.py` (a test with the sample), `README.md`, `ARCHITECTURE.md` (module table, pipeline diagram, dependencies), `CLAUDE.md` (a "PowerPoint input" section like "Excel input"; commands), `docs/GUIDE.md`, new `docs/POWERPOINT-INPUT.md` (what is read, what each output shows, limitations), `CHANGELOG.md` through `scripts/release.py` only

- [ ] **Step 1:** write the documents; `cd python && maturin develop --release && pytest tests` in `/root/.venvs/kakemono-py`.
- [ ] **Step 2: Commit** — `docs: document PowerPoint input`
- [ ] **Step 3:** PR (when the user asks): title in English, assignee `iamcaominhtien`, labels `enhancement` and `documentation`, then `python3 scripts/release.py bump --pr <N>` and `chore(release): vX.Y.Z`; SonarQube issues fixed after the checks.

## Order and size

Tasks 1–5 give every flowing output (a usable v1 for reading and RAG); 6 makes it robust; 7–9 add the pages; 10 ships it. Tasks 7 and 9 are the largest (the drawing and its fidelity), as the workbook print was for Excel.
