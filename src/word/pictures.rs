//! The picture files of a Word document, made ready for every output:
//! PNG and JPEG as they are, other bitmaps, SVG drawings and Windows
//! metafiles (EMF, WMF, through SVG) turned into PNG.

use crate::model::Picture;
use crate::word::fmt::model::FImage;

/// Pixels per point of an SVG drawing turned into a PNG.
const SVG_SCALE: f32 = 2.0;
/// Signature of a PNG file.
const PNG_MAGIC: &[u8] = b"\x89PNG";
/// Signature of a JPEG file.
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];
/// Signatures of a GIF file.
const GIF_MAGIC: &[u8] = b"GIF8";
/// Signature of a BMP file.
const BMP_MAGIC: &[u8] = b"BM";
/// How far into a file the start of an SVG document is looked for.
const SVG_SNIFF_BYTES: usize = 512;

/// The picture `data` as a PNG or JPEG, or `None` when its format cannot
/// be drawn (TIFF, a metafile that draws nothing, or damaged data).
pub(crate) fn normalize(data: Vec<u8>) -> Option<Picture> {
    if data.starts_with(PNG_MAGIC) {
        return Some(Picture { data, jpeg: false });
    }
    if data.starts_with(JPEG_MAGIC) {
        return Some(Picture { data, jpeg: true });
    }
    if data.starts_with(GIF_MAGIC) || data.starts_with(BMP_MAGIC) {
        return to_png(&data);
    }
    if super::metafile::is_metafile(&data) {
        let svg = super::metafile::to_svg(&data)?;
        let png = crate::backend::svg_to_png(&svg, SVG_SCALE)?;
        return Some(Picture { data: png, jpeg: false });
    }
    if is_svg(&data) {
        let svg = String::from_utf8_lossy(&data);
        let png = crate::backend::svg_to_png(&svg, SVG_SCALE)?;
        return Some(Picture { data: png, jpeg: false });
    }
    None
}

/// The picture `data` for the page layout: as [`normalize`] makes it and,
/// for a metafile with lines of text, also without them, with those lines,
/// which the PDF draws as text.
pub(crate) fn for_layout(data: Vec<u8>) -> Option<FImage> {
    let parts = super::metafile::to_svg_and_text(&data)
        .filter(|(_, texts)| !texts.is_empty())
        .and_then(|(svg, texts)| Some((crate::backend::svg_to_png(&svg, SVG_SCALE)?, texts)));
    let picture = normalize(data)?;
    let (bare, texts) = parts.map_or((None, Vec::new()), |(png, texts)| (Some(png), texts));
    Some(FImage {
        data: picture.data,
        jpeg: picture.jpeg,
        bare,
        texts,
    })
}

/// A bitmap in another format re-encoded as a PNG.
fn to_png(data: &[u8]) -> Option<Picture> {
    let image = image::load_from_memory(data).ok()?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(Picture { data: png, jpeg: false })
}

/// Whether `data` is an SVG drawing.
fn is_svg(data: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&data[..data.len().min(SVG_SNIFF_BYTES)]);
    head.trim_start().starts_with('<') && head.contains("<svg")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PNG and JPEG are kept, SVG becomes PNG, a damaged metafile is
    /// refused.
    #[test]
    fn formats() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        assert_eq!(normalize(png.clone()).unwrap().data, png);
        assert!(normalize(vec![0xFF, 0xD8, 0xFF, 0xE0]).unwrap().jpeg);
        assert!(normalize(vec![1, 0, 0, 0, 0x6C, 0, 0, 0]).is_none());
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"2\"><rect width=\"4\" height=\"2\"/></svg>";
        let picture = normalize(svg.to_vec()).unwrap();
        assert!(picture.data.starts_with(PNG_MAGIC));
    }
}
