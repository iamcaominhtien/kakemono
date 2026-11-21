//! Excel workbooks as input: the sample workbook of `tests/data/excel`,
//! written by hand as a flat OpenDocument spreadsheet (`sample.fods`) and
//! saved as `sample.xlsx` by LibreOffice. After a change to `sample.fods`,
//! make the workbook again with:
//!
//! ```text
//! soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods
//! ```

use kakemono::model::{Block, BlockKind, FloatKind, Table};
use kakemono::{
    Analysis, ChunkOptions, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, Paper, chunks_jsonl,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The sample workbook.
const SAMPLE_XLSX: &str = "excel/sample.xlsx";

/// The path of a file of `tests/data`.
fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

/// Loads and analyses a file of `tests/data`.
fn load(name: &str) -> (Paper, Analysis) {
    let paper = Paper::from_bytes(std::fs::read(data(name)).unwrap()).unwrap();
    let analysis = paper.analyze();
    (paper, analysis)
}

#[test]
fn sample_lists_its_visible_sheets() {
    let (paper, analysis) = load(SAMPLE_XLSX);
    assert_eq!(paper.format(), InputFormat::Xlsx);
    let headings: Vec<_> = analysis
        .doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Heading { level: 1 }))
        .map(|block| block.text())
        .collect();
    assert_eq!(headings, ["Prices", "Merged", "Two tables", "Form", "Links", "Frozen"]);
}

#[test]
fn sample_has_its_title_and_no_pages() {
    let (paper, analysis) = load(SAMPLE_XLSX);
    assert!(!paper.format().has_pages());
    assert_eq!(paper.format().name(), "xlsx");
    assert_eq!(paper.page_count(), 0);
    assert_eq!(analysis.doc.meta.title.as_deref(), Some("Sample workbook"));
}

#[test]
fn a_workbook_cannot_be_laid_out_as_pdf() {
    let (paper, _) = load(SAMPLE_XLSX);
    let err = paper.to_pdf().unwrap_err();
    assert!(err.to_string().contains("a workbook has no page layout"), "{err}");
}

/// The table blocks of the sheet `name` of the sample, and the blocks of
/// the sheet (without its heading).
fn sheet(analysis: &Analysis, name: &str) -> Vec<Block> {
    let blocks = &analysis.doc.blocks;
    let start = blocks
        .iter()
        .position(|block| matches!(block.kind, BlockKind::Heading { level: 1 }) && block.text() == name)
        .unwrap_or_else(|| panic!("no sheet {name}"));
    blocks[start + 1..]
        .iter()
        .take_while(|block| !matches!(block.kind, BlockKind::Heading { .. }))
        .cloned()
        .collect()
}

/// The tables of `blocks`.
fn tables(blocks: &[Block]) -> Vec<&Table> {
    blocks.iter().filter_map(|block| block.table.as_ref()).collect()
}

/// The text of each cell of the row `row` of `table`.
fn row_texts(table: &Table, row: usize) -> Vec<String> {
    table.rows[row]
        .iter()
        .map(|cell| cell.spans.iter().map(|span| span.text.as_str()).collect())
        .collect()
}

#[test]
fn sample_prices_is_a_captioned_table_with_a_note() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let blocks = sheet(&analysis, "Prices");
    let figure = &blocks[0];
    assert!(matches!(
        figure.kind,
        BlockKind::Figure {
            kind: FloatKind::Table,
            image: None,
            label: None
        }
    ));
    assert!(figure.caption_above);
    // The caption lines are joined by a dash: a newline in a caption shows
    // as a space.
    assert_eq!(figure.text(), "Table 1: Prices — (in euros)");
    let table = figure.table.as_ref().unwrap();
    assert_eq!(table.header_rows, 1);
    assert_eq!(row_texts(table, 0), ["Year", "Date", "Price", "Change"]);
    let first = row_texts(table, 1);
    assert_eq!(first[1], "1995-01-01");
    assert_eq!(first[3], "12.5%");
    assert_eq!(table.col_align, ["right"; 4]);
    let last = blocks.last().unwrap();
    assert!(matches!(last.kind, BlockKind::Paragraph { .. }));
    assert_eq!(last.text(), "Source: test");
}

#[test]
fn sample_merged_header_has_two_rows() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let blocks = sheet(&analysis, "Merged");
    let tables = tables(&blocks);
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].header_rows, 2);
    assert!(tables[0].rows[0].iter().any(|cell| cell.colspan == 2));
}

#[test]
fn sample_two_tables_sheet_has_three_tables() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let blocks = sheet(&analysis, "Two tables");
    let heads: Vec<_> = tables(&blocks).iter().map(|table| row_texts(table, 0)).collect();
    assert_eq!(heads, [["Fruit", "Kilos"], ["City", "Visitors"], ["Tool", "Count"]]);
}

#[test]
fn sample_form_is_paragraphs() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let blocks = sheet(&analysis, "Form");
    assert!(
        blocks
            .iter()
            .all(|block| matches!(block.kind, BlockKind::Paragraph { .. }))
    );
    let texts: Vec<_> = blocks.iter().map(Block::text).collect();
    assert_eq!(
        texts,
        ["Name: Alice Martin", "City: Lyon", "Status: active", "Comment: none"]
    );
}

#[test]
fn sample_link_keeps_its_target() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let blocks = sheet(&analysis, "Links");
    let tables = tables(&blocks);
    let linked = tables[0]
        .rows
        .iter()
        .flatten()
        .flat_map(|cell| &cell.spans)
        .any(|span| span.href.as_deref() == Some("https://example.com/"));
    assert!(linked);
}

#[test]
fn sample_frozen_rows_are_the_header() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let blocks = sheet(&analysis, "Frozen");
    let tables = tables(&blocks);
    // A table of text only: without the two frozen rows, only the first
    // row would be the header.
    assert_eq!(tables[0].header_rows, 2);
    assert_eq!(row_texts(tables[0], 1), ["(id)", "(text)", "(name)"]);
}

#[test]
fn sample_blocks_are_numbered_in_order() {
    let (_, analysis) = load(SAMPLE_XLSX);
    let ids: Vec<usize> = analysis.doc.blocks.iter().map(|block| block.id).collect();
    assert_eq!(ids, (0..ids.len()).collect::<Vec<_>>());
}

#[test]
fn excel_outputs_match_their_snapshot() {
    let (paper, analysis) = load(SAMPLE_XLSX);
    let mut outputs: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, mode) in [
        ("hard", Mode::Hard),
        ("hard-95", Mode::Hard95),
        ("medium", Mode::Medium),
        ("friendly", Mode::Friendly),
    ] {
        let options = HtmlOptions {
            mode,
            embed_fonts: false,
            ..Default::default()
        };
        outputs.push((
            format!("html-{name}"),
            paper.to_html(&analysis, &options).html.into_bytes(),
        ));
    }
    outputs.push(("docx".into(), paper.to_docx(&analysis, &DocxOptions::default())));
    outputs.push((
        "markdown".into(),
        paper
            .to_markdown(&analysis, &MarkdownOptions::default())
            .markdown
            .into_bytes(),
    ));
    outputs.push((
        "latex".into(),
        paper.to_latex(&analysis, &LatexOptions::default()).latex.into_bytes(),
    ));
    outputs.push(("epub".into(), paper.to_epub(&analysis, &EpubOptions::default())));
    outputs.push((
        "content".into(),
        paper.to_json(&analysis, &JsonOptions::default()).json.into_bytes(),
    ));
    outputs.push((
        "chunks".into(),
        chunks_jsonl(&paper.to_chunks(&analysis, &ChunkOptions::default())).into_bytes(),
    ));
    let mut text = String::new();
    for (output, bytes) in outputs {
        let bytes = String::from_utf8_lossy(&bytes)
            .replace(env!("CARGO_PKG_VERSION"), "VERSION")
            .into_bytes();
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        text.push_str(&format!("{SAMPLE_XLSX} {output} {hash}\n"));
    }
    let file = data("snapshots/excel-sample.txt");
    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        old, text,
        "the output of the sample workbook changed; if this is intended, run \
         KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test excel"
    );
}
