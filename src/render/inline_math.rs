//! Formulas inside running text ("the regret of fₜ(θₜ) ..."), found in the
//! styled spans of a paragraph and rebuilt as [`MathNode`] trees, so that
//! every reflowing output writes them as math: MathML in the HTML modes,
//! LaTeX (`$…$`) in Markdown, Office Math in Word.
//!
//! The analysis gives inline math only as spans: glyphs of a math font carry
//! the `MATH` style, raised and lowered glyphs `SUP` and `SUB`, and the
//! brackets and operators between them are often plain text. A formula is a
//! run that starts at a math-font span and goes on over scripts, other
//! math-font spans and the symbols between them (never over a word of
//! plain text). Brackets around it join it when they balance.
//!
//! An inline fraction stacked on a bar comes from the analysis already
//! read as "a/b", with brackets around compound terms (and around the
//! radicand of a radical that starts a term); a radical sign takes the atom
//! or the bracketed group after it. Stacked pieces without a bar stay
//! scripts, as the spans say.

use crate::model::{MATH, MONO, MathNode, SUB, SUP, Span};

/// Opening brackets taken into a formula from the text just before it,
/// when the formula closes them.
const OPENERS: &[char] = &['(', '[', '{', '⟨', '|', '‖', '⌊', '⌈'];
/// Closing brackets taken into a formula from the text just after it, as
/// long as the formula has opened more than it has closed.
const CLOSERS: &[char] = &[')', ']', '}', '⟩', '|', '‖', '⌋', '⌉'];
/// Characters that end a sentence or a clause: symbols between two math
/// spans that hold one of these, then a space, are text, not math ("x. Then
/// y" has words anyway, but "x; y" must stay two formulas).
const CLAUSE_ENDS: &[char] = &['.', ',', ';', ':', '?', '!'];
/// Operators that a formula may start with: the number just before one
/// ("1 − 10⁻⁸") joins the formula.
const LEADING_OPERATORS: &[char] = &[
    '=', '−', '-', '+', '×', '·', '<', '>', '≤', '≥', '≈', '∼', '/', '≜', '∝',
];
/// Function names written upright as one identifier (`sin`, `log`).
const FUNCTION_NAMES: &[&str] = &[
    "arccos", "arcsin", "arctan", "arg", "cos", "cosh", "cot", "coth", "csc", "deg", "det", "dim", "exp", "gcd", "inf",
    "ker", "lg", "lim", "liminf", "limsup", "ln", "log", "max", "min", "Pr", "sec", "sin", "sinh", "sup", "tan",
    "tanh", "argmax", "argmin", "softmax", "diag", "tr", "sign", "Var", "Cov", "KL",
];

/// A piece of running text: plain text, or a formula.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Piece {
    /// Spans written as text, with their styles.
    Text(Vec<Span>),
    /// A formula, with its text as the PDF gives it (alt text) and the
    /// spans it was read from (without the spaces at its ends), which put
    /// back in running text are read as the same formula again.
    Math(MathNode, String, Vec<Span>),
}

/// What a token is to the formula finder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A span in a math font: a formula starts or goes on here.
    Core,
    /// A raised or lowered span: a script of what comes before it.
    Script,
    /// Symbols, digits and spaces of plain text: inside a formula when a
    /// formula goes on after them.
    Glue,
    /// Words, links and code: never in a formula.
    Text,
}

/// A piece of a span, classified.
#[derive(Debug, Clone)]
struct Token {
    /// The span (its text is a part of the original span's text).
    span: Span,
    /// What it is to the formula finder.
    kind: Kind,
}

/// The pieces of `spans`: text, and the formulas found in it. The text of
/// the pieces, put together, is the text of the spans. A span that carries
/// its formula ([`Span::math`], from a Word document) is that formula as it
/// is.
pub(crate) fn split(spans: &[Span]) -> Vec<Piece> {
    if spans.iter().all(|span| span.math.is_none()) {
        return find_formulas(spans);
    }
    let mut pieces = Vec::new();
    let mut start = 0;
    for (idx, span) in spans.iter().enumerate() {
        let Some(node) = &span.math else { continue };
        if start < idx {
            pieces.extend(find_formulas(&spans[start..idx]));
        }
        pieces.push(Piece::Math((**node).clone(), span.text.clone(), vec![span.clone()]));
        start = idx + 1;
    }
    if start < spans.len() {
        pieces.extend(find_formulas(&spans[start..]));
    }
    pieces
}

/// The pieces of `spans` (none of which carries its formula): the
/// formulas are found in their styles.
fn find_formulas(spans: &[Span]) -> Vec<Piece> {
    let mut tokens = tokenize(spans);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut text: Vec<Span> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let Some(end) = formula_end(&tokens, i) else {
            if !tokens[i].span.text.is_empty() {
                text.push(tokens[i].span.clone());
            }
            i += 1;
            continue;
        };
        // Brackets opened just before the formula, and closed in it, join it;
        // so does a number before an operator that starts it.
        let mut opened = take_openers(&mut tokens, i, end, &mut text);
        if opened.is_empty() {
            opened = take_operand(&tokens, i, &mut text);
            if !opened.is_empty() {
                // "(1 − β₂)": the brackets around the operand and the formula.
                let texts = opened
                    .iter()
                    .chain(&tokens[i..end])
                    .map(|token| token.span.text.as_str());
                let closes_after = tokens
                    .get(end)
                    .is_some_and(|token| token.kind == Kind::Glue && token.span.text.starts_with(CLOSERS));
                let wanted = (-bracket_balance(texts)).max(0) as usize + usize::from(closes_after);
                let mut brackets = take_text_openers(&mut text, wanted);
                brackets.append(&mut opened);
                opened = brackets;
            }
        }
        let mut run: Vec<Token> = opened.into_iter().chain(tokens[i..end].iter().cloned()).collect();
        let rest = take_closers(&mut tokens, end, &mut run);
        // The scripts of a closing bracket taken in ("(A⁻¹)ᵀ") go with it.
        let mut next = end;
        if rest.is_none()
            && tokens
                .get(end)
                .is_some_and(|token| token.kind == Kind::Text && token.span.text.is_empty())
        {
            next = end + 1;
            while tokens.get(next).is_some_and(|token| token.kind == Kind::Script) {
                run.push(tokens[next].clone());
                next += 1;
            }
        }
        let raw: String = run.iter().map(|token| token.span.text.as_str()).collect();
        let alt = raw.trim();
        // Symbols alone ("{", "=") are no formula, and glyphs that could not
        // be read make none.
        // Neither is a number glued to a word ("C2", "B1913+16").
        let glued = alt.starts_with(|ch: char| ch.is_ascii_digit())
            && text
                .last()
                .is_some_and(|span| span.text.ends_with(char::is_alphanumeric));
        if glued || !alt.contains(char::is_alphanumeric) || alt.contains(char::REPLACEMENT_CHARACTER) {
            text.extend(run.into_iter().map(|token| token.span));
            text.extend(rest);
            i = next;
            continue;
        }
        // The spaces at the ends of the run stay in the text around it.
        let lead = &raw[..raw.len() - raw.trim_start().len()];
        if !lead.is_empty() {
            text.push(Span::plain(lead));
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(std::mem::take(&mut text)));
        }
        pieces.push(Piece::Math(build(&run), alt.to_string(), trimmed_spans(&run)));
        let trail = &raw[raw.trim_end().len()..];
        if !trail.is_empty() {
            text.push(Span::plain(trail));
        }
        if let Some(rest) = rest {
            text.push(rest);
        }
        i = next;
    }
    if !text.is_empty() {
        pieces.push(Piece::Text(text));
    }
    pieces
}

/// The spans of the tokens of a formula, without the spaces at its ends
/// (which stay in the text around it).
fn trimmed_spans(run: &[Token]) -> Vec<Span> {
    let mut spans: Vec<Span> = run.iter().map(|token| token.span.clone()).collect();
    while let Some(first) = spans.first_mut() {
        first.text = first.text.trim_start().to_string();
        if !first.text.is_empty() {
            break;
        }
        spans.remove(0);
    }
    while let Some(last) = spans.last_mut() {
        last.text = last.text.trim_end().to_string();
        if !last.text.is_empty() {
            break;
        }
        spans.pop();
    }
    spans
}

/// Whether `spans` hold a formula at all (a quick test before [`split`]).
pub(crate) fn has_math(spans: &[Span]) -> bool {
    spans.iter().any(|span| span.style & MATH != 0 || span.math.is_some())
}

/// The spans cut into tokens: plain text into words, symbol runs and
/// spaces; math-font and script spans whole.
fn tokenize(spans: &[Span]) -> Vec<Token> {
    let mut tokens = Vec::new();
    for span in spans {
        let kind = if span.href.is_some() || span.style & MONO != 0 {
            Kind::Text
        } else if span.style & (SUP | SUB) != 0 {
            Kind::Script
        } else if span.style & MATH != 0 && !is_prose(&span.text) {
            Kind::Core
        } else {
            for (part, kind) in cut_plain(&span.text) {
                tokens.push(Token {
                    span: Span {
                        text: part.to_string(),
                        ..span.clone()
                    },
                    kind,
                });
            }
            continue;
        };
        tokens.push(Token {
            span: span.clone(),
            kind,
        });
    }
    tokens
}

/// Plain text cut into words (text), and runs of digits, of other
/// symbols and of spaces, and single brackets (glue).
fn cut_plain(text: &str) -> Vec<(&str, Kind)> {
    let class = |ch: char| {
        if ch.is_alphabetic() {
            0
        } else if ch.is_whitespace() {
            1
        } else if ch.is_ascii_digit() {
            3
        } else {
            2
        }
    };
    let mut parts = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        // Brackets are tokens of their own, so that a formula can end at one.
        let bracket = |ch: char| OPENERS.contains(&ch) || CLOSERS.contains(&ch);
        let next_differs = chars
            .peek()
            .is_none_or(|&(_, next)| class(next) != class(ch) || bracket(ch) || bracket(next));
        if next_differs {
            let end = idx + ch.len_utf8();
            let kind = if class(ch) == 0 { Kind::Text } else { Kind::Glue };
            parts.push((&text[start..end], kind));
            start = end;
        }
    }
    parts
}

/// Whether the text of a math-font span reads as words rather than math
/// (two words of two letters or more, as in a theorem set in a math font).
fn is_prose(text: &str) -> bool {
    text.split_whitespace()
        .filter(|word| word.chars().filter(|ch| ch.is_alphabetic()).count() >= 2)
        .count()
        >= 2
}

/// The end (exclusive) of the formula that starts at `tokens[start]`, or
/// `None` when no formula starts there.
///
/// A formula starts at a math-font span, or at a number raised to a signed
/// power (`10⁻⁸`). It goes on over scripts and math-font spans, and over
/// glue that is followed by one of them (unless the glue ends a clause).
fn formula_end(tokens: &[Token], start: usize) -> Option<usize> {
    let token = &tokens[start];
    let starts = match token.kind {
        // A formula does not start with the punctuation of a number (".01").
        Kind::Core => !token.span.text.starts_with(['.', ',', ';', ':']),
        Kind::Glue => signed_power(tokens, start),
        _ => false,
    };
    if !starts {
        return None;
    }
    let mut end = start + 1;
    loop {
        match tokens.get(end).map(|token| token.kind) {
            Some(Kind::Core | Kind::Script) => end += 1,
            Some(Kind::Glue) => {
                let after = end
                    + tokens[end..]
                        .iter()
                        .take_while(|token| token.kind == Kind::Glue)
                        .count();
                let goes_on = match tokens.get(after).map(|token| token.kind) {
                    Some(Kind::Core) => true,
                    // "= 10⁻⁸": a power of ten after the glue; "(1 − γ)²": the
                    // power of a bracket.
                    Some(Kind::Script) => {
                        signed_power(tokens, after - 1) || tokens[after - 1].span.text.trim_end().ends_with(CLOSERS)
                    }
                    _ => false,
                };
                let open = bracket_balance(tokens[start..end].iter().map(|token| token.span.text.as_str())) > 0;
                // Inside brackets ("[0, 1)"), a comma does not end the formula.
                if !goes_on || (!open && ends_clause(&tokens[end..after])) {
                    end += glue_tail(tokens, start, end, after);
                    // The power of a number that ends the formula ("× 10⁷").
                    while tokens.get(end).is_some_and(|token| token.kind == Kind::Script)
                        && tokens[end - 1].kind == Kind::Glue
                        && tokens[end - 1].span.text.ends_with(|ch: char| ch.is_ascii_digit())
                    {
                        end += 1;
                    }
                    break;
                }
                end = after;
            }
            _ => break,
        }
    }
    Some(end)
}

/// Whether `tokens[idx]` is a number written right before a script that
/// starts with a sign: a power of ten or a value with its error bars.
fn signed_power(tokens: &[Token], idx: usize) -> bool {
    let text = tokens[idx].span.text.as_str();
    let number = !text.is_empty() && text.chars().all(|ch| ch.is_ascii_digit() || ch == '.');
    number
        && tokens
            .get(idx + 1)
            .is_some_and(|next| next.kind == Kind::Script && next.span.text.trim_start().starts_with(['−', '-', '+']))
}

/// How many of the glue tokens `tokens[end..after]` end the formula
/// `tokens[start..end]`: those up to the end of the clause that close its
/// brackets and end with a number or a bracket ("= 0.9", "≥ 1", "0, 1)",
/// the digits of a number cut by a change of font). None when that is not
/// so ("θ (see").
fn glue_tail(tokens: &[Token], start: usize, end: usize, after: usize) -> usize {
    let mut balance = bracket_balance(tokens[start..end].iter().map(|token| token.span.text.as_str()));
    let mut taken = 0;
    for (k, token) in tokens[end..after].iter().enumerate() {
        let next_is_space = tokens
            .get(end + k + 1)
            .is_none_or(|next| next.kind != Kind::Glue || next.span.text.trim().is_empty());
        let clause_end = token.span.text.contains(CLAUSE_ENDS) && next_is_space;
        if balance <= 0 && clause_end {
            break;
        }
        balance += bracket_balance(std::iter::once(token.span.text.as_str()));
        if balance < 0 {
            break;
        }
        let text = token.span.text.trim();
        let closes = text.ends_with(|ch: char| ch.is_ascii_digit() || CLOSERS.contains(&ch));
        if balance == 0 && !text.is_empty() && closes {
            taken = k + 1;
        }
    }
    taken
}

/// Takes from the end of `text` the number (and the space after it) that
/// is the left operand of a formula starting with an operator at
/// `tokens[start]`, and returns it as tokens.
fn take_operand(tokens: &[Token], start: usize, text: &mut Vec<Span>) -> Vec<Token> {
    let head = tokens[start].span.text.trim_start();
    let after_operator = head.starts_with(LEADING_OPERATORS);
    // "2.|8 × 10⁴": the formula starts in the middle of a number.
    let mid_number =
        head.starts_with(|ch: char| ch.is_ascii_digit()) && start > 0 && tokens[start - 1].span.text.ends_with('.');
    if !after_operator && !mid_number {
        return Vec::new();
    }
    let mut first = start;
    if after_operator
        && first > 0
        && tokens[first - 1].kind == Kind::Glue
        && tokens[first - 1].span.text.trim().is_empty()
    {
        first -= 1;
    }
    // The decimal point may be set in the math font ("2|.|8").
    let number_part = |token: &Token| {
        (token.kind == Kind::Glue || token.span.text == ".")
            && !token.span.text.is_empty()
            && token.span.text.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
    };
    let digits_end = first;
    while first > 0 && number_part(&tokens[first - 1]) {
        first -= 1;
    }
    let is_number = first < digits_end && tokens[first].span.text.starts_with(|ch: char| ch.is_ascii_digit());
    if !is_number {
        return Vec::new();
    }
    // Those tokens are the last spans of the text.
    let taken = &tokens[first..start];
    let matches = text.len() >= taken.len()
        && text[text.len() - taken.len()..]
            .iter()
            .zip(taken)
            .all(|(span, token)| span.text == token.span.text);
    if !matches {
        return Vec::new();
    }
    text.truncate(text.len() - taken.len());
    taken.to_vec()
}

/// Takes up to `wanted` opening brackets from the end of `text` (the last
/// span only) and returns them as tokens.
fn take_text_openers(text: &mut Vec<Span>, wanted: usize) -> Vec<Token> {
    let Some(last) = text.last_mut() else {
        return Vec::new();
    };
    let count = last
        .text
        .chars()
        .rev()
        .take_while(|ch| OPENERS.contains(ch))
        .take(wanted)
        .count();
    if count == 0 {
        return Vec::new();
    }
    let keep = last.text.char_indices().rev().nth(count - 1).map_or(0, |(idx, _)| idx);
    let mut brackets = last.clone();
    brackets.text = last.text[keep..].to_string();
    last.text.truncate(keep);
    if last.text.is_empty() {
        text.pop();
    }
    vec![Token {
        span: brackets,
        kind: Kind::Glue,
    }]
}

/// Whether glue tokens end a clause: punctuation that ends one, followed
/// by a space.
fn ends_clause(glue: &[Token]) -> bool {
    let text: String = glue.iter().map(|token| token.span.text.as_str()).collect();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if CLAUSE_ENDS.contains(&ch) && chars.peek().is_some_and(|next| next.is_whitespace()) {
            return true;
        }
    }
    false
}

/// Takes from the end of the text before the formula (`text`, whose last
/// span is `tokens[start - 1]` when that is glue) the opening brackets
/// that the formula `tokens[start..end]` closes, or that the text right
/// after it closes ("(α)" is one formula), and returns them as tokens.
fn take_openers(tokens: &mut [Token], start: usize, end: usize, text: &mut Vec<Span>) -> Vec<Token> {
    let balance = bracket_balance(tokens[start..end].iter().map(|token| token.span.text.as_str()));
    let closed_after = tokens
        .get(end)
        .filter(|token| token.kind == Kind::Glue)
        .map_or(0, |token| {
            token.span.text.chars().take_while(|ch| CLOSERS.contains(ch)).count()
        });
    let wanted = (-balance).max(0) as usize + closed_after;
    if wanted == 0 || start == 0 || tokens[start - 1].kind != Kind::Glue {
        return Vec::new();
    }
    let Some(last) = text.last_mut() else {
        return Vec::new();
    };
    let tail: String = last
        .text
        .chars()
        .rev()
        .take_while(|ch| OPENERS.contains(ch))
        .take(wanted)
        .collect();
    if tail.is_empty() {
        return Vec::new();
    }
    let keep = last.text.len() - tail.len();
    let mut opened = last.clone();
    opened.text = last.text[keep..].to_string();
    last.text.truncate(keep);
    if last.text.is_empty() {
        text.pop();
    }
    vec![Token {
        span: opened,
        kind: Kind::Glue,
    }]
}

/// Moves into `run` the closing brackets at the start of `tokens[end]`
/// that the run has opened and not closed; returns the rest of that token
/// as text (`None` when nothing is taken). `tokens[end]` is left empty
/// when it is all taken.
fn take_closers(tokens: &mut [Token], end: usize, run: &mut Vec<Token>) -> Option<Span> {
    let balance = bracket_balance(run.iter().map(|token| token.span.text.as_str()));
    let token = tokens
        .get_mut(end)
        .filter(|token| token.kind == Kind::Glue && balance > 0)?;
    let head: String = token
        .span
        .text
        .chars()
        .take_while(|ch| CLOSERS.contains(ch))
        .take(balance.unsigned_abs() as usize)
        .collect();
    if head.is_empty() {
        return None;
    }
    let mut closed = token.span.clone();
    closed.text = head.clone();
    run.push(Token {
        span: closed,
        kind: Kind::Glue,
    });
    let rest = Span {
        text: token.span.text[head.len()..].to_string(),
        ..token.span.clone()
    };
    // The rest goes to the text; the token itself is skipped.
    token.span.text.clear();
    token.kind = Kind::Text;
    Some(rest).filter(|rest| !rest.text.is_empty())
}

/// Brackets opened minus brackets closed in `texts` (`|` and `‖` count as
/// neither: they open and close alike).
fn bracket_balance<'a>(texts: impl Iterator<Item = &'a str>) -> i32 {
    texts
        .flat_map(str::chars)
        .map(|ch| match ch {
            '(' | '[' | '{' | '⟨' | '⌊' | '⌈' => 1,
            ')' | ']' | '}' | '⟩' | '⌋' | '⌉' => -1,
            _ => 0,
        })
        .sum()
}

/// The formula tree of a run of tokens: the atoms of the math-font spans
/// and of the glue in a row, with the script spans attached to the atom
/// before them.
fn build(run: &[Token]) -> MathNode {
    let mut nodes: Vec<MathNode> = Vec::new();
    // The script slot filled last (`SUP` or `SUB`), to append the next
    // script span of the same kind to it.
    let mut last_script: Option<u8> = None;
    for token in run {
        if token.kind == Kind::Script {
            let which = if token.span.style & SUP != 0 { SUP } else { SUB };
            let script = fold_radicals(atoms(&token.span.text));
            attach_script(&mut nodes, script, which, last_script == Some(which));
            last_script = Some(which);
        } else {
            nodes.extend(atoms(&token.span.text));
            last_script = None;
        }
    }
    MathNode::row(fold_radicals(nodes))
}

/// A radical sign and the atom after it, as a square root of that atom
/// (`√T`); or, when brackets follow the sign, the root of what they hold
/// (`√(1 − β₂ᵗ)`, an inline fraction's term read with brackets around its
/// radicand), the bar of the root standing for them.
fn fold_radicals(nodes: Vec<MathNode>) -> Vec<MathNode> {
    let is_op = |node: &MathNode, ch: &str| matches!(node, MathNode::Operator { text, .. } if text == ch);
    // Only a letter or a number, with its scripts: a root over more than
    // that cannot be told from the spans.
    let simple = |node: &MathNode| match node {
        MathNode::Ident { .. } | MathNode::Number(_) => true,
        MathNode::Scripts { base, .. } | MathNode::UnderOver { base, .. } => matches!(
            **base,
            MathNode::Ident { .. } | MathNode::Number(_) | MathNode::UnderOver { .. }
        ),
        _ => false,
    };
    let mut out = Vec::with_capacity(nodes.len());
    let mut i = 0;
    while i < nodes.len() {
        let radical = is_op(&nodes[i], "\u{221A}");
        if radical && let Some(close) = closing_bracket(&nodes, i + 1) {
            out.push(MathNode::Sqrt(Box::new(MathNode::row(nodes[i + 2..close].to_vec()))));
            i = close + 1;
        } else if radical && nodes.get(i + 1).is_some_and(simple) {
            out.push(MathNode::Sqrt(Box::new(nodes[i + 1].clone())));
            i += 2;
        } else {
            out.push(nodes[i].clone());
            i += 1;
        }
    }
    out
}

/// The index of the `)` that closes the `(` at `nodes[open]`, when the
/// brackets hold something and the closing one has no script; `None`
/// otherwise.
fn closing_bracket(nodes: &[MathNode], open: usize) -> Option<usize> {
    let is_op = |node: &MathNode, ch: &str| matches!(node, MathNode::Operator { text, .. } if text == ch);
    if !nodes.get(open).is_some_and(|node| is_op(node, "(")) {
        return None;
    }
    let mut depth = 0;
    for (idx, node) in nodes.iter().enumerate().skip(open) {
        if is_op(node, "(") {
            depth += 1;
        } else if is_op(node, ")") {
            depth -= 1;
            if depth == 0 {
                return (idx > open + 1).then_some(idx);
            }
        }
    }
    None
}

/// Attaches `script` as the superscript (`which` is `SUP`) or subscript of
/// the last node of `nodes`. With `append`, the script goes on the one
/// attached just before (a script split over several spans).
fn attach_script(nodes: &mut Vec<MathNode>, script: Vec<MathNode>, which: u8, append: bool) {
    if script.is_empty() {
        return;
    }
    let base = nodes.pop().unwrap_or_else(|| MathNode::Row(Vec::new()));
    let (base, mut sub, mut sup) = match base {
        MathNode::Scripts { base, sub, sup } => (base, sub, sup),
        other => (Box::new(other), None, None),
    };
    let slot = if which == SUP { &mut sup } else { &mut sub };
    match slot.take() {
        Some(existing) if append => {
            let mut row = match *existing {
                MathNode::Row(row) => row,
                other => vec![other],
            };
            row.extend(script);
            *slot = Some(Box::new(MathNode::row(row)));
        }
        Some(existing) => {
            // The slot is taken by a script of its own: this one is a new
            // script of the whole.
            let (old_sub, old_sup) = if which == SUP {
                (sub.take(), Some(existing))
            } else {
                (Some(existing), sup.take())
            };
            let whole = MathNode::Scripts {
                base,
                sub: old_sub,
                sup: old_sup,
            };
            let node = if which == SUP {
                MathNode::Scripts {
                    base: Box::new(whole),
                    sub: None,
                    sup: Some(Box::new(MathNode::row(script))),
                }
            } else {
                MathNode::Scripts {
                    base: Box::new(whole),
                    sub: Some(Box::new(MathNode::row(script))),
                    sup: None,
                }
            };
            nodes.push(node);
            return;
        }
        None => *slot = Some(Box::new(MathNode::row(script))),
    }
    nodes.push(MathNode::Scripts { base, sub, sup });
}

/// The atoms of a piece of formula text: numbers, identifiers (a function
/// name as one, other letters one by one) and operators; spaces are left
/// out.
fn atoms(text: &str) -> Vec<MathNode> {
    let chars: Vec<char> = text.chars().collect();
    let mut nodes = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
        } else if ch.is_ascii_digit() {
            let end = number_end(&chars, i);
            nodes.push(MathNode::Number(chars[i..end].iter().collect()));
            i = end;
        } else if is_combining_mark(ch) {
            // An accent drawn over the atom before it (θ̄, v̂).
            if let Some(base) = nodes.pop() {
                nodes.push(MathNode::UnderOver {
                    base: Box::new(base),
                    under: None,
                    over: Some(Box::new(MathNode::Operator {
                        text: spacing_accent(ch).to_string(),
                        height_em: None,
                    })),
                    accent: true,
                });
            }
            i += 1;
        } else if ch.is_alphabetic() {
            let end = i + chars[i..].iter().take_while(|ch| ch.is_ascii_alphabetic()).count();
            let word: String = chars[i..end].iter().collect();
            if end > i + 1 && FUNCTION_NAMES.contains(&word.as_str()) {
                nodes.push(MathNode::Ident {
                    text: word,
                    upright: true,
                });
                i = end;
            } else {
                nodes.push(MathNode::Ident {
                    text: ch.to_string(),
                    upright: false,
                });
                i += 1;
            }
        } else {
            let (text, len) = operator(&chars[i..]);
            nodes.push(MathNode::Operator { text, height_em: None });
            i += len;
        }
    }
    nodes
}

/// Whether `ch` is a combining diacritical mark (U+0300 to U+036F), an
/// accent over the character before it.
fn is_combining_mark(ch: char) -> bool {
    ('\u{300}'..='\u{36F}').contains(&ch)
}

/// The spacing form of a combining accent (`¯` for U+0304), which math
/// fonts draw over a base; other marks stay as they are.
fn spacing_accent(mark: char) -> char {
    match mark {
        '\u{300}' => '`',
        '\u{301}' => '´',
        '\u{302}' => '^',
        '\u{303}' => '~',
        '\u{304}' | '\u{305}' => '¯',
        '\u{306}' => '˘',
        '\u{307}' => '˙',
        '\u{308}' => '¨',
        '\u{30C}' => 'ˇ',
        other => other,
    }
}

/// The end of the number that starts at `chars[start]`: digits, with a
/// decimal point between digits.
fn number_end(chars: &[char], start: usize) -> usize {
    let mut end = start;
    while end < chars.len() {
        let digit = chars[end].is_ascii_digit();
        let point = chars[end] == '.' && chars.get(end + 1).is_some_and(char::is_ascii_digit);
        if !(digit || point) {
            break;
        }
        end += 1;
    }
    end
}

/// The operator at the start of `chars` and its length: "..." as an
/// ellipsis, any other character on its own.
fn operator(chars: &[char]) -> (String, usize) {
    if chars.len() >= 3 && chars[..3].iter().all(|&ch| ch == '.') {
        return ("…".to_string(), 3);
    }
    (chars[0].to_string(), 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ITALIC;

    fn span(text: &str, style: u8) -> Span {
        Span {
            style,
            ..Span::plain(text)
        }
    }

    fn math_texts(pieces: &[Piece]) -> Vec<String> {
        pieces
            .iter()
            .filter_map(|piece| match piece {
                Piece::Math(_, alt, _) => Some(alt.clone()),
                Piece::Text(_) => None,
            })
            .collect()
    }

    fn text_of(pieces: &[Piece]) -> String {
        pieces
            .iter()
            .map(|piece| match piece {
                Piece::Math(_, alt, _) => format!("[{alt}]"),
                Piece::Text(spans) => spans.iter().map(|span| span.text.as_str()).collect(),
            })
            .collect()
    }

    const MI: u8 = MATH | ITALIC;

    #[test]
    fn radical_before_brackets_covers_them() {
        // "√(1−β₂ᵗ)/x", the radicand of an inline fraction's term in brackets.
        let spans = [
            span("\u{221A}", MATH),
            span("(1", 0),
            span("\u{2212}", MATH),
            span("\u{3B2}", MI),
            span("2", SUB),
            span(")/", 0),
            span("x", MI),
        ];
        let pieces = split(&spans);
        let [Piece::Math(node, _, _)] = pieces.as_slice() else {
            panic!("one formula: {pieces:?}")
        };
        assert_eq!(super::super::latex::math::latex(node), "\\sqrt{1-\\beta_{2}}/x");
    }

    #[test]
    fn fraction_with_a_squared_denominator_keeps_its_bracket() {
        // "∑ₜ tγᵗ < 1/((1−γ)²):", an inline fraction read with brackets.
        let spans = [
            span("series, ", 0),
            span("\u{2211}", MATH),
            span("t ", MI | SUB),
            span("t\u{3B3}", MI),
            span("t ", MI | SUP),
            span("< ", MI),
            span("1/((1", 0),
            span("\u{2212}", MATH),
            span("\u{3B3}", MI),
            span(")", 0),
            span("2", SUP),
            span("):", 0),
        ];
        assert_eq!(
            text_of(&split(&spans)),
            "series, [\u{2211}t t\u{3B3}t < 1/((1\u{2212}\u{3B3})2)]:"
        );
    }

    #[test]
    fn function_with_subscripts_and_brackets() {
        // "prediction fₜ(θₜ) and"
        let spans = [
            span("the prediction ", 0),
            span("f", MI),
            span("t", MI | SUB),
            span("(", 0),
            span("θ", MI),
            span("t", MI | SUB),
            span(") and the best", 0),
        ];
        let pieces = split(&spans);
        assert_eq!(text_of(&pieces), "the prediction [ft(θt)] and the best");
        let Piece::Math(node, _, _) = &pieces[1] else {
            panic!("a formula")
        };
        assert_eq!(crate::render::latex::math::latex(node), "f_{t}(\\theta_{t})");
    }

    #[test]
    fn opening_bracket_before_the_formula_joins_it() {
        let spans = [span("rate (", 0), span("α", MI), span(") is", 0)];
        assert_eq!(text_of(&split(&spans)), "rate [(α)] is");
    }

    #[test]
    fn words_and_clauses_end_a_formula() {
        let spans = [
            span("α", MI),
            span(" and ", 0),
            span("β", MI),
            span("; ", 0),
            span("γ", MI),
        ];
        assert_eq!(math_texts(&split(&spans)), ["α", "β", "γ"]);
        let spans = [
            span("α", MI),
            span(" = ", 0),
            span("0.9", 0),
            span(", ", 0),
            span("β", MI),
        ];
        assert_eq!(math_texts(&split(&spans)), ["α = 0.9", "β"]);
        // Inside brackets, a comma does not end the formula.
        let spans = [span("β", MI), span(" ∈ [0, 1) such", 0)];
        assert_eq!(math_texts(&split(&spans)), ["β ∈ [0, 1)"]);
    }

    #[test]
    fn footnote_marks_and_links_are_not_math() {
        let mut cite = span("[3]", 0);
        cite.href = Some("#ref-3".into());
        let spans = [span("Kingma", 0), span("∗", MATH | SUP), span(" see ", 0), cite];
        assert!(math_texts(&split(&spans)).is_empty());
    }

    #[test]
    fn number_before_an_operator_joins_the_formula() {
        let spans = [
            span("layer has 2.8 ", 0),
            span("× ", MATH),
            span("10", 0),
            span("4", SUP),
            span(" weights", 0),
        ];
        assert_eq!(text_of(&split(&spans)), "layer has [2.8 × 104] weights");
        let spans = [
            span("is (1 ", 0),
            span("− ", MATH),
            span("β", MI),
            span("2", SUB),
            span(") here", 0),
        ];
        assert_eq!(text_of(&split(&spans)), "is [(1 − β2)] here");
    }

    #[test]
    fn list_in_brackets_with_math_commas() {
        let spans = [
            span("β", MI),
            span("2 ", SUB),
            span("∈ ", MATH),
            span("[0", 0),
            span(".", MI),
            span("99", 0),
            span(", ", MI),
            span("0", 0),
            span(".", MI),
            span("9999], and log", 0),
        ];
        assert_eq!(text_of(&split(&spans)), "[β2 ∈ [0.99, 0.9999]], and log");
    }

    #[test]
    fn script_after_a_closing_bracket() {
        let spans = [
            span("so is (", 0),
            span("A", MI),
            span("−1", SUP),
            span(")", 0),
            span("⊤", SUP),
            span(" too", 0),
        ];
        assert_eq!(text_of(&split(&spans)), "so is [(A−1)⊤] too");
    }

    #[test]
    fn power_of_ten() {
        let spans = [
            span("strain of 1.0 × ", 0),
            span("10", 0),
            span("−21", SUP),
            span(". The", 0),
        ];
        assert_eq!(math_texts(&split(&spans)), ["10−21"]);
    }

    #[test]
    fn scripts_split_over_spans_join() {
        // β₁,ₜ: "1" then ",t" both lowered.
        let spans = [span("β", MI), span("1", SUB), span(",t", MI | SUB)];
        let pieces = split(&spans);
        let Piece::Math(node, _, _) = &pieces[0] else {
            panic!("a formula")
        };
        assert_eq!(crate::render::latex::math::latex(node), "\\beta_{1,t}");
    }
}
