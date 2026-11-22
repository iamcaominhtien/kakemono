//! A sheet as read from its part: the cells with their text, the merged
//! ranges, the hidden rows and columns, and what the file declares about
//! its tables. Rows and columns are zero-based.

use crate::model::Span;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Rows and columns of a square of the sheet that [`MergeIndex`] files
/// ranges by.
const SQUARE_CELLS: u32 = 64;

/// The cells and declarations of one sheet.
#[derive(Debug, Clone, Default)]
pub(crate) struct Grid {
    /// The cells that have a value, by (row, column). An empty cell is kept
    /// only when it centres across the selection, since the text to its
    /// left spans it then.
    pub(crate) cells: BTreeMap<(u32, u32), GridCell>,
    /// The merged ranges, none overlapping another (of two that overlap,
    /// the first in the file is kept).
    pub(crate) merges: Vec<Merge>,
    /// The hidden rows.
    pub(crate) hidden_rows: HashSet<u32>,
    /// The hidden columns.
    pub(crate) hidden_cols: HashSet<u32>,
    /// What the file says about the tables of the sheet.
    pub(crate) declared: Declared,
    /// The rows with a value left out because they are past the limit of
    /// rows (`ExcelConfig::max_rows`).
    pub(crate) truncated_rows: u32,
    /// The columns with a value left out because they are past the limit
    /// of columns (`ExcelConfig::max_cols`), in the rows read.
    pub(crate) truncated_cols: u32,
    /// The widths of the columns and the heights of the rows, where the
    /// drawings of the sheet are placed.
    pub(crate) sizes: Sizes,
    /// The cells that show a picture of their own (Excel 365's "Place in
    /// cell"): their place and their value metadata (`vm`, from 1).
    pub(crate) cell_pictures: Vec<((u32, u32), u32)>,
    /// The paper the sheet is printed on (`pageSetup paperSize`): width
    /// and height in points; `None` when the file names none, or one of
    /// the sizes not known here.
    pub(crate) paper_pt: Option<(f32, f32)>,
}

/// The widths of the columns and the heights of the rows of a sheet, as
/// far as the file gives them, in points.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Sizes {
    /// The width of the columns the file sizes (`col`): first and last
    /// column, width in points.
    pub(crate) cols: Vec<(u32, u32, f32)>,
    /// The height of the rows the file sizes (`row ht`), in points.
    pub(crate) rows: BTreeMap<u32, f32>,
    /// The width of the other columns (`sheetFormatPr`); `None` for
    /// Excel's default.
    pub(crate) default_col_pt: Option<f32>,
    /// The height of the other rows (`sheetFormatPr`); `None` for Excel's
    /// default.
    pub(crate) default_row_pt: Option<f32>,
}

/// A corner of a drawing on the sheet: a cell (row, column, both
/// 0-based) and the offset from its top left corner, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Marker {
    /// The row.
    pub(crate) row: u32,
    /// The offset down from the row's top.
    pub(crate) row_off_pt: f32,
    /// The column.
    pub(crate) col: u32,
    /// The offset right from the column's left edge.
    pub(crate) col_off_pt: f32,
}

/// Excel's default column width: 8.43 characters of Calibri 11, 64
/// pixels.
const DEFAULT_COL_PT: f32 = 48.0;
/// Excel's default row height, in points.
const DEFAULT_ROW_PT: f32 = 15.0;

impl Sizes {
    /// The width and height in points of the box from `from` to `to`;
    /// hidden rows and columns (`hidden_rows`, `hidden_cols`) take no room.
    /// A box whose `to` comes before its `from` has no size on that side.
    pub(crate) fn span_pt(
        &self,
        from: Marker,
        to: Marker,
        hidden_rows: &HashSet<u32>,
        hidden_cols: &HashSet<u32>,
    ) -> (f32, f32) {
        let col_width = |col: u32| {
            if hidden_cols.contains(&col) {
                return 0.0;
            }
            self.cols
                .iter()
                .rev()
                .find(|(first, last, _)| (*first..=*last).contains(&col))
                .map_or(self.default_col_pt.unwrap_or(DEFAULT_COL_PT), |(_, _, width)| *width)
        };
        let default_row = self.default_row_pt.unwrap_or(DEFAULT_ROW_PT);
        // The rows: the default height for all, corrected by the rows
        // the file sizes or hides (a sheet may have a million rows).
        let rows_between = |first: u32, last: u32| -> f32 {
            if last <= first {
                return 0.0;
            }
            let mut height = (last - first) as f32 * default_row;
            for (row, row_height) in self.rows.range(first..last) {
                height += if hidden_rows.contains(row) { 0.0 } else { *row_height } - default_row;
            }
            height -= hidden_rows
                .iter()
                .filter(|row| (first..last).contains(*row) && !self.rows.contains_key(row))
                .count() as f32
                * default_row;
            height
        };
        // The columns are at most 16 384.
        let cols_between = |first: u32, last: u32| -> f32 {
            (first..last.min(first.saturating_add(MAX_SHEET_COLS)))
                .map(col_width)
                .sum()
        };
        let width = cols_between(from.col, to.col) - from.col_off_pt + to.col_off_pt;
        let height = rows_between(from.row, to.row) - from.row_off_pt + to.row_off_pt;
        (width.max(0.0), height.max(0.0))
    }
}

/// The most columns of a sheet (XFD).
const MAX_SHEET_COLS: u32 = 16_384;

/// One cell: its text as Excel shows it, and what kind of value it holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GridCell {
    /// The text, with the style of the cell's font (or of its rich-text
    /// runs) and its hyperlink.
    pub(crate) spans: Vec<Span>,
    /// The kind of value.
    pub(crate) kind: CellKind,
    /// The cell's text is centred across the selection
    /// (`centerContinuous`).
    pub(crate) center_continuous: bool,
}

/// The kind of value of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CellKind {
    /// A number (not a date).
    Number,
    /// A number shown as a date or time, or a date written ISO (`t="d"`).
    Date,
    /// Text: a shared, inline or formula string.
    Text,
    /// `TRUE` or `FALSE`.
    Bool,
    /// An error value (`#N/A`, `#DIV/0!`).
    Error,
}

/// An Excel Table: its range and its number of header rows
/// (`headerRowCount`).
pub(crate) type Table = (Merge, u32);

/// A range of cells, inclusive: a merge, a table, an autofilter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Merge {
    /// The first row.
    pub(crate) top: u32,
    /// The first column.
    pub(crate) left: u32,
    /// The last row.
    pub(crate) bottom: u32,
    /// The last column.
    pub(crate) right: u32,
}

impl Merge {
    /// Whether the two ranges have a cell in common.
    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.top <= other.bottom && other.top <= self.bottom && self.left <= other.right && other.left <= self.right
    }
}

/// Ranges filed by the squares of the sheet ([`SQUARE_CELLS`]) they cover,
/// so that those overlapping a range are found among the ranges of its
/// squares, not among all of them: a sheet's merges, which do not overlap
/// each other, are few in a square.
#[derive(Default)]
pub(crate) struct MergeIndex {
    /// The ranges, in the order they were added.
    ranges: Vec<Merge>,
    /// The ranges of each square, by (row, column) of the square.
    squares: HashMap<(u32, u32), Vec<usize>>,
}

impl MergeIndex {
    /// The ranges `ranges`, filed.
    pub(crate) fn new(ranges: &[Merge]) -> Self {
        let mut index = Self::default();
        for range in ranges {
            index.add(*range);
        }
        index
    }

    /// Files `range`.
    pub(crate) fn add(&mut self, range: Merge) {
        for square in squares(&range) {
            self.squares.entry(square).or_default().push(self.ranges.len());
        }
        self.ranges.push(range);
    }

    /// The ranges filed, in the order they were added.
    pub(crate) fn into_ranges(self) -> Vec<Merge> {
        self.ranges
    }

    /// The ranges that overlap `area`, each once, in the order they were
    /// added.
    pub(crate) fn overlapping(&self, area: &Merge) -> Vec<Merge> {
        let mut found: Vec<usize> = squares(area)
            .filter_map(|square| self.squares.get(&square))
            .flatten()
            .copied()
            .filter(|&index| self.ranges[index].overlaps(area))
            .collect();
        found.sort_unstable();
        found.dedup();
        found.into_iter().map(|index| self.ranges[index]).collect()
    }
}

/// The squares of the sheet that `range` covers.
fn squares(range: &Merge) -> impl Iterator<Item = (u32, u32)> + use<> {
    let rows = range.top / SQUARE_CELLS..=range.bottom / SQUARE_CELLS;
    let cols = range.left / SQUARE_CELLS..=range.right / SQUARE_CELLS;
    rows.flat_map(move |row| cols.clone().map(move |col| (row, col)))
}

/// What the file declares about the tables of a sheet, from which their
/// header rows are found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    /// The Excel Tables (`xl/tables/`) of the sheet: their range and their
    /// number of header rows (`headerRowCount`).
    pub(crate) tables: Vec<Table>,
    /// The range of the sheet's autofilter: its first row is a header.
    pub(crate) auto_filter: Option<Merge>,
    /// The rows frozen at the top (`pane ySplit`, `state="frozen"`).
    pub(crate) frozen_rows: u32,
    /// The rows printed at the top of every page (`_xlnm.Print_Titles`):
    /// first and last, inclusive.
    pub(crate) print_title_rows: Option<(u32, u32)>,
}
