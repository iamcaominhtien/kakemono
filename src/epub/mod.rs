//! EPUB books (`.epub`) as input: the XHTML content documents of the book,
//! in the order of its spine, read into a [`Document`] without the layout
//! analysis a PDF needs, since the markup says what each paragraph is.
//!
//! The book is read into the same intermediate form as a Word document
//! (`word::ir`), which `word::build` turns into the document: headings,
//! lists, tables, captions, notes, code and the bibliography are recognised
//! by the same rules. Like a Word document, a book has no pages: its
//! pictures are files stored in the [`Document`] ([`Document::pictures`]).
//!
//! - `mod.rs`: the package (`META-INF/container.xml`, the package document
//!   with its metadata, manifest and spine) and the paths inside it.
//! - `html`: XHTML elements → paragraphs, tables, notes and pictures.
//! - `css`: the few properties of the style sheets that give a run its
//!   style (bold, italics, monospace, scripts, small capitals).
//! - `mathml`: MathML → [`MathNode`](crate::model::MathNode).
//! - `entities`: the named character references of HTML, which XML does
//!   not know.

mod css;
mod entities;
pub(crate) mod html;
mod mathml;

use crate::model::Document;
use crate::word::zip::ZipArchive;
use anyhow::{Context, Result, bail};
use roxmltree::{Document as Xml, Node, ParsingOptions};
use std::collections::HashMap;

/// The contents of the `mimetype` entry of an EPUB.
const MIMETYPE: &str = "application/epub+zip";
/// The entry that lists the encrypted files of a book.
const ENCRYPTION: &str = "META-INF/encryption.xml";
/// The entry that names the package document.
const CONTAINER: &str = "META-INF/container.xml";
/// Namespace of the package document (OPF).
const OPF_NS: &str = "http://www.idpf.org/2007/opf";
/// Namespace of Dublin Core (`dc:`), in the metadata of the package.
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
/// Media types of the content documents read from the spine: XHTML, and
/// SVG (a page of a comic or a picture book), whose drawing or picture
/// becomes a figure.
const CONTENT_TYPES: [&str; 3] = ["application/xhtml+xml", "text/html", "image/svg+xml"];
/// Most nodes in one XML document: no limit, as for a Word document (a
/// long chapter is one document).
const MAX_XML_NODES: u32 = u32::MAX;

/// The files a content document refers to (style sheets, pictures): the
/// entries of the archive of a book, or the files of a web page.
pub(crate) trait Resources {
    /// The bytes of the file at `path` (an entry of the archive, a path
    /// next to the page, or a URL), or `None` when there is none. A `data:`
    /// URL is its own content.
    fn read(&self, path: &str) -> Option<Vec<u8>>;
}

impl Resources for ZipArchive<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(data) = data_url(path) {
            return Some(data);
        }
        ZipArchive::read(self, path).ok().flatten()
    }
}

/// The content of a `data:` URL (base64 or percent-encoded), or `None`
/// when `url` is not one.
pub(crate) fn data_url(url: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let rest = url.strip_prefix("data:")?;
    let (header, payload) = rest.split_once(',')?;
    if header.ends_with(";base64") {
        let clean: String = payload.chars().filter(|ch| !ch.is_whitespace()).collect();
        let clean = percent_decode(&clean);
        // Padding is optional: some pages leave it out.
        base64::engine::general_purpose::STANDARD_NO_PAD
            .decode(clean.trim_end_matches('='))
            .ok()
    } else {
        Some(percent_decode_bytes(payload))
    }
}

/// What the package document says about the book.
#[derive(Debug, Default)]
struct Package {
    /// The title (`dc:title`), if any.
    title: Option<String>,
    /// The authors (`dc:creator`), in order.
    creators: Vec<String>,
    /// Paths in the archive of the content documents, in reading order.
    spine: Vec<String>,
}

/// Whether `data` is an EPUB: a ZIP archive with the `mimetype` of an
/// EPUB, or with the container file that names its package.
pub fn is_epub(data: &[u8]) -> bool {
    if !data.starts_with(b"PK\x03\x04") {
        return false;
    }
    let Ok(zip) = ZipArchive::new(data) else {
        return false;
    };
    zip.read_text("mimetype")
        .ok()
        .flatten()
        .is_some_and(|text| text.trim() == MIMETYPE)
        || zip.contains(CONTAINER)
}

/// Reads an EPUB book into a document. `source` names where it came from
/// ([`Meta::source`](crate::model::Meta::source)).
///
/// # Errors
/// When the archive is damaged, has no package document or no content
/// document, or a content document is not well-formed XML.
pub fn read(data: &[u8], source: Option<String>) -> Result<Document> {
    let zip = ZipArchive::new(data)?;
    let opf = rootfile(&zip)?;
    let package = package(&zip, &opf)?;
    if package.spine.is_empty() {
        bail!("the EPUB has no content documents in its spine");
    }
    let mut docs = Vec::with_capacity(package.spine.len());
    for path in &package.spine {
        crate::common::cancel::check();
        let text = zip
            .read_text(path)?
            .with_context(|| format!("the EPUB has no content document {path}"))?;
        docs.push((path.as_str(), text));
    }
    let book = html::read_documents(&zip, &docs, package.title.as_deref(), &package.creators).map_err(|err| {
        // Fonts may be obfuscated in a readable book too: the encryption
        // is named only when the text cannot be read.
        if zip.contains(ENCRYPTION) {
            err.context("the EPUB is encrypted (DRM): its content cannot be read")
        } else {
            err
        }
    })?;
    let mut doc = crate::word::build_document(book, source);
    doc.meta.source_format = Some("epub".to_string());
    Ok(doc)
}

/// Parses an XML document of the book. DTDs are allowed (an XHTML 1.1
/// content document declares one) but never fetched.
///
/// # Errors
/// When the text is not well-formed XML.
fn parse(text: &str) -> Result<Xml<'_>> {
    let options = ParsingOptions {
        allow_dtd: true,
        nodes_limit: MAX_XML_NODES,
    };
    Ok(Xml::parse_with_options(text, options)?)
}

/// The path of the package document, from the container file.
///
/// # Errors
/// When the container is missing or names no package.
fn rootfile(zip: &ZipArchive<'_>) -> Result<String> {
    let text = zip
        .read_text(CONTAINER)?
        .with_context(|| format!("the EPUB has no {CONTAINER}"))?;
    let xml = parse(&text).with_context(|| format!("reading {CONTAINER}"))?;
    let path = xml
        .descendants()
        .filter(|node| node.has_tag_name("rootfile"))
        .find(|node| {
            node.attribute("media-type")
                .is_none_or(|media| media == "application/oebps-package+xml")
        })
        .and_then(|node| node.attribute("full-path"))
        .context("the EPUB container names no package document")?;
    Ok(percent_decode(path))
}

/// Reads the package document at `opf`: the metadata and the spine.
///
/// # Errors
/// When it is missing or is not well-formed XML.
fn package(zip: &ZipArchive<'_>, opf: &str) -> Result<Package> {
    let text = zip
        .read_text(opf)?
        .with_context(|| format!("the EPUB has no package document {opf}"))?;
    let xml = parse(&text).with_context(|| format!("reading {opf}"))?;
    let root = xml.root_element();
    let metadata = |name: &'static str| {
        root.descendants()
            .filter(move |node| node.tag_name().name() == name && node.tag_name().namespace() == Some(DC_NS))
            .map(|node| clean_text(&node))
            .filter(|text| !text.is_empty())
    };
    // The manifest: id → (path, media type).
    let mut manifest: HashMap<&str, (String, &str)> = HashMap::new();
    for item in root
        .descendants()
        .filter(|node| node.tag_name().name() == "item" && node.tag_name().namespace() == Some(OPF_NS))
    {
        if let (Some(id), Some(href)) = (item.attribute("id"), item.attribute("href")) {
            let media = item.attribute("media-type").unwrap_or_default();
            manifest.insert(id, (resolve(opf, href).0, media));
        }
    }
    let spine = root
        .descendants()
        .filter(|node| node.tag_name().name() == "itemref" && node.tag_name().namespace() == Some(OPF_NS))
        .filter_map(|itemref| manifest.get(itemref.attribute("idref")?))
        .filter(|(_, media)| CONTENT_TYPES.contains(media))
        .map(|(path, _)| path.clone())
        .collect();
    Ok(Package {
        title: metadata("title").next(),
        creators: metadata("creator").collect(),
        spine,
    })
}

/// The text of an element, with its white space collapsed.
fn clean_text(node: &Node<'_, '_>) -> String {
    let text: String = node
        .descendants()
        .filter(Node::is_text)
        .filter_map(|text| text.text())
        .collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The reference `href`, made in the document at path `base`, resolved
/// into a path of the archive and a fragment (empty when there is none).
/// Percent-encoded characters are decoded; `..` and `.` are followed.
fn resolve(base: &str, href: &str) -> (String, String) {
    // A URL with a scheme (a picture of a web page, a `data:` URL) is
    // already resolved.
    if href.starts_with("data:") {
        return (href.to_string(), String::new());
    }
    if href.contains("://") {
        let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
        return (path.to_string(), percent_decode(fragment));
    }
    let (path, fragment) = href.split_once('#').unwrap_or((href, ""));
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    if path.is_empty() {
        return (base.to_string(), percent_decode(fragment));
    }
    let mut parts: Vec<String> = if path.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').map(str::to_string).collect()
    };
    // The last part of the base is its file name.
    if !path.starts_with('/') {
        parts.pop();
    }
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(percent_decode(part)),
        }
    }
    (parts.join("/"), percent_decode(fragment))
}

/// `text` with its `%XX` escapes decoded (as UTF-8; invalid bytes are
/// replaced).
fn percent_decode(text: &str) -> String {
    String::from_utf8_lossy(&percent_decode_bytes(text)).into_owned()
}

/// The bytes of `text` with its `%XX` escapes decoded.
fn percent_decode_bytes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{percent_decode, resolve};

    #[test]
    fn references_resolve_to_archive_paths() {
        assert_eq!(
            resolve("EPUB/text/ch1.xhtml", "../media/a%20b.png"),
            ("EPUB/media/a b.png".to_string(), String::new())
        );
        assert_eq!(
            resolve("EPUB/text/ch1.xhtml", "ch2.xhtml#sec-1"),
            ("EPUB/text/ch2.xhtml".to_string(), "sec-1".to_string())
        );
        assert_eq!(
            resolve("EPUB/text/ch1.xhtml", "#fn1"),
            ("EPUB/text/ch1.xhtml".to_string(), "fn1".to_string())
        );
        assert_eq!(
            resolve("content.opf", "text/ch1.xhtml"),
            ("text/ch1.xhtml".to_string(), String::new())
        );
        assert_eq!(percent_decode("%C3%A9t%C3%A9"), "été");
        assert_eq!(percent_decode("100%"), "100%");
    }
}
