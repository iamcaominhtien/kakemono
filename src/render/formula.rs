//! What the writers of formulas as text (LaTeX, Typst) read alike from a
//! rebuilt formula ([`MathNode`]): accents, primes, the delimiters that
//! stretch, the big operators, the function names set upright, and the
//! matrices and `cases` that a table between tall delimiters is. Each
//! writer only names them in its own syntax.

use crate::model::MathNode;
use unicode_normalization::UnicodeNormalization;

/// Function names that LaTeX and Typst set upright as operators of their
/// own (`\sin`, `sin`).
pub(crate) const FUNCTIONS: &[&str] = &[
    "arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "det", "dim", "exp", "gcd", "inf",
    "ker", "lg", "lim", "liminf", "limsup", "ln", "log", "max", "min", "Pr", "sec", "sin", "sinh", "sup", "tan",
    "tanh",
];

/// An accent above a base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Accent {
    /// `x̂`.
    Hat,
    /// `x̀`.
    Grave,
    /// `x́`.
    Acute,
    /// `x̆`.
    Breve,
    /// `x̌`.
    Caron,
    /// `x̃`.
    Tilde,
    /// `x̄`, a short bar.
    Bar,
    /// A bar over the whole base.
    Overline,
    /// `ẋ`.
    Dot,
    /// `ẍ`.
    DoubleDot,
    /// `x⃗`.
    Arrow,
}

/// Accents by the character the analysis reads for them.
const ACCENTS: &[(char, Accent)] = &[
    ('^', Accent::Hat),
    ('ˆ', Accent::Hat),
    ('\u{302}', Accent::Hat),
    ('\u{300}', Accent::Grave),
    ('\u{301}', Accent::Acute),
    ('\u{306}', Accent::Breve),
    ('\u{30C}', Accent::Caron),
    ('~', Accent::Tilde),
    ('˜', Accent::Tilde),
    ('\u{303}', Accent::Tilde),
    ('¯', Accent::Bar),
    ('\u{304}', Accent::Bar),
    ('‾', Accent::Overline),
    ('―', Accent::Overline),
    ('—', Accent::Overline),
    ('−', Accent::Overline),
    ('˙', Accent::Dot),
    ('·', Accent::Dot),
    ('\u{307}', Accent::Dot),
    ('¨', Accent::DoubleDot),
    ('\u{308}', Accent::DoubleDot),
    ('→', Accent::Arrow),
    ('⃗', Accent::Arrow),
    ('ˇ', Accent::Caron),
    ('˘', Accent::Breve),
];

/// The accent that the character `ch` stands for.
fn accent_of_char(ch: char) -> Option<Accent> {
    ACCENTS
        .iter()
        .find(|(accent, _)| *accent == ch)
        .map(|(_, accent)| *accent)
}

/// The accent that `text`, a single character, stands for.
pub(crate) fn accent_of(text: &str) -> Option<Accent> {
    let mut chars = text.chars();
    let (Some(ch), None) = (chars.next(), chars.next()) else {
        return None;
    };
    accent_of_char(ch)
}

/// A letter with an accent written as one character (`ã`, `ĝ`): the letter
/// and its accent, or `None` when `ch` is no such letter.
pub(crate) fn accented_letter(ch: char) -> Option<(char, Accent)> {
    let mut parts = std::iter::once(ch).nfd();
    let (Some(base), Some(mark), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    accent_of_char(mark).map(|accent| (base, accent))
}

/// The number of primes of a superscript made only of primes (`′`, `″`),
/// or `None` for any other superscript.
pub(crate) fn primes(sup: &MathNode) -> Option<usize> {
    let MathNode::Operator { text, .. } = sup else {
        return None;
    };
    let count = text.chars().map(|ch| match ch {
        '′' | '\'' => Some(1),
        '″' => Some(2),
        '‴' => Some(3),
        _ => None,
    });
    let total: Option<usize> = count.sum();
    total.filter(|&n| n > 0)
}

/// Whether `text` is a big operator (∑, ∫, ⋃, …) that takes limits.
pub(crate) fn is_big_operator(text: &str) -> bool {
    matches!(text, "∑" | "∏" | "∐" | "∫" | "∬" | "∭" | "∮" | "⋃" | "⋂" | "⨁" | "⨂")
}

/// Whether `text` is a delimiter that can be drawn taller (`\big`, `lr`).
pub(crate) fn is_delimiter(text: &str) -> bool {
    matches!(
        text,
        "(" | ")" | "[" | "]" | "{" | "}" | "|" | "‖" | "⟨" | "⟩" | "⌊" | "⌋" | "⌈" | "⌉" | "/"
    )
}

/// The delimiters around a matrix, or a `cases`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatrixKind {
    /// `( … )`.
    Paren,
    /// `[ … ]`.
    Bracket,
    /// `{ … }`.
    Brace,
    /// `| … |`.
    Bar,
    /// `‖ … ‖`.
    DoubleBar,
    /// `{` alone: cases.
    Cases,
}

/// A matrix or `cases` that starts a row of nodes: its delimiters, its
/// rows, and how many nodes of the row it takes.
pub(crate) struct Matrix<'a> {
    /// The delimiters.
    pub(crate) kind: MatrixKind,
    /// The rows of cells.
    pub(crate) rows: &'a [Vec<MathNode>],
    /// Nodes of the row it takes (the delimiters and the table).
    pub(crate) used: usize,
}

/// The matrix or `cases` that starts `nodes` (a tall opening delimiter, a
/// table and, for a matrix, the matching tall closing one); `None` when
/// `nodes` starts otherwise.
pub(crate) fn matrix(nodes: &[MathNode]) -> Option<Matrix<'_>> {
    fn tall(node: &MathNode) -> Option<&str> {
        match node {
            MathNode::Operator {
                text,
                height_em: Some(_),
            } => Some(text.as_str()),
            _ => None,
        }
    }
    let open = tall(nodes.first()?)?;
    let MathNode::Table { rows, .. } = nodes.get(1)? else {
        return None;
    };
    let close = nodes.get(2).and_then(tall);
    let (kind, used) = match (open, close) {
        ("(", Some(")")) => (MatrixKind::Paren, 3),
        ("[", Some("]")) => (MatrixKind::Bracket, 3),
        ("{", Some("}")) => (MatrixKind::Brace, 3),
        ("|", Some("|")) => (MatrixKind::Bar, 3),
        ("‖", Some("‖")) => (MatrixKind::DoubleBar, 3),
        ("{", None) => (MatrixKind::Cases, 2),
        _ => return None,
    };
    Some(Matrix { kind, rows, used })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accents_and_primes() {
        assert_eq!(accent_of("^"), Some(Accent::Hat));
        assert_eq!(accent_of("^^"), None);
        assert_eq!(accented_letter('ã'), Some(('a', Accent::Tilde)));
        assert_eq!(accented_letter('a'), None);
        let prime = |text: &str| MathNode::Operator {
            text: text.into(),
            height_em: None,
        };
        assert_eq!(primes(&prime("′″")), Some(3));
        assert_eq!(primes(&prime("x")), None);
    }
}
