//! The shared strings of a workbook (`xl/sharedStrings.xml`): the text of
//! every string cell, by index, with the styles of its rich-text runs.

use crate::model::Span;
use crate::model::style::{BOLD, ITALIC, SUB, SUP};
use crate::word::docx::parse;
use anyhow::{Context, Result};
use roxmltree::Node;

use super::workbook::is_sml;

/// The shared strings of `xml`, in order: each string as its runs. A plain
/// string is one unstyled span; a rich string keeps bold, italic,
/// superscript and subscript. The phonetic reading of East Asian text
/// (furigana, `rPh`) is left out.
///
/// # Errors
/// When `xml` is not well-formed.
pub(crate) fn read(xml: &str) -> Result<Vec<Vec<Span>>> {
    let doc = parse(xml).context("reading the shared strings")?;
    Ok(doc
        .root_element()
        .children()
        .filter(|node| is_sml(*node, "si"))
        .map(string_item)
        .collect())
}

/// The runs of one string item (`si`): its `t`, or its `r` runs.
pub(crate) fn string_item(item: Node<'_, '_>) -> Vec<Span> {
    let mut spans = Vec::new();
    for node in item.children() {
        if is_sml(node, "t") {
            push(&mut spans, text(node), 0);
        } else if is_sml(node, "r") {
            let style = node.children().find(|props| is_sml(*props, "rPr")).map_or(0, run_style);
            let run_text: String = node.children().filter(|part| is_sml(*part, "t")).map(text).collect();
            push(&mut spans, run_text, style);
        }
    }
    spans
}

/// Adds a run, joined to the previous one when it has the same style.
fn push(spans: &mut Vec<Span>, text: String, style: u8) {
    if text.is_empty() {
        return;
    }
    match spans.last_mut() {
        Some(last) if last.style == style => last.text.push_str(&text),
        _ => spans.push(Span::styled(text, style)),
    }
}

/// The style bits of a run's properties (`rPr`).
fn run_style(props: Node<'_, '_>) -> u8 {
    let mut style = 0;
    for node in props.children().filter(Node::is_element) {
        let name = node.tag_name().name();
        let on = || !matches!(node.attribute("val"), Some("0" | "false" | "off"));
        match name {
            "b" if on() => style |= BOLD,
            "i" if on() => style |= ITALIC,
            "vertAlign" => match node.attribute("val") {
                Some("superscript") => style |= SUP,
                Some("subscript") => style |= SUB,
                _ => {}
            },
            _ => {}
        }
    }
    style
}

/// The text of a `t` element, with the `_xHHHH_` escapes of Office Open
/// XML (characters XML cannot hold, such as `_x000D_`) decoded.
pub(crate) fn text(node: Node<'_, '_>) -> String {
    unescape(node.text().unwrap_or_default())
}

/// `text` with every `_xHHHH_` escape replaced by its character (`_x005F_`
/// is the escape of `_` itself).
fn unescape(text: &str) -> String {
    /// Length of an escape: `_x`, four hexadecimal digits, `_`.
    const ESCAPE_LEN: usize = 7;
    if !text.contains("_x") {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("_x") {
        out.push_str(&rest[..at]);
        let candidate = &rest[at..];
        let decoded = candidate
            .get(..ESCAPE_LEN)
            .filter(|escape| escape.ends_with('_'))
            .and_then(|escape| u32::from_str_radix(&escape[2..6], 16).ok())
            .and_then(char::from_u32);
        match decoded {
            Some(ch) => {
                out.push(ch);
                rest = &candidate[ESCAPE_LEN..];
            }
            None => {
                out.push_str("_x");
                rest = &candidate[2..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of a string, without styles.
    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|span| span.text.as_str()).collect()
    }

    /// `body` as the items of a shared strings part.
    fn sst(body: &str) -> String {
        format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{body}</sst>"#)
    }

    #[test]
    fn furigana_is_left_out() {
        let xml = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>東京</t><rPh sb="0" eb="2"><t>トウキョウ</t></rPh></si></sst>"#;
        assert_eq!(plain(&read(xml).unwrap()[0]), "東京");
    }

    #[test]
    fn rich_runs_keep_bold() {
        let xml = sst("<si><r><rPr><b/></rPr><t>B</t></r><r><t>n</t></r></si>");
        let strings = read(&xml).unwrap();
        let runs: Vec<_> = strings[0].iter().map(|span| (span.text.as_str(), span.style)).collect();
        assert_eq!(runs, [("B", BOLD), ("n", 0)]);
    }

    #[test]
    fn runs_keep_italic_and_scripts() {
        let xml = sst(concat!(
            r#"<si><r><rPr><i/></rPr><t>x</t></r><r><rPr><vertAlign val="superscript"/></rPr><t>2</t></r>"#,
            r#"<r><rPr><vertAlign val="subscript"/></rPr><t>i</t></r><r><rPr><b val="0"/></rPr><t>.</t></r></si>"#,
        ));
        let strings = read(&xml).unwrap();
        let runs: Vec<_> = strings[0].iter().map(|span| (span.text.as_str(), span.style)).collect();
        assert_eq!(runs, [("x", ITALIC), ("2", SUP), ("i", SUB), (".", 0)]);
    }

    #[test]
    fn strings_keep_their_index() {
        let xml = sst(r#"<si><t>a</t></si><si><t/></si><si><t xml:space="preserve"> c </t></si>"#);
        let strings: Vec<String> = read(&xml).unwrap().iter().map(|spans| plain(spans)).collect();
        assert_eq!(strings, ["a", "", " c "]);
    }

    #[test]
    fn escaped_characters_are_decoded() {
        let xml = sst("<si><t>a_x000D_b_x005F_x0041_ c_x</t></si>");
        assert_eq!(plain(&read(&xml).unwrap()[0]), "a\rb_x0041_ c_x");
    }
}
