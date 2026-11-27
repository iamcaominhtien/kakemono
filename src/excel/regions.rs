//! The regions of a sheet: its blocks of cells, which empty rows and
//! columns separate. Each region becomes a table (or paragraphs) of its
//! own; side-by-side tables are two regions.

use super::ExcelConfig;
use super::grid::{Grid, Merge};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};

/// The most empty columns a text is taken to run over.
const MAX_OVERFLOW_COLS: u32 = 64;

/// The regions of `grid`, as bounding boxes sorted by (top, left).
///
/// Only visible cells count: two cells are in the same region when their
/// rows, counted without the hidden ones, differ by at most
/// `region_gap_rows` and their columns by at most `region_gap_cols`; a
/// merge connects its whole area, and a text that runs over the empty
/// cells to its right the cell it runs up to. Regions whose boxes overlap
/// are one
/// region.
pub(crate) fn split(grid: &Grid, config: &ExcelConfig) -> Vec<Merge> {
    let rows = Visible::new(&grid.hidden_rows);
    let cols = Visible::new(&grid.hidden_cols);
    let visible = |(row, col): (u32, u32)| !grid.hidden_rows.contains(&row) && !grid.hidden_cols.contains(&col);
    // The items: one per visible cell, with its box on the sheet.
    let mut boxes: Vec<Merge> = Vec::new();
    // The visible position of each item's cells: (row, column) → item.
    let mut at: HashMap<(u32, u32), usize> = HashMap::new();
    let mut sets = Sets::default();
    // The last cell of a text centred across the selection, in visible
    // places: a blank centred cell right after it is part of the text.
    let mut centred: Option<(u32, u32)> = None;
    // The texts that run over empty cells up to another cell of their
    // row: (item, that cell).
    let mut reaches: Vec<(usize, (u32, u32))> = Vec::new();
    for (&(row, col), cell) in &grid.cells {
        if !visible((row, col)) {
            continue;
        }
        let place = (rows.rank(row), cols.rank(col));
        if is_blank(&cell.spans) {
            let follows = centred.is_some_and(|(last_row, last_col)| last_row == place.0 && last_col + 1 == place.1);
            if !(cell.center_continuous && follows) {
                centred = None;
                continue;
            }
            centred = Some(place);
        } else {
            centred = cell.center_continuous.then_some(place);
        }
        let item = sets.add();
        boxes.push(Merge {
            top: row,
            left: col,
            bottom: row,
            right: col,
        });
        at.insert(place, item);
        // A text that runs over the empty cells to its right up to the
        // next cell of its row is next to that cell, as on the sheet.
        if let Some(reached) = runs_up_to(grid, (row, col), cell.overflow_pt) {
            reaches.push((item, (row, reached)));
        }
    }
    for (item, (row, col)) in reaches {
        if let Some(&other) = at.get(&(rows.rank(row), cols.rank(col))) {
            sets.union(item, other);
        }
    }
    for merge in &grid.merges {
        if !visible((merge.top, merge.left)) || !grid.cells.contains_key(&(merge.top, merge.left)) {
            continue;
        }
        let Some(&item) = at.get(&(rows.rank(merge.top), cols.rank(merge.left))) else {
            continue;
        };
        let area = &mut boxes[item];
        area.bottom = area.bottom.max(merge.bottom);
        area.right = area.right.max(merge.right);
        // The cells on the edge of the merge connect it to the cells
        // around it: those inside are covered by it.
        for (row, col) in edge(merge) {
            if !visible((row, col)) {
                continue;
            }
            let place = (rows.rank(row), cols.rank(col));
            match at.get(&place) {
                Some(&other) => sets.union(item, other),
                None => {
                    at.insert(place, item);
                }
            }
        }
    }
    let gap_rows = u32::try_from(config.region_gap_rows).unwrap_or(u32::MAX);
    let gap_cols = u32::try_from(config.region_gap_cols).unwrap_or(u32::MAX);
    connect_near(&mut sets, at, gap_rows, gap_cols);
    let mut regions: HashMap<usize, Merge> = HashMap::new();
    for (item, area) in boxes.iter().enumerate() {
        regions
            .entry(sets.find(item))
            .and_modify(|region| *region = bounding(region, area))
            .or_insert(*area);
    }
    let mut regions: Vec<Merge> = regions.into_values().collect();
    regions.sort_by_key(|region| (region.top, region.left));
    join_overlapping(regions)
}

/// The column of the cell of its row that the text of the cell at
/// `(row, col)`, `overflow_pt` wide, runs up to over the empty cells to its
/// right; `None` when it ends before one.
fn runs_up_to(grid: &Grid, (row, col): (u32, u32), overflow_pt: f32) -> Option<u32> {
    let mut room = overflow_pt - grid.sizes.col_pt(col);
    let mut next = col + 1;
    while room > 0.0 && next - col <= MAX_OVERFLOW_COLS {
        if grid.cells.contains_key(&(row, next)) {
            return Some(next);
        }
        if !grid.hidden_cols.contains(&next) {
            room -= grid.sizes.col_pt(next);
        }
        next += 1;
    }
    None
}

/// Puts together the items of the places `at` (visible row and column →
/// item) whose rows differ by at most `gap_rows` and columns by at most
/// `gap_cols`.
fn connect_near(sets: &mut Sets, at: HashMap<(u32, u32), usize>, gap_rows: u32, gap_cols: u32) {
    let mut places: Vec<((u32, u32), usize)> = at.into_iter().collect();
    places.sort_unstable();
    // The places of each row: (row, [(column, item)]), by row then column.
    let mut lines: Vec<(u32, Vec<(u32, usize)>)> = Vec::new();
    for ((row, col), item) in places {
        match lines.last_mut() {
            Some((last, line)) if *last == row => line.push((col, item)),
            _ => lines.push((row, vec![(col, item)])),
        }
    }
    for (index, (row, line)) in lines.iter().enumerate() {
        // In a row, two places near each other are next to each other.
        for pair in line.windows(2) {
            if pair[1].0 - pair[0].0 <= gap_cols {
                sets.union(pair[0].1, pair[1].1);
            }
        }
        for (_, other) in lines[index + 1..]
            .iter()
            .take_while(|(below, _)| below - row <= gap_rows)
        {
            // The places of `other` near each place of `line`: a window
            // that moves to the right with it.
            let mut start = 0;
            for &(col, item) in line {
                while start < other.len() && other[start].0.saturating_add(gap_cols) < col {
                    start += 1;
                }
                for &(_, other_item) in other[start..]
                    .iter()
                    .take_while(|(near, _)| *near <= col.saturating_add(gap_cols))
                {
                    sets.union(item, other_item);
                }
            }
        }
    }
}

/// Whether the text of `spans` is only white space.
pub(crate) fn is_blank(spans: &[crate::model::Span]) -> bool {
    spans.iter().all(|span| span.text.trim().is_empty())
}

/// The smallest range holding both `a` and `b`.
fn bounding(a: &Merge, b: &Merge) -> Merge {
    Merge {
        top: a.top.min(b.top),
        left: a.left.min(b.left),
        bottom: a.bottom.max(b.bottom),
        right: a.right.max(b.right),
    }
}

/// `regions` with every two that overlap made one, sorted by (top, left).
///
/// A pass sweeps the regions down the sheet, by top: the open ones, those
/// reaching down to the top of the next, all hold its first row, so two
/// open ones that overlap share columns, and open ones are kept apart by
/// joining them as they come, filed by their left column. A joined region
/// is taller and wider: one already closed may overlap it now, which the
/// next pass finds. A pass is near-linear, and there are few (two, when
/// nothing overlaps once joined).
fn join_overlapping(mut regions: Vec<Merge>) -> Vec<Merge> {
    loop {
        regions.sort_by_key(|region| (region.top, region.left));
        let count = regions.len();
        let mut closed: Vec<Merge> = Vec::with_capacity(count);
        // The open regions by left column; their columns do not overlap.
        let mut open: BTreeMap<u32, Merge> = BTreeMap::new();
        // The open regions by bottom row, lowest first; one joined since
        // it was filed here is no longer open as it was.
        let mut ends: BinaryHeap<Reverse<(u32, u32, u32, u32)>> = BinaryHeap::new();
        for region in regions {
            close_ended(&mut open, &mut ends, &mut closed, region.top);
            let region = absorb_overlapping(&mut open, region);
            open.insert(region.left, region);
            ends.push(Reverse((region.bottom, region.left, region.top, region.right)));
        }
        closed.extend(open.into_values());
        if closed.len() == count {
            closed.sort_by_key(|region| (region.top, region.left));
            return closed;
        }
        regions = closed;
    }
}

/// Moves to `closed` the regions of `open` that end above row `top`
/// (filed in `ends` by bottom row; one joined since it was filed is no
/// longer open as it was, and is left).
fn close_ended(
    open: &mut BTreeMap<u32, Merge>,
    ends: &mut BinaryHeap<Reverse<(u32, u32, u32, u32)>>,
    closed: &mut Vec<Merge>,
    top: u32,
) {
    while let Some(&Reverse((bottom, left, filed_top, right))) = ends.peek() {
        if bottom >= top {
            break;
        }
        ends.pop();
        let filed = Merge {
            top: filed_top,
            left,
            bottom,
            right,
        };
        if open.get(&left) == Some(&filed) {
            open.remove(&left);
            closed.push(filed);
        }
    }
}

/// `region` joined with the regions of `open` that share its columns
/// (taken out of `open`), until none does.
fn absorb_overlapping(open: &mut BTreeMap<u32, Merge>, mut region: Merge) -> Merge {
    loop {
        let hits: Vec<u32> = open
            .range(..=region.right)
            .rev()
            .take_while(|(_, other)| other.right >= region.left)
            .map(|(&left, _)| left)
            .collect();
        if hits.is_empty() {
            return region;
        }
        for left in hits {
            if let Some(other) = open.remove(&left) {
                region = bounding(&region, &other);
            }
        }
    }
}

/// The cells on the edge of `merge`.
fn edge(merge: &Merge) -> impl Iterator<Item = (u32, u32)> + '_ {
    let rows = (merge.top..=merge.bottom).flat_map(move |row| {
        let cols: Vec<u32> = if row == merge.top || row == merge.bottom {
            (merge.left..=merge.right).collect()
        } else {
            vec![merge.left, merge.right]
        };
        cols.into_iter().map(move |col| (row, col))
    });
    rows.collect::<HashSet<_>>().into_iter()
}

/// The rows (or columns) of a sheet without its hidden ones: the place of
/// each among the visible ones.
pub(crate) struct Visible {
    /// The hidden ones, in order.
    hidden: Vec<u32>,
}

impl Visible {
    pub(crate) fn new(hidden: &HashSet<u32>) -> Self {
        let mut hidden: Vec<u32> = hidden.iter().copied().collect();
        hidden.sort_unstable();
        Self { hidden }
    }

    /// The place of `index` among the visible ones (of a hidden one: that
    /// of the next visible one).
    fn rank(&self, index: u32) -> u32 {
        let before = self.hidden.partition_point(|&hidden| hidden < index);
        index - u32::try_from(before).unwrap_or(index)
    }

    /// How many of `first` to `last` (inclusive) are visible; 0 when
    /// `last` is before `first`.
    pub(crate) fn count(&self, first: u32, last: u32) -> u64 {
        if last < first {
            return 0;
        }
        let hidden = self.hidden.partition_point(|&hidden| hidden <= last)
            - self.hidden.partition_point(|&hidden| hidden < first);
        (u64::from(last) - u64::from(first) + 1).saturating_sub(u64::try_from(hidden).unwrap_or(u64::MAX))
    }
}

/// Disjoint sets of items (union-find).
#[derive(Default)]
struct Sets {
    /// The parent of each item; a root is its own parent.
    parent: Vec<usize>,
}

impl Sets {
    /// A new item, alone in its set.
    fn add(&mut self) -> usize {
        self.parent.push(self.parent.len());
        self.parent.len() - 1
    }

    /// The root of the set of `item`.
    fn find(&mut self, mut item: usize) -> usize {
        while self.parent[item] != item {
            self.parent[item] = self.parent[self.parent[item]];
            item = self.parent[item];
        }
        item
    }

    /// Puts the sets of `a` and `b` together.
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[a.max(b)] = a.min(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::excel::grid::{CellKind, DEFAULT_COL_PT, GridCell};
    use crate::model::Span;

    /// A grid with a text cell at each of `cells`.
    fn grid(cells: &[(u32, u32)]) -> Grid {
        let mut grid = Grid::default();
        for &place in cells {
            grid.cells.insert(
                place,
                GridCell {
                    spans: vec![Span::plain("x")],
                    kind: CellKind::Text,
                    center_continuous: false,
                    own_font: Vec::new(),
                    overflow_pt: 0.0,
                },
            );
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

    #[test]
    fn side_by_side_tables_are_two_regions() {
        // Columns A-B and D-E, with column C empty between them.
        let cells: Vec<_> = (0..3)
            .flat_map(|row| [(row, 0), (row, 1), (row, 3), (row, 4)])
            .collect();
        let regions = split(&grid(&cells), &ExcelConfig::default());
        assert_eq!(regions, [area(0, 0, 2, 1), area(0, 3, 2, 4)]);
    }

    #[test]
    fn three_empty_rows_split_and_one_does_not() {
        // Rows 0-1, one empty row, rows 3-4, three empty rows, rows 8-9.
        let cells: Vec<_> = [0, 1, 3, 4, 8, 9]
            .iter()
            .flat_map(|&row| [(row, 0), (row, 1)])
            .collect();
        let regions = split(&grid(&cells), &ExcelConfig::default());
        assert_eq!(regions, [area(0, 0, 4, 1), area(8, 0, 9, 1)]);
    }

    #[test]
    fn a_merge_connects_its_area() {
        // A merge over A1:D1 joins the cells below A and D, which an empty
        // column would part.
        let mut grid = grid(&[(0, 0), (1, 0), (1, 3)]);
        grid.merges.push(area(0, 0, 0, 3));
        assert_eq!(split(&grid, &ExcelConfig::default()), [area(0, 0, 1, 3)]);
    }

    #[test]
    fn a_long_text_reaches_the_cell_it_runs_up_to() {
        // A long text in A runs over the empty B and C up to D: the rows
        // are one table, as on the sheet.
        let cells: Vec<_> = (0..3).flat_map(|row| [(row, 0), (row, 3), (row, 4)]).collect();
        let mut grid = grid(&cells);
        for row in 0..3 {
            grid.cells.get_mut(&(row, 0)).unwrap().overflow_pt = 3.5 * DEFAULT_COL_PT;
        }
        assert_eq!(split(&grid, &ExcelConfig::default()), [area(0, 0, 2, 4)]);
        // A short one stays apart.
        let grid = super::tests::grid(&cells);
        assert_eq!(split(&grid, &ExcelConfig::default()).len(), 2);
    }

    #[test]
    fn hidden_rows_do_not_part_regions() {
        let mut grid = grid(&[(0, 0), (5, 0)]);
        grid.hidden_rows.extend(1..5);
        assert_eq!(split(&grid, &ExcelConfig::default()), [area(0, 0, 5, 0)]);
        // A cell in a hidden column is left out.
        let mut grid = super::tests::grid(&[(0, 0), (0, 4)]);
        grid.hidden_cols.insert(4);
        assert_eq!(split(&grid, &ExcelConfig::default()), [area(0, 0, 0, 0)]);
    }

    #[test]
    fn a_large_sheet_is_one_region_and_huge_gaps_do_not_overflow() {
        let cells: Vec<_> = (0..20_000).flat_map(|row| (0..20).map(move |col| (row, col))).collect();
        let grid = grid(&cells);
        assert_eq!(split(&grid, &ExcelConfig::default()), [area(0, 0, 19_999, 19)]);
        let config = ExcelConfig {
            region_gap_rows: usize::MAX,
            region_gap_cols: usize::MAX,
            ..ExcelConfig::default()
        };
        let grid = super::tests::grid(&[(0, 0), (9, 900)]);
        assert_eq!(split(&grid, &config), [area(0, 0, 9, 900)]);
    }

    #[test]
    fn exactly_the_gap_splits() {
        let config = ExcelConfig::default();
        assert_eq!(config.region_gap_rows, 2);
        assert_eq!(config.region_gap_cols, 1);
        // Two empty rows (1 and 2) split; one empty row does not.
        assert_eq!(
            split(&grid(&[(0, 0), (3, 0)]), &config),
            [area(0, 0, 0, 0), area(3, 0, 3, 0)]
        );
        assert_eq!(split(&grid(&[(0, 0), (2, 0)]), &config), [area(0, 0, 2, 0)]);
        // One empty column splits; none does not.
        assert_eq!(
            split(&grid(&[(0, 0), (0, 2)]), &config),
            [area(0, 0, 0, 0), area(0, 2, 0, 2)]
        );
        assert_eq!(split(&grid(&[(0, 0), (0, 1)]), &config), [area(0, 0, 0, 1)]);
    }

    /// `grid` with a blank cell centred across the selection at `place`.
    fn with_blank_centred(mut grid: Grid, place: (u32, u32)) -> Grid {
        grid.cells.insert(
            place,
            GridCell {
                spans: Vec::new(),
                kind: CellKind::Text,
                center_continuous: true,
                own_font: Vec::new(),
                overflow_pt: 0.0,
            },
        );
        grid
    }

    #[test]
    fn blank_centred_cells_count_only_after_centred_text() {
        // A blank centred cell after a text that is not centred joins
        // nothing: B and D stay apart.
        let grid = with_blank_centred(super::tests::grid(&[(0, 1), (0, 3)]), (0, 2));
        assert_eq!(
            split(&grid, &ExcelConfig::default()),
            [area(0, 1, 0, 1), area(0, 3, 0, 3)]
        );
        // After a centred text, the blank centred cells are part of it.
        let mut grid = with_blank_centred(
            with_blank_centred(super::tests::grid(&[(0, 0), (0, 4)]), (0, 1)),
            (0, 2),
        );
        grid.cells.get_mut(&(0, 0)).unwrap().center_continuous = true;
        assert_eq!(
            split(&grid, &ExcelConfig::default()),
            [area(0, 0, 0, 2), area(0, 4, 0, 4)]
        );
        let grid = with_blank_centred(grid, (0, 3));
        assert_eq!(split(&grid, &ExcelConfig::default()), [area(0, 0, 0, 4)]);
    }
}
