//! Excel workbooks as input: the sample workbook of `tests/data/excel`,
//! written by hand as a flat OpenDocument spreadsheet (`sample.fods`) and
//! saved as `sample.xlsx` by LibreOffice. After a change to `sample.fods`,
//! make the workbook again with:
//!
//! ```text
//! soffice --headless --convert-to xlsx --outdir tests/data/excel tests/data/excel/sample.fods
//! ```

use kakemono::model::BlockKind;
use kakemono::{Analysis, InputFormat, Paper};
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
