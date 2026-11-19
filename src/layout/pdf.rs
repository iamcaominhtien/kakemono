//! The laid-out pages written as a PDF: text with embedded font subsets
//! (and its Unicode, so it can be searched and copied), lines, rectangles,
//! pictures and links.

use super::display::{Destination, Item, Page, TextRun};
use super::fonts::FontBook;
use crate::word::fmt::model::{FImage, Rgb};
use anyhow::{Result, anyhow};
use krilla::color::rgb;
use krilla::destination::{Destination as PdfDestination, XyzDestination};
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::image::Image;
use krilla::page::PageSettings;
use krilla::paint::{Fill, Stroke, StrokeDash};
use krilla::text::{Font, GlyphId, KrillaGlyph};
use krilla::{Document, action::LinkAction, annotation::LinkAnnotation, annotation::Target, metadata::Metadata};
use std::collections::HashMap;

/// Slant of faked italics (tangent of the angle).
const FAKE_ITALIC_SLANT: f32 = 0.2;
/// Width of the stroke that fakes bold, as a share of the font size.
const FAKE_BOLD_STROKE_RATIO: f32 = 0.02;

/// Writes `pages` as a PDF.
///
/// # Errors
/// When krilla cannot write the document (a damaged font or picture).
pub fn write(
    pages: &[Page],
    destinations: &[Destination],
    book: &FontBook,
    images: &[FImage],
    title: Option<&str>,
) -> Result<Vec<u8>> {
    let mut document = Document::new();
    let mut fonts: HashMap<usize, Font> = HashMap::new();
    let mut pictures: HashMap<usize, Option<Image>> = HashMap::new();
    let bookmarks: HashMap<&str, &Destination> = destinations.iter().map(|dest| (dest.name.as_str(), dest)).collect();
    for page in pages {
        let Some(settings) = PageSettings::from_wh(page.width.max(1.0), page.height.max(1.0)) else {
            continue;
        };
        let mut pdf_page = document.start_page_with(settings);
        let mut links = Vec::new();
        {
            let mut surface = pdf_page.surface();
            for item in &page.items {
                match item {
                    Item::Text(run) => {
                        let font = fonts.entry(run.face).or_insert_with(|| {
                            let face = book.get(run.face);
                            Font::new(face.data.clone().into(), face.index).expect("a font that was shaped loads")
                        });
                        draw_text(&mut surface, run, font.clone());
                    }
                    Item::Rect { x, y, w, h, color } => {
                        let Some(rect) = Rect::from_xywh(*x, *y, w.max(0.01), h.max(0.01)) else {
                            continue;
                        };
                        let mut path = PathBuilder::new();
                        path.push_rect(rect);
                        let Some(path) = path.finish() else { continue };
                        surface.set_fill(Some(fill(*color)));
                        surface.set_stroke(None);
                        surface.draw_path(&path);
                    }
                    Item::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        width,
                        color,
                        dash,
                    } => {
                        let mut path = PathBuilder::new();
                        path.move_to(*x1, *y1);
                        path.line_to(*x2, *y2);
                        let Some(path) = path.finish() else { continue };
                        surface.set_fill(None);
                        surface.set_stroke(Some(Stroke {
                            paint: rgb::Color::new(color[0], color[1], color[2]).into(),
                            width: *width,
                            dash: dash.map(|dash| StrokeDash {
                                array: vec![dash, dash],
                                offset: 0.0,
                            }),
                            ..Default::default()
                        }));
                        surface.draw_path(&path);
                    }
                    Item::Image {
                        index,
                        x,
                        y,
                        w,
                        h,
                        crop,
                    } => {
                        let picture = pictures.entry(*index).or_insert_with(|| {
                            let image = images.get(*index)?;
                            let data = image.data.clone().into();
                            if image.jpeg {
                                Image::from_jpeg(data, true).ok()
                            } else {
                                Image::from_png(data, true).ok()
                            }
                        });
                        let Some(picture) = picture.clone() else { continue };
                        draw_image(&mut surface, picture, *x, *y, *w, *h, *crop);
                    }
                    Item::Link { x, y, w, h, target } => links.push((*x, *y, *w, *h, target.clone())),
                }
            }
            surface.finish();
        }
        for (x, y, w, h, target) in links {
            let Some(rect) = Rect::from_xywh(x, y, w.max(0.1), h.max(0.1)) else {
                continue;
            };
            let target = if let Some(name) = target.strip_prefix('#') {
                let Some(dest) = bookmarks.get(name) else { continue };
                Target::Destination(PdfDestination::Xyz(XyzDestination::new(
                    dest.page,
                    Point::from_xy(0.0, dest.y),
                )))
            } else {
                Target::Action(LinkAction::new(target).into())
            };
            pdf_page.add_annotation(LinkAnnotation::new(rect, target).into());
        }
        pdf_page.finish();
    }
    let mut metadata = Metadata::new().creator("kakemono".to_string());
    if let Some(title) = title {
        metadata = metadata.title(title.to_string());
    }
    document.set_metadata(metadata);
    document.finish().map_err(|err| anyhow!("writing the PDF: {err:?}"))
}

/// A solid fill of colour `color`.
fn fill(color: Rgb) -> Fill {
    Fill {
        paint: rgb::Color::new(color[0], color[1], color[2]).into(),
        ..Default::default()
    }
}

/// Draws a text run.
fn draw_text(surface: &mut krilla::surface::Surface<'_>, run: &TextRun, font: Font) {
    if run.glyphs.is_empty() || run.size <= 0.0 {
        return;
    }
    let size = run.size;
    let scale = if run.scale > 0.0 { run.scale } else { 1.0 };
    // Glyph advances are in points; krilla wants them relative to the size.
    let glyphs: Vec<KrillaGlyph> = run
        .glyphs
        .iter()
        .enumerate()
        .map(|(at, glyph)| {
            let next = run.glyphs.get(at + 1).map_or(glyph.x + glyph.advance, |next| next.x);
            KrillaGlyph {
                glyph_id: GlyphId::new(u32::from(glyph.id)),
                text_range: glyph.text.0..glyph.text.1,
                x_advance: (next - glyph.x) / size / scale,
                x_offset: glyph.dx / size / scale,
                y_offset: -glyph.dy / size,
                y_advance: 0.0,
                location: None,
            }
        })
        .collect();
    let start = run.glyphs.first().map_or(0.0, |glyph| glyph.x);
    let needs_transform = run.fake_italic || (scale - 1.0).abs() > f32::EPSILON;
    let origin_x = run.x + start;
    if needs_transform {
        let skew = if run.fake_italic { -FAKE_ITALIC_SLANT } else { 0.0 };
        surface.push_transform(&Transform::from_row(scale, 0.0, skew, 1.0, origin_x, run.y));
    }
    let point = if needs_transform {
        Point::from_xy(0.0, 0.0)
    } else {
        Point::from_xy(origin_x, run.y)
    };
    surface.set_fill(Some(fill(run.color)));
    if run.fake_bold {
        surface.set_stroke(Some(Stroke {
            paint: rgb::Color::new(run.color[0], run.color[1], run.color[2]).into(),
            width: size * FAKE_BOLD_STROKE_RATIO,
            ..Default::default()
        }));
    } else {
        surface.set_stroke(None);
    }
    surface.draw_glyphs(point, &glyphs, font, &run.text, size, false);
    surface.set_stroke(None);
    if needs_transform {
        surface.pop();
    }
}

/// Draws a picture in a box, with its cropping.
fn draw_image(
    surface: &mut krilla::surface::Surface<'_>,
    image: Image,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    crop: [f32; 4],
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let [left, top, right, bottom] = crop;
    let shown_w = 1.0 - left - right;
    let shown_h = 1.0 - top - bottom;
    if shown_w <= 0.0 || shown_h <= 0.0 {
        return;
    }
    let full_w = w / shown_w;
    let full_h = h / shown_h;
    let cropped = left != 0.0 || top != 0.0 || right != 0.0 || bottom != 0.0;
    if cropped {
        let Some(rect) = Rect::from_xywh(x, y, w, h) else {
            return;
        };
        let mut path = PathBuilder::new();
        path.push_rect(rect);
        let Some(path) = path.finish() else { return };
        surface.push_clip_path(&path, &krilla::paint::FillRule::NonZero);
    }
    surface.push_transform(&Transform::from_translate(x - left * full_w, y - top * full_h));
    if let Some(size) = Size::from_wh(full_w, full_h) {
        surface.draw_image(image, size);
    }
    surface.pop();
    if cropped {
        surface.pop();
    }
}
