//! Reconstructed tables as Word tables (`w:tbl`): merged cells, column
//! alignment and the rules of the paper.

use super::parts::{TEXT_WIDTH_TWIPS, TWIPS_PER_PT};
use crate::model::{Span, Table, TableCell};

/// Narrowest grid column, in twips: a column of a few characters.
const MIN_COL_TWIPS: u32 = 300;
/// Width of a thin rule, in eighths of a point.
const THIN_RULE_EIGHTHS: u8 = 4;
/// Width of a heavy rule (booktabs `\toprule`), in eighths of a point.
const HEAVY_RULE_EIGHTHS: u8 = 10;

/// How a table is set: the width of the text column it fits, and the
/// style (and so the size) of its text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fit {
    /// The width it is narrowed to, in twips.
    pub(crate) width_twips: u32,
    /// The paragraph style of its cells.
    pub(crate) style: &'static str,
    /// The size of its text against the 9-point `TableText`.
    pub(crate) scale: f32,
}

/// The table styles by text size, largest first: style, size against 9
/// points.
const SIZES: [(&str, f32); 4] = [
    ("TableText", 1.0),
    ("TableText8", 8.0 / 9.0),
    ("TableText7", 7.0 / 9.0),
    ("TableText6", 6.0 / 9.0),
];

impl Fit {
    /// A table in the portrait text column, in 9-point text.
    pub(crate) const PORTRAIT: Self = Self {
        width_twips: TEXT_WIDTH_TWIPS,
        style: "TableText",
        scale: 1.0,
    };

    /// How `table` is set: in the portrait column when it fits; else, a
    /// table sized by its content (no geometry), on a landscape page of
    /// width `landscape_twips`, in the largest text size it fits in (the
    /// smallest when none does). The second value says it is landscape.
    pub(crate) fn of(table: &Table, landscape_twips: u32) -> (Self, bool) {
        if table.geom.col_bounds.len() == table.n_cols + 1 {
            return (Self::PORTRAIT, false);
        }
        let natural: f32 = content_widths_pt(table).iter().sum::<f32>() * TWIPS_PER_PT;
        if natural <= TEXT_WIDTH_TWIPS as f32 {
            return (Self::PORTRAIT, false);
        }
        let (style, scale) = SIZES
            .iter()
            .copied()
            .find(|(_, scale)| natural * scale <= landscape_twips as f32)
            .unwrap_or(SIZES[SIZES.len() - 1]);
        let fit = Self {
            width_twips: landscape_twips,
            style,
            scale,
        };
        (fit, true)
    }
}

/// What occupies one grid position of a table.
#[derive(Clone, Copy)]
enum Slot<'t> {
    /// Nothing: an empty cell.
    Empty,
    /// The first row of `cell`, which starts at this column.
    Start(&'t TableCell),
    /// A later row of a cell spanning several rows.
    Covered(&'t TableCell),
}

/// The table as a `w:tbl`. `cell_runs` writes the runs of the text of a
/// cell (it registers the links).
pub(crate) fn table_xml(table: &Table, fit: Fit, cell_runs: &mut dyn FnMut(&[Span]) -> String) -> String {
    let slots = grid(table);
    let mut out = String::from(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:jc w:val=\"center\"/><w:tblLayout w:type=\"autofit\"/>\
<w:tblCellMar><w:left w:w=\"80\" w:type=\"dxa\"/><w:right w:w=\"80\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>",
    );
    for width in col_widths(table, fit) {
        out.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
    }
    out.push_str("</w:tblGrid>");
    for (row_idx, row) in slots.iter().enumerate() {
        out.push_str("<w:tr>");
        if row_idx < table.header_rows {
            out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
        }
        let mut col = 0;
        while col < table.n_cols {
            let (span, xml) = cell_xml(table, (row_idx, col), (row[col], fit.style), cell_runs);
            out.push_str(&xml);
            col += span;
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}

/// What occupies every grid position, row by row.
fn grid(table: &Table) -> Vec<Vec<Slot<'_>>> {
    let n_rows = table.rows.len();
    let mut slots = vec![vec![Slot::Empty; table.n_cols]; n_rows];
    for (row_idx, row) in table.rows.iter().enumerate() {
        for cell in row.iter().filter(|cell| cell.col < table.n_cols) {
            slots[row_idx][cell.col] = Slot::Start(cell);
            for covered in slots
                .iter_mut()
                .take((row_idx + cell.rowspan).min(n_rows))
                .skip(row_idx + 1)
            {
                covered[cell.col] = Slot::Covered(cell);
            }
        }
    }
    slots
}

/// One `w:tc` at (`row`, `col`) and the number of grid columns it takes.
fn cell_xml(
    table: &Table,
    (row, col): (usize, usize),
    (slot, style): (Slot<'_>, &str),
    cell_runs: &mut dyn FnMut(&[Span]) -> String,
) -> (usize, String) {
    let (cell, merge) = match slot {
        Slot::Empty => (None, ""),
        Slot::Start(cell) if cell.rowspan > 1 => (Some(cell), "<w:vMerge w:val=\"restart\"/>"),
        Slot::Start(cell) => (Some(cell), ""),
        Slot::Covered(cell) => (Some(cell), "<w:vMerge/>"),
    };
    let span = cell.map_or(1, |cell| cell.colspan.clamp(1, table.n_cols - col));
    let grid_span = if span > 1 {
        format!("<w:gridSpan w:val=\"{span}\"/>")
    } else {
        String::new()
    };
    let borders = borders(table, row, col, span, matches!(slot, Slot::Covered(_)));
    let align = match table.col_align.get(col).copied() {
        Some("center") => "center",
        Some("right") => "right",
        _ => "left",
    };
    let text = match slot {
        Slot::Start(cell) => cell_runs(&cell.spans),
        _ => String::new(),
    };
    let xml = format!(
        "<w:tc><w:tcPr>{grid_span}{merge}{borders}<w:vAlign w:val=\"center\"/></w:tcPr>\
<w:p><w:pPr><w:pStyle w:val=\"{style}\"/><w:jc w:val=\"{align}\"/></w:pPr>{text}</w:p></w:tc>"
    );
    (span, xml)
}

/// The `w:tcBorders` of a cell at (`row`, `col`) spanning `span` columns:
/// the rule above it (not for the later rows of a merged cell), the rule
/// below the last row, and the vertical rules.
fn borders(table: &Table, row: usize, col: usize, span: usize, covered: bool) -> String {
    let end = (col + span).min(table.n_cols);
    let widest = |rules: &[u8]| rules.iter().copied().max().unwrap_or(0);
    let mut sides = String::new();
    let above = if covered {
        0
    } else {
        table.rule_above.get(row).map_or(0, |rules| widest(&rules[col..end]))
    };
    push_rule(&mut sides, "top", above);
    if table.rule_left.get(row).is_some_and(|rules| rules[col]) {
        push_rule(&mut sides, "left", 1);
    }
    if row + 1 == table.rows.len() {
        push_rule(&mut sides, "bottom", widest(&table.rule_bottom[col..end]));
    }
    if end == table.n_cols && table.rule_right.get(row).copied().unwrap_or(false) {
        push_rule(&mut sides, "right", 1);
    }
    if sides.is_empty() {
        sides
    } else {
        format!("<w:tcBorders>{sides}</w:tcBorders>")
    }
}

/// Appends the border `side` for a rule of weight `weight` (0 none, 1
/// thin, 2 heavy).
fn push_rule(out: &mut String, side: &str, weight: u8) {
    let size = match weight {
        0 => return,
        1 => THIN_RULE_EIGHTHS,
        _ => HEAVY_RULE_EIGHTHS,
    };
    out.push_str(&format!(
        "<w:{side} w:val=\"single\" w:sz=\"{size}\" w:space=\"0\" w:color=\"000000\"/>"
    ));
}

/// The widths of the grid columns in twips: the widths of the columns in
/// the PDF (or for its content, [`content_widths_pt`]), narrowed together to
/// fit the text column when they are wider.
fn col_widths(table: &Table, fit: Fit) -> Vec<u32> {
    let bounds = &table.geom.col_bounds;
    let widths_pt: Vec<f32> = if bounds.len() == table.n_cols + 1 {
        bounds.windows(2).map(|pair| (pair[1] - pair[0]).max(0.0)).collect()
    } else {
        content_widths_pt(table).iter().map(|width| width * fit.scale).collect()
    };
    let room = fit.width_twips as f32;
    let total: f32 = widths_pt.iter().sum::<f32>() * TWIPS_PER_PT;
    let per_pt = if total > room || total <= 0.0 {
        room / (widths_pt.iter().sum::<f32>().max(f32::EPSILON))
    } else {
        TWIPS_PER_PT
    };
    let min = (MIN_COL_TWIPS as f32 * fit.scale).round() as u32;
    widths_pt
        .iter()
        .map(|width| ((width * per_pt).round() as u32).max(min))
        .collect()
}

/// Width of a letter of a table's text, about, in points.
const LETTER_PT: f32 = 5.5;
/// The most letters a column is made wide for: longer texts wrap.
const MAX_COL_LETTERS: f32 = 40.0;
/// The fewest letters a column is made wide for.
const MIN_COL_LETTERS: f32 = 2.0;
/// The room around a cell's text, in points.
const CELL_PAD_PT: f32 = 8.0;
/// The first code point of the wide letters (CJK radicals onwards).
const WIDE_FROM: u32 = 0x2E80;

/// The widths of a table that has no geometry (one read from a workbook or
/// a Word document), in points: each column as wide as its longest line of
/// a one-column cell (a wide, Japanese or Chinese, letter counting two),
/// between [`MIN_COL_LETTERS`] and [`MAX_COL_LETTERS`] letters.
fn content_widths_pt(table: &Table) -> Vec<f32> {
    let mut letters = vec![MIN_COL_LETTERS; table.n_cols];
    for cell in table.rows.iter().flatten().filter(|cell| cell.colspan <= 1) {
        let Some(width) = letters.get_mut(cell.col) else {
            continue;
        };
        let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
        let longest = text
            .lines()
            .map(|line| {
                line.chars()
                    .map(|ch| if u32::from(ch) >= WIDE_FROM { 2.0 } else { 1.0 })
                    .sum::<f32>()
            })
            .fold(0.0, f32::max);
        *width = width.max(longest.min(MAX_COL_LETTERS));
    }
    letters.iter().map(|count| count * LETTER_PT + CELL_PAD_PT).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TableGeom;

    #[test]
    fn a_table_without_geometry_is_as_wide_as_its_text() {
        let cell = |text: &str, col: usize| TableCell {
            col,
            colspan: 1,
            rowspan: 1,
            spans: vec![Span::plain(text)],
        };
        let table = Table {
            n_cols: 2,
            header_rows: 0,
            col_align: vec!["left", "right"],
            rows: vec![vec![cell("A rather long label of a row", 0), cell("12", 1)]],
            rule_above: Vec::new(),
            rule_bottom: Vec::new(),
            rule_left: Vec::new(),
            rule_right: Vec::new(),
            geom: TableGeom::default(),
        };
        let widths = col_widths(&table, Fit::PORTRAIT);
        assert!(widths[0] > 3 * widths[1], "{widths:?}");
        assert!(widths.iter().sum::<u32>() <= TEXT_WIDTH_TWIPS);
    }
}
