//! Formulas as Office Math (OMML), the native equations of Word: a
//! [`MathNode`] tree becomes `m:` elements that Word shows and edits as an
//! equation.

use super::xml::esc;
use crate::model::MathNode;

/// Large operators written as `m:nary` (limits placed by Word, the symbol
/// grown in display style) rather than as a plain symbol with limits.
const NARY_OPERATORS: &[char] = &[
    '∑', '∏', '∐', '∫', '∬', '∭', '∮', '∯', '∰', '⋃', '⋂', '⋁', '⋀', '⨁', '⨂', '⨀', '⨄', '⨆',
];
/// A space of at least this many em is written as em spaces; a narrower
/// one as a thin space.
const EM_SPACE_MIN_EM: f32 = 0.75;
/// A space narrower than this (in em) is left out.
const THIN_SPACE_MIN_EM: f32 = 0.1;

/// A displayed formula: `node` as an `m:oMathPara`, centred on its line by
/// Word.
pub(crate) fn display_math(node: &MathNode) -> String {
    let mut out = String::from("<m:oMathPara><m:oMath>");
    write_node(node, &mut out);
    out.push_str("</m:oMath></m:oMathPara>");
    out
}

/// A formula inside running text: an `m:oMath` in line with the runs of
/// its paragraph.
pub(crate) fn inline_math(node: &MathNode) -> String {
    let mut out = String::from("<m:oMath>");
    write_node(node, &mut out);
    out.push_str("</m:oMath>");
    out
}

/// Writes `node` as a sequence of OMML elements (OMML has no row element:
/// the children of a row follow one another).
fn write_node(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Ident { text, upright } => {
            // A name of several letters (`sin`, `max`) is upright, as in
            // MathML; Word would set it in italics.
            let upright = *upright || text.chars().count() > 1;
            run(text, upright, out);
        }
        MathNode::Number(text) | MathNode::Operator { text, .. } => run(text, false, out),
        MathNode::Text(text) => run(text, true, out),
        MathNode::Space(width) => write_space(*width, out),
        MathNode::Row(nodes) => nodes.iter().for_each(|node| write_node(node, out)),
        MathNode::Frac { num, den } => {
            out.push_str("<m:f>");
            wrap("m:num", num, out);
            wrap("m:den", den, out);
            out.push_str("</m:f>");
        }
        MathNode::Sqrt(base) => {
            out.push_str("<m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/>");
            wrap("m:e", base, out);
            out.push_str("</m:rad>");
        }
        MathNode::Root { base, index } => {
            out.push_str("<m:rad>");
            wrap("m:deg", index, out);
            wrap("m:e", base, out);
            out.push_str("</m:rad>");
        }
        MathNode::Scripts { base, sub, sup } => write_scripts(base, sub.as_deref(), sup.as_deref(), out),
        MathNode::UnderOver {
            base,
            under,
            over,
            accent,
        } => write_under_over(base, under.as_deref(), over.as_deref(), *accent, out),
        MathNode::Table { rows, align } => write_matrix(rows, align, out),
    }
}

/// Writes a math run. `upright` sets it in roman type (`m:sty p`) instead
/// of the italic Word gives letters.
fn run(text: &str, upright: bool, out: &mut String) {
    out.push_str("<m:r>");
    if upright {
        out.push_str("<m:rPr><m:sty m:val=\"p\"/></m:rPr>");
    }
    out.push_str(&format!("<m:t xml:space=\"preserve\">{}</m:t></m:r>", esc(text)));
}

/// Writes a horizontal space of `width` em with Unicode space characters.
fn write_space(width: f32, out: &mut String) {
    if width >= EM_SPACE_MIN_EM {
        let count = width.round().max(1.0) as usize;
        run(&"\u{2003}".repeat(count), true, out);
    } else if width >= THIN_SPACE_MIN_EM {
        run("\u{2009}", true, out);
    }
}

/// Writes `<tag>node</tag>`.
fn wrap(tag: &str, node: &MathNode, out: &mut String) {
    out.push_str(&format!("<{tag}>"));
    write_node(node, out);
    out.push_str(&format!("</{tag}>"));
}

/// Writes an optional `<tag>node</tag>`, or an empty `<tag/>`.
fn wrap_opt(tag: &str, node: Option<&MathNode>, out: &mut String) {
    match node {
        Some(node) => wrap(tag, node, out),
        None => out.push_str(&format!("<{tag}/>")),
    }
}

/// The symbol of a large operator (`∑`, `∫`, ...) written as `m:nary`, or
/// `None` when `node` is not one.
fn nary_symbol(node: &MathNode) -> Option<char> {
    let MathNode::Operator { text, .. } = node else {
        return None;
    };
    let mut chars = text.chars();
    let symbol = chars.next()?;
    (chars.next().is_none() && NARY_OPERATORS.contains(&symbol)).then_some(symbol)
}

/// Writes a large operator with its limits (`m:nary`), placed under and
/// over it (`undOvr`) or as scripts (`subSup`). The operand, which follows
/// the operator in the tree, stays outside: the `m:e` of the operator is
/// left empty.
fn write_nary(symbol: char, lower: Option<&MathNode>, upper: Option<&MathNode>, under_over: bool, out: &mut String) {
    let location = if under_over { "undOvr" } else { "subSup" };
    out.push_str(&format!(
        "<m:nary><m:naryPr><m:chr m:val=\"{}\"/><m:limLoc m:val=\"{location}\"/>",
        esc(&symbol.to_string())
    ));
    if lower.is_none() {
        out.push_str("<m:subHide m:val=\"1\"/>");
    }
    if upper.is_none() {
        out.push_str("<m:supHide m:val=\"1\"/>");
    }
    out.push_str("</m:naryPr>");
    wrap_opt("m:sub", lower, out);
    wrap_opt("m:sup", upper, out);
    out.push_str("<m:e/></m:nary>");
}

/// Writes a base with a subscript and/or a superscript.
fn write_scripts(base: &MathNode, sub: Option<&MathNode>, sup: Option<&MathNode>, out: &mut String) {
    if let Some(symbol) = nary_symbol(base) {
        return write_nary(symbol, sub, sup, false, out);
    }
    let (tag, parts): (&str, Vec<(&str, &MathNode)>) = match (sub, sup) {
        (Some(sub), Some(sup)) => ("m:sSubSup", vec![("m:sub", sub), ("m:sup", sup)]),
        (Some(sub), None) => ("m:sSub", vec![("m:sub", sub)]),
        (None, Some(sup)) => ("m:sSup", vec![("m:sup", sup)]),
        (None, None) => return write_node(base, out),
    };
    out.push_str(&format!("<{tag}>"));
    wrap("m:e", base, out);
    for (part, node) in parts {
        wrap(part, node, out);
    }
    out.push_str(&format!("</{tag}>"));
}

/// Writes limits or an accent above and/or below a base: a large operator
/// as `m:nary`, an accent as `m:acc`, anything else as `m:limLow` and
/// `m:limUpp`.
fn write_under_over(
    base: &MathNode,
    under: Option<&MathNode>,
    over: Option<&MathNode>,
    accent: bool,
    out: &mut String,
) {
    if let Some(symbol) = nary_symbol(base) {
        return write_nary(symbol, under, over, true, out);
    }
    if accent
        && under.is_none()
        && let Some(mark) = over.and_then(accent_mark)
    {
        out.push_str(&format!("<m:acc><m:accPr><m:chr m:val=\"{mark}\"/></m:accPr>"));
        wrap("m:e", base, out);
        out.push_str("</m:acc>");
        return;
    }
    let mut inner = String::new();
    match under {
        Some(under) => {
            inner.push_str("<m:limLow>");
            wrap("m:e", base, &mut inner);
            wrap("m:lim", under, &mut inner);
            inner.push_str("</m:limLow>");
        }
        None => write_node(base, &mut inner),
    }
    match over {
        Some(over) => {
            out.push_str(&format!("<m:limUpp><m:e>{inner}</m:e>"));
            wrap("m:lim", over, out);
            out.push_str("</m:limUpp>");
        }
        None => out.push_str(&inner),
    }
}

/// The combining character Word uses for an accent drawn as `node` (a hat,
/// a tilde, a bar, dots, an arrow), or `None` for anything else.
fn accent_mark(node: &MathNode) -> Option<&'static str> {
    let MathNode::Operator { text, .. } = node else {
        return None;
    };
    Some(match text.as_str() {
        "^" | "ˆ" | "\u{302}" => "\u{302}",
        "~" | "˜" | "\u{303}" => "\u{303}",
        "¯" | "‾" | "_" | "\u{305}" | "\u{304}" => "\u{305}",
        "˙" | "." | "\u{307}" => "\u{307}",
        "¨" | "\u{308}" => "\u{308}",
        "ˇ" | "\u{30c}" => "\u{30c}",
        "´" | "\u{301}" => "\u{301}",
        "`" | "\u{300}" => "\u{300}",
        "→" | "\u{20d7}" => "\u{20d7}",
        _ => return None,
    })
}

/// Writes the rows of a multi-line display as a matrix (`m:m`), each
/// column aligned as `align` says.
fn write_matrix(rows: &[Vec<MathNode>], align: &[&str], out: &mut String) {
    let n_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    out.push_str("<m:m><m:mPr><m:mcs>");
    for col in 0..n_cols {
        let justify = match align.get(col).copied() {
            Some("left") => "left",
            Some("right") => "right",
            _ => "center",
        };
        out.push_str(&format!(
            "<m:mc><m:mcPr><m:count m:val=\"1\"/><m:mcJc m:val=\"{justify}\"/></m:mcPr></m:mc>"
        ));
    }
    out.push_str("</m:mcs></m:mPr>");
    for row in rows {
        out.push_str("<m:mr>");
        for col in 0..n_cols {
            match row.get(col) {
                Some(cell) => wrap("m:e", cell, out),
                None => out.push_str("<m:e/>"),
            }
        }
        out.push_str("</m:mr>");
    }
    out.push_str("</m:m>");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ident(text: &str) -> MathNode {
        MathNode::Ident {
            text: text.into(),
            upright: false,
        }
    }

    fn op(text: &str) -> MathNode {
        MathNode::Operator {
            text: text.into(),
            height_em: None,
        }
    }

    #[test]
    fn fraction_and_root() {
        let node = MathNode::Frac {
            num: Box::new(ident("a")),
            den: Box::new(MathNode::Sqrt(Box::new(MathNode::Number("2".into())))),
        };
        assert_eq!(
            display_math(&node),
            "<m:oMathPara><m:oMath><m:f><m:num><m:r><m:t xml:space=\"preserve\">a</m:t></m:r></m:num>\
<m:den><m:rad><m:radPr><m:degHide m:val=\"1\"/></m:radPr><m:deg/><m:e><m:r><m:t xml:space=\"preserve\">2</m:t>\
</m:r></m:e></m:rad></m:den></m:f></m:oMath></m:oMathPara>"
        );
    }

    #[test]
    fn sum_with_limits_is_nary() {
        let node = MathNode::UnderOver {
            base: Box::new(op("∑")),
            under: Some(Box::new(ident("i"))),
            over: Some(Box::new(ident("n"))),
            accent: false,
        };
        let xml = display_math(&node);
        assert!(xml.contains("<m:nary><m:naryPr><m:chr m:val=\"∑\"/><m:limLoc m:val=\"undOvr\"/></m:naryPr>"));
        assert!(xml.contains("<m:sub><m:r>"));
        assert!(xml.contains("<m:e/></m:nary>"));
    }

    #[test]
    fn hat_is_an_accent_and_function_names_are_upright() {
        let hat = MathNode::UnderOver {
            base: Box::new(ident("x")),
            under: None,
            over: Some(Box::new(op("^"))),
            accent: true,
        };
        assert!(display_math(&hat).contains("<m:acc><m:accPr><m:chr m:val=\"\u{302}\"/></m:accPr><m:e>"));
        assert!(display_math(&ident("log")).contains("<m:sty m:val=\"p\"/>"));
        assert!(!display_math(&ident("x")).contains("<m:sty"));
    }

    #[test]
    fn scripts_and_tables() {
        let node = MathNode::Table {
            rows: vec![vec![
                MathNode::Scripts {
                    base: Box::new(ident("x")),
                    sub: Some(Box::new(MathNode::Number("1".into()))),
                    sup: None,
                },
                op("="),
            ]],
            align: vec!["right", "left"],
        };
        let xml = display_math(&node);
        assert!(xml.contains("<m:mcJc m:val=\"right\"/>"));
        assert!(xml.contains("<m:mr><m:e><m:sSub><m:e>"));
    }
}
