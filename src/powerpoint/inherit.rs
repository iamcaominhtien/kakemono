//! What a slide's shapes take from its layout and master: a placeholder
//! is matched with the layout's placeholder of the same index or type, and
//! that one with the master's of the same type; the properties of each
//! level of text come from the first of these that gives them: the
//! paragraph, the shape's list styles, the layout's and the master's
//! placeholders, the master's text styles, the presentation's default text
//! style.

use super::package::P_NS;
use crate::word::xml::{DRAWING_NS, child};
use roxmltree::Node;

/// The placeholder types of page furniture (date, footer, slide number,
/// header, the slide's picture on a notes page): drawn on the page, left
/// out of the text.
pub(crate) const FURNITURE: [&str; 5] = ["dt", "ftr", "sldNum", "hdr", "sldImg"];
/// The deepest level of text (`a:lvl9pPr`).
const MAX_LEVEL: u8 = 8;

/// A placeholder (`p:ph`): its type and index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Placeholder<'a> {
    /// Its type (`title`, `ctrTitle`, `body`, `obj`, `subTitle`, `dt`, ...);
    /// `obj` when it names none.
    pub(crate) kind: &'a str,
    /// Its index, when it names one.
    pub(crate) idx: Option<&'a str>,
}

impl Placeholder<'_> {
    /// It holds a title (`title`, `ctrTitle`).
    pub(crate) fn is_title(&self) -> bool {
        matches!(self.kind, "title" | "ctrTitle")
    }

    /// It is page furniture (date, footer, slide number, ...).
    pub(crate) fn is_furniture(&self) -> bool {
        FURNITURE.contains(&self.kind)
    }

    /// The text styles of the master its text takes (`p:titleStyle`,
    /// `p:bodyStyle` or `p:otherStyle`).
    fn style_name(&self) -> &'static str {
        if self.is_title() {
            "titleStyle"
        } else if self.is_furniture() {
            "otherStyle"
        } else {
            "bodyStyle"
        }
    }

    /// Its type as a master names it: a master has one placeholder of each
    /// type, `body` standing for content (`obj`) and subtitles, `title` for
    /// centred titles.
    fn master_kind(&self) -> &str {
        match self.kind {
            "ctrTitle" => "title",
            "obj" | "subTitle" | "tbl" | "chart" | "pic" | "clipArt" | "dgm" | "media" => "body",
            other => other,
        }
    }
}

/// The placeholder `shape` is, if any (`p:nvSpPr/p:nvPr/p:ph`, or the same
/// in a picture or a graphic frame).
pub(crate) fn placeholder<'a>(shape: Node<'a, '_>) -> Option<Placeholder<'a>> {
    let ph = shape
        .children()
        .find(|node| node.is_element() && node.tag_name().name().starts_with("nv"))
        .and_then(|nv| child(nv, P_NS, "nvPr"))
        .and_then(|nv| child(nv, P_NS, "ph"))?;
    Some(Placeholder {
        kind: ph.attribute("type").unwrap_or("obj"),
        idx: ph.attribute("idx"),
    })
}

/// The layout and master a slide takes its placeholders and text styles
/// from, and the presentation's default text style.
#[derive(Clone, Copy, Default)]
pub(crate) struct Inheritance<'a, 'input> {
    /// The layout's shape tree.
    pub(crate) layout: Option<Node<'a, 'input>>,
    /// The master's shape tree.
    pub(crate) master: Option<Node<'a, 'input>>,
    /// The master's text styles (`p:txStyles`).
    pub(crate) styles: Option<Node<'a, 'input>>,
    /// The presentation's default text style (`p:defaultTextStyle`).
    pub(crate) defaults: Option<Node<'a, 'input>>,
}

impl<'a, 'input> Inheritance<'a, 'input> {
    /// The type of the placeholder `ph` of a slide: its own, else that of
    /// the layout's placeholder it is matched with.
    pub(crate) fn kind_of(&self, ph: Placeholder<'a>) -> &'a str {
        if ph.kind != "obj" || ph.idx.is_none() {
            return ph.kind;
        }
        self.layout_match(ph)
            .and_then(placeholder)
            .map_or(ph.kind, |layout| layout.kind)
    }

    /// The layout's placeholder `ph` (of a slide) is matched with: the one
    /// of the same index, else the first of the same type.
    pub(crate) fn layout_match(&self, ph: Placeholder<'_>) -> Option<Node<'a, 'input>> {
        let tree = self.layout?;
        let shapes: Vec<(Node<'a, 'input>, Placeholder<'a>)> = tree
            .descendants()
            .filter_map(|shape| placeholder(shape).map(|own| (shape, own)))
            .collect();
        ph.idx
            .and_then(|idx| shapes.iter().find(|(_, own)| own.idx == Some(idx)))
            .or_else(|| shapes.iter().find(|(_, own)| own.master_kind() == ph.master_kind()))
            .map(|(shape, _)| *shape)
    }

    /// The master's placeholder of the type of `ph`.
    pub(crate) fn master_match(&self, ph: Placeholder<'_>) -> Option<Node<'a, 'input>> {
        let kind = ph.master_kind();
        self.master?
            .descendants()
            .find(|shape| placeholder(*shape).is_some_and(|own| own.master_kind() == kind))
    }

    /// The list styles that give the text of `shape` its properties, most
    /// specific first: the shape's own (`a:lstStyle`), then for a
    /// placeholder its layout's, its master's and the master's text styles
    /// for its type, then the presentation's default text style.
    pub(crate) fn list_styles(&self, shape: Node<'a, 'input>) -> Vec<Node<'a, 'input>> {
        let mut styles: Vec<Node<'a, 'input>> = own_list_style(shape).into_iter().collect();
        if let Some(ph) = placeholder(shape) {
            let ph = Placeholder {
                kind: self.kind_of(ph),
                ..ph
            };
            let layout = self.layout_match(ph);
            styles.extend(layout.and_then(own_list_style));
            let master = layout
                .and_then(placeholder)
                .map_or_else(|| self.master_match(ph), |own| self.master_match(own));
            styles.extend(master.and_then(own_list_style));
            styles.extend(self.styles.and_then(|styles| child(styles, P_NS, ph.style_name())));
        }
        styles.extend(self.defaults);
        styles
    }
}

/// The properties of a level of text: the paragraph's own (`a:pPr`), then
/// each list style's `a:lvlNpPr` for its level, most specific first.
pub(crate) fn level_props<'a, 'input>(
    para_props: Option<Node<'a, 'input>>,
    styles: &[Node<'a, 'input>],
    level: u8,
) -> Vec<Node<'a, 'input>> {
    let name = format!("lvl{}pPr", level.min(MAX_LEVEL) + 1);
    para_props
        .into_iter()
        .chain(styles.iter().filter_map(|style| child(*style, DRAWING_NS, &name)))
        .collect()
}

/// The list style of a shape's text body (`p:txBody/a:lstStyle`).
fn own_list_style<'a, 'input>(shape: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    shape
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "txBody")
        .and_then(|body| child(body, DRAWING_NS, "lstStyle"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::docx::parse;

    const NS: &str = "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
                      xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

    /// A shape tree of placeholders (type, idx, a marker in its list style).
    fn tree(shapes: &[(Option<&str>, Option<&str>, &str)]) -> String {
        let mut out = format!("<p:spTree {NS}>");
        for (kind, idx, mark) in shapes {
            let kind = kind.map(|kind| format!(" type=\"{kind}\"")).unwrap_or_default();
            let idx = idx.map(|idx| format!(" idx=\"{idx}\"")).unwrap_or_default();
            out += &format!(
                "<p:sp><p:nvSpPr><p:cNvPr id=\"1\" name=\"{mark}\"/><p:cNvSpPr/><p:nvPr><p:ph{kind}{idx}/></p:nvPr></p:nvSpPr>\
                 <p:txBody><a:bodyPr/><a:lstStyle><a:lvl1pPr marL=\"{mark}\"/></a:lstStyle></p:txBody></p:sp>"
            );
        }
        out + "</p:spTree>"
    }

    /// The `marL` marks of the level-1 properties of the list styles of the
    /// first shape of `slide`, inherited from `layout` and `master`.
    fn marks(slide: &str, layout: &str, master: &str) -> Vec<String> {
        let (slide, layout, master) = (parse(slide).unwrap(), parse(layout).unwrap(), parse(master).unwrap());
        let inheritance = Inheritance {
            layout: Some(layout.root_element()),
            master: Some(master.root_element()),
            ..Default::default()
        };
        let shape = slide.root_element().first_element_child().unwrap();
        let styles = inheritance.list_styles(shape);
        level_props(None, &styles, 0)
            .iter()
            .filter_map(|props| props.attribute("marL").map(str::to_string))
            .collect()
    }

    #[test]
    fn placeholder_by_idx_then_type() {
        let master = tree(&[(Some("title"), None, "mt"), (Some("body"), Some("1"), "mb")]);
        let layout = tree(&[
            (Some("title"), None, "lt"),
            (None, Some("1"), "l1"),
            (None, Some("13"), "l13"),
        ]);
        // By index: the layout's idx 13, which is content: the master's body.
        assert_eq!(
            marks(&tree(&[(None, Some("13"), "s")]), &layout, &master),
            ["s", "l13", "mb"]
        );
        // By type: a title.
        assert_eq!(
            marks(&tree(&[(Some("title"), None, "s")]), &layout, &master),
            ["s", "lt", "mt"]
        );
    }

    #[test]
    fn body_stands_for_obj_and_untyped() {
        let master = tree(&[(Some("body"), Some("1"), "mb")]);
        let layout = tree(&[(Some("subTitle"), Some("1"), "ls")]);
        assert_eq!(
            marks(&tree(&[(None, Some("1"), "s")]), &layout, &master),
            ["s", "ls", "mb"]
        );
        let slide = tree(&[(None, Some("1"), "s")]);
        let doc = parse(&slide).unwrap();
        let layout = parse(&layout).unwrap();
        let inheritance = Inheritance {
            layout: Some(layout.root_element()),
            ..Default::default()
        };
        let ph = placeholder(doc.root_element().first_element_child().unwrap()).unwrap();
        assert_eq!(inheritance.kind_of(ph), "subTitle");
    }

    #[test]
    fn orphan_placeholder_keeps_its_own_text() {
        // No layout placeholder of its index or type, and no master.
        let layout = tree(&[(Some("title"), None, "lt")]);
        let slide = tree(&[(Some("body"), Some("7"), "s")]);
        let (slide, layout) = (parse(&slide).unwrap(), parse(&layout).unwrap());
        let inheritance = Inheritance {
            layout: Some(layout.root_element()),
            ..Default::default()
        };
        let shape = slide.root_element().first_element_child().unwrap();
        let styles = inheritance.list_styles(shape);
        assert_eq!(styles.len(), 1);
    }

    #[test]
    fn levels_merge_from_paragraph_to_default_style() {
        let master = format!(
            "<p:sldMaster {NS}><p:txStyles><p:bodyStyle><a:lvl2pPr marL=\"m2\"/></p:bodyStyle></p:txStyles></p:sldMaster>"
        );
        let defaults = format!("<p:defaultTextStyle {NS}><a:lvl2pPr marL=\"d2\"/></p:defaultTextStyle>");
        let slide = tree(&[(Some("body"), Some("1"), "s")]);
        let (master, defaults, slide) = (
            parse(&master).unwrap(),
            parse(&defaults).unwrap(),
            parse(&slide).unwrap(),
        );
        let inheritance = Inheritance {
            styles: child(master.root_element(), P_NS, "txStyles"),
            defaults: Some(defaults.root_element()),
            ..Default::default()
        };
        let shape = slide.root_element().first_element_child().unwrap();
        let styles = inheritance.list_styles(shape);
        let marks: Vec<&str> = level_props(None, &styles, 1)
            .iter()
            .filter_map(|props| props.attribute("marL"))
            .collect();
        assert_eq!(marks, ["m2", "d2"]);
    }
}
