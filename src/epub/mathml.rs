//! MathML (`<math>`) read into a [`MathNode`] tree, the structure every
//! output writes formulas from (MathML again, Office Math, LaTeX).

use crate::model::MathNode;
use roxmltree::Node;

/// Width of `thinmathspace`, in em.
const THIN_SPACE_EM: f32 = 0.167;
/// Width of `mediummathspace`, in em.
const MEDIUM_SPACE_EM: f32 = 0.222;
/// Width of `thickmathspace`, in em.
const THICK_SPACE_EM: f32 = 0.278;
/// Em per `ex`, roughly, for a space given in ex.
const EM_PER_EX: f32 = 0.5;

/// The formula of a `<math>` element, and whether it is displayed on a
/// line of its own; `None` when it holds nothing.
pub(crate) fn read(math: Node<'_, '_>) -> Option<(MathNode, bool)> {
    let display = math.attribute("display") == Some("block") || math.attribute("mode") == Some("display");
    let node = MathNode::row(children(math));
    let empty = matches!(&node, MathNode::Row(nodes) if nodes.is_empty());
    (!empty).then_some((node, display))
}

/// The nodes of the element children of `node`.
fn children(node: Node<'_, '_>) -> Vec<MathNode> {
    node.children().filter(Node::is_element).filter_map(element).collect()
}

/// The `n`-th element child of `node`, or an empty row.
fn arg(node: Node<'_, '_>, n: usize) -> Box<MathNode> {
    Box::new(
        node.children()
            .filter(Node::is_element)
            .nth(n)
            .and_then(element)
            .unwrap_or(MathNode::Row(Vec::new())),
    )
}

/// The node of one MathML element; `None` for what shows nothing (an
/// annotation, `<none/>`, a phantom).
fn element(node: Node<'_, '_>) -> Option<MathNode> {
    let name = node.tag_name().name();
    Some(match name {
        "mi" => {
            let text = token_text(node);
            let upright = node.attribute("mathvariant") == Some("normal") && text.chars().count() == 1;
            MathNode::Ident { text, upright }
        }
        "mn" => MathNode::Number(token_text(node)),
        "mo" => MathNode::Operator {
            text: token_text(node),
            height_em: None,
        },
        "mtext" | "ms" => MathNode::Text(token_text(node)),
        "mspace" => MathNode::Space(node.attribute("width").map_or(0.0, space_em)),
        "mfrac" => MathNode::Frac {
            num: arg(node, 0),
            den: arg(node, 1),
        },
        "msqrt" => MathNode::Sqrt(Box::new(MathNode::row(children(node)))),
        "mroot" => MathNode::Root {
            base: arg(node, 0),
            index: arg(node, 1),
        },
        "msub" | "msup" | "msubsup" => scripts(node, name),
        "munder" | "mover" | "munderover" => under_over(node, name),
        "mtable" => table(node),
        "mfenced" => fenced(node),
        "semantics" => node.children().filter(Node::is_element).find_map(element)?,
        "annotation" | "annotation-xml" | "none" | "mprescripts" | "mphantom" | "maligngroup" | "malignmark" => {
            return None;
        }
        _ => MathNode::row(children(node)),
    })
}

/// A `<msub>`, `<msup>` or `<msubsup>`.
fn scripts(node: Node<'_, '_>, name: &str) -> MathNode {
    let (sub, sup) = match name {
        "msub" => (Some(arg(node, 1)), None),
        "msup" => (None, Some(arg(node, 1))),
        _ => (Some(arg(node, 1)), Some(arg(node, 2))),
    };
    MathNode::Scripts {
        base: arg(node, 0),
        sub,
        sup,
    }
}

/// A `<munder>`, `<mover>` or `<munderover>`.
fn under_over(node: Node<'_, '_>, name: &str) -> MathNode {
    let (under, over) = match name {
        "munder" => (Some(arg(node, 1)), None),
        "mover" => (None, Some(arg(node, 1))),
        _ => (Some(arg(node, 1)), Some(arg(node, 2))),
    };
    MathNode::UnderOver {
        base: arg(node, 0),
        under,
        over,
        accent: node.attribute("accent") == Some("true"),
    }
}

/// A `<mtable>`: its rows of cells, and the alignment of its columns.
fn table(node: Node<'_, '_>) -> MathNode {
    let rows: Vec<Vec<MathNode>> = node
        .children()
        .filter(|row| row.has_tag_name("mtr") || row.has_tag_name("mlabeledtr"))
        .map(|row| {
            row.children()
                .filter(|cell| cell.has_tag_name("mtd"))
                .map(|cell| MathNode::row(children(cell)))
                .collect()
        })
        .collect();
    let n_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let given: Vec<&str> = node
        .attribute("columnalign")
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    let align = (0..n_cols)
        .map(|col| match given.get(col).or(given.last()).copied() {
            Some("left") => "left",
            Some("right") => "right",
            _ => "center",
        })
        .collect();
    MathNode::Table { rows, align }
}

/// A `<mfenced>`: its children between its delimiters, with separators.
fn fenced(node: Node<'_, '_>) -> MathNode {
    let operator = |text: &str| MathNode::Operator {
        text: text.to_string(),
        height_em: None,
    };
    let separators: Vec<char> = node
        .attribute("separators")
        .unwrap_or(",")
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    let mut nodes = vec![operator(node.attribute("open").unwrap_or("("))];
    for (at, child) in children(node).into_iter().enumerate() {
        if at > 0
            && let Some(sep) = separators.get(at - 1).or(separators.last())
        {
            nodes.push(operator(&sep.to_string()));
        }
        nodes.push(child);
    }
    nodes.push(operator(node.attribute("close").unwrap_or(")")));
    MathNode::Row(nodes)
}

/// The text of a token element, trimmed, as MathML wants.
fn token_text(node: Node<'_, '_>) -> String {
    let text: String = node
        .descendants()
        .filter(Node::is_text)
        .filter_map(|child| child.text())
        .collect();
    text.trim().to_string()
}

/// The width of a `<mspace>`, in em.
fn space_em(width: &str) -> f32 {
    let width = width.trim();
    match width {
        "thinmathspace" => return THIN_SPACE_EM,
        "mediummathspace" => return MEDIUM_SPACE_EM,
        "thickmathspace" => return THICK_SPACE_EM,
        _ => {}
    }
    if let Some(em) = width.strip_suffix("em") {
        return em.trim().parse().unwrap_or(0.0);
    }
    if let Some(ex) = width.strip_suffix("ex") {
        return ex.trim().parse::<f32>().unwrap_or(0.0) * EM_PER_EX;
    }
    0.0
}

#[cfg(test)]
mod tests {
    use super::read;
    use crate::model::MathNode;

    #[test]
    fn fractions_scripts_and_annotations() {
        let text = r#"<math xmlns="http://www.w3.org/1998/Math/MathML" display="block"><semantics><mrow>
            <msub><mi>x</mi><mn>1</mn></msub><mo>=</mo><mfrac><mn>1</mn><mi>L</mi></mfrac></mrow>
            <annotation encoding="application/x-tex">x_1=\frac1L</annotation></semantics></math>"#;
        let xml = roxmltree::Document::parse(text).unwrap();
        let (node, display) = read(xml.root_element()).unwrap();
        assert!(display);
        let MathNode::Row(nodes) = node else {
            panic!("a row: {node:?}")
        };
        assert_eq!(nodes.len(), 3);
        assert!(matches!(
            &nodes[0],
            MathNode::Scripts {
                sub: Some(_),
                sup: None,
                ..
            }
        ));
        assert!(matches!(&nodes[2], MathNode::Frac { .. }));
    }
}
