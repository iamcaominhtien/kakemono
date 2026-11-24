//! Web pages (`.html`, `.htm`, `.xhtml`, or an `http(s)` address) as
//! input: the article of the page read into a [`Document`], as an EPUB
//! book is.
//!
//! A page is rarely well-formed XML, and holds much besides its article.
//! It is parsed as a browser parses it (html5ever), its article is found
//! and cleaned, and it is written as one XHTML document that the reader of
//! `epub` reads into the intermediate form of a Word document (`word::ir`),
//! which `word::build` turns into the document. Like a book, a page has no
//! pages: its pictures are stored in the [`Document`].
//!
//! - `decode`: the character encoding of the page.
//! - `dom`: the tree of the page, edited, then written as XHTML.
//! - `clean`: the article: clutter removed, the element that holds it,
//!   its title and authors, its pictures and links.
//! - `notes`: notes and the markup of LaTeXML (arXiv) and MediaWiki
//!   (Wikipedia), made into the semantics the reader of `epub` knows.
//!
//! The pictures of a page of the Web are fetched with it; those of a file
//! are read next to it. A rule about markup belongs in `epub/html.rs`
//! when a book can have it too, here when only a web page does.

mod clean;
mod decode;
mod dom;
mod notes;

use crate::epub::{Resources, data_url};
use crate::model::Document;
use anyhow::{Context, Result};
use clean::Base;
use dom::{Dom, Space};
use std::collections::HashMap;
use std::path::Path;
use url::Url;

/// How far into a file the start of an HTML document is looked for.
const SNIFF_BYTES: usize = 1024;
/// The name of the document of a page without a file name (in the
/// bookmark of its start).
const PAGE_NAME: &str = "index.html";
/// Most bytes of style sheets read for a page: the sheets of some pages
/// are megabytes of rules for their layout, which say nothing of the text.
const MAX_STYLE_BYTES: usize = 2 * 1024 * 1024;

/// Fetches the pictures at the given addresses: their bytes by address
/// (one that cannot be fetched is left out).
pub type FetchPictures = dyn Fn(&[String]) -> HashMap<String, Vec<u8>>;

/// Where a page comes from, which says where its pictures are.
#[derive(Debug, Clone, Copy)]
pub enum Origin<'a> {
    /// Bytes alone: only the pictures inside the page (`data:` URLs) are
    /// shown.
    Bytes,
    /// A file: its pictures and style sheets are files next to it.
    File(&'a Path),
    /// A page of the Web at `url`; its HTTP response named the encoding
    /// `charset`. Its pictures are fetched.
    Url {
        /// The address of the page (after redirections).
        url: &'a str,
        /// The `charset` of its `Content-Type`.
        charset: Option<&'a str>,
    },
}

/// Whether `data` is an HTML (or XHTML) document: it starts with a
/// doctype, an `<html>` element, or a comment or XML declaration before
/// one.
pub fn is_html(data: &[u8]) -> bool {
    let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data);
    let start = data
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(data.len());
    let head = data[start..data.len().min(start + SNIFF_BYTES)].to_ascii_lowercase();
    let starts = |prefix: &[u8]| head.starts_with(prefix);
    starts(b"<!doctype html")
        || starts(b"<html")
        || starts(b"<head")
        || starts(b"<body")
        || ((starts(b"<?xml") || starts(b"<!--")) && head.windows(5).any(|window| window == b"<html"))
}

/// Reads the article of the web page `data` from `origin` into a document.
/// `source` names where it came from ([`Meta::source`](crate::model::Meta::source)).
/// The pictures of a page of the Web are fetched
/// ([`fetch_pictures`](crate::source::web::fetch_pictures)).
///
/// # Errors
/// When the address of a page of the Web is not a URL (any markup can be
/// read: the parser accepts what a browser accepts).
pub fn read(data: &[u8], origin: Origin<'_>, source: Option<String>) -> Result<Document> {
    read_with(data, origin, source, &crate::source::web::fetch_pictures)
}

/// [`read`], the pictures of a page of the Web fetched by `fetch` (their
/// bytes by address; one left out is not shown).
///
/// # Errors
/// As [`read`].
pub fn read_with(data: &[u8], origin: Origin<'_>, source: Option<String>, fetch: &FetchPictures) -> Result<Document> {
    let charset = match origin {
        Origin::Url { charset, .. } => charset,
        _ => None,
    };
    let text = decode::decode(data, charset);
    let mut dom = Dom::parse(&text);
    drop(text);
    let base_url = match origin {
        Origin::Url { url, .. } => Some(Url::parse(url).with_context(|| format!("'{url}' is not a URL"))?),
        Origin::File(path) => directory_url(path),
        Origin::Bytes => None,
    };
    let base = match (&origin, &base_url) {
        (Origin::Url { .. }, Some(url)) => Base::Url(url),
        (Origin::File(_), Some(url)) => Base::File(url),
        _ => Base::None,
    };
    let article = clean::article(&mut dom, &base);
    notes::mark(&mut dom, article.root);

    let mut head = Vec::new();
    if let Some(title) = &article.title {
        let element = dom.element("title", &[]);
        let text = dom.text_node(title);
        dom.append(element, text);
        head.push(element);
    }
    let mut style_bytes = 0;
    for &style in &article.styles {
        style_bytes += dom.text(style).len();
        if style_bytes <= MAX_STYLE_BYTES {
            head.push(style);
        }
    }
    if matches!(origin, Origin::File(_)) {
        head.extend(&article.sheets);
    }
    let files = PageFiles {
        fetched: match origin {
            Origin::Url { .. } => fetch(&picture_urls(&dom, article.root)),
            _ => HashMap::new(),
        },
    };
    remove_small_pictures(&mut dom, article.root, &files);
    let xhtml = dom.to_xhtml(&head, article.root, article.lang.as_deref());
    let name = match origin {
        Origin::File(path) => path.file_name().and_then(|name| name.to_str()).unwrap_or(PAGE_NAME),
        _ => PAGE_NAME,
    };
    let book = crate::epub::html::read_documents(&files, &[(name, xhtml)], article.title.as_deref(), &article.authors)?;
    let mut doc = crate::word::build_document(book, source);
    doc.meta.source_format = Some("html".to_string());
    Ok(doc)
}

/// Removes the pictures in `root` whose file is smaller than
/// [`MIN_PICTURE_PX`](clean::MIN_PICTURE_PX) on a side: icons, bullets and
/// tracking pixels whose size only a style sheet gives.
fn remove_small_pictures(dom: &mut Dom, root: dom::Id, files: &PageFiles) {
    for img in dom.find_all(root, "img") {
        let Some(src) = dom.attr(img, "src") else {
            continue;
        };
        let src = src.split_once('#').map_or(src, |(src, _)| src);
        let small = files
            .read(src)
            .and_then(|data| crate::word::pixel_size(&data))
            .is_some_and(|(width, height)| width < clean::MIN_PICTURE_PX || height < clean::MIN_PICTURE_PX);
        if small {
            dom.detach(img);
        }
    }
}

/// The `file:` URL of the directory of the file `path`.
fn directory_url(path: &Path) -> Option<Url> {
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let dir = std::fs::canonicalize(dir).ok()?;
    Url::from_directory_path(dir).ok()
}

/// The addresses of the pictures of the article in `root` to fetch, in
/// order, once each.
fn picture_urls(dom: &Dom, root: dom::Id) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    for node in dom.descendants(root) {
        let src = match dom.space(node) {
            Some(Space::Html) if dom.is(node, "img") => dom.attr(node, "src"),
            Some(Space::Svg) if dom.name(node) == Some("image") => {
                dom.attr(node, "xlink:href").or_else(|| dom.attr(node, "href"))
            }
            _ => None,
        };
        if let Some(src) = src
            && (src.starts_with("http://") || src.starts_with("https://"))
        {
            let src = src.split_once('#').map_or(src, |(src, _)| src).to_string();
            if !urls.contains(&src) {
                urls.push(src);
            }
        }
    }
    urls
}

/// The files of a page: the pictures fetched with it, or the files next
/// to it (`file:` URLs).
#[derive(Debug, Default)]
struct PageFiles {
    /// Pictures fetched, by address.
    fetched: HashMap<String, Vec<u8>>,
}

impl Resources for PageFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(data) = data_url(path) {
            return Some(data);
        }
        if let Some(data) = self.fetched.get(path) {
            return Some(data.clone());
        }
        let file = Url::parse(path)
            .ok()
            .filter(|url| url.scheme() == "file")?
            .to_file_path()
            .ok()?;
        std::fs::read(file).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::is_html;

    #[test]
    fn pages_are_recognised() {
        assert!(is_html(b"\xef\xbb\xbf  <!DOCTYPE html><title>x</title>"));
        assert!(is_html(b"<html lang=en>"));
        assert!(is_html(
            b"<?xml version=\"1.0\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">"
        ));
        assert!(is_html(b"<!-- saved from url -->\n<HTML>"));
        assert!(!is_html(b"%PDF-1.7"));
        assert!(!is_html(b"PK\x03\x04"));
        assert!(!is_html(b"<?xml version=\"1.0\"?><svg/>"));
        assert!(!is_html(b"# Markdown"));
    }
}
