//! The picture files of a Word document, made ready for every output:
//! PNG and JPEG as they are, other bitmaps, SVG drawings and Windows
//! metafiles (EMF, WMF, through SVG) turned into PNG.

use crate::backend::within_pixel_cap;
use crate::model::Picture;
use crate::word::fmt::model::FImage;

/// Most bytes of the pictures of one document, once made ready (256 MiB):
/// the pictures past it are left out. Each file of a package may hold up to
/// 512 MiB, and a package many files.
pub(crate) const MAX_PICTURE_BYTES: usize = 256 * 1024 * 1024;
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

/// The picture `data` made ready ([`normalize`]) and added to `pictures`,
/// whose index it gets; `None` when it cannot be shown, or when the
/// pictures would then hold more than `budget` bytes
/// ([`MAX_PICTURE_BYTES`] for a document).
pub(crate) fn store(pictures: &mut Vec<Picture>, data: Vec<u8>, budget: usize) -> Option<usize> {
    let used: usize = pictures.iter().map(|picture| picture.data.len()).sum();
    if used >= budget {
        return None;
    }
    let picture = normalize(data)?;
    if used + picture.data.len() > budget {
        return None;
    }
    pictures.push(picture);
    Some(pictures.len() - 1)
}

/// [`store`] for the page layout: the picture `data` as [`for_layout`]
/// makes it, added to `images`.
pub(crate) fn store_for_layout(images: &mut Vec<FImage>, data: Vec<u8>, budget: usize) -> Option<usize> {
    let size = |image: &FImage| image.data.len() + image.bare.as_ref().map_or(0, Vec::len);
    let used: usize = images.iter().map(size).sum();
    if used >= budget {
        return None;
    }
    let image = for_layout(data)?;
    if used + size(&image) > budget {
        return None;
    }
    images.push(image);
    Some(images.len() - 1)
}

/// The picture `data` as a PNG or JPEG, or `None` when its format cannot
/// be drawn (TIFF, a metafile that draws nothing, or damaged data), or when
/// it is a bitmap of more than [`MAX_RASTER_PIXELS`](crate::backend::MAX_RASTER_PIXELS) pixels (an SVG or a
/// metafile that large is drawn smaller).
pub(crate) fn normalize(data: Vec<u8>) -> Option<Picture> {
    if !within_pixel_cap(&data) {
        return None;
    }
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

    /// The header of a PNG that says it is `width` × `height`, without pixels.
    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut chunk = b"IHDR".to_vec();
        chunk.extend(width.to_be_bytes());
        chunk.extend(height.to_be_bytes());
        chunk.extend([8, 6, 0, 0, 0]);
        // The CRC-32 of the chunk, which the decoder checks.
        let mut crc = !0u32;
        for &byte in &chunk {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        let mut data = b"\x89PNG\r\n\x1a\n\0\0\0\x0d".to_vec();
        data.extend(chunk);
        data.extend((!crc).to_be_bytes());
        data
    }

    #[test]
    fn a_raster_picture_over_the_pixel_cap_is_skipped() {
        assert!(normalize(png_header(100_000, 100_000)).is_none());
        assert!(for_layout(png_header(100_000, 100_000)).is_none());
        assert!(normalize(png_header(400, 300)).is_some());
    }

    #[test]
    fn a_huge_svg_picture_is_drawn_smaller() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><rect width="10" height="10"/></svg>"#;
        let picture = normalize(svg.to_vec()).expect("drawn smaller");
        assert!(picture.data.starts_with(PNG_MAGIC));
    }

    #[test]
    fn pictures_past_the_byte_budget_are_skipped() {
        let mut pictures = Vec::new();
        let png = png_header(4, 2);
        let budget = png.len() * 2;
        assert_eq!(store(&mut pictures, png.clone(), budget), Some(0));
        assert_eq!(store(&mut pictures, png.clone(), budget), Some(1));
        assert_eq!(store(&mut pictures, png.clone(), budget), None);
        assert_eq!(pictures.len(), 2);
    }

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
