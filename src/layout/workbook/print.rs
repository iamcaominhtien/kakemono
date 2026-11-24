//! A workbook printed as Excel prints it (the default PDF of a workbook):
//! each visible sheet's print area (or the cells it uses) cut into pages
//! by its print settings (paper, orientation, margins, scale or fitting to
//! pages, manual breaks, rows repeated at the top, page order, centring),
//! and on each page the cells with their fills, borders, fonts and
//! alignment, the text running over the empty cells beside it, and the
//! charts, pictures and shapes where they are anchored. It draws the same
//! reading of the file as the sheet view (`excel::view`), with the fonts
//! and PDF writer of the Word layout.

use super::super::display::{Item, Page};
use super::super::fonts::FontBook;
use super::super::para::{self, Ctx};
use super::super::pdf;
use super::super::shape::Shaper;
use super::WorkbookPdfConfig;
use crate::chart::{Rgb, Theme};
use crate::excel::drawing::{Drawn, Shape, ShapeRun, TextShape};
use crate::excel::grid::{CellKind, Grid, GridCell, Marker, Merge};
use crate::excel::styles::{Color, HAlign, Line, Look, Styles, VAlign};
use crate::excel::view::{alignment, col_width, look_index, object_box, shape_svg};
use crate::excel::{DrawnSheet, SheetBook};
use crate::model::style::{BOLD, ITALIC};
use crate::word::fmt::model::{
    Align, BreakKind, FDoc, FImage, Inline, LineSpacing, Para, ParaProps, RunFmt, Underline,
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// US Letter, Excel's paper when a sheet names none, in points.
const LETTER_PT: (f32, f32) = (612.0, 792.0);
/// Excel's default font when the styles name none.
const DEFAULT_FONT: &str = "Calibri";
/// Excel's default font size, in points.
const DEFAULT_SIZE_PT: f32 = 11.0;
/// The smallest scale a sheet is fitted to pages at (Excel's 10 %).
const MIN_SCALE: f32 = 0.1;
/// Points in a pixel at 96 pixels per inch: borders are given in pixels.
const PT_PER_PX: f32 = 0.75;
/// The space between a cell's edge and its text, left and right, in
/// points (two pixels).
const CELL_PADDING_PT: f32 = 1.5;
/// The space above and below a cell's text, in points.
const CELL_PADDING_TOP_PT: f32 = 1.0;
/// The width of one step of a cell's indent, in points.
const INDENT_STEP_PT: f32 = 7.5;
/// The space between a shape's edge and its text, in points (0.1 inch
/// left and right, 0.05 inch above and below, as Office's text boxes).
const SHAPE_PADDING_PT: (f32, f32) = (7.2, 3.6);
/// The colour of printed gridlines.
const GRID_LINE_COLOR: Rgb = [0xBF, 0xBF, 0xBF];
/// The most empty columns a text runs over, as the sheet view.
const MAX_OVERFLOW_COLS: usize = 40;
/// URL schemes a cell's link may have in the PDF.
const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "ftp://"];
/// Japanese families tried, in order, for the East Asian text of a cell
/// whose font is a Western one.
const CJK_FAMILIES: &[&str] = &[
    "Noto Sans CJK JP",
    "Source Han Sans JP",
    "IPAexGothic",
    "IPAGothic",
    "Yu Gothic",
    "MS Gothic",
    "Noto Serif CJK JP",
];

/// The PDF of the workbook `book` printed as Excel prints it, at most
/// `config.print_max_pages` pages; one empty page of Letter paper when no
/// sheet prints anything.
///
/// # Errors
/// When the PDF cannot be written.
pub(crate) fn to_pdf(book: &SheetBook, config: &WorkbookPdfConfig) -> Result<Vec<u8>> {
    let mut fonts = FontBook::new(&[], &HashMap::new());
    let cjk = CJK_FAMILIES
        .iter()
        .find(|name| fonts.has_family(name))
        .map_or_else(|| CJK_FAMILIES[0].to_string(), ToString::to_string);
    let mut images: Vec<FImage> = book
        .pictures
        .iter()
        .map(|picture| FImage {
            data: picture.data.clone(),
            format: picture.format,
            bare: None,
            texts: Vec::new(),
        })
        .collect();
    let mut shaper = Shaper::default();
    let mut coverage = HashMap::new();
    let doc = FDoc::default();
    let notes = HashMap::new();
    let mut ctx = Ctx {
        book: &mut fonts,
        shaper: &mut shaper,
        doc: &doc,
        notes: &notes,
        own_note: None,
        page: None,
        page_count: None,
        section_pages: None,
        coverage: &mut coverage,
        page_refs: None,
        origin: 0.0,
    };
    let mut pages = Vec::new();
    for sheet in &book.sheets {
        let left = config.print_max_pages.saturating_sub(pages.len());
        if left == 0 {
            break;
        }
        let mut printer = Printer {
            sheet,
            grid: &sheet.grid,
            styles: &book.styles,
            theme: &book.theme,
            cjk: &cjk,
            images: &mut images,
            ctx: &mut ctx,
        };
        pages.extend(printer.pages(left));
    }
    if pages.is_empty() {
        pages.push(Page {
            width: LETTER_PT.0,
            height: LETTER_PT.1,
            items: Vec::new(),
        });
    }
    drop(ctx);
    pdf::write(&pages, &[], &fonts, &images, book.title.as_deref())
}

/// What one sheet is printed with.
struct Printer<'a, 'c, 'f> {
    /// The sheet.
    sheet: &'a DrawnSheet,
    /// Its cells.
    grid: &'a Grid,
    /// The cell formats.
    styles: &'a Styles,
    /// The theme colours.
    theme: &'a Theme,
    /// The Japanese family of cells set in a Western font.
    cjk: &'a str,
    /// The pictures of the PDF: those of the workbook, then the outlines
    /// of shapes.
    images: &'a mut Vec<FImage>,
    /// The fonts and shaper.
    ctx: &'c mut Ctx<'f>,
}

/// The part of a sheet that prints: its rows and columns, with where each
/// starts on the sheet (hidden ones taking no room).
struct Area {
    /// The box printed.
    range: Merge,
    /// The left edge of each column of the range, from the sheet's left
    /// edge, and the right edge of the last one: `right - left + 2`
    /// numbers.
    xs: Vec<f32>,
    /// The top edge of each row of the range, and the bottom of the last.
    ys: Vec<f32>,
    /// The rows that print (not hidden), in order.
    rows: Vec<u32>,
    /// The columns that print, in order.
    cols: Vec<u32>,
}

impl Area {
    /// The left edge of the column `col` of the range.
    fn x(&self, col: u32) -> f32 {
        self.xs[(col - self.range.left) as usize]
    }

    /// The width of the column `col`.
    fn width(&self, col: u32) -> f32 {
        self.x(col + 1) - self.x(col)
    }

    /// The top edge of the row `row` of the range.
    fn y(&self, row: u32) -> f32 {
        self.ys[(row - self.range.top) as usize]
    }

    /// The height of the row `row`.
    fn height(&self, row: u32) -> f32 {
        self.y(row + 1) - self.y(row)
    }
}

/// How the pages of a sheet are laid out.
struct Layout {
    /// Width and height of the paper, turned as the sheet says.
    paper: (f32, f32),
    /// The top left corner of the room within the margins.
    origin: (f32, f32),
    /// Width and height of the room within the margins.
    room: (f32, f32),
    /// The scale of the sheet on the pages.
    scale: f32,
    /// The rows repeated at the top of each page.
    titles: Vec<u32>,
    /// Their height on the sheet.
    titles_h: f32,
}

/// The scale a sheet of `area` prints at with the settings `print` on
/// pages of `room` (width, height within the margins): fitted to its pages
/// (never larger than 100 %), else its own scale.
fn scale(print: &crate::excel::grid::PrintSetup, area: &Area, room: (f32, f32)) -> f32 {
    if !print.fit_to_page {
        return print.scale_percent / 100.0;
    }
    let total_w: f32 = area.cols.iter().map(|&col| area.width(col)).sum();
    let total_h: f32 = area.rows.iter().map(|&row| area.height(row)).sum();
    let mut scale: f32 = 1.0;
    if print.fit_width > 0 && total_w > 0.0 {
        scale = scale.min(print.fit_width as f32 * room.0 / total_w);
    }
    if print.fit_height > 0 && total_h > 0.0 {
        scale = scale.min(print.fit_height as f32 * room.1 / total_h);
    }
    scale.max(MIN_SCALE)
}

/// A band of a page: rows printed one under the other (the rows repeated
/// at the top of each page, or the page's own rows), and how the sheet
/// maps onto the page there.
struct Band {
    /// The rows, in order.
    rows: Vec<u32>,
    /// The sheet's top of the band.
    sheet_top: f32,
    /// The sheet's bottom of the band.
    sheet_bottom: f32,
    /// The page's top of the band.
    page_top: f32,
}

/// Where the columns of a page are.
struct Columns {
    /// The columns, in order.
    cols: Vec<u32>,
    /// The sheet's left edge of the first.
    sheet_left: f32,
    /// The sheet's right edge of the last.
    sheet_right: f32,
    /// The page's left edge of the first.
    page_left: f32,
}

impl Printer<'_, '_, '_> {
    /// The pages of the sheet, at most `max` of them.
    fn pages(&mut self, max: usize) -> Vec<Page> {
        let Some(area) = self.area() else {
            return Vec::new();
        };
        let print = &self.grid.print;
        let (short, long) = {
            let (width, height) = self.grid.paper_pt.unwrap_or(LETTER_PT);
            (width.min(height), width.max(height))
        };
        let paper = if print.landscape { (long, short) } else { (short, long) };
        let [margin_l, margin_r, margin_t, margin_b] = print.margins_pt;
        let room = (
            (paper.0 - margin_l - margin_r).max(paper.0 / 4.0),
            (paper.1 - margin_t - margin_b).max(paper.1 / 4.0),
        );
        let titles: Vec<u32> = self
            .grid
            .declared
            .print_title_rows
            .map(|(first, last)| {
                area.rows
                    .iter()
                    .copied()
                    .filter(|row| (first..=last).contains(row))
                    .collect()
            })
            .unwrap_or_default();
        let titles_h: f32 = titles.iter().map(|&row| area.height(row)).sum();
        let scale = scale(print, &area, room);
        // Fitting the sheet to a number of pages leaves the manual breaks
        // of that side out; a side not fitted (0 pages) keeps them, as
        // LibreOffice does.
        let keep = |fitted: u32, breaks: &[u32]| -> HashSet<u32> {
            if print.fit_to_page && fitted > 0 {
                HashSet::new()
            } else {
                breaks.iter().copied().collect()
            }
        };
        let col_groups = groups(
            &area.cols,
            |col| area.width(col),
            room.0 / scale,
            &keep(print.fit_width, &print.col_breaks),
            0.0,
        );
        let row_groups = groups(
            &area.rows,
            |row| area.height(row),
            room.1 / scale,
            &keep(print.fit_height, &print.row_breaks),
            titles_h,
        );
        let layout = Layout {
            paper,
            origin: (margin_l, margin_t),
            room,
            scale,
            titles,
            titles_h,
        };
        let mut order = Vec::new();
        if print.over_then_down {
            for rows in &row_groups {
                order.extend(col_groups.iter().map(|cols| (rows, cols)));
            }
        } else {
            for cols in &col_groups {
                order.extend(row_groups.iter().map(|rows| (rows, cols)));
            }
        }
        order
            .into_iter()
            .take(max)
            .map(|(rows, cols)| self.page(&area, &layout, rows, cols))
            .collect()
    }

    /// The page of the rows `rows` and columns `cols` of `area`.
    fn page(&mut self, area: &Area, layout: &Layout, rows: &[u32], cols: &[u32]) -> Page {
        let print = &self.grid.print;
        let scale = layout.scale;
        // The repeated rows head every page whose rows come after them.
        let repeat = layout
            .titles
            .last()
            .is_some_and(|last| rows.first().is_some_and(|first| first > last));
        let content_w: f32 = cols.iter().map(|&col| area.width(col)).sum::<f32>() * scale;
        let repeated_h = if repeat { layout.titles_h } else { 0.0 };
        let content_h: f32 = (rows.iter().map(|&row| area.height(row)).sum::<f32>() + repeated_h) * scale;
        let centred = |on: bool, room: f32, content: f32| if on { (room - content).max(0.0) / 2.0 } else { 0.0 };
        let left = layout.origin.0 + centred(print.h_centered, layout.room.0, content_w);
        let top = layout.origin.1 + centred(print.v_centered, layout.room.1, content_h);
        let columns = Columns {
            cols: cols.to_vec(),
            sheet_left: area.x(cols[0]),
            sheet_right: area.x(*cols.last().unwrap_or(&cols[0]) + 1),
            page_left: left,
        };
        let mut bands = Vec::new();
        let mut page_top = top;
        if repeat {
            bands.push(band(area, &layout.titles, page_top));
            page_top += layout.titles_h * scale;
        }
        bands.push(band(area, rows, page_top));
        let mut page = Page {
            width: layout.paper.0,
            height: layout.paper.1,
            items: Vec::new(),
        };
        self.draw_page(&mut page.items, area, &columns, &bands, scale, repeat);
        page
    }

    /// The part of the sheet that prints: its print area, else from A1 to
    /// the last cell with a value or a visible format (a fill or a border),
    /// a merge or a drawn object; `None` when nothing prints.
    fn area(&mut self) -> Option<Area> {
        let grid = self.grid;
        let range = match self.sheet.print_area {
            Some(range) => range,
            None => {
                let used = self.used_range()?;
                Merge {
                    right: self.spill_right(used.right),
                    ..used
                }
            }
        };
        // Within the rows and columns read.
        let bottom = range.bottom.min(range.top.saturating_add(MAX_PRINT_ROWS));
        let right = range.right.min(range.left.saturating_add(MAX_PRINT_COLS));
        let range = Merge { bottom, right, ..range };
        let origin = Marker {
            row: range.top,
            row_off_pt: 0.0,
            col: range.left,
            col_off_pt: 0.0,
        };
        let start = grid.sizes.span_pt(
            Marker {
                row: 0,
                col: 0,
                ..origin
            },
            origin,
            &grid.hidden_rows,
            &grid.hidden_cols,
        );
        let mut xs = vec![start.0];
        for col in range.left..=range.right {
            let width = if grid.hidden_cols.contains(&col) {
                0.0
            } else {
                col_width(&grid.sizes, col)
            };
            xs.push(xs[xs.len() - 1] + width);
        }
        let default_row = grid.sizes.default_row_pt.unwrap_or(DEFAULT_ROW_PT);
        let mut ys = vec![start.1];
        for row in range.top..=range.bottom {
            let height = if grid.hidden_rows.contains(&row) {
                0.0
            } else {
                grid.sizes.rows.get(&row).copied().unwrap_or(default_row)
            };
            ys.push(ys[ys.len() - 1] + height);
        }
        let rows: Vec<u32> = (range.top..=range.bottom)
            .filter(|row| !grid.hidden_rows.contains(row))
            .collect();
        let cols: Vec<u32> = (range.left..=range.right)
            .filter(|col| !grid.hidden_cols.contains(col))
            .collect();
        (!rows.is_empty() && !cols.is_empty()).then_some(Area {
            range,
            xs,
            ys,
            rows,
            cols,
        })
    }

    /// The last column a sheet whose cells end at column `right` prints:
    /// further right when the last text of a row runs over the empty cells
    /// past it, as Excel prints such text whole.
    fn spill_right(&mut self, right: u32) -> u32 {
        // The last cell of each row.
        let mut last: HashMap<u32, (u32, &GridCell)> = HashMap::new();
        for (&(row, col), cell) in &self.grid.cells {
            last.insert(row, (col, cell));
        }
        let mut spill = right;
        for (row, (col, cell)) in last {
            let look = self.styles.look(look_index(self.grid, (row, col)) as usize);
            let in_merge =
                self.grid.merges.iter().any(|merge| {
                    (merge.top..=merge.bottom).contains(&row) && (merge.left..=merge.right).contains(&col)
                });
            if cell.kind != CellKind::Text
                || look.wrap
                || in_merge
                || !matches!(alignment(look.h_align, Some(cell.kind)), "left" | "center")
            {
                continue;
            }
            let base = self.run(look, 1.0);
            let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
            let (_, text_w) = para::text_items(self.ctx, &text.replace(['\n', '\r', '\t'], " "), &base, 0.0, 0.0);
            let indent = f32::from(look.indent) * INDENT_STEP_PT;
            let mut room = col_width(&self.grid.sizes, col) - 2.0 * CELL_PADDING_PT - indent;
            let mut next = col;
            while room < text_w && next < col.saturating_add(MAX_OVERFLOW_COLS as u32) {
                next += 1;
                if !self.grid.hidden_cols.contains(&next) {
                    room += col_width(&self.grid.sizes, next);
                }
            }
            spill = spill.max(next);
        }
        spill
    }

    /// From A1 to the last cell with a value or a visible format (a fill
    /// or a border), a merge or a drawn object; `None` when there is none.
    fn used_range(&self) -> Option<Merge> {
        let grid = self.grid;
        let mut last: Option<(u32, u32)> = None;
        let mut extend = |(row, col): (u32, u32)| {
            let (last_row, last_col) = last.unwrap_or((0, 0));
            last = Some((last_row.max(row), last_col.max(col)));
        };
        grid.cells.keys().copied().for_each(&mut extend);
        for (&at, &style) in &grid.looks.cells {
            let look = self.styles.look(style as usize);
            if look.fill.is_some() || look.borders.iter().any(Option::is_some) {
                extend(at);
            }
        }
        for merge in &grid.merges {
            extend((merge.bottom, merge.right));
        }
        for shape in &self.sheet.shapes {
            if let Some(marker) = shape.frame.to.or(shape.frame.from) {
                extend((marker.row, marker.col));
            }
        }
        let (bottom, right) = last?;
        Some(Merge {
            top: 0,
            left: 0,
            bottom,
            right,
        })
    }

    /// Draws a page: the cells of `bands` × `columns` (fills, gridlines,
    /// borders, then text), then the drawn objects of its own rows
    /// (`repeat`: the first band is the repeated rows).
    fn draw_page(
        &mut self,
        items: &mut Vec<Item>,
        area: &Area,
        columns: &Columns,
        bands: &[Band],
        scale: f32,
        repeat: bool,
    ) {
        let mut texts = Vec::new();
        let mut lines = Vec::new();
        for band in bands {
            let cells = self.band_cells(area, columns, band);
            for (at, rect, span) in &cells {
                let own = self.styles.look(look_index(self.grid, *at) as usize);
                let edged;
                let look = match span {
                    // A merged range's right and bottom borders are those
                    // of its right column and bottom row, as Excel draws.
                    Some(merge) => {
                        edged = Look {
                            borders: [
                                own.borders[0],
                                self.styles
                                    .look(look_index(self.grid, (merge.top, merge.right)) as usize)
                                    .borders[1],
                                own.borders[2],
                                self.styles
                                    .look(look_index(self.grid, (merge.bottom, merge.left)) as usize)
                                    .borders[3],
                            ],
                            ..own.clone()
                        };
                        &edged
                    }
                    None => own,
                };
                let rect = self.to_page(*rect, columns, band, scale);
                if let Some(fill) = look.fill.and_then(|color| self.rgb(color)) {
                    items.push(Item::Rect {
                        x: rect[0],
                        y: rect[1],
                        width: rect[2],
                        height: rect[3],
                        color: fill,
                    });
                }
                if self.grid.print.grid_lines {
                    grid_lines(&mut lines, rect, scale);
                }
                self.borders(&mut lines, look, rect, scale);
                let clip = self.clip(area, columns, band, *at, *span, rect, scale, look);
                texts.extend(self.cell_text(*at, look, rect, clip, scale));
            }
        }
        items.append(&mut lines);
        items.append(&mut texts);
        // The drawn objects of the page's own rows.
        let Some(band) = bands.get(usize::from(repeat)) else {
            return;
        };
        let region = [
            columns.page_left,
            band.page_top,
            columns.page_left + (columns.sheet_right - columns.sheet_left) * scale,
            band.page_top + (band.sheet_bottom - band.sheet_top) * scale,
        ];
        for shape in &self.sheet.shapes {
            self.shape(items, shape, columns, band, scale, region);
        }
    }

    /// The cells of a band of a page: (first cell, box on the sheet,
    /// columns and rows of a merge); a merge once, with its whole box,
    /// also when it starts on an earlier page.
    #[allow(clippy::type_complexity)]
    fn band_cells(&self, area: &Area, columns: &Columns, band: &Band) -> Vec<((u32, u32), [f32; 4], Option<Merge>)> {
        let first_col = columns.cols[0];
        let last_col = *columns.cols.last().unwrap_or(&first_col);
        let (Some(&first_row), Some(&last_row)) = (band.rows.first(), band.rows.last()) else {
            return Vec::new();
        };
        let page = Merge {
            top: first_row,
            left: first_col,
            bottom: last_row,
            right: last_col,
        };
        let mut covered = HashSet::new();
        let mut cells = Vec::new();
        for merge in self.grid.merges.iter().filter(|merge| merge.overlaps(&page)) {
            let merge = Merge {
                bottom: merge.bottom.min(area.range.bottom),
                right: merge.right.min(area.range.right),
                ..*merge
            };
            if merge.top < area.range.top || merge.left < area.range.left {
                continue;
            }
            for row in merge.top.max(first_row)..=merge.bottom.min(last_row) {
                for col in merge.left.max(first_col)..=merge.right.min(last_col) {
                    covered.insert((row, col));
                }
            }
            let rect = [
                area.x(merge.left),
                area.y(merge.top),
                area.x(merge.right + 1) - area.x(merge.left),
                area.y(merge.bottom + 1) - area.y(merge.top),
            ];
            cells.push(((merge.top, merge.left), rect, Some(merge)));
        }
        for &row in &band.rows {
            for &col in &columns.cols {
                if covered.contains(&(row, col)) {
                    continue;
                }
                let rect = [area.x(col), area.y(row), area.width(col), area.height(row)];
                cells.push(((row, col), rect, None));
            }
        }
        cells
    }

    /// A box of the sheet (left, top, width, height) on the page, through
    /// `columns` and `band`.
    fn to_page(&self, [x, y, width, height]: [f32; 4], columns: &Columns, band: &Band, scale: f32) -> [f32; 4] {
        [
            columns.page_left + (x - columns.sheet_left) * scale,
            band.page_top + (y - band.sheet_top) * scale,
            width * scale,
            height * scale,
        ]
    }

    /// Where the text of the cell at `at` (box `rect` on the page; `span`
    /// when it is a merge) may show: its box, the band and the page's
    /// columns; for a text not wrapped, also the empty cells beside it
    /// that it runs over, by its alignment.
    #[allow(clippy::too_many_arguments)]
    fn clip(
        &self,
        area: &Area,
        columns: &Columns,
        band: &Band,
        at: (u32, u32),
        span: Option<Merge>,
        rect: [f32; 4],
        scale: f32,
        look: &Look,
    ) -> [f32; 4] {
        let band_top = band.page_top;
        let band_bottom = band.page_top + (band.sheet_bottom - band.sheet_top) * scale;
        let page_left = columns.page_left;
        let page_right = columns.page_left + (columns.sheet_right - columns.sheet_left) * scale;
        let mut left = rect[0];
        let mut right = rect[0] + rect[2];
        let cell = self.grid.cells.get(&at);
        let runs_over = span.is_none() && !look.wrap && cell.is_some_and(|cell| cell.kind == CellKind::Text);
        if runs_over {
            let free = |col: u32| {
                !self.grid.cells.contains_key(&(at.0, col))
                    && !self.grid.merges.iter().any(|merge| {
                        (merge.top..=merge.bottom).contains(&at.0) && (merge.left..=merge.right).contains(&col)
                    })
            };
            let position = columns.cols.iter().position(|&col| col == at.1).unwrap_or(0);
            let align = alignment(look.h_align, cell.map(|cell| cell.kind));
            if matches!(align, "left" | "center") {
                for &col in columns.cols[position + 1..].iter().take(MAX_OVERFLOW_COLS) {
                    if !free(col) {
                        break;
                    }
                    right += area.width(col) * scale;
                }
            }
            if matches!(align, "right" | "center") {
                for &col in columns.cols[..position].iter().rev().take(MAX_OVERFLOW_COLS) {
                    if !free(col) {
                        break;
                    }
                    left -= area.width(col) * scale;
                }
            }
        }
        let top = rect[1].max(band_top);
        let bottom = (rect[1] + rect[3]).min(band_bottom);
        [left.max(page_left), top, right.min(page_right), bottom]
    }

    /// The lines of the borders of a cell of look `look` at `rect`.
    fn borders(&self, lines: &mut Vec<Item>, look: &Look, [x, y, width, height]: [f32; 4], scale: f32) {
        let sides = [
            (x, y, x, y + height),
            (x + width, y, x + width, y + height),
            (x, y, x + width, y),
            (x, y + height, x + width, y + height),
        ];
        for ((x1, y1, x2, y2), edge) in sides.into_iter().zip(look.borders) {
            let Some(edge) = edge else { continue };
            let color = edge.color.and_then(|color| self.rgb(color)).unwrap_or([0; 3]);
            let line_w = (edge.width_px * PT_PER_PX * scale.max(0.5)).max(0.25);
            let dash = match edge.line {
                Line::Dashed => Some(line_w * 3.0),
                Line::Dotted => Some(line_w),
                Line::Solid | Line::Double => None,
            };
            if edge.line == Line::Double {
                // Two thin lines, a line apart.
                let thin = line_w / 3.0;
                let (dx, dy) = if x1 == x2 { (thin, 0.0) } else { (0.0, thin) };
                for sign in [-1.0, 1.0] {
                    lines.push(Item::Line {
                        x1: x1 + sign * dx,
                        y1: y1 + sign * dy,
                        x2: x2 + sign * dx,
                        y2: y2 + sign * dy,
                        width: thin,
                        color,
                        dash: None,
                    });
                }
                continue;
            }
            lines.push(Item::Line {
                x1,
                y1,
                x2,
                y2,
                width: line_w,
                color,
                dash,
            });
        }
    }

    /// The items of the text of the cell at `at` (look `look`, box `rect`
    /// on the page), cut to `clip`.
    fn cell_text(&mut self, at: (u32, u32), look: &Look, rect: [f32; 4], clip: [f32; 4], scale: f32) -> Vec<Item> {
        let Some(cell) = self.grid.cells.get(&at) else {
            return Vec::new();
        };
        if cell.spans.iter().all(|span| span.text.trim().is_empty()) {
            return Vec::new();
        }
        let base = self.run(look, scale);
        let pad = CELL_PADDING_PT * scale;
        let pad_top = CELL_PADDING_TOP_PT * scale;
        let indent = f32::from(look.indent) * INDENT_STEP_PT * scale;
        let align = alignment(look.h_align, Some(cell.kind));
        let [x, y, width, height] = rect;
        let inner_w = (width - 2.0 * pad - indent).max(1.0);
        let (ascent, descent, _) = self.ctx.line_metrics(&base);
        let (mut items, text_h) = if look.stacked || look.rotation.is_some() {
            self.stacked_text(cell, &base, width, (ascent, descent))
        } else if look.wrap || look.h_align == HAlign::Justify {
            self.wrapped_text(cell, &base, align, pad, indent, inner_w)
        } else {
            let mut items = Vec::new();
            let mut pen = 0.0;
            for span in &cell.spans {
                let fmt = self.span_run(&base, span);
                let text = span.text.replace(['\n', '\r', '\t'], " ");
                let (mut run, run_w) = para::text_items(self.ctx, &text, &fmt, pen, ascent);
                items.append(&mut run);
                pen += run_w;
            }
            let start = match align {
                "right" => width - pad - indent - pen,
                "center" => (width - pen) / 2.0,
                _ => pad + indent,
            };
            translate(&mut items, start, 0.0);
            (items, ascent + descent)
        };
        let top = match look.v_align {
            VAlign::Top => pad_top,
            VAlign::Middle => (height - text_h) / 2.0,
            VAlign::Bottom => height - pad_top - text_h,
        };
        translate(&mut items, x, y + top);
        clip_items(items, clip)
    }

    /// The text of `cell` a letter a line, centred in a cell `width`
    /// wide, set in `base` whose lines have `metrics` (ascent, descent):
    /// turned text is set so, as stacked text is. Its items from the top
    /// left of the cell, and its height.
    fn stacked_text(&mut self, cell: &GridCell, base: &RunFmt, width: f32, metrics: (f32, f32)) -> (Vec<Item>, f32) {
        let (ascent, descent) = metrics;
        let mut items = Vec::new();
        let mut line_y = ascent;
        for ch in cell
            .spans
            .iter()
            .flat_map(|span| span.text.chars())
            .filter(|ch| !ch.is_whitespace())
        {
            let (mut run, run_w) = para::text_items(self.ctx, &ch.to_string(), base, 0.0, line_y);
            translate(&mut run, (width - run_w) / 2.0, 0.0);
            items.append(&mut run);
            line_y += ascent + descent;
        }
        (items, line_y - ascent)
    }

    /// The text of `cell` wrapped at `inner_w`, aligned `align`, `pad` and
    /// `indent` in from the cell's left edge. Its items from the top left
    /// of the cell, and its height.
    fn wrapped_text(
        &mut self,
        cell: &GridCell,
        base: &RunFmt,
        align: &str,
        pad: f32,
        indent: f32,
        inner_w: f32,
    ) -> (Vec<Item>, f32) {
        let para = Para {
            props: ParaProps {
                align: match align {
                    "center" => Align::Center,
                    "right" => Align::Right,
                    "justify" => Align::Both,
                    _ => Align::Left,
                },
                line: LineSpacing::Auto(1.0),
                ..Default::default()
            },
            mark: base.clone(),
            inlines: self.inlines(cell, base),
            ..Default::default()
        };
        let laid = para::layout(self.ctx, &para, inner_w);
        let mut items = Vec::new();
        let mut line_top = 0.0;
        let left = pad + if align == "right" { 0.0 } else { indent };
        for mut line in laid.lines {
            translate(&mut line.items, left, line_top);
            items.append(&mut line.items);
            line_top += line.height;
        }
        (items, line_top)
    }

    /// The inlines of a cell's text for a wrapped paragraph: a line feed
    /// is a line break.
    fn inlines(&self, cell: &GridCell, base: &RunFmt) -> Vec<Inline> {
        let mut inlines = Vec::new();
        for span in &cell.spans {
            let fmt = self.span_run(base, span);
            for (index, line) in span.text.split('\n').enumerate() {
                if index > 0 {
                    inlines.push(Inline::Break {
                        kind: BreakKind::Line,
                        fmt: fmt.clone(),
                    });
                }
                let line = line.replace(['\t', '\r'], " ");
                if !line.is_empty() {
                    inlines.push(Inline::Text {
                        text: line,
                        fmt: fmt.clone(),
                    });
                }
            }
        }
        inlines
    }

    /// The run of a cell's text in the font of `look`, scaled by `scale`.
    fn run(&self, look: &Look, scale: f32) -> RunFmt {
        let base = &self.styles.look(0).font;
        let font = &look.font;
        let name = font
            .name
            .clone()
            .or_else(|| base.name.clone())
            .unwrap_or_else(|| DEFAULT_FONT.to_string());
        // A Japanese font sets the whole text; a Western one leaves its
        // Japanese letters to a Japanese family.
        let east_asia = if name.is_ascii() {
            self.cjk.to_string()
        } else {
            name.clone()
        };
        let size = font.size_pt.or(base.size_pt).unwrap_or(DEFAULT_SIZE_PT) * scale;
        RunFmt {
            font: name.clone(),
            font_hansi: name.clone(),
            font_east_asia: east_asia,
            font_cs: name,
            size,
            size_cs: size,
            bold: font.bold,
            italic: font.italic,
            underline: font.underline.then_some(Underline::Single),
            strike: font.strike,
            color: font
                .color
                .and_then(|color| self.rgb(color))
                .filter(|rgb| *rgb != [0; 3]),
            ..RunFmt::default()
        }
    }

    /// The run of a span of a cell set in `base`: its bold, italic, colour
    /// and link (of the schemes a PDF may open).
    fn span_run(&self, base: &RunFmt, span: &crate::model::Span) -> RunFmt {
        let mut fmt = base.clone();
        fmt.bold |= span.style & BOLD != 0;
        fmt.italic |= span.style & ITALIC != 0;
        if span.color != [0; 3] {
            fmt.color = Some(span.color);
        }
        fmt.href = span
            .href
            .clone()
            .filter(|href| LINK_SCHEMES.iter().any(|scheme| href.starts_with(scheme)));
        fmt
    }

    /// Draws a chart, picture or shape of the sheet where it is anchored,
    /// the part of it in `region` of the page.
    fn shape(
        &mut self,
        items: &mut Vec<Item>,
        shape: &Shape,
        columns: &Columns,
        band: &Band,
        scale: f32,
        region: [f32; 4],
    ) {
        let Some(sheet_box) = object_box(self.grid, shape) else {
            return;
        };
        let [x, y, width, height] = self.to_page(sheet_box, columns, band, scale);
        if width < 0.0 || height < 0.0 {
            return;
        }
        match &shape.drawn {
            Drawn::Picture(picture) => {
                if let Some(index) = picture.index {
                    items.extend(image(index, [x, y, width, height], region));
                }
            }
            Drawn::CellPicture(picture) => {
                let Some(index) = picture.index else { return };
                // In its cell, as large as fits without changing its shape.
                let (own_w, own_h) = picture.size_pt.unwrap_or((width, height));
                let fit = (width / own_w.max(1.0)).min(height / own_h.max(1.0));
                let (fit_w, fit_h) = (own_w * fit, own_h * fit);
                let placed = [x + (width - fit_w) / 2.0, y + (height - fit_h) / 2.0, fit_w, fit_h];
                items.extend(image(index, placed, region));
            }
            Drawn::Chart(chart) => {
                if let Some((_, Some(index))) = chart.placed.as_deref() {
                    items.extend(image(*index, [x, y, width, height], region));
                }
            }
            Drawn::Text(text) => {
                if let Some((svg, [dx, dy, svg_w, svg_h])) = shape_svg(text, width / scale, height / scale) {
                    let index = self.images.len();
                    self.images.push(FImage {
                        data: svg.into_bytes(),
                        format: crate::model::PictureFormat::Svg,
                        bare: None,
                        texts: Vec::new(),
                    });
                    let placed = [x + dx * scale, y + dy * scale, svg_w * scale, svg_h * scale];
                    items.extend(image(index, placed, region));
                }
                items.extend(self.shape_text(text, [x, y, width, height], scale, region));
            }
        }
    }

    /// The items of the text of a shape in its box `rect` on the page.
    fn shape_text(&mut self, shape: &TextShape, rect: [f32; 4], scale: f32, region: [f32; 4]) -> Vec<Item> {
        if shape.text.is_empty() {
            return Vec::new();
        }
        let mut fmt = self.run(self.styles.look(0), scale);
        if let Some(size) = shape.size_pt {
            fmt.size = size * scale;
            fmt.size_cs = fmt.size;
        }
        fmt.bold = shape.bold;
        fmt.color = shape.color.filter(|rgb| *rgb != [0; 3]);
        let (pad_x, pad_y) = (SHAPE_PADDING_PT.0 * scale, SHAPE_PADDING_PT.1 * scale);
        let [x, y, width, height] = rect;
        let inlines = shape_inlines(shape, &fmt, scale);
        let para = Para {
            props: ParaProps {
                align: match shape.align.as_deref() {
                    Some("ctr") => Align::Center,
                    Some("r") => Align::Right,
                    Some("just" | "dist") => Align::Both,
                    _ => Align::Left,
                },
                line: LineSpacing::Auto(1.0),
                ..Default::default()
            },
            mark: fmt,
            inlines,
            ..Default::default()
        };
        let laid = para::layout(self.ctx, &para, (width - 2.0 * pad_x).max(1.0));
        let mut items = Vec::new();
        let mut line_top = 0.0;
        for mut line in laid.lines {
            translate(&mut line.items, 0.0, line_top);
            items.append(&mut line.items);
            line_top += line.height;
        }
        let top = match shape.anchor.as_deref() {
            Some("ctr") => (height - line_top) / 2.0,
            Some("b") => height - pad_y - line_top,
            _ => pad_y,
        };
        translate(&mut items, x + pad_x, y + top);
        clip_items(items, [region[0], region[1], region[2], region[3]])
    }

    /// A colour of the styles as RGB.
    fn rgb(&self, color: Color) -> Option<Rgb> {
        color.rgb(self.theme, self.styles.palette.as_deref())
    }
}

/// The inlines of the text of `shape`, run by run, each run in `fmt`
/// with the look it sets of its own (sizes times `scale`).
fn shape_inlines(shape: &TextShape, fmt: &RunFmt, scale: f32) -> Vec<Inline> {
    let plain;
    let runs = if shape.runs.is_empty() {
        plain = [ShapeRun {
            text: shape.text.clone(),
            ..ShapeRun::default()
        }];
        &plain[..]
    } else {
        &shape.runs[..]
    };
    let mut inlines = Vec::new();
    for run in runs {
        let mut own = fmt.clone();
        own.bold = run.bold.unwrap_or(own.bold);
        own.italic = run.italic.unwrap_or(own.italic);
        if run.underline == Some(true) {
            own.underline = Some(Underline::Single);
        }
        if let Some(size) = run.size_pt {
            own.size = size * scale;
            own.size_cs = own.size;
        }
        if let Some(color) = run.color {
            own.color = Some(color).filter(|rgb| *rgb != [0; 3]);
        }
        for (index, line) in run.text.split('\n').enumerate() {
            if index > 0 {
                inlines.push(Inline::Break {
                    kind: BreakKind::Line,
                    fmt: own.clone(),
                });
            }
            if !line.is_empty() {
                inlines.push(Inline::Text {
                    text: line.to_string(),
                    fmt: own.clone(),
                });
            }
        }
    }
    inlines
}

/// Excel's default row height, in points.
const DEFAULT_ROW_PT: f32 = 15.0;
/// The most rows of a print area or used range printed.
const MAX_PRINT_ROWS: u32 = 1_048_575;
/// The most columns printed.
const MAX_PRINT_COLS: u32 = 16_383;

/// The band of `rows` of `area` starting at `page_top` on the page.
fn band(area: &Area, rows: &[u32], page_top: f32) -> Band {
    let first = rows.first().copied().unwrap_or(area.range.top);
    let last = rows.last().copied().unwrap_or(first);
    Band {
        rows: rows.to_vec(),
        sheet_top: area.y(first),
        sheet_bottom: area.y(last + 1),
        page_top,
    }
}

/// `items` cut into groups that each fit `room` (by `size`), a new group
/// also at each item of `breaks` (but the first); the first group has
/// `first_room_less` less room, and so does every later one (the rows
/// repeated at the top). A single item larger than the room has a group
/// of its own.
fn groups(items: &[u32], size: impl Fn(u32) -> f32, room: f32, breaks: &HashSet<u32>, repeated: f32) -> Vec<Vec<u32>> {
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut current: Vec<u32> = Vec::new();
    let mut used = 0.0;
    for &item in items {
        let item_size = size(item);
        // A page after the first holds the repeated rows too; the first
        // holds them in their place.
        let room_here = if groups.is_empty() { room } else { room - repeated };
        let slack = (room_here * ROOM_SLACK_RATIO).max(MIN_ROOM_SLACK_PT);
        let full = !current.is_empty() && (used + item_size > room_here + slack || breaks.contains(&item));
        if full {
            groups.push(std::mem::take(&mut current));
            used = 0.0;
        }
        current.push(item);
        used += item_size;
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// What a page's rows or columns may exceed its room by, as a share of
/// it, running into the margin: Excel rounds sizes to pixels, so a sheet
/// made to fill its page (a form at 93 %) may be a point or two larger
/// here.
const ROOM_SLACK_RATIO: f32 = 0.005;
/// The least of that, in points.
const MIN_ROOM_SLACK_PT: f32 = 0.5;

/// The thin grey lines around a cell at `rect`, when the gridlines print.
fn grid_lines(lines: &mut Vec<Item>, [x, y, width, height]: [f32; 4], scale: f32) {
    let line_w = (0.5 * scale).max(0.25);
    for (x1, y1, x2, y2) in [
        (x + width, y, x + width, y + height),
        (x, y + height, x + width, y + height),
    ] {
        lines.push(Item::Line {
            x1,
            y1,
            x2,
            y2,
            width: line_w,
            color: GRID_LINE_COLOR,
            dash: None,
        });
    }
}

/// The picture `index` at `placed` on the page (left, top, width,
/// height), the part of it within `region`; none when no part is.
fn image(index: usize, placed: [f32; 4], region: [f32; 4]) -> Option<Item> {
    let [x, y, width, height] = placed;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    let left = x.max(region[0]);
    let top = y.max(region[1]);
    let right = (x + width).min(region[2]);
    let bottom = (y + height).min(region[3]);
    if right <= left || bottom <= top {
        return None;
    }
    Some(Item::Image {
        index,
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
        crop: [
            (left - x) / width,
            (top - y) / height,
            (x + width - right) / width,
            (y + height - bottom) / height,
        ],
    })
}

/// Moves `items` by `dx` and `dy`.
fn translate(items: &mut [Item], dx: f32, dy: f32) {
    for item in items {
        match item {
            Item::Text(run) => {
                run.x += dx;
                run.y += dy;
            }
            Item::Rect { x, y, .. } | Item::Image { x, y, .. } | Item::Link { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
        }
    }
}

/// `items` cut to the box `clip` (left, top, right, bottom): the glyphs
/// of a run outside it left out, as Excel cuts a cell's text at its edges;
/// a run whose line is outside it left out whole.
fn clip_items(items: Vec<Item>, [left, top, right, bottom]: [f32; 4]) -> Vec<Item> {
    let mut kept = Vec::with_capacity(items.len());
    for mut item in items {
        match &mut item {
            Item::Text(run) => {
                // The line shows when most of it is in the box.
                let middle = run.y - run.size * 0.35;
                if middle < top || middle > bottom {
                    continue;
                }
                let origin = run.x;
                run.glyphs.retain(|glyph| {
                    let start = origin + glyph.x;
                    start >= left - 0.5 && start + glyph.advance <= right + 0.5
                });
                if run.glyphs.is_empty() {
                    continue;
                }
            }
            Item::Line { x1, y1, x2, y2, .. } => {
                *x1 = x1.clamp(left, right);
                *x2 = x2.clamp(left, right);
                if *y1 < top || *y1 > bottom || *x1 == *x2 && *y1 == *y2 {
                    continue;
                }
                *y2 = y2.clamp(top, bottom);
            }
            Item::Rect {
                x, y, width, height, ..
            }
            | Item::Link {
                x, y, width, height, ..
            } => {
                let x2 = (*x + *width).min(right);
                let y2 = (*y + *height).min(bottom);
                *x = x.max(left);
                *y = y.max(top);
                if x2 <= *x || y2 <= *y {
                    continue;
                }
                *width = x2 - *x;
                *height = y2 - *y;
            }
            Item::Image { .. } => {}
        }
        kept.push(item);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_cut_into_pages_by_room_and_breaks() {
        let rows: Vec<u32> = (0..10).collect();
        let size = |_| 10.0;
        let none = HashSet::new();
        assert_eq!(
            groups(&rows, size, 35.0, &none, 0.0),
            [vec![0, 1, 2], vec![3, 4, 5], vec![6, 7, 8], vec![9]]
        );
        // A break starts a page; the first row never breaks.
        let breaks: HashSet<u32> = [0, 5].into();
        assert_eq!(
            groups(&rows, size, 100.0, &breaks, 0.0),
            [vec![0, 1, 2, 3, 4], vec![5, 6, 7, 8, 9]]
        );
        // Rows repeated at the top take room on every page but the first.
        assert_eq!(
            groups(&rows, size, 40.0, &none, 20.0),
            [vec![0, 1, 2, 3], vec![4, 5], vec![6, 7], vec![8, 9]]
        );
        // A row taller than a page has a page of its own.
        assert_eq!(groups(&[0, 1], |_| 50.0, 30.0, &none, 0.0), [vec![0], vec![1]]);
    }

    #[test]
    fn a_picture_across_the_edge_of_a_page_is_cropped() {
        let Some(Item::Image {
            x,
            y,
            width,
            height,
            crop,
            ..
        }) = image(3, [80.0, 10.0, 40.0, 20.0], [0.0, 0.0, 100.0, 100.0])
        else {
            panic!("a picture")
        };
        assert_eq!((x, y, width, height), (80.0, 10.0, 20.0, 20.0));
        assert_eq!(crop, [0.0, 0.0, 0.5, 0.0]);
        assert!(image(3, [120.0, 10.0, 40.0, 20.0], [0.0, 0.0, 100.0, 100.0]).is_none());
    }

    #[test]
    fn text_is_cut_at_its_clip() {
        let glyph = |x: f32| crate::layout::display::Glyph {
            id: 1,
            x,
            advance: 5.0,
            dx: 0.0,
            dy: 0.0,
            text: (0, 1),
        };
        let run = crate::layout::display::TextRun {
            face: crate::layout::fonts::FaceId::default(),
            size: 10.0,
            x: 0.0,
            y: 20.0,
            color: [0; 3],
            glyphs: vec![glyph(0.0), glyph(5.0), glyph(10.0)],
            text: "abc".into(),
            fake_bold: false,
            fake_italic: false,
            scale: 1.0,
        };
        let kept = clip_items(vec![Item::Text(run.clone())], [0.0, 0.0, 10.0, 30.0]);
        let [Item::Text(cut)] = kept.as_slice() else {
            panic!("one run")
        };
        assert_eq!(cut.glyphs.len(), 2);
        // A line outside the box is left out.
        assert!(clip_items(vec![Item::Text(run)], [0.0, 30.0, 10.0, 60.0]).is_empty());
    }
}
