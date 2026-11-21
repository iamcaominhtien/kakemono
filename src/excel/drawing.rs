//! The text of the shapes and text boxes of a sheet's drawing part
//! (`xl/drawings/drawingN.xml`): Japanese workbooks often put text, org
//! charts and flows in shapes rather than in cells.

use crate::word::docx::parse;
use anyhow::Result;
use roxmltree::Node;

/// A shape's anchor (row, column, both 0-based) and its text.
pub(crate) type Shape = ((u32, u32), String);

/// The anchor of an `absoluteAnchor`, which names no cell: it sorts after
/// every anchored shape, and absolute ones stay in document order.
const UNANCHORED: (u32, u32) = (u32::MAX, u32::MAX);

/// The text of each shape (`sp`) of a drawing with some, also inside a
/// group, with the cell its anchor starts at; sorted by anchor (row, then
/// column), shapes with the same anchor in document order. Pictures,
/// connectors and charts give nothing. The shapes of a group share the
/// group's anchor. An `mc:AlternateContent` is read through its `Choice`
/// only (its `Fallback` repeats it). A shape's text is its paragraphs
/// (`a:p`) joined by `\n`; its runs are joined, a `a:br` is a `\n`; a
/// shape with only blanks is left out.
///
/// # Errors
/// When the XML is damaged.
pub(crate) fn texts(xml: &str) -> Result<Vec<Shape>> {
    let doc = parse(xml)?;
    let mut shapes = Vec::new();
    anchors(doc.root_element(), &mut shapes);
    shapes.sort_by_key(|(anchor, _)| *anchor);
    Ok(shapes)
}

/// Reads the anchors among the children of `node`.
fn anchors(node: Node<'_, '_>, shapes: &mut Vec<Shape>) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor" => {
                let anchor = from(child).unwrap_or(UNANCHORED);
                collect(child, anchor, shapes);
            }
            "AlternateContent" => alternative(child, |inner| anchors(inner, shapes)),
            _ => {}
        }
    }
}

/// Reads the shapes among the children of `node` (an anchor or a group),
/// which all get `anchor`.
fn collect(node: Node<'_, '_>, anchor: (u32, u32), shapes: &mut Vec<Shape>) {
    for child in node.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "sp" => {
                if let Some(text) = text_of(child) {
                    shapes.push((anchor, text));
                }
            }
            "grpSp" => collect(child, anchor, shapes),
            "AlternateContent" => alternative(child, |inner| collect(inner, anchor, shapes)),
            _ => {}
        }
    }
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

/// The text of a shape: its paragraphs joined by `\n`; `None` when it has
/// only blanks.
fn text_of(shape: Node<'_, '_>) -> Option<String> {
    let body = named(shape, "txBody").next()?;
    let paragraphs: Vec<String> = named(body, "p").map(paragraph).collect();
    let text = paragraphs.join("\n");
    (!text.trim().is_empty()).then_some(text)
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

    const HEAD: &str = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">"#;

    fn sp(text: &str) -> String {
        format!(r#"<xdr:sp><xdr:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></xdr:txBody></xdr:sp>"#)
    }

    fn two_cell(row: u32, col: u32, inner: &str) -> String {
        format!(
            r#"<xdr:twoCellAnchor><xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>9</xdr:col><xdr:row>9</xdr:row></xdr:to>{inner}<xdr:clientData/></xdr:twoCellAnchor>"#
        )
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
    fn pictures_and_connectors_give_nothing() {
        let inner = r#"<xdr:pic><xdr:nvPicPr/><xdr:blipFill/></xdr:pic><xdr:cxnSp><xdr:nvCxnSpPr/><xdr:spPr/></xdr:cxnSp><xdr:graphicFrame><a:graphic><a:t>chart</a:t></a:graphic></xdr:graphicFrame>"#;
        let xml = format!("{HEAD}{}</xdr:wsDr>", two_cell(0, 0, inner));
        assert!(texts(&xml).unwrap().is_empty());
    }

    #[test]
    fn damaged_xml_is_an_error() {
        assert!(texts("<xdr:wsDr").is_err());
    }
}
