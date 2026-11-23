//! Formulas written as Typst math from their rebuilt structure
//! ([`MathNode`]), for the Typst output.
//!
//! Typst reads Unicode in math (Greek letters, operators, the letters of the
//! math alphabets): those characters are written as they are. What differs
//! from LaTeX is the syntax: a run of letters is a variable name, so every
//! token is set apart by a space; `/` is a fraction and `(`, `[`, `{`, `,`,
//! `;` group or separate arguments, so the characters of the formula are
//! escaped; words are strings (`"where"`).

use crate::model::MathNode;
use crate::render::formula::{Accent, FUNCTIONS, MatrixKind, accent_of, accented_letter, is_delimiter, matrix, primes};

/// Characters of a formula that Typst gives a meaning to, written escaped
/// (`\(`): delimiters (which Typst would pair and stretch), argument
/// separators, and markup characters. The others (`=`, `<`, `.`) are
/// tokens of their own, apart from their neighbours, so that they form no
/// shorthand (`<=`).
const ESCAPED: &str = "()[]{},;/\\#$\"_^&'*~`@|√∛∜";

/// Characters written as another character or a Typst name: the pieces of
/// tall brackets that Typst draws as one glyph (the corners of a bracket as
/// ceiling and floor signs, its middle as a bar; the brace pieces other
/// than the middle are left out), and the backslash, which Typst reads as
/// an escape.
const REPLACED: &[(char, &str)] = &[
    ('\\', "backslash"),
    ('⎡', "⌈"),
    ('⎢', "|"),
    ('⎣', "⌊"),
    ('⎤', "⌉"),
    ('⎥', "|"),
    ('⎦', "⌋"),
    ('⎨', "{"),
    ('⎬', "}"),
    ('⎧', ""),
    ('⎩', ""),
    ('⎫', ""),
    ('⎭', ""),
    ('⎪', ""),
    ('–', "−"),
    ('-', "−"),
    ('˜', "∼"),
];

/// Width of a space (em) from which it is written as `quad`.
const QUAD_MIN_EM: f32 = 0.9;
/// Width of a space (em) from which it is written as `thick` rather than
/// `thin`.
const THICK_SPACE_MIN_EM: f32 = 0.25;

/// The Typst math of a formula, without the `$` around it.
pub(crate) fn typst(node: &MathNode) -> String {
    let mut out = String::new();
    write_node(node, &mut out);
    out.trim().to_string()
}

/// Appends a token to `out`, set apart from the one before by a space.
fn push(out: &mut String, token: &str) {
    if token.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with(' ') {
        out.push(' ');
    }
    out.push_str(token);
}

/// Writes `node` as Typst math.
fn write_node(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Ident { text, upright } => push(out, &ident(text, *upright)),
        MathNode::Number(text) => push(out, &number(text)),
        MathNode::Operator { text, height_em } => {
            let symbol = symbols(text);
            match height_em {
                Some(height) if is_delimiter(text) && !symbol.is_empty() => {
                    push(out, &format!("lr(size: #{height:.1}em, {symbol})"));
                }
                _ => push(out, &symbol),
            }
        }
        MathNode::Text(text) => push(out, &string(text)),
        MathNode::Space(width) => push(out, space(*width)),
        MathNode::Row(nodes) => write_row(nodes, out),
        MathNode::Frac { num, den } => push(out, &format!("frac({}, {})", arg(num), arg(den))),
        MathNode::Sqrt(base) => push(out, &format!("sqrt({})", arg(base))),
        MathNode::Root { base, index } => push(out, &format!("root({}, {})", arg(index), arg(base))),
        MathNode::Scripts { base, sub, sup } => push(out, &scripts(base, sub.as_deref(), sup.as_deref())),
        MathNode::UnderOver {
            base,
            under,
            over,
            accent,
        } => push(out, &under_over(base, under.as_deref(), over.as_deref(), *accent)),
        MathNode::Table { rows, align } => push(out, &table(rows, align)),
    }
}

/// The Typst math of `node` on its own (an argument of a function or the
/// content of a script); `""` when empty, since Typst refuses an empty
/// argument.
fn arg(node: &MathNode) -> String {
    let text = typst(node);
    if text.is_empty() { "\"\"".to_string() } else { text }
}

/// A base with scripts: `x_(i)^(2)` for a single token, `attach(…)` for a
/// longer base, and primes after the base (`f'`).
fn scripts(base: &MathNode, sub: Option<&MathNode>, sup: Option<&MathNode>) -> String {
    let text = arg(base);
    let single = !text.contains(' ') || matches!(base, MathNode::Text(_));
    let primes = sup.and_then(primes).map(|count| "'".repeat(count));
    let sup = sup.filter(|_| primes.is_none());
    let mut out = if single || (sub.is_none() && sup.is_none()) {
        let mut out = text;
        if let Some(sub) = sub {
            out.push_str(&format!("_({})", arg(sub)));
        }
        if let Some(sup) = sup {
            out.push_str(&format!("^({})", arg(sup)));
        }
        out
    } else {
        let mut parts = vec![text];
        if let Some(sub) = sub {
            parts.push(format!("b: {}", arg(sub)));
        }
        if let Some(sup) = sup {
            parts.push(format!("t: {}", arg(sup)));
        }
        format!("attach({})", parts.join(", "))
    };
    if let Some(primes) = primes {
        out.push_str(&primes);
    }
    out
}

/// Limits or an accent above and below `base`: an accent function, or the
/// base with `limits` (so that the limits go above and below it even
/// inside the text).
fn under_over(base: &MathNode, under: Option<&MathNode>, over: Option<&MathNode>, accent: bool) -> String {
    if accent
        && under.is_none()
        && let Some(MathNode::Operator { text, .. }) = over
        && let Some(found) = accent_of(text)
    {
        return format!("{}({})", accent_function(found), arg(base));
    }
    let mut out = format!("limits({})", arg(base));
    if let Some(under) = under {
        out.push_str(&format!("_({})", arg(under)));
    }
    if let Some(over) = over {
        out.push_str(&format!("^({})", arg(over)));
    }
    out
}

/// The Typst function that sets an accent.
fn accent_function(accent: Accent) -> &'static str {
    match accent {
        Accent::Hat => "hat",
        Accent::Grave => "grave",
        Accent::Acute => "acute",
        Accent::Breve => "breve",
        Accent::Caron => "caron",
        Accent::Tilde => "tilde",
        Accent::Bar => "macron",
        Accent::Overline => "overline",
        Accent::Dot => "dot",
        Accent::DoubleDot => "dot.double",
        Accent::Arrow => "arrow",
    }
}

/// Writes the nodes of a row. A table between two tall delimiters is a
/// matrix (`mat(delim: "[", …)`), and one after a tall opening brace only
/// is `cases`: Typst sizes their delimiters itself.
fn write_row(nodes: &[MathNode], out: &mut String) {
    let mut i = 0;
    while i < nodes.len() {
        if let Some(found) = matrix(&nodes[i..]) {
            push(out, &matrix_typst(found.kind, found.rows));
            i += found.used;
        } else {
            write_node(&nodes[i], out);
            i += 1;
        }
    }
}

/// A matrix (`mat(delim: "[", …)`) or `cases` in Typst.
fn matrix_typst(kind: MatrixKind, rows: &[Vec<MathNode>]) -> String {
    let cells = |sep: &str| -> Vec<String> {
        rows.iter()
            .map(|row| row.iter().map(arg).collect::<Vec<_>>().join(sep))
            .collect()
    };
    let delim = match kind {
        MatrixKind::Paren => "\"(\"",
        MatrixKind::Bracket => "\"[\"",
        MatrixKind::Brace => "\"{\"",
        MatrixKind::Bar => "\"|\"",
        MatrixKind::DoubleBar => "\"||\"",
        MatrixKind::Cases => return format!("cases({})", cells(" & ").join(", ")),
    };
    format!("mat(delim: {delim}, {})", cells(", ").join("; "))
}

/// A multi-line display: aligned at `&` when its columns alternate right
/// and left (as Typst aligns them, an equation per line), a matrix without
/// delimiters otherwise.
fn table(rows: &[Vec<MathNode>], align: &[&str]) -> String {
    let n_cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let aligned = n_cols > 1
        && (0..n_cols).all(|col| align.get(col).copied() == Some(if col % 2 == 0 { "right" } else { "left" }));
    if aligned {
        let lines: Vec<String> = rows
            .iter()
            .map(|row| row.iter().map(typst).collect::<Vec<_>>().join(" & "))
            .collect();
        return lines.join(" \\ ");
    }
    let body: Vec<String> = rows
        .iter()
        .map(|row| row.iter().map(arg).collect::<Vec<_>>().join(", "))
        .collect();
    let side = |name: &str| align.iter().take(n_cols).all(|value| *value == name);
    let align = if side("left") {
        "align: #left, "
    } else if side("right") {
        "align: #right, "
    } else {
        ""
    };
    format!("mat(delim: #none, {align}{})", body.join("; "))
}

/// A horizontal space of `width` em.
fn space(width: f32) -> &'static str {
    if width >= 2.0 * QUAD_MIN_EM {
        "wide"
    } else if width >= QUAD_MIN_EM {
        "quad"
    } else if width >= THICK_SPACE_MIN_EM {
        "thick"
    } else if width > 0.0 {
        "thin"
    } else {
        ""
    }
}

/// An identifier: a function name as Typst's (`sin`), another word as an
/// upright string (`"softmax"`), a letter upright when it is set so
/// (`upright(d)`), and other letters one by one.
fn ident(text: &str, upright: bool) -> String {
    if FUNCTIONS.contains(&text) {
        return text.to_string();
    }
    let letters = text.chars().count();
    let plain_word = text.chars().all(|ch| ch.is_ascii_alphabetic());
    if plain_word && letters > 1 {
        return string(text);
    }
    if plain_word && upright {
        return format!("upright({text})");
    }
    symbols(text)
}

/// A number: its digits and decimal point together, any other character
/// (a thousands separator) as a token of its own.
fn number(text: &str) -> String {
    let mut out = String::new();
    let mut digits = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || (ch == '.' && !digits.is_empty()) {
            digits.push(ch);
            continue;
        }
        push(&mut out, &digits);
        digits.clear();
        push(&mut out, &symbols(&ch.to_string()));
    }
    // A trailing point would be read as a field access.
    if digits.ends_with('.') {
        digits.pop();
        push(&mut out, &digits);
        push(&mut out, "\\.");
    } else {
        push(&mut out, &digits);
    }
    out
}

/// The characters of `text` as Typst math tokens: the [`REPLACED`]
/// characters as theirs, those Typst gives a meaning to escaped, a letter
/// with an accent as the accent over the letter (`tilde(a)`), the rest as
/// they are.
fn symbols(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if let Some((_, with)) = REPLACED.iter().find(|(symbol, _)| *symbol == ch) {
            push(&mut out, with);
        } else if ESCAPED.contains(ch) {
            push(&mut out, &format!("\\{ch}"));
        } else if ch.is_control() {
            // Control characters have no glyph.
        } else if let Some((base, accent)) = accented_letter(ch) {
            push(&mut out, &format!("{}({base})", accent_function(accent)));
        } else {
            push(&mut out, ch.encode_utf8(&mut [0; 4]));
        }
    }
    out
}

/// `text` as a Typst string (`"…"`), which math sets as upright text.
pub(crate) fn string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' | '\r' | '\t' => out.push(' '),
            ch if ch.is_control() || ch == super::text::CALL_END => {}
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
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
    fn fraction_with_greek_and_scripts() {
        let node = MathNode::Row(vec![
            ident("α"),
            op("="),
            MathNode::Frac {
                num: Box::new(MathNode::Scripts {
                    base: Box::new(ident("x")),
                    sub: Some(Box::new(MathNode::Number("1".into()))),
                    sup: Some(Box::new(MathNode::Number("2".into()))),
                }),
                den: Box::new(MathNode::Sqrt(Box::new(ident("𝐲")))),
            },
        ]);
        assert_eq!(typst(&node), "α = frac(x_(1)^(2), sqrt(𝐲))");
    }

    #[test]
    fn letters_and_words_are_apart() {
        let node = MathNode::Row(vec![
            op("∑"),
            ident("x"),
            ident("sin"),
            ident("θ"),
            ident("softmax"),
            MathNode::Text("if \"a\"".into()),
            ident("ab"),
        ]);
        assert_eq!(typst(&node), "∑ x sin θ \"softmax\" \"if \\\"a\\\"\" \"ab\"");
    }

    #[test]
    fn sum_with_limits_prime_and_long_base() {
        let node = MathNode::Row(vec![
            MathNode::UnderOver {
                base: Box::new(op("∑")),
                under: Some(Box::new(MathNode::Row(vec![
                    ident("i"),
                    op("="),
                    MathNode::Number("1".into()),
                ]))),
                over: Some(Box::new(ident("n"))),
                accent: false,
            },
            MathNode::Scripts {
                base: Box::new(ident("f")),
                sub: None,
                sup: Some(Box::new(op("′"))),
            },
            MathNode::Scripts {
                base: Box::new(MathNode::Row(vec![op("("), ident("a"), op(")")])),
                sub: Some(Box::new(ident("i"))),
                sup: None,
            },
        ]);
        assert_eq!(typst(&node), "limits(∑)_(i = 1)^(n) f' attach(\\( a \\), b: i)");
    }

    #[test]
    fn delimiters_numbers_and_accents() {
        let tall = MathNode::Operator {
            text: "(".into(),
            height_em: Some(2.0),
        };
        let node = MathNode::Row(vec![
            tall,
            MathNode::Number("1,000.5".into()),
            op("/"),
            ident("ã"),
            op("⎡"),
            MathNode::UnderOver {
                base: Box::new(ident("x")),
                under: None,
                over: Some(Box::new(op("^"))),
                accent: true,
            },
            ident("ℝ"),
            op("\\"),
        ]);
        assert_eq!(
            typst(&node),
            "lr(size: #2.0em, \\() 1 \\, 000.5 \\/ tilde(a) ⌈ hat(x) ℝ backslash"
        );
    }

    #[test]
    fn matrix_cases_and_aligned_lines() {
        let tall = |text: &str| MathNode::Operator {
            text: text.into(),
            height_em: Some(2.0),
        };
        let table = MathNode::Table {
            rows: vec![vec![ident("a"), ident("b")], vec![ident("c"), ident("d")]],
            align: vec!["center", "center"],
        };
        let node = MathNode::Row(vec![ident("A"), op("="), tall("["), table.clone(), tall("]")]);
        assert_eq!(typst(&node), "A = mat(delim: \"[\", a, b; c, d)");
        let node = MathNode::Row(vec![tall("{"), table]);
        assert_eq!(typst(&node), "cases(a & b, c & d)");
        let aligned = MathNode::Table {
            rows: vec![vec![ident("x"), ident("y")], vec![ident("z"), ident("w")]],
            align: vec!["right", "left"],
        };
        assert_eq!(typst(&aligned), "x & y \\ z & w");
    }
}
