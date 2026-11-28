//! A slide's shapes as the content of its section: its title, and its
//! other shapes as pieces, one per shape of the shape tree (a group is one
//! piece, its shapes in their own order), each with where it lies on the
//! slide, which `order` reads in order: text as paragraphs and list items,
//! tables, pictures, charts, SmartArt as a list. Page furniture (date,
//! footer, slide number) and empty placeholders give nothing: a
//! placeholder's prompt text lives in the layout, which is never read for
//! text.

use super::inherit::{FURNITURE, Inheritance, placeholder};
use super::media::{DIAGRAM_NS, Media};
use super::order::{Frame, reading_order};
use super::package::{EMU_PER_PT, P_NS};
use super::text::{TextContext, inlines_text, paragraph_sizes_pt, paragraphs};
use crate::model::Span;
use crate::word::docx::svg_blip;
use crate::word::ir::{Cell, Inline, Item, ListMark, Para, Row, Table, VMerge};
use crate::word::xml::{CHART_NS, DRAWING_NS, MC_NS, REL_NS, attr, child, children};
use roxmltree::Node;

/// The deepest group of shapes read: the shapes of deeper groups are left
/// out (a guard against a hostile file).
const MAX_GROUP_DEPTH: usize = 64;
/// Namespace of the Office 2017 decorative mark of a picture (`adec:`).
const DECORATIVE_NS: &str = "http://schemas.microsoft.com/office/drawing/2017/decorative";
/// The extensions of picture files, which a description that is a file
/// name ends with.
const PICTURE_EXTENSIONS: [&str; 10] = ["png", "jpg", "jpeg", "gif", "bmp", "tif", "tiff", "wmf", "emf", "svg"];
/// The `uri` of the graphic data of a table.
const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

/// What a slide gives.
#[derive(Debug, Default)]
pub(crate) struct SlideContent {
    /// The text of its title, when it has one with text.
    pub(crate) title: Option<Vec<Inline>>,
    /// Its title is a centred title (`ctrTitle`), or its layout is a title
    /// slide's: the slide is a title slide.
    pub(crate) title_slide: bool,
    /// The paragraphs of its subtitle (`subTitle`).
    pub(crate) subtitle: Vec<Para>,
    /// The rest, one piece per shape, in the order of the shape tree.
    pub(crate) pieces: Vec<Piece>,
}

impl SlideContent {
    /// It gives nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.title.is_none() && self.subtitle.is_empty() && self.pieces.is_empty()
    }
}

/// What a shape of a slide's shape tree gives.
#[derive(Debug, Default)]
pub(crate) struct Piece {
    /// Its paragraphs, tables, pictures and charts.
    pub(crate) items: Vec<Item>,
    /// Where it lies on the slide, when its shape says.
    pub(crate) frame: Option<Frame>,
    /// For a text box (a shape with text, not a placeholder): what the
    /// rules about titles and repeated text read of it.
    pub(crate) text: Option<TextInfo>,
}

/// What the rules about titles and repeated text read of a text box.
#[derive(Debug, Clone, Default)]
pub(crate) struct TextInfo {
    /// The size of its largest text, in points.
    pub(crate) size_pt: f32,
    /// The size of its first paragraph's largest text, in points, and its
    /// number of words; `None` when the sizes are not known.
    pub(crate) first: Option<(f32, usize)>,
    /// The size of the largest text of its other paragraphs, in points (0
    /// for none).
    pub(crate) rest_size_pt: f32,
    /// Its text, its words separated by single spaces.
    pub(crate) plain: String,
    /// Its number of paragraphs.
    pub(crate) paragraphs: usize,
    /// One of its paragraphs is an item of a list.
    pub(crate) listed: bool,
}

impl TextInfo {
    /// Its number of words.
    pub(crate) fn words(&self) -> usize {
        self.plain.split_whitespace().count()
    }
}

/// The content of the slide whose shape tree is `tree`; its pictures and
/// charts are stored in `media`. The shapes of a group are read in their
/// reading order (`order`), shapes overlapping by `overlap_pt` told apart.
pub(crate) fn slide(
    tree: Node<'_, '_>,
    inheritance: &Inheritance<'_, '_>,
    context: &TextContext<'_>,
    media: &mut Media<'_>,
    overlap_pt: f32,
) -> SlideContent {
    let mut reader = Reader {
        inheritance,
        context,
        media,
        content: SlideContent::default(),
        items: Vec::new(),
        text: None,
        overlap_pt,
    };
    reader.shapes(tree, 0);
    reader.content
}

/// A slide being read.
struct Reader<'s, 'a, 'i, 'm, 'z> {
    /// Its layout, master and default text style.
    inheritance: &'s Inheritance<'a, 'i>,
    /// Its number and relationships.
    context: &'s TextContext<'s>,
    /// The deck's pictures and charts.
    media: &'m mut Media<'z>,
    /// What it gave so far.
    content: SlideContent,
    /// What the shape being read gave so far.
    items: Vec<Item>,
    /// What the rules read of the shape being read, when it is a text box.
    text: Option<TextInfo>,
    /// How much shapes may overlap and still be read by their position
    /// (`PowerPointConfig::reading_overlap_pt`).
    overlap_pt: f32,
}

impl<'a, 'i> Reader<'_, 'a, 'i, '_, '_> {
    /// Adds the content of the shapes of the shape tree or group `tree`.
    fn shapes(&mut self, tree: Node<'a, 'i>, depth: usize) {
        let pieces = self.pieces(tree, depth);
        self.content.pieces = pieces;
    }

    /// The pieces of the shapes of the shape tree or group `tree`, one per
    /// shape that gives something, in tree order.
    fn pieces(&mut self, tree: Node<'a, 'i>, depth: usize) -> Vec<Piece> {
        let mut out = Vec::new();
        if depth > MAX_GROUP_DEPTH {
            return out;
        }
        // The items of the shape that holds this group, set apart.
        let outer = std::mem::take(&mut self.items);
        for node in tree.children().filter(Node::is_element) {
            self.text = None;
            self.node(node, depth);
            if !self.items.is_empty() {
                out.push(Piece {
                    items: std::mem::take(&mut self.items),
                    frame: self.frame_of(node),
                    // A group's shapes are placed in the group's own
                    // space: only the slide's own text boxes are read for
                    // titles and furniture.
                    text: if depth == 0 { self.text.take() } else { None },
                });
            }
        }
        self.text = None;
        self.items = outer;
        out
    }

    /// Adds the content of the shape, group or alternative `node`.
    fn node(&mut self, node: Node<'a, 'i>, depth: usize) {
        if node.tag_name().namespace() == Some(MC_NS) && node.tag_name().name() == "AlternateContent" {
            self.alternate(node, depth);
            return;
        }
        if node.tag_name().namespace() != Some(P_NS) || is_hidden(node) {
            return;
        }
        match node.tag_name().name() {
            "sp" => self.shape(node),
            "grpSp" => {
                // A group's shapes in their reading order, in the group's
                // own space.
                let pieces = self.pieces(node, depth + 1);
                self.items.extend(ordered_items(pieces, self.overlap_pt));
            }
            "pic" => self.picture(node),
            "graphicFrame" => self.frame(node),
            _ => {}
        }
    }

    /// Where the shape `node` lies on the slide: its own position and size
    /// (`a:xfrm`, `p:xfrm`), else, for a placeholder, its layout's or
    /// master's; for an alternative, that of the shape it holds.
    fn frame_of(&self, node: Node<'a, 'i>) -> Option<Frame> {
        if node.tag_name().name() == "AlternateContent" {
            return node
                .children()
                .filter(Node::is_element)
                .flat_map(|branch| branch.children().filter(Node::is_element))
                .find_map(|inner| self.frame_of(inner));
        }
        let own = own_xfrm(node).and_then(frame_of_xfrm);
        own.or_else(|| {
            let ph = placeholder(node)?;
            let layout = self.inheritance.layout_match(ph);
            layout.and_then(own_xfrm).and_then(frame_of_xfrm).or_else(|| {
                let master = layout.and_then(placeholder).map_or_else(
                    || self.inheritance.master_match(ph),
                    |own| self.inheritance.master_match(own),
                );
                master.and_then(own_xfrm).and_then(frame_of_xfrm)
            })
        })
    }

    /// Adds the content of an `mc:AlternateContent`: its first choice, or
    /// its fallback when the choice gives nothing.
    fn alternate(&mut self, node: Node<'a, 'i>, depth: usize) {
        let before = self.items.len();
        if let Some(choice) = child(node, MC_NS, "Choice") {
            self.inner(choice, depth);
        }
        if self.items.len() == before
            && let Some(fallback) = child(node, MC_NS, "Fallback")
        {
            self.inner(fallback, depth);
        }
    }

    /// Adds the content of the shapes inside a branch of an alternative,
    /// as part of the piece being read.
    fn inner(&mut self, branch: Node<'a, 'i>, depth: usize) {
        for node in branch.children().filter(Node::is_element) {
            self.node(node, depth);
        }
    }

    /// Adds the text of the shape `node` (`p:sp`).
    fn shape(&mut self, node: Node<'a, 'i>) {
        let Some(body) = child(node, P_NS, "txBody") else {
            return;
        };
        // A placeholder with only an index takes its type from its layout's.
        let kind = placeholder(node).map(|ph| self.inheritance.kind_of(ph));
        if kind.is_some_and(|kind| FURNITURE.contains(&kind)) {
            return;
        }
        let styles = self.inheritance.list_styles(node);
        let paras = paragraphs(body, &styles, self.context);
        if paras.is_empty() {
            return;
        }
        let content = &mut self.content;
        match kind {
            Some(kind @ ("title" | "ctrTitle")) if content.title.is_none() => {
                content.title = Some(title_text(paras));
                content.title_slide |= kind == "ctrTitle";
            }
            Some("subTitle") => content.subtitle.extend(paras),
            _ => {
                if kind.is_none() {
                    let sizes = paragraph_sizes_pt(body, &styles);
                    // The sizes of the paragraphs with text, when they are
                    // those read (a paragraph of a formula alone has none).
                    let sizes = if sizes.len() == paras.len() { sizes } else { Vec::new() };
                    self.text = Some(TextInfo {
                        size_pt: sizes.iter().copied().fold(0.0, f32::max),
                        first: sizes
                            .first()
                            .map(|size| (*size, inlines_text(&paras[0].inlines).split_whitespace().count())),
                        rest_size_pt: sizes.iter().skip(1).copied().fold(0.0, f32::max),
                        plain: paras
                            .iter()
                            .map(|para| inlines_text(&para.inlines))
                            .collect::<Vec<_>>()
                            .join(" ")
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                        paragraphs: paras.len(),
                        listed: paras.iter().any(|para| para.list.is_some()),
                    });
                }
                self.items.extend(paras.into_iter().map(Item::Para));
            }
        }
    }

    /// Adds the picture `node` (`p:pic`), unless it is marked decorative.
    fn picture(&mut self, node: Node<'a, 'i>) {
        let props = non_visual(node);
        if props.is_some_and(is_decorative) {
            return;
        }
        let Some(blip) = child(node, P_NS, "blipFill").and_then(|fill| child(fill, DRAWING_NS, "blip")) else {
            return;
        };
        let Some(id) = attr(blip, REL_NS, "embed") else {
            return;
        };
        let rels = self.context.rels;
        // The SVG of a picture stored with a PNG beside it.
        let index = svg_blip(blip)
            .and_then(|svg| self.media.picture(rels, svg))
            .or_else(|| self.media.picture(rels, id));
        let (width_pt, height_pt) =
            extent(child(node, P_NS, "spPr").and_then(|props| child(props, DRAWING_NS, "xfrm")));
        let alt = props
            .and_then(|props| props.attribute("descr").or_else(|| props.attribute("title")))
            .filter(|alt| !is_file_name(alt))
            .unwrap_or_default()
            .to_string();
        self.push_inline(Inline::Picture {
            index,
            width_pt,
            height_pt,
            alt,
        });
    }

    /// Adds the content of a graphic frame (`p:graphicFrame`): a table, a
    /// chart, a SmartArt diagram, or the picture of an embedded object.
    fn frame(&mut self, node: Node<'a, 'i>) {
        let Some(data) =
            child(node, DRAWING_NS, "graphic").and_then(|graphic| child(graphic, DRAWING_NS, "graphicData"))
        else {
            return;
        };
        let size = extent(child(node, P_NS, "xfrm"));
        let rels = self.context.rels;
        if let Some(table) = child(data, DRAWING_NS, "tbl") {
            if data.attribute("uri") == Some(TABLE_URI) || data.attribute("uri").is_none() {
                self.table(table);
            }
        } else if let Some(id) = child(data, CHART_NS, "chart").and_then(|chart| attr(chart, REL_NS, "id")) {
            if let Some(chart) = self.media.chart(rels, id, size) {
                self.push_inline(chart);
            }
        } else if let Some(ids) = child(data, DIAGRAM_NS, "relIds") {
            let nodes = attr(ids, REL_NS, "dm")
                .map(|id| self.media.diagram(rels, id))
                .unwrap_or_default();
            self.items.extend(nodes.into_iter().map(|(level, text)| {
                Item::Para(Para {
                    style: "List Paragraph".to_string(),
                    list: Some(ListMark { level, marker: None }),
                    inlines: vec![Inline::Text(Span::plain(text))],
                    ..Default::default()
                })
            }));
        } else {
            // An embedded object (`p:oleObj`) shows its picture.
            for pic in data.descendants().filter(|item| item.tag_name().name() == "pic") {
                if pic.tag_name().namespace() == Some(P_NS) {
                    self.picture(pic);
                    break;
                }
            }
        }
    }

    /// Adds the table `table` (`a:tbl`): merged cells keep their spans,
    /// and the first row is a header when the table says so (`firstRow`).
    fn table(&mut self, table: Node<'a, 'i>) {
        let header = child(table, DRAWING_NS, "tblPr")
            .and_then(|props| props.attribute("firstRow"))
            .is_some_and(|value| matches!(value, "1" | "true"));
        let styles: Vec<Node<'a, 'i>> = self.inheritance.defaults.into_iter().collect();
        let mut rows = Vec::new();
        for (index, row) in children(table, DRAWING_NS, "tr").enumerate() {
            let mut cells = Vec::new();
            for cell in children(row, DRAWING_NS, "tc") {
                // A cell covered by the one to its left is part of it.
                if flag(cell, "hMerge") {
                    continue;
                }
                let merge = if flag(cell, "vMerge") {
                    VMerge::Continue
                } else if cell
                    .attribute("rowSpan")
                    .and_then(|span| span.parse::<usize>().ok())
                    .is_some_and(|span| span > 1)
                {
                    VMerge::Restart
                } else {
                    VMerge::None
                };
                let body = child(cell, DRAWING_NS, "txBody")
                    .map(|body| paragraphs(body, &styles, self.context))
                    .unwrap_or_default();
                cells.push(Cell {
                    span: cell
                        .attribute("gridSpan")
                        .and_then(|span| span.parse().ok())
                        .unwrap_or(1usize)
                        .max(1),
                    merge,
                    body: body.into_iter().map(Item::Para).collect(),
                });
            }
            rows.push(Row {
                cells,
                header: header && index == 0,
            });
        }
        if !rows.is_empty() {
            self.items.push(Item::Table(Table { rows }));
        }
    }

    /// Adds a paragraph holding only `inline` (a picture or a chart).
    fn push_inline(&mut self, inline: Inline) {
        self.items.push(Item::Para(Para {
            inlines: vec![inline],
            ..Default::default()
        }));
    }
}

/// The content of `pieces` in reading order (`order`), shapes overlapping
/// by `overlap_pt` told apart.
pub(crate) fn ordered_items(pieces: Vec<Piece>, overlap_pt: f32) -> Vec<Item> {
    let frames: Vec<_> = pieces.iter().map(|piece| piece.frame).collect();
    let order = reading_order(&frames, overlap_pt);
    let mut pieces: Vec<Option<Piece>> = pieces.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|index| pieces[index].take())
        .flat_map(|piece| piece.items)
        .collect()
}

/// The position and size of a shape (`a:xfrm` of its shape properties, or
/// its own `p:xfrm` for a graphic frame).
fn own_xfrm<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    child(node, P_NS, "xfrm").or_else(|| {
        node.children()
            .find(|item| item.is_element() && matches!(item.tag_name().name(), "spPr" | "grpSpPr"))
            .and_then(|props| child(props, DRAWING_NS, "xfrm"))
    })
}

/// The frame an `a:xfrm` gives, in points; `None` when it says no size.
fn frame_of_xfrm(xfrm: Node<'_, '_>) -> Option<Frame> {
    let number = |node: Option<Node<'_, '_>>, name: &str| {
        node.and_then(|node| node.attribute(name))
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .map(|emu| emu / EMU_PER_PT)
    };
    let off = child(xfrm, DRAWING_NS, "off");
    let ext = child(xfrm, DRAWING_NS, "ext");
    Some(Frame {
        x: number(off, "x").unwrap_or(0.0),
        y: number(off, "y").unwrap_or(0.0),
        width: number(ext, "cx").filter(|width| *width >= 0.0)?,
        height: number(ext, "cy").filter(|height| *height >= 0.0)?,
    })
}

/// The text of a title's paragraphs, on one line: paragraphs and line
/// breaks become spaces.
pub(crate) fn title_text(paras: Vec<Para>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for para in paras {
        if !out.is_empty() && !inlines_text(&out).ends_with(' ') {
            out.push(Inline::Text(Span::plain(" ")));
        }
        for mut inline in para.inlines {
            if let Inline::Text(span) = &mut inline {
                span.text = span.text.replace('\n', " ");
            }
            out.push(inline);
        }
    }
    out
}

/// The size of a shape (`a:xfrm`/`p:xfrm` `ext`), in points; zero when it
/// says none.
fn extent(xfrm: Option<Node<'_, '_>>) -> (f32, f32) {
    let Some(ext) = xfrm.and_then(|xfrm| child(xfrm, DRAWING_NS, "ext")) else {
        return (0.0, 0.0);
    };
    let side = |name: &str| {
        ext.attribute(name)
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|emu| emu.is_finite() && *emu >= 0.0)
            .map_or(0.0, |emu| emu / EMU_PER_PT)
    };
    (side("cx"), side("cy"))
}

/// The non-visual properties of a shape (`p:cNvPr`).
fn non_visual<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|item| item.is_element() && item.tag_name().name().starts_with("nv"))
        .and_then(|nv| nv.children().find(|item| item.tag_name().name() == "cNvPr"))
}

/// Whether a shape is hidden (`p:cNvPr hidden="1"`).
fn is_hidden(node: Node<'_, '_>) -> bool {
    non_visual(node).is_some_and(|props| flag(props, "hidden"))
}

/// Whether a picture is marked decorative (`adec:decorative val="1"`):
/// screen readers skip it, and so does the document.
fn is_decorative(props: Node<'_, '_>) -> bool {
    props.descendants().any(|item| {
        item.tag_name().namespace() == Some(DECORATIVE_NS)
            && item.tag_name().name() == "decorative"
            && flag(item, "val")
    })
}

/// Whether a picture's description is only the name or path of the file
/// it was inserted from (`C:\\Users\\...\\MC900433934[1].PNG`,
/// `photo.jpg`), which older PowerPoints stored: no description for a
/// reader.
fn is_file_name(alt: &str) -> bool {
    let alt = alt.trim();
    let path = alt.contains('\\') && (alt.get(1..3) == Some(":\\") || alt.starts_with("\\\\"));
    let extension = alt
        .rsplit_once('.')
        .is_some_and(|(_, ext)| PICTURE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
    path || (extension && !alt.contains(' '))
}

/// Whether the attribute `name` of `node` is true (`1` or `true`).
fn flag(node: Node<'_, '_>, name: &str) -> bool {
    node.attribute(name).is_some_and(|value| matches!(value, "1" | "true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_no_description() {
        assert!(is_file_name(r"C:\Users\someone\AppData\Local\Temp\MC900433934[1].PNG"));
        assert!(is_file_name("IMG_0042.jpg"));
        assert!(is_file_name(r"\\server\share\logo"));
        assert!(!is_file_name("A reef seen from above"));
        assert!(!is_file_name("Chart of sales in 2024. Source: survey.png data"));
    }
}
