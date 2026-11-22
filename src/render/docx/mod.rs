//! Word output: the document as a `.docx` file (Office Open XML), built
//! from the same [`Document`] as the HTML modes. Headings, lists, captions
//! and references get Word styles, rebuilt tables become Word tables,
//! formulas whose structure is known become Office Math equations, and
//! figures (and the other formulas) are pictures of their area of the page.

mod body;
mod omml;

/// The Office Math the Word output writes for a formula in running text
/// (for the tests of the Word reader).
#[cfg(test)]
pub(crate) use omml::inline_math;
mod parts;
mod table;
mod xml;

use super::assets::MARK;
use super::zip::{Compression, ZipWriter};
use super::{Equations, InputFormat, doc_title};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::{ImageSpec, PictureFormat};
use crate::render::Pictures;
use parts::{
    APP_PROPS, CONTENT_TYPES, DOCUMENT_NAMESPACES, PACKAGE_RELS, SETTINGS, STYLES, XML_DECL, core_props, document_rels,
};

/// Everything that controls how a document is written as a Word file.
#[derive(Debug, Clone)]
pub struct DocxOptions {
    /// Pixels per PDF point of the pictures of figures and formulas.
    pub scale: f32,
    /// Worker threads for drawing the pictures (0 = one per CPU).
    pub jobs: usize,
    /// How displayed formulas are written: [`Equations::MathMl`] gives
    /// Office Math equations (editable in Word) where the structure of the
    /// formula is clear, pictures elsewhere.
    pub equations: Equations,
}

impl Default for DocxOptions {
    /// Pictures at 3 pixels per point, formulas as equations where their
    /// structure is clear, one worker thread per CPU.
    fn default() -> Self {
        Self {
            scale: 3.0,
            jobs: 0,
            equations: Equations::MathMl,
        }
    }
}

/// A picture the body refers to by a placeholder, drawn after the body is
/// written.
pub(crate) struct PendingImage {
    /// The area of the page to draw.
    spec: ImageSpec,
    /// Width in the document, in EMU.
    width_emu: u64,
    /// Height in the document, in EMU.
    height_emu: u64,
    /// Alternative text.
    alt: String,
    /// What the picture shows (`fig`, `tab`, `eq`, ...), for the name of
    /// its file in [`DocxOutput::files`].
    hint: &'static str,
}

/// A Word document and its pictures as files of their own (see
/// [`render_docx_with_images`]).
pub struct DocxOutput {
    /// The bytes of the `.docx` file, with the pictures embedded.
    pub docx: Vec<u8>,
    /// Each picture of the document: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

/// The URI of the extension of an `a:blip` that holds an SVG picture.
const SVG_BLIP_EXT: &str = "{96DAC541-7B7A-43D3-8B79-37D633B846F1}";
/// The namespace of `asvg:svgBlip`.
const SVG_BLIP_NS: &str = "http://schemas.microsoft.com/office/drawing/2016/SVG/main";

/// A drawn picture: its data, its format, and for an SVG the PNG drawn of
/// it, which Word shows where it cannot show SVG.
type DrawnPicture = (Vec<u8>, PictureFormat, Option<std::sync::Arc<[u8]>>);

/// The parts of the package made from the drawn pictures.
#[derive(Default)]
struct Media {
    /// The files: (path in the package, data).
    files: Vec<(String, Vec<u8>)>,
    /// Their relationships: (id, target relative to `word/`).
    rels: Vec<(String, String)>,
    /// Each picture of the document: the index of its file in
    /// [`Media::files`] (its SVG, not the PNG beside it), and what it shows
    /// (see [`PendingImage::hint`]).
    shown: Vec<(usize, &'static str)>,
}

impl Media {
    /// Adds the file `data` as `word/media/image{number}.{ext}` with its
    /// relationship, and returns the relationship's id.
    fn add(&mut self, data: Vec<u8>, ext: &str) -> String {
        let number = self.files.len() + 1;
        let id = format!("rIdImg{number}");
        self.rels.push((id.clone(), format!("media/image{number}.{ext}")));
        self.files.push((format!("word/media/image{number}.{ext}"), data));
        id
    }

    /// Whether one of the files is an SVG picture.
    fn has_svg(&self) -> bool {
        self.files.iter().any(|(name, _)| name.ends_with(".svg"))
    }
}

/// Writes an analysed document as a `.docx` file and returns its bytes.
///
/// The pictures are drawn from `pdf` on `opts.jobs` threads; one that
/// cannot be drawn is left out. The same document always gives the same
/// bytes.
pub fn render_docx(analysis: &Analysis, pdf: Option<&PdfFile>, input: InputFormat, opts: &DocxOptions) -> Vec<u8> {
    write_package(analysis, pdf, input, opts).0
}

/// Writes an analysed document as a `.docx` file, like [`render_docx`], and
/// returns each of its pictures as a file in `image_dir` as well
/// (`image_dir/001-fig.png`, ...): the pictures stay embedded in the
/// document.
pub fn render_docx_with_images(
    analysis: &Analysis,
    pdf: Option<&PdfFile>,
    input: InputFormat,
    opts: &DocxOptions,
    image_dir: &str,
) -> DocxOutput {
    let (docx, mut media) = write_package(analysis, pdf, input, opts);
    let files = media
        .shown
        .iter()
        .enumerate()
        .map(|(i, &(file, hint))| {
            let (name, data) = std::mem::take(&mut media.files[file]);
            let ext = match name.rsplit_once('.') {
                Some((_, "jpeg")) => "jpg",
                Some((_, "svg")) => "svg",
                _ => "png",
            };
            (format!("{image_dir}/{:03}-{hint}.{ext}", i + 1), data)
        })
        .collect();
    DocxOutput { docx, files }
}

/// Writes the `.docx` package of a document read from a file of the format
/// `input`, and returns its bytes with the pictures in it.
fn write_package(
    analysis: &Analysis,
    pdf: Option<&PdfFile>,
    input: InputFormat,
    opts: &DocxOptions,
) -> (Vec<u8>, Media) {
    let doc = &analysis.doc;
    let mut body = body::Body::new(doc, opts);
    body.write(doc);
    let pictures = Pictures::new(pdf, doc);
    let drawn = crate::common::parallel::map(
        body.images.len(),
        opts.jobs,
        || pictures.renderer(),
        |renderer, i| {
            let spec = &body.images[i].spec;
            let (data, format) = pictures.region(renderer.as_ref(), spec, opts.scale)?;
            // The PNG beside an SVG, made once however often it is used.
            let fallback = spec.embedded.and_then(|index| pictures.svg_png(index));
            Some((data, format, fallback))
        },
    );
    let mut media = Media::default();
    let content = fill_images(&body.xml, &body.images, drawn, &mut media);
    let links: Vec<(String, String)> = body
        .links
        .iter()
        .enumerate()
        .map(|(i, url)| (format!("rIdLink{i}"), url.clone()))
        .collect();

    let document = format!("{XML_DECL}<w:document {DOCUMENT_NAMESPACES}><w:body>{content}</w:body></w:document>");
    let mut zip = ZipWriter::default();
    let mut text = |name: &str, data: &str| zip.add(name, data.as_bytes(), Compression::Deflated);
    let content_types = if media.has_svg() {
        // SVG pictures are rare: the other documents keep the bytes they had.
        CONTENT_TYPES.replacen(
            "<Override",
            "<Default Extension=\"svg\" ContentType=\"image/svg+xml\"/><Override",
            1,
        )
    } else {
        CONTENT_TYPES.to_string()
    };
    text("[Content_Types].xml", &format!("{XML_DECL}{content_types}"));
    text("_rels/.rels", &format!("{XML_DECL}{PACKAGE_RELS}"));
    text(
        "docProps/core.xml",
        &core_props(&doc_title(doc), &doc.meta, input.label()),
    );
    text("docProps/app.xml", &format!("{XML_DECL}{APP_PROPS}"));
    text("word/document.xml", &document);
    text("word/styles.xml", &format!("{XML_DECL}{STYLES}"));
    text("word/settings.xml", &format!("{XML_DECL}{SETTINGS}"));
    text("word/_rels/document.xml.rels", &document_rels(&media.rels, &links));
    for (name, data) in &media.files {
        zip.add(name, data, Compression::Stored);
    }
    (zip.finish(), media)
}

/// Replaces the picture placeholders of `xml` with the drawn pictures
/// (`drawn[i]` for `images[i]`: data, format, and for an SVG the PNG drawn
/// of it), adding their files and
/// relationships to `media`. An SVG picture is written as Word writes one:
/// a PNG drawn of it, which any reader shows, with the SVG beside it
/// (`asvg:svgBlip`), which Word 2016 and later show. A picture that could
/// not be drawn leaves nothing.
///
/// # Panics
/// When `xml` holds a placeholder that is not closed or whose index is not
/// a number (it was not written by the body).
fn fill_images(xml: &str, images: &[PendingImage], drawn: Vec<Option<DrawnPicture>>, media: &mut Media) -> String {
    let mut drawn = drawn;
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find(MARK) {
        out.push_str(&rest[..start]);
        let after = &rest[start + MARK.len_utf8()..];
        let end = after.find(MARK).expect("placeholders are closed");
        let index: usize = after[..end].parse().expect("placeholder index");
        if let Some((data, format, fallback)) = drawn[index].take() {
            let (id, svg_id) = match format {
                PictureFormat::Svg => match fallback {
                    Some(png) => {
                        let id = media.add(png.to_vec(), "png");
                        let svg_id = media.add(data, "svg");
                        (id, Some(svg_id))
                    }
                    // No PNG can be drawn of it: the SVG alone.
                    None => (media.add(data, "svg"), None),
                },
                PictureFormat::Jpeg => (media.add(data, "jpeg"), None),
                PictureFormat::Png => (media.add(data, "png"), None),
            };
            media.shown.push((media.files.len() - 1, images[index].hint));
            let number = media.shown.len();
            out.push_str(&drawing_xml(number, &id, svg_id.as_deref(), &images[index]));
        }
        rest = &after[end + MARK.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// The run that shows picture number `number` (relationship `rel_id`, and
/// `svg_id` that of its SVG when it has one) in line with the text.
fn drawing_xml(number: usize, rel_id: &str, svg_id: Option<&str>, image: &PendingImage) -> String {
    let (cx, cy) = (image.width_emu, image.height_emu);
    let blip = match svg_id {
        Some(svg_id) => format!(
            "<a:blip r:embed=\"{rel_id}\"><a:extLst><a:ext uri=\"{SVG_BLIP_EXT}\">\
<asvg:svgBlip xmlns:asvg=\"{SVG_BLIP_NS}\" r:embed=\"{svg_id}\"/></a:ext></a:extLst></a:blip>"
        ),
        None => format!("<a:blip r:embed=\"{rel_id}\"/>"),
    };
    format!(
        "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
<wp:docPr id=\"{number}\" name=\"Picture {number}\" descr=\"{alt}\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"{number}\" name=\"image{number}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill>{blip}<a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
        alt = xml::esc(&image.alt)
    )
}
