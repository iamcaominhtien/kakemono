//! Office Math (OMML), the equations of Word, read into [`MathNode`]
//! trees: the reverse of `render/docx/omml.rs`.
//!
//! OMML has no row element: the children of an argument (`m:e`, `m:num`,
//! ...) follow one another, and their runs (`m:r`) hold text that is cut
//! here into numbers, identifiers and operators, as MathML wants.

use super::xml::{MATH_NS, WORD_NS, attr, child, children, is};
use crate::analysis::math::{Alphabet, styled};
use crate::model::MathNode;
use roxmltree::Node;

/// Large operators whose limits go under and over them in a displayed
/// formula when the document does not say where (`m:limLoc`).
const UNDER_OVER_OPERATORS: &[char] = &['∑', '∏', '∐', '⋃', '⋂', '⋁', '⋀', '⨁', '⨂', '⨀', '⨄', '⨆'];
/// The large operator of an `m:nary` that names none: an integral.
const DEFAULT_NARY: &str = "∫";
/// Height (in em) of a large operator, for the delimiters around it.
const NARY_HEIGHT_EM: f32 = 2.2;
/// Height (in em) of a line of a formula.
const LINE_HEIGHT_EM: f32 = 1.0;
/// Extra height (in em) of a bar, a radical or a script over its content.
const RAISE_EM: f32 = 0.3;
/// A delimiter around content at most this tall (in em) keeps its normal
/// size.
const PLAIN_DELIMITER_MAX_EM: f32 = 1.4;
/// Width (in em) of a space character in a formula run.
const SPACE_EM: f32 = 0.25;

/// The formula of an `m:oMathPara` or `m:oMath` element. Several
/// `m:oMath` in one `m:oMathPara` are lines of one display: a table of one
/// column.
pub(crate) fn formula(node: Node<'_, '_>) -> MathNode {
    if is(node, MATH_NS, "oMathPara") {
        let lines: Vec<MathNode> = children(node, MATH_NS, "oMath").map(argument).collect();
        return match lines.len() {
            0 => MathNode::Row(Vec::new()),
            1 => lines.into_iter().next().expect("one line"),
            _ => MathNode::Table {
                rows: lines.into_iter().map(|line| vec![line]).collect(),
                align: vec!["center"],
            },
        };
    }
    argument(node)
}

/// The plain text of a formula, for alt text and search: its symbols in
/// order, fractions as `a/b`, roots with `√`.
pub(crate) fn plain_text(node: &MathNode) -> String {
    let mut out = String::new();
    write_plain(node, &mut out);
    out.trim().to_string()
}

/// The children of an argument element (`m:e`, `m:num`, `m:oMath`, ...)
/// as one node.
fn argument(node: Node<'_, '_>) -> MathNode {
    let mut nodes = Vec::new();
    for item in node.children().filter(Node::is_element) {
        push_element(item, &mut nodes);
    }
    MathNode::row(nodes)
}

/// The argument `name` of `node` (an empty row when it is missing).
fn arg(node: Node<'_, '_>, name: &str) -> MathNode {
    child(node, MATH_NS, name).map_or(MathNode::Row(Vec::new()), argument)
}

/// The argument `name` of `node`, or `None` when it is missing or empty.
fn opt_arg(node: Node<'_, '_>, name: &str) -> Option<Box<MathNode>> {
    let arg = child(node, MATH_NS, name).map(argument)?;
    (!is_empty(&arg)).then(|| Box::new(arg))
}

/// The value of the property `prop` of the properties element `props`
/// (`m:chr`, `m:begChr`, ...) of `node`.
fn prop<'a>(node: Node<'a, '_>, props: &str, prop: &str) -> Option<&'a str> {
    attr(child(child(node, MATH_NS, props)?, MATH_NS, prop)?, MATH_NS, "val")
}

/// Whether the on/off property `prop` of `props` is on (`m:val` of `1`,
/// `on` or `true`, or no value).
fn flag(node: Node<'_, '_>, props: &str, prop: &str) -> bool {
    child(node, MATH_NS, props)
        .and_then(|props| child(props, MATH_NS, prop))
        .is_some_and(|flag| !matches!(attr(flag, MATH_NS, "val"), Some("0" | "off" | "false")))
}

/// Adds the nodes of one OMML element to `out`.
fn push_element(node: Node<'_, '_>, out: &mut Vec<MathNode>) {
    let ns = node.tag_name().namespace();
    if ns == Some(WORD_NS) {
        return push_word_element(node, out);
    }
    if ns != Some(MATH_NS) {
        return;
    }
    let name = node.tag_name().name();
    if name.ends_with("Pr") {
        return;
    }
    match name {
        "r" => push_run(node, out),
        "f" => out.push(fraction(node)),
        "rad" => out.push(radical(node)),
        "sSub" => out.push(scripts(arg(node, "e"), opt_arg(node, "sub"), None)),
        "sSup" => out.push(scripts(arg(node, "e"), None, opt_arg(node, "sup"))),
        "sSubSup" => out.push(scripts(arg(node, "e"), opt_arg(node, "sub"), opt_arg(node, "sup"))),
        "sPre" => {
            out.push(scripts(
                MathNode::Row(Vec::new()),
                opt_arg(node, "sub"),
                opt_arg(node, "sup"),
            ));
            out.push(arg(node, "e"));
        }
        "nary" => nary(node, out),
        "d" => delimited(node, out),
        "func" => {
            out.push(upright(arg(node, "fName")));
            out.push(arg(node, "e"));
        }
        "acc" => {
            let mark = prop(node, "accPr", "chr").unwrap_or("\u{302}");
            out.push(over(arg(node, "e"), mark, true));
        }
        "bar" => {
            let top = prop(node, "barPr", "pos") == Some("top");
            out.push(if top {
                over(arg(node, "e"), "\u{305}", true)
            } else {
                under(arg(node, "e"), "_")
            });
        }
        "groupChr" => {
            let top = prop(node, "groupChrPr", "pos") == Some("top");
            let mark = prop(node, "groupChrPr", "chr").unwrap_or("⏟");
            out.push(if top {
                over(arg(node, "e"), mark, false)
            } else {
                under(arg(node, "e"), mark)
            });
        }
        "limLow" => out.push(MathNode::UnderOver {
            base: Box::new(arg(node, "e")),
            under: opt_arg(node, "lim"),
            over: None,
            accent: false,
        }),
        "limUpp" => out.push(MathNode::UnderOver {
            base: Box::new(arg(node, "e")),
            under: None,
            over: opt_arg(node, "lim"),
            accent: false,
        }),
        "m" => out.push(matrix(node)),
        "eqArr" => out.push(equation_array(node)),
        // A phantom only reserves room; its content is not shown.
        "phant" => {}
        "oMath" | "oMathPara" => out.push(formula(node)),
        // Boxes, borders and any element not known here: their content.
        _ => out.push(argument(node)),
    }
}

/// Adds the nodes of a WordprocessingML element inside a formula: normal
/// text runs, and the containers Word wraps them in.
fn push_word_element(node: Node<'_, '_>, out: &mut Vec<MathNode>) {
    match node.tag_name().name() {
        "r" => {
            let text = super::xml::run_text(node);
            if !text.trim().is_empty() {
                out.push(MathNode::Text(text));
            }
        }
        "ins" | "smartTag" | "customXml" | "sdt" | "sdtContent" | "hyperlink" => {
            node.children()
                .filter(Node::is_element)
                .for_each(|item| push_element(item, out));
        }
        _ => {}
    }
}

/// A fraction: stacked (`m:f`), or written on one line (`m:type` `lin`)
/// as `a/b`.
fn fraction(node: Node<'_, '_>) -> MathNode {
    let (num, den) = (arg(node, "num"), arg(node, "den"));
    match prop(node, "fPr", "type") {
        Some("lin") | Some("skw") => MathNode::Row(vec![num, operator("/"), den]),
        _ => MathNode::Frac {
            num: Box::new(num),
            den: Box::new(den),
        },
    }
}

/// A radical: a square root, or a root with its index.
fn radical(node: Node<'_, '_>) -> MathNode {
    let base = Box::new(arg(node, "e"));
    match opt_arg(node, "deg").filter(|_| !flag(node, "radPr", "degHide")) {
        Some(index) => MathNode::Root { base, index },
        None => MathNode::Sqrt(base),
    }
}

/// `base` with its scripts, or `base` alone when it has none.
fn scripts(base: MathNode, sub: Option<Box<MathNode>>, sup: Option<Box<MathNode>>) -> MathNode {
    if sub.is_none() && sup.is_none() {
        return base;
    }
    MathNode::Scripts {
        base: Box::new(base),
        sub,
        sup,
    }
}

/// A large operator (`m:nary`) with its limits, then its operand.
fn nary(node: Node<'_, '_>, out: &mut Vec<MathNode>) {
    let symbol = prop(node, "naryPr", "chr").unwrap_or(DEFAULT_NARY).to_string();
    let lower = opt_arg(node, "sub").filter(|_| !flag(node, "naryPr", "subHide"));
    let upper = opt_arg(node, "sup").filter(|_| !flag(node, "naryPr", "supHide"));
    let under_over = match prop(node, "naryPr", "limLoc") {
        Some(location) => location == "undOvr",
        None => symbol
            .chars()
            .next()
            .is_some_and(|ch| UNDER_OVER_OPERATORS.contains(&ch)),
    };
    let base = Box::new(operator(&symbol));
    out.push(if lower.is_none() && upper.is_none() {
        *base
    } else if under_over {
        MathNode::UnderOver {
            base,
            under: lower,
            over: upper,
            accent: false,
        }
    } else {
        MathNode::Scripts {
            base,
            sub: lower,
            sup: upper,
        }
    });
    let operand = arg(node, "e");
    if !is_empty(&operand) {
        out.push(operand);
    }
}

/// Content between delimiters (`m:d`): the opening one, the arguments
/// separated by the separator, the closing one. The delimiters grow with
/// tall content.
fn delimited(node: Node<'_, '_>, out: &mut Vec<MathNode>) {
    let begin = prop(node, "dPr", "begChr").unwrap_or("(");
    let end = prop(node, "dPr", "endChr").unwrap_or(")");
    let separator = prop(node, "dPr", "sepChr").unwrap_or("|");
    let parts: Vec<MathNode> = children(node, MATH_NS, "e").map(argument).collect();
    let tallest = parts.iter().map(height_em).fold(LINE_HEIGHT_EM, f32::max);
    let height = (tallest > PLAIN_DELIMITER_MAX_EM).then_some(tallest);
    let delimiter = |text: &str| MathNode::Operator {
        text: text.to_string(),
        height_em: height,
    };
    if !begin.is_empty() {
        out.push(delimiter(begin));
    }
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            out.push(delimiter(separator));
        }
        out.push(part);
    }
    if !end.is_empty() {
        out.push(delimiter(end));
    }
}

/// A matrix (`m:m`): its rows of cells, each column aligned as its
/// `m:mcJc` says.
fn matrix(node: Node<'_, '_>) -> MathNode {
    let rows: Vec<Vec<MathNode>> = children(node, MATH_NS, "mr")
        .map(|row| children(row, MATH_NS, "e").map(argument).collect())
        .collect();
    let n_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut align: Vec<&'static str> = child(node, MATH_NS, "mPr")
        .and_then(|props| child(props, MATH_NS, "mcs"))
        .map(|columns| {
            children(columns, MATH_NS, "mc")
                .flat_map(|column| {
                    let justify = prop(column, "mcPr", "mcJc");
                    let count = prop(column, "mcPr", "count")
                        .and_then(|count| count.parse::<usize>().ok())
                        .unwrap_or(1);
                    std::iter::repeat_n(alignment(justify), count)
                })
                .collect()
        })
        .unwrap_or_default();
    align.resize(n_cols, "center");
    MathNode::Table { rows, align }
}

/// An equation array (`m:eqArr`): lines aligned at their `&` marks, which
/// cut each line into cells, right- and left-aligned in turn.
fn equation_array(node: Node<'_, '_>) -> MathNode {
    let rows: Vec<Vec<MathNode>> = children(node, MATH_NS, "e")
        .map(|line| split_at_marks(argument(line)))
        .collect();
    let n_cols = rows.iter().map(Vec::len).max().unwrap_or(1);
    let align = (0..n_cols)
        .map(|col| match (n_cols, col % 2) {
            (1, _) => "center",
            (_, 0) => "right",
            _ => "left",
        })
        .collect();
    MathNode::Table { rows, align }
}

/// The cells of a line of an equation array: the parts between its `&`
/// operators.
fn split_at_marks(line: MathNode) -> Vec<MathNode> {
    let nodes = match line {
        MathNode::Row(nodes) => nodes,
        other => vec![other],
    };
    let mut cells = vec![Vec::new()];
    for node in nodes {
        match node {
            MathNode::Operator { ref text, .. } if text == "&" => cells.push(Vec::new()),
            other => cells.last_mut().expect("one cell at least").push(other),
        }
    }
    cells.into_iter().map(MathNode::row).collect()
}

/// The MathML alignment of a matrix column (`m:mcJc`).
fn alignment(justify: Option<&str>) -> &'static str {
    match justify {
        Some("left") => "left",
        Some("right") => "right",
        _ => "center",
    }
}

/// `base` with the mark `mark` above it (an accent when `accent`).
fn over(base: MathNode, mark: &str, accent: bool) -> MathNode {
    MathNode::UnderOver {
        base: Box::new(base),
        under: None,
        over: Some(Box::new(operator(mark))),
        accent,
    }
}

/// `base` with the mark `mark` below it.
fn under(base: MathNode, mark: &str) -> MathNode {
    MathNode::UnderOver {
        base: Box::new(base),
        under: Some(Box::new(operator(mark))),
        over: None,
        accent: false,
    }
}

/// An operator of normal size.
fn operator(text: &str) -> MathNode {
    MathNode::Operator {
        text: text.to_string(),
        height_em: None,
    }
}

/// `node` with its single letters set upright (a function name).
fn upright(node: MathNode) -> MathNode {
    match node {
        MathNode::Ident { text, .. } => MathNode::Ident { text, upright: true },
        MathNode::Row(nodes) => {
            // The letters of a name cut into single identifiers join again.
            if nodes.iter().all(|node| matches!(node, MathNode::Ident { .. })) && nodes.len() > 1 {
                let text = nodes
                    .iter()
                    .map(|node| match node {
                        MathNode::Ident { text, .. } => text.as_str(),
                        _ => "",
                    })
                    .collect();
                return MathNode::Ident { text, upright: true };
            }
            MathNode::Row(nodes.into_iter().map(upright).collect())
        }
        other => other,
    }
}

/// Adds the nodes of a math run (`m:r`): its text cut into numbers,
/// identifiers and operators. Upright text (`m:sty` `p`) keeps its words
/// whole; normal text (`m:nor`) is text.
fn push_run(node: Node<'_, '_>, out: &mut Vec<MathNode>) {
    let text: String = node
        .descendants()
        .filter(|item| is(*item, MATH_NS, "t") || is(*item, WORD_NS, "t"))
        .filter_map(|item| item.text())
        .collect();
    if text.is_empty() {
        return;
    }
    let props = child(node, MATH_NS, "rPr");
    let normal = props.and_then(|props| child(props, MATH_NS, "nor")).is_some();
    if normal {
        out.push(MathNode::Text(text));
        return;
    }
    let style = props
        .and_then(|props| child(props, MATH_NS, "sty"))
        .and_then(|sty| attr(sty, MATH_NS, "val"));
    let script = props
        .and_then(|props| child(props, MATH_NS, "scr"))
        .and_then(|scr| attr(scr, MATH_NS, "val"));
    let alphabet = match (script, style) {
        (Some("double-struck"), _) => Alphabet::DoubleStruck,
        (Some("script"), _) => Alphabet::Script,
        (Some("fraktur"), _) => Alphabet::Fraktur,
        (_, Some("b")) => Alphabet::Bold,
        (_, Some("bi")) => Alphabet::BoldItalic,
        _ => Alphabet::Plain,
    };
    let straight = matches!(style, Some("p" | "b")) && alphabet == Alphabet::Plain;
    tokenize(&styled(&text, alphabet), straight, out);
}

/// Cuts the text of a run into nodes: runs of digits (with a decimal
/// point between digits) are numbers, letters identifiers (one per letter,
/// or a whole word when `straight`), other characters operators.
fn tokenize(text: &str, straight: bool, out: &mut Vec<MathNode>) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || (chars[i] == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                i += 1;
            }
            out.push(MathNode::Number(chars[start..i].iter().collect()));
            continue;
        }
        if ch.is_alphabetic() {
            if straight {
                let start = i;
                while i < chars.len() && chars[i].is_alphabetic() {
                    i += 1;
                }
                out.push(MathNode::Ident {
                    text: chars[start..i].iter().collect(),
                    upright: true,
                });
                continue;
            }
            out.push(MathNode::Ident {
                text: ch.to_string(),
                upright: false,
            });
        } else if ch.is_whitespace() {
            out.push(MathNode::Space(SPACE_EM));
        } else {
            out.push(operator(&ch.to_string()));
        }
        i += 1;
    }
}

/// Whether a node shows nothing (an empty row).
fn is_empty(node: &MathNode) -> bool {
    match node {
        MathNode::Row(nodes) => nodes.iter().all(is_empty),
        _ => false,
    }
}

/// A rough height of a node in em, to size the delimiters around it.
fn height_em(node: &MathNode) -> f32 {
    match node {
        MathNode::Row(nodes) => nodes.iter().map(height_em).fold(LINE_HEIGHT_EM, f32::max),
        MathNode::Frac { num, den } => height_em(num) + height_em(den) + RAISE_EM,
        MathNode::Sqrt(base) | MathNode::Root { base, .. } => height_em(base) + RAISE_EM,
        MathNode::Scripts { base, sub, sup } => {
            height_em(base) + if sub.is_some() || sup.is_some() { RAISE_EM } else { 0.0 }
        }
        MathNode::UnderOver { base, under, over, .. } => {
            let base_height = match base.as_ref() {
                MathNode::Operator { text, .. } if text.chars().count() == 1 && !under.is_none() => NARY_HEIGHT_EM,
                other => height_em(other),
            };
            base_height + [under, over].iter().filter(|part| part.is_some()).count() as f32 * RAISE_EM * 2.0
        }
        MathNode::Table { rows, .. } => rows
            .iter()
            .map(|row| row.iter().map(height_em).fold(LINE_HEIGHT_EM, f32::max))
            .sum(),
        MathNode::Operator { text, .. } if text.chars().next().is_some_and(|ch| UNDER_OVER_OPERATORS.contains(&ch)) => {
            NARY_HEIGHT_EM
        }
        _ => LINE_HEIGHT_EM,
    }
}

/// Writes the plain text of `node` to `out` (see [`plain_text`]).
fn write_plain(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Ident { text, .. }
        | MathNode::Number(text)
        | MathNode::Operator { text, .. }
        | MathNode::Text(text) => out.push_str(text),
        MathNode::Space(_) => out.push(' '),
        MathNode::Row(nodes) => nodes.iter().for_each(|node| write_plain(node, out)),
        MathNode::Frac { num, den } => {
            write_plain(num, out);
            out.push('/');
            write_plain(den, out);
        }
        MathNode::Sqrt(base) => {
            out.push('√');
            write_plain(base, out);
        }
        MathNode::Root { base, index } => {
            write_plain(index, out);
            out.push('√');
            write_plain(base, out);
        }
        MathNode::Scripts { base, sub, sup } => {
            write_plain(base, out);
            for part in [sub, sup].into_iter().flatten() {
                write_plain(part, out);
            }
        }
        MathNode::UnderOver { base, under, over, .. } => {
            write_plain(base, out);
            for part in [under, over].into_iter().flatten() {
                write_plain(part, out);
            }
        }
        MathNode::Table { rows, .. } => {
            for (i, row) in rows.iter().enumerate() {
                if i > 0 {
                    out.push_str("; ");
                }
                row.iter().for_each(|cell| write_plain(cell, out));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::docx_omml_for_tests as write;

    /// The formula of OMML written by kakemono's own Word output.
    fn read(xml: &str) -> MathNode {
        let doc = format!("<root xmlns:m=\"{MATH_NS}\" xmlns:w=\"{WORD_NS}\">{xml}</root>",);
        let parsed = roxmltree::Document::parse(&doc).unwrap();
        let node = parsed.root_element().first_element_child().unwrap();
        formula(node)
    }

    fn ident(text: &str) -> MathNode {
        MathNode::Ident {
            text: text.into(),
            upright: false,
        }
    }

    /// A formula written as Office Math by the Word output reads back as
    /// the same tree.
    #[test]
    fn round_trip_through_the_word_output() {
        let node = MathNode::Frac {
            num: Box::new(MathNode::Row(vec![
                ident("a"),
                operator("+"),
                MathNode::Number("2".into()),
            ])),
            den: Box::new(MathNode::Sqrt(Box::new(ident("x")))),
        };
        assert_eq!(read(&write(&node)), node);
        let scripts = MathNode::Scripts {
            base: Box::new(ident("x")),
            sub: Some(Box::new(MathNode::Number("1".into()))),
            sup: Some(Box::new(MathNode::Number("2".into()))),
        };
        assert_eq!(read(&write(&scripts)), scripts);
    }

    /// A sum with limits is an operator with limits under and over it, then
    /// its operand.
    #[test]
    fn nary_sum() {
        let node = read(
            "<m:oMath><m:nary><m:naryPr><m:chr m:val=\"∑\"/></m:naryPr><m:sub><m:r><m:t>i</m:t></m:r></m:sub>\
<m:sup><m:r><m:t>n</m:t></m:r></m:sup><m:e><m:r><m:t>x</m:t></m:r></m:e></m:nary></m:oMath>",
        );
        let MathNode::Row(nodes) = node else { panic!("a row") };
        assert!(matches!(
            &nodes[0],
            MathNode::UnderOver {
                under: Some(_),
                over: Some(_),
                ..
            }
        ));
        assert_eq!(nodes[1], ident("x"));
    }

    /// Numbers stay whole, function names are upright identifiers, and the
    /// plain text keeps the symbols.
    #[test]
    fn runs_are_tokenized() {
        let node = read(
            "<m:oMath><m:func><m:fName><m:r><m:rPr><m:sty m:val=\"p\"/></m:rPr><m:t>sin</m:t></m:r></m:fName>\
<m:e><m:r><m:t>x</m:t></m:r></m:e></m:func><m:r><m:t>=0.5</m:t></m:r></m:oMath>",
        );
        assert_eq!(
            node,
            MathNode::Row(vec![
                MathNode::Ident {
                    text: "sin".into(),
                    upright: true
                },
                ident("x"),
                operator("="),
                MathNode::Number("0.5".into()),
            ])
        );
        assert_eq!(plain_text(&node), "sinx=0.5");
    }

    /// Delimiters around a fraction grow; around a letter they do not.
    #[test]
    fn delimiters_grow_with_their_content() {
        let tall = read(
            "<m:oMath><m:d><m:e><m:f><m:num><m:r><m:t>1</m:t></m:r></m:num><m:den><m:r><m:t>2</m:t></m:r></m:den>\
</m:f></m:e></m:d></m:oMath>",
        );
        let MathNode::Row(nodes) = tall else { panic!("a row") };
        assert!(matches!(&nodes[0], MathNode::Operator { height_em: Some(_), .. }));
        let flat = read("<m:oMath><m:d><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d></m:oMath>");
        assert_eq!(flat, MathNode::Row(vec![operator("("), ident("x"), operator(")")]));
    }
}
