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
pub(crate) fn table_xml(table: &Table, cell_runs: &mut dyn FnMut(&[Span]) -> String) -> String {
    let slots = grid(table);
    let mut out = String::from(
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/><w:jc w:val=\"center\"/><w:tblLayout w:type=\"autofit\"/>\
<w:tblCellMar><w:left w:w=\"80\" w:type=\"dxa\"/><w:right w:w=\"80\" w:type=\"dxa\"/></w:tblCellMar></w:tblPr><w:tblGrid>",
    );
    for width in col_widths(table) {
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
            let (span, xml) = cell_xml(table, row_idx, col, row[col], cell_runs);
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
    row: usize,
    col: usize,
    slot: Slot<'_>,
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
<w:p><w:pPr><w:pStyle w:val=\"TableText\"/><w:jc w:val=\"{align}\"/></w:pPr>{text}</w:p></w:tc>"
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
/// the PDF, narrowed together to fit the text column when they are wider.
fn col_widths(table: &Table) -> Vec<u32> {
    let bounds = &table.geom.col_bounds;
    let widths_pt: Vec<f32> = if bounds.len() == table.n_cols + 1 {
        bounds.windows(2).map(|pair| (pair[1] - pair[0]).max(0.0)).collect()
    } else {
        vec![1.0; table.n_cols]
    };
    let total: f32 = widths_pt.iter().sum::<f32>() * TWIPS_PER_PT;
    let fit = if total > TEXT_WIDTH_TWIPS as f32 || total <= 0.0 {
        TEXT_WIDTH_TWIPS as f32 / (widths_pt.iter().sum::<f32>().max(f32::EPSILON))
    } else {
        TWIPS_PER_PT
    };
    widths_pt
        .iter()
        .map(|width| ((width * fit).round() as u32).max(MIN_COL_TWIPS))
        .collect()
}
