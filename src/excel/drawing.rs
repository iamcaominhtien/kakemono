//! The text of the shapes and text boxes of a sheet's drawing part
//! (`xl/drawings/drawingN.xml`), its pictures and its charts: Japanese
//! workbooks often put text, org charts and flows in shapes rather than in
//! cells, covers have logos and photos, and dashboards charts. Each comes
//! with its frame (where it is drawn on the sheet) and a shape with its
//! look (fill, line, text), which the sheet view draws.

use super::grid::Marker;
use crate::chart::{Placed, Rgb, Theme, drawing_color};
use crate::word::docx::parse;
use crate::word::pictures::bounded_size;
use anyhow::Result;
use roxmltree::Node;

/// What a shape, a picture or a chart of a drawing gives.
#[derive(Debug, Clone)]
pub(crate) enum Drawn {
    /// A shape or text box, with or without text.
    Text(TextShape),
    /// A picture.
    Picture(PictureRef),
    /// A chart.
    Chart(ChartRef),
    /// A picture placed in a cell (Excel 365's "Place in cell"): it is
    /// shown after the table of its cell.
    CellPicture(PictureRef),
}

/// A shape or a text box: its text and how it looks.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TextShape {
    /// Its paragraphs joined by `\n`; empty for a shape without text.
    pub(crate) text: String,
    /// Its fill; `None` for none.
    pub(crate) fill: Option<Rgb>,
    /// The colour of its outline; `None` for none.
    pub(crate) line: Option<Rgb>,
    /// The colour of its text; `None` for the default (black).
    pub(crate) color: Option<Rgb>,
    /// The size of its text in points, from its first run that gives one.
    pub(crate) size_pt: Option<f32>,
    /// Its first run is bold.
    pub(crate) bold: bool,
    /// The alignment of its first paragraph: `l`, `ctr`, `r` (`a:pPr algn`).
    pub(crate) align: Option<String>,
    /// Where its text sits: `t`, `ctr`, `b` (`a:bodyPr anchor`).
    pub(crate) anchor: Option<String>,
    /// Its preset outline (`a:prstGeom prst`: `rect`, `ellipse`,
    /// `downArrow`, `straightConnector1`, ...); `None` for a drawn one.
    pub(crate) geometry: Option<String>,
    /// The width of its outline in points; `None` for the default.
    pub(crate) line_width_pt: Option<f32>,
    /// Its outline is dashed or dotted.
    pub(crate) dashed: bool,
    /// Its line starts with an arrow head (`a:headEnd`).
    pub(crate) head: bool,
    /// Its line ends with an arrow head (`a:tailEnd`).
    pub(crate) tail: bool,
    /// It is mirrored left to right (`flipH`).
    pub(crate) flip_h: bool,
    /// It is mirrored top to bottom (`flipV`).
    pub(crate) flip_v: bool,
    /// It is turned clockwise by this many degrees (`rot`).
    pub(crate) rotation_deg: f32,
}

/// The unit of DrawingML angles (`rot`): 60 000ths of a degree.
const ANGLE_UNIT: f32 = 60_000.0;

/// A chart of a drawing (a `graphicFrame` whose graphic is a chart).
#[derive(Debug, Clone)]
pub(crate) struct ChartRef {
    /// The relationship (in the drawing part) of its chart part: the
    /// `r:id` of its `c:chart` (or `cx:chart`).
    pub(crate) rel: String,
    /// The cells its anchor spans, from its top left corner to its bottom
    /// right one (a `twoCellAnchor`).
    pub(crate) cells: Option<(Marker, Marker)>,
    /// Its size in points, from its anchor's `ext` (a `oneCellAnchor` or
    /// an `absoluteAnchor`).
    pub(crate) size_pt: Option<(f32, f32)>,
    /// The chart read and drawn, with the index of its picture in
    /// [`Document::pictures`](crate::model::Document::pictures) when it was
    /// drawn; `None` until then, or when its part cannot be read.
    pub(crate) placed: Option<Box<(Placed, Option<usize>)>>,
}

/// A picture of a drawing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PictureRef {
    /// The relationship (in the drawing part) of its file: the `r:embed`
    /// of its `a:blip`.
    pub(crate) rel: String,
    /// Its alternative text: its description, else its name.
    pub(crate) alt: String,
    /// Its size in points, from its shape properties or its anchor.
    pub(crate) size_pt: Option<(f32, f32)>,
    /// Its index in [`Document::pictures`](crate::model::Document::pictures)
    /// once its file is read; `None` until then, or when it cannot be shown.
    pub(crate) index: Option<usize>,
}

/// A shape, picture or chart of a drawing: the cell its anchor starts at
/// (row, column, both 0-based), where it is drawn, and what it gives.
#[derive(Debug, Clone)]
pub(crate) struct Shape {
    /// The cell its anchor starts at; [`UNANCHORED`] for an absolute one.
    pub(crate) at: (u32, u32),
    /// Where it is drawn.
    pub(crate) frame: Frame,
    /// What it gives.
    pub(crate) drawn: Drawn,
}

/// Where a shape is drawn on its sheet: the box of its anchor, and the
/// part of that box it takes when it is in a group.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Frame {
    /// The top left corner of its anchor (a `twoCellAnchor` or a
    /// `oneCellAnchor`).
    pub(crate) from: Option<Marker>,
    /// The bottom right corner of a `twoCellAnchor`.
    pub(crate) to: Option<Marker>,
    /// The position of an `absoluteAnchor`, in points from the sheet's top
    /// left corner.
    pub(crate) pos_pt: Option<(f32, f32)>,
    /// The size its anchor gives (`ext`), in points.
    pub(crate) ext_pt: Option<(f32, f32)>,
    /// For a shape in a group: the part of the anchor's box it takes, as
    /// shares of its width and height: left, top, width, height.
    pub(crate) part: Option<[f32; 4]>,
}

/// The anchor of an `absoluteAnchor`, which names no cell: it sorts after
/// every anchored shape, and absolute ones stay in document order.
const UNANCHORED: (u32, u32) = (u32::MAX, u32::MAX);
/// English Metric Units in a point: the unit of DrawingML sizes.
const EMU_PER_PT: f32 = 12_700.0;
/// The unit of DrawingML font sizes (`sz`): hundredths of a point.
const FONT_SIZE_UNIT: f32 = 100.0;

/// The text of each shape (`sp`), each picture (`pic`) and each chart (a
/// `graphicFrame` of a chart) of a drawing, also inside a group, with the
/// cell its anchor starts at and its frame; sorted by anchor (row, then
/// column), those with the same anchor in document order. A connector
/// is a shape without text; a picture linked rather than embedded and
/// other graphics give nothing. The shapes of a group share the group's anchor. An
/// `mc:AlternateContent` is read through its `Choice` only (its `Fallback`
/// repeats it). A shape's text is its paragraphs (`a:p`) joined by `\n`;
/// its runs are joined, a `a:br` is a `\n`; a shape with only blanks is
/// left out unless it is filled or outlined. Theme colours are resolved
/// with `theme`.
///
/// # Errors
/// When the XML is damaged.
pub(crate) fn items(xml: &str, theme: &Theme) -> Result<Vec<Shape>> {
    let doc = parse(xml)?;
    let mut shapes = Vec::new();
    anchors(doc.root_element(), theme, &mut shapes);
    shapes.sort_by_key(|shape| shape.at);
    Ok(shapes)
}

/// Reads the anchors among the children of `node`.
fn anchors(node: Node<'_, '_>, theme: &Theme, shapes: &mut Vec<Shape>) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" => {
                let at = from(child).unwrap_or(UNANCHORED);
                let size = named(child, "ext").next().and_then(extent);
                let cells = markers(child);
                let frame = Frame {
                    from: cells.map(|(from, _)| from).or_else(|| marker(child, "from")),
                    to: cells.map(|(_, to)| to),
                    pos_pt: named(child, "pos").next().and_then(position),
                    ext_pt: size,
                    part: None,
                };
                let start = shapes.len();
                let reader = Collector { at, frame, theme };
                reader.collect(child, None, shapes);
                // A picture without a size of its own has the anchor's;
                // a chart always has.
                for shape in &mut shapes[start..] {
                    match &mut shape.drawn {
                        Drawn::Picture(picture) => picture.size_pt = picture.size_pt.or(size),
                        Drawn::Chart(chart) => {
                            chart.size_pt = size;
                            chart.cells = cells;
                        }
                        Drawn::Text(_) | Drawn::CellPicture(_) => {}
                    }
                }
            }
            "AlternateContent" => alternative(child, |inner| anchors(inner, theme, shapes)),
            _ => {}
        }
    }
}

/// How the coordinates of a group's children map to shares of the box of
/// the group's anchor, on each axis: share = coordinate × scale + offset.
#[derive(Debug, Clone, Copy)]
struct Map {
    /// The scale of x and y.
    scale: [f32; 2],
    /// The offset of x and y.
    offset: [f32; 2],
}

impl Map {
    /// The share of the anchor's box of the point (`x`, `y`).
    fn apply(self, x: f32, y: f32) -> [f32; 2] {
        [x * self.scale[0] + self.offset[0], y * self.scale[1] + self.offset[1]]
    }
}

/// What the shapes of one anchor share.
struct Collector<'t> {
    /// The cell the anchor starts at.
    at: (u32, u32),
    /// The anchor's frame.
    frame: Frame,
    /// The theme colours.
    theme: &'t Theme,
}

impl Collector<'_> {
    /// Reads the shapes among the children of `node` (an anchor or a
    /// group); `map` places the children of a group in the anchor's box.
    fn collect(&self, node: Node<'_, '_>, map: Option<Map>, shapes: &mut Vec<Shape>) {
        for child in node.children().filter(Node::is_element) {
            let push = |drawn, props: Option<Node<'_, '_>>, shapes: &mut Vec<Shape>| {
                let part = map
                    .zip(props.and_then(|props| named(props, "xfrm").next()).and_then(xfrm))
                    .map(|(map, ([x, y], [width, height]))| {
                        let [left, top] = map.apply(x, y);
                        let [right, bottom] = map.apply(x + width, y + height);
                        [left, top, right - left, bottom - top]
                    });
                shapes.push(Shape {
                    at: self.at,
                    frame: Frame { part, ..self.frame },
                    drawn,
                });
            };
            match child.tag_name().name() {
                "sp" | "cxnSp" => {
                    if let Some(shape) = text_shape(child, self.theme) {
                        push(Drawn::Text(shape), named(child, "spPr").next(), shapes);
                    }
                }
                "pic" => {
                    if let Some(picture) = picture(child) {
                        push(Drawn::Picture(picture), named(child, "spPr").next(), shapes);
                    }
                }
                "graphicFrame" => {
                    if let Some(chart) = chart(child) {
                        push(Drawn::Chart(chart), Some(child), shapes);
                    }
                }
                "grpSp" => {
                    let inner = named(child, "grpSpPr")
                        .next()
                        .and_then(|props| named(props, "xfrm").next())
                        .and_then(|xfrm| group_map(xfrm, map));
                    self.collect(child, inner, shapes);
                }
                "AlternateContent" => alternative(child, |inner| self.collect(inner, map, shapes)),
                _ => {}
            }
        }
    }
}

/// The map of the children of a group whose `a:xfrm` is `xfrm`, inside
/// the group mapped by `outer` (`None` for a group right under its
/// anchor); `None` when its child extent is empty or missing.
fn group_map(xfrm: Node<'_, '_>, outer: Option<Map>) -> Option<Map> {
    let pair = |name: &str, x: &str, y: &str| -> Option<[f32; 2]> {
        let node = named(xfrm, name).next()?;
        let value = |attr: &str| {
            node.attribute(attr)?
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|value| value.is_finite())
        };
        Some([value(x)?, value(y)?])
    };
    let child_off = pair("chOff", "x", "y")?;
    let child_ext = pair("chExt", "cx", "cy")?;
    if child_ext[0] <= 0.0 || child_ext[1] <= 0.0 {
        return None;
    }
    match outer {
        // Right under the anchor, the group fills the anchor's box.
        None => Some(Map {
            scale: [1.0 / child_ext[0], 1.0 / child_ext[1]],
            offset: [-child_off[0] / child_ext[0], -child_off[1] / child_ext[1]],
        }),
        Some(outer) => {
            let off = pair("off", "x", "y")?;
            let ext = pair("ext", "cx", "cy")?;
            // A child point p is at off + (p - chOff) × ext / chExt in the
            // outer group's coordinates.
            let ratio = [ext[0] / child_ext[0], ext[1] / child_ext[1]];
            Some(Map {
                scale: [outer.scale[0] * ratio[0], outer.scale[1] * ratio[1]],
                offset: [
                    outer.scale[0] * (off[0] - child_off[0] * ratio[0]) + outer.offset[0],
                    outer.scale[1] * (off[1] - child_off[1] * ratio[1]) + outer.offset[1],
                ],
            })
        }
    }
}

/// The offset and extent (in EMU) of an `a:xfrm`.
fn xfrm(node: Node<'_, '_>) -> Option<([f32; 2], [f32; 2])> {
    let pair = |name: &str, x: &str, y: &str| -> Option<[f32; 2]> {
        let node = named(node, name).next()?;
        let value = |attr: &str| {
            node.attribute(attr)?
                .trim()
                .parse::<f32>()
                .ok()
                .filter(|value| value.is_finite())
        };
        Some([value(x)?, value(y)?])
    };
    Some((pair("off", "x", "y")?, pair("ext", "cx", "cy")?))
}

/// Calls `read` on the `Choice` of an `mc:AlternateContent`, or on its
/// `Fallback` when it has no `Choice`.
fn alternative<'a, 'i>(node: Node<'a, 'i>, read: impl FnOnce(Node<'a, 'i>)) {
    let pick = |name: &str| named(node, name).next();
    if let Some(chosen) = pick("Choice").or_else(|| pick("Fallback")) {
        read(chosen);
    }
}

/// The child elements of `node` named `name`, whatever their namespace.
fn named<'a, 'i, 'n>(node: Node<'a, 'i>, name: &'n str) -> impl Iterator<Item = Node<'a, 'i>> + 'n
where
    'a: 'n,
    'i: 'n,
{
    node.children()
        .filter(move |child| child.is_element() && child.tag_name().name() == name)
}

/// The (row, column) of the `from` marker of an anchor.
fn from(anchor: Node<'_, '_>) -> Option<(u32, u32)> {
    let marker = named(anchor, "from").next()?;
    let number = |name: &str| named(marker, name).next()?.text()?.trim().parse::<u32>().ok();
    Some((number("row")?, number("col")?))
}

/// The marker `name` (`from`, `to`) of an anchor, with its offsets.
fn marker(anchor: Node<'_, '_>, name: &str) -> Option<Marker> {
    let marker = named(anchor, name).next()?;
    let number = |name: &str| named(marker, name).next()?.text()?.trim().parse::<i64>().ok();
    let index = |name: &str| u32::try_from(number(name)?).ok();
    let offset = |name: &str| {
        let points = number(name).unwrap_or(0) as f32 / EMU_PER_PT;
        if points.is_finite() { points } else { 0.0 }
    };
    Some(Marker {
        row: index("row")?,
        row_off_pt: offset("rowOff"),
        col: index("col")?,
        col_off_pt: offset("colOff"),
    })
}

/// The two corners of a `twoCellAnchor`: its `from` and `to` markers;
/// `None` for another anchor, or one with a damaged marker.
fn markers(anchor: Node<'_, '_>) -> Option<(Marker, Marker)> {
    (anchor.tag_name().name() == "twoCellAnchor")
        .then(|| Some((marker(anchor, "from")?, marker(anchor, "to")?)))
        .flatten()
}

/// The position in points of an `absoluteAnchor`'s `pos`.
fn position(pos: Node<'_, '_>) -> Option<(f32, f32)> {
    let emu = |name: &str| {
        pos.attribute(name)?
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
    };
    Some((emu("x")? / EMU_PER_PT, emu("y")? / EMU_PER_PT))
}

/// The chart of a `graphicFrame`: the relationship of its chart part;
/// `None` for another graphic (a diagram, an OLE object).
fn chart(frame: Node<'_, '_>) -> Option<ChartRef> {
    let graphic = named(frame, "graphic").next()?;
    let data = named(graphic, "graphicData").next()?;
    let chart = named(data, "chart").next()?;
    let rel = chart
        .attributes()
        .find(|attr| attr.name() == "id")
        .map(|attr| attr.value().trim().to_string())
        .filter(|rel| !rel.is_empty())?;
    Some(ChartRef {
        rel,
        cells: None,
        size_pt: None,
        placed: None,
    })
}

/// A picture (`pic`) embedded in the package; `None` for one linked to a
/// file outside it, or without a file.
fn picture(pic: Node<'_, '_>) -> Option<PictureRef> {
    let blip = named(named(pic, "blipFill").next()?, "blip").next()?;
    let rel = blip
        .attributes()
        .find(|attr| attr.name() == "embed")
        .map(|attr| attr.value().to_string())
        .filter(|rel| !rel.is_empty())?;
    let props = named(pic, "nvPicPr").next().and_then(|nv| named(nv, "cNvPr").next());
    let attr = |name: &str| {
        props
            .and_then(|props| props.attribute(name))
            .map(str::trim)
            .filter(|text| !text.is_empty())
    };
    let alt = attr("descr").or_else(|| attr("name")).unwrap_or_default().to_string();
    let size_pt = named(pic, "spPr")
        .next()
        .and_then(|props| named(props, "xfrm").next())
        .and_then(|xfrm| named(xfrm, "ext").next())
        .and_then(extent);
    Some(PictureRef {
        rel,
        alt,
        size_pt,
        index: None,
    })
}

/// The size in points of an `ext` element (`cx`, `cy` in EMU), bounded
/// ([`bounded_size`]); `None` when it is missing, empty or not finite.
fn extent(ext: Node<'_, '_>) -> Option<(f32, f32)> {
    let emu = |name: &str| ext.attribute(name)?.trim().parse::<f32>().ok();
    bounded_size((emu("cx")? / EMU_PER_PT, emu("cy")? / EMU_PER_PT))
}

/// A shape (`sp`) or connector (`cxnSp`): its text and look; `None` when
/// it has only blanks and is neither filled nor outlined.
fn text_shape(shape: Node<'_, '_>, theme: &Theme) -> Option<TextShape> {
    let body = named(shape, "txBody").next();
    let paragraphs: Vec<String> = body
        .into_iter()
        .flat_map(|body| named(body, "p"))
        .map(paragraph)
        .collect();
    let mut text = paragraphs.join("\n");
    if text.trim().is_empty() {
        text.clear();
    }
    let props = named(shape, "spPr").next();
    let style = named(shape, "style").next();
    let fill = paint(props, style, false, theme);
    let line = paint(props, style, true, theme);
    if text.is_empty() && fill.is_none() && line.is_none() {
        return None;
    }
    let mut read = TextShape {
        text,
        fill,
        line,
        ..TextShape::default()
    };
    if let Some(body) = body {
        text_look(&mut read, body, style, theme);
    }
    if let Some(props) = props {
        outline(&mut read, props);
    }
    Some(read)
}

/// The fill (or with `line`, the outline colour) of a shape: its own (in
/// its properties `props`), else its style's reference (whose index 0
/// means none); `None` for none.
fn paint(props: Option<Node<'_, '_>>, style: Option<Node<'_, '_>>, line: bool, theme: &Theme) -> Option<Rgb> {
    let holder = if line {
        props.and_then(|props| named(props, "ln").next())
    } else {
        props
    };
    // Some(None): the shape says it has none; None: it says nothing.
    let own = holder.and_then(|holder| {
        if named(holder, "noFill").next().is_some() {
            Some(None)
        } else {
            named(holder, "solidFill").next().map(Some)
        }
    });
    match own {
        Some(Some(node)) => drawing_color(node, theme, [0; 3]),
        Some(None) => None,
        None => {
            let reference = if line { "lnRef" } else { "fillRef" };
            let node = style.and_then(|style| named(style, reference).next())?;
            (node.attribute("idx") != Some("0")).then(|| drawing_color(node, theme, [0; 3]))?
        }
    }
}

/// Reads into `shape` the look of the text of its `txBody` `body`: the
/// size, colour and weight of its first run that gives them (the colour
/// else of its style's font reference), its alignment and where it sits.
fn text_look(shape: &mut TextShape, body: Node<'_, '_>, style: Option<Node<'_, '_>>, theme: &Theme) {
    let runs: Vec<Node<'_, '_>> = body
        .descendants()
        .filter(|node| node.is_element() && matches!(node.tag_name().name(), "rPr" | "defRPr"))
        .collect();
    shape.size_pt = runs.iter().find_map(|props| {
        let size = props.attribute("sz")?.trim().parse::<f32>().ok()? / FONT_SIZE_UNIT;
        (size.is_finite() && size > 0.0).then_some(size)
    });
    shape.color = runs
        .iter()
        .find_map(|props| named(*props, "solidFill").next())
        .and_then(|fill| drawing_color(fill, theme, [0; 3]))
        .or_else(|| {
            let font = style.and_then(|style| named(style, "fontRef").next())?;
            drawing_color(font, theme, [0; 3])
        });
    shape.bold = runs.first().is_some_and(|props| props.attribute("b") == Some("1"));
    shape.align = named(body, "p")
        .find_map(|para| named(para, "pPr").next()?.attribute("algn"))
        .map(str::to_string);
    shape.anchor = named(body, "bodyPr")
        .next()
        .and_then(|props| props.attribute("anchor"))
        .map(str::to_string);
}

/// Reads into `shape` its outline from its properties `props`: its
/// preset geometry, the width, dashes and arrow heads of its line, and
/// how it is mirrored and turned.
fn outline(shape: &mut TextShape, props: Node<'_, '_>) {
    shape.geometry = named(props, "prstGeom")
        .next()
        .and_then(|geom| geom.attribute("prst"))
        .map(str::to_string);
    let line = named(props, "ln").next();
    shape.line_width_pt = line
        .and_then(|line| line.attribute("w"))
        .and_then(|width| width.trim().parse::<f32>().ok())
        .map(|width| width / EMU_PER_PT)
        .filter(|width| width.is_finite() && *width >= 0.0);
    shape.dashed = line
        .and_then(|line| named(line, "prstDash").next())
        .and_then(|dash| dash.attribute("val"))
        .is_some_and(|dash| dash != "solid");
    let arrow = |name: &str| {
        line.and_then(|line| named(line, name).next())
            .and_then(|end| end.attribute("type"))
            .is_some_and(|kind| kind != "none")
    };
    shape.head = arrow("headEnd");
    shape.tail = arrow("tailEnd");
    let xfrm = named(props, "xfrm").next();
    let flag = |name: &str| {
        xfrm.and_then(|xfrm| xfrm.attribute(name))
            .is_some_and(|value| value == "1" || value == "true")
    };
    shape.flip_h = flag("flipH");
    shape.flip_v = flag("flipV");
    shape.rotation_deg = xfrm
        .and_then(|xfrm| xfrm.attribute("rot"))
        .and_then(|rot| rot.trim().parse::<f32>().ok())
        .map(|rot| (rot / ANGLE_UNIT).rem_euclid(360.0))
        .filter(|rot| rot.is_finite())
        .unwrap_or(0.0);
}

/// The text of an `a:p`: runs and fields joined, a `a:br` is a `\n`.
fn paragraph(para: Node<'_, '_>) -> String {
    let mut text = String::new();
    for node in para.children().filter(Node::is_element) {
        match node.tag_name().name() {
            "r" | "fld" => {
                for part in named(node, "t") {
                    text.push_str(part.text().unwrap_or_default());
                }
            }
            "br" => text.push('\n'),
            _ => {}
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::pictures::MAX_PICTURE_SIDE_PT;

    const HEAD: &str = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#;

    fn sp(text: &str) -> String {
        format!(r#"<xdr:sp><xdr:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></xdr:txBody></xdr:sp>"#)
    }

    fn two_cell(row: u32, col: u32, inner: &str) -> String {
        format!(
            r#"<xdr:twoCellAnchor><xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>9</xdr:col><xdr:row>9</xdr:row></xdr:to>{inner}<xdr:clientData/></xdr:twoCellAnchor>"#
        )
    }

    /// The shapes of `xml`, with the Office theme.
    fn items(xml: &str) -> Result<Vec<Shape>> {
        super::items(xml, &Theme::default())
    }

    /// The anchors and texts of `xml`, pictures left out.
    fn texts(xml: &str) -> Result<Vec<((u32, u32), String)>> {
        Ok(items(xml)?
            .into_iter()
            .filter_map(|shape| match shape.drawn {
                Drawn::Text(text) => Some((shape.at, text.text)),
                Drawn::Picture(_) | Drawn::Chart(_) | Drawn::CellPicture(_) => None,
            })
            .collect())
    }

    #[test]
    fn shape_text_is_read_in_anchor_order() {
        let xml = format!(
            r#"{HEAD}{}{}<xdr:oneCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:row>2</xdr:row></xdr:from>{}</xdr:oneCellAnchor><xdr:absoluteAnchor><xdr:pos x="0" y="0"/>{}</xdr:absoluteAnchor>{}</xdr:wsDr>"#,
            two_cell(5, 0, &sp("late")),
            two_cell(
                2,
                3,
                r#"<xdr:sp><xdr:txBody><a:p><a:r><a:t>Two</a:t></a:r><a:br/><a:r><a:t>lines</a:t></a:r></a:p><a:p><a:fld id="x" type="t"><a:t>2024</a:t></a:fld></a:p></xdr:txBody></xdr:sp>"#
            ),
            sp("one-cell"),
            sp("absolute"),
            two_cell(0, 0, &sp("   ")),
        );
        assert_eq!(
            texts(&xml).unwrap(),
            vec![
                ((2, 1), "one-cell".to_string()),
                ((2, 3), "Two\nlines\n2024".to_string()),
                ((5, 0), "late".to_string()),
                ((u32::MAX, u32::MAX), "absolute".to_string()),
            ]
        );
    }

    #[test]
    fn grouped_shapes_are_read() {
        let group = format!(
            r#"<xdr:grpSp><xdr:nvGrpSpPr/><xdr:grpSpPr/>{}<xdr:grpSp>{}</xdr:grpSp>{}</xdr:grpSp>"#,
            sp("a"),
            sp("b"),
            sp("c")
        );
        let xml = format!("{HEAD}{}</xdr:wsDr>", two_cell(4, 1, &group));
        let shapes = texts(&xml).unwrap();
        let read: Vec<_> = shapes.iter().map(|(anchor, text)| (*anchor, text.as_str())).collect();
        assert_eq!(read, [((4, 1), "a"), ((4, 1), "b"), ((4, 1), "c")]);
    }

    #[test]
    fn alternate_content_is_read_once() {
        let alt = format!(
            r#"<mc:AlternateContent><mc:Choice Requires="a14">{}</mc:Choice><mc:Fallback>{}</mc:Fallback></mc:AlternateContent>"#,
            sp("box"),
            sp("box")
        );
        let xml = format!("{HEAD}{}</xdr:wsDr>", two_cell(0, 0, &alt));
        assert_eq!(texts(&xml).unwrap().len(), 1);
    }

    #[test]
    fn connectors_charts_and_pictures_without_a_file_give_nothing() {
        let inner = r#"<xdr:pic><xdr:nvPicPr/><xdr:blipFill/></xdr:pic><xdr:pic><xdr:blipFill><a:blip r:link="rId9"/></xdr:blipFill></xdr:pic><xdr:cxnSp><xdr:nvCxnSpPr/><xdr:spPr/></xdr:cxnSp><xdr:graphicFrame><a:graphic><a:t>chart</a:t></a:graphic></xdr:graphicFrame>"#;
        let xml = format!(
            r#"{}</xdr:wsDr>"#,
            HEAD.replace(
                '>',
                r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#
            ) + &two_cell(0, 0, inner)
        );
        assert!(items(&xml).unwrap().is_empty());
    }

    #[test]
    fn pictures_are_read_with_their_alt_text_and_size() {
        let head = HEAD.replace(
            '>',
            r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        );
        let pic = |props: &str, xfrm: &str| {
            format!(
                r#"<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" {props}/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill><xdr:spPr>{xfrm}</xdr:spPr></xdr:pic>"#
            )
        };
        let xml = format!(
            r#"{head}{}<xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>1</xdr:row></xdr:from><xdr:ext cx="254000" cy="127000"/>{}</xdr:oneCellAnchor>{}</xdr:wsDr>"#,
            two_cell(
                3,
                0,
                &pic(
                    r#"name="Picture 1" descr="Logo""#,
                    r#"<a:xfrm><a:ext cx="1270000" cy="635000"/></a:xfrm>"#
                )
            ),
            pic(r#"name="Picture 2""#, ""),
            two_cell(5, 0, &pic("", "")),
        );
        let read: Vec<_> = items(&xml)
            .unwrap()
            .into_iter()
            .map(|shape| match shape.drawn {
                Drawn::Picture(picture) => (shape.at, picture.rel, picture.alt, picture.size_pt),
                Drawn::Text(text) => panic!("text {}", text.text),
                Drawn::Chart(_) | Drawn::CellPicture(_) => panic!("not a picture of the drawing"),
            })
            .collect();
        assert_eq!(
            read,
            [
                ((1, 0), "rId1".into(), "Picture 2".into(), Some((20.0, 10.0))),
                ((3, 0), "rId1".into(), "Logo".into(), Some((100.0, 50.0))),
                ((5, 0), "rId1".into(), String::new(), None),
            ]
        );
    }

    #[test]
    fn infinite_or_absurd_picture_sizes_are_bounded() {
        let head = HEAD.replace(
            '>',
            r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        );
        let size = |cx: &str, cy: &str| {
            let pic = format!(
                r#"<xdr:pic><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill><xdr:spPr><a:xfrm><a:ext cx="{cx}" cy="{cy}"/></a:xfrm></xdr:spPr></xdr:pic>"#
            );
            match &items(&format!("{head}{}</xdr:wsDr>", two_cell(0, 0, &pic))).unwrap()[0].drawn {
                Drawn::Picture(picture) => picture.size_pt,
                Drawn::Text(_) | Drawn::Chart(_) | Drawn::CellPicture(_) => None,
            }
        };
        assert_eq!(size("inf", "12700"), None);
        assert_eq!(size("NaN", "12700"), None);
        assert_eq!(
            size("1e30", "5e29"),
            Some((MAX_PICTURE_SIDE_PT, MAX_PICTURE_SIDE_PT / 2.0))
        );
    }

    #[test]
    fn charts_are_read_with_their_anchor_also_in_groups() {
        let head = HEAD.replace(
            '>',
            r#" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">"#,
        );
        let frame = |rel: &str| {
            format!(
                r#"<xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/></xdr:nvGraphicFramePr><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart r:id="{rel}"/></a:graphicData></a:graphic></xdr:graphicFrame>"#
            )
        };
        let two_cell = r#"<xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>12700</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>25400</xdr:colOff><xdr:row>10</xdr:row><xdr:rowOff>127000</xdr:rowOff></xdr:to>"#;
        let xml = format!(
            r#"{head}{two_cell}<xdr:grpSp>{}{}</xdr:grpSp><xdr:clientData/></xdr:twoCellAnchor><xdr:oneCellAnchor><xdr:from><xdr:col>0</xdr:col><xdr:row>20</xdr:row></xdr:from><xdr:ext cx="2540000" cy="1270000"/>{}</xdr:oneCellAnchor></xdr:wsDr>"#,
            frame("rId1"),
            r#"<xdr:pic><xdr:blipFill><a:blip r:embed="rId3"/></xdr:blipFill></xdr:pic>"#,
            frame("rId2"),
        );
        let read = items(&xml).unwrap();
        assert_eq!(read.len(), 3);
        let Drawn::Chart(first) = &read[0].drawn else {
            panic!("a chart")
        };
        assert_eq!(read[0].at, (2, 1));
        assert_eq!(first.rel, "rId1");
        let (from, to) = first.cells.unwrap();
        assert_eq!((from.row, from.col, from.col_off_pt), (2, 1, 1.0));
        assert_eq!((to.row, to.col, to.col_off_pt, to.row_off_pt), (10, 4, 2.0, 10.0));
        // The picture of the group is found too.
        assert!(matches!(&read[1].drawn, Drawn::Picture(picture) if picture.rel == "rId3"));
        let Drawn::Chart(second) = &read[2].drawn else {
            panic!("a chart")
        };
        assert_eq!(
            (second.rel.as_str(), second.size_pt, second.cells),
            ("rId2", Some((200.0, 100.0)), None)
        );
    }

    #[test]
    fn shapes_in_a_group_take_their_part_of_the_anchor() {
        let child = |x: u32, y: u32, cx: u32, cy: u32| {
            format!(
                r#"<xdr:sp><xdr:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></xdr:spPr></xdr:sp>"#
            )
        };
        // The outer group maps 0..200 × 0..100 onto the anchor; the inner
        // one, at (100, 50) sized 100 × 50 in it, maps its own 0..10.
        let group = format!(
            r#"<xdr:grpSp><xdr:grpSpPr><a:xfrm><a:off x="5000" y="5000"/><a:ext cx="1" cy="1"/><a:chOff x="0" y="0"/><a:chExt cx="200" cy="100"/></a:xfrm></xdr:grpSpPr>{}<xdr:grpSp><xdr:grpSpPr><a:xfrm><a:off x="100" y="50"/><a:ext cx="100" cy="50"/><a:chOff x="0" y="0"/><a:chExt cx="10" cy="10"/></a:xfrm></xdr:grpSpPr>{}</xdr:grpSp></xdr:grpSp>"#,
            child(100, 0, 100, 50),
            child(0, 0, 5, 10),
        );
        let shapes = items(&format!("{HEAD}{}</xdr:wsDr>", two_cell(1, 1, &group))).unwrap();
        let parts: Vec<_> = shapes.iter().map(|shape| shape.frame.part).collect();
        assert_eq!(parts, [Some([0.5, 0.0, 0.5, 0.5]), Some([0.5, 0.5, 0.25, 0.5])]);
        let frame = shapes[0].frame;
        assert_eq!((frame.from.unwrap().row, frame.to.unwrap().col), (1, 9));
        assert!(
            matches!(&shapes[0].drawn, Drawn::Text(shape) if shape.fill == Some([0, 255, 0]) && shape.text.is_empty())
        );
    }

    #[test]
    fn connectors_are_lines_with_their_ends_flips_and_turn() {
        let connector = r#"<xdr:cxnSp><xdr:spPr><a:xfrm rot="5400000" flipH="1"><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></a:xfrm><a:prstGeom prst="bentConnector3"/><a:ln w="25400"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill><a:prstDash val="dash"/><a:tailEnd type="triangle"/></a:ln></xdr:spPr></xdr:cxnSp>"#;
        let shapes = items(&format!("{HEAD}{}</xdr:wsDr>", two_cell(0, 0, connector))).unwrap();
        let Drawn::Text(line) = &shapes[0].drawn else {
            panic!("a shape")
        };
        assert_eq!(line.geometry.as_deref(), Some("bentConnector3"));
        assert_eq!(
            (line.line, line.line_width_pt, line.fill),
            (Some([255, 0, 0]), Some(2.0), None)
        );
        assert!(line.dashed && line.tail && !line.head && line.flip_h && !line.flip_v);
        assert_eq!(line.rotation_deg, 90.0);
    }

    #[test]
    fn a_shape_without_colours_of_its_own_takes_its_style_s() {
        let shape = r#"<xdr:sp><xdr:spPr><a:prstGeom prst="downArrow"/></xdr:spPr><xdr:style><a:lnRef idx="2"><a:schemeClr val="accent1"><a:shade val="50000"/></a:schemeClr></a:lnRef><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef><a:fontRef idx="minor"><a:schemeClr val="lt1"/></a:fontRef></xdr:style><xdr:txBody><a:bodyPr anchor="ctr"/><a:p><a:pPr algn="ctr"/><a:r><a:rPr sz="1400" b="1"/><a:t>Go</a:t></a:r></a:p></xdr:txBody></xdr:sp>"#;
        let shapes = items(&format!("{HEAD}{}</xdr:wsDr>", two_cell(0, 0, shape))).unwrap();
        let Drawn::Text(shape) = &shapes[0].drawn else {
            panic!("a shape")
        };
        assert_eq!(shape.fill, Some([0x44, 0x72, 0xC4]));
        assert_eq!(shape.line, Some([0x22, 0x39, 0x62]));
        assert_eq!(shape.color, Some([255, 255, 255]));
        assert_eq!((shape.size_pt, shape.bold), (Some(14.0), true));
        assert_eq!(
            (shape.align.as_deref(), shape.anchor.as_deref()),
            (Some("ctr"), Some("ctr"))
        );
        // A fill of none wins over the style's.
        let bare = shape_xml_without_fill();
        let shapes = items(&format!("{HEAD}{}</xdr:wsDr>", two_cell(0, 0, &bare))).unwrap();
        assert!(matches!(&shapes[0].drawn, Drawn::Text(shape) if shape.fill.is_none() && shape.text == "Note"));
    }

    /// A shape whose properties say it has no fill, with a style that
    /// gives one.
    fn shape_xml_without_fill() -> String {
        r#"<xdr:sp><xdr:spPr><a:noFill/></xdr:spPr><xdr:style><a:fillRef idx="1"><a:schemeClr val="accent1"/></a:fillRef></xdr:style><xdr:txBody><a:p><a:r><a:t>Note</a:t></a:r></a:p></xdr:txBody></xdr:sp>"#.to_string()
    }

    #[test]
    fn damaged_xml_is_an_error() {
        assert!(texts("<xdr:wsDr").is_err());
    }
}
