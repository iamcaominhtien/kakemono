//! A workbook as one HTML page that shows its sheets as Excel does (the
//! `medium` mode for Excel input): a tab per visible sheet, and on each the
//! grid of its cells with their widths, heights, merges, fonts, fills,
//! borders and alignment, the column letters and row numbers, and its
//! charts, pictures and shapes drawn over the cells where they are
//! anchored. It is a second reading of the file, with the look of every
//! cell, as the page layout of a Word document is.

use super::drawing::{Drawn, Frame, Shape, TextShape};
use super::grid::{CellKind, Grid, GridCell, Marker, Merge, Sizes};
use super::styles::{Color, Edge, HAlign, Line, Look, Styles, VAlign};
use super::{DrawnSheet, ExcelConfig, read_book};
use crate::chart::{Rgb, Theme};
use crate::model::style::{BOLD, ITALIC, SUB, SUP};
use crate::model::{Picture, Span};
use crate::render::escape::esc;
use anyhow::Result;
use base64::Engine as _;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

/// How much of a workbook the sheet view draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ViewConfig {
    /// The most cells read from one sheet (see
    /// [`ExcelConfig::max_cells`]): a page of a million table cells is too
    /// heavy for a browser; the rows after are left out, with a note.
    pub(crate) max_cells: usize,
    /// The most positions (rows × columns, empty ones too) drawn of one
    /// sheet; the rows past it are left out, with a note.
    pub(crate) max_positions: usize,
    /// The most columns drawn of one sheet.
    pub(crate) max_cols: u32,
    /// The most empty columns to its right a text overflows into, as in
    /// Excel.
    pub(crate) max_overflow_cols: u32,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            max_cells: 200_000,
            max_positions: 600_000,
            max_cols: 300,
            max_overflow_cols: 40,
        }
    }
}

/// Width of the column of row numbers, in points.
const ROW_HEADER_PT: f32 = 36.0;
/// Height of the row of column letters, in points.
const COL_HEADER_PT: f32 = 15.0;
/// Excel's default font size, in points.
const DEFAULT_SIZE_PT: f32 = 11.0;
/// Excel's default row height, in points (Calibri 11).
const DEFAULT_ROW_PT: f32 = 15.0;
/// The width of one step of a cell's indent, in points (three spaces of
/// the default font).
const INDENT_STEP_PT: f32 = 7.5;
/// The sides of a cell, in the order of [`Look::borders`].
const BORDER_SIDES: [&str; 4] = ["left", "right", "top", "bottom"];
/// Points in a pixel at 96 pixels per inch.
const PT_PER_PX: f32 = 0.75;
/// Size of a picture shown in its cell when its cell has no size, in
/// points.
const CELL_PICTURE_PT: (f32, f32) = (48.0, 15.0);
/// The width of a shape's outline when it gives none, in points.
const DEFAULT_LINE_PT: f32 = 0.75;
/// The thinnest line drawn, in points.
const MIN_LINE_PT: f32 = 0.5;
/// The radius of the corners of a rounded rectangle, in points.
const ROUND_CORNER_PT: f32 = 6.0;
/// The size of an arrow head, in widths of its line.
const ARROW_SIZE: u32 = 4;
/// URL schemes a cell's link may have; others (`javascript:`, local
/// files) are not links in the page.
const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "ftp://"];

/// The style of the page.
const CSS: &str = "*{box-sizing:border-box}\
body{margin:0;background:#fff;color:#000;font-family:Calibri,Carlito,'Segoe UI',Arial,'Noto Sans CJK JP','Noto Sans JP',sans-serif}\
header{position:sticky;top:0;z-index:5;background:#f3f3f3;border-bottom:1px solid #c8c8c8}\
header h1{font-size:13px;font-weight:600;margin:0;padding:6px 10px 2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}\
nav{display:flex;gap:2px;padding:0 6px;overflow-x:auto}\
nav a{flex:none;padding:4px 12px;font-size:13px;color:#333;text-decoration:none;border:1px solid transparent;border-bottom:none;border-radius:4px 4px 0 0;white-space:nowrap}\
nav a[aria-selected=true]{background:#fff;border-color:#c8c8c8;color:#000;font-weight:600;position:relative;top:1px}\
.sheet{padding:0 0 24px}\
.sheet>h2{font-size:15px;margin:12px 10px}\
.js .sheet>h2{display:none}\
.js .sheet[hidden]{display:none}\
.scroll{overflow:auto;max-width:100%}\
.grid{position:relative;width:max-content}\
table.cells{border-collapse:collapse;table-layout:fixed;background:#fff}\
.cells td{padding:0 2px;overflow:hidden;white-space:nowrap;vertical-align:bottom;line-height:1.15}\
.cells.lines td{box-shadow:inset -1px -1px 0 #e1e1e1}\
.cells th{background:#f3f3f3;color:#555;font-weight:400;font-size:11px;font-family:Calibri,Carlito,Arial,sans-serif;border:1px solid #d0d0d0;padding:0;text-align:center;overflow:hidden;white-space:nowrap}\
.cells thead th{position:sticky;top:0;z-index:2}\
.cells tbody th{position:sticky;left:0;z-index:1;line-height:0}\
.cells thead th:first-child{left:0;z-index:3}\
.cells td>div{overflow:hidden}\
.cells .w{white-space:pre-wrap;overflow-wrap:break-word;padding:0 1px}\
.cells .ov{overflow:visible}\
.cells .ov>div{overflow:hidden;white-space:pre}\
.cells a{color:inherit}\
.shape{position:absolute;display:flex;flex-direction:column;white-space:pre-wrap;line-height:1.2;padding:3.6pt 7.2pt;font-size:11pt}\
.shape>span{position:relative}\
.outline{position:absolute;left:0;top:0;width:100%;height:100%;overflow:visible}\
.line{position:absolute;overflow:visible}\
.pic{position:absolute;display:block}\
.note{margin:8px 10px;color:#555;font-size:13px}";

/// Shows the sheet whose tab is clicked, the first one at the start (or
/// the one the address names); without scripts every sheet shows, one
/// after the other.
const JS: &str = "(function(){var d=document;d.documentElement.classList.add('js');\
var tabs=[].slice.call(d.querySelectorAll('nav a')),sheets=[].slice.call(d.querySelectorAll('.sheet'));\
function show(id){var found=false;sheets.forEach(function(s){var on=s.id===id;s.hidden=!on;found=found||on});\
if(!found&&sheets.length){sheets[0].hidden=false;id=sheets[0].id}\
tabs.forEach(function(t){t.setAttribute('aria-selected',t.getAttribute('href')==='#'+id?'true':'false')})}\
tabs.forEach(function(t){t.addEventListener('click',function(e){e.preventDefault();var id=t.getAttribute('href').slice(1);\
show(id);history.replaceState(null,'','#'+id)})});\
addEventListener('hashchange',function(){show(location.hash.slice(1));scrollTo(0,0)});show(location.hash.slice(1))})();";

/// The sheet view of the workbook `data`, read with `config` and drawn
/// within the limits of `view`; titled with the workbook's title, else
/// `name` (where it came from), else its sheet's name.
///
/// # Errors
/// When the file is damaged or is not a workbook.
pub(crate) fn to_html(data: &[u8], config: &ExcelConfig, view: &ViewConfig, name: Option<&str>) -> Result<String> {
    let config = ExcelConfig {
        max_cells: config.max_cells.min(view.max_cells),
        max_cols: config
            .max_cols
            .min(usize::try_from(view.max_cols).unwrap_or(usize::MAX)),
        ..*config
    };
    let book = read_book(data, &config)?;
    let page = Page {
        styles: &book.styles,
        theme: &book.theme,
        pictures: &book.pictures,
        view,
        ids: std::cell::Cell::new(0),
        classes: std::cell::RefCell::new(Classes::default()),
        sheet_ids: book
            .sheets
            .iter()
            .enumerate()
            .map(|(index, sheet)| (sheet.name.to_lowercase(), index + 1))
            .collect(),
    };
    let title = book.title.as_deref().filter(|title| !title.trim().is_empty()).or(name);
    Ok(page.html(title, &book.sheets))
}

/// What every sheet of the page is drawn with.
struct Page<'a> {
    /// The cell formats of the workbook.
    styles: &'a Styles,
    /// The colours of its theme.
    theme: &'a Theme,
    /// The pictures read (charts drawn too), by index.
    pictures: &'a [Picture],
    /// The limits.
    view: &'a ViewConfig,
    /// The next id of an arrow head.
    ids: std::cell::Cell<u32>,
    /// The classes of the cell looks, shared by all the sheets: a class
    /// name means the same look on every sheet of the page.
    classes: std::cell::RefCell<Classes>,
    /// The number of each sheet's section (`sheet-N`), by its name in
    /// lower case.
    sheet_ids: HashMap<String, usize>,
}

impl Page<'_> {
    /// The page of the sheets `sheets` (name, grid, drawn objects).
    fn html(&self, title: Option<&str>, sheets: &[DrawnSheet]) -> String {
        let title = title.filter(|title| !title.trim().is_empty()).unwrap_or(match sheets {
            [sheet] => &sheet.name,
            _ => "Workbook",
        });
        let mut out = String::new();
        let _ = write!(
            out,
            "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"generator\" content=\"kakemono\"><title>{}</title><style>{CSS}</style></head><body><header><h1>{}</h1><nav>",
            esc(title),
            esc(title)
        );
        for (index, DrawnSheet { name, .. }) in sheets.iter().enumerate() {
            let _ = write!(out, "<a href=\"#sheet-{}\">{}</a>", index + 1, esc(name));
        }
        out.push_str("</nav></header><main>");
        for (index, DrawnSheet { name, grid, shapes, .. }) in sheets.iter().enumerate() {
            let _ = write!(
                out,
                "<section class=\"sheet\" id=\"sheet-{}\"><h2>{}</h2>",
                index + 1,
                esc(name)
            );
            self.sheet(&mut out, grid, shapes);
            out.push_str("</section>");
        }
        let _ = writeln!(out, "</main><script>{JS}</script></body></html>");
        out
    }

    /// Writes the grid of a sheet and its drawn objects.
    fn sheet(&self, out: &mut String, grid: &Grid, shapes: &[Shape]) {
        let area = Area::of(grid, shapes, self.view);
        let default_look = self.styles.look(0);
        let base_size = default_look.font.size_pt.unwrap_or(DEFAULT_SIZE_PT);
        let base_font = family(default_look.font.name.as_deref());
        let _ = write!(
            out,
            "<div class=\"scroll\"><div class=\"grid\"><table class=\"cells{}\" style=\"font-family:{};font-size:{}pt;width:{}pt\"><colgroup><col style=\"width:{ROW_HEADER_PT}pt\">",
            if grid.looks.no_grid_lines { "" } else { " lines" },
            esc(&base_font),
            num(base_size),
            num(ROW_HEADER_PT + area.cols.iter().map(|&col| area.width(col)).sum::<f32>()),
        );
        for &col in &area.cols {
            let _ = write!(out, "<col style=\"width:{}pt\">", num(area.width(col)));
        }
        out.push_str("</colgroup><thead><tr><th></th>");
        for &col in &area.cols {
            let _ = write!(out, "<th style=\"height:{COL_HEADER_PT}pt\">{}</th>", column_name(col));
        }
        out.push_str("</tr></thead><tbody>");
        let merges = Merges::new(grid, &area);
        let mut classes = self.classes.borrow_mut();
        // Only the rules of the looks this sheet adds are written with it.
        let known = classes.css.len();
        let mut body = String::new();
        for &row in &area.rows {
            let height = area.height(row);
            let _ = write!(body, "<tr style=\"height:{}pt\"><th>{}</th>", num(height), row + 1);
            for (position, &col) in area.cols.iter().enumerate() {
                if merges.covered.contains(&(row, col)) {
                    continue;
                }
                self.cell(&mut body, grid, &area, &merges, &mut classes, (row, col), position);
            }
            body.push_str("</tr>");
        }
        out.push_str(&body);
        out.push_str("</tbody></table>");
        if classes.css.len() > known {
            let _ = write!(out, "<style>{}</style>", &classes.css[known..]);
        }
        drop(classes);
        for shape in shapes {
            self.shape(out, grid, &area, shape);
        }
        out.push_str("</div></div>");
        for note in area.notes(grid) {
            let _ = write!(out, "<p class=\"note\">{}</p>", esc(&note));
        }
    }

    /// Writes the cell at `at`, the `position`-th column drawn; its look
    /// is a class of `classes`.
    #[allow(clippy::too_many_arguments)]
    fn cell(
        &self,
        out: &mut String,
        grid: &Grid,
        area: &Area,
        merges: &Merges,
        classes: &mut Classes,
        at: (u32, u32),
        position: usize,
    ) {
        let index = look_index(grid, at);
        let look = self.styles.look(index as usize);
        let cell = grid.cells.get(&at);
        let span = merges.spans.get(&at).copied();
        let kind = cell.map(|cell| cell.kind);
        let align = alignment(look.h_align, kind);
        let mut class = classes.of(index, align, || self.cell_style(look, align));
        let (cols, rows) = span.unwrap_or((1, 1));
        let mut attrs = String::new();
        if cols > 1 {
            let _ = write!(attrs, " colspan=\"{cols}\"");
        }
        if rows > 1 {
            let _ = write!(attrs, " rowspan=\"{rows}\"");
        }
        // A cell's text keeps to the height of its rows, as in Excel.
        let height: f32 = area
            .rows
            .iter()
            .skip_while(|&&row| row != at.0)
            .take(rows as usize)
            .map(|&row| area.height(row))
            .sum();
        let fill = grid.looks.fills.get(&at).copied();
        let text = cell.map(|cell| self.filled_text(cell, fill, look)).unwrap_or_default();
        let mut inner_style = String::new();
        if fill.is_some() && !text.is_empty() {
            inner_style.push_str("display:flex;");
        }
        if !text.is_empty() {
            let wraps = look.wraps();
            // A text centred across the selection is set in its box,
            // wrapped there when it wraps.
            let selection = grid.selection_width(at, &area.cols[position + 1..], |col| area.width(col));
            if selection > 0.0 {
                class.push_str(if wraps { " ov w" } else { " ov" });
                if wraps {
                    inner_style.push_str("white-space:pre-wrap;");
                }
                let own = area.width(at.1);
                let _ = write!(inner_style, "width:{}pt;", num(own + selection - 4.0 * PT_PER_PX));
            } else if wraps {
                class.push_str(" w");
            } else if span.is_none()
                && kind == Some(CellKind::Text)
                && let Some((left, right)) = self.overflow(grid, area, at, position, align)
            {
                // Text wider than its cell runs over the empty cells
                // beside it, as in Excel.
                class.push_str(" ov");
                if left > 0.0 {
                    let _ = write!(inner_style, "margin-left:-{}pt;", num(left));
                }
                let own = area.width(at.1);
                let _ = write!(inner_style, "width:{}pt;", num(own + left + right - 4.0 * PT_PER_PX));
            }
            inner_style.push_str(turned(look.stacked, look.rotation));
        }
        let class = class.trim();
        let _ = write!(out, "<td{attrs}");
        if !class.is_empty() {
            let _ = write!(out, " class=\"{class}\"");
        }
        let mut cell_style = if cols > 1 || rows > 1 {
            self.merge_edges(grid, at, (cols, rows), look)
        } else {
            String::new()
        };
        cell_style.push_str(&self.bar_style(grid, at));
        if !cell_style.is_empty() {
            let _ = write!(out, " style=\"{cell_style}\"");
        }
        out.push('>');
        if !text.is_empty() {
            let _ = write!(
                out,
                "<div style=\"max-height:{}pt;{inner_style}\">{text}</div>",
                num(height)
            );
        }
        out.push_str("</td>");
    }

    /// The CSS of the data bar of the cell at `at`, empty when it has
    /// none: from its colour to white, as Excel shades it, within a pixel
    /// or two of the cell's edges.
    fn bar_style(&self, grid: &Grid, at: (u32, u32)) -> String {
        let Some((bar, color)) = grid
            .looks
            .bars
            .get(&at)
            .and_then(|bar| Some((bar, self.rgb(bar.color)?)))
        else {
            return String::new();
        };
        format!(
            "background-image:linear-gradient(90deg,{},#fff);background-repeat:no-repeat;background-position:1px 50%;background-size:calc({}% - 2px) calc(100% - 4px);",
            hex(color),
            num(bar.length * 100.0)
        )
    }

    /// The CSS of a cell of look `look` aligned `align`: its fill,
    /// borders, font, alignment and indent; empty for the default look.
    fn cell_style(&self, look: &Look, align: &str) -> String {
        let mut style = String::new();
        if let Some(fill) = look.fill.and_then(|color| self.rgb(color)) {
            let _ = write!(style, "background:{};box-shadow:none;", hex(fill));
        }
        for (side, edge) in BORDER_SIDES.iter().zip(look.borders) {
            if let Some(edge) = edge {
                style.push_str(&self.edge_css(side, edge));
            }
        }
        let font = &look.font;
        let base = &self.styles.look(0).font;
        if let Some(name) = &font.name
            && Some(name) != base.name.as_ref()
        {
            let _ = write!(style, "font-family:{};", family(Some(name)));
        }
        if let Some(size) = font.size_pt
            && Some(size) != base.size_pt
        {
            let _ = write!(style, "font-size:{}pt;", num(size));
        }
        if let Some(color) = font.color.and_then(|color| self.rgb(color))
            && color != [0; 3]
        {
            let _ = write!(style, "color:{};", hex(color));
        }
        if font.bold {
            style.push_str("font-weight:700;");
        }
        if font.italic {
            style.push_str("font-style:italic;");
        }
        match (font.underline, font.strike) {
            (true, true) => style.push_str("text-decoration:underline line-through;"),
            (true, false) => style.push_str("text-decoration:underline;"),
            (false, true) => style.push_str("text-decoration:line-through;"),
            (false, false) => {}
        }
        if align == "distributed" {
            style.push_str("text-align:justify;text-align-last:justify;");
        } else if align != "left" {
            let _ = write!(style, "text-align:{align};");
        }
        match look.v_align {
            VAlign::Top => style.push_str("vertical-align:top;"),
            VAlign::Middle => style.push_str("vertical-align:middle;"),
            VAlign::Bottom => {}
        }
        if look.indent > 0 {
            let padding = f32::from(look.indent) * INDENT_STEP_PT;
            if align == "distributed" {
                let _ = write!(style, "padding:0 calc(2px + {0}pt);", num(padding));
            } else {
                let side = if align == "right" { "right" } else { "left" };
                let _ = write!(style, "padding-{side}:{}pt;", num(padding));
            }
        }
        style
    }

    /// The CSS of the border `edge` on the `side` of a cell.
    fn edge_css(&self, side: &str, edge: Edge) -> String {
        let line = match edge.line {
            Line::Solid => "solid",
            Line::Dashed => "dashed",
            Line::Dotted | Line::Hair => "dotted",
            Line::Double => "double",
        };
        let color = edge.color.and_then(|color| self.rgb(color)).unwrap_or([0; 3]);
        format!("border-{side}:{}px {line} {};", num(edge.width_px), hex(color))
    }

    /// The CSS of the right and bottom borders of a merged range of `cols`
    /// by `rows` cells from `at`, when they differ from its first cell's
    /// look `look`: as Excel draws them, those of its right column and of
    /// its bottom row.
    fn merge_edges(&self, grid: &Grid, at: (u32, u32), (cols, rows): (u32, u32), look: &Look) -> String {
        let mut style = String::new();
        let right = self
            .styles
            .look(look_index(grid, (at.0, at.1 + cols.saturating_sub(1))) as usize)
            .borders[1];
        let bottom = self
            .styles
            .look(look_index(grid, (at.0 + rows.saturating_sub(1), at.1)) as usize)
            .borders[3];
        for (side, edge, own) in [("right", right, look.borders[1]), ("bottom", bottom, look.borders[3])] {
            if edge != own {
                match edge {
                    Some(edge) => style.push_str(&self.edge_css(side, edge)),
                    None => {
                        let _ = write!(style, "border-{side}:none;");
                    }
                }
            }
        }
        style
    }

    /// How far the text of the cell `at` (the `position`-th column drawn)
    /// may run over the empty cells beside it, in points: to the left and
    /// to the right, by its alignment; `None` when it may not.
    fn overflow(&self, grid: &Grid, area: &Area, at: (u32, u32), position: usize, align: &str) -> Option<(f32, f32)> {
        let free = |col: u32| grid.is_blank((at.0, col)) && !area.merged.contains(&(at.0, col));
        let reach = |columns: &mut dyn Iterator<Item = &u32>| -> f32 {
            columns
                .take(self.view.max_overflow_cols as usize)
                .take_while(|&&col| free(col))
                .map(|&col| area.width(col))
                .sum()
        };
        let right = || reach(&mut area.cols[position + 1..].iter());
        let left = || reach(&mut area.cols[..position].iter().rev());
        let (left, right) = match align {
            "left" => (0.0, right()),
            "right" => (left(), 0.0),
            "center" => {
                let room = left().min(right());
                (room, room)
            }
            _ => return None,
        };
        (left + right > 0.0).then_some((left, right))
    }

    /// The text of `cell` as [`text`](Self::text) writes it; with a fill
    /// (`fill`, its byte offset), the text before it and the text after it
    /// at the two edges of a flex box, as Excel sets an accounting format.
    fn filled_text(&self, cell: &GridCell, fill: Option<usize>, look: &Look) -> String {
        let [span] = cell.spans.as_slice() else {
            return self.text(cell, look);
        };
        let Some((before, after)) = fill.and_then(|at| Some((span.text.get(..at)?, span.text.get(at..)?))) else {
            return self.text(cell, look);
        };
        let part = |text: &str| {
            self.text(
                &GridCell {
                    spans: vec![Span {
                        text: text.to_string(),
                        ..span.clone()
                    }],
                    ..cell.clone()
                },
                look,
            )
        };
        format!("{}<span style=\"flex:1\"></span>{}", part(before), part(after))
    }

    /// The text of a cell as HTML, with its links (of the schemes a page
    /// may open) and the bold, italic, superscript and subscript of its
    /// runs; a run that sets its own font takes neither the bold nor the
    /// italic of the cell's look (`look`), nor its colour when it gives one.
    fn text(&self, cell: &GridCell, look: &Look) -> String {
        let mut out = String::new();
        let cell_color = look.font.color.and_then(|color| self.rgb(color)).unwrap_or_default();
        for (index, span) in cell.spans.iter().enumerate() {
            let mut body = esc(&span.text);
            for (flag, tag) in [(SUB, "sub"), (SUP, "sup"), (ITALIC, "i"), (BOLD, "b")] {
                if span.style & flag != 0 {
                    body = format!("<{tag}>{body}</{tag}>");
                }
            }
            let (own_style, own_color) = cell.own_font(index);
            if own_style {
                let mut css = String::new();
                if look.font.bold && span.style & BOLD == 0 {
                    css.push_str("font-weight:400;");
                }
                if look.font.italic && span.style & ITALIC == 0 {
                    css.push_str("font-style:normal;");
                }
                if own_color && span.color == [0; 3] && cell_color != [0; 3] {
                    css.push_str("color:#000000;");
                }
                if !css.is_empty() {
                    body = format!("<span style=\"{css}\">{body}</span>");
                }
            }
            if span.color != [0; 3] {
                body = format!("<span style=\"color:{}\">{body}</span>", hex(span.color));
            }
            match &span.href {
                Some(href) if LINK_SCHEMES.iter().any(|scheme| href.starts_with(scheme)) => {
                    let _ = write!(
                        out,
                        "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{body}</a>",
                        esc(href)
                    );
                }
                Some(href) if href.starts_with('#') => match self.sheet_id(&href[1..]) {
                    Some(id) => {
                        let _ = write!(out, "<a href=\"#sheet-{id}\">{body}</a>");
                    }
                    None => out.push_str(&body),
                },
                _ => out.push_str(&body),
            }
        }
        out
    }

    /// The number of the section of the sheet an internal link's place
    /// (`'Sheet name'!A1`, `Sheet!A1`) names; `None` for another place (a
    /// defined name) or a sheet not shown.
    fn sheet_id(&self, place: &str) -> Option<usize> {
        let (sheet, _) = place.rsplit_once('!')?;
        let sheet = match sheet.strip_prefix('\'').and_then(|rest| rest.strip_suffix('\'')) {
            Some(quoted) => quoted.replace("''", "'"),
            None => sheet.to_string(),
        };
        self.sheet_ids.get(&sheet.to_lowercase()).copied()
    }

    /// Writes a chart, picture or shape where it is drawn on the sheet.
    fn shape(&self, out: &mut String, grid: &Grid, area: &Area, shape: &Shape) {
        if area.frame_box(grid, &shape.frame).is_none() {
            return;
        }
        let Some([x, y, width, height]) = object_box(grid, shape) else {
            return;
        };
        if width < 0.0 || height < 0.0 || (width == 0.0 && height == 0.0) {
            return;
        }
        let place = format!(
            "left:{}pt;top:{}pt;width:{}pt;height:{}pt",
            num(x + ROW_HEADER_PT),
            num(y + COL_HEADER_PT),
            num(width),
            num(height)
        );
        let picture = |index: Option<usize>, alt: &str, fit: bool| -> Option<String> {
            let picture = self.pictures.get(index?)?;
            let data = base64::engine::general_purpose::STANDARD.encode(&picture.data);
            Some(format!(
                "<img class=\"pic\" style=\"{place}{}\" alt=\"{}\" src=\"data:{};base64,{data}\">",
                if fit { ";object-fit:contain" } else { "" },
                esc(alt),
                picture.format.mime()
            ))
        };
        let html = match &shape.drawn {
            Drawn::Picture(picture_ref) => picture(picture_ref.index, &picture_ref.alt, false),
            Drawn::CellPicture(picture_ref) => picture(picture_ref.index, &picture_ref.alt, true),
            Drawn::Chart(chart) => chart.placed.as_ref().and_then(|placed| {
                let (placed, index) = placed.as_ref();
                picture(*index, placed.title.as_deref().unwrap_or("Chart"), false)
            }),
            Drawn::Text(text) => Some(self.text_shape(text, &place, width, height)),
        };
        if let Some(html) = html {
            out.push_str(&html);
        }
    }

    /// A shape with its fill, outline and text, at `place`, `width` by
    /// `height` points: a line or connector as an SVG line with its arrow
    /// heads; a preset outline that is not a box (arrows, triangles,
    /// brackets, ...) as an SVG path under its text; a box with CSS.
    fn text_shape(&self, shape: &TextShape, place: &str, width: f32, height: f32) -> String {
        let geometry = shape.geometry.as_deref().unwrap_or("rect");
        let mut style = format!("{place};");
        if shape.rotation_deg != 0.0 {
            let _ = write!(style, "transform:rotate({}deg);", num(shape.rotation_deg));
        }
        let stroke = shape.line.map(hex);
        let stroke_pt = shape.line_width_pt.unwrap_or(DEFAULT_LINE_PT).max(MIN_LINE_PT);
        if let Some(points) = line_points(geometry, width, height) {
            return stroke.map_or_else(String::new, |stroke| {
                self.line_svg(shape, &points, &style, &stroke, stroke_pt, (width, height))
            });
        }
        let mut outline = String::new();
        if let Some((path, filled)) = shape_path(shape, width, height) {
            let fill = if filled { shape.fill.map(hex) } else { None };
            let mirror = match (shape.flip_h, shape.flip_v) {
                (false, false) => String::new(),
                (flip_h, flip_v) => format!(
                    " transform=\"translate({} {}) scale({} {})\"",
                    if flip_h { num(width) } else { "0".into() },
                    if flip_v { num(height) } else { "0".into() },
                    if flip_h { -1 } else { 1 },
                    if flip_v { -1 } else { 1 }
                ),
            };
            let _ = write!(
                outline,
                "<svg class=\"outline\" viewBox=\"0 0 {} {}\" preserveAspectRatio=\"none\"><path d=\"{path}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\" vector-effect=\"non-scaling-stroke\"{mirror}/></svg>",
                num(width.max(MIN_BOX_PT)),
                num(height.max(MIN_BOX_PT)),
                fill.as_deref().unwrap_or("none"),
                stroke.as_deref().unwrap_or("none"),
                num(stroke_pt / PT_PER_PX),
            );
        } else {
            if let Some(fill) = shape.fill {
                let _ = write!(style, "background:{};", hex(fill));
            }
            if let Some(stroke) = &stroke {
                let _ = write!(
                    style,
                    "border:{}pt {} {stroke};",
                    num(stroke_pt),
                    if shape.dashed { "dashed" } else { "solid" }
                );
            }
            match geometry {
                "ellipse" | "flowChartConnector" => style.push_str("border-radius:50%;"),
                "roundRect" | "round2SameRect" | "snipRoundRect" | "flowChartAlternateProcess" => {
                    style.push_str("border-radius:8px;");
                }
                "flowChartTerminator" => style.push_str("border-radius:999px;"),
                _ => {}
            }
        }
        text_style(&mut style, shape);
        text_box_css(&mut style, shape, callout_tip(shape, width, height).is_some());
        let text = if shape.text.is_empty() {
            String::new()
        } else {
            format!("<span>{}</span>", runs_html(shape))
        };
        format!("<div class=\"shape\" style=\"{style}\">{outline}{text}</div>")
    }

    /// A line or connector through `points` in a box `width` by `height`
    /// points, at `style`, drawn in `stroke`, `stroke_pt` wide, mirrored
    /// as `shape` is, with its arrow heads.
    fn line_svg(
        &self,
        shape: &TextShape,
        points: &[[f32; 2]],
        style: &str,
        stroke: &str,
        stroke_pt: f32,
        (width, height): (f32, f32),
    ) -> String {
        let path: Vec<String> = points
            .iter()
            .map(|&[x, y]| {
                let x = if shape.flip_h { width - x } else { x };
                let y = if shape.flip_v { height - y } else { y };
                format!("{},{}", num(x), num(y))
            })
            .collect();
        let id = self.next_id();
        let marker = |name: &str, used: bool| {
            if used {
                format!(" marker-{name}=\"url(#a{id})\"")
            } else {
                String::new()
            }
        };
        format!(
            "<svg class=\"line\" style=\"{style}min-width:{MIN_LINE_PT}pt;min-height:{MIN_LINE_PT}pt;\" viewBox=\"0 0 {} {}\"><defs><marker id=\"a{id}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"{ARROW_SIZE}\" markerHeight=\"{ARROW_SIZE}\" orient=\"auto-start-reverse\"><path d=\"M0,0L10,5L0,10z\" fill=\"{stroke}\"/></marker></defs><polyline points=\"{}\" fill=\"none\" stroke=\"{stroke}\" stroke-width=\"{}\"{}{}{}/></svg>",
            num(width.max(MIN_LINE_PT)),
            num(height.max(MIN_LINE_PT)),
            path.join(" "),
            num(stroke_pt),
            if shape.dashed { " stroke-dasharray=\"4 3\"" } else { "" },
            marker("start", shape.head),
            marker("end", shape.tail),
        )
    }

    /// A number not used before on the page, for the ids of its arrow
    /// heads.
    fn next_id(&self) -> u32 {
        let id = self.ids.get();
        self.ids.set(id + 1);
        id
    }

    /// A colour of the styles as RGB.
    fn rgb(&self, color: Color) -> Option<Rgb> {
        color.rgb(self.theme, self.styles.palette.as_deref())
    }
}

/// The classes of the looks of the cells of a page: each look (and
/// alignment) is written once, as a CSS rule, not on every cell.
#[derive(Default)]
struct Classes {
    /// The class of each look and alignment; empty for the default look.
    names: HashMap<(u32, &'static str), String>,
    /// The CSS rules of the classes.
    css: String,
}

impl Classes {
    /// The class of the look `index` aligned `align`, made with the CSS
    /// `style` gives the first time.
    fn of(&mut self, index: u32, align: &'static str, style: impl FnOnce() -> String) -> String {
        if let Some(name) = self.names.get(&(index, align)) {
            return name.clone();
        }
        let style = style();
        let name = if style.is_empty() {
            String::new()
        } else {
            let name = format!("k{}", self.names.len());
            // As specific as the gridlines' rule, and after it.
            let _ = write!(self.css, ".cells td.{name}{{{style}}}");
            name
        };
        self.names.insert((index, align), name.clone());
        name
    }
}

/// The alignment of a cell of horizontal alignment `align` holding a
/// value of `kind`: numbers and dates right, logical and error values
/// centred, text left, unless the cell says otherwise.
pub(crate) fn alignment(align: HAlign, kind: Option<CellKind>) -> &'static str {
    match (align, kind) {
        (HAlign::General, Some(CellKind::Number | CellKind::Date)) => "right",
        (HAlign::General, Some(CellKind::Bool | CellKind::Error)) => "center",
        (HAlign::General | HAlign::Left, _) => "left",
        (HAlign::Center, _) => "center",
        (HAlign::Right, _) => "right",
        (HAlign::Justify, _) => "justify",
        (HAlign::Distributed, _) => "distributed",
    }
}

/// The CSS of a cell's text that is `stacked` or turned by `rotation`
/// degrees: every letter turns with the text, Japanese ones too.
fn turned(stacked: bool, rotation: Option<i16>) -> &'static str {
    match rotation {
        _ if stacked => "writing-mode:vertical-rl;text-orientation:upright;margin:auto;",
        Some(degrees) if degrees > 0 => {
            "writing-mode:vertical-rl;text-orientation:sideways;transform:rotate(180deg);margin:auto;"
        }
        Some(_) => "writing-mode:vertical-rl;text-orientation:sideways;margin:auto;",
        None => "",
    }
}

/// The rows and columns of a sheet that are drawn, with their sizes.
struct Area<'g> {
    /// The rows drawn (not hidden), in order.
    rows: Vec<u32>,
    /// The columns drawn (not hidden), in order.
    cols: Vec<u32>,
    /// The sizes of the sheet.
    sizes: &'g Sizes,
    /// The width of each column drawn, in points.
    widths: HashMap<u32, f32>,
    /// The positions covered by a merge, with its first cell.
    merged: HashSet<(u32, u32)>,
    /// The rows with a value or a format past the limit of positions.
    rows_left_out: u32,
}

impl<'g> Area<'g> {
    /// The rows and columns of `grid` to draw: up to the last one with a
    /// value, a format, a merge or a drawn object, hidden ones left out,
    /// within the limits of `view`.
    fn of(grid: &'g Grid, shapes: &[Shape], view: &ViewConfig) -> Self {
        let mut last_row = 0;
        let mut last_col = 0;
        let mut any = false;
        let mut extend = |(row, col): (u32, u32)| {
            any = true;
            last_row = last_row.max(row);
            last_col = last_col.max(col);
        };
        grid.cells.keys().copied().for_each(&mut extend);
        grid.looks.cells.keys().copied().for_each(&mut extend);
        for merge in &grid.merges {
            extend((merge.bottom, merge.right));
        }
        for shape in shapes {
            let corner = shape.frame.to.or(shape.frame.from);
            if let Some(marker) = corner {
                extend((marker.row, marker.col));
            }
        }
        let last_col = if any { last_col.min(view.max_cols - 1) } else { 0 };
        let cols: Vec<u32> = (0..=last_col).filter(|col| !grid.hidden_cols.contains(col)).collect();
        let max_rows = view.max_positions / cols.len().max(1);
        let mut rows: Vec<u32> = Vec::new();
        let mut rows_left_out: u32 = 0;
        if any {
            for row in (0..=last_row).filter(|row| !grid.hidden_rows.contains(row)) {
                if rows.len() >= max_rows {
                    rows_left_out = rows_left_out.saturating_add(1);
                } else {
                    rows.push(row);
                }
            }
        }
        let widths = cols.iter().map(|&col| (col, col_width(&grid.sizes, col))).collect();
        let mut merged = HashSet::new();
        for merge in &grid.merges {
            for row in merge.top..=merge.bottom.min(last_row) {
                for col in merge.left..=merge.right.min(last_col) {
                    merged.insert((row, col));
                }
            }
        }
        Self {
            rows,
            cols,
            sizes: &grid.sizes,
            widths,
            merged,
            rows_left_out,
        }
    }

    /// The width of the column `col`, in points.
    fn width(&self, col: u32) -> f32 {
        self.widths.get(&col).copied().unwrap_or(0.0)
    }

    /// The height of the row `row`, in points.
    fn height(&self, row: u32) -> f32 {
        self.sizes
            .rows
            .get(&row)
            .copied()
            .unwrap_or(self.sizes.default_row_pt.unwrap_or(DEFAULT_ROW_PT))
    }

    /// The box (left, top, width, height, in points from the top left of
    /// the first cell) of a frame, when it starts in the part of the sheet
    /// drawn.
    fn frame_box(&self, grid: &Grid, frame: &Frame) -> Option<[f32; 4]> {
        let [x, y, width, height] = frame_box(grid, frame)?;
        // Only the part of the sheet drawn shows objects.
        let total_width: f32 = self.cols.iter().map(|&col| self.width(col)).sum();
        let total_height: f32 = self.rows.iter().map(|&row| self.height(row)).sum();
        let finite = [x, y, width, height].iter().all(|value| value.is_finite());
        (finite && x < total_width.max(CELL_PICTURE_PT.0) && y < total_height.max(CELL_PICTURE_PT.1))
            .then_some([x, y, width, height])
    }

    /// The sentences saying what of the sheet is not drawn.
    fn notes(&self, grid: &Grid) -> Vec<String> {
        let mut notes = Vec::new();
        let rows = grid.truncated_rows.saturating_add(self.rows_left_out);
        for (count, one, many) in [
            (rows, "row is", "rows are"),
            (grid.truncated_cols, "column is", "columns are"),
        ] {
            if count == 1 {
                notes.push(format!("1 more {one} not shown."));
            } else if count > 1 {
                notes.push(format!("{count} more {many} not shown."));
            }
        }
        notes
    }
}

/// The merges of a sheet as drawn: the columns and rows each spans (of
/// those drawn), by its first cell, and the positions the others cover.
struct Merges {
    /// Columns and rows drawn of each merge, by its first cell.
    spans: HashMap<(u32, u32), (u32, u32)>,
    /// The positions covered by a merge, its first cell left out.
    covered: HashSet<(u32, u32)>,
}

impl Merges {
    /// The merges of `grid` within `area`. A merge whose first cell is
    /// hidden starts at its first cell drawn.
    fn new(grid: &Grid, area: &Area) -> Self {
        let mut spans = HashMap::new();
        let mut covered = HashSet::new();
        let drawn_rows: HashSet<u32> = area.rows.iter().copied().collect();
        let drawn_cols: HashSet<u32> = area.cols.iter().copied().collect();
        for merge in &grid.merges {
            let Merge {
                top,
                left,
                bottom,
                right,
            } = *merge;
            let rows: Vec<u32> = (top..=bottom).filter(|row| drawn_rows.contains(row)).collect();
            let cols: Vec<u32> = (left..=right).filter(|col| drawn_cols.contains(col)).collect();
            let (Some(&first_row), Some(&first_col)) = (rows.first(), cols.first()) else {
                continue;
            };
            if rows.len() * cols.len() <= 1 {
                continue;
            }
            spans.insert(
                (first_row, first_col),
                (
                    u32::try_from(cols.len()).unwrap_or(u32::MAX),
                    u32::try_from(rows.len()).unwrap_or(u32::MAX),
                ),
            );
            for &row in &rows {
                for &col in &cols {
                    if (row, col) != (first_row, first_col) {
                        covered.insert((row, col));
                    }
                }
            }
        }
        Self { spans, covered }
    }
}

/// The cell format of the position `at`: its own, else its row's, else
/// its column's.
pub(crate) fn look_index(grid: &Grid, at: (u32, u32)) -> u32 {
    if let Some(&style) = grid.looks.cells.get(&at) {
        return style;
    }
    if let Some(&style) = grid.looks.rows.get(&at.0) {
        return style;
    }
    grid.looks
        .cols
        .iter()
        .rev()
        .find(|(first, last, _)| (*first..=*last).contains(&at.1))
        .map_or(0, |(_, _, style)| *style)
}

/// Writes to `style` how the text box of `shape` holds its text: its
/// insets, its wrapping and where it is cut (not for a `callout`, whose tip
/// lies outside).
fn text_box_css(style: &mut String, shape: &TextShape, callout: bool) {
    // A shape without text has no insets, which would make a small one
    // larger; a text keeps them, overflowing a narrow box as in Excel.
    let insets = if shape.text.is_empty() {
        Some([0.0; 4])
    } else {
        shape.insets_pt
    };
    if let Some([left, top, right, bottom]) = insets {
        let _ = write!(
            style,
            "padding:{}pt {}pt {}pt {}pt;",
            num(top),
            num(right),
            num(bottom),
            num(left)
        );
    }
    // A text that is not wrapped keeps on one line, past the box's sides
    // (rather than cut there) when the font set is wider.
    if shape.nowrap {
        style.push_str("white-space:pre;");
    }
    // Only a text is cut at the box: an outline's stroke straddles it.
    if shape.clip && !shape.text.is_empty() && !callout {
        style.push_str(if shape.nowrap {
            "overflow-y:clip;"
        } else {
            "overflow:hidden;"
        });
    }
}

/// The width of the column `col` of a sheet of sizes `sizes`, in points.
pub(crate) fn col_width(sizes: &Sizes, col: u32) -> f32 {
    let origin = Marker {
        row: 0,
        row_off_pt: 0.0,
        col,
        col_off_pt: 0.0,
    };
    let next = Marker { col: col + 1, ..origin };
    sizes.span_pt(origin, next, &HashSet::new(), &HashSet::new()).0
}

/// The HTML of the text of `shape`, run by run: a run that sets its own
/// look is a `span` with it.
fn runs_html(shape: &TextShape) -> String {
    if shape.runs.is_empty() {
        return esc(&shape.text);
    }
    let mut html = String::new();
    for run in &shape.runs {
        let mut css = String::new();
        if let Some(bold) = run.bold {
            css.push_str(if bold { "font-weight:700;" } else { "font-weight:400;" });
        }
        if let Some(italic) = run.italic {
            css.push_str(if italic {
                "font-style:italic;"
            } else {
                "font-style:normal;"
            });
        }
        if run.underline == Some(true) {
            css.push_str("text-decoration:underline;");
        }
        if let Some(size) = run.size_pt {
            let _ = write!(css, "font-size:{}pt;", num(size));
        }
        if let Some(color) = run.color {
            let _ = write!(css, "color:{};", hex(color));
        }
        if css.is_empty() || run.text == "\n" {
            html.push_str(&esc(&run.text));
        } else {
            let _ = write!(html, "<span style=\"{css}\">{}</span>", esc(&run.text));
        }
    }
    html
}

/// Adds to `style` the CSS of the text of `shape`: its colour, size,
/// weight, alignment and where it sits.
fn text_style(style: &mut String, shape: &TextShape) {
    if let Some(color) = shape.color {
        let _ = write!(style, "color:{};", hex(color));
    }
    if let Some(size) = shape.size_pt {
        let _ = write!(style, "font-size:{}pt;", num(size));
    }
    if shape.bold {
        style.push_str("font-weight:700;");
    }
    match shape.align.as_deref() {
        Some("ctr") => style.push_str("text-align:center;"),
        Some("r") => style.push_str("text-align:right;"),
        Some("just" | "dist") => style.push_str("text-align:justify;"),
        _ => {}
    }
    match shape.anchor.as_deref() {
        Some("ctr") => style.push_str("justify-content:center;"),
        Some("b") => style.push_str("justify-content:flex-end;"),
        _ => {}
    }
}

/// Whether a shape turned by `degrees` (clockwise) is turned by about a
/// quarter or three quarters: 45 to 135 or 225 to 315 degrees.
pub(crate) fn quarter_turned(degrees: f32) -> bool {
    (45.0..135.0).contains(&degrees) || (225.0..315.0).contains(&degrees)
}

/// The points of a line or connector of the preset `geometry` in a box
/// `width` by `height` points, from its start to its end (before it is
/// mirrored); `None` for a geometry that is not a line. Elbow connectors
/// bend at the middle, as they do by default.
pub(crate) fn line_points(geometry: &str, width: f32, height: f32) -> Option<Vec<[f32; 2]>> {
    let (mid_x, mid_y) = (width / 2.0, height / 2.0);
    Some(match geometry {
        "line" | "straightConnector1" | "curvedConnector2" | "curvedConnector3" | "curvedConnector4"
        | "curvedConnector5" => {
            vec![[0.0, 0.0], [width, height]]
        }
        "bentConnector2" => vec![[0.0, 0.0], [width, 0.0], [width, height]],
        "bentConnector3" | "bentConnector5" => vec![[0.0, 0.0], [mid_x, 0.0], [mid_x, height], [width, height]],
        "bentConnector4" => vec![
            [0.0, 0.0],
            [mid_x, 0.0],
            [mid_x, mid_y],
            [width, mid_y],
            [width, height],
        ],
        _ => return None,
    })
}

/// The path of the outline of `shape` in a box `width` by `height` points,
/// and whether it is filled: its drawn outline, its callout with its tip,
/// or its preset outline ([`outline_path`]).
pub(crate) fn shape_path(shape: &TextShape, width: f32, height: f32) -> Option<(String, bool)> {
    let (wide, high) = (width.max(MIN_BOX_PT), height.max(MIN_BOX_PT));
    if let Some(path) = &shape.custom_path {
        return Some((scale_path(path, wide / 100.0, high / 100.0), true));
    }
    if let Some(path) = callout_path(shape, wide, high) {
        return Some((path, true));
    }
    outline_path(shape.geometry.as_deref().unwrap_or("rect"), width, height)
}

/// The default place of a callout's tip (`adj1`, `adj2`): from the box's
/// centre, in 100 000ths of its width and height.
const CALLOUT_TIP: [f32; 2] = [-20_833.0, 62_500.0];

/// The tip of `shape` when it is a callout with a wedge (`wedgeRectCallout`,
/// `wedgeRoundRectCallout`), in a box `width` by `height` points.
fn callout_tip(shape: &TextShape, width: f32, height: f32) -> Option<[f32; 2]> {
    if !matches!(
        shape.geometry.as_deref(),
        Some("wedgeRectCallout" | "wedgeRoundRectCallout")
    ) {
        return None;
    }
    let adjust = |index: usize| shape.adjust.get(index).copied().unwrap_or(CALLOUT_TIP[index]) / 100_000.0;
    Some([width / 2.0 + width * adjust(0), height / 2.0 + height * adjust(1)])
}

/// The path of a callout box with its wedge to its tip, as Excel draws a
/// `wedgeRectCallout`: the wedge leaves the side the tip lies beyond,
/// between a sixth and a quarter of that side from its middle.
fn callout_path(shape: &TextShape, width: f32, height: f32) -> Option<String> {
    let [tip_x, tip_y] = callout_tip(shape, width, height)?;
    let (dx, dy) = (tip_x - width / 2.0, tip_y - height / 2.0);
    // Beyond the top or bottom side rather than the left or right one.
    let upright = dy.abs() > (dx * height / width).abs();
    let twelfths = |side: f32, positive: bool| {
        if positive {
            [side * 7.0 / 12.0, side * 10.0 / 12.0]
        } else {
            [side * 2.0 / 12.0, side * 5.0 / 12.0]
        }
    };
    let [x1, x2] = twelfths(width, dx > 0.0);
    let [y1, y2] = twelfths(height, dy > 0.0);
    let mut points = vec![[0.0, 0.0]];
    if upright && dy <= 0.0 {
        points.extend([[x1, 0.0], [tip_x, tip_y], [x2, 0.0]]);
    }
    points.push([width, 0.0]);
    if !upright && dx > 0.0 {
        points.extend([[width, y1], [tip_x, tip_y], [width, y2]]);
    }
    points.push([width, height]);
    if upright && dy > 0.0 {
        points.extend([[x2, height], [tip_x, tip_y], [x1, height]]);
    }
    points.push([0.0, height]);
    if !upright && dx <= 0.0 {
        points.extend([[0.0, y2], [tip_x, tip_y], [0.0, y1]]);
    }
    let steps: Vec<String> = points.iter().map(|[x, y]| format!("{},{}", num(*x), num(*y))).collect();
    Some(format!("M{}Z", steps.join("L")))
}

/// The path of a preset outline that is not a box in a box `width` by
/// `height` points, and whether it is filled (brackets are not); `None`
/// for a box, drawn with CSS.
pub(crate) fn outline_path(geometry: &str, width: f32, height: f32) -> Option<(String, bool)> {
    let (width, height) = (width.max(MIN_BOX_PT), height.max(MIN_BOX_PT));
    if let Some(path) = arrow_path(geometry, width, height) {
        return Some((path, true));
    }
    if let Some(path) = bracket_path(geometry, width, height) {
        return Some((path, false));
    }
    preset_path(geometry).map(|(path, filled)| (scale_path(path, width / 100.0, height / 100.0), filled))
}

/// The smallest side of a shape's box, in points: a shape without width
/// or height is drawn as a line.
const MIN_BOX_PT: f32 = 0.01;

/// The path of an arrow in a box `width` by `height` points, as Excel
/// draws it with its default adjustments: the head as long as half the
/// box's shorter side, the shaft half the box wide.
fn arrow_path(geometry: &str, width: f32, height: f32) -> Option<String> {
    let head = width.min(height) / 2.0;
    let (wide, high) = (width, height);
    Some(match geometry {
        "rightArrow" => polygon(&[
            [0.0, high / 4.0],
            [wide - head, high / 4.0],
            [wide - head, 0.0],
            [wide, high / 2.0],
            [wide - head, high],
            [wide - head, high * 0.75],
            [0.0, high * 0.75],
        ]),
        "leftArrow" => polygon(&[
            [wide, high / 4.0],
            [head, high / 4.0],
            [head, 0.0],
            [0.0, high / 2.0],
            [head, high],
            [head, high * 0.75],
            [wide, high * 0.75],
        ]),
        "downArrow" => polygon(&[
            [wide / 4.0, 0.0],
            [wide * 0.75, 0.0],
            [wide * 0.75, high - head],
            [wide, high - head],
            [wide / 2.0, high],
            [0.0, high - head],
            [wide / 4.0, high - head],
        ]),
        "upArrow" => polygon(&[
            [wide / 4.0, high],
            [wide * 0.75, high],
            [wide * 0.75, head],
            [wide, head],
            [wide / 2.0, 0.0],
            [0.0, head],
            [wide / 4.0, head],
        ]),
        "bentUpArrow" => {
            // Along the bottom, then up to a head at the top right; the
            // shaft and the head's length a quarter of the shorter side.
            let quarter = width.min(height) / 4.0;
            let (tip, shaft) = (wide - quarter, quarter / 2.0);
            polygon(&[
                [0.0, high - quarter],
                [tip - shaft, high - quarter],
                [tip - shaft, quarter],
                [wide - 2.0 * quarter, quarter],
                [tip, 0.0],
                [wide, quarter],
                [tip + shaft, quarter],
                [tip + shaft, high],
                [0.0, high],
            ])
        }
        "bentArrow" => bent_arrow_path(width, height),
        _ => return None,
    })
}

/// A closed path through `points`.
fn polygon(points: &[[f32; 2]]) -> String {
    let mut path = String::new();
    for (index, [left, top]) in points.iter().enumerate() {
        let _ = write!(
            path,
            "{}{},{}",
            if index == 0 { 'M' } else { 'L' },
            num(*left),
            num(*top)
        );
    }
    path.push('Z');
    path
}

/// The path of a bent arrow in a box `width` by `height` points: up from
/// the bottom left, round the corner, then right to its head, as Excel
/// draws it with its default adjustments.
fn bent_arrow_path(width: f32, height: f32) -> String {
    let quarter = width.min(height) / 4.0;
    let (shaft, top) = (quarter, quarter / 2.0);
    let bend = (width.min(height) * 0.4375).min(width - quarter).min(height - top);
    let inner = (bend - shaft).max(0.0);
    format!(
        "M0,{high}L0,{down}A{bend},{bend} 0 0 1 {bend},{top}L{neck},{top}L{neck},0L{wide},{quarter}L{neck},{base}L{neck},{under}L{turn},{under}A{inner},{inner} 0 0 0 {shaft},{low}L{shaft},{high}Z",
        down = num(top + bend),
        neck = num(width - quarter),
        quarter = num(quarter),
        base = num(2.0 * quarter),
        under = num(top + shaft),
        turn = num(shaft + inner),
        low = num(top + shaft + inner),
        high = num(height),
        wide = num(width),
        bend = num(bend),
        top = num(top),
        inner = num(inner),
        shaft = num(shaft),
    )
}

/// The path of a bracket or brace in a box `width` by `height` points, as
/// Excel draws it with its default adjustments: corners as round as a
/// sixth of the box's shorter side (a twelfth for a single bracket or a
/// brace).
fn bracket_path(geometry: &str, width: f32, height: f32) -> Option<String> {
    let side = width.min(height);
    let (right, bottom) = (num(width), num(height));
    Some(match geometry {
        "bracketPair" => {
            let round = side * 0.16667;
            format!(
                "M{round},{bottom}A{round},{round} 0 0 1 0,{low}L0,{round}A{round},{round} 0 0 1 {round},0M{inside},0A{round},{round} 0 0 1 {right},{round}L{right},{low}A{round},{round} 0 0 1 {inside},{bottom}",
                low = num(height - round),
                inside = num(width - round),
                round = num(round),
            )
        }
        "leftBracket" | "rightBracket" => {
            // A left one is a right one mirrored: its sides and turns swap.
            let round = side * 0.08333;
            let (near, far, turn) = if geometry == "rightBracket" {
                ("0", right.as_str(), 1)
            } else {
                (right.as_str(), "0", 0)
            };
            format!(
                "M{near},0A{right},{high} 0 0 {turn} {far},{high}L{far},{low}A{right},{high} 0 0 {turn} {near},{bottom}",
                high = num(round),
                low = num(height - round),
            )
        }
        "leftBrace" | "rightBrace" => {
            let (round, half) = (side * 0.08333, width / 2.0);
            let (near, far, turn, back) = if geometry == "rightBrace" {
                ("0", right.as_str(), 1, 0)
            } else {
                (right.as_str(), "0", 0, 1)
            };
            format!(
                "M{near},0A{middle},{high} 0 0 {turn} {middle},{high}L{middle},{above}A{middle},{high} 0 0 {back} {far},{centre}A{middle},{high} 0 0 {back} {middle},{below}L{middle},{low}A{middle},{high} 0 0 {turn} {near},{bottom}",
                middle = num(half),
                above = num(height / 2.0 - round),
                centre = num(height / 2.0),
                below = num(height / 2.0 + round),
                low = num(height - round),
                high = num(round),
            )
        }
        _ => return None,
    })
}

/// The path of a preset outline that is not a box, in a 100 × 100 box,
/// and whether it is filled (brackets are not); `None` for a box (a
/// rectangle, rounded or not, an ellipse), drawn with CSS.
pub(crate) fn preset_path(geometry: &str) -> Option<(&'static str, bool)> {
    let polygon = match geometry {
        "triangle" | "flowChartExtract" => "M50,0L100,100L0,100Z",
        "flowChartMerge" => "M0,0L100,0L50,100Z",
        "rtTriangle" => "M0,0L0,100L100,100Z",
        "diamond" | "flowChartDecision" => "M50,0L100,50L50,100L0,50Z",
        "leftUpArrow" => "M0,75L25,50L25,62.5L62.5,62.5L62.5,25L50,25L75,0L100,25L87.5,25L87.5,87.5L25,87.5L25,100Z",
        "uturnArrow" => {
            "M0,100L0,40Q0,0 40,0L50,0Q90,0 90,40L90,50L100,50L75,75L50,50L65,50L65,40Q65,25 50,25L40,25Q25,25 25,40L25,100Z"
        }
        "notchedRightArrow" => "M0,25L50,25L50,0L100,50L50,100L50,75L0,75L12.5,50Z",
        "leftRightArrow" => "M0,50L25,0L25,25L75,25L75,0L100,50L75,100L75,75L25,75L25,100Z",
        "upDownArrow" => "M50,0L100,25L75,25L75,75L100,75L50,100L0,75L25,75L25,25L0,25Z",
        "chevron" => "M0,0L75,0L100,50L75,100L0,100L25,50Z",
        "homePlate" => "M0,0L75,0L100,50L75,100L0,100Z",
        "parallelogram" | "flowChartInputOutput" => "M25,0L100,0L75,100L0,100Z",
        "trapezoid" => "M25,0L75,0L100,100L0,100Z",
        "flowChartManualOperation" => "M0,0L100,0L80,100L20,100Z",
        "hexagon" | "flowChartPreparation" => "M25,0L75,0L100,50L75,100L25,100L0,50Z",
        "pentagon" => "M50,0L100,38L81,100L19,100L0,38Z",
        "octagon" => "M29,0L71,0L100,29L100,71L71,100L29,100L0,71L0,29Z",
        "flowChartOffpageConnector" => "M0,0L100,0L100,80L50,100L0,80Z",
        "flowChartManualInput" => "M0,20L100,0L100,100L0,100Z",
        "flowChartDocument" => "M0,0L100,0L100,85C70,70 30,100 0,88Z",
        "flowChartMagneticDisk" | "can" => "M0,12A50,12 0 0 1 100,12L100,88A50,12 0 0 1 0,88Z M0,12A50,12 0 0 0 100,12",
        "mathMultiply" => "M20,5L50,35L80,5L95,20L65,50L95,80L80,95L50,65L20,95L5,80L35,50L5,20Z",
        "mathPlus" => "M38,10L62,10L62,38L90,38L90,62L62,62L62,90L38,90L38,62L10,62L10,38L38,38Z",
        "foldedCorner" => "M0,0L100,0L100,82L82,100L0,100Z M82,100L85,85L100,82",
        _ => {
            return match geometry {
                "bracePair" => Some((
                    "M20,0Q10,0 10,10L10,40Q10,50 0,50Q10,50 10,60L10,90Q10,100 20,100M80,0Q90,0 90,10L90,40Q90,50 100,50Q90,50 90,60L90,90Q90,100 80,100",
                    false,
                )),
                _ => None,
            };
        }
    };
    Some((polygon, true))
}

/// The box (left, top, width, height, in points from the top left of
/// the first cell, hidden rows and columns taking no room) of a frame on
/// the sheet `grid`; `None` for a frame without a corner or a size.
pub(crate) fn frame_box(grid: &Grid, frame: &Frame) -> Option<[f32; 4]> {
    let origin = Marker {
        row: 0,
        row_off_pt: 0.0,
        col: 0,
        col_off_pt: 0.0,
    };
    let offset = |marker: Marker| grid.sizes.span_pt(origin, marker, &grid.hidden_rows, &grid.hidden_cols);
    let (x, y, width, height) = match (frame.from, frame.to, frame.pos_pt) {
        (Some(from), Some(to), _) => {
            let (x, y) = offset(from);
            let (width, height) = grid.sizes.span_pt(from, to, &grid.hidden_rows, &grid.hidden_cols);
            (x, y, width, height)
        }
        (Some(from), None, _) => {
            let (x, y) = offset(from);
            let (width, height) = frame.ext_pt?;
            (x, y, width, height)
        }
        (None, _, Some((x, y))) => {
            let (width, height) = frame.ext_pt?;
            (x, y, width, height)
        }
        (None, _, None) => return None,
    };
    let [x, y, width, height] = match frame.part {
        Some([left, top, part_width, part_height]) => [
            x + left * width,
            y + top * height,
            part_width * width,
            part_height * height,
        ],
        None => [x, y, width, height],
    };
    [x, y, width, height]
        .iter()
        .all(|value| value.is_finite())
        .then_some([x, y, width, height])
}

/// The box of the drawn object `shape` of the sheet `grid`, as
/// [`frame_box`] gives it, but for a shape turned by about a quarter: its
/// anchor is the box of its turned footprint, as Excel writes it, so its
/// own box has the width and height swapped, about the same centre.
pub(crate) fn object_box(grid: &Grid, shape: &Shape) -> Option<[f32; 4]> {
    let [x, y, width, height] = frame_box(grid, &shape.frame)?;
    Some(match &shape.drawn {
        Drawn::Text(text) if shape.frame.part.is_none() && quarter_turned(text.rotation_deg) => {
            let (center_x, center_y) = (x + width / 2.0, y + height / 2.0);
            [center_x - height / 2.0, center_y - width / 2.0, height, width]
        }
        _ => [x, y, width, height],
    })
}

/// The outline of a shape (its fill and line, or its line with its arrow
/// heads) as an SVG file to draw in a box `width` by `height` points, and
/// where that picture goes relative to the box: left, top, width, height
/// (larger than the box, for a turned shape and for its line's width);
/// `None` for a shape with neither fill nor line.
pub(crate) fn shape_svg(shape: &TextShape, width: f32, height: f32) -> Option<(String, [f32; 4])> {
    let (body, stroke_pt) = outline_body(shape, width, height)?;
    // The picture holds the turned shape and its line, arrow heads
    // included.
    let turn = shape.rotation_deg.to_radians();
    let (sin, cos) = (turn.sin().abs(), turn.cos().abs());
    // A callout's tip can lie outside the box.
    let beyond = callout_tip(shape, width, height).map_or(0.0, |[tip_x, tip_y]| {
        (-tip_x).max(tip_x - width).max(-tip_y).max(tip_y - height).max(0.0)
    });
    let pad = stroke_pt * ARROW_SIZE as f32 + beyond;
    let outer = [
        width * cos + height * sin + 2.0 * pad,
        width * sin + height * cos + 2.0 * pad,
    ];
    let left = (outer[0] - width) / 2.0;
    let top = (outer[1] - height) / 2.0;
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><g transform=\"rotate({} {} {}) translate({} {})\">{body}</g></svg>",
        num(shape.rotation_deg),
        num(outer[0] / 2.0),
        num(outer[1] / 2.0),
        num(left),
        num(top),
        w = num(outer[0]),
        h = num(outer[1]),
    );
    Some((svg, [-left, -top, outer[0], outer[1]]))
}

/// The SVG elements of a shape's outline in a box `width` by `height`
/// points, and the width of its line; `None` for a shape with neither fill
/// nor line.
fn outline_body(shape: &TextShape, width: f32, height: f32) -> Option<(String, f32)> {
    let geometry = shape.geometry.as_deref().unwrap_or("rect");
    let stroke_pt = shape.line_width_pt.unwrap_or(DEFAULT_LINE_PT).max(MIN_LINE_PT);
    let stroke = shape.line.map(hex);
    let points = line_points(geometry, width, height);
    if stroke.is_none() && (shape.fill.is_none() || points.is_some()) {
        return None;
    }
    let paint = |fill: Option<Rgb>| {
        format!(
            " fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}",
            fill.map_or_else(|| "none".to_string(), hex),
            stroke.as_deref().unwrap_or("none"),
            num(stroke_pt),
            if shape.dashed { " stroke-dasharray=\"4 3\"" } else { "" }
        )
    };
    let mirror = |[x, y]: [f32; 2]| {
        [
            if shape.flip_h { width - x } else { x },
            if shape.flip_v { height - y } else { y },
        ]
    };
    let body = if let Some(points) = points {
        let path: Vec<String> = points
            .into_iter()
            .map(mirror)
            .map(|[x, y]| format!("{},{}", num(x), num(y)))
            .collect();
        let stroke = stroke.as_deref().unwrap_or("none");
        let marker = |name: &str, used: bool| {
            if used {
                format!(" marker-{name}=\"url(#a)\"")
            } else {
                String::new()
            }
        };
        format!(
            "<defs><marker id=\"a\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"{ARROW_SIZE}\" markerHeight=\"{ARROW_SIZE}\" orient=\"auto-start-reverse\"><path d=\"M0,0L10,5L0,10z\" fill=\"{stroke}\"/></marker></defs><polyline points=\"{}\"{}{}{}/>",
            path.join(" "),
            paint(None),
            marker("start", shape.head),
            marker("end", shape.tail),
        )
    } else if let Some((path, filled)) = shape_path(shape, width, height) {
        let mut scaled = path;
        if shape.flip_h || shape.flip_v {
            scaled = format!(
                "{scaled}\" transform=\"translate({} {}) scale({} {})",
                if shape.flip_h { num(width) } else { "0".into() },
                if shape.flip_v { num(height) } else { "0".into() },
                if shape.flip_h { -1 } else { 1 },
                if shape.flip_v { -1 } else { 1 }
            );
        }
        format!(
            "<path d=\"{scaled}\"{}/>",
            paint(if filled { shape.fill } else { None })
        )
    } else {
        let radius = match geometry {
            "ellipse" | "flowChartConnector" => format!(" rx=\"{}\" ry=\"{}\"", num(width / 2.0), num(height / 2.0)),
            "roundRect" | "round2SameRect" | "snipRoundRect" | "flowChartAlternateProcess" => {
                format!(" rx=\"{}\"", num(ROUND_CORNER_PT.min(width / 2.0).min(height / 2.0)))
            }
            "flowChartTerminator" => format!(" rx=\"{}\"", num(width.min(height) / 2.0)),
            _ => String::new(),
        };
        format!(
            "<rect width=\"{}\" height=\"{}\"{radius}{}/>",
            num(width),
            num(height),
            paint(shape.fill)
        )
    };
    Some((body, stroke_pt))
}

/// The path `path` of a 100 × 100 box scaled by `scale_x` and `scale_y`:
/// its coordinates, and the radii of its arcs (whose angle and flags stay).
fn scale_path(path: &str, scale_x: f32, scale_y: f32) -> String {
    let mut out = String::new();
    let mut command = ' ';
    let mut index = 0;
    let mut number = String::new();
    let flush = |number: &mut String, out: &mut String, command: char, index: &mut usize| {
        if number.is_empty() {
            return;
        }
        let value: f32 = number.parse().unwrap_or(0.0);
        // In an arc (rx ry angle large sweep x y), the angle and the
        // flags are not lengths.
        let position = if command == 'A' { *index % 7 } else { *index % 2 };
        let scaled = match (command, position) {
            ('A', 2..=4) => value,
            ('A', 0 | 5) | (_, 0) => value * scale_x,
            _ => value * scale_y,
        };
        if !out.ends_with(|ch: char| ch.is_ascii_alphabetic()) {
            out.push(' ');
        }
        out.push_str(&num(scaled));
        *index += 1;
        number.clear();
    };
    for ch in path.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' => {
                flush(&mut number, &mut out, command, &mut index);
                command = ch.to_ascii_uppercase();
                index = 0;
                out.push(ch);
            }
            '0'..='9' | '.' | '-' => number.push(ch),
            _ => flush(&mut number, &mut out, command, &mut index),
        }
    }
    flush(&mut number, &mut out, command, &mut index);
    out
}

/// The column letters of the 0-based column `col` (`A`, `Z`, `AA`).
fn column_name(col: u32) -> String {
    let mut name = Vec::new();
    let mut number = col + 1;
    while number > 0 {
        let rest = (number - 1) % 26;
        name.push(b'A' + rest as u8);
        number = (number - 1) / 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

/// The Japanese fonts of Windows (their names as `table_name` folds them)
/// and the families a page shows where they are not installed: the font
/// by its English name, then a font of its metrics (the IPA fonts), the
/// Mac's and Noto's. The proportional (P) fonts have their Latin letters
/// and digits in Arial or Times New Roman, half an em like theirs, as the
/// printed PDF does.
const JAPANESE_FAMILIES: &[(&[&str], &str)] = &[
    (
        &["ms pゴシック", "ms pgothic", "ms ui gothic"],
        "'MS PGothic',Arial,'IPAPGothic','Hiragino Sans','Noto Sans CJK JP','Noto Sans JP',sans-serif",
    ),
    (
        &["ms ゴシック", "ms gothic"],
        "'MS Gothic','IPAGothic','Hiragino Sans','Noto Sans CJK JP','Noto Sans JP',sans-serif",
    ),
    (
        &["ms p明朝", "ms pmincho"],
        "'MS PMincho','Times New Roman','IPAPMincho','Hiragino Mincho ProN','Noto Serif CJK JP','Noto Serif JP',serif",
    ),
    (
        &["ms 明朝", "ms mincho"],
        "'MS Mincho','IPAMincho','Hiragino Mincho ProN','Noto Serif CJK JP','Noto Serif JP',serif",
    ),
    (
        &["游ゴシック", "yu gothic", "メイリオ", "meiryo"],
        "'Yu Gothic','Meiryo','IPAexGothic','Hiragino Sans','Noto Sans CJK JP','Noto Sans JP',sans-serif",
    ),
    (
        &["游明朝", "yu mincho"],
        "'Yu Mincho','IPAexMincho','Hiragino Mincho ProN','Noto Serif CJK JP','Noto Serif JP',serif",
    ),
];

/// The CSS font families of the font `name`, with fallbacks of its kind
/// (serif for a Mincho or Times font, else sans-serif) and for Japanese.
pub(crate) fn family(name: Option<&str>) -> String {
    let fallback = "Carlito,'Segoe UI',Arial,'Noto Sans CJK JP','Noto Sans JP',sans-serif";
    let Some(name) = name.map(str::trim).filter(|name| !name.is_empty()) else {
        return format!("Calibri,{fallback}");
    };
    let quoted = format!("'{}'", name.replace(['\'', '\\', '"', ';', '<', '>', '{', '}'], ""));
    let folded = crate::layout::fonts::table_name(name);
    if let Some((_, stand_ins)) = JAPANESE_FAMILIES
        .iter()
        .find(|(names, _)| names.contains(&folded.as_str()))
    {
        return format!("{quoted},{stand_ins}");
    }
    let lower = name.to_lowercase();
    let serif = ["mincho", "明朝", "times", "serif", "georgia", "cambria", "century"]
        .iter()
        .any(|kind| lower.contains(kind))
        && !["sans", "gothic", "ゴシック"].iter().any(|kind| lower.contains(kind));
    if serif {
        format!("{quoted},'Times New Roman','Noto Serif CJK JP','Noto Serif JP',serif")
    } else {
        format!("{quoted},{fallback}")
    }
}

/// A colour as CSS (`#rrggbb`).
fn hex([red, green, blue]: Rgb) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// A length as CSS: at most two decimals, without trailing zeros.
fn num(value: f32) -> String {
    let text = format!("{:.2}", if value.is_finite() { value } else { 0.0 });
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::zip::{Compression, ZipWriter};

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    const RELS_NS: &str = r#"xmlns="http://schemas.openxmlformats.org/package/2006/relationships""#;
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

    /// A workbook of one sheet, "Data": a bold red title in A1 that runs
    /// over B1, A2:B2 merged on a yellow fill with a border, a hidden
    /// column C, a cell D1 linked to a script, no gridlines, and a picture
    /// anchored at E2 (two by one inches); and a second sheet, "Notes",
    /// whose A1 has the look of the merge and B1 one of its own, linked to
    /// the first sheet, and whose A2:A3 merge takes its bottom border from
    /// A3.
    fn workbook() -> Vec<u8> {
        let sheet = format!(
            r#"<worksheet {NS}><sheetViews><sheetView showGridLines="0"/></sheetViews><cols><col min="3" max="3" width="9" hidden="1"/></cols><sheetData><row r="1"><c r="A1" s="1" t="inlineStr"><is><t>A long title</t></is></c><c r="D1" t="inlineStr"><is><t>Click</t></is></c></row><row r="2" ht="30" customHeight="1"><c r="A2" s="2" t="inlineStr"><is><t>Merged</t></is></c><c r="B2" s="2"/></row></sheetData><mergeCells count="1"><mergeCell ref="A2:B2"/></mergeCells><hyperlinks><hyperlink ref="D1" r:id="rId2"/></hyperlinks><drawing r:id="rId1"/></worksheet>"#
        );
        let styles = format!(
            r#"<styleSheet {NS}><fonts><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="14"/><color rgb="FFFF0000"/><name val="Calibri"/></font></fonts><fills><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/></patternFill></fill></fills><borders><border/><border><bottom style="thin"><color theme="4" tint="-0.5"/></bottom></border></borders><cellXfs><xf/><xf fontId="1"/><xf fillId="2" borderId="1"><alignment horizontal="center"/></xf><xf><alignment vertical="top"/></xf></cellXfs></styleSheet>"#
        );
        let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><xdr:oneCellAnchor><xdr:from><xdr:col>4</xdr:col><xdr:row>1</xdr:row></xdr:from><xdr:ext cx="1828800" cy="914400"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="3" name="Logo"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic><xdr:clientData/></xdr:oneCellAnchor></xdr:wsDr>"#;
        let mut png = Vec::new();
        image::RgbImage::new(4, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let parts: Vec<(String, Vec<u8>)> = vec![
            ("_rels/.rels".into(), format!(r#"<Relationships {RELS_NS}><Relationship Id="rId1" Type="{REL}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#).into_bytes()),
            ("xl/workbook.xml".into(), format!(r#"<workbook {NS}><sheets><sheet name="Data" sheetId="1" r:id="rId1"/><sheet name="Notes" sheetId="2" r:id="rId3"/></sheets></workbook>"#).into_bytes()),
            ("xl/_rels/workbook.xml.rels".into(), format!(r#"<Relationships {RELS_NS}><Relationship Id="rId1" Type="{REL}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{REL}/styles" Target="styles.xml"/><Relationship Id="rId3" Type="{REL}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#).into_bytes()),
            ("xl/worksheets/sheet2.xml".into(), format!(r#"<worksheet {NS}><sheetData><row r="1"><c r="A1" s="2" t="inlineStr"><is><t>Note</t></is></c><c r="B1" s="3" t="inlineStr"><is><t>Small</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Tall</t></is></c></row><row r="3"><c r="A3" s="2"/></row></sheetData><mergeCells count="1"><mergeCell ref="A2:A3"/></mergeCells><hyperlinks><hyperlink ref="B1" location="'Data'!A1"/></hyperlinks></worksheet>"#).into_bytes()),
            ("xl/styles.xml".into(), styles.into_bytes()),
            ("xl/worksheets/sheet1.xml".into(), sheet.into_bytes()),
            ("xl/worksheets/_rels/sheet1.xml.rels".into(), format!(r#"<Relationships {RELS_NS}><Relationship Id="rId1" Type="{REL}/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId2" Type="{REL}/hyperlink" Target="javascript:alert(1)" TargetMode="External"/></Relationships>"#).into_bytes()),
            ("xl/drawings/drawing1.xml".into(), drawing.as_bytes().to_vec()),
            ("xl/drawings/_rels/drawing1.xml.rels".into(), format!(r#"<Relationships {RELS_NS}><Relationship Id="rId1" Type="{REL}/image" Target="../media/image1.png"/></Relationships>"#).into_bytes()),
            ("xl/media/image1.png".into(), png),
        ];
        let mut zip = ZipWriter::default();
        for (name, contents) in &parts {
            zip.add(name, contents, Compression::Deflated);
        }
        zip.finish()
    }

    #[test]
    fn a_sheet_is_drawn_with_its_grid_look_and_pictures() {
        let html = to_html(
            &workbook(),
            &ExcelConfig::default(),
            &ViewConfig::default(),
            Some("book.xlsx"),
        )
        .unwrap();
        assert!(html.contains("<title>book.xlsx</title>"));
        assert!(html.contains(r##"<a href="#sheet-1">Data</a>"##));
        // No gridlines; the hidden column C is not drawn.
        assert!(html.contains(r#"<table class="cells" "#));
        assert!(html.contains(">A</th>") && html.contains(">D</th>") && !html.contains(">C</th>"));
        // The title in its font, running over the empty B1.
        assert!(html.contains(".cells td.k0{font-size:14pt;color:#ff0000;font-weight:700;}"));
        assert!(html.contains(r#"<td class="k0 ov"><div style="max-height:15pt;width:93pt;">"#));
        // The merge, centred on its fill, with its dark blue border.
        assert!(html.contains(r#"<td colspan="2" class="k2">"#));
        assert!(html.contains(
            ".cells td.k2{background:#ffff00;box-shadow:none;border-bottom:1px solid #203864;text-align:center;}"
        ));
        assert!(html.contains("height:30pt"));
        // A link to a script is no link.
        assert!(!html.contains("javascript") && html.contains(">Click<"));
        // The picture at E2: after columns A, B and D (48 points each) and
        // the row numbers; under row 1 (15 points) and the letters.
        assert!(html.contains(r#"<img class="pic" style="left:180pt;top:30pt;width:144pt;height:72pt" alt="Logo" src="data:image/png;base64,"#), "{html}");
        // The classes are the page's: the second sheet's A1 has the class
        // of the same look on the first, its new look a new class, and no
        // class is defined twice.
        let notes = &html[html.find(r#"id="sheet-2""#).unwrap()..];
        assert!(notes.contains(r#"<td class="k2">"#) && notes.contains(r#"<td class="k3">"#));
        // An internal link shows its sheet; a merge's bottom border is its
        // bottom row's.
        assert!(notes.contains(r##"<a href="#sheet-1">Small</a>"##));
        assert!(
            notes.contains(r#"<td rowspan="2" style="border-bottom:1px solid #203864;">"#),
            "{notes}"
        );
        for name in ["k0", "k1", "k2", "k3"] {
            assert!(
                html.matches(&format!(".cells td.{name}{{")).count() <= 1,
                "{name} twice"
            );
        }
    }

    #[test]
    fn columns_are_named_with_letters() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(16_383), "XFD");
    }

    #[test]
    fn fonts_get_fallbacks_of_their_kind() {
        assert!(family(Some("ＭＳ 明朝")).ends_with("serif") && family(Some("ＭＳ 明朝")).contains("Serif CJK"));
        assert!(family(Some("Calibri")).ends_with("sans-serif"));
        // A Japanese font of Windows falls back on a font of its metrics.
        let gothic = family(Some("ＭＳ Ｐゴシック"));
        assert!(
            gothic.starts_with("'ＭＳ Ｐゴシック','MS PGothic',Arial,'IPAPGothic'"),
            "{gothic}"
        );
        assert!(family(Some("Century Gothic")).ends_with("sans-serif"));
        assert!(family(None).starts_with("Calibri,"));
        // A name cannot close the style attribute.
        assert!(!family(Some("x\";<b>{}")).contains(['"', '<', ';', '{']));
    }

    #[test]
    fn an_arrow_s_head_and_a_bracket_s_corners_keep_to_the_shorter_side() {
        // A long arrow keeps a head as long as half its height.
        let (arrow, filled) = outline_path("rightArrow", 200.0, 20.0).unwrap();
        assert!(filled && arrow.starts_with("M0,5L190,5L190,0L200,10"), "{arrow}");
        // A wide bracket pair turns its corners within a sixth of its height.
        let (brackets, filled) = outline_path("bracketPair", 300.0, 30.0).unwrap();
        assert!(!filled && brackets.starts_with("M5,30A5,5 0 0 1 0,25"), "{brackets}");
        let (left, _) = outline_path("leftBrace", 10.0, 60.0).unwrap();
        assert!(left.starts_with("M10,0A5,0.83 0 0 0 5,0.83"), "{left}");
    }

    /// A callout's wedge leaves the side its tip lies beyond, and its
    /// box is drawn as the shape's drawn outline when it has one.
    #[test]
    fn a_callout_points_at_its_tip() {
        let callout = TextShape {
            geometry: Some("wedgeRectCallout".into()),
            adjust: vec![-90_000.0, 50_000.0],
            ..TextShape::default()
        };
        // The tip is 0.9 of the width left of the centre, a half below:
        // beyond the left side, between 10/12 and 7/12 of the height.
        let (path, filled) = shape_path(&callout, 120.0, 60.0).unwrap();
        assert!(filled);
        assert_eq!(path, "M0,0L120,0L120,60L0,60L0,50L-48,60L0,35Z");
        let drawn = TextShape {
            custom_path: Some("M0,0L100,50L0,100Z".into()),
            ..callout
        };
        assert_eq!(shape_path(&drawn, 20.0, 10.0).unwrap().0, "M0 0L20 5L0 10Z");
    }

    #[test]
    fn preset_paths_are_scaled_to_their_box() {
        assert_eq!(scale_path("M50,0L100,100Z", 2.0, 0.5), "M100 0L200 50Z");
        assert_eq!(
            scale_path("M0,12A50,12 0 0 1 100,12", 2.0, 0.5),
            "M0 6A100 6 0 0 1 200 6"
        );
    }

    #[test]
    fn a_turned_shape_s_picture_holds_its_footprint() {
        let shape = TextShape {
            fill: Some([255, 0, 0]),
            rotation_deg: 90.0,
            ..TextShape::default()
        };
        let (svg, [left, top, width, height]) = shape_svg(&shape, 40.0, 10.0).unwrap();
        assert!(
            svg.contains("rotate(90 ")
                && svg.contains(r##"<rect width="40" height="10" fill="#ff0000" stroke="none""##)
        );
        // The turned box, 10 wide and 40 tall, with room for a line of the
        // default width (3 points around), about the box's centre.
        let near = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(near(width, 16.0) && near(height, 46.0), "{width} {height}");
        assert!(near(left, 12.0) && near(top, -18.0), "{left} {top}");
        assert!(shape_svg(&TextShape::default(), 10.0, 10.0).is_none());
    }

    #[test]
    fn lengths_are_short() {
        assert_eq!(num(48.0), "48");
        assert_eq!(num(14.25), "14.25");
        assert_eq!(num(f32::NAN), "0");
    }
}
