//! A slide's shapes as the content of its section: its title, then the
//! text of its other shapes in the order of the shape tree (the order
//! PowerPoint's reading order pane and screen readers follow), groups in
//! their own order. Page furniture (date, footer, slide number) and empty
//! placeholders give nothing: a placeholder's prompt text lives in the
//! layout, which is never read for text.

use super::inherit::{FURNITURE, Inheritance, placeholder};
use super::package::P_NS;
use super::text::{TextContext, inlines_text, paragraphs};
use crate::word::ir::{Inline, Item, Para};
use crate::word::xml::{MC_NS, child};
use roxmltree::Node;

/// The deepest group of shapes read: the shapes of deeper groups are left
/// out (a guard against a hostile file).
const MAX_GROUP_DEPTH: usize = 64;

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

/// The content of the slide whose shape tree is `tree`.
pub(crate) fn slide(tree: Node<'_, '_>, inheritance: &Inheritance<'_, '_>, context: &TextContext<'_>) -> SlideContent {
    let mut content = SlideContent::default();
    shapes(tree, inheritance, context, &mut content, 0);
    content
}

/// Adds the content of the shapes of the shape tree or group `tree`.
fn shapes(
    tree: Node<'_, '_>,
    inheritance: &Inheritance<'_, '_>,
    context: &TextContext<'_>,
    content: &mut SlideContent,
    depth: usize,
) {
    if depth > MAX_GROUP_DEPTH {
        return;
    }
    for node in tree.children().filter(Node::is_element) {
        if node.tag_name().namespace() == Some(MC_NS) && node.tag_name().name() == "AlternateContent" {
            alternate(node, inheritance, context, content, depth);
            continue;
        }
        if node.tag_name().namespace() != Some(P_NS) || is_hidden(node) {
            continue;
        }
        match node.tag_name().name() {
            "sp" => shape(node, inheritance, context, content),
            "grpSp" => shapes(node, inheritance, context, content, depth + 1),
            _ => {}
        }
    }
}

/// Adds the content of an `mc:AlternateContent`: its first choice, or its
/// fallback when the choice gives nothing.
fn alternate(
    node: Node<'_, '_>,
    inheritance: &Inheritance<'_, '_>,
    context: &TextContext<'_>,
    content: &mut SlideContent,
    depth: usize,
) {
    let before = content.body.len();
    if let Some(choice) = child(node, MC_NS, "Choice") {
        shapes(choice, inheritance, context, content, depth);
    }
    if content.body.len() == before
        && let Some(fallback) = child(node, MC_NS, "Fallback")
    {
        shapes(fallback, inheritance, context, content, depth);
    }
}

/// Adds the text of the shape `node` (`p:sp`).
fn shape(node: Node<'_, '_>, inheritance: &Inheritance<'_, '_>, context: &TextContext<'_>, content: &mut SlideContent) {
    let Some(body) = child(node, P_NS, "txBody") else {
        return;
    };
    // A placeholder with only an index takes its type from its layout's.
    let kind = placeholder(node).map(|ph| inheritance.kind_of(ph));
    if kind.is_some_and(|kind| FURNITURE.contains(&kind)) {
        return;
    }
    let paras = paragraphs(body, &inheritance.list_styles(node), context);
    if paras.is_empty() {
        return;
    }
    match kind {
        Some(kind @ ("title" | "ctrTitle")) if content.title.is_none() => {
            content.title = Some(title_text(paras));
            content.centred_title = kind == "ctrTitle";
        }
        Some("subTitle") => content.subtitle.extend(paras),
        _ => content.body.extend(paras.into_iter().map(Item::Para)),
    }
}

/// The text of a title's paragraphs, on one line: paragraphs and line
/// breaks become spaces.
fn title_text(paras: Vec<Para>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for para in paras {
        if !out.is_empty() && !inlines_text(&out).ends_with(' ') {
            out.push(Inline::Text(crate::model::Span::plain(" ")));
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

/// Whether a shape is hidden (`p:cNvPr hidden="1"`).
fn is_hidden(node: Node<'_, '_>) -> bool {
    node.children()
        .find(|item| item.is_element() && item.tag_name().name().starts_with("nv"))
        .and_then(|nv| nv.children().find(|item| item.tag_name().name() == "cNvPr"))
        .and_then(|props| props.attribute("hidden"))
        .is_some_and(|value| matches!(value, "1" | "true"))
}
