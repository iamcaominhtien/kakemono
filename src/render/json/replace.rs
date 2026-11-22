//! Replacing the picture of a figure with the `replace_image` of an edited
//! JSON file (see [`super::apply`]): a PNG, JPEG or SVG file, named by a
//! path relative to the JSON file or given as a data URI. The picture is
//! stored in [`Document::pictures`] and the figure's [`ImageSpec`] points
//! at it, keeping its place on the page; every output then draws it.

use super::apply::ContentReport;
use crate::model::{BlockKind, Document, Picture};
use crate::render::assets::{PictureData, picture_data};
use base64::Engine;
use std::path::Path;

/// Largest relative difference between the aspect (width / height) of a
/// replacement and that of the figure's box before a warning: the picture
/// is still used, fitted into the box and centred in it.
const REPLACE_ASPECT_TOLERANCE_RATIO: f32 = 0.02;

/// Longest side of a replacement (pixels of a PNG or JPEG, units of an
/// SVG) that is accepted: a larger one could not be drawn in memory.
const MAX_REPLACEMENT_SIDE_PX: f32 = 20_000.0;

/// Replaces the picture of the block at `idx` of `doc` with the picture
/// `value` names (a data URI, or a path relative to `base`). Returns
/// whether it was replaced; a problem is a warning in `report`, and the
/// block keeps its picture.
pub(crate) fn apply_replacement(
    doc: &mut Document,
    idx: usize,
    value: &str,
    base: Option<&Path>,
    report: &mut ContentReport,
) -> bool {
    let id = doc.blocks[idx].id;
    let BlockKind::Figure { image: Some(spec), .. } = &doc.blocks[idx].kind else {
        report.warnings.push(format!(
            "block {id}: replace_image is ignored: the block is not a figure with a picture"
        ));
        return false;
    };
    let (box_width, box_height) = (spec.rect.width(), spec.rect.height());
    let (data, folder) = match load(value, base) {
        Ok(loaded) => loaded,
        Err(why) => {
            report
                .warnings
                .push(format!("block {id}: replace_image {value}: {why}"));
            return false;
        }
    };
    let picture = Picture {
        jpeg: data.starts_with(&[0xFF, 0xD8, 0xFF]),
        data,
    };
    let Some((kind, size)) = picture_data(&picture).and_then(|kind| picture_size(&kind).map(|size| (kind, size)))
    else {
        report.warnings.push(format!(
            "block {id}: replace_image {value} is not a PNG, JPEG or SVG picture"
        ));
        return false;
    };
    if size.0.max(size.1) > MAX_REPLACEMENT_SIDE_PX {
        report.warnings.push(format!(
            "block {id}: replace_image {value} is too large ({:.0}×{:.0}; at most {MAX_REPLACEMENT_SIDE_PX} a side)",
            size.0, size.1
        ));
        return false;
    }
    // A PNG or JPEG whose header is right may still be cut or damaged.
    if let PictureData::Png(data) | PictureData::Jpeg(data) = &kind
        && image::load_from_memory(data).is_err()
    {
        report.warnings.push(format!(
            "block {id}: replace_image {value} is damaged and cannot be read"
        ));
        return false;
    }
    let (aspect, box_aspect) = (size.0 / size.1, box_width / box_height.max(f32::EPSILON));
    let other_shape = (aspect / box_aspect - 1.0).abs() > REPLACE_ASPECT_TOLERANCE_RATIO;
    if other_shape {
        report.warnings.push(format!(
            "block {id}: the aspect of replace_image {value} ({:.0}×{:.0}) differs from the figure's \
             ({box_width:.0}×{box_height:.0} pt): it is fitted into the figure's box",
            size.0, size.1
        ));
    }
    // An SVG always, and a picture of another shape, become an SVG of the
    // figure's box: drawn at the size of the figure, never stretched, with
    // the pictures an SVG names kept inside it.
    let picture = match kind {
        PictureData::Png(_) | PictureData::Jpeg(_) if !other_shape => picture,
        kind => match fitted(&kind, folder.as_deref(), box_width, box_height) {
            Some(svg) => Picture {
                data: svg.into_bytes(),
                jpeg: false,
            },
            None => {
                report
                    .warnings
                    .push(format!("block {id}: replace_image {value} cannot be drawn"));
                return false;
            }
        },
    };
    doc.pictures.push(picture);
    let index = doc.pictures.len() - 1;
    if let BlockKind::Figure { image: Some(spec), .. } = &mut doc.blocks[idx].kind {
        spec.embedded = Some(index);
    }
    true
}

/// `picture` in an SVG of `width` by `height` (the figure's box, in
/// points), fitted and centred. An SVG's text is kept (it becomes paths
/// where it is written out) and the pictures it names, relative to
/// `folder`, are put inside it: its content is nested as an `<svg>`
/// element, not as an `<image>`, in which pictures would not be drawn.
/// `None` when it cannot be read.
fn fitted(picture: &PictureData, folder: Option<&Path>, width: f32, height: f32) -> Option<String> {
    // The picture, and how the nested <svg> fits its own size into the box.
    let (content, fit) = match picture {
        PictureData::Png(data) | PictureData::Jpeg(data) => {
            let mime = if matches!(picture, PictureData::Jpeg(_)) {
                "image/jpeg"
            } else {
                "image/png"
            };
            let href = format!(
                "data:{mime};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(data)
            );
            let image = format!(
                "<image width=\"{width}\" height=\"{height}\" preserveAspectRatio=\"xMidYMid meet\" xlink:href=\"{href}\"/>"
            );
            (image, String::new())
        }
        PictureData::Svg(svg) => {
            let options = resvg::usvg::Options {
                fontdb: crate::backend::render::svg_fonts(),
                resources_dir: folder.map(Path::to_path_buf),
                ..Default::default()
            };
            let tree = resvg::usvg::Tree::from_str(svg, &options).ok()?;
            let write = resvg::usvg::WriteOptions {
                preserve_text: true,
                ..Default::default()
            };
            let out = tree.to_string(&write);
            // The content between the root element's tags.
            let body = out[out.find('>')? + 1..out.rfind("</svg>")?].to_string();
            let size = tree.size();
            let fit = format!(
                " viewBox=\"0 0 {} {}\" preserveAspectRatio=\"xMidYMid meet\"",
                size.width(),
                size.height()
            );
            (body, fit)
        }
    };
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\
         <svg x=\"0\" y=\"0\" width=\"{width}\" height=\"{height}\"{fit}>{content}</svg></svg>"
    ))
}

/// The bytes `value` names: a data URI's payload (base64 or URL-encoded),
/// or the file at that path, relative to `base`; with the folder of that
/// file, where the pictures an SVG names are looked for.
fn load(value: &str, base: Option<&Path>) -> Result<(Vec<u8>, Option<std::path::PathBuf>), String> {
    if let Some(uri) = value.strip_prefix("data:") {
        let (head, payload) = uri.split_once(',').ok_or("a data URI without a comma")?;
        let is_base64 = head.split(';').any(|part| part.trim().eq_ignore_ascii_case("base64"));
        let data = if is_base64 {
            // Pasted data URIs are often cut into lines.
            let payload: String = payload.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .map_err(|err| format!("bad base64: {err}"))?
        } else {
            percent_decoded(payload)
        };
        return Ok((data, base.map(Path::to_path_buf)));
    }
    let path = Path::new(value);
    let path = match base {
        _ if path.is_absolute() => path.to_path_buf(),
        Some(base) => base.join(path),
        None => {
            return Err("a relative path, but the JSON's folder is not known (use Paper::apply_content_in)".into());
        }
    };
    let data = std::fs::read(&path).map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    Ok((data, path.parent().map(Path::to_path_buf)))
}

/// `text` with its `%XX` escapes decoded.
fn percent_decoded(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[at], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// Width and height of a picture: in pixels for a PNG or JPEG, in SVG
/// units for an SVG; `None` when it cannot be read.
fn picture_size(picture: &PictureData) -> Option<(f32, f32)> {
    let raster = |data: &[u8]| {
        image::ImageReader::new(std::io::Cursor::new(data))
            .with_guessed_format()
            .ok()?
            .into_dimensions()
            .ok()
            .map(|(width, height)| (width as f32, height as f32))
    };
    match picture {
        PictureData::Png(data) | PictureData::Jpeg(data) => raster(data),
        PictureData::Svg(svg) => {
            let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).ok()?;
            Some((tree.size().width(), tree.size().height()))
        }
    }
}
