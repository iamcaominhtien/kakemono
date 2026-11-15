//! Styled text runs ([`Span`]) as inline Markdown: emphasis, code, links,
//! and `<sup>`/`<sub>` for raised and lowered text (Markdown has no syntax
//! of its own for them).

use crate::model::{BOLD, ITALIC, MONO, SUB, SUP, Span};
use crate::render::esc;
use std::collections::HashSet;

/// Characters escaped with a backslash anywhere in running text: those that
/// start emphasis, code, links, inline HTML, table cells and (for the
/// Markdown readers that typeset TeX) inline math.
const ESCAPED: &[char] = &['\\', '`', '*', '_', '[', ']', '<', '>', '|', '$'];

/// Link schemes written as Markdown links; other targets stay text.
const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "ftp://"];

/// The style bits that change how a span is written in Markdown (colour,
/// font family and the like are dropped).
const MARKDOWN_STYLE: u8 = BOLD | ITALIC | MONO | SUP | SUB;

/// Writes spans as inline Markdown.
pub(crate) struct Inline<'a> {
    /// The anchors of the document that are written (as `<a id>`): a link
    /// to any other anchor stays text.
    pub(crate) anchors: &'a HashSet<String>,
}

impl Inline<'_> {
    /// The spans as one line of inline Markdown. Neighbouring spans of the
    /// same style and link are joined first, so that emphasis is not
    /// closed and opened again inside a word.
    pub(crate) fn spans(&self, spans: &[Span]) -> String {
        let mut out = String::new();
        for (text, style, href) in merge(spans) {
            let body = styled(&text, style);
            match href.as_deref().filter(|href| self.is_link(href)) {
                Some(href) if !body.trim().is_empty() => {
                    // The spaces around the text stay outside the link.
                    let lead = &body[..body.len() - body.trim_start().len()];
                    let trail = &body[body.trim_end().len()..];
                    out.push_str(&format!("{lead}[{}]({}){trail}", body.trim(), destination(href)));
                }
                _ => out.push_str(&body),
            }
        }
        out.trim().to_string()
    }

    /// Whether `href` is written as a link: an anchor of the document that
    /// is written, or a web or mail address.
    fn is_link(&self, href: &str) -> bool {
        match href.strip_prefix('#') {
            Some(anchor) => self.anchors.contains(anchor),
            None => LINK_SCHEMES.iter().any(|scheme| href.starts_with(scheme)),
        }
    }
}

/// Neighbouring spans with the same Markdown style and link, joined:
/// (text, style, link).
fn merge(spans: &[Span]) -> Vec<(String, u8, Option<String>)> {
    let mut runs: Vec<(String, u8, Option<String>)> = Vec::new();
    for span in spans {
        let style = span.style & MARKDOWN_STYLE;
        let text = span.text.replace(['\n', '\r', '\t'], " ");
        match runs.last_mut() {
            Some((last, last_style, last_href)) if *last_style == style && *last_href == span.href => {
                last.push_str(&text);
            }
            _ => runs.push((text, style, span.href.clone())),
        }
    }
    runs
}

/// The text of one run with its style. The spaces at its ends go outside
/// the emphasis and code marks, where Markdown expects them.
fn styled(text: &str, style: u8) -> String {
    let inner = text.trim_matches(' ');
    if inner.is_empty() {
        return text.to_string();
    }
    let lead = &text[..text.len() - text.trim_start_matches(' ').len()];
    let trail = &text[text.trim_end_matches(' ').len()..];
    let mut body = if style & MONO != 0 {
        code_span(inner)
    } else {
        escape(inner)
    };
    // CommonMark opens or closes `*` next to punctuation only when a space
    // is on the other side ("*x:*A" stays as it is): such text gets HTML tags.
    let ends_alphanumeric = inner.starts_with(char::is_alphanumeric) && inner.ends_with(char::is_alphanumeric);
    if style & ITALIC != 0 {
        body = if ends_alphanumeric {
            format!("*{body}*")
        } else {
            format!("<em>{body}</em>")
        };
    }
    if style & BOLD != 0 {
        body = if ends_alphanumeric {
            format!("**{body}**")
        } else {
            format!("<strong>{body}</strong>")
        };
    }
    if style & SUP != 0 {
        body = format!("<sup>{body}</sup>");
    }
    if style & SUB != 0 {
        body = format!("<sub>{body}</sub>");
    }
    format!("{lead}{body}{trail}")
}

/// `text` as inline code, between enough backticks that none inside closes
/// it.
fn code_span(text: &str) -> String {
    let longest = text.split(|ch| ch != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    // A space separates the fence from a backtick at either end.
    let pad = if text.starts_with('`') || text.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{text}{pad}{fence}")
}

/// Escapes the characters of running text that Markdown would read as
/// syntax (see [`ESCAPED`]).
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ESCAPED.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Escapes what would start a block at the beginning of a line of text: a
/// heading (`#`), a list item (`-`, `+`, `1.`), a quote (`>`, escaped
/// already), a rule or a setext underline (`=`).
pub(crate) fn escape_line_start(line: &str) -> String {
    if line.starts_with(['#', '-', '+', '=']) {
        return format!("\\{line}");
    }
    let digits = line.len() - line.trim_start_matches(|ch: char| ch.is_ascii_digit()).len();
    let rest = &line[digits..];
    if digits > 0 && (rest.starts_with(". ") || rest.starts_with(") ") || rest == "." || rest == ")") {
        return format!("{}\\{}", &line[..digits], rest);
    }
    line.to_string()
}

/// A link destination: in angle brackets when it holds spaces or
/// parentheses, which would end a bare one.
fn destination(href: &str) -> String {
    if href.contains([' ', '(', ')', '<', '>']) {
        format!("<{}>", href.replace('<', "%3C").replace('>', "%3E").replace(' ', "%20"))
    } else {
        href.to_string()
    }
}

/// Spans as HTML for the cells of a table written as HTML (Markdown is not
/// read inside them by every reader).
pub(crate) fn spans_html(spans: &[Span], anchors: &HashSet<String>) -> String {
    let mut out = String::new();
    for (text, style, href) in merge(spans) {
        let inner = text.trim_matches(' ');
        let lead = &text[..text.len() - text.trim_start_matches(' ').len()];
        let trail = if inner.is_empty() {
            ""
        } else {
            &text[text.trim_end_matches(' ').len()..]
        };
        let mut body = esc(if inner.is_empty() { &text } else { inner });
        for (bit, tag) in [
            (MONO, "code"),
            (ITALIC, "em"),
            (BOLD, "strong"),
            (SUP, "sup"),
            (SUB, "sub"),
        ] {
            if style & bit != 0 {
                body = format!("<{tag}>{body}</{tag}>");
            }
        }
        let inline = Inline { anchors };
        let body = match href.filter(|href| inline.is_link(href)) {
            Some(href) => format!("<a href=\"{}\">{body}</a>", esc(&href)),
            None => body,
        };
        out.push_str(&format!("{lead}{body}{trail}"));
    }
    out.trim().to_string()
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
    fn emphasis_keeps_spaces_outside() {
        let anchors = HashSet::new();
        let inline = Inline { anchors: &anchors };
        let spans = [span("a ", 0), span("bold ", BOLD), span("x_1 * y", 0)];
        assert_eq!(inline.spans(&spans), "a **bold** x\\_1 \\* y");
        let spans = [span("Bottom row:", ITALIC), span("A", 0)];
        assert_eq!(inline.spans(&spans), "<em>Bottom row:</em>A");
    }

    #[test]
    fn links_only_to_known_anchors_and_web() {
        let anchors: HashSet<String> = ["ref-1".to_string()].into();
        let inline = Inline { anchors: &anchors };
        let mut known = span("[1]", 0);
        known.href = Some("#ref-1".into());
        let mut unknown = span(" [2]", 0);
        unknown.href = Some("#ref-2".into());
        let mut web = span(" site", 0);
        web.href = Some("https://example.org/a b".into());
        assert_eq!(
            inline.spans(&[known, unknown, web]),
            "[\\[1\\]](#ref-1) \\[2\\] [site](<https://example.org/a%20b>)"
        );
    }

    #[test]
    fn code_with_backticks() {
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("`x"), "`` `x ``");
    }

    #[test]
    fn line_start_is_not_a_block() {
        assert_eq!(escape_line_start("# not a heading"), "\\# not a heading");
        assert_eq!(escape_line_start("2019. A year"), "2019\\. A year");
        assert_eq!(escape_line_start("2019 was"), "2019 was");
    }
}
