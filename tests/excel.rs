//! Excel workbooks as input: the sample workbook of `tests/data/excel`,
//! written by hand as a flat OpenDocument spreadsheet (`sample.fods`) and
//! saved as `sample.xlsx` by LibreOffice. After a change to `sample.fods`,
//! make the workbook again with:
//!
//! ```text
//! soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods
//! ```
//!
//! The workbook of charts and pictures, `charts.xlsx`, is written by
//! `python3 -I tests/data/excel/make-chart-xlsx.py`.

use kakemono::model::{Block, BlockKind, FloatKind, Table};
use kakemono::{
    Analysis, ChunkOptions, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, Paper, chunks_jsonl,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The sample workbook.
const SAMPLE_XLSX: &str = "excel/sample.xlsx";
/// Charts, a picture in a group and pictures placed in cells, written by
/// `make-chart-xlsx.py`.
const CHARTS_XLSX: &str = "excel/charts.xlsx";

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
fn friendly_sample_lists_its_sheets_in_the_contents() {
    let (paper, analysis) = load(SAMPLE_XLSX);
    let options = HtmlOptions {
        mode: Mode::Friendly,
        embed_fonts: false,
        ..Default::default()
    };
    let html = paper.to_html(&analysis, &options).html;
    assert!(html.contains("<nav class=\"toc\""), "no contents column");
    assert!(
        html.contains("<a href=\"#sec-two-tables\">"),
        "no link to the sheet Two tables"
    );
    assert!(html.contains("<div class=\"layout\">"));
}

#[test]
fn latex_of_a_workbook_names_its_source() {
    let (paper, analysis) = load(SAMPLE_XLSX);
    let latex = paper.to_latex(&analysis, &LatexOptions::default()).latex;
    assert!(
        latex.starts_with("% Converted from an Excel workbook by kakemono."),
        "{}",
        latex.lines().next().unwrap_or_default()
    );
}

/// The hash of each output of the workbook `name`, one line each.
fn output_hashes(name: &str) -> String {
    let (paper, analysis) = load(name);
    let mut outputs: Vec<(String, Vec<u8>)> = Vec::new();
    for (mode_name, mode) in [
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
            format!("html-{mode_name}"),
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
        text.push_str(&format!("{name} {output} {hash}\n"));
    }
    text
}

#[test]
fn excel_outputs_match_their_snapshot() {
    let text = output_hashes(SAMPLE_XLSX) + &output_hashes(CHARTS_XLSX);
    let file = data("snapshots/excel-sample.txt");
    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        old, text,
        "the output of the sample workbooks changed; if this is intended, run \
         KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test excel"
    );
}

/// The charts of a sheet take their place among its drawn objects, in
/// anchor order: a figure of the SVG drawing (captioned with the chart's
/// title, else "Chart N"), then the data table with the same caption; a
/// chart without cached data says so. Pictures in groups and pictures
/// placed in cells are figures too.
#[test]
fn charts_and_pictures_take_their_place() {
    let (paper, analysis) = load(CHARTS_XLSX);
    let doc = &analysis.doc;
    let shown: Vec<String> = doc
        .blocks
        .iter()
        .map(|block| match (&block.kind, &block.table) {
            (BlockKind::Figure { image: Some(image), .. }, _) => {
                let picture = &doc.pictures[image.embedded.unwrap()];
                format!("{} {}", picture.format.extension(), block.text())
            }
            (_, Some(table)) => format!("table {} ({} rows)", block.text(), table.rows.len()),
            _ => block.text(),
        })
        .collect();
    assert_eq!(
        shown,
        [
            "Sales",
            "table  (5 rows)",
            "svg Sales by quarter",
            "table Sales by quarter (data) (5 rows)",
            "svg Chart 2",
            "table Chart 2 (data) (5 rows)",
            "Our logo",
            "png ",
            "svg Channels",
            "table Channels (data) (4 rows)",
            "Forecast",
            "The data of this chart is not stored in the file.",
            "Photos",
            "table  (3 rows)",
            "png ",
            "png ",
        ]
    );
    // The column chart is drawn at the size of its cells, from the top
    // left of E2 to that of L17: seven columns of 75 pixels (10.71
    // characters), fifteen rows of 15 points.
    let BlockKind::Figure { image: Some(image), .. } = &doc.blocks[2].kind else {
        panic!("a chart");
    };
    assert_eq!((image.rect.width(), image.rect.height()), (393.75, 225.0));
    // Its values are on its bars.
    let svg = String::from_utf8(doc.pictures[image.embedded.unwrap()].data.clone()).unwrap();
    assert!(svg.contains(">170</text>") && svg.contains(">140</text>"), "{svg}");
    // The chunks give the data, under the chart's caption.
    let chunks = chunks_jsonl(&paper.to_chunks(&analysis, &ChunkOptions::default()));
    assert!(
        chunks.contains("Sales by quarter (data)\\n| North | South\\nQ1 | 120 | 90"),
        "{chunks}"
    );
}

#[test]
fn an_edited_cell_of_the_json_shows_in_markdown() {
    let (paper, mut analysis) = load(SAMPLE_XLSX);
    let content = paper.to_json(&analysis, &JsonOptions::default());
    let mut json: serde_json::Value = serde_json::from_str(&content.json).unwrap();
    // The first price of the Prices table, edited as a translator would.
    let cell = json["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find_map(|block| block.get_mut("table"))
        .unwrap()["rows"][1][2]
        .as_object_mut()
        .unwrap();
    assert_eq!(cell["text"], "1,388.81 €");
    cell.insert("text".into(), "1 388,81 €".into());
    let report = paper.apply_content(&mut analysis, &json.to_string()).unwrap();
    assert_eq!(report.edited_blocks, 1, "{:?}", report.warnings);
    let markdown = paper.to_markdown(&analysis, &MarkdownOptions::default()).markdown;
    assert!(markdown.contains("| 1 388,81 € |"), "{markdown}");
    assert!(!markdown.contains("1,388.81"));
}
