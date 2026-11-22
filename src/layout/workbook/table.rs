//! A table of the document as a table of the layout: its columns as wide
//! as their text when the table fits the width of the text, else its
//! longer columns wrapped; set smaller, turned on landscape pages, or cut
//! into groups of columns (each repeating the first) until it fits, so
//! that no column is ever lost; rules above it, under its header and
//! below it; its header rows repeated on each page.

use super::{Builder, WorkbookPdfConfig};
use crate::model::{Table as DocTable, TableCell};
use crate::word::fmt::model::{
    Align, Block, Border, BorderStyle, Cell, LineSpacing, Para, ParaProps, Row, Table, TableProps, VAlign, VMerge,
    Width,
};

/// Rule value of the model for a heavy rule (see `model::HEAVY_RULE`).
const HEAVY: u8 = 2;
/// The height of a line of a table's text, in em, to guess the height of
/// its header (the fonts' line height is 1.1 to 1.2 em).
const LINE_HEIGHT_EM: f32 = 1.2;
/// Columns fit the width when they exceed it by less than this, in points
/// (rounding).
const FIT_TOLERANCE_PT: f32 = 0.01;

/// The tables of a document's table, ready for the layout.
pub(super) struct LaidTables {
    /// One table, or the groups of columns of one too wide even on
    /// landscape pages.
    pub(super) tables: Vec<Table>,
    /// They are set on landscape pages.
    pub(super) landscape: bool,
}

/// The widths of the text of a table's cells, measured at one font size.
struct Measured {
    /// The font size they were measured at.
    size: f32,
    /// The width of all the lines of each cell (row by row, in the row's
    /// order) at that size, and its number of line breaks: what its height
    /// is guessed from. (Their paragraphs are not kept: a sheet can have
    /// millions of cells.)
    cells: Vec<Vec<(f32, usize)>>,
    /// The width each column needs on one line (its widest cell).
    natural: Vec<f32>,
    /// The narrowest each column is made when the table wraps: its
    /// typical cell (see [`WorkbookPdfConfig::typical_cell_max_em`]).
    least: Vec<f32>,
    /// The cells over several columns: first column, count, width on one
    /// line, least width.
    spans: Vec<(usize, usize, f32, f32)>,
}

/// Lays out `model` for pages whose text is `widths` wide (portrait,
/// landscape) and `landscape_height` high when turned: at the table's font
/// size when it fits the portrait width, else smaller down to the least
/// size, else on landscape pages from the table's size down, else in
/// groups of columns there.
pub(super) fn lay(
    builder: &mut Builder<'_>,
    model: &DocTable,
    widths: (f32, f32),
    landscape_height: f32,
) -> LaidTables {
    let config = *builder.config;
    let measured = measure(builder, model, config.table_size_pt);
    let sizes = sizes(&config);
    for (landscape, width) in [(false, widths.0), (true, widths.1)] {
        for &size in &sizes {
            if let Some(grid) = fit(&measured, size, width, &config) {
                let table = build(builder, model, &measured, size, grid, landscape_height);
                return LaidTables {
                    tables: vec![table],
                    landscape,
                };
            }
        }
    }
    // Too wide even on landscape pages in the least size: groups of
    // columns, each with the first.
    let size = config.min_table_size_pt;
    let tables = groups(&measured, size, widths.1, &config)
        .iter()
        .map(|keep| {
            let part = columns_of(model, keep);
            let measured = measure(builder, &part, size);
            let grid =
                fit(&measured, size, widths.1, &config).unwrap_or_else(|| squeezed(&measured, size, widths.1, &config));
            build(builder, &part, &measured, size, grid, landscape_height)
        })
        .collect();
    LaidTables {
        tables,
        landscape: true,
    }
}

/// The font sizes tried for a table, largest first.
fn sizes(config: &WorkbookPdfConfig) -> Vec<f32> {
    let step = config.table_size_step_pt.max(0.1);
    let mut sizes = Vec::new();
    let mut size = config.table_size_pt;
    while size > config.min_table_size_pt + FIT_TOLERANCE_PT {
        sizes.push(size);
        size -= step;
    }
    sizes.push(config.min_table_size_pt);
    sizes
}

/// The paragraphs of the cells of `model` at `size`, with the widths of
/// their text, and what each column needs.
fn measure(builder: &mut Builder<'_>, model: &DocTable, size: f32) -> Measured {
    let n_cols = model.n_cols.max(1);
    let config = *builder.config;
    let mut natural = vec![0.0f32; n_cols];
    let mut word = vec![0.0f32; n_cols];
    let mut body = vec![(0.0f32, 0usize); n_cols];
    let mut spans = Vec::new();
    let mut cells = Vec::with_capacity(model.rows.len());
    for (row_at, row) in model.rows.iter().enumerate() {
        let header = row_at < model.header_rows;
        let mut out = Vec::with_capacity(row.len());
        for cell in row {
            let para = cell_para(builder, model, cell, header, size);
            let (one_line, widest, total, breaks) = builder.measurer.measure(&para);
            let col = cell.col.min(n_cols - 1);
            let count = cell.colspan.clamp(1, n_cols - col);
            if count == 1 {
                natural[col] = natural[col].max(one_line);
                word[col] = word[col].max(widest);
                if header {
                    // A header cell on at most so many lines.
                    word[col] = word[col].max(one_line / config.header_max_lines.max(1.0));
                } else if one_line > 0.0 {
                    body[col].0 += one_line;
                    body[col].1 += 1;
                }
            } else {
                let least = one_line.min(config.typical_cell_max_em * size);
                spans.push((col, count, one_line, least));
            }
            out.push((total, breaks));
        }
        cells.push(out);
    }
    let least = (0..n_cols)
        .map(|col| {
            let (sum, count) = body[col];
            let mean = if count == 0 { 0.0 } else { sum / count as f32 };
            mean.max(word[col])
                .min(config.typical_cell_max_em * size)
                .max(config.min_column_em * size)
                .min(natural[col].max(config.min_column_em * size))
        })
        .collect();
    Measured {
        size,
        cells,
        natural,
        least,
        spans,
    }
}

/// The widths of the columns of `measured` set at `size` in a width of
/// `width`: as wide as their text when that fits, else each at least its
/// least width, the rest shared by the columns that need more; `None` when
/// even the least widths do not fit.
fn fit(measured: &Measured, size: f32, width: f32, config: &WorkbookPdfConfig) -> Option<Vec<f32>> {
    let (natural, least) = scaled(measured, size, config);
    let total: f32 = natural.iter().sum();
    if total <= width + FIT_TOLERANCE_PT {
        return Some(natural);
    }
    if least.iter().sum::<f32>() > width + FIT_TOLERANCE_PT {
        return None;
    }
    Some(share(&natural, &least, width))
}

/// The natural and least widths of the columns of `measured` at `size`,
/// padding included; the columns under a cell over several are widened
/// (evenly) to hold it.
fn scaled(measured: &Measured, size: f32, config: &WorkbookPdfConfig) -> (Vec<f32>, Vec<f32>) {
    let ratio = size / measured.size;
    let pad = 2.0 * config.cell_padding_em * size;
    let mut natural: Vec<f32> = measured.natural.iter().map(|width| width * ratio + pad).collect();
    let mut least: Vec<f32> = measured.least.iter().map(|width| width * ratio + pad).collect();
    for &(col, count, one_line, at_least) in &measured.spans {
        for (widths, need) in [(&mut natural, one_line), (&mut least, at_least)] {
            let have: f32 = widths[col..col + count].iter().sum();
            let missing = need * ratio + pad - have;
            if missing > 0.0 {
                for column in &mut widths[col..col + count] {
                    *column += missing / count as f32;
                }
            }
        }
    }
    // A cell over several columns can widen the least width of a column
    // past its natural one.
    for (natural, least) in natural.iter_mut().zip(&least) {
        *natural = natural.max(*least);
    }
    (natural, least)
}

/// The widths of columns that share `width`: each at least its `least`
/// width; what is left goes first to the columns that need little more to
/// be on one line (they get their `natural` width), then to the others in
/// proportion to what they lack.
fn share(natural: &[f32], least: &[f32], width: f32) -> Vec<f32> {
    let mut widths = least.to_vec();
    let mut rest = width - least.iter().sum::<f32>();
    let mut open: Vec<usize> = (0..widths.len()).filter(|&col| natural[col] > least[col]).collect();
    while rest > FIT_TOLERANCE_PT && !open.is_empty() {
        let lacking: f32 = open.iter().map(|&col| natural[col] - widths[col]).sum();
        let even = rest / open.len() as f32;
        // The columns that need less than an even share are given what
        // they need, and the others share again what is left.
        let (done, others): (Vec<usize>, Vec<usize>) =
            open.iter().partition(|&&col| natural[col] - widths[col] <= even);
        if done.is_empty() {
            for &col in &open {
                widths[col] += rest * (natural[col] - widths[col]) / lacking;
            }
            break;
        }
        for &col in &done {
            rest -= natural[col] - widths[col];
            widths[col] = natural[col];
        }
        open = others;
    }
    widths
}

/// The least widths of `measured` at `size`, made smaller in proportion to
/// fill `width`: for a group of columns whose cells over several columns
/// ask for more than the width.
fn squeezed(measured: &Measured, size: f32, width: f32, config: &WorkbookPdfConfig) -> Vec<f32> {
    let (_, least) = scaled(measured, size, config);
    let total: f32 = least.iter().sum();
    least.iter().map(|column| column * width / total.max(1.0)).collect()
}

/// The groups of columns of a table too wide for `width` at `size`, each
/// with the first column and as many next ones as fit by their least
/// widths; a column too wide even alone with the first is a group of its
/// own.
fn groups(measured: &Measured, size: f32, width: f32, config: &WorkbookPdfConfig) -> Vec<Vec<usize>> {
    let (_, least) = scaled(measured, size, config);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for col in 1..least.len() {
        match groups.last_mut() {
            Some(group) if used + least[col] <= width + FIT_TOLERANCE_PT => group.push(col),
            _ => {
                groups.push(vec![0, col]);
                used = least[0];
            }
        }
        used += least[col];
    }
    if groups.is_empty() {
        groups.push(vec![0]);
    }
    groups
}

/// The table of the columns `keep` (in order) of `model`: a cell is at its
/// first kept column and spans the kept columns it covered; a cell that
/// covers none is left out. The rules follow their columns.
fn columns_of(model: &DocTable, keep: &[usize]) -> DocTable {
    let rows = model
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .filter_map(|cell| {
                    let covered: Vec<usize> = (0..keep.len())
                        .filter(|&new| (cell.col..cell.col + cell.colspan.max(1)).contains(&keep[new]))
                        .collect();
                    Some(TableCell {
                        col: *covered.first()?,
                        colspan: covered.len(),
                        rowspan: cell.rowspan,
                        spans: cell.spans.clone(),
                    })
                })
                .collect()
        })
        .collect();
    let pick = |values: &[u8]| keep.iter().map(|&col| values.get(col).copied().unwrap_or(0)).collect();
    let pick_bool = |values: &[bool]| {
        keep.iter()
            .map(|&col| values.get(col).copied().unwrap_or(false))
            .collect()
    };
    DocTable {
        n_cols: keep.len(),
        header_rows: model.header_rows,
        col_align: keep
            .iter()
            .map(|&col| model.col_align.get(col).copied().unwrap_or("left"))
            .collect(),
        rows,
        rule_above: model.rule_above.iter().map(|row| pick(row)).collect(),
        rule_bottom: pick(&model.rule_bottom),
        rule_left: model.rule_left.iter().map(|row| pick_bool(row)).collect(),
        rule_right: model.rule_right.clone(),
        geom: model.geom.clone(),
    }
}

/// The paragraph of `cell` at `size`: aligned as its column (a header cell
/// over several columns centred), its text kept with the next row in the
/// header.
fn cell_para(builder: &Builder<'_>, model: &DocTable, cell: &TableCell, header: bool, size: f32) -> Para {
    let align = if header && cell.colspan > 1 {
        Align::Center
    } else {
        match model.col_align.get(cell.col).copied() {
            Some("right") => Align::Right,
            Some("center") => Align::Center,
            _ => Align::Left,
        }
    };
    Para {
        props: ParaProps {
            align,
            line: LineSpacing::Auto(1.0),
            keep_next: header,
            ..Default::default()
        },
        mark: builder.families.run(size),
        inlines: builder.families.inlines(&cell.spans, size),
        ..Default::default()
    }
}

/// The layout's table of `model`, whose cells were `measured`, at `size`
/// in the column widths `grid`; its header rows are repeated on each page
/// when they take at most their share of `page_height`.
fn build(
    builder: &Builder<'_>,
    model: &DocTable,
    measured: &Measured,
    size: f32,
    grid: Vec<f32>,
    page_height: f32,
) -> Table {
    let config = builder.config;
    let header_rows = model.header_rows.min(model.rows.len());
    let repeat = header_rows < model.rows.len()
        && header_height(measured, model, size, &grid, config) <= config.max_head_ratio * page_height;
    let pad = config.cell_padding_em * size;
    let pad_top = config.cell_padding_top_em * size;
    let mut rows_of = Rows {
        builder,
        model,
        size,
        margins: [pad, pad_top, pad, pad_top],
        rules: Rules::of(model, config),
        header_rows,
        keep_whole: model.rows.len() - header_rows <= config.max_unbroken_body_rows,
        merged: vec![None; grid.len()],
    };
    let mut rows = Vec::with_capacity(model.rows.len());
    for row_at in 0..model.rows.len() {
        let cant_split =
            row_height(measured, model, row_at, size, &grid, config) <= config.max_unsplit_row_ratio * page_height;
        rows.push(Row {
            height: None,
            header: row_at < header_rows && repeat,
            cant_split,
            cells: rows_of.cells(row_at),
            ..Default::default()
        });
    }
    let total: f32 = grid.iter().sum();
    Table {
        props: TableProps {
            width: Width::Pt(total),
            align: Align::Center,
            fixed: true,
            cell_margins: rows_of.margins,
            ..Default::default()
        },
        grid,
        rows,
    }
}

/// Makes the cells of the rows of a table, one row after the other.
struct Rows<'a, 'b> {
    /// The builder, for the fonts.
    builder: &'a Builder<'b>,
    /// The table.
    model: &'a DocTable,
    /// The font size of its text.
    size: f32,
    /// The margins of its cells: left, top, right, bottom.
    margins: [f32; 4],
    /// Its rules.
    rules: Rules,
    /// Its number of header rows.
    header_rows: usize,
    /// Each row is kept with the next (a short table).
    keep_whole: bool,
    /// The cells merged down from a row above, by their first column:
    /// (last row they cover, their column count).
    merged: Vec<Option<(usize, usize)>>,
}

impl Rows<'_, '_> {
    /// The cells of row `row_at`, every grid column covered: its own, the
    /// places a cell merged down from above covers, and empty ones where it
    /// has no value.
    fn cells(&mut self, row_at: usize) -> Vec<Cell> {
        let row = &self.model.rows[row_at];
        let n_cols = self.merged.len();
        let header = row_at < self.header_rows;
        let mut cells = Vec::new();
        let (mut col, mut next) = (0, 0);
        while col < n_cols {
            if let Some((_, count)) = self.merged[col].filter(|&(last, _)| last >= row_at) {
                let mut cell = blank_cell(self.builder, self.size, count, self.margins);
                cell.vmerge = VMerge::Continue;
                cell.borders = self.rules.borders(row_at, col, count, true);
                cells.push(cell);
                col += count;
                continue;
            }
            // A cell is placed at its column (or at the first free one, if
            // its column was taken: its text is never lost).
            let Some(doc_cell) = row.get(next).filter(|cell| cell.col <= col) else {
                let mut cell = blank_cell(self.builder, self.size, 1, self.margins);
                cell.borders = self.rules.borders(row_at, col, 1, false);
                cells.push(cell);
                col += 1;
                continue;
            };
            let count = doc_cell.colspan.clamp(1, n_cols - col);
            let rowspan = doc_cell.rowspan.clamp(1, self.model.rows.len() - row_at);
            let mut para = cell_para(self.builder, self.model, doc_cell, header, self.size);
            // Kept with the next row: the header, and every row of a short
            // table, which then moves whole to a new page.
            para.props.keep_next = header || (self.keep_whole && row_at + rowspan < self.model.rows.len());
            let mut cell = Cell {
                span: count,
                vmerge: if rowspan > 1 { VMerge::Restart } else { VMerge::None },
                margins: self.margins,
                valign: if rowspan > 1 { VAlign::Center } else { VAlign::Top },
                blocks: vec![Block::Para(Box::new(para))],
                borders: self.rules.borders(row_at, col, count, false),
                ..Default::default()
            };
            if rowspan > 1 {
                cell.borders[2] = None;
                self.merged[col] = Some((row_at + rowspan - 1, count));
            }
            cells.push(cell);
            next += 1;
            col += count;
        }
        cells
    }
}

/// An empty cell over `count` columns at `size`, with `margins`: a place
/// with no value, or one under a cell merged down.
fn blank_cell(builder: &Builder<'_>, size: f32, count: usize, margins: [f32; 4]) -> Cell {
    Cell {
        span: count,
        margins,
        blocks: vec![Block::Para(Box::new(Para {
            props: ParaProps {
                line: LineSpacing::Auto(1.0),
                ..Default::default()
            },
            mark: builder.families.run(size),
            ..Default::default()
        }))],
        ..Default::default()
    }
}

/// The height of the header rows of `model` at `size` in the columns
/// `grid`, guessed from the widths of their text (see [`row_height`]).
fn header_height(measured: &Measured, model: &DocTable, size: f32, grid: &[f32], config: &WorkbookPdfConfig) -> f32 {
    (0..model.header_rows.min(model.rows.len()))
        .map(|row| row_height(measured, model, row, size, grid, config))
        .sum()
}

/// The height of row `row` of `model` at `size` in the columns `grid`,
/// guessed from the widths of its text: as high as its cell of most lines,
/// each cell's text taking as many lines as it is wider than its columns.
fn row_height(
    measured: &Measured,
    model: &DocTable,
    row: usize,
    size: f32,
    grid: &[f32],
    config: &WorkbookPdfConfig,
) -> f32 {
    let ratio = size / measured.size;
    let pad = 2.0 * config.cell_padding_em * size;
    let mut lines = 1.0f32;
    for (cell, &(total, breaks)) in model.rows[row].iter().zip(&measured.cells[row]) {
        let col = cell.col.min(grid.len().saturating_sub(1));
        let count = cell.colspan.clamp(1, grid.len() - col);
        let room: f32 = grid[col..col + count].iter().sum::<f32>() - pad;
        lines = lines.max((total * ratio / room.max(1.0)).ceil() + breaks as f32);
    }
    lines * LINE_HEIGHT_EM * size + 2.0 * config.cell_padding_top_em * size
}

/// The rules of a table: heavy above and below, light under the header,
/// as the model has them.
struct Rules {
    /// Rule above each position (`model::Table::rule_above`).
    above: Vec<Vec<u8>>,
    /// Rule below the last row, per column.
    bottom: Vec<u8>,
    /// Vertical rule on the left of each position.
    left: Vec<Vec<bool>>,
    /// Vertical rule on the right edge, per row.
    right: Vec<bool>,
    /// Number of header rows (when the table has a body).
    header_rows: usize,
    /// Number of rows.
    rows: usize,
    /// Number of columns.
    cols: usize,
    /// Width of a heavy rule.
    heavy: f32,
    /// Width of a light rule.
    light: f32,
}

impl Rules {
    /// The rules of `model`, at the widths of `config`.
    fn of(model: &DocTable, config: &WorkbookPdfConfig) -> Self {
        let rows = model.rows.len();
        let header_rows = if model.header_rows < rows { model.header_rows } else { 0 };
        Self {
            above: model.rule_above.clone(),
            bottom: model.rule_bottom.clone(),
            left: model.rule_left.clone(),
            right: model.rule_right.clone(),
            header_rows,
            rows,
            cols: model.n_cols.max(1),
            heavy: config.heavy_rule_pt,
            light: config.light_rule_pt,
        }
    }

    /// The border of a rule of value `value` (0: none).
    fn line(&self, value: u8) -> Option<Border> {
        (value > 0).then_some(Border {
            width: if value >= HEAVY { self.heavy } else { self.light },
            space: 0.0,
            color: [0; 3],
            style: BorderStyle::Single,
        })
    }

    /// The rule above row `row` over the `count` columns from `col`: the
    /// heaviest of theirs.
    fn above(&self, row: usize, col: usize, count: usize) -> Option<Border> {
        let values = self.above.get(row)?;
        self.line(
            (col..col + count)
                .filter_map(|at| values.get(at).copied())
                .max()
                .unwrap_or(0),
        )
    }

    /// The borders (top, left, bottom, right) of a cell of row `row` over
    /// the `count` columns from `col` (`continued`: under a cell merged
    /// down, which draws no top). The rule under the header is the bottom
    /// of the last header row, so that it is repeated with it; the rule
    /// below the table is the bottom of the last row.
    fn borders(&self, row: usize, col: usize, count: usize, continued: bool) -> [Option<Border>; 4] {
        let top = if continued || (row > 0 && row == self.header_rows) {
            None
        } else {
            self.above(row, col, count)
        };
        let bottom = if row + 1 == self.rows {
            self.line(
                (col..col + count)
                    .filter_map(|at| self.bottom.get(at).copied())
                    .max()
                    .unwrap_or(0),
            )
        } else if self.header_rows > 0 && row + 1 == self.header_rows {
            self.above(self.header_rows, col, count)
        } else {
            None
        };
        let left = self
            .left
            .get(row)
            .and_then(|values| values.get(col))
            .copied()
            .unwrap_or(false);
        let right = col + count >= self.cols && self.right.get(row).copied().unwrap_or(false);
        [top, self.line(u8::from(left)), bottom, self.line(u8::from(right))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;

    /// The text of a cell's spans.
    fn text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn short_columns_get_what_they_need_first() {
        // Two columns lack little, one a lot: the first two are on one
        // line, the third takes the rest.
        let widths = share(&[30.0, 30.0, 400.0], &[20.0, 20.0, 50.0], 200.0);
        assert_eq!(&widths[..2], &[30.0, 30.0]);
        assert!((widths[2] - 140.0).abs() < 0.01);
    }

    #[test]
    fn groups_repeat_the_first_column() {
        let measured = Measured {
            size: 10.0,
            cells: Vec::new(),
            natural: vec![50.0; 5],
            least: vec![50.0; 5],
            spans: Vec::new(),
        };
        let config = WorkbookPdfConfig {
            cell_padding_em: 0.0,
            ..Default::default()
        };
        let groups = groups(&measured, 10.0, 160.0, &config);
        assert_eq!(groups, vec![vec![0, 1, 2], vec![0, 3, 4]]);
    }

    #[test]
    fn a_column_subset_keeps_spans_and_rules() {
        let cell = |col, colspan, text: &str| TableCell {
            col,
            colspan,
            rowspan: 1,
            spans: vec![Span::plain(text)],
        };
        let model = DocTable::ruled(
            vec![
                vec![cell(0, 1, "a"), cell(1, 2, "bc")],
                vec![cell(0, 1, "1"), cell(1, 1, "2"), cell(2, 1, "3")],
            ],
            3,
            1,
            vec!["left"; 3],
        );
        let part = columns_of(&model, &[0, 2]);
        assert_eq!(part.n_cols, 2);
        assert_eq!(text(&part.rows[0][1].spans), "bc");
        assert_eq!(part.rows[0][1].colspan, 1);
        assert_eq!(part.rule_above[0], vec![HEAVY, HEAVY]);
    }
}
