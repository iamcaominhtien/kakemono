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
use super::header;
use crate::chart::{Rgb, Theme};
use crate::excel::drawing::{DEFAULT_INSETS_PT, Drawn, Shape, ShapeRun, TextShape};
use crate::excel::grid::{CellKind, DEFAULT_COL_PT, Grid, GridCell, Marker, Merge, OwnFont, Sizes};
use crate::excel::styles::{Color, Line, Look, Styles, VAlign};
use crate::excel::view::{alignment, col_width, column_name, look_index, object_box, shape_svg, text_insets};
use crate::excel::{DrawnSheet, SheetBook};
use crate::model::style::{BOLD, ITALIC, SUB, SUP};
use crate::word::fmt::model::{
    Align, BreakKind, FDoc, FImage, Inline, LineSpacing, Para, ParaProps, RunFmt, Underline, VertAlign,
};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// The first code point of the wide letters (CJK radicals onwards) that
/// a distributed text spreads one by one, as words.
const WIDE_LETTERS_FROM: u32 = 0x2E80;
/// US Letter, Excel's paper when a sheet names none, in points.
const LETTER_PT: (f32, f32) = (612.0, 792.0);
/// Excel's default font when the styles name none.
const DEFAULT_FONT: &str = "Calibri";
/// Excel's default font size, in points.
const DEFAULT_SIZE_PT: f32 = 11.0;
/// Half an em: the width of a digit of `HALF_EM_DIGIT_FONTS`.
const HALF_EM: f32 = 0.5;
/// The East Asian fonts whose digits print half an em wide (their names as
/// `table_name` folds them).
const HALF_EM_DIGIT_FONTS: &[&str] = &[
    "ms ゴシック",
    "ms pゴシック",
    "ms 明朝",
    "ms gothic",
    "ms pgothic",
    "ms mincho",
];
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
/// The width of the thinnest border (`hair`), and the least of any border,
/// on paper, in points.
const HAIR_LINE_PT: f32 = 0.25;
/// The colour of printed gridlines.
const GRID_LINE_COLOR: Rgb = [0xBF, 0xBF, 0xBF];
/// The fewest digits the printed row numbers make room for.
const MIN_HEADING_DIGITS: usize = 3;
/// The room around the printed row numbers, in points of the sheet.
const HEADING_PAD_PT: f32 = 4.0;
/// The most empty columns a text runs over, as the sheet view.
const MAX_OVERFLOW_COLS: usize = 40;
/// How near a text's clip is to the page's right edge to be cut there, in
/// points.
const PAGE_EDGE_SLACK_PT: f32 = 0.01;
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
/// sheet prints anything. A header's `&F` prints `file`, its file name.
///
/// # Errors
/// When the PDF cannot be written.
pub(crate) fn to_pdf(book: &SheetBook, config: &WorkbookPdfConfig, file: Option<&str>) -> Result<Vec<u8>> {
    let mut fonts = FontBook::new(&[], &HashMap::new());
    fonts.require_font()?;
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
            file: file.unwrap_or_default(),
            sizes: sheet.grid.sizes.clone(),
            placing: None,
            page_right: f32::INFINITY,
            repeats_cols: false,
        };
        printer.printed_columns();
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
    pdf::write(pages, &[], &fonts, &images, book.title.as_deref())
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
    /// The name of the workbook, as `&F` prints it.
    file: &'a str,
    /// The sizes of the sheet as it prints: its columns as wide as the
    /// printer draws them (`printed_columns`).
    sizes: Sizes,
    /// The sizes of the sheet with its rows fitted to their text and its
    /// columns as they print, where its drawn objects are placed (they
    /// move with their cells); `None` when the sheet's own sizes are.
    placing: Option<Grid>,
    /// The right edge of the columns of the page drawn: a text cut there
    /// keeps the letter that crosses it, whose rest the next page's
    /// columns leave out.
    page_right: f32,
    /// The page drawn repeats the print title columns at its left: their
    /// texts do not run on into its other columns.
    repeats_cols: bool,
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
    /// The columns repeated at the left of each page.
    title_cols: Vec<u32>,
    /// Their width on the sheet.
    titles_w: f32,
    /// The width of the row numbers and the height of the column letters
    /// printed (`printOptions headings`) on the sheet; 0 when they are
    /// not.
    headings: (f32, f32),
}

/// The scale a sheet of `area` prints at with the settings `print` on
/// pages of `room` (width, height within the margins), with its printed
/// `headings` (width, height): fitted to its pages (never larger than
/// 100 %), else its own scale.
fn scale(print: &crate::excel::grid::PrintSetup, area: &Area, room: (f32, f32), headings: (f32, f32)) -> f32 {
    if !print.fit_to_page {
        return print.scale_percent / 100.0;
    }
    let total_w: f32 = area.cols.iter().map(|&col| area.width(col)).sum::<f32>() + headings.0;
    let total_h: f32 = area.rows.iter().map(|&row| area.height(row)).sum::<f32>() + headings.1;
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
    /// Sets the columns as wide as the printer draws them. Excel keeps a
    /// column's width in digits of the default font; the screen measures a
    /// digit in whole pixels (an 11-point MS P Gothic's 7.33 as 8), the
    /// printer at its own resolution, nearly exactly, so a column prints
    /// narrower or wider than the screen shows it (Microsoft's KB 214394).
    /// The digit is half an em in MS Gothic or MS Mincho, measured in a
    /// Western default font when it (or a font of its metrics) is
    /// installed; the columns are left as the screen draws them otherwise
    /// (the scales Excel stores for sheets fitted to a page agree with the
    /// printer for Calibri, Arial and MS P Gothic, with the screen for Yu
    /// Gothic and MS P Mincho).
    fn printed_columns(&mut self) {
        let screen_digit = self.sizes.screen_digit_pt;
        let font = &self.styles.look(0).font;
        let size = font.size_pt.unwrap_or(DEFAULT_SIZE_PT);
        let name = font.name.as_deref().map(crate::layout::fonts::table_name);
        let digit = if name.as_deref().is_some_and(|name| HALF_EM_DIGIT_FONTS.contains(&name)) {
            HALF_EM * size
        } else if self.styles.east_asian_default() {
            return;
        } else if font.name.as_deref().is_some_and(|name| self.ctx.book.has_family(name)) {
            let base = self.run(self.styles.look(0), 1.0);
            para::text_items(self.ctx, "0", &base, 0.0, 0.0).1
        } else {
            return;
        };
        if screen_digit <= 0.0 || digit <= 0.0 {
            return;
        }
        let ratio = digit / screen_digit;
        for (_, _, width) in &mut self.sizes.cols {
            *width *= ratio;
        }
        self.sizes.default_col_pt = Some(self.sizes.default_col_pt.unwrap_or(DEFAULT_COL_PT) * ratio);
    }

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
        let (titles, title_cols) = self.titles(&area);
        let titles_h: f32 = titles.iter().map(|&row| area.height(row)).sum();
        let titles_w: f32 = title_cols.iter().map(|&col| area.width(col)).sum();
        let headings = if print.headings {
            self.headings(&area)
        } else {
            (0.0, 0.0)
        };
        let scale = scale(print, &area, room, headings);
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
            room.0 / scale - headings.0,
            &keep(print.fit_width, &print.col_breaks),
            titles_w,
        );
        let row_groups = groups(
            &area.rows,
            |row| area.height(row),
            room.1 / scale - headings.1,
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
            title_cols,
            titles_w,
            headings,
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
        // A page with nothing to print is left out, as Excel does.
        let mut pages: Vec<Page> = order
            .into_iter()
            .map(|(rows, cols)| self.page(&area, &layout, rows, cols))
            .filter(|page| !page.items.is_empty())
            .take(max)
            .collect();
        self.headers_and_footers(&mut pages, &layout);
        pages
    }

    /// The print title rows and columns of the sheet that print in `area`.
    fn titles(&self, area: &Area) -> (Vec<u32>, Vec<u32>) {
        let within = |range: Option<(u32, u32)>, items: &[u32]| -> Vec<u32> {
            range
                .map(|(first, last)| {
                    items
                        .iter()
                        .copied()
                        .filter(|item| (first..=last).contains(item))
                        .collect()
                })
                .unwrap_or_default()
        };
        let declared = &self.grid.declared;
        (
            within(declared.print_title_rows, &area.rows),
            within(declared.print_title_cols, &area.cols),
        )
    }

    /// Draws the headers and footers of the sheet's `pages`, numbered from
    /// 1.
    fn headers_and_footers(&mut self, pages: &mut [Page], layout: &Layout) {
        let (date, time) = header::now();
        let count = pages.len();
        for (index, page) in pages.iter_mut().enumerate() {
            let fields = header::Fields {
                page: index + 1,
                pages: count,
                sheet: &self.sheet.name,
                file: self.file,
                date: &date,
                time: &time,
            };
            self.header_footer(&mut page.items, layout.paper, layout.scale, &fields);
        }
    }

    /// Draws the header and footer of the page that `fields` describes, on
    /// `paper`: each section's lines at the left margin, the centre and
    /// the right margin, the header's top at its distance from the top
    /// of the paper and the footer's foot at its distance from the foot,
    /// in the workbook's default font, scaled with the sheet (`scale`)
    /// unless they are set not to be.
    fn header_footer(&mut self, items: &mut Vec<Item>, paper: (f32, f32), scale: f32, fields: &header::Fields<'_>) {
        let print = &self.grid.print;
        let setup = &print.header_footer;
        let (head, foot) = setup.of_page(fields.page);
        let scale = if setup.scale_with_doc { scale } else { 1.0 };
        let [margin_l, margin_r, ..] = print.margins_pt;
        let (left, right) = if setup.align_with_margins {
            (margin_l, paper.0 - margin_r)
        } else {
            (print.header_pt, paper.0 - print.header_pt)
        };
        let (header_pt, footer_pt) = (print.header_pt, print.footer_pt);
        for (code, top_down) in [(head.to_string(), true), (foot.to_string(), false)] {
            if code.is_empty() {
                continue;
            }
            for (index, lines) in header::sections(&code, fields).iter().enumerate() {
                let mut placed = Vec::new();
                for line in lines {
                    placed.push(self.header_line(line, scale));
                }
                let total: f32 = placed.iter().map(|(_, _, height)| height).sum();
                let mut top = if top_down {
                    header_pt
                } else {
                    paper.1 - footer_pt - total
                };
                for (mut line_items, width, height) in placed {
                    let x = match index {
                        0 => left,
                        1 => (paper.0 - width) / 2.0,
                        _ => right - width,
                    };
                    translate(&mut line_items, x, top);
                    items.append(&mut line_items);
                    top += height;
                }
            }
        }
    }

    /// The items of a line of a header at `scale`, from its top left, with
    /// its width and height.
    fn header_line(&mut self, line: &[header::Piece], scale: f32) -> (Vec<Item>, f32, f32) {
        let fmts: Vec<RunFmt> = line
            .iter()
            .map(|piece| {
                let mut look = self.styles.look(0).clone();
                let font = &mut look.font;
                font.name = piece.fmt.font.clone().or(font.name.take());
                font.size_pt = piece.fmt.size_pt.or(font.size_pt);
                font.bold = piece.fmt.bold;
                font.italic = piece.fmt.italic;
                font.underline = piece.fmt.underline;
                font.strike = piece.fmt.strike;
                let mut fmt = self.run(&look, scale);
                if piece.fmt.color.is_some() {
                    fmt.color = piece.fmt.color.filter(|rgb| *rgb != [0; 3]);
                }
                fmt
            })
            .collect();
        let (mut ascent, mut descent) = (0.0_f32, 0.0_f32);
        for fmt in &fmts {
            let (above, below, _) = self.ctx.line_metrics(fmt);
            ascent = ascent.max(above);
            descent = descent.max(below);
        }
        let mut items = Vec::new();
        let mut pen = 0.0;
        for (piece, fmt) in line.iter().zip(&fmts) {
            let (mut run, run_w) = para::text_items(self.ctx, &piece.text, fmt, pen, ascent);
            items.append(&mut run);
            pen += run_w;
        }
        (items, pen, ascent + descent)
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
        // The repeated columns lead every page whose columns come after
        // them.
        let repeat_cols = layout
            .title_cols
            .last()
            .is_some_and(|last| cols.first().is_some_and(|first| first > last));
        let (heading_w, heading_h) = layout.headings;
        let repeated_w = if repeat_cols { layout.titles_w } else { 0.0 } + heading_w;
        let content_w: f32 = (cols.iter().map(|&col| area.width(col)).sum::<f32>() + repeated_w) * scale;
        let repeated_h = if repeat { layout.titles_h } else { 0.0 } + heading_h;
        let content_h: f32 = (rows.iter().map(|&row| area.height(row)).sum::<f32>() + repeated_h) * scale;
        let centred = |on: bool, room: f32, content: f32| if on { (room - content).max(0.0) / 2.0 } else { 0.0 };
        let heading_left = layout.origin.0 + centred(print.h_centered, layout.room.0, content_w);
        let heading_top = layout.origin.1 + centred(print.v_centered, layout.room.1, content_h);
        let left = heading_left + heading_w * scale;
        let top = heading_top + heading_h * scale;
        let columns_of = |cols: &[u32], page_left: f32| Columns {
            cols: cols.to_vec(),
            sheet_left: area.x(cols[0]),
            sheet_right: area.x(*cols.last().unwrap_or(&cols[0]) + 1),
            page_left,
        };
        let columns = columns_of(cols, left + (repeated_w - heading_w) * scale);
        self.repeats_cols = repeat_cols;
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
        let mut all_columns = Vec::new();
        if repeat_cols {
            let titles = columns_of(&layout.title_cols, left);
            self.draw_page(&mut page.items, area, &titles, &bands, scale, repeat);
            all_columns.push(titles);
        }
        self.draw_page(&mut page.items, area, &columns, &bands, scale, repeat);
        all_columns.push(columns);
        if heading_w > 0.0 {
            self.draw_headings(
                &mut page.items,
                area,
                (&all_columns, &bands),
                (heading_left, heading_top),
                scale,
            );
        }
        page
    }

    /// The width of the row numbers and the height of the column letters
    /// Excel prints for `area` (`printOptions headings`), on the sheet: as
    /// wide as the digits of its last row (three at least) and as high as
    /// a row of the default height.
    fn headings(&self, area: &Area) -> (f32, f32) {
        let last = area.rows.last().copied().unwrap_or(0) + 1;
        let digits = last.to_string().len().max(MIN_HEADING_DIGITS);
        let digit = if self.sizes.screen_digit_pt > 0.0 {
            self.sizes.screen_digit_pt
        } else {
            HALF_EM * DEFAULT_SIZE_PT
        };
        (
            digits as f32 * digit + HEADING_PAD_PT,
            self.sizes.default_row_pt.unwrap_or(DEFAULT_ROW_PT),
        )
    }

    /// Draws the printed headings of a page: the letters of `columns`
    /// above them and the numbers of the rows of `bands` at their left,
    /// each in a box, with the corner box at `(left, top)`.
    fn draw_headings(
        &mut self,
        items: &mut Vec<Item>,
        area: &Area,
        (columns, bands): (&[Columns], &[Band]),
        (left, top): (f32, f32),
        scale: f32,
    ) {
        let Some(first) = columns.first() else { return };
        let width = first.page_left - left;
        let height = bands.first().map_or(0.0, |band| band.page_top) - top;
        let base = self.run(self.styles.look(0), scale);
        let (ascent, descent, _) = self.ctx.line_metrics(&base);
        let mut boxes = vec![([left, top, width, height], String::new())];
        for group in columns {
            for &col in &group.cols {
                let x = group.page_left + (area.x(col) - group.sheet_left) * scale;
                boxes.push(([x, top, area.width(col) * scale, height], column_name(col)));
            }
        }
        for band in bands {
            for &row in &band.rows {
                let y = band.page_top + (area.y(row) - band.sheet_top) * scale;
                boxes.push(([left, y, width, area.height(row) * scale], (row + 1).to_string()));
            }
        }
        for ([x, y, box_w, box_h], label) in boxes {
            if box_w <= 0.0 || box_h <= 0.0 {
                continue;
            }
            outline(items, [x, y, box_w, box_h], scale);
            let (mut run, run_w) = para::text_items(self.ctx, &label, &base, 0.0, ascent);
            translate(
                &mut run,
                x + (box_w - run_w) / 2.0,
                y + (box_h - ascent - descent) / 2.0,
            );
            items.append(&mut run);
        }
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
        let start = self.sizes.span_pt(
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
                col_width(&self.sizes, col)
            };
            xs.push(xs[xs.len() - 1] + width);
        }
        let ys = self.row_edges(range, &xs, start.1);
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

    /// The top edge of each row of `range` (column edges `xs`) from
    /// `top`, and the bottom of the last: rows fitted to their text where
    /// Excel fits them, the drawn objects then placed on those heights.
    fn row_edges(&mut self, range: Merge, xs: &[f32], top: f32) -> Vec<f32> {
        let grid = self.grid;
        let default_row = grid.sizes.default_row_pt.unwrap_or(DEFAULT_ROW_PT);
        let mut ys = vec![top];
        let mut fitted_rows = Vec::new();
        for row in range.top..=range.bottom {
            let height = if grid.hidden_rows.contains(&row) {
                0.0
            } else {
                let height = grid.sizes.rows.get(&row).copied().unwrap_or(default_row);
                if grid.looks.custom_rows.contains(&row) {
                    height
                } else {
                    let fitted = self.fitted_height(row, range, xs, height);
                    if fitted > height {
                        fitted_rows.push((row, fitted));
                    }
                    fitted
                }
            };
            ys.push(ys[ys.len() - 1] + height);
        }
        if !fitted_rows.is_empty() || self.sizes != grid.sizes {
            let mut sizes = self.sizes.clone();
            sizes.rows.extend(fitted_rows);
            self.placing = Some(Grid {
                sizes,
                hidden_rows: grid.hidden_rows.clone(),
                hidden_cols: grid.hidden_cols.clone(),
                ..Grid::default()
            });
        }
        ys
    }

    /// The height of the row `row`, `height` high in the file, fitted to
    /// its wrapped text as Excel fits a row whose height is not set by
    /// hand: taller when a cell of the range `range` (column edges `xs`)
    /// needs more lines in the font set in its place than the row holds at
    /// Excel's line height, never shorter. Lines are counted, not
    /// measured: a font whose lines are taller than Excel's (a Japanese
    /// stand-in) does not grow a row whose lines are as many. A merged
    /// cell does not count, as in Excel.
    fn fitted_height(&mut self, row: u32, range: Merge, xs: &[f32], height: f32) -> f32 {
        let grid = self.grid;
        let base_size = self.styles.look(0).font.size_pt.unwrap_or(DEFAULT_SIZE_PT);
        let base_line = grid.sizes.default_row_pt.unwrap_or(DEFAULT_ROW_PT) / base_size;
        let mut fitted = height;
        for (&(_, col), cell) in grid.cells.range((row, range.left)..=(row, range.right)) {
            let look = self.styles.look(look_index(grid, (row, col)) as usize);
            if !(look.wraps())
                || look.stacked
                || look.rotation.is_some()
                || grid
                    .merges
                    .iter()
                    .any(|merge| (merge.top..=merge.bottom).contains(&row) && (merge.left..=merge.right).contains(&col))
            {
                continue;
            }
            let index = (col - range.left) as usize;
            let width = xs[index + 1] - xs[index];
            let base = self.run(look, 1.0);
            let indent = f32::from(look.indent) * INDENT_STEP_PT;
            let inner_w = (width - 2.0 * CELL_PADDING_PT - indent).max(1.0);
            let align = alignment(look.h_align, Some(cell.kind));
            let lines = para::layout(self.ctx, &self.wrapped_para(cell, &base, align), inner_w)
                .lines
                .len();
            let line = base_line * look.font.size_pt.unwrap_or(base_size);
            // The lines the row held as Excel set it, and those it lacks.
            #[expect(clippy::cast_precision_loss, reason = "a cell's lines are few")]
            let lacking = lines as f32 - (height / line).round().max(1.0);
            if lacking > 0.0 {
                fitted = fitted.max(height + lacking * line);
            }
        }
        fitted
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
            let mut room = col_width(&self.sizes, col) - 2.0 * CELL_PADDING_PT - indent;
            let mut next = col;
            while room < text_w && next < col.saturating_add(MAX_OVERFLOW_COLS as u32) {
                next += 1;
                if !self.grid.hidden_cols.contains(&next) {
                    room += col_width(&self.sizes, next);
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
        self.page_right = columns.page_left + (columns.sheet_right - columns.sheet_left) * scale;
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
                // A merged range cut by a page break shows only its part
                // on the page.
                let region = span.map(|_| page_region(columns, band, scale));
                let keep = |item: Item| match region {
                    Some(region) => cut(item, region),
                    None => Some(item),
                };
                if let Some(fill) = look.fill.and_then(|color| self.cell_rgb(color)) {
                    items.extend(keep(Item::Rect {
                        x: rect[0],
                        y: rect[1],
                        width: rect[2],
                        height: rect[3],
                        color: fill,
                    }));
                }
                items.extend(self.data_bar(*at, rect, scale).and_then(keep));
                let mut edges = Vec::new();
                if self.grid.print.grid_lines {
                    grid_lines(&mut edges, rect, scale);
                }
                self.borders(&mut edges, look, rect, scale);
                lines.extend(edges.into_iter().filter_map(keep));
                // A text centred across the selection is set in its box.
                let position = columns.cols.iter().position(|col| *col == at.1).unwrap_or(0);
                let mut text_rect = rect;
                text_rect[2] += self
                    .grid
                    .selection_width(*at, &columns.cols[position + 1..], |col| area.width(col))
                    * scale;
                let clip = self.clip(area, columns, band, *at, *span, rect, scale, look);
                texts.extend(self.cell_text(*at, look, text_rect, clip, scale));
            }
            texts.extend(self.spilled_in(area, columns, band, scale));
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

    /// The texts of cells left of the page's columns that run over the
    /// empty cells up to them and on into the page, as Excel prints the
    /// rest of a long title on the page of the next columns.
    fn spilled_in(&mut self, area: &Area, columns: &Columns, band: &Band, scale: f32) -> Vec<Item> {
        let Some(&first) = columns.cols.first() else {
            return Vec::new();
        };
        let before = area.cols.iter().position(|&col| col == first).unwrap_or(0);
        let page_left = columns.page_left;
        let titles = self.grid.declared.print_title_cols.filter(|_| self.repeats_cols);
        let mut items = Vec::new();
        for &row in &band.rows {
            let free = |col: u32| self.grid.is_blank((row, col)) && !self.grid.merged((row, col));
            let Some(&from) = area.cols[..before]
                .iter()
                .rev()
                .take(MAX_OVERFLOW_COLS)
                .find(|&&col| !free(col))
            else {
                continue;
            };
            if titles.is_some_and(|(first, last)| (first..=last).contains(&from)) {
                continue;
            }
            let at = (row, from);
            let look = self.styles.look(look_index(self.grid, at) as usize);
            let cell = self.grid.cells.get(&at);
            let left_aligned = alignment(look.h_align, cell.map(|cell| cell.kind)) == "left";
            let runs_over = left_aligned
                && !look.wrap
                && !self.grid.merged(at)
                && !cell.is_some_and(|cell| cell.center_continuous)
                && cell.is_some_and(|cell| cell.kind == CellKind::Text);
            if !runs_over || !free(first) {
                continue;
            }
            let reach: f32 = columns
                .cols
                .iter()
                .take_while(|&&col| free(col))
                .take(MAX_OVERFLOW_COLS)
                .map(|&col| area.width(col))
                .sum();
            let rect = self.to_page(
                [area.x(from), area.y(row), area.width(from), area.height(row)],
                columns,
                band,
                scale,
            );
            let clip = [page_left, rect[1], page_left + reach * scale, rect[1] + rect[3]];
            let look = look.clone();
            items.extend(self.cell_text(at, &look, rect, clip, scale));
        }
        items
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
    /// columns, with the selection a text is centred across; for a text not
    /// wrapped, also the empty cells beside it that it runs over, by its
    /// alignment.
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
        let position = columns.cols.iter().position(|&col| col == at.1).unwrap_or(0);
        // A text centred across the selection shows over all of it.
        let selection = self
            .grid
            .selection_width(at, &columns.cols[position + 1..], |col| area.width(col));
        right += selection * scale;
        let runs_over =
            span.is_none() && selection == 0.0 && !look.wrap && cell.is_some_and(|cell| cell.kind == CellKind::Text);
        if runs_over {
            let free = |col: u32| self.grid.is_blank((at.0, col)) && !self.grid.merged((at.0, col));
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

    /// The data bar of the cell at `at` (box `rect` on the page), in the
    /// middle of its shading from the colour to white, within the cell's
    /// edges; `None` when it has none.
    fn data_bar(&self, at: (u32, u32), rect: [f32; 4], scale: f32) -> Option<Item> {
        let bar = self.grid.looks.bars.get(&at)?;
        let color = self.cell_rgb(bar.color)?;
        let inset = DATA_BAR_INSET_PT * scale;
        Some(Item::Rect {
            x: rect[0] + inset / 2.0,
            y: rect[1] + inset,
            width: ((rect[2] - inset) * bar.length).max(0.0),
            height: (rect[3] - 2.0 * inset).max(0.0),
            color: color.map(|channel| channel + (u8::MAX - channel) / 4),
        })
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
            let color = edge.color.and_then(|color| self.cell_rgb(color)).unwrap_or([0; 3]);
            let line_w = if edge.line == Line::Hair {
                HAIR_LINE_PT
            } else {
                (edge.width_px * PT_PER_PX * scale.max(0.5)).max(HAIR_LINE_PT)
            };
            let dash = match edge.line {
                Line::Dashed => Some(line_w * 3.0),
                Line::Dotted => Some(line_w),
                Line::Solid | Line::Hair | Line::Double => None,
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
            let room = if look.stacked && look.wrap {
                height - 2.0 * pad_top
            } else {
                f32::INFINITY
            };
            self.stacked_text(cell, &base, (width, room), (ascent, descent))
        } else if let Some(filled) = self.filled_text(at, cell, &base, (pad + indent, width - pad - indent), ascent) {
            // A number's fill sets it across the cell, whatever its
            // alignment and wrapping.
            (filled, ascent + descent)
        } else if align == "distributed" {
            self.distributed_text(cell, &base, (pad + indent, inner_w - indent), (ascent, descent))
        } else if look.wraps() {
            self.wrapped_text(cell, &base, align, pad, indent, inner_w)
        } else {
            let mut items = Vec::new();
            let mut pen = 0.0;
            for (index, span) in cell.spans.iter().enumerate() {
                let fmt = self.span_run(&base, span, cell.own_font(index));
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
        let at_page_edge = (clip[2] - self.page_right).abs() < PAGE_EDGE_SLACK_PT;
        clip_items(items, clip, at_page_edge)
    }

    /// The text of the number `cell` at `at` whose format has a fill
    /// (`*x`), set in `base`: the text before the fill from `left`, the
    /// text after it ending at `right`, as Excel sets an accounting format.
    /// `None` when it has no fill.
    fn filled_text(
        &mut self,
        at: (u32, u32),
        cell: &GridCell,
        base: &RunFmt,
        (left, right): (f32, f32),
        ascent: f32,
    ) -> Option<Vec<Item>> {
        let fill = *self.grid.looks.fills.get(&at)?;
        let [span] = cell.spans.as_slice() else {
            return None;
        };
        // The text has a space for the fill (`$ -`).
        let (before, after) = (span.text.get(..fill)?, span.text.get(fill..)?.trim_start());
        let fmt = self.span_run(base, span, cell.own_font(0));
        let (mut items, _) = para::text_items(self.ctx, before, &fmt, 0.0, ascent);
        let (mut tail, tail_w) = para::text_items(self.ctx, after, &fmt, 0.0, ascent);
        translate(&mut items, left, 0.0);
        translate(&mut tail, right - tail_w, 0.0);
        items.append(&mut tail);
        Some(items)
    }

    /// The text of `cell` a letter a line, centred in a cell `width`
    /// wide, set in `base` whose lines have `metrics` (ascent, descent):
    /// turned text is set so, as stacked text is. Letters that pass `room`
    /// (the height of a wrapped cell) go on in a new column to the left,
    /// as a line break does; spaces at the foot of a column hang. Its
    /// items from the top left of the cell, and its height.
    fn stacked_text(
        &mut self,
        cell: &GridCell,
        base: &RunFmt,
        (width, room): (f32, f32),
        metrics: (f32, f32),
    ) -> (Vec<Item>, f32) {
        let (ascent, descent) = metrics;
        let pitch = ascent + descent;
        let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
        let columns = if room.is_finite() {
            stacked_columns(&text, ((room / pitch).floor() as usize).max(1))
        } else {
            vec![text.chars().filter(|ch| !ch.is_whitespace()).collect()]
        };
        let mut items = Vec::new();
        let mut text_h: f32 = 0.0;
        let first_x = (width + (columns.len() as f32 - 1.0) * pitch) / 2.0;
        for (number, column) in columns.iter().enumerate() {
            let middle = first_x - number as f32 * pitch;
            let mut line_y = ascent;
            for ch in column {
                if !ch.is_whitespace() {
                    let (mut run, run_w) = para::text_items(self.ctx, &ch.to_string(), base, 0.0, line_y);
                    translate(&mut run, middle - run_w / 2.0, 0.0);
                    items.append(&mut run);
                }
                line_y += pitch;
            }
            text_h = text_h.max(line_y - ascent);
        }
        (items, text_h)
    }

    /// The text of `cell` spread over `room` points from `left`, as
    /// Excel distributes a text: its words, its spaces and each of its
    /// Japanese and Chinese letters apart by even gaps, the first at the
    /// left and the last at the right; one word centred. A text wider
    /// than `room` is wrapped, and every line is spread so; a line feed
    /// starts a line. Its items from the top of the cell, and its height.
    fn distributed_text(
        &mut self,
        cell: &GridCell,
        base: &RunFmt,
        (left, room): (f32, f32),
        (ascent, descent): (f32, f32),
    ) -> (Vec<Item>, f32) {
        let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
        let mut items = Vec::new();
        let mut line_top = 0.0;
        for paragraph in text.trim().split('\n') {
            let runs: Vec<(Vec<Item>, f32, bool)> = distributed_units(paragraph)
                .iter()
                .map(|unit| {
                    let (run, run_w) = para::text_items(self.ctx, unit, base, 0.0, ascent);
                    (run, run_w, unit.trim().is_empty())
                })
                .collect();
            for line in fill_lines(runs, room) {
                let used: f32 = line.iter().map(|(_, run_w, _)| run_w).sum();
                let gap = if line.len() > 1 {
                    (room - used) / (line.len() - 1) as f32
                } else {
                    0.0
                };
                let mut pen = if line.len() > 1 {
                    left
                } else {
                    left + (room - used) / 2.0
                };
                for (mut run, run_w, _) in line {
                    translate(&mut run, pen, line_top);
                    items.append(&mut run);
                    pen += run_w + gap;
                }
                line_top += ascent + descent;
            }
        }
        (items, line_top.max(ascent + descent))
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
        let para = self.wrapped_para(cell, base, align);
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

    /// The text of `cell` as a paragraph set in `base`, aligned `align`.
    fn wrapped_para(&self, cell: &GridCell, base: &RunFmt, align: &str) -> Para {
        Para {
            props: ParaProps {
                align: match align {
                    "center" => Align::Center,
                    "right" => Align::Right,
                    "justify" => Align::Both,
                    // Every line spread across the cell, as Excel does.
                    "distributed" => Align::Distribute,
                    _ => Align::Left,
                },
                line: LineSpacing::Auto(1.0),
                ..Default::default()
            },
            mark: base.clone(),
            inlines: self.inlines(cell, base),
            ..Default::default()
        }
    }

    /// The inlines of a cell's text for a wrapped paragraph: a line feed
    /// is a line break.
    fn inlines(&self, cell: &GridCell, base: &RunFmt) -> Vec<Inline> {
        let mut inlines = Vec::new();
        for (index, span) in cell.spans.iter().enumerate() {
            let fmt = self.span_run(base, span, cell.own_font(index));
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
        // A Japanese font sets the whole text (its Latin letters and
        // digits in a Western font of its kind when it is not installed);
        // a Western one leaves its Japanese letters to a Japanese family.
        let latin = self.ctx.book.latin_stand_in(&name);
        let east_asia = if name.is_ascii() && latin.is_none() {
            self.cjk.to_string()
        } else {
            name.clone()
        };
        let western = latin.map_or_else(|| name.clone(), ToString::to_string);
        let size = font.size_pt.or(base.size_pt).unwrap_or(DEFAULT_SIZE_PT) * scale;
        RunFmt {
            font: western.clone(),
            font_hansi: western.clone(),
            font_east_asia: east_asia,
            font_cs: western,
            size,
            size_cs: size,
            bold: font.bold,
            italic: font.italic,
            underline: font.underline.then_some(Underline::Single),
            strike: font.strike,
            color: font
                .color
                .and_then(|color| self.cell_rgb(color))
                .filter(|rgb| *rgb != [0; 3]),
            ..RunFmt::default()
        }
    }

    /// The run of a span of a cell set in `base`: its bold, italic,
    /// underline, superscript, colour and link (of the schemes a PDF may
    /// open): those of the cell's look and the span's own, or the span's
    /// alone when it is a rich run that sets its own (`own`; a colour,
    /// black too).
    fn span_run(&self, base: &RunFmt, span: &crate::model::Span, own: OwnFont) -> RunFmt {
        let mut fmt = base.clone();
        if own.style {
            fmt.bold = span.style & BOLD != 0;
            fmt.italic = span.style & ITALIC != 0;
            fmt.underline = own.underline.then_some(Underline::Single);
        } else {
            fmt.bold |= span.style & BOLD != 0;
            fmt.italic |= span.style & ITALIC != 0;
        }
        if span.style & SUP != 0 {
            fmt.vert = VertAlign::Super;
        } else if span.style & SUB != 0 {
            fmt.vert = VertAlign::Sub;
        }
        if self.grid.print.black_and_white {
            fmt.color = None;
        } else if span.color != [0; 3] {
            fmt.color = Some(span.color);
        } else if own.color {
            fmt.color = None;
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
        let Some(sheet_box) = object_box(self.placing.as_ref().unwrap_or(self.grid), shape) else {
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
        let [x, y, width, height] = rect;
        let [left, top_inset, right, bottom_inset] = text_insets(shape, width / scale, height / scale)
            .unwrap_or(DEFAULT_INSETS_PT)
            .map(|inset| inset * scale);
        let inlines = shape_inlines(shape, &fmt, scale);
        let align = match shape.align.as_deref() {
            Some("ctr") => Align::Center,
            Some("r") => Align::Right,
            Some("just" | "dist") => Align::Both,
            _ => Align::Left,
        };
        let para = Para {
            props: ParaProps {
                align: if shape.nowrap { Align::Left } else { align },
                line: LineSpacing::Auto(1.0),
                ..Default::default()
            },
            mark: fmt,
            inlines,
            ..Default::default()
        };
        // A text that is not wrapped is set on lines as long as they are,
        // each placed in the box as its alignment says.
        let inner = (width - left - right).max(1.0);
        let laid = para::layout(self.ctx, &para, if shape.nowrap { NOWRAP_WIDTH_PT } else { inner });
        let mut items = Vec::new();
        let mut line_top = 0.0;
        for mut line in laid.lines {
            let shift = if shape.nowrap {
                let end = line_end(&line.items);
                match align {
                    Align::Center => (inner - end) / 2.0,
                    Align::Right => inner - end,
                    _ => 0.0,
                }
            } else {
                0.0
            };
            translate(&mut line.items, shift, line_top);
            items.append(&mut line.items);
            line_top += line.height;
        }
        let top = match shape.anchor.as_deref() {
            Some("ctr") => (height - line_top) / 2.0,
            Some("b") => height - bottom_inset - line_top,
            _ => top_inset,
        };
        translate(&mut items, x + left, y + top);
        // A shape that cuts its text keeps the lines whose baseline is
        // within its edges.
        let [clip_top, clip_bottom] = if shape.clip {
            items.retain(|item| !matches!(item, Item::Text(run) if run.y > y + height));
            [region[1].max(y), region[3].min(y + height)]
        } else {
            [region[1], region[3]]
        };
        clip_items(items, [region[0], clip_top, region[2], clip_bottom], false)
    }

    /// A colour of the styles as RGB.
    fn rgb(&self, color: Color) -> Option<Rgb> {
        color.rgb(self.theme, self.styles.palette.as_deref())
    }

    /// The colour of a cell's fill, border or text: none when the sheet
    /// prints in black and white (a border or text is then black).
    fn cell_rgb(&self, color: Color) -> Option<Rgb> {
        if self.grid.print.black_and_white {
            None
        } else {
            self.rgb(color)
        }
    }
}

/// The width of a line no text box is as wide as, in points: that of a
/// text that is not wrapped.
const NOWRAP_WIDTH_PT: f32 = 100_000.0;

/// Where the text of a line's items `items` ends, from its left edge.
fn line_end(items: &[Item]) -> f32 {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Text(run) => run.glyphs.last().map(|glyph| run.x + glyph.x + glyph.advance),
            _ => None,
        })
        .fold(0.0, f32::max)
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
        // A page after the first holds the repeated rows (or columns) too;
        // the first holds them in their place.
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

/// The box of a page's part of the sheet (`columns` × `band`), as
/// `[left, top, right, bottom]` on the page.
/// The columns of stacked `text`, at most `per_column` letters each: a
/// line break starts a new column, spaces that pass the foot of a column
/// hang, and a column's trailing spaces are dropped.
fn stacked_columns(text: &str, per_column: usize) -> Vec<Vec<char>> {
    let mut columns = Vec::new();
    let mut column: Vec<char> = Vec::new();
    for ch in text.chars() {
        if ch == '\n' {
            columns.push(std::mem::take(&mut column));
        } else if ch.is_whitespace() {
            if column.len() < per_column {
                column.push(ch);
            }
        } else {
            if column.len() >= per_column {
                columns.push(std::mem::take(&mut column));
            }
            column.push(ch);
        }
    }
    columns.push(column);
    for column in &mut columns {
        while column.last().is_some_and(|ch| ch.is_whitespace()) {
            column.pop();
        }
    }
    columns
}

fn page_region(columns: &Columns, band: &Band, scale: f32) -> [f32; 4] {
    [
        columns.page_left,
        band.page_top,
        columns.page_left + (columns.sheet_right - columns.sheet_left) * scale,
        band.page_top + (band.sheet_bottom - band.sheet_top) * scale,
    ]
}

/// `item` (a line or a filled box) cut to `[left, top, right, bottom]`;
/// `None` when none of it is inside.
fn cut(item: Item, [left, top, right, bottom]: [f32; 4]) -> Option<Item> {
    let outside = |low: f32, high: f32, from: f32, to: f32| high < from - CUT_SLACK_PT || low > to + CUT_SLACK_PT;
    match item {
        Item::Line {
            x1,
            y1,
            x2,
            y2,
            width,
            color,
            dash,
        } => {
            if outside(x1.min(x2), x1.max(x2), left, right) || outside(y1.min(y2), y1.max(y2), top, bottom) {
                return None;
            }
            Some(Item::Line {
                x1: x1.clamp(left, right),
                y1: y1.clamp(top, bottom),
                x2: x2.clamp(left, right),
                y2: y2.clamp(top, bottom),
                width,
                color,
                dash,
            })
        }
        Item::Rect {
            x,
            y,
            width,
            height,
            color,
        } => {
            let (x0, y0) = (x.max(left), y.max(top));
            let (x1, y1) = ((x + width).min(right), (y + height).min(bottom));
            (x1 > x0 && y1 > y0).then_some(Item::Rect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
                color,
            })
        }
        other => Some(other),
    }
}

/// How far a data bar keeps from the top and bottom of its cell, in
/// points (two pixels).
const DATA_BAR_INSET_PT: f32 = 1.5;

/// How far a line may lie outside the page's part of the sheet and still
/// be drawn (on its edge), in points: rounding.
const CUT_SLACK_PT: f32 = 0.01;

/// The units a distributed text spreads apart: its words, its spaces and
/// each of its wide (Japanese and Chinese) letters.
fn distributed_units(text: &str) -> Vec<String> {
    let mut units: Vec<String> = Vec::new();
    let mut latin = String::new();
    for ch in text.trim().chars() {
        if ch.is_whitespace() || u32::from(ch) >= WIDE_LETTERS_FROM {
            units.extend((!latin.is_empty()).then(|| std::mem::take(&mut latin)));
            units.push(ch.to_string());
        } else {
            latin.push(ch);
        }
    }
    units.extend((!latin.is_empty()).then_some(latin));
    units
}

/// `runs` (items, width, is a space) cut into lines at most `room` wide,
/// a run wider than it alone on its line; the spaces at the ends of a
/// line are left out.
fn fill_lines<T>(runs: Vec<(T, f32, bool)>, room: f32) -> Vec<Vec<(T, f32, bool)>> {
    let mut lines: Vec<Vec<(T, f32, bool)>> = vec![Vec::new()];
    let mut used = 0.0;
    for run in runs {
        let line = lines.last_mut().expect("there is always a line");
        if run.2 && line.is_empty() {
            continue;
        }
        if !line.is_empty() && used + run.1 > room && !run.2 {
            while line.last().is_some_and(|last| last.2) {
                line.pop();
            }
            lines.push(Vec::new());
            used = 0.0;
        }
        used += run.1;
        lines.last_mut().expect("there is always a line").push(run);
    }
    if let Some(line) = lines.last_mut() {
        while line.last().is_some_and(|last| last.2) {
            line.pop();
        }
    }
    lines.retain(|line| !line.is_empty());
    lines
}

/// The thin grey lines around the four sides of a box at `[x, y, width,
/// height]` (a printed heading).
fn outline(lines: &mut Vec<Item>, [x, y, width, height]: [f32; 4], scale: f32) {
    grid_lines(lines, [x, y, width, height], scale);
    grid_lines(lines, [x - width, y - height, width, height], scale);
}

/// The thin grey lines of the right and bottom edges of a cell at `[x, y,
/// width, height]`, when the gridlines print.
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
fn clip_items(items: Vec<Item>, [left, top, right, bottom]: [f32; 4], open_right: bool) -> Vec<Item> {
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
                    let end = if open_right { start } else { start + glyph.advance };
                    start >= left - 0.5 && end <= right + 0.5
                });
                if run.glyphs.is_empty() {
                    continue;
                }
                keep_shown_text(run);
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

/// Keeps of the text of `run`, whose glyphs were cut to those its cell
/// shows, only what they show, and frees the rest: a cell of 32,000
/// characters on one line shows a few dozen, and holding all of them for
/// each cell of a page took gigabytes.
fn keep_shown_text(run: &mut crate::layout::display::TextRun) {
    run.glyphs.shrink_to_fit();
    let start = run.glyphs.iter().map(|glyph| glyph.text.0).min().unwrap_or(0);
    let end = run.glyphs.iter().map(|glyph| glyph.text.1).max().unwrap_or(0);
    let (Ok(from), Ok(to)) = (usize::try_from(start), usize::try_from(end)) else {
        return;
    };
    if from == 0 && to == run.text.len() {
        return;
    }
    let Some(shown) = run.text.get(from..to) else {
        return;
    };
    run.text = shown.to_string();
    for glyph in &mut run.glyphs {
        glyph.text = (glyph.text.0 - start, glyph.text.1 - start);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_merge_cut_by_a_page_break_is_drawn_only_on_the_page() {
        let region = [10.0, 0.0, 100.0, 50.0];
        let line = |x1, x2| Item::Line {
            x1,
            y1: 5.0,
            x2,
            y2: 5.0,
            width: 1.0,
            color: [0; 3],
            dash: None,
        };
        let Some(Item::Line { x1, x2, .. }) = cut(line(0.0, 200.0), region) else {
            panic!()
        };
        assert_eq!((x1, x2), (10.0, 100.0));
        // A side of the merge on another page is left out.
        let side = Item::Line {
            x1: 200.0,
            y1: 0.0,
            x2: 200.0,
            y2: 50.0,
            width: 1.0,
            color: [0; 3],
            dash: None,
        };
        assert!(cut(side, region).is_none());
        let fill = Item::Rect {
            x: 50.0,
            y: 0.0,
            width: 100.0,
            height: 20.0,
            color: [0; 3],
        };
        let Some(Item::Rect { x, width, .. }) = cut(fill, region) else {
            panic!()
        };
        assert_eq!((x, width), (50.0, 50.0));
    }

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
        let kept = clip_items(vec![Item::Text(run.clone())], [0.0, 0.0, 10.0, 30.0], false);
        let [Item::Text(cut)] = kept.as_slice() else {
            panic!("one run")
        };
        assert_eq!(cut.glyphs.len(), 2);
        // At a page's edge, the letter that crosses it stays.
        let kept = clip_items(vec![Item::Text(run.clone())], [0.0, 0.0, 7.0, 30.0], true);
        let [Item::Text(cut)] = kept.as_slice() else {
            panic!("one run")
        };
        assert_eq!(cut.glyphs.len(), 2);
        // A line outside the box is left out.
        assert!(clip_items(vec![Item::Text(run)], [0.0, 30.0, 10.0, 60.0], false).is_empty());
    }
}
