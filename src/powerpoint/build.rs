//! A slide's shapes as the content of its section: its title, then its
//! other shapes in the order of the shape tree (the order PowerPoint's
//! reading order pane and screen readers follow), groups in their own
//! order: text as paragraphs and list items, tables, pictures, charts,
//! SmartArt as a list. Page furniture (date, footer, slide number) and
//! empty placeholders give nothing: a placeholder's prompt text lives in
//! the layout, which is never read for text.

use super::inherit::{FURNITURE, Inheritance, placeholder};
use super::media::{DIAGRAM_NS, Media};
use super::package::{EMU_PER_PT, P_NS};
use super::text::{TextContext, inlines_text, paragraphs};
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
/// The `uri` of the graphic data of a table.
const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";

/// What a slide gives.
#[derive(Debug, Default)]
pub(crate) struct SlideContent {
    /// The text of its title, when it has one with text.
    pub(crate) title: Option<Vec<Inline>>,
    /// Its title is a centred title (`ctrTitle`): the slide is a title
    /// slide.
    pub(crate) centred_title: bool,
    /// The paragraphs of its subtitle (`subTitle`).
    pub(crate) subtitle: Vec<Para>,
    /// The rest, in reading order.
    pub(crate) body: Vec<Item>,
}

impl SlideContent {
    /// It gives nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.title.is_none() && self.subtitle.is_empty() && self.body.is_empty()
    }
}

/// The content of the slide whose shape tree is `tree`; its pictures and
/// charts are stored in `media`.
pub(crate) fn slide(
    tree: Node<'_, '_>,
    inheritance: &Inheritance<'_, '_>,
    context: &TextContext<'_>,
    media: &mut Media<'_>,
) -> SlideContent {
    let mut reader = Reader {
        inheritance,
        context,
        media,
        content: SlideContent::default(),
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
}

impl<'a, 'i> Reader<'_, 'a, 'i, '_, '_> {
    /// Adds the content of the shapes of the shape tree or group `tree`.
    fn shapes(&mut self, tree: Node<'a, 'i>, depth: usize) {
        if depth > MAX_GROUP_DEPTH {
            return;
        }
        for node in tree.children().filter(Node::is_element) {
            if node.tag_name().namespace() == Some(MC_NS) && node.tag_name().name() == "AlternateContent" {
                self.alternate(node, depth);
                continue;
            }
            if node.tag_name().namespace() != Some(P_NS) || is_hidden(node) {
                continue;
            }
            match node.tag_name().name() {
                "sp" => self.shape(node),
                "grpSp" => self.shapes(node, depth + 1),
                "pic" => self.picture(node),
                "graphicFrame" => self.frame(node),
                _ => {}
            }
        }
    }

    /// Adds the content of an `mc:AlternateContent`: its first choice, or
    /// its fallback when the choice gives nothing.
    fn alternate(&mut self, node: Node<'a, 'i>, depth: usize) {
        let before = self.content.body.len();
        if let Some(choice) = child(node, MC_NS, "Choice") {
            self.shapes(choice, depth);
        }
        if self.content.body.len() == before
            && let Some(fallback) = child(node, MC_NS, "Fallback")
        {
            self.shapes(fallback, depth);
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
        let paras = paragraphs(body, &self.inheritance.list_styles(node), self.context);
        if paras.is_empty() {
            return;
        }
        let content = &mut self.content;
        match kind {
            Some(kind @ ("title" | "ctrTitle")) if content.title.is_none() => {
                content.title = Some(title_text(paras));
                content.centred_title = kind == "ctrTitle";
            }
            Some("subTitle") => content.subtitle.extend(paras),
            _ => content.body.extend(paras.into_iter().map(Item::Para)),
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
            self.content.body.extend(nodes.into_iter().map(|(level, text)| {
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
            self.content.body.push(Item::Table(Table { rows }));
        }
    }

    /// Adds a paragraph holding only `inline` (a picture or a chart).
    fn push_inline(&mut self, inline: Inline) {
        self.content.body.push(Item::Para(Para {
            inlines: vec![inline],
            ..Default::default()
        }));
    }
}

/// The text of a title's paragraphs, on one line: paragraphs and line
/// breaks become spaces.
fn title_text(paras: Vec<Para>) -> Vec<Inline> {
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

/// Whether the attribute `name` of `node` is true (`1` or `true`).
fn flag(node: Node<'_, '_>, name: &str) -> bool {
    node.attribute(name).is_some_and(|value| matches!(value, "1" | "true"))
}
