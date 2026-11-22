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
/// is still used, but stretched or fitted into the box.
const REPLACE_ASPECT_TOLERANCE_RATIO: f32 = 0.02;

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
    let data = match load(value, base) {
        Ok(data) => data,
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
    let Some(size) = picture_data(&picture).and_then(|kind| picture_size(&kind)) else {
        report.warnings.push(format!(
            "block {id}: replace_image {value} is not a PNG, JPEG or SVG picture"
        ));
        return false;
    };
    let (aspect, box_aspect) = (size.0 / size.1, box_width / box_height.max(f32::EPSILON));
    if (aspect / box_aspect - 1.0).abs() > REPLACE_ASPECT_TOLERANCE_RATIO {
        report.warnings.push(format!(
            "block {id}: the aspect of replace_image {value} ({:.0}×{:.0}) differs from the figure's \
             ({box_width:.0}×{box_height:.0} pt): it is fitted into the figure's box",
            size.0, size.1
        ));
    }
    doc.pictures.push(picture);
    let index = doc.pictures.len() - 1;
    if let BlockKind::Figure { image: Some(spec), .. } = &mut doc.blocks[idx].kind {
        spec.embedded = Some(index);
    }
    true
}

/// The bytes `value` names: a data URI's payload (base64 or URL-encoded),
/// or the file at that path, relative to `base`.
fn load(value: &str, base: Option<&Path>) -> Result<Vec<u8>, String> {
    if let Some(uri) = value.strip_prefix("data:") {
        let (head, payload) = uri.split_once(',').ok_or("a data URI without a comma")?;
        return if head.ends_with(";base64") {
            base64::engine::general_purpose::STANDARD
                .decode(payload.trim())
                .map_err(|err| format!("bad base64: {err}"))
        } else {
            Ok(percent_decoded(payload))
        };
    }
    let path = Path::new(value);
    let path = match base {
        _ if path.is_absolute() => path.to_path_buf(),
        Some(base) => base.join(path),
        None => {
            return Err("a relative path, but the JSON's folder is not known (use Paper::apply_content_in)".into());
        }
    };
    std::fs::read(&path).map_err(|err| format!("cannot read {}: {err}", path.display()))
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
