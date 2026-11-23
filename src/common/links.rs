//! The link targets kept from the inputs and written to the outputs.
//!
//! Links come from PDF annotations, Word hyperlinks and fields, and from
//! edited JSON. Only web and mail addresses, and anchors of the document,
//! are written as links: other schemes (`javascript:`, `data:`, `file:`,
//! `vbscript:`) and relative paths keep their text and lose the link.

/// URL schemes written as links, in lower case.
pub const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "ftp://"];

/// Whether `href` is a web or mail address (of one of [`LINK_SCHEMES`],
/// in any case).
pub fn is_web_link(href: &str) -> bool {
    LINK_SCHEMES.iter().any(|scheme| {
        href.get(..scheme.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(scheme))
    })
}

/// Whether `href` may be written as a link: an anchor of the document
/// (`#sec-2`) or a web or mail address.
pub fn is_safe_link(href: &str) -> bool {
    href.starts_with('#') || is_web_link(href)
}

#[cfg(test)]
mod tests {
    use super::{is_safe_link, is_web_link};

    /// Web and mail addresses are links, in any case; scripts, data, local
    /// files and relative paths are not.
    #[test]
    fn schemes() {
        for href in [
            "https://arxiv.org/abs/1",
            "HTTP://x.org",
            "mailto:a@b.org",
            "ftp://f.org/a",
        ] {
            assert!(is_web_link(href), "{href}");
        }
        for href in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            " javascript:alert(1)",
            "java\tscript:alert(1)",
            "vbscript:x",
            "data:text/html,x",
            "file:///etc/passwd",
            "other.docx",
            "",
            "http",
            "é",
        ] {
            assert!(!is_web_link(href), "{href:?}");
        }
        assert!(is_safe_link("#fig-2"));
        assert!(!is_safe_link("javascript:alert(1)"));
    }
}
