//! Small helpers over `roxmltree` for the WordprocessingML of a `.docx`:
//! namespaces, children by name, attribute values.

use roxmltree::Node;

/// Namespace of WordprocessingML (`w:`).
pub(crate) const WORD_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// Namespace of Office Math (`m:`).
pub(crate) const MATH_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// Namespace of relationship ids in document parts (`r:`).
pub(crate) const REL_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Namespace of DrawingML (`a:`).
pub(crate) const DRAWING_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// Namespace of the placement of drawings in a document (`wp:`).
pub(crate) const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// Namespace of VML (`v:`), the pictures of older documents.
pub(crate) const VML_NS: &str = "urn:schemas-microsoft-com:vml";
/// Namespace of the markup compatibility elements (`mc:`).
pub(crate) const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
/// Namespace of the package relationships (`.rels` parts).
pub(crate) const PACKAGE_REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
/// Namespace of Dublin Core (`dc:`), in the document properties.
pub(crate) const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

/// Whether `node` is the element `name` of namespace `ns`.
pub(crate) fn is(node: Node<'_, '_>, ns: &str, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name && node.tag_name().namespace() == Some(ns)
}

/// The first child element `name` of namespace `ns`.
pub(crate) fn child<'a, 'input>(node: Node<'a, 'input>, ns: &str, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|item| is(*item, ns, name))
}

/// The child elements `name` of namespace `ns`, in order.
pub(crate) fn children<'a, 'input>(
    node: Node<'a, 'input>,
    ns: &'a str,
    name: &'a str,
) -> impl Iterator<Item = Node<'a, 'input>> + 'a {
    node.children().filter(move |item| is(*item, ns, name))
}

/// The attribute `name` of namespace `ns`; an attribute without a
/// namespace of the same name is taken too, as some writers leave it out.
pub(crate) fn attr<'a>(node: Node<'a, '_>, ns: &str, name: &str) -> Option<&'a str> {
    node.attribute((ns, name)).or_else(|| node.attribute(name))
}

/// The `w:val` of the child `name` of `node` (a property such as
/// `<w:pStyle w:val="Heading1"/>`).
pub(crate) fn val<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    attr(child(node, WORD_NS, name)?, WORD_NS, "val")
}

/// Whether the on/off property `name` of `props` is on: present, with no
/// `w:val` or a true one. `None` when it is not set.
pub(crate) fn toggle(props: Node<'_, '_>, name: &str) -> Option<bool> {
    let prop = child(props, WORD_NS, name)?;
    Some(!matches!(
        attr(prop, WORD_NS, "val"),
        Some("0" | "false" | "off" | "none")
    ))
}

/// The text of a run (`w:r`): its `w:t`, tabs as spaces, line breaks as
/// newlines, non-breaking hyphens; deleted text is not part of it.
pub(crate) fn run_text(run: Node<'_, '_>) -> String {
    let mut out = String::new();
    for item in run.children().filter(Node::is_element) {
        if item.tag_name().namespace() != Some(WORD_NS) {
            continue;
        }
        match item.tag_name().name() {
            "t" => out.push_str(item.text().unwrap_or_default()),
            "tab" | "ptab" => out.push(' '),
            "br" | "cr" => out.push('\n'),
            "noBreakHyphen" => out.push('\u{2011}'),
            "sym" => out.extend(symbol(item)),
            _ => {}
        }
    }
    out
}

/// The character of a `w:sym` element (a character of a symbol font, such
/// as Symbol or Wingdings), mapped to Unicode where it is a Symbol letter
/// or sign.
pub(crate) fn symbol(node: Node<'_, '_>) -> Option<char> {
    let code = u32::from_str_radix(attr(node, WORD_NS, "char")?, 16).ok()?;
    // Symbol fonts put their characters in the private use area F0xx.
    let code = if (0xF000..=0xF0FF).contains(&code) {
        code - 0xF000
    } else {
        code
    };
    let font = attr(node, WORD_NS, "font").unwrap_or_default();
    if font.eq_ignore_ascii_case("Symbol") {
        return u8::try_from(code).ok().and_then(super::symbols::symbol_font_char);
    }
    char::from_u32(code)
}
