//! The picture files of a Word document, made ready for every output:
//! PNG and JPEG as they are, other bitmaps turned into PNG, SVG drawings
//! and Windows metafiles (EMF, WMF, through SVG) kept as vector: SVG
//! written again by usvg (`backend::sanitize_svg`).

use crate::backend::svg::ReadySvg;
use crate::backend::within_pixel_cap;
use crate::model::{Picture, PictureFormat};
use crate::word::fmt::model::FImage;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::Mutex;

/// Most bytes of the pictures of one document, once made ready (256 MiB):
/// the pictures past it are left out. Each file of a package may hold up to
/// 512 MiB, and a package many files.
pub(crate) const MAX_PICTURE_BYTES: usize = 256 * 1024 * 1024;
/// Points in a pixel at 96 pixels per inch: the size of a picture whose
/// document gives none.
pub(crate) const PT_PER_PX: f32 = 0.75;
/// The size in points of a picture whose document and file give none.
pub(crate) const FALLBACK_PICTURE_PT: (f32, f32) = (300.0, 200.0);
/// The longest side of a picture, in points: 22 inches, Word's largest
/// page. A document can give any size, an infinite one too; a larger
/// picture is made smaller, keeping its aspect.
pub(crate) const MAX_PICTURE_SIDE_PT: f32 = 1584.0;
/// Signature of a PNG file.
const PNG_MAGIC: &[u8] = b"\x89PNG";
/// Signature of a JPEG file.
const JPEG_MAGIC: &[u8] = &[0xFF, 0xD8, 0xFF];
/// Signatures of a GIF file.
const GIF_MAGIC: &[u8] = b"GIF8";
/// Signature of a BMP file.
const BMP_MAGIC: &[u8] = b"BM";
/// Signature of a gzip-compressed file.
const GZIP_MAGIC: &[u8] = &[0x1f, 0x8b];
/// Most bytes of the vector pictures (SVG, metafiles) last made ready that
/// are kept for the next reading of the same file (64 MiB): the text and
/// the page layout read a Word file each, and an SVG can take seconds to
/// read.
const MAX_RECENT_VECTOR_BYTES: usize = 64 * 1024 * 1024;
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

/// The size `(width, height)` in points, made at most
/// [`MAX_PICTURE_SIDE_PT`] on each side, keeping its aspect; `None` when a
/// side is not a positive finite number.
pub(crate) fn bounded_size((width, height): (f32, f32)) -> Option<(f32, f32)> {
    if !(width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0) {
        return None;
    }
    let shrink = (MAX_PICTURE_SIDE_PT / width.max(height)).min(1.0);
    Some((width * shrink, height * shrink))
}

/// The size in points of `picture` at 96 pixels per inch, bounded
/// ([`bounded_size`]): of a bitmap, by the size its header gives; of an
/// SVG, by that of the PNG drawn of it at 2 pixels per point (found when
/// it was read). `None` when it has none.
pub(crate) fn pixel_size_pt(picture: &Picture) -> Option<(f32, f32)> {
    let (width, height) = match picture.format {
        PictureFormat::Svg => picture.png_size?,
        PictureFormat::Png | PictureFormat::Jpeg => crate::backend::declared_size(&picture.data)?,
    };
    bounded_size((width as f32 * PT_PER_PX, height as f32 * PT_PER_PX))
}

/// The picture `data` as a PNG, a JPEG or an SVG written again by usvg,
/// or `None` when its format cannot be drawn (TIFF, a metafile that draws
/// nothing, or damaged data), or when it is a bitmap of more than
/// [`MAX_RASTER_PIXELS`](crate::backend::MAX_RASTER_PIXELS) pixels (an SVG
/// or a metafile can have any size: one drawn as a bitmap is drawn smaller).
pub(crate) fn normalize(data: Vec<u8>) -> Option<Picture> {
    if !within_pixel_cap(&data) {
        return None;
    }
    if data.starts_with(PNG_MAGIC) {
        return Some(Picture::bitmap(data, PictureFormat::Png));
    }
    if data.starts_with(JPEG_MAGIC) {
        return Some(Picture::bitmap(data, PictureFormat::Jpeg));
    }
    if data.starts_with(GIF_MAGIC) || data.starts_with(BMP_MAGIC) {
        return to_png(&data);
    }
    if super::metafile::is_metafile(&data) {
        return made_once(&data, || svg_picture(super::metafile::to_svg(&data)?.as_bytes()));
    }
    if is_svg(&data) {
        return made_once(&data, || svg_picture(&data));
    }
    None
}

/// The vector picture `data` made ready by `make`, or as it was made last
/// time when the same file was made ready a little before (by the other
/// reading of the same Word file): the last ones are kept, up to
/// [`MAX_RECENT_VECTOR_BYTES`], by the SHA-256 of their file.
fn made_once(data: &[u8], make: impl FnOnce() -> Option<Picture>) -> Option<Picture> {
    type Recent = VecDeque<([u8; 32], Option<Picture>)>;
    static RECENT: Mutex<Recent> = Mutex::new(VecDeque::new());
    let key: [u8; 32] = Sha256::digest(data).into();
    let found = RECENT.lock().ok().and_then(|recent| {
        recent
            .iter()
            .find(|(made, _)| *made == key)
            .map(|(_, picture)| picture.clone())
    });
    if let Some(picture) = found {
        return picture;
    }
    let picture = make();
    if let Ok(mut recent) = RECENT.lock() {
        let size = |picture: &Option<Picture>| picture.as_ref().map_or(0, |picture| picture.data.len());
        recent.push_back((key, picture.clone()));
        while recent.iter().map(|(_, picture)| size(picture)).sum::<usize>() > MAX_RECENT_VECTOR_BYTES {
            recent.pop_front();
        }
    }
    picture
}

/// The picture `data` for the page layout: as [`normalize`] makes it and,
/// for a metafile with lines of text, also without them, with those lines,
/// which the PDF draws as text.
pub(crate) fn for_layout(data: Vec<u8>) -> Option<FImage> {
    // The picture without its text, when it can be kept as SVG.
    let parts = super::metafile::to_svg_and_text(&data)
        .filter(|(_, texts)| !texts.is_empty())
        .and_then(|(svg, texts)| Some((crate::backend::sanitize_svg(svg.as_bytes())?, texts)));
    let picture = normalize(data)?;
    let (bare, texts) = parts.map_or((None, Vec::new()), |(svg, texts)| (Some(svg.into_bytes()), texts));
    Some(FImage {
        data: picture.data,
        format: picture.format,
        bare,
        texts,
    })
}

/// The SVG `data` as a picture, read once (`backend::prepare_svg`): written
/// again by usvg, or a PNG drawn of it when it is too large to write as
/// SVG; `None` when it cannot be read, or asks for too much.
fn svg_picture(data: &[u8]) -> Option<Picture> {
    Some(match crate::backend::prepare_svg(data)? {
        ReadySvg::Svg { svg, png_size, .. } => Picture {
            data: svg.into_bytes(),
            format: PictureFormat::Svg,
            png_size,
        },
        ReadySvg::Png(png) => Picture::bitmap(png, PictureFormat::Png),
    })
}

/// A bitmap in another format re-encoded as a PNG.
fn to_png(data: &[u8]) -> Option<Picture> {
    let image = image::load_from_memory(data).ok()?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(Picture::bitmap(png, PictureFormat::Png))
}

/// Whether `data` is an SVG drawing, or a gzip-compressed file (an `.svgz`:
/// Word reads no other compressed picture), which
/// `backend::prepare_svg` reads within its byte cap.
fn is_svg(data: &[u8]) -> bool {
    if data.starts_with(GZIP_MAGIC) {
        return true;
    }
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
    fn a_huge_svg_picture_stays_svg_and_is_sized_as_its_png_would_be() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><rect width="10" height="10"/></svg>"#;
        let picture = normalize(svg.to_vec()).expect("kept");
        assert_eq!(picture.format, PictureFormat::Svg);
        // The size the PNG drawn of it, within the pixel cap, had.
        let (width, height) = pixel_size_pt(&picture).expect("a size");
        assert_eq!((width, height), (MAX_PICTURE_SIDE_PT, MAX_PICTURE_SIDE_PT));
        let small =
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="10" height="10"/></svg>"#;
        let small = normalize(small.to_vec()).unwrap();
        assert_eq!(
            pixel_size_pt(&small),
            Some((40.0 * 2.0 * PT_PER_PX, 20.0 * 2.0 * PT_PER_PX))
        );
    }

    #[test]
    fn the_size_of_a_large_picture_keeps_its_aspect() {
        // 4000 × 1000 pixels: 3000 × 750 points, bounded to the largest side.
        let png = |width, height| Picture::bitmap(png_header(width, height), PictureFormat::Png);
        let (width, height) = pixel_size_pt(&png(4000, 1000)).unwrap();
        assert_eq!((width, height), (MAX_PICTURE_SIDE_PT, MAX_PICTURE_SIDE_PT / 4.0));
        assert_eq!(pixel_size_pt(&png(400, 200)), Some((300.0, 150.0)));
        let damaged = Picture::bitmap(b"not a picture".to_vec(), PictureFormat::Png);
        assert_eq!(pixel_size_pt(&damaged), None);
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

    /// PNG and JPEG are kept, an SVG stays SVG (written again by usvg), a
    /// damaged metafile is refused.
    #[test]
    fn formats() {
        let png = b"\x89PNG\r\n\x1a\nrest".to_vec();
        let picture = normalize(png.clone()).unwrap();
        assert_eq!((picture.data, picture.format), (png, PictureFormat::Png));
        assert_eq!(
            normalize(vec![0xFF, 0xD8, 0xFF, 0xE0]).unwrap().format,
            PictureFormat::Jpeg
        );
        assert!(normalize(vec![1, 0, 0, 0, 0x6C, 0, 0, 0]).is_none());
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"4\" height=\"2\" onload=\"alert(1)\"><script>alert(2)</script><rect width=\"4\" height=\"2\"/></svg>";
        let picture = normalize(svg.to_vec()).unwrap();
        assert_eq!(picture.format, PictureFormat::Svg);
        let text = String::from_utf8(picture.data).unwrap();
        assert!(text.contains("<path") && !text.contains("alert"), "{text}");
    }

    #[test]
    fn a_vector_picture_is_made_ready_once_for_both_readings() {
        // The text and the page layout read the same file: an SVG that
        // takes long to read (a PNG drawn of many filters) is read once.
        let defs: String = (0..100)
            .map(|i| format!(r#"<filter id="f{i}"><feGaussianBlur stdDeviation="3"/></filter>"#))
            .collect();
        let rects: String = (0..100)
            .map(|i| format!(r#"<rect filter="url(#f{i})" x="{i}" width="2000" height="1000"/>"#))
            .collect();
        let svg = format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="2000" height="1000">{defs}{rects}</svg>"#);
        let start = std::time::Instant::now();
        let first = normalize(svg.clone().into_bytes()).expect("a PNG");
        let once = start.elapsed();
        let start = std::time::Instant::now();
        let image = for_layout(svg.into_bytes()).expect("a PNG");
        let again = start.elapsed();
        assert_eq!((first.data, first.format), (image.data, image.format));
        assert!(again * 10 < once, "{once:?} then {again:?}");
    }

    #[test]
    fn a_compressed_svg_is_read() {
        use std::io::Write;
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="2"><rect width="4" height="2"/></svg>"#;
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gzip.write_all(svg).unwrap();
        let picture = normalize(gzip.finish().unwrap()).expect("an SVG");
        assert_eq!(picture.format, PictureFormat::Svg);
        assert_eq!(picture.png_size, Some((8, 4)));
    }

    #[test]
    fn an_svg_of_many_copies_is_kept_as_a_png() {
        // 10^4 copies of a path of 200 segments.
        let leaf = format!("M0 0{}", " l1 1".repeat(200));
        let groups: String = (1..=4)
            .map(|level| {
                format!(
                    r##"<g id="a{level}">{}</g>"##,
                    format!(r##"<use href="#a{}"/>"##, level - 1).repeat(10)
                )
            })
            .collect();
        let svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50"><defs><path id="a0" d="{leaf}"/>{groups}</defs><use href="#a4"/></svg>"##
        );
        let picture = normalize(svg.into_bytes()).expect("a PNG");
        assert_eq!(picture.format, PictureFormat::Png);
        assert!(picture.data.starts_with(PNG_MAGIC));
    }

    /// A metafile becomes SVG; for the page layout, so does the picture
    /// without its lines of text.
    #[test]
    fn a_metafile_becomes_svg() {
        // A placeable WMF (0,0,1000,500 at 1000 units per inch): a window,
        // a rectangle, a line of text, the end.
        let mut wmf = Vec::new();
        wmf.extend_from_slice(&0x9AC6_CDD7u32.to_le_bytes());
        for value in [0u16, 0, 0, 1000, 500, 1000, 0, 0, 0] {
            wmf.extend_from_slice(&value.to_le_bytes());
        }
        for value in [1u16, 9, 0x300, 0, 0, 3, 0, 0, 0] {
            wmf.extend_from_slice(&value.to_le_bytes());
        }
        let mut record = |function: u16, params: &[i16]| {
            wmf.extend_from_slice(&(3 + params.len() as u32).to_le_bytes());
            wmf.extend_from_slice(&function.to_le_bytes());
            for param in params {
                wmf.extend_from_slice(&param.to_le_bytes());
            }
        };
        record(0x020B, &[0, 0]);
        record(0x020C, &[500, 1000]);
        record(0x041B, &[400, 900, 100, 100]);
        record(0x0521, &[2, i16::from_le_bytes(*b"Hi"), 300, 200]);
        record(0, &[]);
        let picture = normalize(wmf.clone()).expect("an SVG");
        assert_eq!(picture.format, PictureFormat::Svg);
        let image = for_layout(wmf).expect("an SVG");
        assert_eq!(image.format, PictureFormat::Svg);
        let bare = String::from_utf8(image.bare.expect("the picture without its text")).unwrap();
        assert!(bare.starts_with("<svg") && bare.contains("<path"), "{bare}");
        assert_eq!(image.texts.len(), 1);
    }
}
