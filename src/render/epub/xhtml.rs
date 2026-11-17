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
/// A link to another site (`href="https://..."`), with its address.
static WEB_HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#" href="([a-z]+:[^"]*)""#).unwrap());
/// Characters a URI may not hold, as escaped in the HTML, and their
/// percent-encoding: an address read from the text of a PDF can end with
/// one (`https://www"`), which browsers accept but EPUB readers refuse.
const URI_ESCAPES: [(&str, &str); 7] = [
    ("&quot;", "%22"),
    ("&lt;", "%3C"),
    ("&gt;", "%3E"),
    (" ", "%20"),
    ("\\", "%5C"),
    ("^", "%5E"),
    ("`", "%60"),
];

/// A `class` attribute.
static CLASS_ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#" class="[^"]*""#).unwrap());
/// A link to a place in the same document (`href="#fig-2"`).
static LOCAL_HREF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r##" href="#([^"]*)""##).unwrap());

/// A flowing-mode HTML fragment as well-formed XHTML.
pub(crate) fn from_html(html: &str) -> String {
    let text: String = html.chars().filter(|&ch| xml_char(ch)).collect();
    let text = CODE_TOOLS.replace_all(&text, "");
    let text = VOID_ELEMENT.replace_all(&text, "<$1$2/>");
    let text = WEB_HREF.replace_all(&text, |caps: &Captures<'_>| {
        if !valid_host(&caps[1]) {
            return String::new();
        }
        let uri = URI_ESCAPES
            .iter()
            .fold(caps[1].to_string(), |uri, (from, to)| uri.replace(from, to));
        format!(" href=\"{uri}\"")
    });
    let text = text
        .replace(" loading=\"lazy\"", "")
        .replace("<math ", &format!("<math xmlns=\"{MATHML_NS}\" "));
    unlink(&text)
}

/// The values of the `id` attributes of an XHTML fragment, in order.
pub(crate) fn ids(xhtml: &str) -> Vec<String> {
    ID_ATTR.captures_iter(xhtml).map(|caps| caps[1].to_string()).collect()
}

/// Rewrites the links of the content document number `own` to places in
/// the book: a place in another document gets that document's file name
/// (`file_name(index)`) in front of its fragment; a link to a place that
/// is in no document becomes plain text (a `<span>`), so that it points
/// nowhere instead of to a missing place.
pub(crate) fn relink(
    xhtml: &str,
    own: usize,
    place: &HashMap<String, usize>,
    file_name: impl Fn(usize) -> String,
) -> String {
    let linked = LOCAL_HREF.replace_all(xhtml, |caps: &Captures<'_>| match place.get(&caps[1]) {
        Some(&doc) if doc == own => caps[0].to_string(),
        Some(&doc) => format!(" href=\"{}#{}\"", file_name(doc), &caps[1]),
        None => String::new(),
    });
    unlink(&linked)
}

/// Turns the links that have lost their `href` (an address that is no
/// address, a place that is in no document) into `<span>`s with the same
/// class: EPUB readers require an `href` on every link. Links do not nest,
/// so the next `</a>` closes each one.
fn unlink(xhtml: &str) -> String {
    let mut out = String::with_capacity(xhtml.len());
    let mut rest = xhtml;
    while let Some(start) = rest.find("<a") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('>').filter(|_| after.starts_with([' ', '>'])) else {
            out.push_str(&rest[..start + 2]);
            rest = after;
            continue;
        };
        let attrs = &after[..end];
        let Some(close) = after[end..].find("</a>").map(|at| end + at) else {
            out.push_str(&rest[..start + 2]);
            rest = after;
            continue;
        };
        out.push_str(&rest[..start]);
        if attrs.contains(" href=\"") {
            out.push_str(&rest[start..start + 2 + close + 4]);
        } else {
            let class = CLASS_ATTR.find(attrs).map_or("", |found| found.as_str());
            out.push_str(&format!("<span{class}>{}</span>", &after[end + 1..close]));
        }
        rest = &after[close + 4..];
    }
    out.push_str(rest);
    out
}

/// Whether the host of a web address (`https://host/...`) is made of the
/// characters a host name may have; addresses without `//` (`mailto:`)
/// have no host to check.
fn valid_host(uri: &str) -> bool {
    let Some((_, rest)) = uri.split_once("//") else {
        return true;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = host.split(':').next().unwrap_or_default();
    !host.is_empty()
        && host
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '.' | '-' | '_'))
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
    fn web_addresses_are_valid_uris() {
        let html =
            "<a href=\"https://www&quot;\" target=\"_blank\">https://www&quot;</a><a href=\"https://a.b/c d\">x</a>";
        assert_eq!(
            from_html(html),
            "<span>https://www&quot;</span><a href=\"https://a.b/c%20d\">x</a>"
        );
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
            "<a href=\"#a\">1</a><a href=\"c1.xhtml#b\">2</a><span class=\"xref\">3</span>"
        );
        assert_eq!(ids("<p id=\"x\">a</p><h2 id=\"y\">b</h2>"), ["x", "y"]);
    }
}
