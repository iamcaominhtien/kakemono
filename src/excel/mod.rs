//! Excel workbooks as input: an `.xlsx` file (Office Open XML
//! SpreadsheetML) read into a [`Document`], without the layout analysis a
//! PDF needs, since the cells say what the content is.
//!
//! Each visible sheet becomes a section: a heading with the sheet's name,
//! then its content. A workbook has no pages, like a Word document.

mod strings;
mod workbook;

use crate::model::{Block, BlockKind, Document, Meta, Span};
use crate::word::zip::ZipArchive;
use anyhow::Result;
use workbook::Workbook;

/// Font size of the running text, in points: Excel's default font size.
const BODY_SIZE_PT: f32 = 11.0;
/// Distance between baselines, as a ratio of the font size.
const LEADING_RATIO: f32 = 1.2;

/// How a workbook is read: the thresholds that cut a sheet into tables and
/// find their header rows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExcelConfig {
    /// Fully empty rows, at least this many in a row, separate two regions
    /// (tables) of a sheet.
    pub region_gap_rows: usize,
    /// Fully empty columns, at least this many side by side, separate two
    /// regions of a sheet.
    pub region_gap_cols: usize,
    /// A row is the first body row of a table when at least this share of
    /// its cells has the most common type (number, date, text, boolean) of
    /// its column in the rows below.
    pub header_type_match_ratio: f32,
    /// How many rows below a row are looked at to find the types of the
    /// columns.
    pub header_lookahead_rows: usize,
    /// A column is right-aligned when at least this share of its body
    /// cells are numbers.
    pub numeric_column_ratio: f32,
    /// The most rows read from one sheet; the rest are left out, with a
    /// paragraph saying how many.
    pub max_rows: usize,
    /// The most columns read from one sheet.
    pub max_cols: usize,
    /// The number of header rows of every table, when the user gives it;
    /// `None` to find them from the file and the cells.
    pub header_rows: Option<usize>,
}

impl Default for ExcelConfig {
    fn default() -> Self {
        Self {
            region_gap_rows: 2,
            region_gap_cols: 1,
            header_type_match_ratio: 0.75,
            header_lookahead_rows: 20,
            numeric_column_ratio: 0.5,
            max_rows: 100_000,
            max_cols: 1_000,
            header_rows: None,
        }
    }
}

/// Whether `data` is an Excel workbook: a ZIP package whose main part is a
/// workbook.
pub fn detect(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
        && ZipArchive::new(data)
            .ok()
            .is_some_and(|zip| workbook::workbook_part(&zip).is_some())
}

/// Reads an Excel workbook into a document. `source` names where it came
/// from ([`Meta::source`]).
///
/// # Errors
/// When the file is damaged or is not a workbook.
pub fn read(data: &[u8], config: &ExcelConfig, source: Option<String>) -> Result<Document> {
    let zip = ZipArchive::new(data)?;
    let workbook = Workbook::read(&zip)?;
    // A workbook without string cells has no shared strings part.
    let strings = match zip.read_text(&workbook.shared_strings_part(&zip))? {
        Some(xml) => strings::read(&xml)?,
        None => Vec::new(),
    };
    // The cells of the sheets, which the strings, the date system, the
    // print titles and `config` are for, are not read yet.
    let _ = (strings, workbook.date1904, &workbook.print_titles, config);
    Ok(build(&workbook, source))
}

/// The document of a workbook: one section per visible sheet.
fn build(workbook: &Workbook, source: Option<String>) -> Document {
    let blocks = workbook
        .sheets
        .iter()
        .filter(|sheet| sheet.visible)
        .enumerate()
        .map(|(id, sheet)| {
            let mut block = Block::new(id, BlockKind::Heading { level: 1 });
            block.spans = vec![Span::plain(sheet.name.clone())];
            block
        })
        .collect();
    Document {
        meta: Meta {
            title: workbook.title.clone(),
            source,
            ..Default::default()
        },
        pages: Vec::new(),
        body_size: BODY_SIZE_PT,
        leading: BODY_SIZE_PT * LEADING_RATIO,
        body_font: String::new(),
        math_font: String::new(),
        blocks,
        pictures: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::zip::{Compression, ZipWriter};

    /// A package with the parts `parts` (name, contents).
    fn package(parts: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = ZipWriter::default();
        for (name, contents) in parts {
            zip.add(name, contents.as_bytes(), Compression::Deflated);
        }
        zip.finish()
    }

    /// The package relationships of a workbook at `xl/workbook.xml`.
    const PACKAGE_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    /// A workbook with one sheet.
    const WORKBOOK: &str = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    /// The relationships of [`WORKBOOK`].
    const WORKBOOK_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;

    #[test]
    fn docx_is_not_taken_for_a_workbook() {
        assert!(!detect(&std::fs::read("tests/data/word/sample.docx").unwrap()));
    }

    #[test]
    fn xlsx_is_a_workbook() {
        assert!(detect(&std::fs::read("tests/data/excel/sample.xlsx").unwrap()));
    }

    #[test]
    fn missing_shared_strings_give_empty_cells() {
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>5</v></c></row></sheetData></worksheet>"#;
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/worksheets/sheet1.xml", sheet),
        ]);
        assert!(detect(&data));
        let doc = read(&data, &ExcelConfig::default(), None).unwrap();
        assert_eq!(doc.blocks[0].text(), "Data");
    }

    #[test]
    fn the_1904_system_and_print_titles_are_read() {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="1"/><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Old" sheetId="2" state="veryHidden" r:id="rId2"/></sheets><definedNames><definedName name="_xlnm.Print_Titles" localSheetId="0">Data!$2:$3</definedName></definedNames></workbook>"#;
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ]);
        let zip = ZipArchive::new(&data).unwrap();
        let workbook = Workbook::read(&zip).unwrap();
        assert!(workbook.date1904);
        assert_eq!(workbook.print_titles.get(&0), Some(&(1, 2)));
        let sheets: Vec<_> = workbook
            .sheets
            .iter()
            .map(|sheet| (sheet.name.as_str(), sheet.visible, sheet.part.as_str()))
            .collect();
        assert_eq!(sheets, [("Data", true, "xl/worksheets/sheet1.xml"), ("Old", false, "")]);
    }
}
