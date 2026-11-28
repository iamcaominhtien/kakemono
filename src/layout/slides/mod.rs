//! A PowerPoint deck drawn as pages, as PowerPoint shows its slides: each
//! visible slide one page of the slide's size, painted in order with
//! - its background (`p:bg`), its own else its layout's else its master's;
//! - the master's shapes, then the layout's, placeholders excepted (they
//!   are templates), unless the slide or layout hides them
//!   (`showMasterSp="0"`);
//! - its own shapes in the order of the shape tree: outlines and fills
//!   drawn by `excel::view::shape_svg`, pictures (cropped), charts as their
//!   SVG, tables (`table`), SmartArt as the drawing PowerPoint saved of it,
//!   groups placed through their child space, and text (`text`) laid out
//!   with the fonts and line breaking of the Word layout.
//!
//! It is a second reading of the file, with every look, as the sheet view
//! is of a workbook; the document (`powerpoint::read`) is the first.

mod look;
mod table;
mod text;

use super::display::{Item, Page};
use super::fonts::FontBook;
use super::para::Ctx;
use super::pdf;
use super::shape::Shaper;
use crate::chart::Theme;
use crate::excel::view::shape_svg;
use crate::powerpoint::inherit::{Inheritance, placeholder};
use crate::powerpoint::media::Media;
use crate::powerpoint::package::{EMU_PER_PT, P_NS, Presentation, SlideRef};
use crate::powerpoint::text::A14_NS;
use crate::powerpoint::{Parts, shape_tree};
use crate::word::docx::{Rel, alternate, chart_rel, parse, read_rels, svg_blip};
use crate::word::fmt::model::{FDoc, FImage};
use crate::word::ir::Inline as IrInline;
use crate::word::xml::{DRAWING_NS, MC_NS, REL_NS, attr, child};
use crate::word::zip::ZipArchive;
use anyhow::Result;
use look::{Fonts, Look, Paint};
use roxmltree::{Document as Xml, Node};
use std::collections::HashMap;
use text::{Body, Context};

/// The deepest group of shapes drawn (a guard against a hostile file).
const MAX_GROUP_DEPTH: usize = 64;
/// PowerPoint's distance between default tab stops (`defTabSz`), in points.
const DEFAULT_TAB_PT: f32 = 72.0;
/// The `uri` of the graphic data of a table.
const TABLE_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/table";
/// Namespace of the drawing PowerPoint saves of a SmartArt diagram.
const DIAGRAM_DRAWING_NS: &str = "http://schemas.microsoft.com/office/drawing/2008/diagram";
/// Namespace of the SmartArt data model.
const DIAGRAM_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
/// A share in DrawingML's units (thousandths of a percent): 100 000 is all.
const SHARE_UNITS: f32 = 100_000.0;

/// The PDF of the deck `data`: one page per visible slide; one empty page
/// of the slide size when none is visible.
///
/// # Errors
/// When the archive is damaged or has no presentation part, or the PDF
/// cannot be written.
pub(crate) fn to_pdf(data: &[u8]) -> Result<Vec<u8>> {
    let drawn = draw(data)?;
    pdf::write(drawn.pages, &[], &drawn.fonts, &drawn.images, drawn.title.as_deref())
}

/// The slides of a deck drawn, with what the PDF needs to write them.
struct Drawn {
    /// One page per visible slide.
    pages: Vec<Page>,
    /// The fonts the text was laid out with.
    fonts: FontBook,
    /// The pictures the pages draw.
    images: Vec<FImage>,
    /// The deck's title.
    title: Option<String>,
}

/// The pages of the deck `data`, as [`to_pdf`] writes them.
///
/// # Errors
/// When the archive is damaged or has no presentation part.
fn draw(data: &[u8]) -> Result<Drawn> {
    let zip = ZipArchive::new(data)?;
    let presentation = Presentation::read(&zip)?;
    let mut parts = Parts::new(&zip);
    let presentation_xml = parts.text(&presentation.part).unwrap_or_default();
    let presentation_doc = parse(&presentation_xml).ok();
    let defaults = presentation_doc
        .as_ref()
        .and_then(|doc| child(doc.root_element(), P_NS, "defaultTextStyle"));
    let theme = parts
        .related(&presentation.part, "theme")
        .and_then(|part| zip.read(&part).ok().flatten());
    let mut painter = Painter {
        media: Media::new(&zip, theme.as_deref()),
        fonts: FontBook::new(&[], &HashMap::new()),
        shaper: Shaper::default(),
        coverage: HashMap::new(),
        doc: FDoc {
            default_tab: DEFAULT_TAB_PT,
            ..FDoc::default()
        },
        notes: HashMap::new(),
        images: Vec::new(),
        media_images: HashMap::new(),
        table_styles: parts
            .related(&presentation.part, "tableStyles")
            .and_then(|part| parts.text(&part)),
        outlines: HashMap::new(),
    };
    let (width, height) = presentation.size_pt;
    let mut pages = Vec::new();
    for slide in presentation.visible() {
        let mut page = Page {
            width,
            height,
            items: Vec::new(),
        };
        painter.slide(&mut parts, &zip, slide, defaults, (width, height), &mut page.items);
        page.compact();
        pages.push(page);
    }
    if pages.is_empty() {
        pages.push(Page {
            width,
            height,
            items: Vec::new(),
        });
    }
    Ok(Drawn {
        pages,
        title: crate::word::docx::document_title(&zip)?,
        fonts: painter.fonts,
        images: painter.images,
    })
}

/// What the slides are drawn with.
struct Painter<'z> {
    /// The deck's pictures and charts.
    media: Media<'z>,
    /// The fonts.
    fonts: FontBook,
    /// The shaper.
    shaper: Shaper,
    /// Faces that cover each character, cached.
    coverage: HashMap<(usize, char), bool>,
    /// The document the text layout reads its settings from.
    doc: FDoc,
    /// No notes.
    notes: HashMap<(bool, String), String>,
    /// The pictures drawn: those of the deck, the outlines of shapes.
    images: Vec<FImage>,
    /// The index in `images` of each picture of `media`.
    media_images: HashMap<usize, usize>,
    /// The deck's table styles (`ppt/tableStyles.xml`).
    table_styles: Option<String>,
    /// The index in `images` of each shape outline drawn, by its SVG.
    outlines: HashMap<String, usize>,
}

/// Where a group's shapes are placed: a point `(x, y)` of the group's
/// child space is at `(left + (x - child_x) * scale_x, ...)` on the page.
#[derive(Debug, Clone, Copy)]
struct Map {
    /// The page's left of the child space's origin.
    left: f32,
    /// The page's top of it.
    top: f32,
    /// The child space's origin, x.
    child_x: f32,
    /// The child space's origin, y.
    child_y: f32,
    /// Horizontal scale.
    scale_x: f32,
    /// Vertical scale.
    scale_y: f32,
}

impl Map {
    /// The slide's own space.
    const SLIDE: Self = Self {
        left: 0.0,
        top: 0.0,
        child_x: 0.0,
        child_y: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
    };

    /// A frame of the child space, on the page.
    fn rect(&self, [x, y, width, height]: [f32; 4]) -> [f32; 4] {
        [
            self.left + (x - self.child_x) * self.scale_x,
            self.top + (y - self.child_y) * self.scale_y,
            width * self.scale_x,
            height * self.scale_y,
        ]
    }

    /// The space of the group whose `a:xfrm` is `xfrm`, inside this one. A
    /// group whose child extent is empty is not scaled.
    fn group(&self, xfrm: Node<'_, '_>) -> Self {
        let pair = |name: &str, first: &str, second: &str| {
            let node = child(xfrm, DRAWING_NS, name);
            let value = |attribute: &str| {
                node.and_then(|node| node.attribute(attribute))
                    .and_then(|value| value.trim().parse::<f32>().ok())
                    .filter(|value| value.is_finite())
                    .map_or(0.0, |emu| emu / EMU_PER_PT)
            };
            (value(first), value(second))
        };
        let (x, y) = pair("off", "x", "y");
        let (width, height) = pair("ext", "cx", "cy");
        let (child_x, child_y) = pair("chOff", "x", "y");
        let (child_width, child_height) = pair("chExt", "cx", "cy");
        let [left, top, outer_width, outer_height] = self.rect([x, y, width, height]);
        let scale = |outer: f32, inner: f32, own: f32| if inner > 0.0 { outer / inner } else { own };
        Self {
            left,
            top,
            child_x,
            child_y,
            scale_x: scale(outer_width, child_width, self.scale_x),
            scale_y: scale(outer_height, child_height, self.scale_y),
        }
    }
}

/// One shape tree being drawn: a slide's, its layout's, its master's, or
/// a SmartArt drawing's.
struct Tree<'s, 'a, 'i> {
    /// The slide's theme.
    look: &'s Look<'a, 'i>,
    /// Where its placeholders inherit from.
    inheritance: &'s Inheritance<'a, 'i>,
    /// The relationships of the part the tree is in.
    rels: &'s HashMap<String, Rel>,
    /// The slide's number.
    number: usize,
    /// It is the slide's own tree: its placeholders are drawn (a layout's
    /// or master's are templates).
    own: bool,
}

impl Painter<'_> {
    /// The context of the text layout.
    fn ctx(&mut self) -> Ctx<'_> {
        Ctx {
            book: &mut self.fonts,
            shaper: &mut self.shaper,
            doc: &self.doc,
            notes: &self.notes,
            own_note: None,
            page: None,
            page_count: None,
            section_pages: None,
            coverage: &mut self.coverage,
            page_refs: None,
            origin: 0.0,
        }
    }

    /// The index in `images` of the picture `index` of `media`.
    fn media_image(&mut self, index: usize) -> Option<usize> {
        if let Some(at) = self.media_images.get(&index) {
            return Some(*at);
        }
        let picture = self.media.pictures.get(index)?;
        self.images.push(FImage {
            data: picture.data.clone(),
            format: picture.format,
            bare: None,
            texts: Vec::new(),
        });
        let at = self.images.len() - 1;
        self.media_images.insert(index, at);
        Some(at)
    }

    /// The index in `images` of the SVG picture `svg`: a master's shapes
    /// come back on every slide, so each is stored once.
    fn outline_image(&mut self, svg: String) -> usize {
        if let Some(index) = self.outlines.get(&svg) {
            return *index;
        }
        self.images.push(FImage {
            data: svg.clone().into_bytes(),
            format: crate::model::PictureFormat::Svg,
            bare: None,
            texts: Vec::new(),
        });
        self.outlines.insert(svg, self.images.len() - 1);
        self.images.len() - 1
    }

    /// Draws the slide `slide` into `items`, its text styles defaulting to
    /// the presentation's `defaults`.
    fn slide(
        &mut self,
        parts: &mut Parts<'_>,
        zip: &ZipArchive<'_>,
        slide: &SlideRef,
        defaults: Option<Node<'_, '_>>,
        size: (f32, f32),
        items: &mut Vec<Item>,
    ) {
        let layout_part = parts.related(&slide.part, "slideLayout");
        let master_part = layout_part
            .as_deref()
            .and_then(|part| parts.related(part, "slideMaster"));
        let theme_part = master_part.as_deref().and_then(|part| parts.related(part, "theme"));
        let texts = [
            parts.text(&slide.part),
            layout_part.as_deref().and_then(|part| parts.text(part)),
            master_part.as_deref().and_then(|part| parts.text(part)),
            theme_part.as_deref().and_then(|part| parts.text(part)),
        ];
        // A damaged layout, master or theme is read as none.
        let [slide_doc, layout_doc, master_doc, theme_doc] = texts
            .each_ref()
            .map(|text| text.as_deref().and_then(|text| parse(text).ok()));
        let Some(slide_doc) = slide_doc.as_ref() else {
            return;
        };
        let rels_of = |part: Option<&str>| -> HashMap<String, Rel> {
            part.and_then(|part| read_rels(zip, part).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|(id, _, rel)| (id, rel))
                .collect()
        };
        let slide_rels = rels_of(Some(&slide.part));
        let layout_rels = rels_of(layout_part.as_deref());
        let master_rels = rels_of(master_part.as_deref());
        let (slide_root, layout_root, master_root) = (
            slide_doc.root_element(),
            layout_doc.as_ref().map(Xml::root_element),
            master_doc.as_ref().map(Xml::root_element),
        );
        let look = slide_look(
            texts[3].as_deref(),
            theme_doc.as_ref().map(Xml::root_element),
            [master_root, layout_root, Some(slide_root)],
        );
        let inheritance = Inheritance {
            layout: layout_doc.as_ref().and_then(shape_tree),
            master: master_doc.as_ref().and_then(shape_tree),
            styles: master_root.and_then(|root| child(root, P_NS, "txStyles")),
            defaults,
        };
        // The background: the first of the slide, layout and master that
        // has one.
        let background = [
            (Some(slide_root), &slide_rels),
            (layout_root, &layout_rels),
            (master_root, &master_rels),
        ]
        .into_iter()
        .find_map(|(root, rels)| {
            let bg = child(child(root?, P_NS, "cSld")?, P_NS, "bg")?;
            Some((bg, rels))
        });
        if let Some((bg, rels)) = background {
            self.background(bg, &look, rels, size, items);
        }
        let shows = |root: Option<Node<'_, '_>>| {
            root.and_then(|root| root.attribute("showMasterSp"))
                .is_none_or(|value| !matches!(value, "0" | "false"))
        };
        let tree = |rels, own| Tree {
            look: &look,
            inheritance: &inheritance,
            rels,
            number: slide.number,
            own,
        };
        if shows(Some(slide_root)) {
            if shows(layout_root)
                && let Some(master) = inheritance.master
            {
                self.shapes(&tree(&master_rels, false), master, Map::SLIDE, 0, items);
            }
            if let Some(layout) = inheritance.layout {
                self.shapes(&tree(&layout_rels, false), layout, Map::SLIDE, 0, items);
            }
        }
        if let Some(own) = shape_tree(slide_doc) {
            self.shapes(&tree(&slide_rels, true), own, Map::SLIDE, 0, items);
        }
    }

    /// Draws the background `bg` (`p:bg`) of a slide `size` large.
    fn background(
        &mut self,
        bg: Node<'_, '_>,
        look: &Look<'_, '_>,
        rels: &HashMap<String, Rel>,
        size: (f32, f32),
        items: &mut Vec<Item>,
    ) {
        let paint = if let Some(props) = child(bg, P_NS, "bgPr") {
            look.fill(props, [0; 3]).flatten()
        } else {
            child(bg, P_NS, "bgRef").and_then(|reference| look.fill_ref(reference))
        };
        let (width, height) = size;
        match paint {
            Some(Paint::Clear(color, opacity)) => items.push(Item::Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
                // Over the white of the page.
                color: color.map(|channel| (f32::from(channel) * opacity + 255.0 * (1.0 - opacity)).round() as u8),
            }),
            Some(Paint::Gradient(_, gradient)) => {
                let svg = format!(
                    "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><defs>{}</defs><rect width=\"{width}\" height=\"{height}\" fill=\"url(#g)\"/></svg>",
                    gradient.svg("g")
                );
                let index = self.outline_image(svg);
                items.push(Item::Image {
                    index,
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                    crop: [0.0; 4],
                });
            }
            Some(Paint::Solid(color)) => items.push(Item::Rect {
                x: 0.0,
                y: 0.0,
                width,
                height,
                color,
            }),
            Some(Paint::Picture(id)) => {
                if let Some(index) = self.media.picture(rels, &id).and_then(|index| self.media_image(index)) {
                    items.push(Item::Image {
                        index,
                        x: 0.0,
                        y: 0.0,
                        width,
                        height,
                        crop: [0.0; 4],
                    });
                }
            }
            None => {}
        }
    }

    /// Draws the shapes of the tree or group `parent`, placed by `map`.
    fn shapes(&mut self, tree: &Tree<'_, '_, '_>, parent: Node<'_, '_>, map: Map, depth: usize, items: &mut Vec<Item>) {
        if depth > MAX_GROUP_DEPTH {
            return;
        }
        for node in parent.children().filter(Node::is_element) {
            self.node(tree, node, map, depth, items);
        }
    }

    /// Draws the shape, group or alternative `node`.
    fn node(&mut self, tree: &Tree<'_, '_, '_>, node: Node<'_, '_>, map: Map, depth: usize, items: &mut Vec<Item>) {
        let name = node.tag_name().name();
        if node.tag_name().namespace() == Some(MC_NS) {
            if name == "AlternateContent" {
                // A formula's shape is drawn as the picture of its fallback.
                let choice = child(node, MC_NS, "Choice");
                let branch = match (choice, child(node, MC_NS, "Fallback")) {
                    (Some(choice), Some(fallback))
                        if choice.descendants().any(|item| {
                            item.tag_name().namespace() == Some(A14_NS) && item.tag_name().name() == "m"
                        }) =>
                    {
                        Some(fallback)
                    }
                    _ => alternate(node),
                };
                if let Some(branch) = branch {
                    self.shapes(tree, branch, map, depth, items);
                }
            }
            return;
        }
        if is_hidden(node) {
            return;
        }
        match name {
            "sp" | "cxnSp" => self.shape(tree, node, map, items),
            "grpSp" => {
                let group = child(node, P_NS, "grpSpPr")
                    .or_else(|| node.children().find(|item| item.tag_name().name() == "grpSpPr"))
                    .and_then(|props| child(props, DRAWING_NS, "xfrm"))
                    .map_or(map, |xfrm| map.group(xfrm));
                self.shapes(tree, node, group, depth + 1, items);
            }
            "pic" => self.picture(tree, node, map, items),
            "graphicFrame" => self.frame(tree, node, map, depth, items),
            _ => {}
        }
    }

    /// The frame of `node` on the page: its own, else (a placeholder of
    /// the slide) its layout's or master's.
    fn rect_of(&self, tree: &Tree<'_, '_, '_>, node: Node<'_, '_>, map: Map) -> Option<[f32; 4]> {
        let own = own_xfrm(node).and_then(frame_of_xfrm);
        let inherited = || {
            let ph = placeholder(node)?;
            let layout = tree.inheritance.layout_match(ph);
            layout.and_then(own_xfrm).and_then(frame_of_xfrm).or_else(|| {
                let master = layout.and_then(placeholder).map_or_else(
                    || tree.inheritance.master_match(ph),
                    |own| tree.inheritance.master_match(own),
                );
                master.and_then(own_xfrm).and_then(frame_of_xfrm)
            })
        };
        own.or_else(|| if tree.own { inherited() } else { None })
            .map(|frame| map.rect(frame))
    }

    /// The shapes a placeholder of the slide inherits from, most specific
    /// first: its layout's and its master's.
    fn inherited<'a, 'i>(tree: &Tree<'_, 'a, 'i>, node: Node<'_, '_>) -> Vec<Node<'a, 'i>> {
        let Some(ph) = placeholder(node) else {
            return Vec::new();
        };
        let ph = crate::powerpoint::inherit::Placeholder {
            kind: tree.inheritance.kind_of(ph),
            ..ph
        };
        let layout = tree.inheritance.layout_match(ph);
        let master = layout.and_then(placeholder).map_or_else(
            || tree.inheritance.master_match(ph),
            |own| tree.inheritance.master_match(own),
        );
        layout.into_iter().chain(master).collect()
    }

    /// Draws a shape or connector: its outline and fill, then its text.
    fn shape(&mut self, tree: &Tree<'_, '_, '_>, node: Node<'_, '_>, map: Map, items: &mut Vec<Item>) {
        // A layout's or master's placeholder is a template.
        if !tree.own && placeholder(node).is_some() {
            return;
        }
        let Some(rect) = self.rect_of(tree, node, map) else {
            return;
        };
        let inherited = if tree.own {
            Self::inherited(tree, node)
        } else {
            Vec::new()
        };
        let style = node
            .children()
            .find(|item| item.is_element() && item.tag_name().name() == "style");
        let turn_deg = self.outline(tree, node, style, &inherited, rect, items);
        let Some(body) = node
            .children()
            .find(|item| item.is_element() && item.tag_name().name() == "txBody")
        else {
            return;
        };
        let look = tree.look;
        let font_ref = style.and_then(|style| child(style, DRAWING_NS, "fontRef"));
        let body_props: Vec<Node<'_, '_>> = std::iter::once(body)
            .chain(
                inherited
                    .iter()
                    .filter_map(|shape| shape.children().find(|item| item.tag_name().name() == "txBody")),
            )
            .filter_map(|body| child(body, DRAWING_NS, "bodyPr"))
            .collect();
        let text_body = Body {
            body,
            props: body_props,
            styles: tree.inheritance.list_styles(node),
            font_color: font_ref.and_then(|reference| look.color(reference, [0; 3])),
            font_major: font_ref
                .and_then(|reference| reference.attribute("idx"))
                .map(|idx| idx == "major"),
            font_bold: None,
            turn_deg,
            insets_pt: None,
        };
        let context = Context {
            look,
            rels: tree.rels,
            number: tree.number,
        };
        let (mut laid, _) = text::draw(&mut self.ctx(), &context, &text_body, rect);
        items.append(&mut laid);
    }

    /// Draws the fill and outline of the shape `node` in `rect`, with its
    /// style `style` and the placeholders it inherits from `inherited`;
    /// gives its turn, in degrees.
    fn outline(
        &mut self,
        tree: &Tree<'_, '_, '_>,
        node: Node<'_, '_>,
        style: Option<Node<'_, '_>>,
        inherited: &[Node<'_, '_>],
        rect: [f32; 4],
        items: &mut Vec<Item>,
    ) -> f32 {
        let props = properties(node);
        let look = tree.look;
        // The fill: the shape's own, else its placeholder's, else its
        // style's.
        let fill = props
            .into_iter()
            .chain(inherited.iter().filter_map(|shape| properties(*shape)))
            .find_map(|props| look.fill(props, [0; 3]))
            .unwrap_or_else(|| {
                style
                    .and_then(|style| child(style, DRAWING_NS, "fillRef"))
                    .and_then(|reference| look.fill_ref(reference))
            });
        let [x, y, width, height] = rect;
        if let Some(Paint::Picture(id)) = &fill {
            let crop = props
                .and_then(|props| child(props, DRAWING_NS, "blipFill"))
                .map_or([0.0; 4], crop_of);
            self.push_picture(tree.rels, id, rect, crop, items);
        }
        let has_geometry = |props: &Node<'_, '_>| {
            child(*props, DRAWING_NS, "prstGeom").is_some() || child(*props, DRAWING_NS, "custGeom").is_some()
        };
        // The geometry: its own, else its placeholder's.
        let geometry = props
            .filter(has_geometry)
            .or_else(|| {
                inherited
                    .iter()
                    .filter_map(|shape| properties(*shape))
                    .find(has_geometry)
            })
            .or(props);
        let outline = look.outline(props, geometry, style, fill.as_ref().and_then(Paint::rgb));
        if (outline.fill.is_some() || outline.line.is_some())
            && let Some((svg, [dx, dy, svg_width, svg_height])) = shape_svg(&outline, width, height)
        {
            let svg = match &fill {
                Some(Paint::Clear(_, opacity)) => see_through(&svg, *opacity),
                Some(Paint::Gradient(_, gradient)) => with_gradient(&svg, gradient),
                _ => svg,
            };
            let index = self.outline_image(svg);
            items.push(Item::Image {
                index,
                x: x + dx,
                y: y + dy,
                width: svg_width,
                height: svg_height,
                crop: [0.0; 4],
            });
        }
        outline.rotation_deg
    }

    /// Draws the picture of the relationship `id` of `rels` in `rect`,
    /// cropped by `crop`.
    fn push_picture(
        &mut self,
        rels: &HashMap<String, Rel>,
        id: &str,
        rect: [f32; 4],
        crop: [f32; 4],
        items: &mut Vec<Item>,
    ) {
        let [x, y, width, height] = rect;
        if let Some(index) = self.media.picture(rels, id).and_then(|index| self.media_image(index)) {
            items.push(Item::Image {
                index,
                x,
                y,
                width,
                height,
                crop,
            });
        }
    }

    /// Draws a picture (`p:pic`), cropped as it says (`a:srcRect`).
    fn picture(&mut self, tree: &Tree<'_, '_, '_>, node: Node<'_, '_>, map: Map, items: &mut Vec<Item>) {
        if !tree.own && placeholder(node).is_some() {
            return;
        }
        let Some([x, y, width, height]) = self.rect_of(tree, node, map) else {
            return;
        };
        let Some(fill) = node
            .children()
            .find(|item| item.is_element() && item.tag_name().name() == "blipFill")
        else {
            return;
        };
        let Some(blip) = child(fill, DRAWING_NS, "blip") else {
            return;
        };
        let id = svg_blip(blip).or_else(|| attr(blip, REL_NS, "embed"));
        if let Some(id) = id {
            self.push_picture(tree.rels, id, [x, y, width, height], crop_of(fill), items);
        }
    }

    /// Draws a graphic frame: a table, a chart, a SmartArt diagram, or an
    /// embedded object's picture.
    fn frame(&mut self, tree: &Tree<'_, '_, '_>, node: Node<'_, '_>, map: Map, depth: usize, items: &mut Vec<Item>) {
        let Some(rect) = self.rect_of(tree, node, map) else {
            return;
        };
        let Some(data) =
            child(node, DRAWING_NS, "graphic").and_then(|graphic| child(graphic, DRAWING_NS, "graphicData"))
        else {
            return;
        };
        let [x, y, width, height] = rect;
        if data.attribute("uri") == Some(TABLE_URI) {
            if let Some(table) = child(data, DRAWING_NS, "tbl") {
                let context = Context {
                    look: tree.look,
                    rels: tree.rels,
                    number: tree.number,
                };
                let styles = tree
                    .inheritance
                    .styles
                    .and_then(|styles| child(styles, P_NS, "otherStyle"));
                let lists: Vec<Node<'_, '_>> = styles.into_iter().chain(tree.inheritance.defaults).collect();
                let table_styles = self.table_styles.clone();
                table::draw(
                    &mut self.ctx(),
                    &context,
                    table,
                    &lists,
                    table_styles.as_deref(),
                    rect,
                    items,
                );
            }
            return;
        }
        if let Some(id) = chart_rel(data) {
            if let Some(IrInline::Chart(chart)) = self.media.chart(tree.rels, id, (width, height))
                && let Some(index) = chart.1.and_then(|index| self.media_image(index))
            {
                items.push(Item::Image {
                    index,
                    x,
                    y,
                    width,
                    height,
                    crop: [0.0; 4],
                });
            }
            return;
        }
        if let Some(ids) = child(data, DIAGRAM_NS, "relIds") {
            self.diagram(tree, ids, rect, depth, items);
            return;
        }
        // An embedded object: the picture it shows, in the frame.
        let fill = data
            .descendants()
            .find(|item| item.is_element() && item.tag_name().name() == "pic")
            .and_then(|pic| pic.children().find(|item| item.tag_name().name() == "blipFill"));
        if let Some(fill) = fill {
            let id = child(fill, DRAWING_NS, "blip").and_then(|blip| attr(blip, REL_NS, "embed"));
            if let Some(index) = id
                .and_then(|id| self.media.picture(tree.rels, id))
                .and_then(|index| self.media_image(index))
            {
                items.push(Item::Image {
                    index,
                    x,
                    y,
                    width,
                    height,
                    crop: crop_of(fill),
                });
            }
        }
    }

    /// Draws a SmartArt diagram in the frame `rect` as the drawing
    /// PowerPoint saved of it (its data model names it).
    fn diagram(
        &mut self,
        tree: &Tree<'_, '_, '_>,
        ids: Node<'_, '_>,
        rect: [f32; 4],
        depth: usize,
        items: &mut Vec<Item>,
    ) {
        let Some(model) = attr(ids, REL_NS, "dm")
            .and_then(|id| tree.rels.get(id))
            .filter(|rel| !rel.external)
        else {
            return;
        };
        let Some(model_xml) = self.media.read_text(&model.target) else {
            return;
        };
        let Ok(model_doc) = parse(&model_xml) else {
            return;
        };
        let Some(drawing) = model_doc
            .descendants()
            .find(|node| node.tag_name().name() == "dataModelExt")
            .and_then(|ext| ext.attribute("relId"))
            .and_then(|id| tree.rels.get(id))
            .filter(|rel| !rel.external)
        else {
            return;
        };
        let Some(xml) = self.media.read_text(&drawing.target) else {
            return;
        };
        let Ok(doc) = parse(&xml) else {
            return;
        };
        let Some(shapes) = doc
            .descendants()
            .find(|node| node.has_tag_name((DIAGRAM_DRAWING_NS, "spTree")))
        else {
            return;
        };
        let rels: HashMap<String, Rel> = self.media.rels(&drawing.target);
        let inner = Tree {
            look: tree.look,
            inheritance: &Inheritance::default(),
            rels: &rels,
            number: tree.number,
            own: true,
        };
        let map = Map {
            left: rect[0],
            top: rect[1],
            ..Map::SLIDE
        };
        for shape in shapes.children().filter(Node::is_element) {
            if shape.tag_name().name() == "sp" {
                self.shape(&inner, shape, map, items);
            } else if shape.tag_name().name() == "grpSp" {
                self.node(&inner, shape, map, depth + 1, items);
            }
        }
    }
}

/// The theme of a slide: the theme part `theme_text` (whose root is
/// `theme_root`), its colours mapped by the colour map of the master, then
/// those of the layout and the slide that override it
/// (`p:clrMapOvr/a:overrideClrMapping`), of `roots` (master, layout, slide).
fn slide_look<'a, 'i>(
    theme_text: Option<&str>,
    theme_root: Option<Node<'a, 'i>>,
    roots: [Option<Node<'_, '_>>; 3],
) -> Look<'a, 'i> {
    let [master, layout, slide] = roots;
    let mut theme = Theme::read(theme_text.map(str::as_bytes));
    if let Some(map) = master.and_then(|root| child(root, P_NS, "clrMap")) {
        theme = theme.with_map(map);
    }
    for root in [layout, slide].into_iter().flatten() {
        if let Some(map) = child(root, P_NS, "clrMapOvr").and_then(|over| child(over, DRAWING_NS, "overrideClrMapping"))
        {
            theme = theme.with_map(map);
        }
    }
    Look {
        theme,
        fonts: Fonts::read(theme_root),
        format: theme_root.and_then(|root| {
            root.descendants()
                .find(|node| node.has_tag_name((DRAWING_NS, "fmtScheme")))
        }),
    }
}

/// The SVG of a shape's outline `svg` with its fill seen through at
/// `opacity`.
fn see_through(svg: &str, opacity: f32) -> String {
    svg.replacen(" fill=\"#", &format!(" fill-opacity=\"{opacity:.3}\" fill=\"#"), 1)
}

/// The SVG of a shape's outline `svg` filled with `gradient`.
fn with_gradient(svg: &str, gradient: &look::Gradient) -> String {
    let Some(fill) = svg.find(" fill=\"#") else {
        return svg.to_string();
    };
    let end = svg[fill + 8..].find('"').map_or(svg.len(), |at| fill + 8 + at + 1);
    let mut out = format!("{} fill=\"url(#g)\"{}", &svg[..fill], &svg[end..]);
    if let Some(open) = out.find('>') {
        out.insert_str(open + 1, &format!("<defs>{}</defs>", gradient.svg("g")));
    }
    out
}

/// The shape properties of a shape (`p:spPr`, `dsp:spPr`).
fn properties<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|item| item.is_element() && item.tag_name().name() == "spPr")
}

/// The `a:xfrm` of a shape, picture or graphic frame.
fn own_xfrm<'a, 'i>(node: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    node.children()
        .find(|item| item.is_element() && item.tag_name().name() == "xfrm")
        .or_else(|| {
            node.children()
                .find(|item| item.is_element() && matches!(item.tag_name().name(), "spPr" | "grpSpPr"))
                .and_then(|props| child(props, DRAWING_NS, "xfrm"))
        })
}

/// The frame an `a:xfrm` gives, in points: left, top, width, height;
/// `None` when it says no size.
fn frame_of_xfrm(xfrm: Node<'_, '_>) -> Option<[f32; 4]> {
    let number = |node: Option<Node<'_, '_>>, name: &str| {
        node.and_then(|node| node.attribute(name))
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .map(|emu| emu / EMU_PER_PT)
    };
    let off = child(xfrm, DRAWING_NS, "off");
    let ext = child(xfrm, DRAWING_NS, "ext");
    Some([
        number(off, "x").unwrap_or(0.0),
        number(off, "y").unwrap_or(0.0),
        number(ext, "cx").filter(|width| *width >= 0.0)?,
        number(ext, "cy").filter(|height| *height >= 0.0)?,
    ])
}

/// The share of a picture cut off each side (left, top, right, bottom)
/// by the `a:srcRect` of its fill `fill`.
fn crop_of(fill: Node<'_, '_>) -> [f32; 4] {
    let Some(rect) = child(fill, DRAWING_NS, "srcRect") else {
        return [0.0; 4];
    };
    ["l", "t", "r", "b"].map(|side| {
        rect.attribute(side)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .map_or(0.0, |value| (value / SHARE_UNITS).clamp(-1.0, 1.0))
    })
}

/// Whether a shape is hidden (its `cNvPr hidden="1"`).
fn is_hidden(node: Node<'_, '_>) -> bool {
    node.children()
        .find(|item| item.is_element() && item.tag_name().name().starts_with("nv"))
        .and_then(|nv| nv.children().find(|item| item.tag_name().name() == "cNvPr"))
        .and_then(|props| props.attribute("hidden"))
        .is_some_and(|value| matches!(value, "1" | "true"))
}

#[cfg(test)]
mod tests;
