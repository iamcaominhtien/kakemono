//! The files that describe an EPUB book rather than hold its text: the
//! container entry point, the package document (metadata, manifest and
//! reading order), the navigation document and, for EPUB 2 readers, the
//! NCX table of contents.

use super::super::esc;

/// `META-INF/container.xml`: where the package document is.
pub(crate) const CONTAINER: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
<rootfiles><rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
"#;

/// Name of the navigation document, inside `EPUB/`.
pub(crate) const NAV_FILE: &str = "nav.xhtml";
/// Name of the style sheet, inside `EPUB/`.
pub(crate) const CSS_FILE: &str = "style.css";
/// Name of the NCX table of contents, inside `EPUB/`.
const NCX_FILE: &str = "toc.ncx";

/// What the package document says about the book.
pub(crate) struct BookInfo<'a> {
    /// The unique identifier (`dc:identifier`).
    pub identifier: &'a str,
    /// The title.
    pub title: &'a str,
    /// The authors, one `dc:creator` each.
    pub authors: &'a [String],
    /// Where the book comes from (`dc:source`), such as the arXiv page.
    pub source: Option<&'a str>,
    /// Last modification (`dcterms:modified`), `CCYY-MM-DDThh:mm:ssZ`.
    pub modified: &'a str,
}

/// One content document of the book.
pub(crate) struct ContentDoc {
    /// File name inside `EPUB/`.
    pub file: String,
    /// It holds MathML (a manifest property readers rely on).
    pub mathml: bool,
}

/// One entry of the table of contents.
pub(crate) struct NavEntry {
    /// Depth, from 0 for the outermost entries.
    pub depth: usize,
    /// Text of the entry.
    pub label: String,
    /// Target, relative to `EPUB/` (`c002.xhtml#sec-3`).
    pub href: String,
}

/// The package document (`EPUB/package.opf`). `images` are the picture
/// files (paths inside `EPUB/`); `cover`, when set, is the one shown by
/// readers in their library.
pub(crate) fn package_opf(info: &BookInfo<'_>, docs: &[ContentDoc], images: &[String], cover: Option<&str>) -> String {
    let mut meta = format!(
        "<dc:identifier id=\"uid\">{}</dc:identifier>\n<dc:title>{}</dc:title>\n<dc:language>en</dc:language>\n",
        esc(info.identifier),
        esc(info.title)
    );
    for author in info.authors {
        meta.push_str(&format!("<dc:creator>{}</dc:creator>\n", esc(author)));
    }
    if let Some(source) = info.source {
        meta.push_str(&format!("<dc:source>{}</dc:source>\n", esc(source)));
    }
    meta.push_str(&format!(
        "<meta property=\"dcterms:modified\">{}</meta>\n<meta name=\"generator\" content=\"kakemono\"/>\n",
        esc(info.modified)
    ));
    let mut manifest = format!(
        "<item id=\"nav\" href=\"{NAV_FILE}\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>\n\
<item id=\"ncx\" href=\"{NCX_FILE}\" media-type=\"application/x-dtbncx+xml\"/>\n\
<item id=\"css\" href=\"{CSS_FILE}\" media-type=\"text/css\"/>\n"
    );
    let mut spine = String::new();
    for (i, doc) in docs.iter().enumerate() {
        let props = if doc.mathml { " properties=\"mathml\"" } else { "" };
        manifest.push_str(&format!(
            "<item id=\"c{i}\" href=\"{}\" media-type=\"application/xhtml+xml\"{props}/>\n",
            esc(&doc.file)
        ));
        spine.push_str(&format!("<itemref idref=\"c{i}\"/>\n"));
    }
    for (i, image) in images.iter().enumerate() {
        let is_cover = cover == Some(image.as_str());
        if is_cover {
            // EPUB 2 readers find the cover through this element.
            meta.push_str(&format!("<meta name=\"cover\" content=\"img{i}\"/>\n"));
        }
        manifest.push_str(&format!(
            "<item id=\"img{i}\" href=\"{}\" media-type=\"{}\"{}/>\n",
            esc(image),
            media_type(image),
            if is_cover { " properties=\"cover-image\"" } else { "" }
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"uid\" xml:lang=\"en\">\n\
<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n{meta}</metadata>\n\
<manifest>\n{manifest}</manifest>\n<spine toc=\"ncx\">\n{spine}</spine>\n</package>\n"
    )
}

/// The navigation document: the table of contents as nested lists.
pub(crate) fn nav_xhtml(title: &str, entries: &[NavEntry]) -> String {
    let mut list = String::new();
    nested(entries, &mut list, |entry| {
        format!("<a href=\"{}\">{}</a>", esc(&entry.href), esc(&entry.label))
    });
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"en\" xml:lang=\"en\">\n\
<head>\n<meta charset=\"utf-8\"/>\n<title>{title}</title>\n<link rel=\"stylesheet\" type=\"text/css\" href=\"{CSS_FILE}\"/>\n</head>\n\
<body>\n<nav epub:type=\"toc\" id=\"toc\">\n<h2>Contents</h2>\n{list}</nav>\n</body>\n</html>\n",
        title = esc(title)
    )
}

/// The NCX table of contents, which EPUB 2 readers use instead of the
/// navigation document.
pub(crate) fn toc_ncx(info: &BookInfo<'_>, entries: &[NavEntry]) -> String {
    let mut points = String::new();
    let depth = entries.iter().map(|entry| entry.depth + 1).max().unwrap_or(1);
    let mut open = 0;
    for (i, entry) in entries.iter().enumerate() {
        while open > entry.depth {
            points.push_str("</navPoint>\n");
            open -= 1;
        }
        if open == entry.depth && i > 0 {
            points.push_str("</navPoint>\n");
        }
        points.push_str(&format!(
            "<navPoint id=\"n{i}\" playOrder=\"{}\"><navLabel><text>{}</text></navLabel><content src=\"{}\"/>\n",
            i + 1,
            esc(&entry.label),
            esc(&entry.href)
        ));
        open = entry.depth;
    }
    if !entries.is_empty() {
        points.push_str(&"</navPoint>\n".repeat(open + 1));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n\
<head>\n<meta name=\"dtb:uid\" content=\"{}\"/>\n<meta name=\"dtb:depth\" content=\"{depth}\"/>\n\
<meta name=\"dtb:totalPageCount\" content=\"0\"/>\n<meta name=\"dtb:maxPageNumber\" content=\"0\"/>\n</head>\n\
<docTitle><text>{}</text></docTitle>\n<navMap>\n{points}</navMap>\n</ncx>\n",
        esc(info.identifier),
        esc(info.title)
    )
}

/// Writes `entries` as nested `<ol>` lists, each item made by `item`. The
/// depths of consecutive entries grow by at most one (see
/// [`NavEntry::depth`]).
fn nested(entries: &[NavEntry], out: &mut String, item: impl Fn(&NavEntry) -> String) {
    out.push_str("<ol>\n");
    let mut open = 0;
    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            if entry.depth > open {
                out.push_str("\n<ol>\n");
            } else {
                out.push_str("</li>\n");
                for _ in entry.depth..open {
                    out.push_str("</ol>\n</li>\n");
                }
            }
        }
        out.push_str(&format!("<li>{}", item(entry)));
        open = entry.depth;
    }
    if !entries.is_empty() {
        out.push_str("</li>\n");
        out.push_str(&"</ol>\n</li>\n".repeat(open));
    }
    out.push_str("</ol>\n");
}

/// The media type of a picture file, from its extension.
fn media_type(file: &str) -> &'static str {
    if file.ends_with(".jpg") || file.ends_with(".jpeg") {
        "image/jpeg"
    } else if file.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "image/png"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Entries at the given depths, labelled by their index.
    fn entries(depths: &[usize]) -> Vec<NavEntry> {
        depths
            .iter()
            .enumerate()
            .map(|(i, &depth)| NavEntry {
                depth,
                label: i.to_string(),
                href: format!("c.xhtml#{i}"),
            })
            .collect()
    }

    #[test]
    fn contents_lists_nest() {
        let mut out = String::new();
        nested(&entries(&[0, 1, 2, 0, 1]), &mut out, |entry| entry.label.clone());
        assert_eq!(
            out.replace('\n', ""),
            "<ol><li>0<ol><li>1<ol><li>2</li></ol></li></ol></li><li>3<ol><li>4</li></ol></li></ol>"
        );
    }

    #[test]
    fn ncx_points_nest() {
        let info = BookInfo {
            identifier: "id",
            title: "T",
            authors: &[],
            source: None,
            modified: "2000-01-01T00:00:00Z",
        };
        let ncx = toc_ncx(&info, &entries(&[0, 1, 1, 0]));
        assert_eq!(ncx.matches("<navPoint ").count(), 4);
        assert_eq!(ncx.matches("</navPoint>").count(), 4);
        assert!(ncx.contains("content src=\"c.xhtml#1\"/>\n</navPoint>\n<navPoint id=\"n2\""));
        assert!(ncx.contains("c.xhtml#2\"/>\n</navPoint>\n</navPoint>\n<navPoint id=\"n3\""));
    }
}
