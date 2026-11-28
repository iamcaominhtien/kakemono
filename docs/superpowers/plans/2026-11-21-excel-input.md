# Excel input (`.xlsx`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `kakemono convert book.xlsx -o x.md` (and every output without pages) works, reading the workbook directly into a `model::Document`.

**Architecture:** A new module `src/excel/` reads the zip parts (with `word::zip` and `roxmltree`) into a small grid model per sheet (`grid.rs`), then `build.rs` splits each sheet into regions and turns each into caption / header / body / notes blocks. `Paper` gets a `Source::Sheet` variant. Separately and first, the chunks renderer learns to cut tables between rows and repeat their header.

**Tech Stack:** Rust (edition of the crate), `roxmltree`, `crate::word::zip::ZipArchive`; LibreOffice (`soffice`) only to produce the committed test sample.

**Spec:** `docs/superpowers/specs/2026-11-21-excel-input-design.md`

## Global Constraints

- Rust only for the reader; no new dependency in `Cargo.toml`.
- Dates and times are written ISO: `2024-01-31`, `14:05`, `2024-01-31 14:05` (seconds `14:05:09` only when the code shows seconds). Decimal `.`, thousands `,`.
- Built-in currency ids 5–8: number without a symbol. A symbol is kept only when a custom code writes it.
- Furigana (`rPh`) never reaches the text.
- Thresholds are fields of `ExcelConfig` (documented, unit-suffixed), re-exported from `src/config.rs` as `kakemono::config::ExcelConfig`. Defaults: `region_gap_rows: 2`, `region_gap_cols: 1`, `header_type_match_ratio: 0.75`, `header_lookahead_rows: 20`, `numeric_column_ratio: 0.5`, `max_rows: 100_000`, `max_cols: 1_000`, `header_rows: None`.
- `InputFormat::Xlsx` (`name() == "xlsx"`, `has_pages() == false`) is a breaking change of the public API: the commit that adds it is `feat!:`.
- Conventional Commits, English, imperative; each commit ends with the session's `Co-Authored-By`/`Claude-Session` lines.
- Before each commit: `cargo fmt --check && cargo clippy --all-targets -- -D warnings` and `cargo test --release`.
- No wildcard `use x::*` outside test modules (SonarQube `rust:S2208`).

## Review Focus

1. A workbook whose `sharedStrings.xml` is absent, or whose cells reference an out-of-range string index → empty text, no panic. Test in Task 2 (`missing_shared_strings_give_empty_cells`).
2. A sheet whose `<dimension>` claims `A1:XFD1048576` but has 3 cells (common in exports) → the grid is built from the cells present, not from the dimension. Test in Task 4 (`dimension_is_not_trusted`).
3. A cell with a date format but a negative or huge serial (`-1`, `3e6`) → written as the plain number, no panic. Test in Task 3 (`impossible_dates_fall_back_to_numbers`).
4. A merge that overlaps another merge or reaches past the grid limits → clamped/ignored, no panic. Test in Task 4 (`overlapping_merges_keep_the_first`).
5. A `.docx` must still be read as Word, and a zip that is neither → still a "not a PDF" error as today. Test in Task 2 (`docx_is_not_taken_for_a_workbook`).

---

### Task 1: Chunks cut tables between rows and repeat the header

**Files:**
- Modify: `src/render/chunks/mod.rs` (`block_text`, `Builder::add_block`, a new `table_pieces`)
- Test: unit tests in `src/render/chunks/mod.rs`; snapshots `tests/data/snapshots/*.txt` (chunks hashes) updated

**Interfaces:**
- Produces: `fn table_pieces(block: &Block, room: usize, counter: &TokenCounter) -> Option<Vec<String>>` — `None` when the block has no rebuilt table.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_long_table_is_cut_between_rows_and_repeats_its_header() {
    // A Figure{Table} block, caption "Table 1: Prices", header row ["Year","Price"],
    // 200 body rows ["2000+i", "i.5"]; max_tokens 64.
    let chunks = render_chunks(&doc(vec![table_block(200)]), &opts(64));
    assert!(chunks.len() > 1);
    for chunk in &chunks {
        assert!(chunk.text.contains("Table 1: Prices\nYear | Price\n"), "{}", chunk.text);
        assert!(chunk.text.lines().filter(|l| l.contains(" | ")).all(|l| l.split(" | ").count() == 2));
    }
    let rows: usize = chunks.iter().map(|c| c.text.lines().filter(|l| l.starts_with("20")).count()).sum();
    assert_eq!(rows, 200);
}

#[test]
fn a_table_without_header_is_still_cut_between_rows() {
    // Same, header_rows 0 and no caption: every line of every chunk is a whole row.
}

#[test]
fn a_row_longer_than_a_chunk_is_cut_between_words() { /* one row of 300 words, max 64 */ }
```

- [ ] **Step 2: Run them, expect FAIL** — `cargo test --release --lib chunks`
- [ ] **Step 3: Implement.** In `add_block`, for a block with `table`, use `table_pieces`: head = caption (as `block_text` writes it) + header rows joined `"\n"`; then greedily add body rows while `count(head + rows) <= room`; a single row over budget goes through `split_long` with the head prepended to each part. A head that takes more than half of `room` is dropped (same rule as the section path in `enter`).
- [ ] **Step 4: Run `cargo test --release`.** Expect the new tests to pass and the chunks hash of every paper with a long table to change. Inspect two of them: `KAKEMONO_PRINT_METRICS=1` is not needed; run `./target/release/kakemono convert tests/data/1602.03837v1.pdf -o /tmp/x.jsonl` before/after and diff the chunks that hold table rows. Then `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test real_papers --test word`.
- [ ] **Step 5: Commit** — `feat(chunks): cut tables between rows and repeat their header`

### Task 2: Workbook detection, sheets and strings, wired into `Paper`

**Files:**
- Create: `src/excel/mod.rs`, `src/excel/workbook.rs`, `src/excel/strings.rs`
- Create: `tests/data/excel/sample.fods` (source) and `tests/data/excel/sample.xlsx` (`soffice --headless --convert-to xlsx sample.fods`), `tests/excel.rs`
- Modify: `src/lib.rs` (`pub mod excel;`), `src/config.rs` (re-export `ExcelConfig`), `src/paper.rs` (`InputFormat::Xlsx`, `Source::Sheet`, `from_bytes`, `from_bytes_with`, `analyze_with`, `to_pdf`, `word_format`), `Cargo.toml` `include` if `tests/` data is listed there (check)

**Interfaces:**
- Produces:
  - `pub fn excel::detect(data: &[u8]) -> bool`
  - `pub fn excel::read(data: &[u8], config: &ExcelConfig, source: Option<String>) -> anyhow::Result<Document>`
  - `pub struct ExcelConfig { region_gap_rows: usize, region_gap_cols: usize, header_type_match_ratio: f32, header_lookahead_rows: usize, numeric_column_ratio: f32, max_rows: usize, max_cols: usize, header_rows: Option<usize> }` + `Default` with the Global Constraints values (in `src/excel/mod.rs`)
  - `pub(crate) struct Workbook { sheets: Vec<SheetRef>, date1904: bool, title: Option<String>, print_titles: HashMap<usize, (u32, u32)> }`, `pub(crate) struct SheetRef { name: String, visible: bool, part: String }`
  - `pub(crate) fn strings::read(xml: &str) -> Result<Vec<Vec<Span>>>` (rich runs → `Span` with `BOLD`/`ITALIC`/`SUP`/`SUB`; `rPh` skipped)
  - `Paper::from_bytes_with(data: Vec<u8>, excel: &ExcelConfig) -> Result<Paper>`; `from_bytes` calls it with the default.
- For this task `build` is a stub: one `Heading{1}` per visible sheet, nothing else (Task 5 fills it).

- [ ] **Step 1: Write `sample.fods`** with sheets: `Prices` (title row "Table 1: Prices", unit row "(in euros)", header Year/Date/Price/Change, 30 rows with a date column `YYYY-MM-DD`-unrelated format `DD/MM/YYYY`, currency `#,##0.00 "€"`, percent `0.0%`, a note row "Source: test"), `Merged` (2-row merged header Region | Population{Men, Women}), `Two tables` (two tables separated by 3 empty rows, and one beside them after an empty column), `Hidden` (hidden sheet), `Form` (only label/value cells in one column, text values), `Links` (a hyperlink cell), `Frozen` (all-text table with frozen first 2 rows: header is 2 rows). Convert to `sample.xlsx`; commit both. Note the command in `tests/excel.rs`'s module doc.
- [ ] **Step 2: Failing tests**

```rust
// src/excel/strings.rs
#[test] fn furigana_is_left_out() {
    let xml = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>東京</t><rPh sb="0" eb="2"><t>トウキョウ</t></rPh></si></sst>"#;
    assert_eq!(plain(&read(xml).unwrap()[0]), "東京");
}
#[test] fn rich_runs_keep_bold() { /* <r><rPr><b/></rPr><t>B</t></r><r><t>n</t></r> → spans ["B" BOLD, "n" 0] */ }
// src/excel/mod.rs
#[test] fn docx_is_not_taken_for_a_workbook() { assert!(!detect(&std::fs::read("tests/data/word/sample.docx").unwrap())); }
#[test] fn missing_shared_strings_give_empty_cells() { /* zip with workbook + sheet whose c t="s" v=5, no sharedStrings → read() is Ok */ }
// tests/excel.rs
#[test] fn sample_lists_its_visible_sheets() {
    let (paper, analysis) = load("excel/sample.xlsx");
    assert_eq!(paper.format(), InputFormat::Xlsx);
    let headings: Vec<_> = /* Heading blocks' text */;
    assert_eq!(headings, ["Prices", "Merged", "Two tables", "Form", "Links", "Frozen"]);
}
```

- [ ] **Step 3: Run, expect FAIL.**
- [ ] **Step 4: Implement.** `detect`: `PK` header, zip opens, `_rels/.rels` officeDocument target ends in `workbook.xml` and exists. In `from_bytes_with` check `excel::detect` before `word::detect`. `to_pdf` on a workbook: `bail!("a workbook has no page layout")`. `word_format()` returns `None` for `Source::Sheet`, so `word_layout_html` is skipped and `html_options` gives `friendly`.
- [ ] **Step 5: Run `cargo test --release`, expect PASS** (no existing snapshot changes).
- [ ] **Step 6: Commit** — `feat!: read Excel workbooks (.xlsx) as input` with a `BREAKING CHANGE: InputFormat has a new variant, Xlsx.` footer.

### Task 3: Number formats

**Files:**
- Create: `src/excel/styles.rs`, `src/excel/numfmt.rs`

**Interfaces:**
- Produces:
  - `pub(crate) struct Styles { xfs: Vec<Xf> }`, `pub(crate) struct Xf { num_fmt: NumFmt, bold: bool, italic: bool, center_continuous: bool }`; `pub(crate) fn styles::read(xml: &str) -> Result<Styles>`; `Styles::get(&self, s: usize) -> &Xf` (default xf when out of range)
  - `pub(crate) enum NumFmt { General, Code(String) }` (built-in ids mapped to their en-US code, except dates → ISO tokens, currency 5–8 → `#,##0` / `#,##0.00` with their negative sections)
  - `pub(crate) fn numfmt::format(value: f64, fmt: &NumFmt, date1904: bool) -> String`
  - `pub(crate) fn numfmt::is_date(fmt: &NumFmt) -> bool`

- [ ] **Step 1: Failing table test** in `numfmt.rs`:

```rust
#[test]
fn formats_as_excel_shows_them_with_iso_dates() {
    let cases: &[(f64, &str, &str)] = &[
        (1234.5, "General", "1234.5"), (0.1 + 0.2, "General", "0.3"), (1e20, "General", "1E+20"),
        (1234.5, "#,##0.00", "1,234.50"), (-1234.5, "#,##0.00;(#,##0.00)", "(1,234.50)"),
        (0, "0.00;-0.00;\"-\"", "-"), (0.125, "0.0%", "12.5%"), (12345.678, "0.00E+00", "1.23E+04"),
        (1500.0, "\"¥\"#,##0", "¥1,500"), (2.0, "[$€-407] #,##0.00", "€ 2.00"), (5.0, "[Red]0", "5"),
        (45322.0, "m/d/yyyy", "2024-01-31"), (45322.0, "dd\\.mm\\.yy", "2024-01-31"),
        (45322.5856, "yyyy-mm-dd hh:mm", "2024-01-31 14:03"), (0.5856, "h:mm AM/PM", "14:03"),
        (0.585601, "hh:mm:ss", "14:03:16"), (60.0, "yyyy-mm-dd", "1900-02-29"), // Excel's own leap day of 1900
    ];
    for (v, code, want) in cases { assert_eq!(format(*v, &NumFmt::Code(code.to_string()), false), *want, "{code}"); }
}
#[test] fn builtin_14_is_iso_and_builtin_7_has_no_symbol() { /* id 14 → "2024-01-31"; id 7 on 1234.5 → "1,234.50" */ }
#[test] fn the_1904_system_shifts_dates() { /* 0.0 with date1904 → "1904-01-01" */ }
#[test] fn impossible_dates_fall_back_to_numbers() { /* -1.0 and 3e6 with "yyyy-mm-dd" → "-1", "3000000" */ }
```

(A date-only code writes the date; a time-only code writes the time; both → `date time`. Whatever order and separators the code uses, the output is ISO. `General`: up to 11 significant digits like Excel, trailing zeros removed.)

- [ ] **Step 2: Run, expect FAIL.**  **Step 3: Implement.**  **Step 4: Run, expect PASS.**
- [ ] **Step 5: Commit** — `feat(excel): format numbers as Excel shows them, with ISO dates`

### Task 4: Sheet reading into a grid

**Files:**
- Create: `src/excel/sheet.rs`, `src/excel/grid.rs`

**Interfaces:**
- Consumes: `Styles`, `numfmt::format`, shared strings (Tasks 2–3).
- Produces:
  - `pub(crate) struct Grid { cells: BTreeMap<(u32, u32), GridCell>, merges: Vec<Merge>, hidden_rows: HashSet<u32>, hidden_cols: HashSet<u32>, declared: Declared, truncated_rows: u32 }` (0-based row, col)
  - `pub(crate) struct GridCell { spans: Vec<Span>, kind: CellKind, center_continuous: bool }`, `pub(crate) enum CellKind { Number, Date, Text, Bool, Error }`
  - `pub(crate) struct Merge { top: u32, left: u32, bottom: u32, right: u32 }`
  - `pub(crate) struct Declared { tables: Vec<(Merge, u32 /*header rows*/)>, auto_filter: Option<Merge>, frozen_rows: u32, print_title_rows: Option<(u32, u32)> }`
  - `pub(crate) fn sheet::read(xml: &str, ctx: &SheetContext) -> Result<Grid>` where `SheetContext { strings: &[Vec<Span>], styles: &Styles, date1904: bool, links: HashMap<String, String> /* cell ref → URL from sheet rels */, tables: Vec<(Merge, u32)>, print_title_rows: Option<(u32, u32)>, config: &ExcelConfig }`

- [ ] **Step 1: Failing tests** in `sheet.rs`, each with an inline `<worksheet>` XML string:
  - `cells_get_their_formatted_text` (s / str / inlineStr / b → `TRUE` / e → `#N/A` / n with a date style → `CellKind::Date`)
  - `formula_cells_use_their_cached_value` (`<f>` with `<v>` → value; without `<v>` → no cell)
  - `dimension_is_not_trusted` (`<dimension ref="A1:XFD1048576"/>` + 3 cells → 3 cells, no allocation by dimension)
  - `hidden_rows_and_columns_are_recorded` (`<row hidden="1">`, `<col min="2" max="3" hidden="1">`)
  - `overlapping_merges_keep_the_first`
  - `declarations_are_read` (`<autoFilter ref="A3:F9"/>`, `<pane ySplit="3" state="frozen"/>`)
  - `rows_past_the_limit_are_counted_not_read` (`max_rows: 5`, 8 rows → 5 rows, `truncated_rows == 3`)
- [ ] **Step 2: FAIL.**  **Step 3: Implement** with `roxmltree` (`word::docx::parse` options: no DTD, node limit). Cells past `max_cols` are dropped.  **Step 4: PASS.**
- [ ] **Step 5: Commit** — `feat(excel): read the cells, merges and declarations of a sheet`

### Task 5: Building blocks from a grid

**Files:**
- Create: `src/excel/build.rs`, `src/excel/regions.rs`
- Modify: `src/excel/mod.rs` (`read` calls `build::sheet_blocks` for each visible sheet), `tests/excel.rs`

**Interfaces:**
- Consumes: `Grid` (Task 4), `ExcelConfig`.
- Produces:
  - `pub(crate) fn regions::split(grid: &Grid, config: &ExcelConfig) -> Vec<Merge>` — visible cells only; connected blocks where two cells are connected when their rows differ by ≤ `region_gap_rows` and columns by ≤ `region_gap_cols` (a merge connects its whole area); returned as bounding boxes, sorted by (top, left).
  - `pub(crate) fn build::sheet_blocks(name: &str, grid: &Grid, config: &ExcelConfig) -> Vec<Block>`
  - `pub(crate) struct Parts { caption: Vec<u32>, header: Vec<u32>, body: Vec<u32>, notes: Vec<u32> }` and `fn parts(grid: &Grid, region: &Merge, config: &ExcelConfig) -> Parts` (rows by index)

Header decision (`fn header_rows(...) -> usize`), first that applies: `config.header_rows`; a declaration covering the region (table `headerRowCount`; auto-filter top row; `frozen_rows`; print titles) → rows from the first non-caption row down to the declared last header row; the column-type rule (spec, "Header rows" 3) using `header_type_match_ratio` and `header_lookahead_rows`; else 1. Merges reaching into the header extend it. One-row region → 0.

Block output per sheet: `Heading{level: 1}` (sheet name) → for each region: caption rows + table (`Figure{kind: FloatKind::Table, image: None, label: None}`, `spans` = caption lines joined by a `\n` span, `caption_above = true`, `table` with `n_cols`, `header_rows`, `col_align` "right" where body cells are > `numeric_column_ratio` Number/Date, merges → `colspan`/`rowspan`, centre-continuous text spanning the empty cells to its right, rules as `word::build::rebuild_table` sets them) → note rows as `Paragraph`s. A region with < 2 columns of data → one `Paragraph` per row (two cells → `label: value`). `truncated_rows > 0` → a final `Paragraph` "N more rows are not shown.". Empty sheet → no blocks.

- [ ] **Step 1: Failing unit tests** (grids built in code):
  - `side_by_side_tables_are_two_regions`, `three_empty_rows_split_and_one_does_not`
  - `title_and_unit_rows_become_the_caption`, `source_line_becomes_a_note`
  - `two_level_header_found_by_types` (fukuchiyama-like: 3 text header rows over numeric body → 3)
  - `declaration_wins_over_types` (first row `Region | 2021 | 2022` over numbers: the type rule alone gives 0; with `frozen_rows = 1` → 1)
  - `all_text_table_without_declaration_takes_the_first_row`
  - `user_header_rows_wins`
  - `one_column_region_is_paragraphs`, `merged_cells_get_spans`, `numeric_columns_are_right_aligned`
- [ ] **Step 2: Failing integration tests** in `tests/excel.rs` against `sample.xlsx`: Prices → caption `"Table 1: Prices\n(in euros)"`, header `["Year","Date","Price","Change"]`, first body row dates ISO and `"12.5%"`-style; Merged → `header_rows == 2` with a `colspan 2` cell; Two tables → 3 tables; Form → paragraphs only; Links → a span with `href`; Frozen → `header_rows == 2`.
- [ ] **Step 3: FAIL.**  **Step 4: Implement.**  **Step 5: PASS.**
- [ ] **Step 6: Commit** — `feat(excel): split sheets into captioned tables with their header rows`

### Task 6: Text of shapes and text boxes

**Files:**
- Create: `src/excel/drawing.rs`
- Modify: `src/excel/mod.rs`, `src/excel/build.rs`

**Interfaces:**
- Produces: `pub(crate) fn drawing::texts(xml: &str) -> Result<Vec<((u32, u32), String)>>` — for each `xdr:sp` (also inside `xdr:grpSp`) with non-empty `a:t` text, its anchor `xdr:from` (row, col) and its text (paragraphs `a:p` joined by `\n`); sorted by anchor. `build::sheet_blocks` gains `shapes: &[((u32, u32), String)]` and emits one `Paragraph` per shape after the regions.

- [ ] **Step 1: Failing tests**: `shape_text_is_read_in_anchor_order`, `grouped_shapes_are_read`, `pictures_and_connectors_give_nothing` (`xdr:pic`, `xdr:cxnSp`).
- [ ] **Step 2: FAIL.**  **Step 3: Implement** (sheet rels → `drawing` part).  **Step 4: PASS.**
- [ ] **Step 5: Commit** — `feat(excel): read the text of shapes and text boxes`

### Task 7: CLI, Python, docs, snapshot

**Files:**
- Modify: `src/main.rs` (help texts list `.xlsx`; `--header-rows <N>` on `convert`, building `ExcelConfig { header_rows: Some(N), ..Default::default() }` and `Paper::from_bytes_with`), `python/src/lib.rs` (help strings; a `header_rows: Option<usize>` keyword where a Paper is loaded from bytes, if the Python API loads through one function — check), `README.md`, `ARCHITECTURE.md`, `CLAUDE.md` (intro line + commands block: `convert <file.xlsx> -o out/x.md`), `docs/` (new `docs/EXCEL-INPUT.md` like `WORD-INPUT.md`: what a workbook becomes, the header order, ISO dates)
- Test: `tests/excel.rs` (`excel_outputs_match_their_snapshot`, same mechanism as `word_outputs_match_their_snapshot`, file `tests/data/snapshots/excel-sample.txt`), `tests/cli.rs` (`convert sample.xlsx -o x.md` exits 0 and the md has a `| Year |` table; `--header-rows 0` gives no header separator line for `Prices`; `-o x.pdf` fails with "no page layout")

- [ ] **Step 1: Failing tests** as listed.  **Step 2: FAIL.**  **Step 3: Implement.**
- [ ] **Step 4: `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test excel`**, then `cargo test --release`; `cd python && maturin develop --release && pytest tests` (in a venv).
- [ ] **Step 5: Commit** — `feat(cli): convert Excel workbooks, with --header-rows` and `docs: describe the Excel input`

### Task 8: Corpus download and crash check

**Files:**
- Modify: `tests/data/excel-corpus.tsv` (add a `sha256` column, computed from the files surveyed in this session or freshly downloaded; drop rows that are not `.xlsx`/`.xlsm` or no longer download)
- Create: `scripts/fetch-excel-corpus.sh` (modelled on `scripts/fetch-word-corpus.sh`: into `tests/data/excel-corpus/`, sha256 checked)
- Modify: `scripts/word-crash-test.py` (also take `.xlsx`/`.xlsm`; skip the `pdf` output for them) — or document why a separate script

- [ ] **Step 1:** `scripts/fetch-excel-corpus.sh 8` downloads the corpus.
- [ ] **Step 2:** `cargo build --release && python3 scripts/word-crash-test.py tests/data/excel-corpus` → no panic, no timeout. Fix any found in the owning module, with a unit test named after the behaviour.
- [ ] **Step 3:** Run `kakemono convert` to md on 10 files across categories (ONS, OMB, BP, Smartsheet, e-Stat, 福知山市, デジタル庁 spec, IPA DX, 広島市 form, UR flow) and read the output: captions, headers, notes, forms as paragraphs, no furigana, ISO dates. Note in the PR description what still reads badly.
- [ ] **Step 4: Commit** — `test: download the Excel corpus and check it does not crash`
