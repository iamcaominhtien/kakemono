//! Styled text runs ([`Span`]) as the markup of a text format (LaTeX,
//! Typst): the runs are cut at the calls of added notes, neighbouring spans
//! of the same style and link are joined (so that a word is not cut by
//! commands), and the spaces at the ends of a link stay outside it. Each
//! format writes the styles, the links and the notes in its own syntax.

use crate::model::Span;
use crate::render::markdown::inline::merge_runs;
use crate::render::notes::{Segment, segments};

/// How a text format writes the pieces of a run.
pub(crate) trait RunMarkup {
    /// One run of text with its style (bold, italic, ...), escaped.
    fn styled(&self, text: &str, style: u8) -> String;
    /// `body` (already written) as a link to `href`, or `body` alone when
    /// the format cannot link there.
    fn link(&self, href: &str, body: &str) -> String;
    /// The call of an added note (its span) with the note.
    fn note(&self, call: &Span) -> String;
    /// The markup of a line break of the text (`\n`: a `<br>` of an EPUB
    /// book, a manual line break of a Word document), or `None` when the
    /// format sets it as a space here.
    fn line_break(&self) -> Option<&'static str> {
        None
    }
}

/// `spans` in the markup of `markup` (not trimmed).
pub(crate) fn runs(markup: &impl RunMarkup, spans: &[Span]) -> String {
    let mut out = String::new();
    for segment in segments(spans) {
        match segment {
            Segment::Text(spans) => {
                let line_break = markup.line_break();
                for (text, style, href) in merge_runs(spans, line_break.is_some()) {
                    // Each line is styled on its own, so that no command is
                    // left open across a line break.
                    let body = match line_break {
                        Some(line_break) => text
                            .split('\n')
                            .map(|line| markup.styled(line, style))
                            .collect::<Vec<_>>()
                            .join(line_break),
                        None => markup.styled(&text, style),
                    };
                    match href.as_deref().filter(|_| !body.trim().is_empty()) {
                        Some(href) => {
                            // The spaces around the text stay outside the link.
                            let lead = &body[..body.len() - body.trim_start().len()];
                            let trail = &body[body.trim_end().len()..];
                            out.push_str(&format!("{lead}{}{trail}", markup.link(href, body.trim())));
                        }
                        None => out.push_str(&body),
                    }
                }
            }
            Segment::Note(call) => out.push_str(&markup.note(call)),
        }
    }
    out
}

/// `text` without the spaces and the line breaks (`line_break`) at its
/// ends, where a line break would only add an empty line (or, in LaTeX,
/// stop with an error at the start of a paragraph).
pub(crate) fn trim_breaks<'a>(text: &'a str, line_break: &str) -> &'a str {
    let mut text = text.trim();
    while let Some(rest) = text.strip_prefix(line_break).or_else(|| text.strip_suffix(line_break)) {
        text = rest.trim();
    }
    text
}

/// The name of a label for the anchor `anchor`: `prefix`, then the anchor
/// with every character other than an ASCII letter, a digit or one of
/// `keep` replaced by `-`.
pub(crate) fn label_name(prefix: &str, anchor: &str, keep: &[char]) -> String {
    let name: String = anchor
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || keep.contains(&ch) {
                ch
            } else {
                '-'
            }
        })
        .collect();
    format!("{prefix}{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breaks_at_the_ends_are_trimmed() {
        assert_eq!(trim_breaks(" <br>a<br>b <br><br> ", "<br>"), "a<br>b");
        assert_eq!(trim_breaks("<br>", "<br>"), "");
    }
}
