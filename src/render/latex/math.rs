//! Formulas written as LaTeX from their rebuilt structure ([`MathNode`]):
//! for the LaTeX output, and for the Markdown output (`$$ … $$`, which
//! GitHub, Obsidian, Jupyter, Pandoc and most other Markdown readers typeset
//! with MathJax or KaTeX). Only commands of LaTeX and the `amsmath`,
//! `amssymb` and `mathtools` packages are written, except `\lt` and `\gt`
//! (MathJax), which the preamble of the LaTeX output defines.

use crate::model::MathNode;
use crate::render::formula::{
    Accent, FUNCTIONS, MatrixKind, accent_of, accented_letter, is_big_operator, is_delimiter, matrix, primes,
};

/// Symbols written as a LaTeX command (or escaped) rather than as the
/// character itself: Greek letters, operators, relations, arrows,
/// delimiters, and the characters TeX or Markdown give a meaning to.
/// Characters not listed are written as they are (MathJax and KaTeX read
/// most of Unicode).
const SYMBOLS: &[(char, &str)] = &[
    // Greek, lower case.
    ('α', "\\alpha"),
    ('β', "\\beta"),
    ('γ', "\\gamma"),
    ('δ', "\\delta"),
    ('ϵ', "\\epsilon"),
    ('ε', "\\varepsilon"),
    ('ζ', "\\zeta"),
    ('η', "\\eta"),
    ('θ', "\\theta"),
    ('ϑ', "\\vartheta"),
    ('ι', "\\iota"),
    ('κ', "\\kappa"),
    ('λ', "\\lambda"),
    ('μ', "\\mu"),
    ('µ', "\\mu"),
    ('ν', "\\nu"),
    ('ξ', "\\xi"),
    ('π', "\\pi"),
    ('ϖ', "\\varpi"),
    ('ρ', "\\rho"),
    ('ϱ', "\\varrho"),
    ('σ', "\\sigma"),
    ('ς', "\\varsigma"),
    ('τ', "\\tau"),
    ('υ', "\\upsilon"),
    ('ϕ', "\\phi"),
    ('φ', "\\varphi"),
    ('χ', "\\chi"),
    ('ψ', "\\psi"),
    ('ω', "\\omega"),
    // Greek, upper case (those that differ from Latin letters).
    ('Γ', "\\Gamma"),
    ('Δ', "\\Delta"),
    ('Θ', "\\Theta"),
    ('Λ', "\\Lambda"),
    ('Ξ', "\\Xi"),
    ('Π', "\\Pi"),
    ('Σ', "\\Sigma"),
    ('Υ', "\\Upsilon"),
    ('Φ', "\\Phi"),
    ('Ψ', "\\Psi"),
    ('Ω', "\\Omega"),
    // Big operators.
    ('∑', "\\sum"),
    ('∏', "\\prod"),
    ('∐', "\\coprod"),
    ('∫', "\\int"),
    ('∬', "\\iint"),
    ('∭', "\\iiint"),
    ('∮', "\\oint"),
    ('⋃', "\\bigcup"),
    ('⋂', "\\bigcap"),
    ('⨁', "\\bigoplus"),
    ('⨂', "\\bigotimes"),
    // Binary operators.
    ('±', "\\pm"),
    ('∓', "\\mp"),
    ('×', "\\times"),
    ('÷', "\\div"),
    ('·', "\\cdot"),
    ('⋅', "\\cdot"),
    ('∗', "\\ast"),
    ('*', "\\ast"),
    ('∘', "\\circ"),
    ('∙', "\\bullet"),
    ('−', "-"),
    ('⊗', "\\otimes"),
    ('⊕', "\\oplus"),
    ('⊖', "\\ominus"),
    ('⊙', "\\odot"),
    ('∪', "\\cup"),
    ('∩', "\\cap"),
    ('∧', "\\wedge"),
    ('∨', "\\vee"),
    ('∖', "\\setminus"),
    ('†', "\\dagger"),
    ('‡', "\\ddagger"),
    ('⋆', "\\star"),
    // Relations.
    ('<', "\\lt"),
    ('>', "\\gt"),
    ('≤', "\\le"),
    ('≥', "\\ge"),
    ('⩽', "\\leqslant"),
    ('⩾', "\\geqslant"),
    ('≠', "\\ne"),
    ('≈', "\\approx"),
    ('∼', "\\sim"),
    ('~', "\\sim"),
    ('≃', "\\simeq"),
    ('≅', "\\cong"),
    ('≡', "\\equiv"),
    ('≜', "\\triangleq"),
    ('≔', "\\coloneqq"),
    ('∝', "\\propto"),
    ('≪', "\\ll"),
    ('≫', "\\gg"),
    ('≺', "\\prec"),
    ('≻', "\\succ"),
    ('⪯', "\\preceq"),
    ('⪰', "\\succeq"),
    ('∈', "\\in"),
    ('∉', "\\notin"),
    ('∋', "\\ni"),
    ('⊂', "\\subset"),
    ('⊆', "\\subseteq"),
    ('⊃', "\\supset"),
    ('⊇', "\\supseteq"),
    ('⊥', "\\perp"),
    ('∥', "\\parallel"),
    ('∣', "\\mid"),
    ('⊢', "\\vdash"),
    ('⊨', "\\models"),
    // Arrows.
    ('→', "\\to"),
    ('←', "\\leftarrow"),
    ('↔', "\\leftrightarrow"),
    ('⇒', "\\Rightarrow"),
    ('⇐', "\\Leftarrow"),
    ('⇔', "\\Leftrightarrow"),
    ('⟶', "\\longrightarrow"),
    ('⟹', "\\Longrightarrow"),
    ('⟺', "\\Longleftrightarrow"),
    ('⟷', "\\longleftrightarrow"),
    ('⟸', "\\Longleftarrow"),
    ('↦', "\\mapsto"),
    ('↑', "\\uparrow"),
    ('↓', "\\downarrow"),
    // Delimiters.
    ('{', "\\{"),
    ('}', "\\}"),
    ('⟨', "\\langle"),
    ('⟩', "\\rangle"),
    ('⌊', "\\lfloor"),
    ('⌋', "\\rfloor"),
    ('⌈', "\\lceil"),
    ('⌉', "\\rceil"),
    ('‖', "\\|"),
    ('\\', "\\backslash"),
    // Other symbols.
    ('∞', "\\infty"),
    ('∂', "\\partial"),
    ('∇', "\\nabla"),
    ('∀', "\\forall"),
    ('∃', "\\exists"),
    ('∄', "\\nexists"),
    ('¬', "\\neg"),
    ('∅', "\\emptyset"),
    ('ℓ', "\\ell"),
    ('ℏ', "\\hbar"),
    ('ℜ', "\\Re"),
    ('ℑ', "\\Im"),
    ('ℵ', "\\aleph"),
    ('℘', "\\wp"),
    ('√', "\\surd"),
    ('…', "\\ldots"),
    ('⋯', "\\cdots"),
    ('⋮', "\\vdots"),
    ('⋱', "\\ddots"),
    ('′', "'"),
    ('″', "''"),
    ('°', "^{\\circ}"),
    ('⊤', "\\top"),
    ('□', "\\square"),
    ('△', "\\triangle"),
    ('〈', "\\langle"),
    ('〉', "\\rangle"),
    ('◦', "\\circ"),
    ('–', "-"),
    ('˜', "\\sim"),
    // Pieces of tall brackets and braces, which TeX draws as one glyph: the
    // corners of a bracket as ceiling and floor signs, its middle as a bar,
    // the middle of a brace as a brace; the other brace pieces are left out.
    ('⎡', "\\lceil"),
    ('⎢', "\\vert"),
    ('⎣', "\\lfloor"),
    ('⎤', "\\rceil"),
    ('⎥', "\\vert"),
    ('⎦', "\\rfloor"),
    ('⎨', "\\{"),
    ('⎬', "\\}"),
    ('⎧', ""),
    ('⎩', ""),
    ('⎫', ""),
    ('⎭', ""),
    ('⎪', ""),
    ('#', "\\#"),
    ('%', "\\%"),
    ('&', "\\&"),
    ('_', "\\_"),
    ('$', "\\$"),
    ('^', "\\hat{}"),
];

/// The math alphabets of Unicode (block "Mathematical Alphanumeric
/// Symbols"): first code point of the capital A, and the LaTeX command of
/// the alphabet (empty for math italic, the default for letters).
const ALPHABETS: &[(u32, &str)] = &[
    (0x1D400, "\\mathbf"),
    (0x1D434, ""),
    (0x1D468, "\\boldsymbol"),
    (0x1D49C, "\\mathcal"),
    (0x1D4D0, "\\mathcal"),
    (0x1D504, "\\mathfrak"),
    (0x1D538, "\\mathbb"),
    (0x1D56C, "\\mathfrak"),
    (0x1D5A0, "\\mathsf"),
    (0x1D5D4, "\\mathsf"),
    (0x1D608, "\\mathsf"),
    (0x1D63C, "\\mathsf"),
    (0x1D670, "\\mathtt"),
];

/// The math digit alphabets: first code point of the digit 0, and the LaTeX
/// command.
const DIGIT_ALPHABETS: &[(u32, &str)] = &[
    (0x1D7CE, "\\mathbf"),
    (0x1D7D8, "\\mathbb"),
    (0x1D7E2, "\\mathsf"),
    (0x1D7EC, "\\mathsf"),
    (0x1D7F6, "\\mathtt"),
];

/// Letters of the math alphabets that Unicode had encoded before, outside
/// the block (the "holes" of the block): the letter and its alphabet.
const LETTERLIKE: &[(char, char, &str)] = &[
    ('ℎ', 'h', ""),
    ('ℬ', 'B', "\\mathcal"),
    ('ℰ', 'E', "\\mathcal"),
    ('ℱ', 'F', "\\mathcal"),
    ('ℋ', 'H', "\\mathcal"),
    ('ℐ', 'I', "\\mathcal"),
    ('ℒ', 'L', "\\mathcal"),
    ('ℳ', 'M', "\\mathcal"),
    ('ℛ', 'R', "\\mathcal"),
    ('ℯ', 'e', "\\mathcal"),
    ('ℊ', 'g', "\\mathcal"),
    ('ℴ', 'o', "\\mathcal"),
    ('ℭ', 'C', "\\mathfrak"),
    ('ℌ', 'H', "\\mathfrak"),
    ('ℨ', 'Z', "\\mathfrak"),
    ('ℂ', 'C', "\\mathbb"),
    ('ℍ', 'H', "\\mathbb"),
    ('ℕ', 'N', "\\mathbb"),
    ('ℙ', 'P', "\\mathbb"),
    ('ℚ', 'Q', "\\mathbb"),
    ('ℝ', 'R', "\\mathbb"),
    ('ℤ', 'Z', "\\mathbb"),
];

/// The Greek math alphabets of Unicode: first code point (the capital
/// Alpha), and the LaTeX command of the alphabet (empty for italic).
const GREEK_ALPHABETS: &[(u32, &str)] = &[
    (0x1D6A8, "\\boldsymbol"),
    (0x1D6E2, ""),
    (0x1D71C, "\\boldsymbol"),
    (0x1D756, "\\boldsymbol"),
    (0x1D790, "\\boldsymbol"),
];
/// The letters of a Greek math alphabet, in order: 25 capitals (with
/// `ϴ`), nabla, 25 small letters, the partial sign and 6 variants.
const GREEK_LETTERS: &str = "ΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡϴΣΤΥΦΧΨΩ∇αβγδεζηθικλμνξοπρςστυφχψω∂ϵϑϰϕϱϖ";
/// Greek letters that LaTeX has no command for, as the Latin letters
/// they are drawn as.
const GREEK_AS_LATIN: &[(char, char)] = &[
    ('Α', 'A'),
    ('Β', 'B'),
    ('Ε', 'E'),
    ('Ζ', 'Z'),
    ('Η', 'H'),
    ('Ι', 'I'),
    ('Κ', 'K'),
    ('Μ', 'M'),
    ('Ν', 'N'),
    ('Ο', 'O'),
    ('Ρ', 'P'),
    ('Τ', 'T'),
    ('Χ', 'X'),
    ('ο', 'o'),
    ('ϴ', 'Θ'),
    ('ϰ', 'κ'),
];
/// Letters in each math alphabet: 26 capitals, then 26 small letters.
const ALPHABET_LETTERS: u32 = 52;
/// Digits in each math digit alphabet.
const ALPHABET_DIGITS: u32 = 10;

/// Delimiter sizes of TeX (`\big` … `\Bigg`) with the smallest height, in
/// em, that each stands for; a delimiter shorter than all of them keeps its
/// normal size.
const DELIMITER_SIZES: &[(f32, &str)] = &[(3.0, "\\Bigg"), (2.4, "\\bigg"), (1.8, "\\Big"), (1.2, "\\big")];
/// Width of a space (em) from which it is written as `\quad`.
const QUAD_MIN_EM: f32 = 0.9;
/// Width of a space (em) from which it is written as `\;` (a thick space)
/// rather than `\,`.
const THICK_SPACE_MIN_EM: f32 = 0.25;

/// The LaTeX of a displayed formula, without the `$$` around it.
pub(crate) fn latex(node: &MathNode) -> String {
    let mut out = String::new();
    write_node(node, &mut out);
    out.trim().to_string()
}

/// The math-mode LaTeX of a character that has a command of its own (a
/// Greek letter, an operator, a letter of a math alphabet: `α` →
/// `\alpha`), or `None` when the character is written as it is. The LaTeX
/// output writes such characters of running text in math mode.
pub(crate) fn math_char(ch: char) -> Option<String> {
    if let Some((letter, command)) = math_greek(ch) {
        let letter = GREEK_AS_LATIN
            .iter()
            .find(|(greek, _)| *greek == letter)
            .map_or(letter, |(_, latin)| *latin);
        let inner = math_char(letter).unwrap_or_else(|| letter.to_string());
        return Some(match command {
            "" => inner,
            command => format!("{command}{{{inner}}}"),
        });
    }
    if let Some((letter, command)) = math_alphanumeric(ch) {
        return Some(match command {
            "" => letter.to_string(),
            command => format!("{command}{{{letter}}}"),
        });
    }
    SYMBOLS
        .iter()
        .find(|(symbol, _)| *symbol == ch)
        .map(|(_, command)| (*command).to_string())
}

/// Appends `piece` to `out`, with a space between a command name and a
/// letter that follows it (`\alpha x`, not `\alphax`); XeTeX and LuaTeX
/// read any Unicode letter as part of a command name (`\pi𝝃`).
fn push(out: &mut String, piece: &str) {
    let ends_with_command = {
        let letters = out.len() - out.trim_end_matches(|ch: char| ch.is_ascii_alphabetic()).len();
        letters > 0 && out[..out.len() - letters].ends_with('\\')
    };
    if ends_with_command && piece.starts_with(char::is_alphabetic) {
        out.push(' ');
    }
    out.push_str(piece);
}

/// Writes `node` as LaTeX.
fn write_node(node: &MathNode, out: &mut String) {
    match node {
        MathNode::Ident { text, upright } => push(out, &ident(text, *upright)),
        MathNode::Number(text) => push(out, &symbols(text)),
        MathNode::Operator { text, height_em } => {
            let size = height_em.and_then(|height| {
                DELIMITER_SIZES
                    .iter()
                    .find(|(min, _)| height >= *min)
                    .map(|(_, size)| *size)
            });
            if let Some(size) = size.filter(|_| is_delimiter(text)) {
                push(out, size);
            }
            push(out, &symbols(text));
        }
        MathNode::Text(text) => push(out, &format!("\\text{{{}}}", text_escape(text))),
        MathNode::Space(width) => push(out, &space(*width)),
        MathNode::Row(nodes) => write_row(nodes, out),
        MathNode::Frac { num, den } => push(out, &format!("\\frac{{{}}}{{{}}}", group(num), group(den))),
        MathNode::Sqrt(base) => push(out, &format!("\\sqrt{{{}}}", group(base))),
        MathNode::Root { base, index } => push(out, &format!("\\sqrt[{}]{{{}}}", group(index), group(base))),
        MathNode::Scripts { base, sub, sup } => {
            push(out, &script_base(base));
            if let Some(sub) = sub {
                push(out, &format!("_{{{}}}", group(sub)));
            }
            if let Some(sup) = sup {
                match primes(sup) {
                    Some(count) => push(out, &"'".repeat(count)),
                    None => push(out, &format!("^{{{}}}", group(sup))),
                }
            }
        }
        MathNode::UnderOver {
            base,
            under,
            over,
            accent,
        } => push(out, &under_over(base, under.as_deref(), over.as_deref(), *accent)),
        MathNode::Table { rows, align } => push(out, &table(rows, align)),
    }
}

/// The LaTeX of `node` on its own (the content of a `{…}` group).
fn group(node: &MathNode) -> String {
    let mut out = String::new();
    write_node(node, &mut out);
    out.trim().to_string()
}

/// The base of scripts: in braces when it is more than one token, so the
/// scripts attach to all of it.
fn script_base(base: &MathNode) -> String {
    let text = group(base);
    match base {
        MathNode::Row(nodes) if nodes.len() > 1 => format!("{{{text}}}"),
        MathNode::Scripts { .. } => format!("{{{text}}}"),
        _ if text.is_empty() => "{}".to_string(),
        _ => text,
    }
}

/// Limits or an accent above and below `base`: an accent command, a big
/// operator or `\lim` with its limits, or `\underset`/`\overset` for
/// anything else.
fn under_over(base: &MathNode, under: Option<&MathNode>, over: Option<&MathNode>, accent: bool) -> String {
    if accent
        && under.is_none()
        && let Some(MathNode::Operator { text, .. }) = over
        && let Some(found) = accent_of(text)
    {
        let command = accent_command(found);
        return format!("{command}{{{}}}", group(base));
    }
    let limits = |base: String| {
        let mut out = base;
        if let Some(under) = under {
            out.push_str(&format!("_{{{}}}", group(under)));
        }
        if let Some(over) = over {
            out.push_str(&format!("^{{{}}}", group(over)));
        }
        out
    };
    match base {
        MathNode::Operator { text, .. } if is_big_operator(text) => limits(format!("{}\\limits", symbols(text))),
        MathNode::Ident { text, .. } if FUNCTIONS.contains(&text.as_str()) => limits(format!("\\{text}")),
        _ => {
            let mut out = group(base);
            if let Some(over) = over {
                out = format!("\\overset{{{}}}{{{out}}}", group(over));
            }
            if let Some(under) = under {
                out = format!("\\underset{{{}}}{{{out}}}", group(under));
            }
            out
        }
    }
}

/// The LaTeX commands of the accents, in the order of [`Accent`].
const ACCENT_COMMANDS: [&str; 11] = [
    "\\hat",
    "\\grave",
    "\\acute",
    "\\breve",
    "\\check",
    "\\tilde",
    "\\bar",
    "\\overline",
    "\\dot",
    "\\ddot",
    "\\vec",
];

/// The LaTeX command of an accent.
fn accent_command(accent: Accent) -> &'static str {
    ACCENT_COMMANDS[accent as usize]
}

/// Writes the nodes of a row. A table between two tall delimiters is a
/// matrix (`bmatrix`, `pmatrix`, ...), and one after a tall opening brace
/// only is `cases`: TeX sizes their delimiters itself.
fn write_row(nodes: &[MathNode], out: &mut String) {
    let mut i = 0;
    while i < nodes.len() {
        if let Some(found) = matrix(&nodes[i..]) {
            let env = match found.kind {
                MatrixKind::Paren => "pmatrix",
                MatrixKind::Bracket => "bmatrix",
                MatrixKind::Brace => "Bmatrix",
                MatrixKind::Bar => "vmatrix",
                MatrixKind::DoubleBar => "Vmatrix",
                MatrixKind::Cases => "cases",
            };
            let body: Vec<String> = found
                .rows
                .iter()
                .map(|row| row.iter().map(group).collect::<Vec<_>>().join(" & "))
                .collect();
            push(out, &format!("\\begin{{{env}}} {} \\end{{{env}}}", body.join(" \\\\ ")));
            i += found.used;
        } else {
            write_node(&nodes[i], out);
            i += 1;
        }
    }
}

/// A multi-line display as an `array`, with its column alignments.
fn table(rows: &[Vec<MathNode>], align: &[&str]) -> String {
    let n_cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let spec: String = (0..n_cols)
        .map(|col| match align.get(col).copied() {
            Some("left") => 'l',
            Some("right") => 'r',
            _ => 'c',
        })
        .collect();
    let body: Vec<String> = rows
        .iter()
        .map(|row| row.iter().map(group).collect::<Vec<_>>().join(" & "))
        .collect();
    format!("\\begin{{array}}{{{spec}}} {} \\end{{array}}", body.join(" \\\\ "))
}

/// A horizontal space of `width` em.
fn space(width: f32) -> String {
    if width >= 2.0 * QUAD_MIN_EM {
        "\\qquad ".to_string()
    } else if width >= QUAD_MIN_EM {
        "\\quad ".to_string()
    } else if width >= THICK_SPACE_MIN_EM {
        "\\;".to_string()
    } else if width > 0.0 {
        "\\,".to_string()
    } else {
        String::new()
    }
}

/// An identifier: a function name as its command (`\sin`), another word
/// upright (`\mathrm{softmax}`), a letter upright when it is set so
/// (`\mathrm{d}`), and the letters of the math alphabets with their
/// alphabet (`\mathbf{x}`).
fn ident(text: &str, upright: bool) -> String {
    if FUNCTIONS.contains(&text) {
        return format!("\\{text}");
    }
    let letters = text.chars().count();
    let plain_word = text.chars().all(|ch| ch.is_ascii_alphabetic());
    if plain_word && (letters > 1 || upright) {
        return format!("\\mathrm{{{text}}}");
    }
    symbols(text)
}

/// The characters of `text` as LaTeX: math alphanumerics in their alphabet,
/// the [`SYMBOLS`] as commands, the rest as they are.
fn symbols(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if let Some((letter, command)) = math_alphanumeric(ch) {
            match command {
                "" => push(&mut out, &letter.to_string()),
                command => push(&mut out, &format!("{command}{{{letter}}}")),
            }
        } else if let Some((_, command)) = SYMBOLS.iter().find(|(symbol, _)| *symbol == ch) {
            push(&mut out, command);
        } else if let Some((base, accent)) = accented_letter(ch) {
            push(
                &mut out,
                &format!("{}{{{}}}", accent_command(accent), symbols(&base.to_string())),
            );
        } else {
            push(&mut out, ch.encode_utf8(&mut [0; 4]));
        }
    }
    out
}

/// The plain Greek letter and the alphabet command of a character of the
/// Greek math alphabets (`𝝃` → `ξ`, `\boldsymbol`).
fn math_greek(ch: char) -> Option<(char, &'static str)> {
    let code = u32::from(ch);
    let count = GREEK_LETTERS.chars().count() as u32;
    GREEK_ALPHABETS.iter().find_map(|(start, command)| {
        let offset = code.checked_sub(*start).filter(|&offset| offset < count)?;
        let letter = GREEK_LETTERS.chars().nth(offset as usize)?;
        Some((letter, *command))
    })
}

/// The plain letter or digit and the alphabet command of a character of
/// the math alphabets (`𝐱` → `x`, `\mathbf`), or `None` for any other
/// character.
fn math_alphanumeric(ch: char) -> Option<(char, &'static str)> {
    if let Some((_, letter, command)) = LETTERLIKE.iter().find(|(symbol, _, _)| *symbol == ch) {
        return Some((*letter, command));
    }
    let code = u32::from(ch);
    for (start, command) in ALPHABETS {
        if (*start..start + ALPHABET_LETTERS).contains(&code) {
            let offset = code - start;
            let base = if offset < 26 {
                u32::from('A')
            } else {
                u32::from('a') - 26
            };
            return char::from_u32(base + offset).map(|letter| (letter, *command));
        }
    }
    for (start, command) in DIGIT_ALPHABETS {
        if (*start..start + ALPHABET_DIGITS).contains(&code) {
            return char::from_u32(u32::from('0') + code - start).map(|digit| (digit, *command));
        }
    }
    None
}

/// Escapes text for `\text{…}`.
fn text_escape(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\backslash "),
            '{' | '}' | '#' | '$' | '%' | '&' | '_' => {
                out.push('\\');
                out.push(ch);
            }
            '^' | '~' => out.push(' '),
            other => out.push(other),
        }
    }
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
        assert_eq!(latex(&node), "\\alpha=\\frac{x_{1}^{2}}{\\sqrt{\\mathbf{y}}}");
    }

    #[test]
    fn command_is_spaced_from_a_letter() {
        let node = MathNode::Row(vec![op("∑"), ident("x"), ident("sin"), ident("θ")]);
        assert_eq!(latex(&node), "\\sum x\\sin\\theta");
        // XeTeX reads a Unicode letter as part of a command name.
        let node = MathNode::Row(vec![op("π"), ident("ж")]);
        assert_eq!(latex(&node), "\\pi ж");
    }

    #[test]
    fn greek_math_alphabets_are_commands() {
        // As Wikipedia writes them: bold italic xi, italic capital alpha.
        assert_eq!(math_char('𝝃').as_deref(), Some("\\boldsymbol{\\xi}"));
        assert_eq!(math_char('𝛢').as_deref(), Some("A"));
        assert_eq!(math_char('𝛁').as_deref(), Some("\\boldsymbol{\\nabla}"));
    }

    #[test]
    fn sum_with_limits_and_prime() {
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
        ]);
        assert_eq!(latex(&node), "\\sum\\limits_{i=1}^{n}f'");
    }

    #[test]
    fn precomposed_accents_and_bracket_pieces() {
        let node = MathNode::Row(vec![ident("ã"), op("⎡"), ident("ĝ"), op("⎦")]);
        assert_eq!(latex(&node), "\\tilde{a}\\lceil\\hat{g}\\rfloor");
    }

    #[test]
    fn matrix_and_cases() {
        let tall = |text: &str| MathNode::Operator {
            text: text.into(),
            height_em: Some(2.0),
        };
        let table = MathNode::Table {
            rows: vec![vec![ident("a"), ident("b")], vec![ident("c"), ident("d")]],
            align: vec!["center", "center"],
        };
        let node = MathNode::Row(vec![ident("A"), op("="), tall("["), table.clone(), tall("]")]);
        assert_eq!(latex(&node), "A=\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}");
        let node = MathNode::Row(vec![tall("{"), table]);
        assert_eq!(latex(&node), "\\begin{cases} a & b \\\\ c & d \\end{cases}");
    }

    #[test]
    fn accent_and_blackboard() {
        let node = MathNode::Row(vec![
            MathNode::UnderOver {
                base: Box::new(ident("x")),
                under: None,
                over: Some(Box::new(op("^"))),
                accent: true,
            },
            op("∈"),
            ident("ℝ"),
        ]);
        assert_eq!(latex(&node), "\\hat{x}\\in\\mathbb{R}");
    }
}
