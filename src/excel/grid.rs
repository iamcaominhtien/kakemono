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
}

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
