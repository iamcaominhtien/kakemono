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
    /// How the sheet looks, read only for the sheet view.
    pub(crate) looks: Looks,
    /// How the sheet is printed.
    pub(crate) print: PrintSetup,
    /// The embedded objects (a Word document, ...) shown by a picture.
    pub(crate) objects: Vec<EmbeddedObject>,
}

/// An object embedded in a sheet (`oleObject`), which the sheet shows by
/// a picture of it (`objectPr`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EmbeddedObject {
    /// The relationship (in the sheet part) of the picture.
    pub(crate) rel: String,
    /// The top left corner of its anchor.
    pub(crate) from: Marker,
    /// The bottom right corner of its anchor.
    pub(crate) to: Marker,
}

/// How a sheet is printed (`pageSetup`, `pageMargins`, `printOptions`,
/// `pageSetUpPr`, `rowBreaks`, `colBreaks`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PrintSetup {
    /// The paper is turned: landscape (`orientation`).
    pub(crate) landscape: bool,
    /// The scale, in percent (`scale`).
    pub(crate) scale_percent: f32,
    /// The sheet is fitted to pages (`pageSetUpPr fitToPage`).
    pub(crate) fit_to_page: bool,
    /// The most pages wide it is fitted to; 0 for no limit.
    pub(crate) fit_width: u32,
    /// The most pages tall it is fitted to; 0 for no limit.
    pub(crate) fit_height: u32,
    /// The margins in points: left, right, top, bottom.
    pub(crate) margins_pt: [f32; 4],
    /// Centred across the page.
    pub(crate) h_centered: bool,
    /// Centred down the page.
    pub(crate) v_centered: bool,
    /// The gridlines are printed.
    pub(crate) grid_lines: bool,
    /// Pages go across first, then down (`pageOrder="overThenDown"`).
    pub(crate) over_then_down: bool,
    /// The rows that start a page (manual breaks), 0-based.
    pub(crate) row_breaks: Vec<u32>,
    /// The columns that start a page (manual breaks), 0-based.
    pub(crate) col_breaks: Vec<u32>,
}

/// Points in an inch: the unit of `pageMargins`.
const PT_PER_INCH: f32 = 72.0;

impl Default for PrintSetup {
    /// Excel's defaults: portrait at 100 %, its "normal" margins.
    fn default() -> Self {
        Self {
            landscape: false,
            scale_percent: 100.0,
            fit_to_page: false,
            fit_width: 1,
            fit_height: 1,
            margins_pt: [
                0.7 * PT_PER_INCH,
                0.7 * PT_PER_INCH,
                0.75 * PT_PER_INCH,
                0.75 * PT_PER_INCH,
            ],
            h_centered: false,
            v_centered: false,
            grid_lines: false,
            over_then_down: false,
            row_breaks: Vec::new(),
            col_breaks: Vec::new(),
        }
    }
}

/// The cell formats of a sheet's cells, rows and columns, and whether it
/// shows its gridlines: what the sheet view draws, read only for it.
#[derive(Debug, Clone, Default)]
pub(crate) struct Looks {
    /// The cell format (`s`, an index of the styles) of each cell that
    /// has one other than 0, with a value or not.
    pub(crate) cells: BTreeMap<(u32, u32), u32>,
    /// The cell format of the columns that have one (`col style`): first
    /// and last column, format.
    pub(crate) cols: Vec<(u32, u32, u32)>,
    /// The cell format of the rows that have one (`row s`, with
    /// `customFormat`).
    pub(crate) rows: BTreeMap<u32, u32>,
    /// The sheet hides its gridlines (`sheetView showGridLines="0"`).
    pub(crate) no_grid_lines: bool,
    /// The rows whose height is set by hand (`customHeight`); Excel fits
    /// the others to their text, so they grow when a font set in their
    /// place needs more lines.
    pub(crate) custom_rows: HashSet<u32>,
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
    /// The width of a digit of the default font on the screen the sizes
    /// were read for, in points (whole pixels); 0 when unknown.
    pub(crate) screen_digit_pt: f32,
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
pub(crate) const DEFAULT_COL_PT: f32 = 48.0;
/// Excel's default row height, in points.
const DEFAULT_ROW_PT: f32 = 15.0;

impl Sizes {
    /// The width and height in points of the box from `from` to `to`;
    /// hidden rows and columns (`hidden_rows`, `hidden_cols`) take no room,
    /// and an offset larger than its cell ends at the cell's edge.
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
        // An offset past its cell ends at the cell's edge, as in Excel.
        let row_height = |row: u32| rows_between(row, row.saturating_add(1));
        let col_off = |marker: Marker| marker.col_off_pt.clamp(0.0, col_width(marker.col));
        let row_off = |marker: Marker| marker.row_off_pt.clamp(0.0, row_height(marker.row));
        let width = cols_between(from.col, to.col) - col_off(from) + col_off(to);
        let height = rows_between(from.row, to.row) - row_off(from) + row_off(to);
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

impl Grid {
    /// Whether the cell at `at` shows no text (it may be kept for its
    /// centring).
    pub(crate) fn is_blank(&self, at: (u32, u32)) -> bool {
        self.cells.get(&at).is_none_or(|cell| cell.spans.is_empty())
    }

    /// The width of the columns that the text of the cell at `at` is
    /// centred across, past its own (`centerContinuous`): of `after`, the
    /// columns that follow it in order, the blank cells right after it
    /// that are centred so too, outside any merge, as Excel centres such
    /// a text. `width` gives a column's width. Zero for another cell.
    pub(crate) fn selection_width<'c>(
        &self,
        at: (u32, u32),
        after: impl IntoIterator<Item = &'c u32>,
        width: impl Fn(u32) -> f32,
    ) -> f32 {
        if !self.cells.get(&at).is_some_and(|cell| cell.center_continuous) || self.merged(at) {
            return 0.0;
        }
        after
            .into_iter()
            .take_while(|&&col| {
                self.is_blank((at.0, col))
                    && self.cells.get(&(at.0, col)).is_some_and(|cell| cell.center_continuous)
                    && !self.merged((at.0, col))
            })
            .map(|&col| width(col))
            .sum()
    }

    /// Whether the cell at `at` is in a merged range.
    pub(crate) fn merged(&self, (row, col): (u32, u32)) -> bool {
        self.merges
            .iter()
            .any(|merge| (merge.top..=merge.bottom).contains(&row) && (merge.left..=merge.right).contains(&col))
    }
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
