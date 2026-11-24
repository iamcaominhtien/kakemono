//! A small DOM of a web page, made from the tree html5ever builds: the
//! nodes in one vector, which the cleaning (`clean`, `notes`) edits
//! freely, then writes as an XHTML document for the reader of `epub`.

use html5ever::tendril::TendrilSink;
use html5ever::{ParseOpts, local_name, ns, parse_document};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// Namespace of XHTML.
pub(crate) const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
/// Namespace of SVG.
const SVG_NS: &str = "http://www.w3.org/2000/svg";
/// Namespace of MathML.
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
/// Namespace of XLink (`xlink:href` in an SVG).
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

/// Index of a node in [`Dom::nodes`].
pub(crate) type Id = usize;

/// The namespace of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Space {
    /// HTML.
    Html,
    /// SVG.
    Svg,
    /// MathML.
    Math,
}

/// What a node is.
#[derive(Debug, Clone)]
pub(crate) enum Kind {
    /// The document, the root of the tree.
    Document,
    /// An element: its local name (lower case for HTML), namespace and
    /// attributes (qualified names: `xlink:href`).
    Element {
        /// Local name.
        name: String,
        /// Namespace.
        space: Space,
        /// Attributes, in order.
        attrs: Vec<(String, String)>,
    },
    /// Text.
    Text(String),
}

/// One node.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// What it is.
    pub(crate) kind: Kind,
    /// Its parent; `None` for the document and for a detached node.
    pub(crate) parent: Option<Id>,
    /// Its children, in order.
    pub(crate) children: Vec<Id>,
}

/// The DOM of a page.
#[derive(Debug, Clone)]
pub(crate) struct Dom {
    /// The nodes; the document is node 0.
    pub(crate) nodes: Vec<Node>,
}

impl Dom {
    /// Parses the HTML `text` as a browser does.
    pub(crate) fn parse(text: &str) -> Dom {
        let mut opts = ParseOpts::default();
        // The content of `<noscript>` is markup (a lazy page puts its
        // pictures there), as in a browser without scripts.
        opts.tree_builder.scripting_enabled = false;
        let rc = parse_document(RcDom::default(), opts).one(text);
        let mut dom = Dom {
            nodes: vec![Node {
                kind: Kind::Document,
                parent: None,
                children: Vec::new(),
            }],
        };
        // Iteratively, as a page can nest very deeply.
        let mut stack: Vec<(Handle, Id)> = rc
            .document
            .children
            .borrow()
            .iter()
            .rev()
            .map(|child| (child.clone(), 0))
            .collect();
        while let Some((handle, parent)) = stack.pop() {
            let kind = match &handle.data {
                NodeData::Element { name, attrs, .. } => {
                    let space = if name.ns == ns!(svg) {
                        Space::Svg
                    } else if name.ns == ns!(mathml) {
                        Space::Math
                    } else {
                        Space::Html
                    };
                    let attrs = attrs
                        .borrow()
                        .iter()
                        .filter_map(|attr| {
                            let local = attr.name.local.to_string();
                            let qualified = if attr.name.ns == ns!(xlink) {
                                format!("xlink:{local}")
                            } else if attr.name.ns == ns!(xml) {
                                format!("xml:{local}")
                            } else if attr.name.ns == ns!(xmlns) || attr.name.local == local_name!("xmlns") {
                                return None;
                            } else {
                                local
                            };
                            Some((qualified, attr.value.to_string()))
                        })
                        .collect();
                    Kind::Element {
                        name: name.local.to_string(),
                        space,
                        attrs,
                    }
                }
                NodeData::Text { contents } => Kind::Text(contents.borrow().to_string()),
                // Comments, doctypes, processing instructions.
                _ => continue,
            };
            let id = dom.push(kind);
            dom.append(parent, id);
            // The content of a `<template>`, which is not shown, is not
            // among the children: it is left out.
            for child in handle.children.borrow().iter().rev() {
                stack.push((child.clone(), id));
            }
        }
        dom
    }

    /// Adds a detached node.
    pub(crate) fn push(&mut self, kind: Kind) -> Id {
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
        });
        self.nodes.len() - 1
    }

    /// A new detached HTML element `name` with the attributes `attrs`.
    pub(crate) fn element(&mut self, name: &str, attrs: &[(&str, &str)]) -> Id {
        self.push(Kind::Element {
            name: name.to_string(),
            space: Space::Html,
            attrs: attrs
                .iter()
                .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
                .collect(),
        })
    }

    /// A new detached text node.
    pub(crate) fn text_node(&mut self, text: &str) -> Id {
        self.push(Kind::Text(text.to_string()))
    }

    /// Appends the detached node `child` to `parent`.
    pub(crate) fn append(&mut self, parent: Id, child: Id) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        self.nodes[parent].children.push(child);
    }

    /// Inserts `child` into `parent` at position `at` of its children.
    pub(crate) fn insert(&mut self, parent: Id, at: usize, child: Id) {
        self.detach(child);
        self.nodes[child].parent = Some(parent);
        let at = at.min(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(at, child);
    }

    /// Inserts `node` right after `sibling`.
    pub(crate) fn insert_after(&mut self, sibling: Id, node: Id) {
        if let Some(parent) = self.nodes[sibling].parent {
            let at = self.nodes[parent]
                .children
                .iter()
                .position(|&child| child == sibling)
                .map_or(0, |at| at + 1);
            self.insert(parent, at, node);
        }
    }

    /// Takes `node` (and its content) out of the tree.
    pub(crate) fn detach(&mut self, node: Id) {
        if let Some(parent) = self.nodes[node].parent.take() {
            self.nodes[parent].children.retain(|&child| child != node);
        }
    }

    /// Replaces `node` in the tree by its children.
    pub(crate) fn unwrap(&mut self, node: Id) {
        let Some(parent) = self.nodes[node].parent else {
            return;
        };
        let at = self.nodes[parent]
            .children
            .iter()
            .position(|&child| child == node)
            .unwrap_or(0);
        let children = std::mem::take(&mut self.nodes[node].children);
        self.detach(node);
        for (offset, child) in children.into_iter().enumerate() {
            self.nodes[child].parent = Some(parent);
            self.nodes[parent].children.insert(at + offset, child);
        }
    }

    /// The local name of an element (`None` for another node).
    pub(crate) fn name(&self, node: Id) -> Option<&str> {
        match &self.nodes[node].kind {
            Kind::Element { name, .. } => Some(name),
            _ => None,
        }
    }

    /// Whether `node` is an HTML element named `name`.
    pub(crate) fn is(&self, node: Id, name: &str) -> bool {
        matches!(&self.nodes[node].kind, Kind::Element { name: own, space: Space::Html, .. } if own == name)
    }

    /// Whether `node` is an HTML element with one of the names `names`.
    pub(crate) fn is_any(&self, node: Id, names: &[&str]) -> bool {
        matches!(&self.nodes[node].kind, Kind::Element { name, space: Space::Html, .. } if names.contains(&name.as_str()))
    }

    /// The namespace of an element.
    pub(crate) fn space(&self, node: Id) -> Option<Space> {
        match &self.nodes[node].kind {
            Kind::Element { space, .. } => Some(*space),
            _ => None,
        }
    }

    /// Renames the element `node`.
    pub(crate) fn rename(&mut self, node: Id, new: &str) {
        if let Kind::Element { name, .. } = &mut self.nodes[node].kind {
            *name = new.to_string();
        }
    }

    /// The value of the attribute `key` of `node`.
    pub(crate) fn attr(&self, node: Id, key: &str) -> Option<&str> {
        match &self.nodes[node].kind {
            Kind::Element { attrs, .. } => attrs
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, value)| value.as_str()),
            _ => None,
        }
    }

    /// Sets the attribute `key` of `node` to `value`.
    pub(crate) fn set_attr(&mut self, node: Id, key: &str, value: &str) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[node].kind {
            match attrs.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(key)) {
                Some((_, old)) => *old = value.to_string(),
                None => attrs.push((key.to_string(), value.to_string())),
            }
        }
    }

    /// Removes the attribute `key` of `node`.
    pub(crate) fn remove_attr(&mut self, node: Id, key: &str) {
        if let Kind::Element { attrs, .. } = &mut self.nodes[node].kind {
            attrs.retain(|(name, _)| !name.eq_ignore_ascii_case(key));
        }
    }

    /// The classes of `node`.
    pub(crate) fn classes(&self, node: Id) -> impl Iterator<Item = &str> {
        self.attr(node, "class").unwrap_or_default().split_whitespace()
    }

    /// Whether `node` has the class `class`.
    pub(crate) fn has_class(&self, node: Id, class: &str) -> bool {
        self.classes(node).any(|own| own == class)
    }

    /// Adds the class `class` to `node`.
    pub(crate) fn add_class(&mut self, node: Id, class: &str) {
        if !self.has_class(node, class) {
            let classes = match self.attr(node, "class") {
                Some(old) if !old.trim().is_empty() => format!("{old} {class}"),
                _ => class.to_string(),
            };
            self.set_attr(node, "class", &classes);
        }
    }

    /// The children of `node` that are elements.
    pub(crate) fn element_children(&self, node: Id) -> Vec<Id> {
        self.nodes[node]
            .children
            .iter()
            .copied()
            .filter(|&child| matches!(self.nodes[child].kind, Kind::Element { .. }))
            .collect()
    }

    /// `node` and every node in it, in document order.
    pub(crate) fn descendants(&self, node: Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack = vec![node];
        while let Some(next) = stack.pop() {
            out.push(next);
            stack.extend(self.nodes[next].children.iter().rev());
        }
        out
    }

    /// The elements in `node` (and `node` itself) named `name`.
    pub(crate) fn find_all(&self, node: Id, name: &str) -> Vec<Id> {
        self.descendants(node)
            .into_iter()
            .filter(|&id| self.is(id, name))
            .collect()
    }

    /// The first element in `node` named `name`.
    pub(crate) fn find(&self, node: Id, name: &str) -> Option<Id> {
        self.descendants(node).into_iter().find(|&id| self.is(id, name))
    }

    /// Whether `ancestor` is `node` or contains it.
    pub(crate) fn contains(&self, ancestor: Id, node: Id) -> bool {
        let mut at = Some(node);
        while let Some(id) = at {
            if id == ancestor {
                return true;
            }
            at = self.nodes[id].parent;
        }
        false
    }

    /// Whether `node` is in the tree (its ancestors reach the document).
    pub(crate) fn attached(&self, node: Id) -> bool {
        self.contains(0, node)
    }

    /// The nearest ancestor of `node` (not `node` itself) for which `test`
    /// holds.
    pub(crate) fn ancestor(&self, node: Id, test: impl Fn(Id) -> bool) -> Option<Id> {
        let mut at = self.nodes[node].parent;
        while let Some(id) = at {
            if test(id) {
                return Some(id);
            }
            at = self.nodes[id].parent;
        }
        None
    }

    /// The text of `node` and everything in it.
    pub(crate) fn text(&self, node: Id) -> String {
        let mut out = String::new();
        for id in self.descendants(node) {
            if let Kind::Text(text) = &self.nodes[id].kind {
                out.push_str(text);
            }
        }
        out
    }

    /// The text of `node` with its white space collapsed and trimmed.
    pub(crate) fn clean_text(&self, node: Id) -> String {
        self.text(node).split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The `<html>`, `<head>` and `<body>` elements.
    pub(crate) fn html_head_body(&self) -> (Option<Id>, Option<Id>, Option<Id>) {
        let html = self.element_children(0).into_iter().find(|&id| self.is(id, "html"));
        let part =
            |name: &str| html.and_then(|html| self.element_children(html).into_iter().find(|&id| self.is(id, name)));
        (html, part("head"), part("body"))
    }

    /// The XHTML document of the elements `head` (title, style sheets)
    /// and `body`: well-formed XML, with the namespaces of HTML, SVG and
    /// MathML, which the reader of `epub` reads.
    pub(crate) fn to_xhtml(&self, head: &[Id], body: Id, lang: Option<&str>) -> String {
        let mut out = String::with_capacity(1 << 16);
        out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"");
        out.push_str(XHTML_NS);
        out.push_str("\" xmlns:xlink=\"");
        out.push_str(XLINK_NS);
        out.push('"');
        if let Some(lang) = lang {
            out.push_str(" xml:lang=\"");
            escape_into(&mut out, lang, true);
            out.push('"');
        }
        out.push_str("><head>");
        for &node in head {
            self.write(&mut out, node, Space::Html);
        }
        out.push_str("</head>");
        if self.is(body, "body") {
            self.write(&mut out, body, Space::Html);
        } else {
            out.push_str("<body>");
            self.write(&mut out, body, Space::Html);
            out.push_str("</body>");
        }
        out.push_str("</html>\n");
        out
    }

    /// Writes `node` as XML into `out`; `outer` is the namespace of its
    /// parent (a change of namespace is declared).
    fn write(&self, out: &mut String, node: Id, outer: Space) {
        enum Step {
            Open(Id, Space),
            Close(String),
        }
        let mut stack = vec![Step::Open(node, outer)];
        while let Some(step) = stack.pop() {
            let (id, outer) = match step {
                Step::Close(name) => {
                    out.push_str("</");
                    out.push_str(&name);
                    out.push('>');
                    continue;
                }
                Step::Open(id, outer) => (id, outer),
            };
            match &self.nodes[id].kind {
                Kind::Document => {
                    for &child in self.nodes[id].children.iter().rev() {
                        stack.push(Step::Open(child, outer));
                    }
                }
                Kind::Text(text) => escape_into(out, text, false),
                Kind::Element { name, space, attrs } => {
                    let name = if is_xml_name(name) { name.as_str() } else { "span" };
                    out.push('<');
                    out.push_str(name);
                    if *space != outer {
                        out.push_str(" xmlns=\"");
                        out.push_str(match space {
                            Space::Html => XHTML_NS,
                            Space::Svg => SVG_NS,
                            Space::Math => MATHML_NS,
                        });
                        out.push('"');
                    }
                    for (key, value) in attrs {
                        let plain = key.strip_prefix("xlink:").or_else(|| key.strip_prefix("xml:"));
                        if !is_xml_name(plain.unwrap_or(key)) || (plain.is_none() && key.contains(':')) {
                            continue;
                        }
                        out.push(' ');
                        out.push_str(key);
                        out.push_str("=\"");
                        escape_into(out, value, true);
                        out.push('"');
                    }
                    let children = &self.nodes[id].children;
                    if children.is_empty() {
                        out.push_str("/>");
                        continue;
                    }
                    out.push('>');
                    stack.push(Step::Close(name.to_string()));
                    for &child in children.iter().rev() {
                        stack.push(Step::Open(child, *space));
                    }
                }
            }
        }
    }
}

/// Whether `name` is a name XML accepts (without a prefix): a letter or
/// `_` first, then letters, digits, `-`, `_` and `.`.
fn is_xml_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

/// Appends `text` to `out` escaped for XML (quotes too in an attribute);
/// the characters XML forbids are left out.
fn escape_into(out: &mut String, text: &str, attribute: bool) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if attribute => out.push_str("&quot;"),
            '\t' | '\n' | '\r' if attribute => out.push(' '),
            '\t' | '\n' | '\r' => out.push(ch),
            '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {}
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Dom;

    #[test]
    fn broken_html_becomes_well_formed_xhtml() {
        let dom = Dom::parse(
            "<!doctype html><title>T</title><p>a<b>b<i>c</b>d<br><img src=x.png alt='a\"b' @click=go>\
             <svg viewBox='0 0 10 10'><image xlink:href='p.png'/></svg>&nbsp;&copy;<math><mi>x</mi></math>",
        );
        let (_, _, body) = dom.html_head_body();
        let xhtml = dom.to_xhtml(&[], body.unwrap(), Some("en"));
        assert!(roxmltree::Document::parse(&xhtml).is_ok(), "{xhtml}");
        // The misnested `<i>` is reopened after `</b>`, as in a browser.
        assert!(xhtml.contains("<b>b<i>c</i></b><i>d<br/>"), "{xhtml}");
        assert!(xhtml.contains(r#"<img src="x.png" alt="a&quot;b"/>"#), "{xhtml}");
        assert!(
            xhtml
                .contains(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><image xlink:href="p.png"/>"#),
            "{xhtml}"
        );
        assert!(
            xhtml.contains("\u{a0}\u{a9}<math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mi>x</mi></math>"),
            "{xhtml}"
        );
    }
}
