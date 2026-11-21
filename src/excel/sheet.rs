//! A worksheet part (`xl/worksheets/sheetN.xml`) read into a [`Grid`]:
//! the cells (`sheetData`), with their text formatted as Excel shows it,
//! the merged ranges, the hyperlinks, the hidden rows and columns, and
//! what the sheet declares about its tables (autofilter, frozen rows).
//!
//! The declared size of the sheet (`dimension`) is not trusted: only the
//! cells there are take memory, and only up to the limits of rows and
//! columns of [`ExcelConfig`].

use crate::model::Span;
use crate::model::style::{BOLD, ITALIC};
use crate::word::docx::parse;
use anyhow::{Context, Result};
use roxmltree::Node;
use std::collections::{BTreeMap, HashMap};

use super::ExcelConfig;
use super::grid::{CellKind, Declared, Grid, GridCell, Merge};
use super::numfmt;
use super::strings;
use super::styles::Styles;
use super::workbook::{is_sml, rel_id};

/// Most letters of a column in a cell reference (`XFD`).
const MAX_COLUMN_LETTERS: usize = 3;

/// What a sheet's cells refer to outside the sheet part.
pub(crate) struct SheetContext<'a> {
    /// The shared strings of the workbook.
    pub(crate) strings: &'a [Vec<Span>],
    /// The cell formats of the workbook.
    pub(crate) styles: &'a Styles,
    /// Dates count from 1904.
    pub(crate) date1904: bool,
    /// The external hyperlinks of the sheet's relationships: URL by
    /// relationship id (a `hyperlink`'s `r:id`).
    pub(crate) links: HashMap<String, String>,
    /// The Excel Tables of the sheet: range and header rows.
    pub(crate) tables: Vec<(Merge, u32)>,
    /// The print title rows of the sheet.
    pub(crate) print_title_rows: Option<(u32, u32)>,
    /// The limits of rows and columns.
    pub(crate) config: &'a ExcelConfig,
}

/// The limits of a sheet, as cell indices: rows and columns at or past
/// them are not read.
#[derive(Clone, Copy)]
struct Limits {
    /// The first row not read.
    rows: u32,
    /// The first column not read.
    cols: u32,
}

impl Limits {
    /// Whether the cell `(row, col)` is read.
    fn contain(self, (row, col): (u32, u32)) -> bool {
        row < self.rows && col < self.cols
    }
}

/// Reads the worksheet `xml`.
///
/// # Errors
/// When `xml` is not well-formed.
pub(crate) fn read(xml: &str, ctx: &SheetContext<'_>) -> Result<Grid> {
    let doc = parse(xml).context("reading a worksheet")?;
    let root = doc.root_element();
    let limits = Limits {
        rows: u32::try_from(ctx.config.max_rows).unwrap_or(u32::MAX),
        cols: u32::try_from(ctx.config.max_cols).unwrap_or(u32::MAX),
    };
    let mut grid = Grid::default();
    let mut frozen_rows = 0;
    let mut auto_filter = None;
    for node in root.children().filter(Node::is_element) {
        if is_sml(node, "sheetData") {
            read_rows(node, ctx, limits, &mut grid);
        } else if is_sml(node, "cols") {
            for col in node.children().filter(|col| is_sml(*col, "col")) {
                if !is_on(col.attribute("hidden")) {
                    continue;
                }
                let bound = |name| col.attribute(name).and_then(|value| value.parse::<u32>().ok());
                let (Some(min), Some(max)) = (bound("min"), bound("max")) else {
                    continue;
                };
                // `min` and `max` are one-based.
                let first = min.saturating_sub(1);
                let last = max.min(limits.cols);
                grid.hidden_cols.extend(first..last);
            }
        } else if is_sml(node, "mergeCells") {
            grid.merges = merges(node, limits);
        } else if is_sml(node, "sheetViews") {
            frozen_rows = frozen(node);
        } else if is_sml(node, "autoFilter") {
            auto_filter = node.attribute("ref").and_then(range);
        } else if is_sml(node, "hyperlinks") {
            link(node, ctx, &mut grid.cells);
        }
    }
    grid.declared = Declared {
        tables: ctx.tables.clone(),
        auto_filter,
        frozen_rows,
        print_title_rows: ctx.print_title_rows,
    };
    Ok(grid)
}

/// Reads the rows of `sheetData` into `grid`.
fn read_rows(data: Node<'_, '_>, ctx: &SheetContext<'_>, limits: Limits, grid: &mut Grid) {
    // A row or cell without a reference follows the previous one.
    let mut next_row = 0;
    for row in data.children().filter(|row| is_sml(*row, "row")) {
        let index = row
            .attribute("r")
            .and_then(|value| value.parse::<u32>().ok())
            .and_then(|value| value.checked_sub(1))
            .unwrap_or(next_row);
        next_row = index.saturating_add(1);
        let cells = row.children().filter(|cell| is_sml(*cell, "c"));
        if index >= limits.rows {
            let has_value = cells
                .clone()
                .any(|cell| cell.children().any(|item| is_sml(item, "v") || is_sml(item, "is")));
            grid.truncated_rows = grid.truncated_rows.saturating_add(u32::from(has_value));
            continue;
        }
        if is_on(row.attribute("hidden")) {
            grid.hidden_rows.insert(index);
        }
        let mut next_col = 0;
        for cell in cells {
            let at = cell.attribute("r").and_then(cell_ref).unwrap_or((index, next_col));
            next_col = at.1.saturating_add(1);
            if !limits.contain(at) {
                continue;
            }
            if let Some(value) = read_cell(cell, ctx) {
                grid.cells.entry(at).or_insert(value);
            }
        }
    }
}

/// The value of a cell (`c`); `None` when it has none (a formula without
/// its cached value, an empty string), unless it centres across the
/// selection.
fn read_cell(cell: Node<'_, '_>, ctx: &SheetContext<'_>) -> Option<GridCell> {
    let xf = ctx.styles.get(
        cell.attribute("s")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0),
    );
    let value = || cell.children().find(|item| is_sml(*item, "v")).map(strings::text);
    let (mut spans, kind) = match cell.attribute("t").unwrap_or("n") {
        "s" => {
            let index = value()?.trim().parse::<usize>().ok();
            let spans = index.and_then(|index| ctx.strings.get(index)).cloned();
            (spans.unwrap_or_default(), CellKind::Text)
        }
        "inlineStr" => {
            let spans = cell
                .children()
                .find(|item| is_sml(*item, "is"))
                .map(strings::string_item)
                .unwrap_or_default();
            (spans, CellKind::Text)
        }
        "str" => (plain(value().unwrap_or_default()), CellKind::Text),
        "b" => {
            let text = match value()?.trim() {
                "1" | "true" => "TRUE",
                _ => "FALSE",
            };
            (plain(text.to_string()), CellKind::Bool)
        }
        "e" => (plain(value().unwrap_or_default()), CellKind::Error),
        "d" => {
            // An ISO date and time: the date part, as written (the time
            // when there is no date part).
            let text = value().unwrap_or_default();
            let text = text.trim();
            let date = match text.split_once('T') {
                Some((date, _)) if !date.is_empty() => date,
                Some((_, time)) => time,
                None => text,
            };
            (plain(date.to_string()), CellKind::Date)
        }
        _ => match value() {
            Some(raw) => match raw.trim().parse::<f64>() {
                Ok(number) => {
                    let kind = if numfmt::is_date(&xf.num_fmt) {
                        CellKind::Date
                    } else {
                        CellKind::Number
                    };
                    (plain(numfmt::format(number, &xf.num_fmt, ctx.date1904)), kind)
                }
                Err(_) => (plain(raw), CellKind::Text),
            },
            None => (Vec::new(), CellKind::Text),
        },
    };
    spans.retain(|span| !span.text.is_empty());
    if spans.is_empty() && !xf.center_continuous {
        return None;
    }
    let font = (if xf.bold { BOLD } else { 0 }) | (if xf.italic { ITALIC } else { 0 });
    for span in spans.iter_mut().filter(|span| span.style == 0) {
        span.style = font;
    }
    Some(GridCell {
        spans,
        kind,
        center_continuous: xf.center_continuous,
    })
}

/// `text` as one unstyled span, none when it is empty.
fn plain(text: String) -> Vec<Span> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Span::plain(text)]
    }
}

/// Whether a boolean attribute is set.
fn is_on(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true"))
}

/// The merged ranges of `mergeCells`, cut to the limits; of two that
/// overlap, the first.
fn merges(list: Node<'_, '_>, limits: Limits) -> Vec<Merge> {
    let mut kept: Vec<Merge> = Vec::new();
    // The kept ranges by first row, and the most rows one spans, so that
    // only those that can reach a new range are compared with it.
    let mut by_top: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    let mut tallest = 0;
    for node in list.children().filter(|node| is_sml(*node, "mergeCell")) {
        let Some(merge) = node.attribute("ref").and_then(range) else {
            continue;
        };
        if !limits.contain((merge.top, merge.left)) {
            continue;
        }
        let merge = Merge {
            bottom: merge.bottom.min(limits.rows - 1),
            right: merge.right.min(limits.cols - 1),
            ..merge
        };
        let overlaps = by_top
            .range(merge.top.saturating_sub(tallest)..=merge.bottom)
            .flat_map(|(_, indices)| indices)
            .any(|&index| kept[index].overlaps(&merge));
        if overlaps {
            continue;
        }
        tallest = tallest.max(merge.bottom - merge.top);
        by_top.entry(merge.top).or_default().push(kept.len());
        kept.push(merge);
    }
    kept
}

/// The rows frozen at the top of the first sheet view.
fn frozen(views: Node<'_, '_>) -> u32 {
    views
        .children()
        .find(|view| is_sml(*view, "sheetView"))
        .and_then(|view| view.children().find(|pane| is_sml(*pane, "pane")))
        .filter(|pane| matches!(pane.attribute("state"), Some("frozen" | "frozenSplit")))
        .and_then(|pane| pane.attribute("ySplit")?.parse::<f64>().ok())
        .filter(|rows| rows.is_finite() && *rows >= 0.0)
        // A count of rows, written as a number.
        .map_or(0, |rows| rows.min(f64::from(u32::MAX)) as u32)
}

/// Points the cells of each external hyperlink of `list` to its URL; an
/// internal link (`location`, to a place in the workbook) is left out.
fn link(list: Node<'_, '_>, ctx: &SheetContext<'_>, cells: &mut BTreeMap<(u32, u32), GridCell>) {
    for node in list.children().filter(|node| is_sml(*node, "hyperlink")) {
        let Some(url) = rel_id(node).and_then(|id| ctx.links.get(id)) else {
            continue;
        };
        let Some(area) = node.attribute("ref").and_then(range) else {
            continue;
        };
        for (_, cell) in cells
            .range_mut((area.top, area.left)..=(area.bottom, area.right))
            .filter(|((_, col), _)| (area.left..=area.right).contains(col))
        {
            for span in &mut cell.spans {
                span.href = Some(url.clone());
            }
        }
    }
}

/// A cell reference (`B2`, `$B$2`) as zero-based (row, column).
pub(crate) fn cell_ref(reference: &str) -> Option<(u32, u32)> {
    let reference = reference.trim();
    let digits = reference.find(|ch: char| ch.is_ascii_digit())?;
    let letters: String = reference[..digits].chars().filter(|ch| *ch != '$').collect();
    if letters.is_empty()
        || letters.len() > MAX_COLUMN_LETTERS
        || !letters.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return None;
    }
    let col = letters.bytes().fold(0u32, |col, byte| {
        col * 26 + u32::from(byte.to_ascii_uppercase() - b'A') + 1
    });
    let row = reference[digits..].parse::<u32>().ok()?.checked_sub(1)?;
    Some((row, col - 1))
}

/// A range (`A1:C3`, or one cell `B2`) as a [`Merge`], its corners in
/// order.
pub(crate) fn range(reference: &str) -> Option<Merge> {
    let (first, last) = reference.split_once(':').unwrap_or((reference, reference));
    let (row1, col1) = cell_ref(first)?;
    let (row2, col2) = cell_ref(last)?;
    Some(Merge {
        top: row1.min(row2),
        left: col1.min(col2),
        bottom: row1.max(row2),
        right: col1.max(col2),
    })
}

#[cfg(test)]
mod tests {
    use super::super::grid::CellKind;
    use super::super::styles;
    use super::*;
    use crate::model::style::BOLD;

    /// `body` as a worksheet.
    fn sheet(body: &str) -> String {
        format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{body}</worksheet>"#
        )
    }

    /// Styles: 0 default, 1 a date, 2 bold, 3 centred across the
    /// selection, 4 two decimals.
    fn test_styles() -> Styles {
        styles::read(concat!(
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
            r#"<fonts><font/><font><b/></font></fonts>"#,
            r#"<cellXfs><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="0" fontId="1"/>"#,
            r#"<xf numFmtId="0"><alignment horizontal="centerContinuous"/></xf><xf numFmtId="2"/></cellXfs></styleSheet>"#,
        ))
        .unwrap()
    }

    /// Reads `body` with the shared strings `a`, `b` and `config`.
    fn read_with(body: &str, links: &[(&str, &str)], config: &ExcelConfig) -> Grid {
        let strings = vec![vec![Span::plain("a")], vec![Span::plain("b")]];
        let styles = test_styles();
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: links
                .iter()
                .map(|(id, url)| (id.to_string(), url.to_string()))
                .collect(),
            tables: Vec::new(),
            print_title_rows: None,
            config,
        };
        read(&sheet(body), &ctx).unwrap()
    }

    /// Reads `body` with the default config.
    fn grid(body: &str) -> Grid {
        read_with(body, &[], &ExcelConfig::default())
    }

    /// The text and kind of every cell, by position.
    fn cells(grid: &Grid) -> Vec<((u32, u32), String, CellKind)> {
        grid.cells
            .iter()
            .map(|(at, cell)| {
                (
                    *at,
                    cell.spans.iter().map(|span| span.text.as_str()).collect(),
                    cell.kind,
                )
            })
            .collect()
    }

    /// `(row, col)`, text, kind.
    fn cell(at: (u32, u32), text: &str, kind: CellKind) -> ((u32, u32), String, CellKind) {
        (at, text.to_string(), kind)
    }

    #[test]
    fn cells_get_their_formatted_text() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="str"><f>A1</f><v>x</v></c>"#,
            r#"<c r="C1" t="inlineStr"><is><r><rPr><b/></rPr><t>B</t></r><r><t>n</t></r><rPh><t>ふ</t></rPh></is></c>"#,
            r#"<c r="D1" t="b"><v>1</v></c><c r="E1" t="e"><v>#N/A</v></c><c r="F1" s="1"><v>45322</v></c>"#,
            r#"<c r="G1" s="4"><v>3.14159</v></c><c r="H1" t="d"><v>2024-01-31T00:00:00</v></c><c r="I1" t="b"><v>0</v></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [
                cell((0, 0), "b", CellKind::Text),
                cell((0, 1), "x", CellKind::Text),
                cell((0, 2), "Bn", CellKind::Text),
                cell((0, 3), "TRUE", CellKind::Bool),
                cell((0, 4), "#N/A", CellKind::Error),
                cell((0, 5), "2024-01-31", CellKind::Date),
                cell((0, 6), "3.14", CellKind::Number),
                cell((0, 7), "2024-01-31", CellKind::Date),
                cell((0, 8), "FALSE", CellKind::Bool),
            ]
        );
        let runs: Vec<_> = grid.cells[&(0, 2)].spans.iter().map(|span| span.style).collect();
        assert_eq!(runs, [BOLD, 0]);
    }

    #[test]
    fn the_cell_style_makes_plain_text_bold() {
        let grid = grid(r#"<sheetData><row><c t="s" s="2"><v>0</v></c><c s="2"><v>7</v></c></row></sheetData>"#);
        assert_eq!(grid.cells.len(), 2);
        assert!(grid.cells.values().all(|cell| cell.spans[0].style == BOLD));
    }

    #[test]
    fn formula_cells_use_their_cached_value() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1"><f>1+1</f><v>2</v></c><c r="B1"><f>1+2</f></c>"#,
            r#"<c r="C1" t="str"><f>"x"</f></c></row></sheetData>"#,
        ));
        assert_eq!(cells(&grid), [cell((0, 0), "2", CellKind::Number)]);
    }

    #[test]
    fn cells_without_a_reference_follow_the_previous_one() {
        let grid = grid(concat!(
            r#"<sheetData><row r="2"><c r="C2"><v>1</v></c><c><v>2</v></c></row>"#,
            r#"<row><c><v>3</v></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [
                cell((1, 2), "1", CellKind::Number),
                cell((1, 3), "2", CellKind::Number),
                cell((2, 0), "3", CellKind::Number),
            ]
        );
    }

    #[test]
    fn dimension_is_not_trusted() {
        let grid = grid(concat!(
            r#"<dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row>"#,
            r#"<row r="1048576"><c r="XFD1048576"><v>2</v></c><c r="B1048576"><v>3</v></c></row></sheetData>"#,
        ));
        // The last row is past the default limits: only A1 is read, and
        // nothing is allocated for the declared size.
        assert_eq!(cells(&grid), [cell((0, 0), "1", CellKind::Number)]);
        let config = ExcelConfig {
            max_rows: 2_000_000,
            max_cols: 20_000,
            ..ExcelConfig::default()
        };
        let grid = read_with(
            concat!(
                r#"<dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row>"#,
                r#"<row r="1048576"><c r="XFD1048576"><v>2</v></c><c r="B1048576"><v>3</v></c></row></sheetData>"#,
            ),
            &[],
            &config,
        );
        assert_eq!(grid.cells.len(), 3);
        assert!(grid.cells.contains_key(&(1_048_575, 16_383)));
    }

    #[test]
    fn hidden_rows_and_columns_are_recorded() {
        let grid = grid(concat!(
            r#"<cols><col min="1" max="1" width="9"/><col min="2" max="3" hidden="1"/></cols>"#,
            r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2" hidden="1"><c r="A2"><v>2</v></c></row></sheetData>"#,
        ));
        assert_eq!(grid.hidden_rows, [1].into());
        assert_eq!(grid.hidden_cols, [1, 2].into());
        assert_eq!(grid.cells.len(), 2);
    }

    #[test]
    fn overlapping_merges_keep_the_first() {
        let grid = grid(concat!(
            r#"<sheetData/><mergeCells><mergeCell ref="A1:B2"/><mergeCell ref="B2:C3"/>"#,
            r#"<mergeCell ref="C1:D1"/><mergeCell ref="bad"/><mergeCell ref="A5:A9"/><mergeCell ref="A4:A5"/></mergeCells>"#,
        ));
        let merge = |top, left, bottom, right| Merge {
            top,
            left,
            bottom,
            right,
        };
        assert_eq!(grid.merges, [merge(0, 0, 1, 1), merge(0, 2, 0, 3), merge(4, 0, 8, 0)]);
    }

    #[test]
    fn declarations_are_read() {
        let strings = Vec::new();
        let styles = Styles::default();
        let config = ExcelConfig::default();
        let table = Merge {
            top: 2,
            left: 0,
            bottom: 8,
            right: 5,
        };
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: HashMap::new(),
            tables: vec![(table, 1)],
            print_title_rows: Some((0, 2)),
            config: &config,
        };
        let xml = sheet(concat!(
            r#"<sheetViews><sheetView workbookViewId="0"><pane ySplit="3" topLeftCell="A4" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>"#,
            r#"<sheetData/><autoFilter ref="A3:F9"/>"#,
        ));
        let grid = read(&xml, &ctx).unwrap();
        assert_eq!(grid.declared.auto_filter, Some(table));
        assert_eq!(grid.declared.frozen_rows, 3);
        assert_eq!(grid.declared.tables, [(table, 1)]);
        assert_eq!(grid.declared.print_title_rows, Some((0, 2)));
        // A split that is not frozen freezes nothing.
        let xml = sheet(r#"<sheetViews><sheetView><pane ySplit="1200"/></sheetView></sheetViews><sheetData/>"#);
        assert_eq!(read(&xml, &ctx).unwrap().declared.frozen_rows, 0);
    }

    #[test]
    fn rows_past_the_limit_are_counted_not_read() {
        let rows: String = (1..=8)
            .map(|row| format!(r#"<row r="{row}"><c><v>{row}</v></c></row>"#))
            .collect();
        let config = ExcelConfig {
            max_rows: 5,
            max_cols: 2,
            ..ExcelConfig::default()
        };
        let body = format!(r#"<sheetData>{rows}<row r="9" hidden="1"/></sheetData>"#);
        let grid = read_with(&body, &[], &config);
        assert_eq!(grid.cells.len(), 5);
        assert_eq!(grid.truncated_rows, 3);
        assert!(grid.hidden_rows.is_empty());
        // Cells past the limit of columns are dropped.
        let grid = read_with(
            r#"<sheetData><row><c><v>1</v></c><c><v>2</v></c><c><v>3</v></c></row></sheetData>"#,
            &[],
            &config,
        );
        assert_eq!(grid.cells.len(), 2);
    }

    #[test]
    fn hyperlinks_point_their_cells_to_the_url() {
        let grid = read_with(
            concat!(
                r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1"><v>3</v></c></row></sheetData>"#,
                r#"<hyperlinks><hyperlink ref="A1" r:id="rId1"/><hyperlink ref="B1" location="Sheet2!A1"/><hyperlink ref="C1:C4" r:id="rId1"/></hyperlinks>"#,
            ),
            &[("rId1", "https://example.com/")],
            &ExcelConfig::default(),
        );
        let hrefs: Vec<_> = grid.cells.values().map(|cell| cell.spans[0].href.as_deref()).collect();
        assert_eq!(
            hrefs,
            [Some("https://example.com/"), None, Some("https://example.com/")]
        );
        assert_eq!(grid.cells[&(0, 1)].spans[0].text, "b");
    }

    #[test]
    fn empty_cells_are_kept_only_when_centred_across_the_selection() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1" t="s" s="3"><v>0</v></c><c r="B1" s="3"/><c r="C1" s="2"/>"#,
            r#"<c r="D1" t="s"><v>9</v></c><c r="E1" t="inlineStr"><is><t></t></is></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [cell((0, 0), "a", CellKind::Text), cell((0, 1), "", CellKind::Text)]
        );
        assert!(grid.cells.values().all(|cell| cell.center_continuous));
    }

    #[test]
    fn damaged_sheets_are_an_error() {
        let strings = Vec::new();
        let styles = Styles::default();
        let config = ExcelConfig::default();
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: HashMap::new(),
            tables: Vec::new(),
            print_title_rows: None,
            config: &config,
        };
        assert!(read("<worksheet", &ctx).is_err());
    }
}
