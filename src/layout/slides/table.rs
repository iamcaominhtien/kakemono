//! A slide's table (`a:tbl`) drawn in its frame: its columns as wide as
//! its grid says, its rows as high as they say or as their text needs,
//! merged cells across their columns and rows, and each cell's fill,
//! borders and text from the table's style (`ppt/tableStyles.xml`: the
//! whole table, banded rows and columns, first and last row and column,
//! as the table turns them on) and then from the cell's own properties.

use super::look::{DEFAULT_LINE_PT, Look, Paint, width_pt};
use super::text::{self, Body, Context};
use crate::chart::Rgb;
use crate::layout::display::Item;
use crate::layout::para::Ctx;
use crate::powerpoint::package::EMU_PER_PT;
use crate::word::docx::parse;
use crate::word::xml::{DRAWING_NS, child, children};
use roxmltree::Node;

/// PowerPoint's margins of a cell that names none, in EMU: left, top,
/// right, bottom.
const DEFAULT_MARGINS_EMU: [f32; 4] = [91_440.0, 45_720.0, 91_440.0, 45_720.0];
/// The most rows or columns of a table drawn (a guard against a hostile
/// file).
const MAX_CELLS: usize = 10_000;

/// PowerPoint's default table style, "Medium Style 2 - Accent 1", as
/// PowerPoint writes it in `ppt/tableStyles.xml`: a table may name it
/// without the deck holding it.
const DEFAULT_STYLE: &str = concat!(
    r#"<a:tblStyleLst xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">"#,
    r#"<a:tblStyle styleId="{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}" styleName="Medium Style 2 - Accent 1">"#,
    r#"<a:wholeTbl><a:tcTxStyle><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="dk1"/></a:tcTxStyle>"#,
    r#"<a:tcStyle><a:tcBdr>"#,
    r#"<a:left><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:left>"#,
    r#"<a:right><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:right>"#,
    r#"<a:top><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top>"#,
    r#"<a:bottom><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom>"#,
    r#"<a:insideH><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideH>"#,
    r#"<a:insideV><a:ln w="12700"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:insideV>"#,
    r#"</a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="20000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:wholeTbl>"#,
    r#"<a:band1H><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1H>"#,
    r#"<a:band2H><a:tcStyle><a:tcBdr/></a:tcStyle></a:band2H>"#,
    r#"<a:band1V><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"><a:tint val="40000"/></a:schemeClr></a:solidFill></a:fill></a:tcStyle></a:band1V>"#,
    r#"<a:band2V><a:tcStyle><a:tcBdr/></a:tcStyle></a:band2V>"#,
    r#"<a:lastCol><a:tcTxStyle b="on"><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastCol>"#,
    r#"<a:firstCol><a:tcTxStyle b="on"><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr/><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstCol>"#,
    r#"<a:lastRow><a:tcTxStyle b="on"><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:top><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:top></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:lastRow>"#,
    r#"<a:firstRow><a:tcTxStyle b="on"><a:fontRef idx="minor"><a:prstClr val="black"/></a:fontRef><a:schemeClr val="lt1"/></a:tcTxStyle><a:tcStyle><a:tcBdr><a:bottom><a:ln w="38100"><a:solidFill><a:schemeClr val="lt1"/></a:solidFill></a:ln></a:bottom></a:tcBdr><a:fill><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></a:fill></a:tcStyle></a:firstRow>"#,
    r#"</a:tblStyle></a:tblStyleLst>"#
);

/// The look a table style or a cell gives a cell.
#[derive(Debug, Clone, Copy, Default)]
struct CellLook {
    /// Its fill; `Some(None)` for none.
    fill: Option<Option<Rgb>>,
    /// Its borders: left, top, right, bottom; `Some(None)` for none.
    borders: [Option<Option<(Rgb, f32)>>; 4],
    /// The weight of its text.
    bold: Option<bool>,
    /// The colour of its text.
    color: Option<Rgb>,
}

impl CellLook {
    /// This look with what `other` says laid over it.
    fn over(mut self, other: Self) -> Self {
        self.fill = other.fill.or(self.fill);
        for (own, given) in self.borders.iter_mut().zip(other.borders) {
            *own = given.or(*own);
        }
        self.bold = other.bold.or(self.bold);
        self.color = other.color.or(self.color);
        self
    }
}

/// A cell of the table, placed.
struct Placed<'a, 'i> {
    /// The cell (`a:tc`).
    cell: Node<'a, 'i>,
    /// Its row and column.
    at: (usize, usize),
    /// The rows and columns it spans.
    span: (usize, usize),
}

/// Draws the table `table` in the frame `rect` into `items`; its text
/// takes its levels' properties from `lists` (the master's other text
/// style, the presentation's defaults), its style from `styles` (the
/// deck's table styles part).
pub(super) fn draw(
    ctx: &mut Ctx<'_>,
    context: &Context<'_, '_, '_>,
    table: Node<'_, '_>,
    lists: &[Node<'_, '_>],
    styles: Option<&str>,
    rect: [f32; 4],
    items: &mut Vec<Item>,
) {
    let look = context.look;
    let emu = |node: Node<'_, '_>, name: &str| {
        node.attribute(name)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map_or(0.0, |emu| emu / EMU_PER_PT)
    };
    let widths: Vec<f32> = child(table, DRAWING_NS, "tblGrid")
        .into_iter()
        .flat_map(|grid| children(grid, DRAWING_NS, "gridCol"))
        .take(MAX_CELLS)
        .map(|col| emu(col, "w"))
        .collect();
    let rows: Vec<Node<'_, '_>> = children(table, DRAWING_NS, "tr").take(MAX_CELLS).collect();
    if widths.is_empty() || rows.is_empty() {
        return;
    }
    let props = child(table, DRAWING_NS, "tblPr");
    let on = |name: &str| {
        props
            .and_then(|props| props.attribute(name))
            .is_some_and(|value| matches!(value, "1" | "true"))
    };
    let style_doc = styles.and_then(|xml| parse(xml).ok());
    let style_id = props
        .and_then(|props| child(props, DRAWING_NS, "tableStyleId"))
        .and_then(|id| id.text())
        .map(str::trim);
    let find = |doc: &'_ roxmltree::Document<'_>| {
        doc.root_element().children().position(|node| {
            node.has_tag_name((DRAWING_NS, "tblStyle")) && node.attribute("styleId").map(str::trim) == style_id
        })
    };
    // PowerPoint's default style, which a deck need not write out.
    let built_in = parse(DEFAULT_STYLE).ok();
    let style = [style_doc.as_ref(), built_in.as_ref()]
        .into_iter()
        .flatten()
        .find_map(|doc| doc.root_element().children().nth(find(doc)?));
    let placed = place_cells(&rows, widths.len());
    let (row_count, col_count) = (rows.len(), widths.len());
    let cell_look = |at: (usize, usize)| {
        let (row, col) = at;
        let edges = [col == 0, row == 0, col + 1 == col_count, row + 1 == row_count];
        style_parts(&on, at, (row_count, col_count))
            .into_iter()
            .filter_map(|name| style.and_then(|style| child(style, DRAWING_NS, name)))
            .map(|part| style_look(part, look, edges))
            .fold(CellLook::default(), CellLook::over)
    };
    // Row heights: as given, or as their one-row cells' text needs.
    let mut heights: Vec<f32> = rows.iter().map(|row| emu(*row, "h")).collect();
    let mut looks = Vec::with_capacity(placed.len());
    for cell in &placed {
        let own = own_look(cell.cell, look);
        let total = cell_look(cell.at).over(own);
        looks.push(total);
        if cell.span.0 == 1 {
            let width: f32 = widths[cell.at.1..cell.at.1 + cell.span.1].iter().sum();
            let (_, needed) = cell_text(ctx, context, cell.cell, lists, total, [0.0, 0.0, width, 0.0]);
            heights[cell.at.0] = heights[cell.at.0].max(needed);
        }
    }
    let [x, y, _, _] = rect;
    let (col_x, row_y) = (offsets(x, &widths), offsets(y, &heights));
    let mut lines = Vec::new();
    for (cell, total) in placed.iter().zip(&looks) {
        let (row, col) = cell.at;
        let edges = [
            col_x[col],
            row_y[row],
            col_x[col + cell.span.1],
            row_y[row + cell.span.0],
        ];
        paint_cell(ctx, context, cell.cell, lists, *total, edges, items);
        lines.extend(borders(total, edges));
    }
    // Borders over every cell's fill and text.
    items.append(&mut lines);
}

/// Draws the fill and text of the cell `cell`, of the look `total`,
/// between its edges `edges` (left, top, right, bottom).
fn paint_cell(
    ctx: &mut Ctx<'_>,
    context: &Context<'_, '_, '_>,
    cell: Node<'_, '_>,
    lists: &[Node<'_, '_>],
    total: CellLook,
    edges: [f32; 4],
    items: &mut Vec<Item>,
) {
    let [left, top, right, bottom] = edges;
    if let Some(Some(color)) = total.fill {
        items.push(Item::Rect {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
            color,
        });
    }
    let (mut laid, _) = cell_text(
        ctx,
        context,
        cell,
        lists,
        total,
        [left, top, right - left, bottom - top],
    );
    items.append(&mut laid);
}

/// The borders of a cell of the look `total` between its edges `edges`
/// (left, top, right, bottom).
fn borders(total: &CellLook, edges: [f32; 4]) -> Vec<Item> {
    let [left, top, right, bottom] = edges;
    let sides = [
        (left, top, left, bottom),
        (left, top, right, top),
        (right, top, right, bottom),
        (left, bottom, right, bottom),
    ];
    total
        .borders
        .iter()
        .zip(sides)
        .filter_map(|(border, (x1, y1, x2, y2))| {
            let (color, width) = (*border)??;
            Some(Item::Line {
                x1,
                y1,
                x2,
                y2,
                width,
                color,
                dash: None,
            })
        })
        .collect()
}

/// The cells of the table's rows `rows` on its grid of `cols` columns,
/// each with its place and span; the parts merged cells cover left out.
fn place_cells<'a, 'i>(rows: &[Node<'a, 'i>], cols: usize) -> Vec<Placed<'a, 'i>> {
    let mut placed = Vec::new();
    for (row_at, row) in rows.iter().enumerate() {
        for (col_at, cell) in children(*row, DRAWING_NS, "tc").take(cols).enumerate() {
            let flag = |name: &str| matches!(cell.attribute(name), Some("1" | "true"));
            if flag("hMerge") || flag("vMerge") {
                continue;
            }
            let count = |name: &str| {
                cell.attribute(name)
                    .and_then(|value| value.trim().parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1)
            };
            placed.push(Placed {
                cell,
                at: (row_at, col_at),
                span: (
                    count("rowSpan").min(rows.len() - row_at),
                    count("gridSpan").min(cols - col_at),
                ),
            });
        }
    }
    placed
}

/// The parts of a table style (`wholeTbl`, `band1H`, `firstRow`, ...) that
/// apply to the cell at `at` of a table of `counts` rows and columns, in
/// the order they are laid over each other; `on` tells which the table
/// turns on (`firstRow`, `bandRow`, ...).
fn style_parts(on: &dyn Fn(&str) -> bool, at: (usize, usize), counts: (usize, usize)) -> Vec<&'static str> {
    let ((row, col), (rows, cols)) = (at, counts);
    let mut parts = vec!["wholeTbl"];
    let body_row = if on("firstRow") { row.checked_sub(1) } else { Some(row) };
    if on("bandRow")
        && let Some(body_row) = body_row
    {
        parts.push(if body_row % 2 == 0 { "band1H" } else { "band2H" });
    }
    let body_col = if on("firstCol") { col.checked_sub(1) } else { Some(col) };
    if on("bandCol")
        && let Some(body_col) = body_col
    {
        parts.push(if body_col % 2 == 0 { "band1V" } else { "band2V" });
    }
    for (name, applies) in [
        ("lastCol", col + 1 == cols),
        ("firstCol", col == 0),
        ("lastRow", row + 1 == rows),
        ("firstRow", row == 0),
    ] {
        if applies && on(name) {
            parts.push(name);
        }
    }
    parts
}

/// The edges of rows or columns of the sizes `sizes`, from `start`.
fn offsets(start: f32, sizes: &[f32]) -> Vec<f32> {
    std::iter::once(start)
        .chain(sizes.iter().scan(start, |at, size| {
            *at += size;
            Some(*at)
        }))
        .collect()
}

/// The items of the text of the cell `cell` laid out in `rect`, and the
/// height it needs with its margins.
fn cell_text(
    ctx: &mut Ctx<'_>,
    context: &Context<'_, '_, '_>,
    cell: Node<'_, '_>,
    lists: &[Node<'_, '_>],
    total: CellLook,
    rect: [f32; 4],
) -> (Vec<Item>, f32) {
    let Some(body) = child(cell, DRAWING_NS, "txBody") else {
        return (Vec::new(), 0.0);
    };
    let props = child(cell, DRAWING_NS, "tcPr");
    let margin = |at: usize, name: &str| {
        props
            .and_then(|props| props.attribute(name))
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .unwrap_or(DEFAULT_MARGINS_EMU[at])
            / EMU_PER_PT
    };
    let styles: Vec<Node<'_, '_>> = child(body, DRAWING_NS, "lstStyle")
        .into_iter()
        .chain(lists.iter().copied())
        .collect();
    let text_body = Body {
        body,
        // The cell's properties give its text's anchor.
        props: props.into_iter().collect(),
        styles,
        font_color: total.color,
        font_major: None,
        font_bold: total.bold,
        turn_deg: 0.0,
        insets_pt: Some([
            margin(0, "marL"),
            margin(1, "marT"),
            margin(2, "marR"),
            margin(3, "marB"),
        ]),
    };
    text::draw(ctx, context, &text_body, rect)
}

/// The look a part of a table style (`a:wholeTbl`, `a:firstRow`, ...)
/// gives a cell on the table's edges `edges` (left, top, right, bottom):
/// its outer borders where the cell is on the edge, its inner ones
/// (`insideH`, `insideV`) elsewhere.
fn style_look(part: Node<'_, '_>, look: &Look<'_, '_>, edges: [bool; 4]) -> CellLook {
    let mut out = CellLook::default();
    if let Some(text) = child(part, DRAWING_NS, "tcTxStyle") {
        match text.attribute("b") {
            Some("on") => out.bold = Some(true),
            Some("off") => out.bold = Some(false),
            _ => {}
        }
        out.color = look.first_color(text).or_else(|| {
            let font = child(text, DRAWING_NS, "fontRef")?;
            look.color(font, [0; 3])
        });
    }
    let Some(style) = child(part, DRAWING_NS, "tcStyle") else {
        return out;
    };
    if let Some(fill) = child(style, DRAWING_NS, "fill") {
        out.fill = look.fill(fill, [0; 3]).map(|paint| match paint {
            Some(Paint::Solid(rgb) | Paint::Clear(rgb, _) | Paint::Gradient(rgb, _)) => Some(rgb),
            _ => None,
        });
    } else if let Some(reference) = child(style, DRAWING_NS, "fillRef") {
        out.fill = Some(match look.fill_ref(reference) {
            Some(Paint::Solid(rgb) | Paint::Clear(rgb, _) | Paint::Gradient(rgb, _)) => Some(rgb),
            _ => None,
        });
    }
    if let Some(borders) = child(style, DRAWING_NS, "tcBdr") {
        let names = [
            ("left", "insideV"),
            ("top", "insideH"),
            ("right", "insideV"),
            ("bottom", "insideH"),
        ];
        for ((slot, (outer, inner)), edge) in out.borders.iter_mut().zip(names).zip(edges) {
            let name = if edge { outer } else { inner };
            let Some(side) = child(borders, DRAWING_NS, name) else {
                continue;
            };
            if let Some(line) = child(side, DRAWING_NS, "ln") {
                *slot = Some(line_look(line, look));
            } else if let Some(reference) = child(side, DRAWING_NS, "lnRef") {
                *slot = Some(
                    look.line_ref(reference)
                        .map(|(rgb, width)| (rgb, width.unwrap_or(DEFAULT_LINE_PT))),
                );
            }
        }
    }
    out
}

/// The look a cell's own properties (`a:tcPr`) give it.
fn own_look(cell: Node<'_, '_>, look: &Look<'_, '_>) -> CellLook {
    let mut out = CellLook::default();
    let Some(props) = child(cell, DRAWING_NS, "tcPr") else {
        return out;
    };
    out.fill = look.fill(props, [0; 3]).map(|paint| match paint {
        Some(Paint::Solid(rgb) | Paint::Clear(rgb, _) | Paint::Gradient(rgb, _)) => Some(rgb),
        _ => None,
    });
    for (slot, name) in out.borders.iter_mut().zip(["lnL", "lnT", "lnR", "lnB"]) {
        if let Some(line) = child(props, DRAWING_NS, name) {
            *slot = Some(line_look(line, look));
        }
    }
    out
}

/// The colour and width of the line `line` (`a:ln`, `a:lnL`, ...); `None`
/// when it is not drawn.
fn line_look(line: Node<'_, '_>, look: &Look<'_, '_>) -> Option<(Rgb, f32)> {
    match look.fill(line, [0; 3]) {
        Some(Some(Paint::Solid(rgb) | Paint::Clear(rgb, _) | Paint::Gradient(rgb, _))) => {
            Some((rgb, width_pt(line).unwrap_or(DEFAULT_LINE_PT)))
        }
        _ => None,
    }
}
