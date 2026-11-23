//! Styled text runs ([`Span`]) as Typst markup: the characters Typst gives
//! a meaning to escaped, bold, italic, monospaced, raised and lowered text,
//! links, the added notes as footnotes, and the formulas inside the text as
//! `$…$`.

use super::math::{string, typst};
use crate::model::{BOLD, ITALIC, MONO, SUB, SUP, Span};
use crate::render::inline_math::{Piece, has_math, split};
use crate::render::runs::{RunMarkup, label_name, runs};
use std::collections::HashSet;
use unicode_normalization::UnicodeNormalization;

/// Link schemes written as `#link`; other targets stay text.
const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "ftp://"];

/// Prefix of the label of an anchor of the document, so that the labels
/// cannot clash with those of the template.
const LABEL_PREFIX: &str = "k-";

/// Marks the end of a function call written into the markup
/// (`#strong[…]`): a character of a private-use area, removed from the
/// text, that [`end_calls`] replaces at the end. A call followed by `.` or
/// `(` would go on (a field, a method or arguments): a `;` ends it there.
pub(crate) const CALL_END: char = '\u{e000}';

/// Characters of running text that Typst markup gives a meaning to,
/// escaped wherever they are.
const ESCAPED: &str = "\\#*_`$<>@[]~";

/// Characters that start a heading, a list or a term list at the start of
/// a line or of a content block.
const LINE_START: &str = "=-+/";

/// Writes spans as Typst markup.
pub(crate) struct Text<'a> {
    /// The anchors of the document that get a label: a link to any other
    /// anchor stays text.
    pub(crate) anchors: &'a HashSet<String>,
    /// Write the formulas inside the text as `$…$` (see
    /// [`inline_math`](crate::render::inline_math)).
    pub(crate) math: bool,
}

impl Text<'_> {
    /// The spans as Typst markup (trimmed), with the formulas inside them
    /// as `$…$` when [`Text::math`]. The result can start a line or a
    /// content block.
    pub(crate) fn spans(&self, spans: &[Span]) -> String {
        if !self.math || !has_math(spans) {
            return protect_start(self.runs(spans).trim());
        }
        let mut out = String::new();
        for piece in split(spans) {
            match piece {
                Piece::Text(spans) => out.push_str(&self.runs(&spans)),
                Piece::Math(node, _, _) => {
                    let math = typst(&node);
                    if !math.is_empty() {
                        out.push_str(&format!("${math}$"));
                    }
                }
            }
        }
        protect_start(out.trim())
    }

    /// Text spans as Typst markup (not trimmed), an added note as a
    /// `#footnote` (Typst numbers them).
    fn runs(&self, spans: &[Span]) -> String {
        runs(self, spans)
    }
}

impl RunMarkup for Text<'_> {
    fn styled(&self, text: &str, style: u8) -> String {
        styled(text, style)
    }

    fn note(&self, call: &Span) -> String {
        let note = self.spans(call.note.as_deref().unwrap_or_default());
        format!("#footnote[{note}]{CALL_END}")
    }

    /// `body` as a link to `href`: to an anchor of the document that is
    /// written (`kakemono-link`, which leaves the text alone when the
    /// anchor is not found), or to a web or mail address. Other targets
    /// leave the text alone.
    fn link(&self, href: &str, body: &str) -> String {
        if let Some(anchor) = href.strip_prefix('#') {
            return if self.anchors.contains(anchor) {
                format!("#kakemono-link(<{}>)[{body}]{CALL_END}", label(anchor))
            } else {
                body.to_string()
            };
        }
        if LINK_SCHEMES.iter().any(|scheme| href.starts_with(scheme)) {
            format!("#link({})[{body}]{CALL_END}", string(href))
        } else {
            body.to_string()
        }
    }
}

/// The label of the anchor `anchor`: the anchor with the characters that
/// labels cannot hold replaced.
pub(crate) fn label(anchor: &str) -> String {
    label_name(LABEL_PREFIX, anchor, &['-', '.', ':', '_'])
}

/// The text of one run with its style, as nested calls (`#emph[…]`). The
/// spaces at its ends stay outside the calls, where a line can break.
fn styled(text: &str, style: u8) -> String {
    let inner = text.trim_matches(' ');
    if inner.is_empty() {
        return escape(text);
    }
    let lead = &text[..text.len() - text.trim_start_matches(' ').len()];
    let trail = &text[text.trim_end_matches(' ').len()..];
    let mut body = if style & MONO != 0 {
        format!("#raw({}){CALL_END}", string(&inner.nfc().collect::<String>()))
    } else {
        escape(inner)
    };
    for (bit, function) in [(ITALIC, "emph"), (BOLD, "strong"), (SUP, "super"), (SUB, "sub")] {
        if style & bit != 0 {
            body = format!("#{function}[{}]{CALL_END}", protect_start(&body));
        }
    }
    format!("{lead}{body}{trail}")
}

/// Escapes running text for Typst markup: the characters of [`ESCAPED`],
/// the `-` and `/` that would start a dash, a minus sign, a soft hyphen or
/// a comment, line breaks as spaces; control characters are dropped.
pub(crate) fn escape(text: &str) -> String {
    let chars: Vec<char> = text.nfc().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &ch) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        match ch {
            '\n' | '\r' | '\t' => out.push(' '),
            ch if ch.is_control() || ch == CALL_END => {}
            ch if ESCAPED.contains(ch) => {
                out.push('\\');
                out.push(ch);
            }
            '-' if next.is_none_or(|next| next == '-' || next == '?' || next.is_ascii_digit()) => out.push_str("\\-"),
            '/' if matches!(next, Some('/' | '*')) => out.push_str("\\/"),
            ch => out.push(ch),
        }
    }
    out
}

/// `text` with the start of a line escaped: a character of [`LINE_START`]
/// that would make a heading or a list, or the point after a number that
/// would make a numbered list (`1.`).
pub(crate) fn protect_start(text: &str) -> String {
    if text.starts_with(|ch| LINE_START.contains(ch)) {
        return format!("\\{text}");
    }
    let digits = text.len() - text.trim_start_matches(|ch: char| ch.is_ascii_digit()).len();
    if digits > 0 && text[digits..].starts_with('.') {
        return format!("{}\\{}", &text[..digits], &text[digits..]);
    }
    text.to_string()
}

/// `typst` with each [`CALL_END`] mark replaced by `;` where the text
/// after the call would continue it, and removed elsewhere.
pub(crate) fn end_calls(typst: &str) -> String {
    let mut out = String::with_capacity(typst.len());
    let mut chars = typst.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != CALL_END {
            out.push(ch);
        } else if chars.peek().is_some_and(|next| matches!(next, '.' | '(' | '[' | ';')) {
            out.push(';');
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
        assert_eq!(
            escape("a*b_c #1 $x @y <z> [w] ~ `q` \\"),
            "a\\*b\\_c \\#1 \\$x \\@y \\<z\\> \\[w\\] \\~ \\`q\\` \\\\"
        );
        assert_eq!(escape("--flag -1 well-known a-"), "\\--flag \\-1 well-known a\\-");
        assert_eq!(escape("https://a.org /* x"), "https:\\//a.org \\/\\* x");
        assert_eq!(escape("ﬁnd\nme"), "ﬁnd me");
    }

    #[test]
    fn line_starts_are_protected() {
        assert_eq!(protect_start("= x"), "\\= x");
        assert_eq!(protect_start("12. Twelve"), "12\\. Twelve");
        assert_eq!(protect_start("12 apples"), "12 apples");
        assert_eq!(protect_start("+ - /"), "\\+ - /");
    }

    #[test]
    fn calls_end_before_a_point() {
        assert_eq!(
            end_calls(&format!("#emph[a]{CALL_END}. b #strong[c]{CALL_END} d")),
            "#emph[a];. b #strong[c] d"
        );
    }

    #[test]
    fn styles_links_and_notes() {
        let anchors: HashSet<String> = ["ref 1".to_string()].into();
        let text = Text {
            anchors: &anchors,
            math: false,
        };
        let mut known = span("[1]", 0);
        known.href = Some("#ref 1".into());
        let mut unknown = span(" [2]", 0);
        unknown.href = Some("#ref-2".into());
        let mut web = span(" site", BOLD);
        web.href = Some("https://example.org/a\"b".into());
        let mut note = span("*", 0);
        note.note = Some(vec![span("1. A note.", ITALIC)]);
        let out = end_calls(&text.spans(&[known, unknown, web, span(" code_x", MONO), note]));
        assert_eq!(
            out,
            "#kakemono-link(<k-ref-1>)[\\[1\\]] \\[2\\] #link(\"https://example.org/a\\\"b\")[#strong[site]] \
             #raw(\"code_x\")#footnote[#emph[1\\. A note.]]"
        );
        assert_eq!(end_calls(&text.spans(&[span("x", SUP | ITALIC)])), "#super[#emph[x]]");
    }
}
