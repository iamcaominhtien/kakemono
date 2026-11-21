//! Excel workbooks as input: an `.xlsx` file (Office Open XML
//! SpreadsheetML) read into a [`Document`], without the layout analysis a
//! PDF needs, since the cells say what the content is.
//!
//! Each visible sheet becomes a section: a heading with the sheet's name,
//! then its content. A workbook has no pages, like a Word document.

mod build;
mod grid;
mod numfmt;
mod pull;
mod regions;
mod sheet;
mod strings;
mod styles;
mod workbook;

use crate::model::{Block, Document, Meta};
use crate::word::docx::{parse, read_rels, related_part};
use crate::word::zip::ZipArchive;
use anyhow::Result;
use grid::{Grid, Table};
use sheet::SheetContext;
use std::collections::HashMap;
use workbook::{Workbook, is_sml};

/// Font size of the running text, in points: Excel's default font size.
const BODY_SIZE_PT: f32 = 11.0;
/// Distance between baselines, as a ratio of the font size.
const LEADING_RATIO: f32 = 1.2;
/// Relationship type of the styles part.
const STYLES_REL: &str = "styles";
/// Relationship type of a hyperlink.
const HYPERLINK_REL: &str = "hyperlink";
/// Relationship type of an Excel Table part.
const TABLE_REL: &str = "table";
/// Header rows of an Excel Table without `headerRowCount`.
const DEFAULT_HEADER_ROW_COUNT: u32 = 1;

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
    /// A column is right-aligned when more than this share of its body
    /// cells are numbers or dates.
    pub numeric_column_ratio: f32,
    /// At most this many rows of one text cell directly above a table are
    /// its caption; the single-cell rows above them are paragraphs before
    /// the table.
    pub max_caption_rows: usize,
    /// A region whose body rows have less than this share of their
    /// positions (rows × columns, hidden ones left out) covered by a cell
    /// is not a table but text laid out in cells (a form, a prose sheet):
    /// it becomes one paragraph per row. A merge or a centred text covers
    /// its whole area. 0, the default, keeps every region a table: on the
    /// corpus, the fill of forms and prose sheets (0.29 to 0.50) is that
    /// of many sparse financial tables (0.11 to 0.60), so no share parts
    /// them.
    pub table_min_fill_ratio: f32,
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
            max_caption_rows: 3,
            table_min_fill_ratio: 0.0,
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
    let sheets = read_sheets(&zip, &workbook, config)?;
    Ok(build(&workbook, &sheets, config, source))
}

/// The grid of each visible sheet of `workbook`, with its name. A sheet
/// whose part is missing or damaged has no cells; its damaged
/// relationships or table parts are left out.
///
/// # Errors
/// When the shared strings or the styles are damaged.
fn read_sheets<'w>(zip: &ZipArchive<'_>, workbook: &'w Workbook, config: &ExcelConfig) -> Result<Vec<(&'w str, Grid)>> {
    // A workbook without string cells has no shared strings part.
    let strings = match zip.read_text(&workbook.shared_strings_part(zip))? {
        Some(xml) => strings::read(&xml)?,
        None => Vec::new(),
    };
    let styles = match zip.read_text(&related_part(zip, &workbook.part, STYLES_REL))? {
        Some(xml) => styles::read(&xml)?,
        None => styles::Styles::default(),
    };
    let mut sheets = Vec::new();
    for (index, sheet) in workbook.sheets.iter().enumerate() {
        if !sheet.visible {
            continue;
        }
        let xml = if sheet.part.is_empty() {
            None
        } else {
            zip.read_text(&sheet.part).ok().flatten()
        };
        let grid = match xml {
            Some(xml) => {
                let (links, tables) = sheet_rels(zip, &sheet.part);
                let ctx = SheetContext {
                    strings: &strings,
                    styles: &styles,
                    date1904: workbook.date1904,
                    links,
                    tables,
                    print_title_rows: workbook.print_titles.get(&index).copied(),
                    config,
                };
                sheet::read(&xml, &ctx).unwrap_or_default()
            }
            None => Grid::default(),
        };
        sheets.push((sheet.name.as_str(), grid));
    }
    Ok(sheets)
}

/// The relationships of a sheet part: the URLs of its external hyperlinks
/// by relationship id, and its Excel Tables. Damaged relationships give
/// none, and a damaged table part is left out: the sheet is read without
/// them.
fn sheet_rels(zip: &ZipArchive<'_>, part: &str) -> (HashMap<String, String>, Vec<Table>) {
    let mut links = HashMap::new();
    let mut tables = Vec::new();
    for (id, kind, rel) in read_rels(zip, part).unwrap_or_default() {
        if kind == HYPERLINK_REL && rel.external {
            links.insert(id, rel.target);
        } else if kind == TABLE_REL && !rel.external {
            tables.extend(table(zip, &rel.target));
        }
    }
    (links, tables)
}

/// The range and header rows of the table part `part`; `None` when it is
/// missing or damaged.
fn table(zip: &ZipArchive<'_>, part: &str) -> Option<Table> {
    let xml = zip.read_text(part).ok()??;
    let doc = parse(&xml).ok()?;
    let table = doc.root_element();
    if !is_sml(table, "table") {
        return None;
    }
    let header_rows = table
        .attribute("headerRowCount")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_HEADER_ROW_COUNT);
    Some((table.attribute("ref").and_then(sheet::range)?, header_rows))
}

/// The document of a workbook: one section per visible sheet, given with
/// its grid, read with the thresholds `config`.
fn build(workbook: &Workbook, sheets: &[(&str, Grid)], config: &ExcelConfig, source: Option<String>) -> Document {
    let mut blocks: Vec<Block> = sheets
        .iter()
        .flat_map(|(name, grid)| build::sheet_blocks(name, grid, config))
        .collect();
    for (id, block) in blocks.iter_mut().enumerate() {
        block.id = id;
    }
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
    use grid::Merge;

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
        // The string the cell points to is not there: the cell has no
        // text, so it is left out of the grid, as an empty cell is, and
        // the empty sheet gives no blocks.
        assert!(doc.blocks.is_empty());
        let zip = ZipArchive::new(&data).unwrap();
        let workbook = Workbook::read(&zip).unwrap();
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default()).unwrap();
        assert_eq!(sheets.len(), 1);
        assert!(sheets[0].1.cells.is_empty());
    }

    /// The relationships of a sheet with the table part `xl/tables/table1.xml`.
    const SHEET_TABLE_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#;
    /// A sheet with one number cell.
    const ONE_CELL_SHEET: &str = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><v>7</v></c></row></sheetData></worksheet>"#;

    #[test]
    fn damaged_table_part_is_skipped() {
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
            ("xl/worksheets/_rels/sheet1.xml.rels", SHEET_TABLE_RELS),
            ("xl/tables/table1.xml", "<table ref=\"A1:B2\""),
        ]);
        let zip = ZipArchive::new(&data).unwrap();
        let workbook = Workbook::read(&zip).unwrap();
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default()).unwrap();
        assert_eq!(sheets[0].1.cells.len(), 1);
        assert!(sheets[0].1.declared.tables.is_empty());
        assert!(read(&data, &ExcelConfig::default(), None).is_ok());
    }

    #[test]
    fn damaged_sheet_rels_are_skipped() {
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
            ("xl/worksheets/_rels/sheet1.xml.rels", "<Relationships"),
        ]);
        let zip = ZipArchive::new(&data).unwrap();
        let workbook = Workbook::read(&zip).unwrap();
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default()).unwrap();
        assert_eq!(sheets[0].1.cells.len(), 1);
    }

    #[test]
    fn damaged_sheet_part_gives_an_empty_sheet() {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Bad" sheetId="1" r:id="rId1"/><sheet name="Good" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
        let workbook_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/worksheets/sheet1.xml", "<worksheet><sheetData>"),
            ("xl/worksheets/sheet2.xml", ONE_CELL_SHEET),
        ]);
        let zip = ZipArchive::new(&data).unwrap();
        let workbook = Workbook::read(&zip).unwrap();
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default()).unwrap();
        let read: Vec<_> = sheets.iter().map(|(name, grid)| (*name, grid.cells.len())).collect();
        assert_eq!(read, [("Bad", 0), ("Good", 1)]);
        let doc = super::read(&data, &ExcelConfig::default(), None).unwrap();
        // The damaged sheet is empty: it gives no blocks, not even its
        // heading.
        let texts: Vec<_> = doc.blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Good", "7"]);
    }

    #[test]
    fn sheets_are_read_with_their_styles_links_tables_and_print_titles() {
        let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Hidden" sheetId="2" state="hidden" r:id="rId2"/><sheet name="Data" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="_xlnm.Print_Titles" localSheetId="1">Data!$1:$1</definedName></definedNames></workbook>"#;
        let workbook_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
        let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs><xf numFmtId="0"/><xf numFmtId="10"/></cellXfs></styleSheet>"#;
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Site</t></is></c><c r="B1" s="1"><v>0.125</v></c></row></sheetData><hyperlinks><hyperlink ref="A1" r:id="rId1"/></hyperlinks><tableParts count="1"><tablePart r:id="rId2"/></tableParts></worksheet>"#;
        let sheet_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table1.xml"/></Relationships>"#;
        let table = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="1" name="T" ref="A1:B4" headerRowCount="2"/>"#;
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/styles.xml", styles),
            ("xl/worksheets/sheet1.xml", sheet),
            ("xl/worksheets/_rels/sheet1.xml.rels", sheet_rels),
            ("xl/tables/table1.xml", table),
        ]);
        let zip = ZipArchive::new(&data).unwrap();
        let workbook = Workbook::read(&zip).unwrap();
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default()).unwrap();
        assert_eq!(sheets.len(), 1);
        let (name, grid) = &sheets[0];
        assert_eq!(*name, "Data");
        assert_eq!(
            grid.cells[&(0, 0)].spans[0].href.as_deref(),
            Some("https://example.com/")
        );
        assert_eq!(grid.cells[&(0, 1)].spans[0].text, "12.50%");
        let area = Merge {
            top: 0,
            left: 0,
            bottom: 3,
            right: 1,
        };
        assert_eq!(grid.declared.tables, [(area, 2)]);
        assert_eq!(grid.declared.print_title_rows, Some((0, 0)));
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
