//! Styled text runs ([`Span`]) as LaTeX running text: special characters
//! escaped, the ligatures TeX would make broken, bold, italic, monospaced,
//! raised and lowered text, links, the symbols of math fonts in math mode,
//! and the formulas inside the text as `$…$`.

use super::math::{latex, math_char};
use crate::model::{BOLD, ITALIC, MONO, SUB, SUP, Span};
use crate::render::inline_math::{Piece, has_math, split};
use crate::render::markdown::inline::merge;
use crate::render::notes::{Segment, segments};
use std::cell::RefCell;
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

/// Prefix of the `\label` of an anchor of the document, so that the labels
/// cannot clash with those of packages.
const LABEL_PREFIX: &str = "k:";

/// Characters of running text written as a command or escaped: those TeX
/// gives a meaning to, typographic punctuation (as the TeX input that makes
/// it), ligatures (as their letters) and invisible or special spaces.
const TEXT_SYMBOLS: &[(char, &str)] = &[
    ('\\', "\\textbackslash{}"),
    ('{', "\\{"),
    ('}', "\\}"),
    ('#', "\\#"),
    ('$', "\\$"),
    ('%', "\\%"),
    ('&', "\\&"),
    ('_', "\\_"),
    ('~', "\\textasciitilde{}"),
    ('^', "\\textasciicircum{}"),
    ('<', "\\textless{}"),
    ('>', "\\textgreater{}"),
    ('|', "\\textbar{}"),
    ('\u{a0}', "~"),
    ('\u{ad}', "\\-"),
    ('–', "--"),
    ('—', "---"),
    ('‘', "`"),
    ('’', "'"),
    ('“', "``"),
    ('”', "''"),
    ('…', "\\ldots{}"),
    ('•', "\\textbullet{}"),
    ('†', "\\dag{}"),
    ('‡', "\\ddag{}"),
    ('ﬀ', "ff"),
    ('ﬁ', "fi"),
    ('ﬂ', "fl"),
    ('ﬃ', "ffi"),
    ('ﬄ', "ffl"),
    ('ﬅ', "st"),
    ('ﬆ', "st"),
    ('\u{2002}', "\\enskip{}"),
    ('\u{2003}', "\\quad{}"),
    ('\u{2009}', "\\,"),
    ('\u{202f}', "\\,"),
    ('\u{200b}', ""),
    ('\u{200c}', ""),
    ('\u{200d}', ""),
    ('\u{2060}', ""),
    ('\u{feff}', ""),
];

/// Symbols of running text that have a math command but that the formulas
/// (and the Markdown output) write as they are: the command, in math mode.
const TEXT_MATH: &[(char, &str)] = &[
    ('∆', "\\Delta"),
    ('≲', "\\lesssim"),
    ('≳', "\\gtrsim"),
    ('⪅', "\\lessapprox"),
    ('⪆', "\\gtrapprox"),
];

/// Pairs of characters that TeX joins into one glyph (`--` is a dash,
/// ` `` ` a quotation mark, `!``  an inverted exclamation mark, `,,` a low
/// quotation mark in T1): an empty group goes between them.
const LIGATURE_PAIRS: &[(char, char)] = &[('-', '-'), ('`', '`'), ('\'', '\''), ('!', '`'), ('?', '`'), (',', ',')];

/// The highest character of the Latin blocks (Latin-1 Supplement and Latin
/// Extended-A) that the text fonts of every TeX engine have: they are
/// written as they are. Other characters go to math mode when a math font
/// has them, or are written as they are (see [`super::unknown_chars`]).
pub(crate) const LAST_LATIN: char = '\u{17f}';

/// Characters of the Latin blocks that pdfTeX's T1 fonts lack.
pub(crate) const LATIN_GAPS: &[char] = &['ĸ', 'Ŀ', 'ŀ', 'ŉ', 'Ŧ', 'ŧ', 'ſ'];

/// Writes spans as LaTeX running text.
pub(crate) struct Text<'a> {
    /// The anchors of the document that get a `\label`: a link to any
    /// other anchor stays text.
    pub(crate) anchors: &'a HashSet<String>,
    /// Write the formulas inside the text as `$…$` (see
    /// [`inline_math`](crate::render::inline_math)).
    pub(crate) math: bool,
    /// While a table is written: the texts of its added notes, whose
    /// cells hold only their marks (LaTeX loses a footnote in a table).
    pub(crate) table_notes: RefCell<Option<Vec<String>>>,
}

impl Text<'_> {
    /// The spans as LaTeX running text (trimmed), with the formulas inside
    /// them as `$…$` when [`Text::math`].
    pub(crate) fn spans(&self, spans: &[Span]) -> String {
        if !self.math || !has_math(spans) {
            return self.runs(spans).trim().to_string();
        }
        let mut out = String::new();
        for piece in split(spans) {
            match piece {
                Piece::Text(spans) => out.push_str(&self.runs(&spans)),
                Piece::Math(node, _, _) => {
                    let tex = latex(&node).replace(['\n', '\r'], " ");
                    if !tex.trim().is_empty() {
                        out.push_str(&format!("${}$", tex.trim()));
                    }
                }
            }
        }
        out.trim().to_string()
    }

    /// Text spans as LaTeX (not trimmed), an added note as a
    /// `\footnote` (LaTeX numbers them).
    fn runs(&self, spans: &[Span]) -> String {
        let mut out = String::new();
        for segment in segments(spans) {
            match segment {
                Segment::Text(spans) => out.push_str(&self.text_runs(spans)),
                Segment::Note(call) => {
                    let note = self.spans(call.note.as_deref().unwrap_or_default());
                    match self.table_notes.borrow_mut().as_mut() {
                        Some(notes) => {
                            notes.push(note);
                            out.push_str("\\footnotemark{}");
                        }
                        None => out.push_str(&format!("\\footnote{{{note}}}")),
                    }
                }
            }
        }
        out
    }

    /// Text spans without notes as LaTeX (not trimmed). Neighbouring spans
    /// of the same style and link are joined first, so that a word is not
    /// cut by commands.
    fn text_runs(&self, spans: &[Span]) -> String {
        let mut out = String::new();
        for (text, style, href) in merge(spans) {
            let body = styled(&text, style);
            if body.trim().is_empty() {
                out.push_str(&body);
                continue;
            }
            match href.as_deref() {
                Some(href) => {
                    // The spaces around the text stay outside the link.
                    let lead = &body[..body.len() - body.trim_start().len()];
                    let trail = &body[body.trim_end().len()..];
                    out.push_str(&format!("{lead}{}{trail}", self.link(href, body.trim())));
                }
                None => out.push_str(&body),
            }
        }
        out
    }

    /// `body` as a link to `href`: `\hyperref` to an anchor of the document
    /// that is written, `\href` to a web or mail address. Other targets,
    /// and addresses with an `&` (which a table cell would take for the end
    /// of the cell), leave the text alone.
    fn link(&self, href: &str, body: &str) -> String {
        if let Some(anchor) = href.strip_prefix('#') {
            return if self.anchors.contains(anchor) {
                format!("\\hyperref[{}]{{{body}}}", label(anchor))
            } else {
                body.to_string()
            };
        }
        if crate::common::links::is_web_link(href) && !href.contains('&') {
            format!("\\href{{{}}}{{{body}}}", url(href))
        } else {
            body.to_string()
        }
    }
}

/// The `\label` of the anchor `anchor`: the anchor with the characters that
/// labels cannot hold replaced.
pub(crate) fn label(anchor: &str) -> String {
    let name: String = anchor
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | ':' | '/') {
                ch
            } else {
                '-'
            }
        })
        .collect();
    format!("{LABEL_PREFIX}{name}")
}

/// The text of one run with its style, as nested text commands. The
/// spaces at its ends stay outside the commands, where a line can break.
fn styled(text: &str, style: u8) -> String {
    let inner = text.trim_matches(' ');
    if inner.is_empty() {
        return escape(text);
    }
    let lead = &text[..text.len() - text.trim_start_matches(' ').len()];
    let trail = &text[text.trim_end_matches(' ').len()..];
    let mut body = escape(inner);
    for (bit, command) in [
        (MONO, "\\texttt"),
        (ITALIC, "\\textit"),
        (BOLD, "\\textbf"),
        (SUP, "\\textsuperscript"),
        (SUB, "\\textsubscript"),
    ] {
        if style & bit != 0 {
            body = format!("{command}{{{body}}}");
        }
    }
    format!("{lead}{body}{trail}")
}

/// Escapes running text for LaTeX: the characters of [`TEXT_SYMBOLS`] as
/// their commands, line breaks as spaces, the symbols of the math fonts in
/// math mode (`\ensuremath{\alpha}`), the other characters as they are.
/// Ligatures that TeX would make from the result are broken, and a line
/// may break after the slashes of a web address.
pub(crate) fn escape(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            let escaped = escape_word(word);
            if word.contains("://") || word.starts_with("www.") {
                escaped.replace('/', "/\\allowbreak{}")
            } else {
                escaped
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escapes a run of text without spaces (see [`escape`]).
fn escape_word(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.nfc() {
        if let Some((_, command)) = TEXT_SYMBOLS.iter().find(|(symbol, _)| *symbol == ch) {
            push_text(&mut out, command);
        } else if matches!(ch, '\n' | '\r' | '\t') {
            out.push(' ');
        } else if ch.is_control() {
            // Control characters have no glyph.
        } else if ch.is_ascii() || (ch <= LAST_LATIN && !LATIN_GAPS.contains(&ch)) {
            push_text(&mut out, ch.encode_utf8(&mut [0; 4]));
        } else if let Some(command) = math_char(ch).or_else(|| {
            TEXT_MATH
                .iter()
                .find(|(symbol, _)| *symbol == ch)
                .map(|(_, command)| (*command).to_string())
        }) {
            if !command.is_empty() {
                out.push_str(&format!("\\ensuremath{{{command}}}"));
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Appends `piece` to `out`, with an empty group between them when the
/// last character of `out` and the first of `piece` form a TeX ligature.
fn push_text(out: &mut String, piece: &str) {
    if let (Some(last), Some(first)) = (out.chars().next_back(), piece.chars().next())
        && LIGATURE_PAIRS.contains(&(last, first))
    {
        out.push_str("{}");
    }
    out.push_str(piece);
}

/// A web address for `\href`: `%` and `#` escaped as hyperref reads them,
/// the characters TeX gives a meaning to (and spaces, quotes and non-ASCII
/// characters) percent-encoded, control characters dropped.
fn url(href: &str) -> String {
    let mut out = String::with_capacity(href.len());
    for ch in href.chars() {
        match ch {
            '%' => out.push_str("\\%"),
            '#' => out.push_str("\\#"),
            ch if ch.is_control() => {}
            ch if !ch.is_ascii() || "\\{}~^$ \"<>|`".contains(ch) => {
                for byte in ch.encode_utf8(&mut [0; 4]).bytes() {
                    out.push_str(&format!("\\%{byte:02X}"));
                }
            }
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, style: u8) -> Span {
        Span {
            style,
            ..Span::plain(text)
        }
    }

    #[test]
    fn special_characters_are_escaped() {
        assert_eq!(escape("50% of $x_1 & {y}"), "50\\% of \\$x\\_1 \\& \\{y\\}");
        assert_eq!(
            escape("a\\b ~ ^"),
            "a\\textbackslash{}b \\textasciitilde{} \\textasciicircum{}"
        );
        assert_eq!(escape("“quoted” – dash"), "``quoted'' -- dash");
    }

    #[test]
    fn ligatures_are_broken() {
        assert_eq!(escape("--flag"), "-{}-flag");
        assert_eq!(escape("a-–b"), "a-{}--b");
        assert_eq!(escape("''"), "'{}'");
        assert_eq!(escape("ﬁnd"), "find");
        assert_eq!(
            escape("see https://a.org/b c/d"),
            "see https:/\\allowbreak{}/\\allowbreak{}a.org/\\allowbreak{}b c/d"
        );
    }

    #[test]
    fn math_symbols_of_text_are_in_math_mode() {
        assert_eq!(escape("∆t ≳"), "\\ensuremath{\\Delta}t \\ensuremath{\\gtrsim}");
        assert_eq!(
            escape("α ≤ 𝐱"),
            "\\ensuremath{\\alpha} \\ensuremath{\\le} \\ensuremath{\\mathbf{x}}"
        );
        // Latin letters stay text, composed with their accents.
        assert_eq!(escape("Erdős Ce\u{301}line"), "Erdős Céline");
    }

    #[test]
    fn styles_and_links() {
        let anchors: HashSet<String> = ["ref 1".to_string()].into();
        let text = Text {
            anchors: &anchors,
            math: false,
            table_notes: RefCell::default(),
        };
        let mut known = span("[1]", 0);
        known.href = Some("#ref 1".into());
        let mut unknown = span(" [2]", 0);
        unknown.href = Some("#ref-2".into());
        let mut web = span(" site", BOLD);
        web.href = Some("https://example.org/a b#c%d".into());
        let mut query = span(" q", 0);
        query.href = Some("https://example.org/?a=1&b=2".into());
        assert_eq!(
            text.spans(&[known, unknown, web, query]),
            "\\hyperref[k:ref-1]{[1]} [2] \\href{https://example.org/a\\%20b\\#c\\%d}{\\textbf{site}} q"
        );
        assert_eq!(text.spans(&[span("x", SUP | ITALIC)]), "\\textsuperscript{\\textit{x}}");
    }
}
