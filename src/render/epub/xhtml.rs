//! HTML fragments of the flowing modes turned into XHTML, which EPUB
//! readers parse as XML: void elements closed, characters XML forbids
//! dropped, MathML in its namespace, the controls that need a script
//! removed, and links between the content documents of the book.

use regex::{Captures, Regex};
use std::collections::HashMap;
use std::sync::LazyLock;

/// The namespace of MathML, which an XHTML document must declare on each
/// `<math>` element.
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";

/// An HTML void element (written `<img ...>`), with its attributes.
static VOID_ELEMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(img|br|hr|wbr|col)\b([^>]*?)\s*/?>").unwrap());
/// The Colour and Copy buttons of a code listing, which work only with the
/// script of the friendly mode.
static CODE_TOOLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"<div class="code-tools">.*?</button></div>"#).unwrap());
/// An `id` attribute.
static ID_ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#" id="([^"]*)""#).unwrap());
/// A link to a place in the same document (`href="#fig-2"`).
static LOCAL_HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r##" href="#([^"]*)""##).unwrap());

/// A flowing-mode HTML fragment as well-formed XHTML.
pub(crate) fn from_html(html: &str) -> String {
    let text: String = html.chars().filter(|&ch| xml_char(ch)).collect();
    let text = CODE_TOOLS.replace_all(&text, "");
    let text = VOID_ELEMENT.replace_all(&text, "<$1$2/>");
    text.replace(" loading=\"lazy\"", "")
        .replace("<math ", &format!("<math xmlns=\"{MATHML_NS}\" "))
}

/// The values of the `id` attributes of an XHTML fragment, in order.
pub(crate) fn ids(xhtml: &str) -> Vec<String> {
    ID_ATTR.captures_iter(xhtml).map(|caps| caps[1].to_string()).collect()
}

/// Rewrites the links of the content document number `own` to places in
/// the book: a place in another document gets that document's file name
/// (`file_name(index)`) in front of its fragment; a link to a place that
/// is in no document loses its `href`, so that it points nowhere instead of
/// to a missing place.
pub(crate) fn relink(
    xhtml: &str,
    own: usize,
    place: &HashMap<String, usize>,
    file_name: impl Fn(usize) -> String,
) -> String {
    LOCAL_HREF
        .replace_all(xhtml, |caps: &Captures<'_>| match place.get(&caps[1]) {
            Some(&doc) if doc == own => caps[0].to_string(),
            Some(&doc) => format!(" href=\"{}#{}\"", file_name(doc), &caps[1]),
            None => String::new(),
        })
        .into_owned()
}

/// Whether XML 1.0 allows the character `ch` in a document.
fn xml_char(ch: char) -> bool {
    matches!(ch, '\t' | '\n' | '\r' | ' '..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn void_elements_are_closed() {
        let html = "<p>a<img src=\"x.png\" alt=\"b\" loading=\"lazy\">c<br></p>";
        assert_eq!(from_html(html), "<p>a<img src=\"x.png\" alt=\"b\"/>c<br/></p>");
    }

    #[test]
    fn forbidden_characters_are_dropped() {
        assert_eq!(from_html("a\u{1}b\u{c}c\u{FFFF}"), "abc");
    }

    #[test]
    fn math_gets_its_namespace() {
        assert_eq!(
            from_html("<math class=\"im\"><mi>x</mi></math>"),
            format!("<math xmlns=\"{MATHML_NS}\" class=\"im\"><mi>x</mi></math>")
        );
    }

    #[test]
    fn code_buttons_are_removed() {
        let html = "<div class=\"code-head\"><div class=\"code-tools\"><button>A</button>\
<button><span>B</span></button></div></div><pre>x</pre>";
        assert_eq!(from_html(html), "<div class=\"code-head\"></div><pre>x</pre>");
    }

    #[test]
    fn links_point_into_other_documents() {
        let place = HashMap::from([("a".to_string(), 0), ("b".to_string(), 1)]);
        let xhtml = "<a href=\"#a\">1</a><a href=\"#b\">2</a><a class=\"xref\" href=\"#c\">3</a>";
        assert_eq!(
            relink(xhtml, 0, &place, |doc| format!("c{doc}.xhtml")),
            "<a href=\"#a\">1</a><a href=\"c1.xhtml#b\">2</a><a class=\"xref\">3</a>"
        );
        assert_eq!(ids("<p id=\"x\">a</p><h2 id=\"y\">b</h2>"), ["x", "y"]);
    }
}
