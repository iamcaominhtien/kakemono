//! SVG pictures from other files: made safe to write out, measured, and
//! drawn into a PDF as vector.
//!
//! A foreign SVG is never written out as it came: it is parsed by usvg and
//! written back by usvg's writer, which knows only drawing and text (no
//! script, event handler, `foreignObject`, style sheet or link survives),
//! with only the pictures inside it that [`svg_options`] lets through. What
//! it costs is bounded first: the text and elements an SVG asks for (with
//! the copies `<use>` makes) are counted before it is read, and what is read
//! is kept as SVG only when it is small enough to write, else as a PNG.

use super::render::{
    MAX_RASTER_SVG_POINTS, MAX_SVG_BYTES, MAX_SVG_ELEMENTS, MAX_SVG_FILTERS, MAX_SVG_NODES, MAX_SVG_PATH_BYTES,
    MAX_SVG_POINTS, MAX_SVG_TEXT_CHARS, plan_raster, png_of_tree, svg_options,
};
use krilla::geom::Size;
use krilla::surface::Surface;
use krilla_svg::{SurfaceExt, SvgSettings};
use resvg::usvg;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use usvg::roxmltree;

/// The longest side of the page of an SVG made a PDF, in points: PDF
/// readers take at most 14 400 units, and TeX dimensions are below
/// 16 384 pt.
const MAX_PDF_PAGE_SIDE_PT: f32 = 14_400.0;
/// The shortest side of the page of an SVG made a PDF, in points.
const MIN_PDF_PAGE_SIDE_PT: f32 = 1.0;
/// Pixels per point of the PNG drawn of an SVG picture where a bitmap is
/// needed: the fallback of a Word file, an SVG kept as a PNG, the picture
/// of an SVG with filters in a PDF.
pub const SVG_PNG_SCALE: f32 = 2.0;
/// The deepest nesting of elements (and of `<use>` in them) counted before
/// an SVG is read; a deeper one is left out (usvg refuses it too).
const MAX_SVG_DEPTH: usize = 1024;
/// Signature of a gzip-compressed file (an `.svgz`).
const GZIP_MAGIC: &[u8] = &[0x1f, 0x8b];
/// The elements whose text nodes usvg lays out.
const TEXT_ELEMENTS: &[&str] = &["text", "tspan", "textPath", "a"];

/// An SVG picture made ready to be kept ([`prepare_svg`]).
#[derive(Debug, Clone, PartialEq)]
pub enum ReadySvg {
    /// Kept as SVG: written again by usvg.
    Svg {
        /// The SVG.
        svg: String,
        /// The size in pixels of the PNG [`svg_to_png`](super::svg_to_png)
        /// makes of it at [`SVG_PNG_SCALE`]; `None` when it would make none.
        png_size: Option<(u32, u32)>,
        /// It has filters, which a PDF draws as a bitmap (see
        /// [`svg_to_pdf`]).
        filtered: bool,
    },
    /// Too large to write as SVG: a PNG drawn of it at [`SVG_PNG_SCALE`].
    Png(Vec<u8>),
}

/// The SVG `data` (plain or gzip-compressed) made ready to be kept, read
/// once: written again by usvg, its text kept as text (as paths, a few
/// kilobytes of text would take hundreds of megabytes); a PNG drawn of it
/// when it has more than [`MAX_SVG_NODES`] nodes, [`MAX_SVG_POINTS`] points
/// or [`MAX_SVG_FILTERS`] filters, or when what usvg writes is too long or
/// not well-formed (usvg's writer copies text and ids as they are, `]]>`
/// and `<` too). `None` when it cannot be read, or asks for more than
/// [`MAX_SVG_TEXT_CHARS`] characters, [`MAX_SVG_ELEMENTS`] elements,
/// [`MAX_SVG_PATH_BYTES`] bytes of path data or [`MAX_RASTER_SVG_POINTS`]
/// points.
pub fn prepare_svg(data: &[u8]) -> Option<ReadySvg> {
    let text = decoded(data)?;
    if !weight(&text).is_some_and(Weight::within_caps) {
        return None;
    }
    let tree = usvg::Tree::from_str(&text, &svg_options()).ok()?;
    let (nodes, points) = complexity(tree.root());
    if points > MAX_RASTER_SVG_POINTS {
        return None;
    }
    if nodes > MAX_SVG_NODES || points > MAX_SVG_POINTS || tree.filters().len() > MAX_SVG_FILTERS {
        return png_of_tree(&tree, SVG_PNG_SCALE).map(ReadySvg::Png);
    }
    let svg = tree.to_string(&usvg::WriteOptions {
        preserve_text: true,
        indent: usvg::Indent::None,
        ..usvg::WriteOptions::default()
    });
    if svg.len() > MAX_SVG_BYTES || roxmltree::Document::parse(&svg).is_err() {
        return png_of_tree(&tree, SVG_PNG_SCALE).map(ReadySvg::Png);
    }
    Some(ReadySvg::Svg {
        png_size: plan_raster(&tree, SVG_PNG_SCALE).map(|(_, size)| (size.width(), size.height())),
        filtered: !tree.filters().is_empty(),
        svg,
    })
}

/// The SVG `data` as [`prepare_svg`] writes it, when it keeps it as SVG.
pub fn sanitize_svg(data: &[u8]) -> Option<String> {
    match prepare_svg(data)? {
        ReadySvg::Svg { svg, .. } => Some(svg),
        ReadySvg::Png(_) => None,
    }
}

/// Whether the SVG `data` (an SVG inside another) asks for no more than an
/// SVG picture may: [`MAX_SVG_TEXT_CHARS`], [`MAX_SVG_ELEMENTS`].
pub(crate) fn within_read_caps(data: &[u8]) -> bool {
    decoded(data).is_some_and(|text| weight(&text).is_some_and(Weight::within_caps))
}

/// The SVG `svg` (one written by [`prepare_svg`]) as a PDF 1.5 of one page
/// (pdfLaTeX reads up to 1.5 without a warning), as large as the picture,
/// made smaller when a side is longer than [`MAX_PDF_PAGE_SIDE_PT`]
/// (keeping its aspect) and each side at least [`MIN_PDF_PAGE_SIDE_PT`];
/// drawn as vector, or, when it has filters, as a PNG drawn within the
/// pixel cap (krilla draws each filter as a bitmap of up to 25 million
/// pixels per nested group). `None` when it cannot be read.
pub fn svg_to_pdf(svg: &str) -> Option<Vec<u8>> {
    let tree = parse_svg(svg)?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let shrink = (MAX_PDF_PAGE_SIDE_PT / width.max(height)).min(1.0);
    let side = |length: f32| (length * shrink).clamp(MIN_PDF_PAGE_SIDE_PT, MAX_PDF_PAGE_SIDE_PT);
    let size = Size::from_wh(side(width), side(height))?;
    let configuration = krilla::configure::ConfigurationBuilder::new()
        .with_version(krilla::configure::PdfVersion::Pdf15)
        .finish()
        .ok()?;
    let mut document = krilla::Document::new_with(krilla::SerializeSettings {
        configuration,
        ..krilla::SerializeSettings::default()
    });
    let mut page = document.start_page_with(krilla::page::PageSettings::from_wh(size.width(), size.height())?);
    let mut surface = page.surface();
    if tree.filters().is_empty() {
        draw_vector(&mut surface, &tree, size);
    } else if let Some(image) =
        png_of_tree(&tree, SVG_PNG_SCALE).and_then(|png| krilla::image::Image::from_png(png.into(), true).ok())
    {
        surface.draw_image(image, size);
    }
    surface.finish();
    page.finish();
    document.finish().ok()
}

/// The SVG `svg` (one written by [`prepare_svg`]) parsed for
/// [`draw_vector`]; `None` when it cannot be read.
pub fn parse_svg(svg: &str) -> Option<usvg::Tree> {
    usvg::Tree::from_str(svg, &svg_options()).ok()
}

/// Draws the SVG `tree` on `surface` at its origin, `size` large, as
/// vector. A tree with filters is better drawn as a PNG ([`png_of_tree`]):
/// krilla draws each filter as a bitmap of up to 25 million pixels per
/// nested group.
pub fn draw_vector(surface: &mut Surface<'_>, tree: &usvg::Tree, size: Size) {
    surface.draw_svg(tree, size, SvgSettings::default());
}

/// The text of the SVG `data`, decompressed when it is gzip-compressed;
/// `None` when it is not UTF-8 or longer than [`MAX_SVG_BYTES`].
fn decoded(data: &[u8]) -> Option<Cow<'_, str>> {
    if !data.starts_with(GZIP_MAGIC) {
        return (data.len() <= MAX_SVG_BYTES).then(|| std::str::from_utf8(data).ok().map(Cow::Borrowed))?;
    }
    let mut text = Vec::new();
    flate2::read::GzDecoder::new(data)
        .take(MAX_SVG_BYTES as u64 + 1)
        .read_to_end(&mut text)
        .ok()?;
    if text.len() > MAX_SVG_BYTES {
        return None;
    }
    String::from_utf8(text).ok().map(Cow::Owned)
}

/// What an SVG asks usvg to read: its elements, the characters of its
/// text and the bytes of its path data, each copy that `<use>` makes
/// counted.
#[derive(Debug, Clone, Copy, Default)]
struct Weight {
    /// Elements.
    elements: u64,
    /// Characters of text that usvg lays out.
    text_chars: u64,
    /// Bytes of path data (`d`, `points`).
    path_bytes: u64,
}

impl Weight {
    /// A weight over every cap.
    const OVER: Self = Self {
        elements: u64::MAX,
        text_chars: u64::MAX,
        path_bytes: u64::MAX,
    };

    /// The sum of two weights.
    fn plus(self, other: Self) -> Self {
        Self {
            elements: self.elements.saturating_add(other.elements),
            text_chars: self.text_chars.saturating_add(other.text_chars),
            path_bytes: self.path_bytes.saturating_add(other.path_bytes),
        }
    }

    /// Whether the weight is within [`MAX_SVG_ELEMENTS`],
    /// [`MAX_SVG_TEXT_CHARS`] and [`MAX_SVG_PATH_BYTES`].
    fn within_caps(self) -> bool {
        self.elements <= MAX_SVG_ELEMENTS
            && self.text_chars <= MAX_SVG_TEXT_CHARS
            && self.path_bytes <= MAX_SVG_PATH_BYTES
    }
}

/// The weight of the SVG `text`, read as usvg reads it (a DTD allowed);
/// `None` when it is not well-formed XML.
fn weight(text: &str) -> Option<Weight> {
    let options = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let doc = roxmltree::Document::parse_with_options(text, options).ok()?;
    let ids: HashMap<&str, roxmltree::Node<'_, '_>> = doc
        .descendants()
        .filter_map(|node| Some((node.attribute("id")?, node)))
        .collect();
    let mut counter = Counter {
        ids,
        weights: HashMap::new(),
        open: HashSet::new(),
    };
    Some(counter.of(doc.root_element(), 0))
}

/// Counts the weight of the elements of an SVG, each once.
struct Counter<'a, 'input> {
    /// The elements by their id, for `<use>`.
    ids: HashMap<&'a str, roxmltree::Node<'a, 'input>>,
    /// The weights already counted.
    weights: HashMap<roxmltree::NodeId, Weight>,
    /// The elements being counted, for references that loop.
    open: HashSet<roxmltree::NodeId>,
}

impl<'a> Counter<'a, '_> {
    /// The weight of the element `node`, `depth` elements and references
    /// deep: itself, its children and what a `<use>` copies. A reference
    /// back to an element being counted counts nothing (usvg drops it).
    fn of(&mut self, node: roxmltree::Node<'a, '_>, depth: usize) -> Weight {
        if let Some(weight) = self.weights.get(&node.id()) {
            return *weight;
        }
        if depth > MAX_SVG_DEPTH {
            return Weight::OVER;
        }
        if !self.open.insert(node.id()) {
            return Weight::default();
        }
        let path_bytes = ["d", "points"]
            .iter()
            .filter_map(|name| node.attribute(*name))
            .map(|data| data.len() as u64)
            .sum();
        let mut weight = Weight {
            elements: 1,
            text_chars: 0,
            path_bytes,
        };
        let holds_text = TEXT_ELEMENTS.contains(&node.tag_name().name());
        for child in node.children() {
            if child.is_element() {
                weight = weight.plus(self.of(child, depth + 1));
            } else if holds_text && let Some(text) = child.text() {
                weight.text_chars = weight.text_chars.saturating_add(text.chars().count() as u64);
            }
        }
        if node.tag_name().name() == "use" {
            let href = node
                .attributes()
                .find(|attr| attr.name() == "href")
                .map(|attr| attr.value());
            if let Some(target) = href
                .and_then(|href| href.strip_prefix('#'))
                .and_then(|id| self.ids.get(id))
            {
                weight = weight.plus(self.of(*target, depth + 1));
            }
        }
        self.open.remove(&node.id());
        self.weights.insert(node.id(), weight);
        weight
    }
}

/// The nodes and the points of the paths of `group`, with those of its
/// masks, clip paths, patterns, text and SVG pictures.
fn complexity(group: &usvg::Group) -> (u64, u64) {
    let mut nodes = 0u64;
    let mut points = 0u64;
    for node in group.children() {
        nodes += 1;
        match node {
            usvg::Node::Group(child) => add(&mut nodes, &mut points, complexity(child)),
            usvg::Node::Path(path) => points = points.saturating_add(path.data().len() as u64),
            usvg::Node::Text(text) => add(&mut nodes, &mut points, complexity(text.flattened())),
            usvg::Node::Image(_) => {}
        }
        node.subroots(|root| add(&mut nodes, &mut points, complexity(root)));
    }
    (nodes, points)
}

/// Adds `(nodes, points)` to the counts.
fn add(nodes: &mut u64, points: &mut u64, (more_nodes, more_points): (u64, u64)) {
    *nodes = nodes.saturating_add(more_nodes);
    *points = points.saturating_add(more_points);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::svg_to_png;

    /// A PNG of 2 × 1 pixels, as a data URI.
    fn png_uri() -> String {
        use base64::Engine;
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(2, 1, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        )
    }

    /// An SVG that draws a blue rectangle and an embedded PNG, with every
    /// way a file can run code or reach out.
    fn hostile() -> String {
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="50" onload="alert(1)">
<style>@import url(https://example.com/x.css); rect {{ stroke: #000 }}</style>
<script>alert(2)</script>
<rect width="100" height="50" fill="#0000ff" onclick="alert(3)"/>
<foreignObject width="100" height="50"><div xmlns="http://www.w3.org/1999/xhtml">html</div></foreignObject>
<image width="10" height="10" xlink:href="file:///etc/passwd"/>
<image width="10" height="10" href="https://example.com/t.png"/>
<image width="10" height="10" href="secret.png"/>
<image width="10" height="10" href="{png}"/>
<a xlink:href="javascript:alert(4)"><circle r="5" fill="red"/></a>
</svg>"##,
            png = png_uri()
        )
    }

    #[test]
    fn a_foreign_svg_keeps_only_its_drawing() {
        let svg = sanitize_svg(hostile().as_bytes()).expect("an SVG");
        let lower = svg.to_lowercase();
        for gone in [
            "script",
            "alert",
            "onload",
            "onclick",
            "foreignobject",
            "<div",
            "file:",
            "https:",
            "secret.png",
            "javascript",
            "<style",
            "@import",
        ] {
            assert!(!lower.contains(gone), "{gone} is still in {svg}");
        }
        // The drawing stays: the rectangle, its style, the circle and the
        // embedded PNG.
        assert!(lower.contains("#0000ff") && lower.contains("<path"), "{svg}");
        assert!(lower.contains("stroke=\"#000000\""), "{svg}");
        assert!(svg.contains("data:image/png;base64"), "{svg}");
        // What is written can be read again.
        assert!(sanitize_svg(svg.as_bytes()).is_some());
    }

    #[test]
    fn an_svg_keeps_only_pictures_it_can_size_within_the_cap() {
        use base64::Engine;
        // A PNG header that says 100 000 × 100 000 pixels.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(100_000u32.to_be_bytes());
        png.extend(100_000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let uri = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/><image width="10" height="10" href="{uri}"/></svg>"#
        );
        let svg = sanitize_svg(svg.as_bytes()).expect("an SVG");
        assert!(!svg.contains("<image"), "{svg}");
    }

    #[test]
    fn text_stays_text() {
        // Text written as paths would be a thousand times longer: a few
        // kilobytes of text in a file could become hundreds of megabytes.
        let words = "lorem ipsum &amp; &lt;b&gt; ".repeat(200);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="20"><text x="0" y="15" font-size="12">{words}</text></svg>"#
        );
        let out = sanitize_svg(svg.as_bytes()).expect("an SVG");
        assert!(
            out.contains("<text") && out.contains("lorem ipsum &amp; &lt;b"),
            "{out}"
        );
        assert!(!out.contains("<b>"), "{out}");
        assert!(out.len() < svg.len() * 2, "{} bytes from {}", out.len(), svg.len());
    }

    /// An SVG of `levels` levels of groups of 10 `<use>` of the one below,
    /// over a path of `segments` segments: 10^levels copies of it.
    fn use_laughs(levels: usize, segments: usize) -> String {
        let leaf = format!("M0 0{}", " l1 1".repeat(segments));
        let groups: String = (1..=levels)
            .map(|level| {
                format!(
                    r##"<g id="a{level}">{}</g>"##,
                    format!(r##"<use href="#a{}"/>"##, level - 1).repeat(10)
                )
            })
            .collect();
        format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><defs><path id="a0" d="{leaf}"/>{groups}</defs><use href="#a{levels}"/></svg>"##
        )
    }

    #[test]
    fn copies_made_by_use_do_not_blow_up_the_svg() {
        // 10^5 copies of 100 segments: 1.5 KB that usvg's writer made 84 MB.
        let start = std::time::Instant::now();
        match prepare_svg(use_laughs(5, 100).as_bytes()) {
            Some(ReadySvg::Svg { svg, .. }) => panic!("kept as an SVG of {} bytes", svg.len()),
            Some(ReadySvg::Png(png)) => assert!(png.starts_with(b"\x89PNG")),
            None => {}
        }
        // 10^5 copies of 1500 segments, 10^7 copies of a path: refused.
        assert_eq!(prepare_svg(use_laughs(5, 1500).as_bytes()), None);
        assert_eq!(prepare_svg(use_laughs(7, 1).as_bytes()), None);
        assert!(start.elapsed().as_secs() < 20, "{:?}", start.elapsed());
        // A few copies stay an SVG.
        assert!(matches!(
            prepare_svg(use_laughs(2, 10).as_bytes()),
            Some(ReadySvg::Svg { .. })
        ));
    }

    #[test]
    fn what_is_written_is_well_formed() {
        // The writer copies text and ids as they are: `]]>` in text, `<` in
        // an id.
        for svg in [
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><text x="0" y="20">a ]]&gt; b</text></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect id="&quot;&gt;&lt;script&gt;" width="3" height="3"/></svg>"#,
        ] {
            match prepare_svg(svg.as_bytes()).expect("kept") {
                ReadySvg::Svg { svg: out, .. } => {
                    usvg::roxmltree::Document::parse(&out).unwrap_or_else(|err| panic!("{err}: {out}"));
                }
                ReadySvg::Png(png) => assert!(png.starts_with(b"\x89PNG")),
            }
        }
    }

    #[test]
    fn an_svg_of_too_much_text_is_refused() {
        let start = std::time::Instant::now();
        let long = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><text x="0" y="20">{}</text></svg>"#,
            "lorem ipsum ".repeat(5000)
        );
        assert_eq!(prepare_svg(long.as_bytes()), None);
        // Short text, used many times.
        let copies = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><defs><text id="t" x="0" y="20">{}</text></defs>{}</svg>"##,
            "lorem ipsum ".repeat(30),
            r##"<use href="#t"/>"##.repeat(100)
        );
        assert_eq!(prepare_svg(copies.as_bytes()), None);
        assert!(start.elapsed().as_secs() < 5, "{:?}", start.elapsed());
        let short = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><text x="0" y="20">a label</text></svg>"#;
        assert!(matches!(prepare_svg(short.as_bytes()), Some(ReadySvg::Svg { .. })));
    }

    /// An SVG of `count` nested groups, each with a filter of its own.
    fn filters(count: usize) -> String {
        let defs: String = (0..count)
            .map(|i| format!(r#"<filter id="f{i}"><feGaussianBlur stdDeviation="3"/></filter>"#))
            .collect();
        let open: String = (0..count).map(|i| format!(r#"<g filter="url(#f{i})">"#)).collect();
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">{defs}{open}<rect width="100" height="50"/>{}</svg>"#,
            "</g>".repeat(count)
        )
    }

    #[test]
    fn an_svg_of_many_filters_becomes_a_png() {
        assert!(matches!(prepare_svg(filters(200).as_bytes()), Some(ReadySvg::Png(_))));
        assert!(matches!(
            prepare_svg(filters(3).as_bytes()),
            Some(ReadySvg::Svg { filtered: true, .. })
        ));
        assert!(matches!(
            prepare_svg(hostile().as_bytes()),
            Some(ReadySvg::Svg { filtered: false, .. })
        ));
    }

    #[test]
    fn filters_drawn_as_a_bitmap_share_the_pixel_cap() {
        // 200 blurred rectangles over 5000 × 5000 points: each filter is
        // drawn over the whole picture.
        let rects: String = (0..200)
            .map(|i| format!(r#"<rect filter="url(#f)" x="{i}" width="5000" height="5000"/>"#))
            .collect();
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="5000" height="5000"><filter id="f"><feGaussianBlur stdDeviation="5"/></filter>{rects}</svg>"#
        );
        let start = std::time::Instant::now();
        let png = svg_to_png(&svg, 2.0).expect("drawn smaller");
        assert!(start.elapsed().as_secs() < 30, "{:?}", start.elapsed());
        let (width, height) = crate::backend::declared_size(&png).unwrap();
        assert!(u64::from(width) * u64::from(height) * 200 <= crate::backend::render::MAX_FILTERED_PIXELS);
    }

    #[test]
    fn the_pdf_of_an_svg_is_one_that_pdflatex_reads_without_warning() {
        let svg = sanitize_svg(hostile().as_bytes()).unwrap();
        assert!(svg_to_pdf(&svg).unwrap().starts_with(b"%PDF-1.5"));
    }

    #[test]
    fn what_is_not_an_svg_is_refused() {
        assert!(sanitize_svg(b"not an svg").is_none());
        assert!(sanitize_svg(b"<html><body>no</body></html>").is_none());
    }

    /// The size of the PNG that would be drawn of a picture kept as SVG.
    fn png_size(data: &[u8]) -> Option<(u32, u32)> {
        match prepare_svg(data)? {
            ReadySvg::Svg { png_size, .. } => png_size,
            ReadySvg::Png(_) => None,
        }
    }

    #[test]
    fn the_png_size_is_known_before_drawing() {
        let svg = sanitize_svg(hostile().as_bytes()).unwrap();
        let png = svg_to_png(&svg, 2.0).unwrap();
        let drawn = crate::backend::declared_size(&png).unwrap();
        assert_eq!(png_size(hostile().as_bytes()), Some(drawn));
        assert_eq!(drawn, (200, 100));
        // A huge one, drawn smaller: the size it is drawn at.
        let huge = r#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><rect width="10" height="10"/></svg>"#;
        let drawn = crate::backend::declared_size(&svg_to_png(huge, 2.0).unwrap()).unwrap();
        assert_eq!(png_size(huge.as_bytes()), Some(drawn));
    }

    /// The pictures (`/Subtype/Image`) of the PDF `pdf`.
    fn pictures_in(pdf: &[u8]) -> usize {
        let text = String::from_utf8_lossy(pdf);
        text.matches("/Subtype/Image").count()
    }

    #[test]
    fn an_svg_becomes_a_pdf_of_vector_drawing() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><rect width="100" height="50" fill="#0000ff"/><circle cx="25" cy="25" r="20" fill="red"/></svg>"##;
        let pdf = svg_to_pdf(&sanitize_svg(svg.as_bytes()).unwrap()).expect("a PDF");
        assert!(pdf.starts_with(b"%PDF"));
        assert_eq!(pictures_in(&pdf), 0);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/MediaBox[0 0 100 50]"), "{text}");
    }

    /// The width and height of the first page of the PDF `pdf`.
    fn page_size(pdf: &[u8]) -> (f32, f32) {
        let text = String::from_utf8_lossy(pdf);
        let start = text.find("/MediaBox[").expect("a page") + "/MediaBox[".len();
        let sides: Vec<f32> = text[start..]
            .split(']')
            .next()
            .unwrap()
            .split_whitespace()
            .map(|value| value.parse().unwrap())
            .collect();
        (sides[2] - sides[0], sides[3] - sides[1])
    }

    #[test]
    fn the_page_of_an_svg_of_absurd_size_stays_within_what_pdf_and_tex_read() {
        // PDF pages are at most 14 400 units on a side, TeX dimensions at
        // most 16 384 pt: a picture 2e8 points wide and 0.01 high is made a
        // page that both read.
        for (width, height) in [("200000000", "0.01"), ("0.01", "200000000"), ("1e30", "1e30")] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="10" height="10"/></svg>"#
            );
            let pdf = svg_to_pdf(&sanitize_svg(svg.as_bytes()).unwrap()).expect("a PDF");
            let (width, height) = page_size(&pdf);
            for side in [width, height] {
                assert!((1.0..=MAX_PDF_PAGE_SIDE_PT).contains(&side), "{width} x {height}");
            }
        }
    }

    #[test]
    fn an_svg_with_a_filter_becomes_a_pdf_of_a_picture() {
        let open = r#"<g opacity="0.9">"#.repeat(30);
        let close = "</g>".repeat(30);
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><filter id="f"><feGaussianBlur stdDeviation="5"/></filter><g filter="url(#f)">{open}<rect width="30000" height="30000" fill="red"/>{close}</g></svg>"#
        );
        let pdf = svg_to_pdf(&sanitize_svg(svg.as_bytes()).unwrap()).expect("a PDF");
        assert_eq!(pictures_in(&pdf), 1);
        // A filter in a mask, or in an SVG picture inside, counts too.
        let filtered = r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><filter id="f"><feGaussianBlur stdDeviation="5"/></filter><rect width="50" height="50" filter="url(#f)"/></svg>"#;
        let uri = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.encode(filtered)
        };
        for inner in [
            r#"<mask id="m"><g filter="url(#f)"><rect width="50" height="50" fill="white"/></g></mask><rect width="50" height="50" mask="url(#m)"/>"#.to_string(),
            format!(r#"<image width="50" height="50" href="data:image/svg+xml;base64,{uri}"/>"#),
        ] {
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="50" height="50"><filter id="f"><feGaussianBlur stdDeviation="5"/></filter>{inner}</svg>"#
            );
            let pdf = svg_to_pdf(&sanitize_svg(svg.as_bytes()).unwrap()).expect("a PDF");
            assert_eq!(pictures_in(&pdf), 1, "{svg}");
        }
    }
}
