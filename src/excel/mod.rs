//! Excel workbooks as input: an `.xlsx` file (Office Open XML
//! SpreadsheetML) read into a [`Document`], without the layout analysis a
//! PDF needs, since the cells say what the content is.
//!
//! Each visible sheet becomes a section: a heading with the sheet's name,
//! then its content. A workbook has no pages, like a Word document.

mod build;
mod drawing;
mod grid;
pub(crate) mod numfmt;
mod pull;
mod regions;
mod richdata;
mod sheet;
mod strings;
mod styles;
mod workbook;

use crate::analysis::semantics::text::{slug, unique_anchor};
use crate::chart::Charts;
use crate::model::{Block, BlockKind, Document, Meta, Picture};
use crate::word::docx::{parse, read_rels, related_part};
use crate::word::pictures::{MAX_PICTURE_BYTES, pixel_size_pt, store, store_chart};
use crate::word::zip::ZipArchive;
use anyhow::Result;
use drawing::{Drawn, Shape};
use grid::{Grid, Table};
use sheet::SheetContext;
use std::collections::{HashMap, HashSet};
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
/// Relationship type of the drawing part of a sheet.
const DRAWING_REL: &str = "drawing";
/// Relationship type of the theme part of the workbook.
const THEME_REL: &str = "theme";
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
    /// The most columns read from one sheet; the rest are left out, with a
    /// paragraph saying how many.
    pub max_cols: usize,
    /// The most cells kept from one sheet: once a row takes the count to
    /// this many, the rows after it are left out, with a paragraph saying
    /// how many. Rows and columns alone bound a sheet to `max_rows` ×
    /// `max_cols` cells (100 million), far more than memory holds; a cell
    /// takes a few hundred bytes from the reading to the outputs. 2.5
    /// million keeps whole the largest sheet of the corpus (2.12 million
    /// cells, an e-Stat census table, about 1.5 GB at its peak).
    pub max_cells: usize,
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
            max_cells: 2_500_000,
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
    Ok(read_with_paper(data, config, source)?.0)
}

/// Reads an Excel workbook like [`read`], with the paper its print
/// settings name: width and height in points, from the first visible sheet
/// that names a known paper size (`None` when none does).
///
/// # Errors
/// As [`read`].
pub fn read_with_paper(
    data: &[u8],
    config: &ExcelConfig,
    source: Option<String>,
) -> Result<(Document, Option<(f32, f32)>)> {
    let zip = ZipArchive::new(data)?;
    let workbook = Workbook::read(&zip)?;
    let mut pictures = Pictures::default();
    let sheets = read_sheets(&zip, &workbook, config, &mut pictures)?;
    let paper = sheets.iter().find_map(|(_, grid, _)| grid.paper_pt);
    Ok((build(&workbook, &sheets, pictures.files, config, source), paper))
}

/// The grid of each visible sheet of `workbook`, with its name. A sheet
/// whose part is missing or damaged has no cells; its damaged
/// relationships or table parts are left out. Damaged styles give the
/// default ones: the cells keep their text, with General formats.
///
/// # Errors
/// When the shared strings are damaged: the text of every string cell
/// would be lost without a word.
fn read_sheets<'w>(
    zip: &ZipArchive<'_>,
    workbook: &'w Workbook,
    config: &ExcelConfig,
    pictures: &mut Pictures,
) -> Result<Vec<(&'w str, Grid, Vec<Shape>)>> {
    // A workbook without string cells has no shared strings part.
    let strings = match zip.read_text(&workbook.shared_strings_part(zip))? {
        Some(xml) => strings::read(&xml)?,
        None => Vec::new(),
    };
    let styles = zip
        .read_text(&related_part(zip, &workbook.part, STYLES_REL))
        .ok()
        .flatten()
        .and_then(|xml| styles::read(&xml).ok())
        .unwrap_or_default();
    let in_cells = richdata::cell_pictures(zip, &workbook.part);
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
        let mut shapes = Vec::new();
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
                let mut grid = sheet::read(&xml, &ctx).unwrap_or_default();
                shapes = sheet_shapes(zip, &sheet.part, &grid, pictures);
                shapes.extend(cell_pictures(zip, &mut grid, &in_cells, pictures));
                grid
            }
            None => Grid::default(),
        };
        sheets.push((sheet.name.as_str(), grid, shapes));
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

/// The text of the shapes and text boxes in the drawings of the sheet part
/// `part`, their pictures and their charts, by position. A missing or
/// damaged drawing part is left out, and so is a picture whose file is
/// missing, damaged or in a format that cannot be shown, and a chart whose
/// part is. The files of the pictures, and the drawings of the charts, are
/// added to `pictures` (once each), which the pictures given refer to; a
/// chart is drawn at the size its anchor takes on the sheet `grid`.
fn sheet_shapes(zip: &ZipArchive<'_>, part: &str, grid: &Grid, pictures: &mut Pictures) -> Vec<Shape> {
    let mut shapes: Vec<Shape> = Vec::new();
    for (_, kind, rel) in read_rels(zip, part).unwrap_or_default() {
        if kind == DRAWING_REL && !rel.external {
            let drawing = zip.read_text(&rel.target).ok().flatten();
            let items = drawing.and_then(|xml| drawing::items(&xml).ok()).unwrap_or_default();
            let files: HashMap<String, String> = read_rels(zip, &rel.target)
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, _, rel)| !rel.external)
                .map(|(id, _, rel)| (id, rel.target))
                .collect();
            shapes.extend(items.into_iter().filter_map(|(anchor, item)| match item {
                Drawn::Picture(mut picture) => {
                    let index = pictures.read(zip, files.get(&picture.rel)?)?;
                    picture.size_pt = picture.size_pt.or_else(|| pictures.pixel_size_pt(index));
                    picture.index = Some(index);
                    Some((anchor, Drawn::Picture(picture)))
                }
                Drawn::Chart(mut chart) => {
                    let size = match chart.cells {
                        Some((from, to)) => grid.sizes.span_pt(from, to, &grid.hidden_rows, &grid.hidden_cols),
                        None => chart.size_pt.unwrap_or_default(),
                    };
                    let xml = zip.read(files.get(&chart.rel)?).ok()??;
                    let mut placed = pictures.charts(zip).place(&xml, size)?;
                    let index = placed
                        .svg
                        .take()
                        .and_then(|svg| store_chart(&mut pictures.files, svg, placed.size_pt, MAX_PICTURE_BYTES));
                    chart.placed = Some(Box::new((placed, index)));
                    Some((anchor, Drawn::Chart(chart)))
                }
                text => Some((anchor, text)),
            }));
        }
    }
    shapes.sort_by_key(|(anchor, _)| *anchor);
    shapes
}

/// The pictures placed in the cells of `grid`, by the cells' value
/// metadata `in_cells`, in cell order, each anchored at its cell: a cell
/// whose picture is read loses its value (`#VALUE!` for older readers).
/// A picture whose file cannot be shown is left out, and its cell kept.
fn cell_pictures(
    zip: &ZipArchive<'_>,
    grid: &mut Grid,
    in_cells: &HashMap<u32, richdata::CellPicture>,
    pictures: &mut Pictures,
) -> Vec<Shape> {
    let mut placed: Vec<((u32, u32), u32)> = std::mem::take(&mut grid.cell_pictures);
    placed.sort_unstable();
    let mut shapes = Vec::new();
    for (at, vm) in placed {
        let Some(found) = in_cells.get(&vm) else { continue };
        let Some(index) = pictures.read(zip, &found.part) else {
            continue;
        };
        grid.cells.remove(&at);
        let picture = drawing::PictureRef {
            rel: String::new(),
            alt: found.alt.clone(),
            size_pt: pictures.pixel_size_pt(index),
            index: Some(index),
        };
        shapes.push((at, Drawn::CellPicture(picture)));
    }
    shapes
}

/// The picture files of a workbook, each read and made ready for every
/// output (as a Word document's are) once, and the drawings of its charts.
#[derive(Default)]
struct Pictures {
    /// The pictures read, which [`Drawn::Picture`] and [`Drawn::Chart`]
    /// refer to by index.
    files: Vec<Picture>,
    /// The index of each part read, `None` when it cannot be shown.
    parts: HashMap<String, Option<usize>>,
    /// The charts read so far, with the workbook's theme; made at the
    /// first chart.
    charts: Option<Charts>,
}

impl Pictures {
    /// The charts of the workbook `zip`, coloured by its theme.
    fn charts(&mut self, zip: &ZipArchive<'_>) -> &mut Charts {
        self.charts.get_or_insert_with(|| {
            let theme = workbook::workbook_part(zip)
                .and_then(|part| zip.read(&related_part(zip, &part, THEME_REL)).ok().flatten());
            Charts::new(theme.as_deref())
        })
    }

    /// The index of the picture of the part `part`, read once; `None` when
    /// it is missing, damaged, in a format that cannot be shown, too large,
    /// or past the workbook's budget of picture bytes.
    fn read(&mut self, zip: &ZipArchive<'_>, part: &str) -> Option<usize> {
        if let Some(index) = self.parts.get(part) {
            return *index;
        }
        let index = zip
            .read(part)
            .ok()
            .flatten()
            .and_then(|data| store(&mut self.files, data, MAX_PICTURE_BYTES));
        self.parts.insert(part.to_string(), index);
        index
    }

    /// The size in points of the picture `index` at 96 pixels per inch.
    fn pixel_size_pt(&self, index: usize) -> Option<(f32, f32)> {
        pixel_size_pt(self.files.get(index)?)
    }
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
/// its grid and drawings, read with the thresholds `config`; `pictures`
/// are the picture files the drawings refer to.
fn build(
    workbook: &Workbook,
    sheets: &[(&str, Grid, Vec<Shape>)],
    pictures: Vec<Picture>,
    config: &ExcelConfig,
    source: Option<String>,
) -> Document {
    let mut blocks: Vec<Block> = sheets
        .iter()
        .flat_map(|(name, grid, shapes)| build::sheet_blocks(name, grid, shapes, config))
        .collect();
    // Each sheet's heading is a link target, as a Word heading is: the
    // contents column lists the sheets.
    let mut anchors = HashSet::new();
    for (id, block) in blocks.iter_mut().enumerate() {
        block.id = id;
        if matches!(block.kind, BlockKind::Heading { .. }) {
            let slug = slug(&block.text());
            let base = if slug.is_empty() {
                "sec".to_string()
            } else {
                format!("sec-{slug}")
            };
            block.anchor = Some(unique_anchor(&mut anchors, base));
        }
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
        pictures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockKind, FloatKind};
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
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default(), &mut Pictures::default()).unwrap();
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
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default(), &mut Pictures::default()).unwrap();
        assert_eq!(sheets[0].1.cells.len(), 1);
        assert!(sheets[0].1.declared.tables.is_empty());
        assert!(read(&data, &ExcelConfig::default(), None).is_ok());
    }

    #[test]
    fn damaged_styles_are_skipped() {
        let workbook_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/styles.xml", "<styleSheet><cellXfs>"),
            ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
        ]);
        // The cells are read with the default styles.
        let doc = read(&data, &ExcelConfig::default(), None).unwrap();
        let texts: Vec<_> = doc.blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Data", "7"]);
    }

    #[test]
    fn damaged_shared_strings_are_an_error() {
        let workbook_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#;
        let data = package(&[
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/sharedStrings.xml", "<sst><si>"),
            ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
        ]);
        // The text of the string cells would be lost without a word.
        assert!(read(&data, &ExcelConfig::default(), None).is_err());
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
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default(), &mut Pictures::default()).unwrap();
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
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default(), &mut Pictures::default()).unwrap();
        let read: Vec<_> = sheets.iter().map(|(name, grid, _)| (*name, grid.cells.len())).collect();
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
        let sheets = read_sheets(&zip, &workbook, &ExcelConfig::default(), &mut Pictures::default()).unwrap();
        assert_eq!(sheets.len(), 1);
        let (name, grid, _) = &sheets[0];
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

    /// The relationships of a sheet with a drawing.
    const SHEET_DRAWING_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    /// A drawing with a text box of two lines at C3.
    const DRAWING: &str = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><xdr:twoCellAnchor><xdr:from><xdr:col>2</xdr:col><xdr:row>2</xdr:row></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:row>4</xdr:row></xdr:to><xdr:sp><xdr:txBody><a:p><a:r><a:t>Emergency</a:t></a:r></a:p><a:p><a:r><a:t>03-1234</a:t></a:r></a:p></xdr:txBody></xdr:sp><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;

    fn drawing_package(drawing: Option<&str>) -> Vec<u8> {
        let mut parts = vec![
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
            ("xl/worksheets/_rels/sheet1.xml.rels", SHEET_DRAWING_RELS),
        ];
        if let Some(drawing) = drawing {
            parts.push(("xl/drawings/drawing1.xml", drawing));
        }
        package(&parts)
    }

    #[test]
    fn shape_text_follows_the_cells_of_its_sheet() {
        let data = drawing_package(Some(DRAWING));
        let doc = super::read(&data, &ExcelConfig::default(), None).unwrap();
        let texts: Vec<_> = doc.blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Data", "7", "Emergency", "03-1234"]);
    }

    /// A PNG of 4 × 2 pixels.
    fn png() -> Vec<u8> {
        let mut data = Vec::new();
        image::RgbImage::new(4, 2)
            .write_to(&mut std::io::Cursor::new(&mut data), image::ImageFormat::Png)
            .unwrap();
        data
    }

    /// A drawing with the text box of [`DRAWING`] at C3, a picture at A6
    /// (relationship `rId1`, 2 × 1 inches, alt text "Logo"), a picture
    /// whose file is missing (`rId2`) and a chart.
    const PICTURE_DRAWING: &str = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>5</xdr:row></xdr:from><xdr:ext cx="1828800" cy="914400"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="Picture 2" descr="Logo"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="914400"/></a:xfrm></xdr:spPr></xdr:pic><xdr:clientData/></xdr:oneCellAnchor><xdr:twoCellAnchor><xdr:from><xdr:col>2</xdr:col><xdr:row>2</xdr:row></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:row>4</xdr:row></xdr:to><xdr:sp><xdr:txBody><a:p><a:r><a:t>Emergency</a:t></a:r></a:p></xdr:txBody></xdr:sp><xdr:clientData/></xdr:twoCellAnchor><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>8</xdr:row></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:row>9</xdr:row></xdr:to><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="4" name="Picture 3"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId2"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:twoCellAnchor><xdr:twoCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>9</xdr:row></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:row>12</xdr:row></xdr:to><xdr:graphicFrame><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId3"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
    /// The relationships of [`PICTURE_DRAWING`].
    const PICTURE_DRAWING_RELS: &str = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/missing.png"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/></Relationships>"#;

    #[test]
    fn pictures_become_figures_among_the_shapes_in_anchor_order() {
        let mut zip = ZipWriter::default();
        for (name, contents) in [
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
            ("xl/worksheets/_rels/sheet1.xml.rels", SHEET_DRAWING_RELS),
            ("xl/drawings/drawing1.xml", PICTURE_DRAWING),
            ("xl/drawings/_rels/drawing1.xml.rels", PICTURE_DRAWING_RELS),
        ] {
            zip.add(name, contents.as_bytes(), Compression::Deflated);
        }
        zip.add("xl/media/image1.png", &png(), Compression::Stored);
        let doc = super::read(&zip.finish(), &ExcelConfig::default(), None).unwrap();
        let texts: Vec<_> = doc.blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Data", "7", "Emergency", ""]);
        assert_eq!(doc.pictures.len(), 1);
        assert_eq!(doc.pictures[0].data, png());
        let figure = &doc.blocks[3];
        let BlockKind::Figure {
            kind: FloatKind::Graphic,
            image: Some(image),
            ..
        } = &figure.kind
        else {
            panic!("not a picture: {:?}", figure.kind);
        };
        assert_eq!(image.embedded, Some(0));
        assert_eq!((image.rect.width(), image.rect.height()), (144.0, 72.0));
        assert_eq!(figure.rows[0][0].text, "Logo");
        assert_eq!(figure.id, 3);
    }

    /// A chart part: columns of two points, titled `title`.
    fn chart_part(title: &str) -> String {
        let series = crate::chart::test_parts::ser(0, "S", &["a", "b"], &["1", "2"], "");
        crate::chart::test_parts::space(
            &format!(r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>{title}</a:t></a:r></a:p></c:rich></c:tx></c:title>"#),
            &format!(
                "<c:barChart><c:barDir val=\"col\"/>{series}{}</c:barChart>{}",
                crate::chart::test_parts::AXES,
                crate::chart::test_parts::AXIS_PARTS
            ),
        )
    }

    #[test]
    fn a_chart_is_a_figure_then_its_data_at_its_anchor_s_size() {
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetFormatPr defaultRowHeight="20"/><cols><col min="1" max="2" width="20" customWidth="1"/></cols><sheetData><row r="1"><c r="A1"><v>7</v></c></row><row r="11" ht="40" customHeight="1"/></sheetData></worksheet>"#;
        let mut zip = ZipWriter::default();
        for (name, contents) in [
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/worksheets/sheet1.xml", sheet),
            ("xl/worksheets/_rels/sheet1.xml.rels", SHEET_DRAWING_RELS),
            ("xl/drawings/drawing1.xml", PICTURE_DRAWING),
            ("xl/drawings/_rels/drawing1.xml.rels", PICTURE_DRAWING_RELS),
            ("xl/charts/chart1.xml", &chart_part("Sales")),
        ] {
            zip.add(name, contents.as_bytes(), Compression::Deflated);
        }
        let doc = super::read(&zip.finish(), &ExcelConfig::default(), None).unwrap();
        let texts: Vec<_> = doc.blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Data", "7", "Emergency", "Sales", "Sales (data)"]);
        let figure = &doc.blocks[3];
        let BlockKind::Figure {
            kind: FloatKind::Figure,
            image: Some(image),
            ..
        } = &figure.kind
        else {
            panic!("not a chart: {:?}", figure.kind);
        };
        assert_eq!(
            doc.pictures[image.embedded.unwrap()].format,
            crate::model::PictureFormat::Svg
        );
        // From A10 to E13: two columns of 20 characters (140 pixels) and
        // two of the default width; rows 10 and 12 of 20 points, 11 of 40.
        assert_eq!(
            (image.rect.width(), image.rect.height()),
            (2.0 * 105.0 + 2.0 * 48.0, 80.0)
        );
        let table = doc.blocks[4].table.as_ref().expect("the data table");
        assert_eq!(table.rows.len(), 3);
    }

    #[test]
    fn pictures_in_cells_follow_the_table_of_their_cell() {
        let workbook_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata" Target="metadata.xml"/></Relationships>"#;
        let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Photo</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Cup</t></is></c><c r="B2" t="e" vm="1"><v>#VALUE!</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Pen</t></is></c><c r="B3" t="e" vm="2"><v>#VALUE!</v></c></row></sheetData></worksheet>"#;
        let metadata = r#"<metadata xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:xlrd="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata"><metadataTypes count="1"><metadataType name="XLRICHVALUE" minSupportedVersion="120000"/></metadataTypes><futureMetadata name="XLRICHVALUE" count="2"><bk><extLst><ext uri="{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}"><xlrd:rvb i="0"/></ext></extLst></bk><bk><extLst><ext uri="{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}"><xlrd:rvb i="1"/></ext></extLst></bk></futureMetadata><valueMetadata count="2"><bk><rc t="1" v="0"/></bk><bk><rc t="1" v="1"/></bk></valueMetadata></metadata>"#;
        let values = r#"<rvData xmlns="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata" count="2"><rv s="0"><v>0</v><v>5</v><v>A cup</v></rv><rv s="0"><v>1</v><v>5</v><v></v></rv></rvData>"#;
        let structures = r#"<rvStructures xmlns="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata" count="1"><s t="_localImage"><k n="_rvRel:LocalImageIdentifier" t="i"/><k n="CalcOrigin" t="i"/><k n="Text" t="s"/></s></rvStructures>"#;
        let rels = r#"<richValueRels xmlns="http://schemas.microsoft.com/office/spreadsheetml/2022/richvaluerel" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><rel r:id="rId1"/><rel r:id="rId2"/></richValueRels>"#;
        let rel_files = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/image1.png"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/missing.png"/></Relationships>"#;
        let mut zip = ZipWriter::default();
        for (name, contents) in [
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", WORKBOOK),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/worksheets/sheet1.xml", sheet),
            ("xl/metadata.xml", metadata),
            ("xl/richData/rdrichvalue.xml", values),
            ("xl/richData/rdrichvaluestructure.xml", structures),
            ("xl/richData/richValueRel.xml", rels),
            ("xl/richData/_rels/richValueRel.xml.rels", rel_files),
        ] {
            zip.add(name, contents.as_bytes(), Compression::Deflated);
        }
        zip.add("xl/media/image1.png", &png(), Compression::Stored);
        let doc = super::read(&zip.finish(), &ExcelConfig::default(), None).unwrap();
        assert_eq!(doc.pictures.len(), 1);
        let kinds: Vec<_> = doc
            .blocks
            .iter()
            .map(|block| match &block.kind {
                BlockKind::Figure { image: Some(_), .. } => format!("picture {}", block.rows[0][0].text),
                BlockKind::Figure { .. } => "table".to_string(),
                _ => block.text(),
            })
            .collect();
        assert_eq!(kinds, ["Data", "table", "picture A cup"]);
        // The cell of the picture shown is empty; the other keeps its value.
        let table = doc.blocks[1].table.as_ref().unwrap();
        let text = |row: usize, col: usize| -> String {
            table.rows[row]
                .iter()
                .find(|cell| cell.col == col)
                .map(|cell| cell.spans.iter().map(|span| span.text.as_str()).collect())
                .unwrap_or_default()
        };
        assert_eq!(text(1, 1), "");
        assert_eq!(text(2, 1), "#VALUE!");
    }

    #[test]
    fn a_picture_declaring_a_huge_size_is_left_out_or_drawn_smaller() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(100_000u32.to_be_bytes());
        png.extend(100_000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><rect width="10" height="10"/></svg>"#;
        for (media, drawn) in [(png.as_slice(), false), (svg.as_slice(), true)] {
            let mut zip = ZipWriter::default();
            for (name, contents) in [
                ("_rels/.rels", PACKAGE_RELS),
                ("xl/workbook.xml", WORKBOOK),
                ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
                ("xl/worksheets/sheet1.xml", ONE_CELL_SHEET),
                ("xl/worksheets/_rels/sheet1.xml.rels", SHEET_DRAWING_RELS),
                ("xl/drawings/drawing1.xml", PICTURE_DRAWING),
                ("xl/drawings/_rels/drawing1.xml.rels", PICTURE_DRAWING_RELS),
            ] {
                zip.add(name, contents.as_bytes(), Compression::Deflated);
            }
            zip.add("xl/media/image1.png", media, Compression::Stored);
            let doc = super::read(&zip.finish(), &ExcelConfig::default(), None).unwrap();
            assert_eq!(doc.pictures.len(), usize::from(drawn));
        }
    }

    #[test]
    fn a_damaged_or_missing_drawing_is_left_out() {
        for drawing in [None, Some("<xdr:wsDr")] {
            let data = drawing_package(drawing);
            let doc = super::read(&data, &ExcelConfig::default(), None).unwrap();
            let texts: Vec<_> = doc.blocks.iter().map(Block::text).collect();
            assert_eq!(texts, ["Data", "7"]);
        }
    }
}
