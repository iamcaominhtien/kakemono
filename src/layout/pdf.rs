//! The laid-out pages written as a PDF: text with embedded font subsets
//! (and its Unicode, so it can be searched and copied), lines, rectangles,
//! pictures and links.

use super::display::{Destination, Item, Page, TextRun};
use super::fonts::FontBook;
use crate::model::PictureFormat;
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
/// Two horizontal scales closer than this are the same, and a scale this
/// close to 1 is none: the scale of a stand-in face is a ratio of widths
/// that comes out as 1.0000001 or 0.9999999 for a face of the same widths,
/// a difference no one can see.
const SCALE_TOLERANCE: f32 = 1e-4;

/// The fonts and pictures the items draw from, and those already loaded
/// into the PDF, so that each is loaded once.
struct Loaded<'a> {
    /// The fonts that text runs name.
    book: &'a FontBook,
    /// The document's pictures.
    images: &'a [FImage],
    /// Fonts loaded so far, by face.
    fonts: HashMap<usize, Font>,
    /// Pictures loaded so far, by index (`None` for one that does not decode).
    pictures: HashMap<usize, Option<LoadedPicture>>,
}

/// A picture loaded for the PDF, once for every place it is drawn at.
enum LoadedPicture {
    /// A bitmap.
    Raster(Image),
    /// An SVG drawing, parsed.
    Vector(Box<resvg::usvg::Tree>),
}

impl LoadedPicture {
    /// Draws the picture on `surface` at its origin, `size` large.
    fn draw(&self, surface: &mut krilla::surface::Surface<'_>, size: Size) {
        match self {
            Self::Raster(image) => surface.draw_image(image.clone(), size),
            Self::Vector(tree) => crate::backend::svg::draw_vector(surface, tree, size),
        }
    }
}

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
    let mut loaded = Loaded {
        book,
        images,
        fonts: HashMap::new(),
        pictures: HashMap::new(),
    };
    let bookmarks: HashMap<&str, &Destination> = destinations.iter().map(|dest| (dest.name.as_str(), dest)).collect();
    for page in pages {
        let Some(settings) = PageSettings::from_wh(page.width.max(1.0), page.height.max(1.0)) else {
            continue;
        };
        let mut pdf_page = document.start_page_with(settings);
        let mut links = Vec::new();
        {
            let mut surface = pdf_page.surface();
            // Consecutive runs that continue one another are drawn as one
            // (one text object per line of a face, instead of one per word
            // and per space): a link is drawn apart, after the page, so it
            // does not end a group; anything else drawn does.
            let mut group: Vec<&TextRun> = Vec::new();
            for item in &page.items {
                match item {
                    Item::Link {
                        x,
                        y,
                        width,
                        height,
                        target,
                    } => links.push((*x, *y, *width, *height, target.clone())),
                    Item::Text(run) if run.glyphs.is_empty() || run.size <= 0.0 => {}
                    Item::Text(run) => {
                        if group.last().is_some_and(|last| !continues(last, run)) {
                            draw_runs(&mut surface, &group, &mut loaded);
                            group.clear();
                        }
                        group.push(run);
                    }
                    _ => {
                        draw_runs(&mut surface, &group, &mut loaded);
                        group.clear();
                        draw_item(&mut surface, item, &mut loaded);
                    }
                }
            }
            draw_runs(&mut surface, &group, &mut loaded);
            surface.finish();
        }
        for (x, y, width, height, target) in links {
            add_link(&mut pdf_page, [x, y, width, height], &target, &bookmarks);
        }
        pdf_page.finish();
    }
    let mut metadata = Metadata::new().creator("kakemono".to_string());
    if let Some(title) = title {
        metadata = metadata.title(title.to_string());
    }
    document.set_metadata(metadata);
    let mut bytes = document.finish().map_err(|err| anyhow!("writing the PDF: {err:?}"))?;
    clear_small_caps(&mut bytes);
    Ok(bytes)
}

/// Whether the run `next` continues the run `run` drawn just before it:
/// the same face, size, colour, faked styles and horizontal scale, on the
/// same baseline, so that both can be drawn as one.
fn continues(run: &TextRun, next: &TextRun) -> bool {
    run.face == next.face
        && run.size == next.size
        && run.y == next.y
        && run.color == next.color
        && run.fake_bold == next.fake_bold
        && run.fake_italic == next.fake_italic
        && (run.scale - next.scale).abs() < SCALE_TOLERANCE
}

/// Draws a group of runs that continue one another (none when `runs` is
/// empty), in the face of the first.
fn draw_runs(surface: &mut krilla::surface::Surface<'_>, runs: &[&TextRun], loaded: &mut Loaded<'_>) {
    let Some(first) = runs.first() else { return };
    let book = loaded.book;
    let font = loaded.fonts.entry(first.face).or_insert_with(|| {
        let face = book.get(first.face);
        Font::new(face.data.clone().into(), face.index).expect("a font that was shaped loads")
    });
    draw_text(surface, runs, font.clone());
}

/// Draws one item that is neither text nor a link on a page's surface.
fn draw_item(surface: &mut krilla::surface::Surface<'_>, item: &Item, loaded: &mut Loaded<'_>) {
    match item {
        Item::Text(_) | Item::Link { .. } => {}
        Item::Rect {
            x,
            y,
            width,
            height,
            color,
        } => {
            let Some(rect) = Rect::from_xywh(*x, *y, width.max(0.01), height.max(0.01)) else {
                return;
            };
            let mut path = PathBuilder::new();
            path.push_rect(rect);
            let Some(path) = path.finish() else { return };
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
            let Some(path) = path.finish() else { return };
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
            width,
            height,
            crop,
        } => {
            let images = loaded.images;
            let picture = loaded
                .pictures
                .entry(*index)
                .or_insert_with(|| load_picture(images.get(*index)?));
            let Some(picture) = picture.as_ref() else { return };
            draw_image(surface, picture, *x, *y, *width, *height, *crop);
        }
    }
}

/// Adds a link annotation over `area` (left, top, width, height) to a page:
/// to a bookmark of `bookmarks` when `target` is `#name`, else to a web
/// address. A link to an unknown bookmark is left out.
fn add_link(
    pdf_page: &mut krilla::page::Page<'_>,
    area: [f32; 4],
    target: &str,
    bookmarks: &HashMap<&str, &Destination>,
) {
    let [x, y, width, height] = area;
    let Some(rect) = Rect::from_xywh(x, y, width.max(0.1), height.max(0.1)) else {
        return;
    };
    let target = if let Some(name) = target.strip_prefix('#') {
        let Some(dest) = bookmarks.get(name) else { return };
        Target::Destination(PdfDestination::Xyz(XyzDestination::new(
            dest.page,
            Point::from_xy(0.0, dest.y),
        )))
    } else {
        Target::Action(LinkAction::new(target.to_string()).into())
    };
    pdf_page.add_annotation(LinkAnnotation::new(rect, target).into());
}

/// A solid fill of colour `color`.
fn fill(color: Rgb) -> Fill {
    Fill {
        paint: rgb::Color::new(color[0], color[1], color[2]).into(),
        ..Default::default()
    }
}

/// Draws runs that continue one another ([`continues`]; each with glyphs)
/// as one: their glyphs one after another, each placed at its own pen
/// position, and their text joined.
fn draw_text(surface: &mut krilla::surface::Surface<'_>, runs: &[&TextRun], font: Font) {
    let Some(&first) = runs.first() else { return };
    let size = first.size;
    let scale = if first.scale > 0.0 { first.scale } else { 1.0 };
    let mut text = String::new();
    // Each glyph's pen position on the page and its text range in `text`.
    let mut placed = Vec::new();
    for run in runs {
        let base = text.len();
        text.push_str(&run.text);
        for glyph in &run.glyphs {
            placed.push((run.x + glyph.x, glyph, base));
        }
    }
    // Glyph advances are in points; krilla wants them relative to the size.
    let glyphs: Vec<KrillaGlyph> = placed
        .iter()
        .enumerate()
        .map(|(at, &(x, glyph, base))| {
            let next = placed.get(at + 1).map_or(x + glyph.advance, |&(next, _, _)| next);
            KrillaGlyph {
                glyph_id: GlyphId::new(u32::from(glyph.id)),
                text_range: base + glyph.text.0..base + glyph.text.1,
                x_advance: (next - x) / size / scale,
                x_offset: glyph.dx / size / scale,
                y_offset: -glyph.dy / size,
                y_advance: 0.0,
                location: None,
            }
        })
        .collect();
    let origin_x = placed.first().map_or(first.x, |&(x, _, _)| x);
    let needs_transform = first.fake_italic || (scale - 1.0).abs() > SCALE_TOLERANCE;
    if needs_transform {
        let skew = if first.fake_italic { -FAKE_ITALIC_SLANT } else { 0.0 };
        surface.push_transform(&Transform::from_row(scale, 0.0, skew, 1.0, origin_x, first.y));
    }
    let point = if needs_transform {
        Point::from_xy(0.0, 0.0)
    } else {
        Point::from_xy(origin_x, first.y)
    };
    surface.set_fill(Some(fill(first.color)));
    if first.fake_bold {
        surface.set_stroke(Some(Stroke {
            paint: rgb::Color::new(first.color[0], first.color[1], first.color[2]).into(),
            width: size * FAKE_BOLD_STROKE_RATIO,
            ..Default::default()
        }));
    } else {
        surface.set_stroke(None);
    }
    surface.draw_glyphs(point, &glyphs, font, &text, size, false);
    surface.set_stroke(None);
    if needs_transform {
        surface.pop();
    }
}

/// Draws a picture in a box, with its cropping.
fn draw_image(
    surface: &mut krilla::surface::Surface<'_>,
    image: &LoadedPicture,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    crop: [f32; 4],
) {
    if width <= 0.0 || height <= 0.0 {
        return;
    }
    let [left, top, right, bottom] = crop;
    let shown_w = 1.0 - left - right;
    let shown_h = 1.0 - top - bottom;
    if shown_w <= 0.0 || shown_h <= 0.0 {
        return;
    }
    let full_w = width / shown_w;
    let full_h = height / shown_h;
    let cropped = left != 0.0 || top != 0.0 || right != 0.0 || bottom != 0.0;
    if cropped {
        let Some(rect) = Rect::from_xywh(x, y, width, height) else {
            return;
        };
        let mut path = PathBuilder::new();
        path.push_rect(rect);
        let Some(path) = path.finish() else { return };
        surface.push_clip_path(&path, &krilla::paint::FillRule::NonZero);
    }
    surface.push_transform(&Transform::from_translate(x - left * full_w, y - top * full_h));
    if let Some(size) = Size::from_wh(full_w, full_h) {
        image.draw(surface, size);
    }
    surface.pop();
    if cropped {
        surface.pop();
    }
}

/// The picture `image` loaded for the PDF: a metafile without its lines
/// of text, which are drawn as text over it; an SVG as vector, or as a PNG
/// when it has filters (krilla draws each filter as a bitmap of up to 25
/// million pixels per nested group); `None` when it does not decode.
fn load_picture(image: &FImage) -> Option<LoadedPicture> {
    fn vector(data: &[u8]) -> Option<LoadedPicture> {
        let tree = crate::backend::parse_svg(std::str::from_utf8(data).ok()?)?;
        if tree.filters().is_empty() {
            return Some(LoadedPicture::Vector(Box::new(tree)));
        }
        let png = crate::backend::render::png_of_tree(&tree, crate::backend::svg::SVG_PNG_SCALE)?;
        Image::from_png(png.into(), true).ok().map(LoadedPicture::Raster)
    }
    if let Some(bare) = &image.bare {
        return vector(bare);
    }
    let data = || image.data.clone().into();
    match image.format {
        PictureFormat::Svg => vector(&image.data),
        PictureFormat::Jpeg => Image::from_jpeg(data(), true).ok().map(LoadedPicture::Raster),
        PictureFormat::Png => Image::from_png(data(), true).ok().map(LoadedPicture::Raster),
    }
}

/// Font descriptors flag their font as small capitals.
const SMALL_CAP_FLAG: u32 = 1 << 17;

/// Removes the small-capitals flag krilla gives every font (readers of the
/// PDF, kakemono's own among them, would show its text in small capitals).
/// The number is rewritten in place, padded with spaces, so that no offset
/// of the file moves.
fn clear_small_caps(bytes: &mut [u8]) {
    const KEY: &[u8] = b"/Flags ";
    let mut at = 0;
    while let Some(found) = bytes[at..].windows(KEY.len()).position(|window| window == KEY) {
        let start = at + found + KEY.len();
        let end = start + bytes[start..].iter().take_while(|byte| byte.is_ascii_digit()).count();
        let value = std::str::from_utf8(&bytes[start..end])
            .ok()
            .and_then(|text| text.parse::<u32>().ok());
        if let Some(value) = value.filter(|value| value & SMALL_CAP_FLAG != 0) {
            let text = format!("{:<width$}", value & !SMALL_CAP_FLAG, width = end - start);
            bytes[start..end].copy_from_slice(text.as_bytes());
        }
        at = end.max(start);
    }
}

#[cfg(test)]
mod tests {
    use super::{TextRun, continues};

    /// A run of plain black text at 11 pt on baseline 100, at pen `x`.
    fn run(x: f32, text: &str) -> TextRun {
        TextRun {
            face: 0,
            size: 11.0,
            x,
            y: 100.0,
            color: [0, 0, 0],
            glyphs: Vec::new(),
            text: text.to_string(),
            fake_bold: false,
            fake_italic: false,
            scale: 1.0,
        }
    }

    #[test]
    fn runs_of_one_face_on_one_baseline_continue() {
        let word = run(10.0, "word");
        let mut space = run(30.0, " ");
        // A stand-in face of the same widths: a scale of 1 to within
        // rounding is no scale.
        space.scale = 1.000_000_1;
        assert!(continues(&word, &space));
        let mut other_face = run(30.0, "x");
        other_face.face = 1;
        assert!(!continues(&word, &other_face));
        let mut next_line = run(10.0, "x");
        next_line.y = 114.0;
        assert!(!continues(&word, &next_line));
        let mut red = run(30.0, "x");
        red.color = [200, 0, 0];
        assert!(!continues(&word, &red));
        let mut narrowed = run(30.0, "x");
        narrowed.scale = 0.95;
        assert!(!continues(&word, &narrowed));
    }
}
