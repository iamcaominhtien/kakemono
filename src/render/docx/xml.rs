//! Small pieces of WordprocessingML: escaping, runs of text and their
//! properties.

use crate::model::style::{BOLD, ITALIC, MATH, MONO, SMALL_CAPS, SUB, SUP};

/// Longest bookmark name Word accepts, in characters.
const BOOKMARK_MAX_CHARS: usize = 40;

/// Escapes `text` for XML text and double-quoted attribute values, and
/// drops the characters XML 1.0 does not allow (control characters other
/// than tab, line feed and carriage return; U+FFFE and U+FFFF).
pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(ch),
            '\u{fffe}' | '\u{ffff}' => {}
            ch if ch < ' ' => {}
            ch => out.push(ch),
        }
    }
    out
}

/// The run properties (`<w:rPr>`) of a span with the style flags `style`
/// and the colour `color`, with the character style `char_style` (a link)
/// when there is one. Empty when the run has no properties. The elements
/// follow the order the schema requires.
pub(crate) fn run_props(style: u8, color: [u8; 3], char_style: Option<&str>) -> String {
    let mut props = String::new();
    if let Some(char_style) = char_style {
        props.push_str(&format!("<w:rStyle w:val=\"{char_style}\"/>"));
    }
    if style & MONO != 0 {
        props.push_str("<w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/>");
    } else if style & MATH != 0 {
        props.push_str("<w:rFonts w:ascii=\"Cambria Math\" w:hAnsi=\"Cambria Math\"/>");
    }
    if style & BOLD != 0 {
        props.push_str("<w:b/>");
    }
    if style & ITALIC != 0 {
        props.push_str("<w:i/>");
    }
    if style & SMALL_CAPS != 0 {
        props.push_str("<w:smallCaps/>");
    }
    if color != [0; 3] && char_style.is_none() {
        props.push_str(&format!(
            "<w:color w:val=\"{:02X}{:02X}{:02X}\"/>",
            color[0], color[1], color[2]
        ));
    }
    if style & SUP != 0 {
        props.push_str("<w:vertAlign w:val=\"superscript\"/>");
    } else if style & SUB != 0 {
        props.push_str("<w:vertAlign w:val=\"subscript\"/>");
    }
    if props.is_empty() {
        props
    } else {
        format!("<w:rPr>{props}</w:rPr>")
    }
}

/// `text` as runs with the properties `props`: tabs become `<w:tab/>` and
/// line breaks `<w:br/>`. Empty for empty text.
pub(crate) fn text_runs(text: &str, props: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = format!("<w:r>{props}");
    let mut piece = String::new();
    let flush = |piece: &mut String, out: &mut String| {
        if !piece.is_empty() {
            out.push_str(&format!("<w:t xml:space=\"preserve\">{}</w:t>", esc(piece)));
            piece.clear();
        }
    };
    for ch in text.chars() {
        match ch {
            '\t' => {
                flush(&mut piece, &mut out);
                out.push_str("<w:tab/>");
            }
            '\n' => {
                flush(&mut piece, &mut out);
                out.push_str("<w:br/>");
            }
            '\r' => {}
            ch => piece.push(ch),
        }
    }
    flush(&mut piece, &mut out);
    out.push_str("</w:r>");
    out
}

/// A bookmark name for the anchor `anchor` ("sec-3.1" → "sec_3_1"): Word
/// accepts letters, digits and underscores, starting with a letter, at most
/// 40 characters.
pub(crate) fn bookmark_name(anchor: &str) -> String {
    let mut name: String = anchor
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    if !name.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
        name.insert(0, 'a');
    }
    name.truncate(BOOKMARK_MAX_CHARS);
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_drops_control_characters() {
        assert_eq!(esc("a<b & \"c\"\u{1}\u{fffe}"), "a&lt;b &amp; &quot;c&quot;");
    }

    #[test]
    fn runs_keep_tabs_breaks_and_spaces() {
        assert_eq!(
            text_runs(" a\tb\nc", "<w:rPr><w:b/></w:rPr>"),
            "<w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\"> a</w:t><w:tab/>\
<w:t xml:space=\"preserve\">b</w:t><w:br/><w:t xml:space=\"preserve\">c</w:t></w:r>"
        );
        assert_eq!(text_runs("", ""), "");
    }

    #[test]
    fn run_properties_are_in_schema_order() {
        assert_eq!(
            run_props(BOLD | ITALIC | SUP, [255, 0, 0], None),
            "<w:rPr><w:b/><w:i/><w:color w:val=\"FF0000\"/><w:vertAlign w:val=\"superscript\"/></w:rPr>"
        );
        assert_eq!(run_props(0, [0; 3], None), "");
    }

    #[test]
    fn bookmark_names_are_valid() {
        assert_eq!(bookmark_name("sec-3.1"), "sec_3_1");
        assert_eq!(bookmark_name("3-x"), "a3_x");
        assert_eq!(bookmark_name(&"x".repeat(60)).len(), 40);
    }
}
