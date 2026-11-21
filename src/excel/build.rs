//! The blocks of a sheet: a heading with its name, then each region of
//! its cells as a captioned table (with its header rows) and the notes
//! under it, or as paragraphs when the region is a list of single values
//! (a form, a cover or notes sheet).

use super::ExcelConfig;
use super::grid::{CellKind, Grid, Merge};
use super::regions::{self, Visible, is_blank};
use crate::model::{Block, BlockKind, FloatKind, Span, Table, TableCell};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Between two lines of a caption. The renderers show a newline in a
/// caption as a space, so the lines are joined by a dash instead.
const CAPTION_SEPARATOR: &str = " — ";

/// The rows of a region, by index on the sheet, as they are read: the
/// caption, the header rows and the body of its table, and the notes under
/// it. A region read as paragraphs (no row of two cells or more, or a
/// body too sparse for a table) has all its rows in `body`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Parts {
    /// The leading rows with one text cell above the caption: paragraphs
    /// before the table.
    pub(crate) lead: Vec<u32>,
    /// The last leading rows with one text cell (at most
    /// `max_caption_rows`): a title, a unit line.
    pub(crate) caption: Vec<u32>,
    /// The header rows of the table.
    pub(crate) header: Vec<u32>,
    /// The body rows of the table.
    pub(crate) body: Vec<u32>,
    /// The trailing rows with one text cell: `Source:`, `注:`.
    pub(crate) notes: Vec<u32>,
    /// The region is text laid out in cells, not a table: all its rows are
    /// in `body`, each to be one paragraph.
    pub(crate) paragraphs: bool,
}

/// The blocks of the sheet `name`: a level-1 heading, then its regions in
/// reading order. An empty sheet has none. The blocks are numbered from 0;
/// the caller numbers them in the whole document.
pub(crate) fn sheet_blocks(name: &str, grid: &Grid, config: &ExcelConfig) -> Vec<Block> {
    let mut blocks = Vec::new();
    for region in regions::split(grid, config) {
        let cells = RegionCells::new(grid, &region);
        if cells.cells.is_empty() {
            continue;
        }
        let parts = parts_of(grid, &region, &cells, config);
        if parts.paragraphs {
            blocks.extend(parts.body.iter().map(|&row| paragraph(cells.row_text(row))));
            continue;
        }
        blocks.extend(parts.lead.iter().map(|&row| paragraph(cells.row_text(row))));
        let mut figure = Block::new(
            0,
            BlockKind::Figure {
                kind: FloatKind::Table,
                image: None,
                label: None,
            },
        );
        figure.caption_above = true;
        for &row in &parts.caption {
            if !figure.spans.is_empty() {
                figure.spans.push(Span::plain(CAPTION_SEPARATOR));
            }
            figure
                .spans
                .extend(cells.row(row).flat_map(|(_, cell)| cell.spans.iter().cloned()));
        }
        figure.table = Some(table(&cells, &parts, config));
        blocks.push(figure);
        blocks.extend(parts.notes.iter().map(|&row| paragraph(cells.row_text(row))));
    }
    if grid.truncated_rows > 0 {
        let rows = grid.truncated_rows;
        let text = if rows == 1 {
            "1 more row is not shown.".to_string()
        } else {
            format!("{rows} more rows are not shown.")
        };
        blocks.push(paragraph(vec![Span::plain(text)]));
    }
    if blocks.is_empty() {
        return blocks;
    }
    let mut heading = Block::new(0, BlockKind::Heading { level: 1 });
    heading.spans = vec![Span::plain(name)];
    blocks.insert(0, heading);
    for (id, block) in blocks.iter_mut().enumerate() {
        block.id = id;
    }
    blocks
}

/// The rows of `region`, read into caption, header, body and notes.
#[cfg(test)]
pub(crate) fn parts(grid: &Grid, region: &Merge, config: &ExcelConfig) -> Parts {
    parts_of(grid, region, &RegionCells::new(grid, region), config)
}

/// [`parts`], with the cells of the region already read.
fn parts_of(grid: &Grid, region: &Merge, cells: &RegionCells<'_>, config: &ExcelConfig) -> Parts {
    let rows = cells.rows.clone();
    let text = |rows: Vec<u32>| Parts {
        body: rows,
        paragraphs: true,
        ..Parts::default()
    };
    if cells.is_paragraphs() {
        return text(rows);
    }
    let single = |row: &u32| {
        let mut row = cells.row(*row);
        matches!((row.next(), row.next()), (Some((_, cell)), None) if cell.kind == CellKind::Text)
    };
    let leading = rows.iter().take_while(|row| single(row)).count();
    let lead = leading - leading.min(config.max_caption_rows);
    let notes = rows[leading..].iter().rev().take_while(|row| single(row)).count();
    let table = &rows[leading..rows.len() - notes];
    let header = header_rows(grid, region, cells, table, config);
    let body = &table[header..];
    if fill(grid, region, cells, body) < config.table_min_fill_ratio {
        return text(rows);
    }
    Parts {
        lead: rows[..lead].to_vec(),
        caption: rows[lead..leading].to_vec(),
        header: table[..header].to_vec(),
        body: body.to_vec(),
        notes: rows[rows.len() - notes..].to_vec(),
        paragraphs: false,
    }
}

/// The share of the positions of the body rows `body` (from the first to
/// the last, by the columns of `region`, hidden ones left out) that a cell
/// covers: a merge or a centred text covers its whole area. 1 without a
/// body.
fn fill(grid: &Grid, region: &Merge, cells: &RegionCells<'_>, body: &[u32]) -> f32 {
    let (Some(&first), Some(&last)) = (body.first(), body.last()) else {
        return 1.0;
    };
    let rows = Visible::new(&grid.hidden_rows);
    let cols = Visible::new(&grid.hidden_cols);
    let positions = rows.count(first, last) * cols.count(region.left, region.right);
    if positions == 0 {
        return 1.0;
    }
    let covered: u64 = cells
        .cells
        .iter()
        .filter(|&(&(row, _), cell)| row <= last && cell.bottom >= first)
        .map(|(&(row, col), cell)| rows.count(row.max(first), cell.bottom.min(last)) * cols.count(col, cell.right))
        .sum();
    covered as f32 / positions as f32
}

/// How many of the rows `table` (those after the caption rows) are the
/// header, the first rule that applies deciding: the number the user gives;
/// what the file declares; the types of the columns; else one. Merges
/// reaching into the header extend it. A table of one row has none, and
/// any other keeps at least one body row.
fn header_rows(grid: &Grid, region: &Merge, cells: &RegionCells<'_>, table: &[u32], config: &ExcelConfig) -> usize {
    if table.len() <= 1 {
        return 0;
    }
    // The header of a table leaves it a body row.
    let most = table.len() - 1;
    if let Some(rows) = config.header_rows {
        return rows.min(most);
    }
    let found = declared_header(grid, region, table)
        .map(|rows| rows.min(most))
        .or_else(|| header_by_types(cells, table, config))
        .unwrap_or(1);
    if found == 0 {
        return 0;
    }
    // A merge reaching from the header into the rows below takes them in,
    // unless it reaches the last row: the table keeps a body.
    let mut header = found;
    loop {
        let reach = table[..header]
            .iter()
            .flat_map(|&row| cells.row(row))
            .map(|(_, cell)| table.partition_point(|&row| row <= cell.bottom))
            .max()
            .unwrap_or(header);
        if reach <= header || reach >= table.len() {
            return header;
        }
        header = reach;
    }
}

/// The header rows that the file declares for the rows `table` of
/// `region`, the first declaration that covers the region deciding: an
/// Excel Table (`headerRowCount`), the autofilter (its first row), the
/// frozen rows, the print titles. A declaration whose last header row is
/// above the table (in the caption rows, or above the region) says
/// nothing.
fn declared_header(grid: &Grid, region: &Merge, table: &[u32]) -> Option<usize> {
    let declared = &grid.declared;
    let covers_rows = |first: u32, last: u32| first <= region.bottom && region.top <= last;
    let mut last_rows: Vec<u32> = Vec::new();
    for (area, header_rows) in &declared.tables {
        if area.overlaps(region) {
            // A table declared without a header row has none.
            if *header_rows == 0 {
                return Some(0);
            }
            last_rows.push(area.top.saturating_add(header_rows - 1));
        }
    }
    if let Some(area) = declared.auto_filter.filter(|area| area.overlaps(region)) {
        last_rows.push(area.top);
    }
    if declared.frozen_rows > 0 && covers_rows(0, declared.frozen_rows - 1) {
        last_rows.push(declared.frozen_rows - 1);
    }
    if let Some((_, last)) = declared
        .print_title_rows
        .filter(|&(first, last)| covers_rows(first, last))
    {
        last_rows.push(last);
    }
    let first = *table.first()?;
    last_rows
        .into_iter()
        .find(|&last| last >= first)
        .map(|last| table.partition_point(|&row| row <= last))
}

/// The header rows found from the types of the columns: the first row
/// whose cells have, in at least `header_type_match_ratio` of them, the
/// most common type of their column in the next `header_lookahead_rows`
/// rows is the first body row. `None` for a table of text only, or when no
/// row is found.
fn header_by_types(cells: &RegionCells<'_>, table: &[u32], config: &ExcelConfig) -> Option<usize> {
    let text_only = table
        .iter()
        .flat_map(|&row| cells.row(row))
        .all(|(_, cell)| cell.kind == CellKind::Text);
    if text_only {
        return None;
    }
    let lookahead = config.header_lookahead_rows.max(1);
    // The rows that the rule looks at: those it tries, and those below them.
    let rows: Vec<Vec<(u32, CellKind)>> = table
        .iter()
        .take(lookahead.saturating_mul(2))
        .map(|&row| cells.row(row).map(|(col, cell)| (col, cell.kind)).collect())
        .collect();
    (0..rows.len().saturating_sub(1).min(lookahead)).find(|&index| {
        let below = &rows[index + 1..rows.len().min(index + 1 + lookahead)];
        let mut judged = 0usize;
        let mut matching = 0usize;
        for &(col, kind) in &rows[index] {
            if let Some(common) = most_common(
                below
                    .iter()
                    .flatten()
                    .filter(|(other, _)| *other == col)
                    .map(|&(_, kind)| kind),
            ) {
                judged += 1;
                matching += usize::from(kind == common);
            }
        }
        judged > 0 && matching as f32 >= config.header_type_match_ratio * judged as f32
    })
}

/// The most common of `kinds`; of two as common, the first in the order of
/// [`KINDS`].
fn most_common(kinds: impl Iterator<Item = CellKind>) -> Option<CellKind> {
    let mut counts = [0usize; KINDS.len()];
    for kind in kinds {
        if let Some(index) = KINDS.iter().position(|&other| other == kind) {
            counts[index] += 1;
        }
    }
    let (index, &count) = counts.iter().enumerate().rev().max_by_key(|&(_, count)| count)?;
    (count > 0).then_some(KINDS[index])
}

/// The kinds of value, in the order that breaks a tie between two as
/// common.
const KINDS: [CellKind; 5] = [
    CellKind::Number,
    CellKind::Date,
    CellKind::Text,
    CellKind::Bool,
    CellKind::Error,
];

/// The table of the header and body rows of `parts`: the columns with a
/// cell, merges as spans, numeric columns right-aligned.
fn table(cells: &RegionCells<'_>, parts: &Parts, config: &ExcelConfig) -> Table {
    let rows: Vec<u32> = parts.header.iter().chain(&parts.body).copied().collect();
    let mut cols: Vec<u32> = rows
        .iter()
        .flat_map(|&row| cells.row(row).map(|(col, _)| col))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    cols.sort_unstable();
    let n_cols = cols.len().max(1);
    // The number of the `kept` (sorted) indices from `first` to `last`.
    let count = |kept: &[u32], first: u32, last: u32| {
        (kept.partition_point(|&index| index <= last) - kept.partition_point(|&index| index < first)).max(1)
    };
    let mut numeric = vec![(0usize, 0usize); n_cols];
    let built: Vec<Vec<TableCell>> = rows
        .iter()
        .enumerate()
        .map(|(index, &row)| {
            cells
                .row(row)
                .map(|(col, cell)| {
                    let at = cols.partition_point(|&other| other < col);
                    if index >= parts.header.len() {
                        let (numbers, all) = &mut numeric[at];
                        *all += 1;
                        *numbers += usize::from(matches!(cell.kind, CellKind::Number | CellKind::Date));
                    }
                    TableCell {
                        col: at,
                        colspan: count(&cols, col, cell.right),
                        rowspan: count(&rows, row, cell.bottom),
                        spans: cell.spans.to_vec(),
                    }
                })
                .collect()
        })
        .collect();
    let col_align = numeric
        .iter()
        .map(|&(numbers, all)| {
            if all > 0 && numbers as f32 > config.numeric_column_ratio * all as f32 {
                "right"
            } else {
                "left"
            }
        })
        .collect();
    Table::ruled(built, n_cols, parts.header.len(), col_align)
}

/// A paragraph of the text `spans`.
fn paragraph(spans: Vec<Span>) -> Block {
    let mut block = Block::new(0, BlockKind::Paragraph { in_abstract: false });
    block.spans = spans;
    block
}

/// One cell of a region as it is shown: a merge or a text centred across
/// the empty cells to its right is one cell over all of them.
struct Cell<'g> {
    /// The last row the cell covers.
    bottom: u32,
    /// The last column the cell covers.
    right: u32,
    /// Its text.
    spans: &'g [Span],
    /// Its kind of value.
    kind: CellKind,
}

/// The visible cells of a region, without the empty ones and those a merge
/// covers.
struct RegionCells<'g> {
    /// The cells by (row, column) of their top-left corner.
    cells: BTreeMap<(u32, u32), Cell<'g>>,
    /// The rows with a cell, in order.
    rows: Vec<u32>,
}

impl<'g> RegionCells<'g> {
    fn new(grid: &'g Grid, region: &Merge) -> Self {
        let visible_col = |col: u32| !grid.hidden_cols.contains(&col);
        let mut merged: HashMap<(u32, u32), Merge> = HashMap::new();
        let mut covered: HashSet<(u32, u32)> = HashSet::new();
        for merge in grid.merges.iter().filter(|merge| merge.overlaps(region)) {
            let clipped = Merge {
                top: merge.top.max(region.top),
                left: merge.left.max(region.left),
                bottom: merge.bottom.min(region.bottom),
                right: merge.right.min(region.right),
            };
            merged.insert((merge.top, merge.left), clipped);
            for row in clipped.top..=clipped.bottom {
                covered.extend(
                    grid.cells
                        .range((row, clipped.left)..=(row, clipped.right))
                        .map(|(&place, _)| place)
                        .filter(|&place| place != (merge.top, merge.left)),
                );
            }
        }
        // The next visible column after `col`.
        let next_col = |col: u32| (col + 1..=region.right).find(|&col| visible_col(col));
        let mut cells: BTreeMap<(u32, u32), Cell<'g>> = BTreeMap::new();
        for row in (region.top..=region.bottom).filter(|row| !grid.hidden_rows.contains(row)) {
            // The cell whose text is centred across the cells after it.
            let mut centred: Option<(u32, u32)> = None;
            for (&(_, col), cell) in grid.cells.range((row, region.left)..=(row, region.right)) {
                if !visible_col(col) || covered.contains(&(row, col)) {
                    continue;
                }
                if is_blank(&cell.spans) {
                    match centred
                        .filter(|_| cell.center_continuous)
                        .and_then(|place| cells.get_mut(&place))
                    {
                        Some(open) if next_col(open.right) == Some(col) => open.right = col,
                        _ => centred = None,
                    }
                    continue;
                }
                let merge = merged.get(&(row, col));
                cells.insert(
                    (row, col),
                    Cell {
                        bottom: merge.map_or(row, |merge| merge.bottom),
                        right: merge.map_or(col, |merge| merge.right),
                        spans: &cell.spans,
                        kind: cell.kind,
                    },
                );
                centred = (cell.center_continuous && merge.is_none()).then_some((row, col));
            }
        }
        let mut rows: Vec<u32> = cells.keys().map(|&(row, _)| row).collect();
        rows.dedup();
        Self { cells, rows }
    }

    /// The cells of `row` by column, left to right.
    fn row(&self, row: u32) -> impl Iterator<Item = (u32, &Cell<'g>)> {
        self.cells
            .range((row, 0)..=(row, u32::MAX))
            .map(|(&(_, col), cell)| (col, cell))
    }

    /// The text of the cells of `row`, left to right, joined by a space.
    fn row_text(&self, row: u32) -> Vec<Span> {
        let mut spans = Vec::new();
        for (_, cell) in self.row(row) {
            if !spans.is_empty() {
                spans.push(Span::plain(" "));
            }
            spans.extend(cell.spans.iter().cloned());
        }
        spans
    }

    /// Whether the region is a list of single values: no row has two
    /// cells.
    fn is_paragraphs(&self) -> bool {
        self.rows.iter().all(|&row| self.row(row).nth(1).is_none())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::excel::grid::GridCell;
    use crate::model::{HEAVY_RULE, THIN_RULE};

    /// A grid of the rows `rows`: an empty string is no cell, a number is a
    /// number cell, anything else text.
    fn grid(rows: &[&[&str]]) -> Grid {
        let mut grid = Grid::default();
        for (row, cells) in rows.iter().enumerate() {
            for (col, text) in cells.iter().enumerate() {
                if text.is_empty() {
                    continue;
                }
                let kind = if text.parse::<f64>().is_ok() {
                    CellKind::Number
                } else {
                    CellKind::Text
                };
                let place = (u32::try_from(row).unwrap(), u32::try_from(col).unwrap());
                grid.cells.insert(
                    place,
                    GridCell {
                        spans: vec![Span::plain(*text)],
                        kind,
                        center_continuous: false,
                    },
                );
            }
        }
        grid
    }

    fn area(top: u32, left: u32, bottom: u32, right: u32) -> Merge {
        Merge {
            top,
            left,
            bottom,
            right,
        }
    }

    /// The parts of the only region of `grid`.
    fn only_parts(grid: &Grid, config: &ExcelConfig) -> Parts {
        let regions = regions::split(grid, config);
        assert_eq!(regions.len(), 1, "{regions:?}");
        parts(grid, &regions[0], config)
    }

    /// The table of the only table block of `blocks`.
    fn only_table(blocks: &[Block]) -> &Table {
        let tables: Vec<&Table> = blocks.iter().filter_map(|block| block.table.as_ref()).collect();
        assert_eq!(tables.len(), 1);
        tables[0]
    }

    /// The text of the cells of each row of `table`.
    fn texts(table: &Table) -> Vec<Vec<String>> {
        table
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| cell.spans.iter().map(|span| span.text.as_str()).collect())
                    .collect()
            })
            .collect()
    }

    /// Three text header rows over a numeric body, as in the statistics
    /// tables of Japanese cities.
    const TWO_LEVEL: &[&[&str]] = &[
        &["Area", "Population", "Men", "Women"],
        &["Name", "Total", "of which", "of which"],
        &["", "persons", "persons", "persons"],
        &["North", "10", "4", "6"],
        &["South", "12", "5", "7"],
        &["East", "8", "3", "5"],
        &["West", "9", "4", "5"],
    ];

    #[test]
    fn title_and_unit_rows_become_the_caption() {
        let grid = grid(&[
            &["Table 1: Households"],
            &["(in thousands)"],
            &["Region", "Count"],
            &["North", "5"],
            &["South", "7"],
        ]);
        let config = ExcelConfig::default();
        let parts = only_parts(&grid, &config);
        assert_eq!(
            parts,
            Parts {
                lead: vec![],
                caption: vec![0, 1],
                header: vec![2],
                body: vec![3, 4],
                notes: vec![],
                paragraphs: false,
            }
        );
        let blocks = sheet_blocks("Data", &grid, &config);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text(), "Data");
        assert!(matches!(blocks[0].kind, BlockKind::Heading { level: 1 }));
        assert!(blocks[1].caption_above);
        assert_eq!(blocks[1].text(), "Table 1: Households — (in thousands)");
        assert_eq!(only_table(&blocks).rows.len(), 3);
        let ids: Vec<usize> = blocks.iter().map(|block| block.id).collect();
        assert_eq!(ids, [0, 1]);
    }

    #[test]
    fn source_line_becomes_a_note() {
        let grid = grid(&[
            &["Region", "Count"],
            &["North", "5"],
            &["South", "7"],
            &[],
            &["Source: census"],
        ]);
        let config = ExcelConfig::default();
        let parts = only_parts(&grid, &config);
        assert_eq!(parts.notes, [4]);
        assert_eq!(parts.body, [1, 2]);
        let blocks = sheet_blocks("Data", &grid, &config);
        assert_eq!(blocks.len(), 3);
        assert!(blocks[1].spans.is_empty());
        assert!(matches!(blocks[2].kind, BlockKind::Paragraph { .. }));
        assert_eq!(blocks[2].text(), "Source: census");
    }

    #[test]
    fn two_level_header_found_by_types() {
        let parts = only_parts(&grid(TWO_LEVEL), &ExcelConfig::default());
        assert_eq!(parts.header, [0, 1, 2]);
        assert_eq!(parts.body, [3, 4, 5, 6]);
    }

    #[test]
    fn declaration_wins_over_types() {
        let mut grid = grid(&[&["Region", "2021", "2022"], &["North", "1", "2"], &["South", "3", "4"]]);
        let config = ExcelConfig::default();
        // The years have the type of the numbers under them.
        assert!(only_parts(&grid, &config).header.is_empty());
        grid.declared.frozen_rows = 1;
        assert_eq!(only_parts(&grid, &config).header, [0]);
        // Print titles, an autofilter and an Excel Table say the same.
        grid.declared.frozen_rows = 0;
        grid.declared.print_title_rows = Some((0, 1));
        assert_eq!(only_parts(&grid, &config).header, [0, 1]);
        grid.declared.print_title_rows = None;
        grid.declared.auto_filter = Some(area(0, 0, 2, 2));
        assert_eq!(only_parts(&grid, &config).header, [0]);
        grid.declared.auto_filter = None;
        grid.declared.tables.push((area(0, 0, 2, 2), 2));
        assert_eq!(only_parts(&grid, &config).header, [0, 1]);
    }

    #[test]
    fn declaration_elsewhere_says_nothing() {
        // The frozen row is the title, above the table: the types decide.
        let mut rows: Vec<&[&str]> = vec![&["Title"]];
        rows.extend_from_slice(TWO_LEVEL);
        let mut grid = grid(&rows);
        grid.declared.frozen_rows = 1;
        let parts = only_parts(&grid, &ExcelConfig::default());
        assert_eq!(parts.caption, [0]);
        assert_eq!(parts.header, [1, 2, 3]);
        // An autofilter of another region of the sheet neither.
        grid.declared.frozen_rows = 0;
        grid.declared.auto_filter = Some(area(20, 0, 30, 3));
        assert_eq!(only_parts(&grid, &ExcelConfig::default()).header, [1, 2, 3]);
    }

    #[test]
    fn all_text_table_without_declaration_takes_the_first_row() {
        let grid = grid(&[&["Name", "City"], &["Ann", "Oslo"], &["Bob", "Rome"]]);
        assert_eq!(only_parts(&grid, &ExcelConfig::default()).header, [0]);
    }

    #[test]
    fn user_header_rows_wins() {
        let config = ExcelConfig {
            header_rows: Some(2),
            ..ExcelConfig::default()
        };
        let mut grid = grid(TWO_LEVEL);
        grid.declared.frozen_rows = 1;
        assert_eq!(only_parts(&grid, &config).header, [0, 1]);
    }

    #[test]
    fn one_row_region_has_no_header() {
        let grid = grid(&[&["Name", "City"]]);
        let parts = only_parts(&grid, &ExcelConfig::default());
        assert!(parts.header.is_empty());
        assert_eq!(parts.body, [0]);
    }

    #[test]
    fn one_column_region_is_paragraphs() {
        let grid = grid(&[&["Application form"], &["Name"], &["", ""], &["2024"]]);
        let blocks = sheet_blocks("Form", &grid, &ExcelConfig::default());
        let texts: Vec<String> = blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Form", "Application form", "Name", "2024"]);
        assert!(
            blocks[1..]
                .iter()
                .all(|block| matches!(block.kind, BlockKind::Paragraph { .. }))
        );
    }

    #[test]
    fn merged_cells_get_spans() {
        let mut grid = grid(&[
            &["Region", "Sales", ""],
            &["", "2021", "2022"],
            &["North", "1", "2"],
            &["South", "3", "4"],
        ]);
        grid.merges = vec![area(0, 0, 1, 0), area(0, 1, 0, 2)];
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        let table = only_table(&blocks);
        // The years have the type of the body, but the merge of "Region"
        // takes their row into the header.
        assert_eq!(table.header_rows, 2);
        assert_eq!(table.n_cols, 3);
        let first: Vec<_> = table.rows[0]
            .iter()
            .map(|cell| (cell.col, cell.colspan, cell.rowspan))
            .collect();
        assert_eq!(first, [(0, 1, 2), (1, 2, 1)]);
        let second: Vec<_> = table.rows[1].iter().map(|cell| cell.col).collect();
        assert_eq!(second, [1, 2]);
        assert_eq!(table.rule_above[0], [HEAVY_RULE; 3]);
        assert_eq!(table.rule_above[2], [THIN_RULE; 3]);
        assert_eq!(table.rule_bottom, [HEAVY_RULE; 3]);
    }

    #[test]
    fn a_merge_down_the_whole_table_keeps_a_body() {
        // "Group" is merged down to the last row: taking all the rows into
        // the header would leave no body.
        let mut grid = grid(&[&["Group", "Name", "Count"], &["", "North", "5"], &["", "South", "7"]]);
        grid.merges = vec![area(0, 0, 2, 0)];
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        assert_eq!(only_table(&blocks).header_rows, 1);
    }

    #[test]
    fn declared_header_keeps_a_body() {
        let mut grid = grid(&[&["Name", "City"], &["Ann", "Oslo"]]);
        grid.declared.frozen_rows = 5;
        let parts = only_parts(&grid, &ExcelConfig::default());
        assert_eq!((parts.header, parts.body), (vec![0], vec![1]));
    }

    #[test]
    fn user_header_rows_keep_a_body() {
        let config = ExcelConfig {
            header_rows: Some(5),
            ..ExcelConfig::default()
        };
        let grid = grid(&[&["Name", "City"], &["Ann", "Oslo"]]);
        let parts = only_parts(&grid, &config);
        assert_eq!((parts.header, parts.body), (vec![0], vec![1]));
    }

    #[test]
    fn damaged_header_row_count_does_not_overflow() {
        let mut grid = grid(&[&[], &[], &["Name", "City"], &["Ann", "Oslo"], &["Bob", "Rome"]]);
        grid.declared.tables.push((area(2, 0, 4, 1), u32::MAX));
        let parts = only_parts(&grid, &ExcelConfig::default());
        assert_eq!(parts.body, [4]);
    }

    #[test]
    fn only_the_last_single_rows_are_the_caption() {
        let grid = grid(&[
            &["Read this first."],
            &["Then this."],
            &["Table 1"],
            &["(units)"],
            &["Region", "Count"],
            &["North", "5"],
            &["South", "7"],
        ]);
        let config = ExcelConfig {
            max_caption_rows: 2,
            ..ExcelConfig::default()
        };
        let parts = only_parts(&grid, &config);
        assert_eq!(parts.lead, [0, 1]);
        assert_eq!(parts.caption, [2, 3]);
        let blocks = sheet_blocks("Data", &grid, &config);
        let texts: Vec<String> = blocks.iter().map(Block::text).collect();
        assert_eq!(
            texts[..4],
            ["Data", "Read this first.", "Then this.", "Table 1 — (units)"]
        );
        assert!(matches!(blocks[1].kind, BlockKind::Paragraph { .. }));
        assert!(blocks[3].table.is_some());
    }

    #[test]
    fn sparse_region_is_paragraphs() {
        // A form: labels and values scattered over a wide grid, 6 of its
        // 15 places filled.
        let grid = grid(&[
            &["Name", "Ann", "", "", ""],
            &["", "", "Address", "Oslo", ""],
            &["Date", "", "", "", "2024"],
        ]);
        let config = ExcelConfig {
            table_min_fill_ratio: 0.5,
            ..ExcelConfig::default()
        };
        let blocks = sheet_blocks("Form", &grid, &config);
        let texts: Vec<String> = blocks.iter().map(Block::text).collect();
        assert_eq!(texts, ["Form", "Name Ann", "Address Oslo", "Date 2024"]);
        assert!(
            blocks[1..]
                .iter()
                .all(|block| matches!(block.kind, BlockKind::Paragraph { .. }))
        );
        // A full table stays a table.
        let full = super::tests::grid(&[&["Name", "City"], &["Ann", "Oslo"], &["Bob", "Rome"]]);
        assert_eq!(only_table(&sheet_blocks("Data", &full, &config)).rows.len(), 3);
    }

    #[test]
    fn a_merge_fills_its_area() {
        // The merged labels cover their rows: the body is full.
        let mut grid = grid(&[
            &["Group", "Name", "Count"],
            &["A", "North", "5"],
            &["", "South", "7"],
            &["", "East", "8"],
        ]);
        grid.merges = vec![area(1, 0, 3, 0)];
        let config = ExcelConfig {
            table_min_fill_ratio: 0.9,
            ..ExcelConfig::default()
        };
        assert!(sheet_blocks("Data", &grid, &config)[1].table.is_some());
    }

    #[test]
    fn centred_text_spans_the_empty_cells_after_it() {
        let mut grid = grid(&[
            &["Population", "", "", "Area"],
            &["Total", "Men", "Women", "km2"],
            &["10", "4", "6", "3"],
            &["12", "5", "7", "4"],
        ]);
        let centred = GridCell {
            spans: Vec::new(),
            kind: CellKind::Text,
            center_continuous: true,
        };
        grid.cells.get_mut(&(0, 0)).unwrap().center_continuous = true;
        grid.cells.insert((0, 1), centred.clone());
        grid.cells.insert((0, 2), centred);
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        let table = only_table(&blocks);
        let first: Vec<_> = table.rows[0].iter().map(|cell| (cell.col, cell.colspan)).collect();
        assert_eq!(first, [(0, 3), (3, 1)]);
        assert_eq!(table.header_rows, 2);
    }

    #[test]
    fn numeric_columns_are_right_aligned() {
        let grid = grid(&[
            &["Region", "Count", "Note"],
            &["North", "5", "a"],
            &["South", "7", "8"],
            &["East", "x", "b"],
        ]);
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        let table = only_table(&blocks);
        assert_eq!(table.col_align, ["left", "right", "left"]);
        assert_eq!(texts(table)[1], ["North", "5", "a"]);
    }

    #[test]
    fn hidden_rows_and_columns_are_left_out() {
        let mut grid = grid(&[
            &["Region", "Secret", "Count"],
            &["North", "x", "5"],
            &["Old", "y", "1"],
            &["South", "z", "7"],
        ]);
        grid.hidden_cols.insert(1);
        grid.hidden_rows.insert(2);
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        assert_eq!(
            texts(only_table(&blocks)),
            [["Region", "Count"], ["North", "5"], ["South", "7"]]
        );
    }

    #[test]
    fn empty_sheet_has_no_blocks_and_a_truncated_one_says_so() {
        assert!(sheet_blocks("Empty", &Grid::default(), &ExcelConfig::default()).is_empty());
        let mut grid = grid(&[&["Region", "Count"], &["North", "5"]]);
        grid.truncated_rows = 40;
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        assert_eq!(blocks.last().unwrap().text(), "40 more rows are not shown.");
    }

    #[test]
    fn side_by_side_tables_are_two_tables() {
        let grid = grid(&[
            &["A", "B", "", "C", "D"],
            &["1", "2", "", "3", "4"],
            &["5", "6", "", "7", "8"],
        ]);
        let blocks = sheet_blocks("Data", &grid, &ExcelConfig::default());
        let tables: Vec<_> = blocks
            .iter()
            .filter_map(|block| block.table.as_ref())
            .map(texts)
            .collect();
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[1][0], ["C", "D"]);
    }
}
