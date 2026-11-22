//! SVG pictures from other files: made safe to write out, measured, and
//! drawn into a PDF as vector.
//!
//! A foreign SVG is never written out as it came: it is parsed by usvg and
//! written back by usvg's writer, which knows only drawing and text (no
//! script, event handler, `foreignObject`, style sheet or link survives),
//! with only the pictures inside it that [`svg_options`] lets through.

use super::render::{plan_raster, svg_options, svg_to_png};
use krilla::geom::Size;
use krilla::image::Image;
use krilla::surface::Surface;
use krilla_svg::{SurfaceExt, SvgSettings};
use resvg::usvg;

/// The longest side of the page of an SVG made a PDF, in points: PDF
/// readers take at most 14 400 units, and TeX dimensions are below
/// 16 384 pt.
const MAX_PDF_PAGE_SIDE_PT: f32 = 14_400.0;
/// The shortest side of the page of an SVG made a PDF, in points.
const MIN_PDF_PAGE_SIDE_PT: f32 = 1.0;
/// Pixels per point of the PNG drawn of an SVG picture where a bitmap is
/// needed: the fallback of a Word file, the picture of an SVG with filters
/// in a PDF (krilla would draw it at a size of its own: see [`draw`]).
pub const SVG_PNG_SCALE: f32 = 2.0;

/// The SVG `data` (plain or gzip-compressed) parsed by usvg and written
/// back by usvg's writer, its text kept as text (as paths, a few kilobytes
/// of text would take hundreds of megabytes); `None` when it cannot be
/// read.
pub fn sanitize_svg(data: &[u8]) -> Option<String> {
    let tree = usvg::Tree::from_data(data, &svg_options()).ok()?;
    Some(tree.to_string(&usvg::WriteOptions {
        preserve_text: true,
        indent: usvg::Indent::None,
        ..usvg::WriteOptions::default()
    }))
}

/// The size of the PNG [`svg_to_png`] makes of `svg` at `scale` pixels per
/// point, in pixels, without drawing it; `None` when it would make none.
pub fn svg_png_size(svg: &str, scale: f32) -> Option<(u32, u32)> {
    let tree = parse_svg(svg)?;
    let (_, size) = plan_raster(&tree, scale)?;
    Some((size.width(), size.height()))
}

/// The SVG `svg` (one written by [`sanitize_svg`]) as a PDF of one page,
/// as large as the picture, made smaller when a side is longer than
/// [`MAX_PDF_PAGE_SIDE_PT`] (keeping its aspect) and each side at least
/// [`MIN_PDF_PAGE_SIDE_PT`]; `None` when it cannot be read.
pub fn svg_to_pdf(svg: &str) -> Option<Vec<u8>> {
    let tree = parse_svg(svg)?;
    let (width, height) = (tree.size().width(), tree.size().height());
    let shrink = (MAX_PDF_PAGE_SIDE_PT / width.max(height)).min(1.0);
    let side = |length: f32| (length * shrink).clamp(MIN_PDF_PAGE_SIDE_PT, MAX_PDF_PAGE_SIDE_PT);
    let size = Size::from_wh(side(width), side(height))?;
    let mut document = krilla::Document::new();
    let mut page = document.start_page_with(krilla::page::PageSettings::from_wh(size.width(), size.height())?);
    let mut surface = page.surface();
    draw(&mut surface, &tree, svg, size);
    surface.finish();
    page.finish();
    document.finish().ok()
}

/// The SVG `svg` parsed for [`draw`]; `None` when it cannot be read.
pub fn parse_svg(svg: &str) -> Option<usvg::Tree> {
    usvg::Tree::from_str(svg, &svg_options()).ok()
}

/// Draws the SVG `tree` (parsed from `svg`) on `surface` at its origin,
/// `size` large: as vector, or, when it has filters, as a PNG drawn within
/// the pixel cap (krilla draws a filter as a picture of up to 5000 by 5000
/// pixels per nested group, which an SVG of a few bytes could make take
/// gigabytes).
pub fn draw(surface: &mut Surface<'_>, tree: &usvg::Tree, svg: &str, size: Size) {
    // Every filter of the tree, of its masks, clip paths, patterns and SVG
    // pictures too.
    if tree.filters().is_empty() {
        surface.draw_svg(tree, size, SvgSettings::default());
        return;
    }
    let Some(image) = svg_to_png(svg, SVG_PNG_SCALE).and_then(|png| Image::from_png(png.into(), true).ok()) else {
        return;
    };
    surface.draw_image(image, size);
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn what_is_not_an_svg_is_refused() {
        assert!(sanitize_svg(b"not an svg").is_none());
        assert!(sanitize_svg(b"<html><body>no</body></html>").is_none());
    }

    #[test]
    fn the_png_size_is_known_before_drawing() {
        let svg = sanitize_svg(hostile().as_bytes()).unwrap();
        let png = svg_to_png(&svg, 2.0).unwrap();
        let drawn = crate::backend::declared_size(&png).unwrap();
        assert_eq!(svg_png_size(&svg, 2.0), Some(drawn));
        assert_eq!(drawn, (200, 100));
        // A huge one, drawn smaller: the size it is drawn at.
        let huge = r#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><rect width="10" height="10"/></svg>"#;
        let drawn = crate::backend::declared_size(&svg_to_png(huge, 2.0).unwrap()).unwrap();
        assert_eq!(svg_png_size(huge, 2.0), Some(drawn));
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
