//! Pictures used by the HTML (page backgrounds, figures, equations), drawn
//! after the HTML is written, on several threads.

use super::*;
use crate::backend::PdfFile;
use crate::backend::{PageRenderer, svg_to_png};
use crate::model::*;
use base64::Engine;
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::{Arc, OnceLock};

/// Marks a placeholder; a control character never written to the HTML.
pub(crate) const MARK: char = '\u{1}';

/// Grid steps per PDF point used to match glyph origins: glyphs drawn as
/// HTML are identified by their origin rounded to a quarter point.
const ORIGIN_STEPS_PER_PT: f32 = 4.0;

/// Highest pixels-per-point scale of a PNG page background: whole pages at
/// a higher scale make very large files for little visible gain.
const MAX_PAGE_PNG_SCALE: f32 = 2.5;

/// Largest SVG page background, in bytes. A page with a plot of tens of
/// thousands of points makes an SVG of tens of megabytes, which browsers
/// load and draw slowly: such a page is drawn as a PNG instead, when that is
/// smaller (a page that is mostly a photograph is not).
const MAX_PAGE_SVG_BYTES: usize = 1_500_000;
/// Pixels per PDF point of a page background drawn as a PNG because its
/// SVG was too large.
const HEAVY_PAGE_PNG_SCALE: f32 = 2.0;

/// Pictures drawn ahead of the one the HTML is written up to, per thread:
/// enough to keep every thread busy, few enough that the pictures of a
/// long document are never all held at once.
const DRAW_AHEAD_PER_THREAD: usize = 4;

/// Prefix of an SVG line that draws a raster picture.
const SVG_IMAGE_PREFIX: &str = "<image ";
/// Start of the `src` of a PNG picture in an SVG, up to the payload.
const PNG_PIXEL_MARK: &str = "data:image/png;base64,";
/// The stretched-pixel pictures that [`pixels_to_rects`] replaces have a
/// payload shorter than this (in base64 characters): a 1 by 1 PNG is about
/// 100 characters long.
const PIXEL_PAYLOAD_MAX_LEN: usize = 400;

/// A picture the HTML needs: a page region, or a whole page.
pub(crate) enum ImageJob {
    /// A region of a page (a figure or an equation), with a hint for the
    /// file name.
    Region(ImageSpec, String),
    /// A whole page with everything on it.
    Page(usize),
    /// A page without the glyphs whose origins (in quarter points) are in
    /// the set: their text is drawn as HTML.
    PageWithoutText(usize, HashSet<(i32, i32)>),
}

/// Collects the pictures the HTML needs. While the HTML is written, each
/// picture is only recorded and a placeholder takes the place of its
/// `src`; [`ImageStore::finish`] then draws all pictures on several threads
/// and puts their data (or file names) in place.
pub(crate) struct ImageStore<'r> {
    /// The output options (scale, inline or external images, threads).
    pub(crate) opts: &'r HtmlOptions,
    /// The pictures recorded so far; a placeholder holds an index into it.
    pub(crate) jobs: Vec<ImageJob>,
    /// An SVG picture is written as a PDF of one page (for LaTeX, whose
    /// engines read PDF pictures but not SVG).
    pub(crate) svg_as_pdf: bool,
}

/// A drawn picture: (data, mime type, file extension, name hint).
type DrawnImage = (Vec<u8>, &'static str, &'static str, String);

/// Where the pictures of a document come from: the pages of its PDF, and
/// the picture files stored in the document (a Word document has only
/// those).
#[derive(Clone)]
pub(crate) struct Pictures<'a> {
    /// The PDF the pages and page areas are drawn from; `None` for a Word
    /// document.
    pub(crate) pdf: Option<&'a PdfFile>,
    /// The stored pictures ([`Document::pictures`]).
    pub(crate) stored: &'a [Picture],
    /// The PNG and the PDF made of each stored SVG picture, each made at
    /// most once for an output, however many times the picture is used.
    renditions: Arc<Renditions>,
}

/// A file made of a stored picture, once made: `None` when it could not be.
type Rendition = OnceLock<Option<Arc<[u8]>>>;

/// The PNG and the PDF made of each stored picture, by index.
struct Renditions {
    /// The PNG (`backend::svg_to_png`).
    png: Vec<Rendition>,
    /// The PDF (`backend::svg_to_pdf`).
    pdf: Vec<Rendition>,
}

impl<'a> Pictures<'a> {
    /// The pictures of `doc`, drawn from `pdf` when it has one.
    pub(crate) fn new(pdf: Option<&'a PdfFile>, doc: &'a Document) -> Self {
        let empty = || (0..doc.pictures.len()).map(|_| OnceLock::new()).collect();
        Self {
            pdf,
            stored: &doc.pictures,
            renditions: Arc::new(Renditions {
                png: empty(),
                pdf: empty(),
            }),
        }
    }

    /// The PNG drawn of the stored SVG picture `index` (Word's fallback),
    /// made once; `None` when it is not an SVG or none can be drawn.
    pub(crate) fn svg_png(&self, index: usize) -> Option<Arc<[u8]>> {
        let svg = self.stored_svg(index)?;
        self.renditions.png[index]
            .get_or_init(|| svg_to_png(svg, crate::backend::svg::SVG_PNG_SCALE).map(Arc::from))
            .clone()
    }

    /// The PDF of one page made of the stored SVG picture `index` (for
    /// LaTeX), made once; `None` when it is not an SVG or none can be made.
    pub(crate) fn svg_pdf(&self, index: usize) -> Option<Arc<[u8]>> {
        let svg = self.stored_svg(index)?;
        self.renditions.pdf[index]
            .get_or_init(|| crate::backend::svg_to_pdf(svg).map(Arc::from))
            .clone()
    }

    /// The text of the stored picture `index` when it is an SVG.
    fn stored_svg(&self, index: usize) -> Option<&'a str> {
        let picture = self.stored.get(index)?;
        if picture.format != PictureFormat::Svg {
            return None;
        }
        std::str::from_utf8(&picture.data).ok()
    }

    /// A renderer of the pages of the PDF, for one thread; `None` without a
    /// PDF.
    pub(crate) fn renderer(&self) -> Option<PageRenderer<'a>> {
        self.pdf.map(PdfFile::renderer)
    }

    /// The picture of `spec` at `scale` pixels per point: a stored picture
    /// as it is (an SVG too), or the area of the page drawn by `renderer`
    /// (a PNG or a JPEG). Returns the data and its format, or `None` when
    /// it cannot be drawn.
    pub(crate) fn region(
        &self,
        renderer: Option<&PageRenderer<'_>>,
        spec: &ImageSpec,
        scale: f32,
    ) -> Option<(Vec<u8>, PictureFormat)> {
        match spec.embedded {
            Some(index) => self
                .stored
                .get(index)
                .map(|picture| (picture.data.clone(), picture.format)),
            None => renderer?
                .region_picture(spec.page, spec.rect, scale, spec.transparent)
                .map(|(data, jpeg)| (data, if jpeg { PictureFormat::Jpeg } else { PictureFormat::Png })),
        }
    }
}

impl ImageStore<'_> {
    /// Records a picture and returns the placeholder that stands for its
    /// `src` until [`ImageStore::finish`].
    pub(crate) fn add(&mut self, job: ImageJob) -> String {
        self.jobs.push(job);
        format!("{MARK}{}{MARK}", self.jobs.len() - 1)
    }

    /// Renders a page region and returns the `src` of the image.
    pub fn region(&mut self, spec: &ImageSpec, hint: &str) -> String {
        self.add(ImageJob::Region(*spec, hint.to_string()))
    }

    /// The `width` and `height` attributes of the picture of a page region
    /// (its pixel size): the browser keeps its place, at the right height,
    /// before it loads.
    pub fn region_size_attrs(&self, spec: &ImageSpec) -> String {
        let (width, height) = self.region_size(spec);
        format!(" width=\"{width}\" height=\"{height}\"")
    }

    /// The pixel size of the picture of a page region.
    pub fn region_size(&self, spec: &ImageSpec) -> (u32, u32) {
        let (_, width, height) = crate::backend::render::region_bitmap(spec.rect, self.opts.scale);
        (width, height)
    }

    /// The `src` of a picture of a page without the glyphs whose origins,
    /// rounded to a quarter point, are in `drawn` (or next to one of them).
    pub fn page_without_text(&mut self, page: usize, drawn: HashSet<(i32, i32)>) -> String {
        self.add(ImageJob::PageWithoutText(page, drawn))
    }

    /// The `src` of a picture of a whole page (hard mode background).
    pub fn page(&mut self, page: usize) -> String {
        self.add(ImageJob::Page(page))
    }

    /// Draws one picture: (data, mime type, file extension, name hint).
    /// Returns `None` when the backend cannot draw it.
    pub(crate) fn draw(
        &self,
        pictures: &Pictures<'_>,
        renderer: Option<&PageRenderer<'_>>,
        job: &ImageJob,
    ) -> Option<DrawnImage> {
        let page_hint = |page: usize| format!("page-{}", page + 1);
        match job {
            ImageJob::Region(spec, hint) => {
                if self.svg_as_pdf
                    && let Some(index) = spec.embedded
                    && pictures.stored_svg(index).is_some()
                {
                    return svg_as_pdf(pictures, index, hint);
                }
                let (data, format) = pictures.region(renderer, spec, self.opts.scale)?;
                Some((data, format.mime(), format.extension(), hint.clone()))
            }
            ImageJob::Page(page) => {
                let renderer = renderer?;
                match self.opts.page_image {
                    PageImage::Svg | PageImage::SvgOnly => {
                        let svg = renderer.page_svg(*page)?;
                        if self.too_heavy(&svg)
                            && let Some((png, _, _)) = renderer.page_png(*page, HEAVY_PAGE_PNG_SCALE)
                            && png.len() < svg.len()
                        {
                            return Some((png, "image/png", "png", page_hint(*page)));
                        }
                        Some((svg.into_bytes(), "image/svg+xml", "svg", page_hint(*page)))
                    }
                    PageImage::Png => {
                        let (png, _, _) = renderer.page_png(*page, self.opts.scale.min(MAX_PAGE_PNG_SCALE))?;
                        Some((png, "image/png", "png", page_hint(*page)))
                    }
                }
            }
            ImageJob::PageWithoutText(page, drawn) => {
                let renderer = renderer?;
                // A glyph is removed when its origin, or a grid position next
                // to it, was drawn as HTML (rounding may differ by a step).
                let remove = |x: f32, y: f32| {
                    let (qx, qy) = origin_key(x, y);
                    (-1..=1).any(|dx| (-1..=1).any(|dy| drawn.contains(&(qx + dx, qy + dy))))
                };
                let svg = pixels_to_rects(renderer.page_svg_without_text(*page, &remove)?);
                if self.opts.page_image == PageImage::Png {
                    let png = svg_to_png(&svg, self.opts.scale.min(MAX_PAGE_PNG_SCALE))?;
                    return Some((png, "image/png", "png", page_hint(*page)));
                }
                if self.too_heavy(&svg)
                    && let Some(png) = svg_to_png(&svg, HEAVY_PAGE_PNG_SCALE)
                    && png.len() < svg.len()
                {
                    return Some((png, "image/png", "png", page_hint(*page)));
                }
                Some((svg.into_bytes(), "image/svg+xml", "svg", page_hint(*page)))
            }
        }
    }

    /// Whether an SVG page background is too large to keep as SVG (see
    /// [`MAX_PAGE_SVG_BYTES`]); never with [`PageImage::SvgOnly`].
    fn too_heavy(&self, svg: &str) -> bool {
        self.opts.page_image == PageImage::Svg && svg.len() > MAX_PAGE_SVG_BYTES
    }

    /// Draws all recorded pictures and replaces the placeholders in `html`.
    ///
    /// # Panics
    ///
    /// When `html` contains a placeholder that is not closed or whose index
    /// is not a number (it was not written by [`ImageStore::add`]).
    pub(crate) fn finish(self, pictures: Pictures<'_>, html: String) -> HtmlOutput {
        let mut out = Vec::with_capacity(html.len());
        let mut files = Vec::new();
        self.finish_into(pictures, &html, &mut out, &mut |name, data| {
            files.push((name, data));
            Ok(())
        })
        .expect("writing to memory does not fail");
        let html = String::from_utf8(out).expect("the HTML and the picture sources are UTF-8");
        HtmlOutput { html, files }
    }

    /// Writes `html` to `out` with its placeholders replaced, drawing the
    /// pictures on several threads; each picture file (when pictures are
    /// not inlined) goes to `file` as (relative path, contents).
    ///
    /// When the placeholders come in the order the pictures were recorded,
    /// as the renderers write them, the HTML is written as the pictures are
    /// drawn, a few at a time ([`DRAW_AHEAD_PER_THREAD`]): a long document
    /// never has all its pictures in memory. Otherwise every picture is
    /// drawn first.
    ///
    /// # Errors
    /// The first error of `out` or `file`.
    ///
    /// # Panics
    ///
    /// When `html` contains a placeholder that is not closed or whose index
    /// is not a number (it was not written by [`ImageStore::add`]).
    pub(crate) fn finish_into(
        self,
        pictures: Pictures<'_>,
        html: &str,
        out: &mut dyn Write,
        file: &mut dyn FnMut(String, Vec<u8>) -> io::Result<()>,
    ) -> io::Result<()> {
        let mut writer = SourceWriter {
            store: &self,
            out,
            file,
            files: 0,
            written: HashMap::new(),
            base64: String::new(),
        };
        let mut rest = html;
        if placeholders_in_order(html, self.jobs.len()) {
            let threads = crate::common::parallel::resolve_jobs(self.opts.jobs);
            crate::common::parallel::map_ordered(
                self.jobs.len(),
                self.opts.jobs,
                threads * DRAW_AHEAD_PER_THREAD,
                || pictures.renderer(),
                |renderer, i| self.draw(&pictures, renderer.as_ref(), &self.jobs[i]),
                |_, image| -> io::Result<()> {
                    let (before, _, after) = next_placeholder(rest).expect("one placeholder per picture");
                    writer.out.write_all(before.as_bytes())?;
                    writer.put_image(image)?;
                    rest = after;
                    Ok(())
                },
            )?;
        } else {
            let drawn = crate::common::parallel::map(
                self.jobs.len(),
                self.opts.jobs,
                || pictures.renderer(),
                |renderer, i| self.draw(&pictures, renderer.as_ref(), &self.jobs[i]),
            );
            let mut drawn: Vec<_> = drawn.into_iter().map(Some).collect();
            while let Some((before, index, after)) = next_placeholder(rest) {
                writer.out.write_all(before.as_bytes())?;
                writer.put_image(drawn[index].take().flatten())?;
                rest = after;
            }
        }
        writer.out.write_all(rest.as_bytes())
    }
}

/// The stored SVG picture `index` of `pictures` as a PDF picture, still
/// vector, with the name hint `hint`; a PNG drawn of it when it cannot be
/// made one (see `backend::svg_to_png`), `None` when neither can.
fn svg_as_pdf(pictures: &Pictures<'_>, index: usize, hint: &str) -> Option<DrawnImage> {
    if let Some(pdf) = pictures.svg_pdf(index) {
        return Some((pdf.to_vec(), "application/pdf", "pdf", hint.to_string()));
    }
    let png = pictures.svg_png(index)?;
    Some((
        png.to_vec(),
        PictureFormat::Png.mime(),
        PictureFormat::Png.extension(),
        hint.to_string(),
    ))
}

/// The text before the next placeholder of `html`, the index in the
/// placeholder, and the text after it; `None` when there is none left.
///
/// # Panics
///
/// When the placeholder is not closed or its index is not a number.
fn next_placeholder(html: &str) -> Option<(&str, usize, &str)> {
    let start = html.find(MARK)?;
    let after = &html[start + MARK.len_utf8()..];
    let end = after.find(MARK).expect("placeholders are closed");
    let index = after[..end].parse().expect("placeholder index");
    Some((&html[..start], index, &after[end + MARK.len_utf8()..]))
}

/// Whether `html` holds the placeholders of `count` pictures, each once,
/// in the order they were recorded.
fn placeholders_in_order(html: &str, count: usize) -> bool {
    let mut rest = html;
    let mut expected = 0;
    while let Some((_, index, after)) = next_placeholder(rest) {
        if index != expected {
            return false;
        }
        expected += 1;
        rest = after;
    }
    expected == count
}

/// Writes the `src` of drawn pictures in place of their placeholders.
struct SourceWriter<'a, 'r> {
    /// The store the pictures were recorded in (for the output options).
    store: &'a ImageStore<'r>,
    /// Where the HTML goes.
    out: &'a mut dyn Write,
    /// Where the picture files go, when pictures are not inlined.
    file: &'a mut dyn FnMut(String, Vec<u8>) -> io::Result<()>,
    /// Picture files written so far (they are numbered).
    files: usize,
    /// The files written, by the hash and length of their contents: a
    /// picture used again refers to its file.
    written: HashMap<(u64, usize), String>,
    /// Buffer for the base64 text of an inlined picture.
    base64: String,
}

impl SourceWriter<'_, '_> {
    /// Writes the `src` of a drawn picture: a data URI, or the name of a
    /// new file in the asset directory (whose contents go to `file`). A
    /// picture that could not be drawn (`None`) leaves an empty `src`.
    fn put_image(&mut self, image: Option<DrawnImage>) -> io::Result<()> {
        let Some((data, mime, ext, hint)) = image else {
            return Ok(());
        };
        let opts = self.store.opts;
        if opts.inline_images {
            self.base64.clear();
            self.base64.push_str("data:");
            self.base64.push_str(mime);
            self.base64.push_str(";base64,");
            base64::engine::general_purpose::STANDARD.encode_string(&data, &mut self.base64);
            self.out.write_all(self.base64.as_bytes())
        } else {
            let key = (content_hash(&data), data.len());
            if let Some(name) = self.written.get(&key) {
                return self.out.write_all(name.as_bytes());
            }
            self.files += 1;
            let name = format!("{}/{:03}-{}.{}", opts.asset_dir, self.files, hint, ext);
            self.out.write_all(name.as_bytes())?;
            self.written.insert(key, name.clone());
            (self.file)(name, data)
        }
    }
}

/// A hash of the bytes `data`.
fn content_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

/// Replaces the pictures of a single pixel stretched over an area, which
/// some typesetters use for table rules, by filled rectangles of the same
/// colour. A browser draws a pixelated picture thinner than a pixel as
/// nothing at all, so the rules of such tables would disappear from the page
/// picture; a rectangle that thin is drawn as a faint line, like the PDF.
///
/// Relies on hayro's SVG layout: one `<image transform="..."
/// xlink:href="data:image/png;base64,..." width="1" height="1" .../>` per
/// line. Other pictures and other lines are left alone.
fn pixels_to_rects(svg: String) -> String {
    if !svg.contains(PNG_PIXEL_MARK) {
        return svg;
    }
    let mut colors: HashMap<&str, Option<String>> = HashMap::new();
    let mut out = String::with_capacity(svg.len());
    for line in svg.lines() {
        let rect = line
            .strip_prefix(SVG_IMAGE_PREFIX)
            .filter(|rest| rest.contains("width=\"1\" height=\"1\""))
            .and_then(|rest| {
                let transform = attribute(rest, "transform")?;
                let payload = rest[rest.find(PNG_PIXEL_MARK)? + PNG_PIXEL_MARK.len()..]
                    .split('"')
                    .next()?;
                let fill = colors.entry(payload).or_insert_with(|| pixel_fill(payload)).as_ref()?;
                Some(format!(
                    "<rect transform=\"{transform}\" width=\"1\" height=\"1\" {fill}/>"
                ))
            });
        out.push_str(rect.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

/// The value of the attribute `name` in the text of an SVG element.
fn attribute<'a>(element: &'a str, name: &str) -> Option<&'a str> {
    let start = element.find(&format!("{name}=\""))? + name.len() + 2;
    element[start..].split('"').next()
}

/// The `fill` attributes that colour a rectangle like the 1 by 1 PNG
/// `payload` (base64), or `None` when it is no such picture.
fn pixel_fill(payload: &str) -> Option<String> {
    if payload.len() > PIXEL_PAYLOAD_MAX_LEN {
        return None;
    }
    let png = base64::engine::general_purpose::STANDARD.decode(payload).ok()?;
    let img = image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok()?;
    if (img.width(), img.height()) != (1, 1) {
        return None;
    }
    let [red, green, blue, alpha] = img.to_rgba8().get_pixel(0, 0).0;
    let opacity = if alpha == u8::MAX {
        String::new()
    } else {
        format!(" fill-opacity=\"{:.3}\"", f32::from(alpha) / 255.0)
    };
    Some(format!("fill=\"rgb({red},{green},{blue})\"{opacity}"))
}

/// The key of a glyph origin (`x`, `y` in PDF points) in the sets of glyphs
/// drawn as HTML: the origin rounded to the nearest quarter point.
pub(crate) fn origin_key(x: f32, y: f32) -> (i32, i32) {
    (
        (x * ORIGIN_STEPS_PER_PT).round() as i32,
        (y * ORIGIN_STEPS_PER_PT).round() as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::{MARK, next_placeholder, placeholders_in_order};

    fn placeholder(index: usize) -> String {
        format!("{MARK}{index}{MARK}")
    }

    /// The placeholders are found one after another, with the text around
    /// them.
    #[test]
    fn placeholders_are_split_in_turn() {
        let html = format!(
            "<img src=\"{}\"><p>x</p><img src=\"{}\">",
            placeholder(0),
            placeholder(1)
        );
        let (before, index, rest) = next_placeholder(&html).unwrap();
        assert_eq!((before, index), ("<img src=\"", 0));
        let (before, index, rest) = next_placeholder(rest).unwrap();
        assert_eq!((before, index, rest), ("\"><p>x</p><img src=\"", 1, "\">"));
        assert!(next_placeholder(rest).is_none());
    }

    /// Only placeholders of every picture, each once and in order, let the
    /// pictures be written as they are drawn.
    #[test]
    fn placeholders_must_be_in_order() {
        let html = |indices: &[usize]| indices.iter().map(|&i| placeholder(i) + " ").collect::<String>();
        assert!(placeholders_in_order(&html(&[0, 1, 2]), 3));
        assert!(placeholders_in_order("no pictures", 0));
        assert!(!placeholders_in_order(&html(&[1, 0, 2]), 3));
        assert!(!placeholders_in_order(&html(&[0, 1]), 3));
        assert!(!placeholders_in_order(&html(&[0, 1, 1]), 2));
    }

    /// The PNG and the PDF of an SVG picture are made once, however many
    /// times it is drawn.
    #[test]
    fn the_png_and_pdf_of_an_svg_are_made_once() {
        use crate::model::{Document, Meta, Picture, PictureFormat};
        use std::sync::Arc;
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>"#;
        let Some(crate::backend::svg::ReadySvg::Svg { svg, png_size, .. }) = crate::backend::prepare_svg(svg) else {
            panic!("kept as SVG");
        };
        let picture = Picture {
            data: svg.into_bytes(),
            format: PictureFormat::Svg,
            png_size,
        };
        let doc = Document {
            meta: Meta::default(),
            pages: Vec::new(),
            body_size: 10.0,
            leading: 12.0,
            body_font: String::new(),
            math_font: String::new(),
            blocks: Vec::new(),
            pictures: vec![picture, Picture::bitmap(b"\x89PNG".to_vec(), PictureFormat::Png)],
        };
        let pictures = super::Pictures::new(None, &doc);
        let copy = pictures.clone();
        let png = pictures.svg_png(0).expect("a PNG");
        assert!(Arc::ptr_eq(&png, &copy.svg_png(0).unwrap()));
        let pdf = pictures.svg_pdf(0).expect("a PDF");
        assert!(Arc::ptr_eq(&pdf, &copy.svg_pdf(0).unwrap()));
        // Not an SVG: nothing made.
        assert!(pictures.svg_png(1).is_none() && pictures.svg_pdf(1).is_none());
    }
}
