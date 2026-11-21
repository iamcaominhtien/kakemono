//! A sheet as read from its part: the cells with their text, the merged
//! ranges, the hidden rows and columns, and what the file declares about
//! its tables. Rows and columns are zero-based.

use crate::model::Span;
use std::collections::{BTreeMap, HashSet};

/// The cells and declarations of one sheet.
#[derive(Debug, Clone, Default)]
pub(crate) struct Grid {
    /// The cells that have a value, by (row, column). An empty cell is kept
    /// only when it centres across the selection, since the text to its
    /// left spans it then.
    pub(crate) cells: BTreeMap<(u32, u32), GridCell>,
    /// The merged ranges, none overlapping another (of two that overlap,
    /// the first in the file is kept).
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) merges: Vec<Merge>,
    /// The hidden rows.
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) hidden_rows: HashSet<u32>,
    /// The hidden columns.
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) hidden_cols: HashSet<u32>,
    /// What the file says about the tables of the sheet.
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) declared: Declared,
    /// The rows with a value left out because they are past the limit of
    /// rows (`ExcelConfig::max_rows`).
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) truncated_rows: u32,
}

/// One cell: its text as Excel shows it, and what kind of value it holds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GridCell {
    /// The text, with the style of the cell's font (or of its rich-text
    /// runs) and its hyperlink.
    pub(crate) spans: Vec<Span>,
    /// The kind of value.
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) kind: CellKind,
    /// The cell's text is centred across the selection
    /// (`centerContinuous`).
    #[allow(dead_code)] // used by build (Task 5)
    pub(crate) center_continuous: bool,
}

/// The kind of value of a cell.
#[allow(dead_code)] // matched by build (Task 5)
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

/// What the file declares about the tables of a sheet, from which their
/// header rows are found.
#[allow(dead_code)] // read by build (Task 5)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    /// The Excel Tables (`xl/tables/`) of the sheet: their range and their
    /// number of header rows (`headerRowCount`).
    pub(crate) tables: Vec<(Merge, u32)>,
    /// The range of the sheet's autofilter: its first row is a header.
    pub(crate) auto_filter: Option<Merge>,
    /// The rows frozen at the top (`pane ySplit`, `state="frozen"`).
    pub(crate) frozen_rows: u32,
    /// The rows printed at the top of every page (`_xlnm.Print_Titles`):
    /// first and last, inclusive.
    pub(crate) print_title_rows: Option<(u32, u32)>,
}
