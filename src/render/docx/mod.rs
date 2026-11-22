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
use super::{Equations, doc_title};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::ImageSpec;
use crate::render::Pictures;
use parts::{
    APP_PROPS, CONTENT_TYPES, DOCUMENT_NAMESPACES, FOOTNOTES_REL, FOOTNOTES_TYPE, PACKAGE_RELS, SETTINGS, STYLES,
    XML_DECL, core_props, document_rels, footnotes_xml,
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

/// The parts of the package made from the drawn pictures.
#[derive(Default)]
struct Media {
    /// The files: (path in the package, data).
    files: Vec<(String, Vec<u8>)>,
    /// Their relationships: (id, target relative to `word/`).
    rels: Vec<(String, String)>,
    /// What each picture shows (see [`PendingImage::hint`]).
    hints: Vec<&'static str>,
}

/// Writes an analysed document as a `.docx` file and returns its bytes.
///
/// The pictures are drawn from `pdf` on `opts.jobs` threads; one that
/// cannot be drawn is left out. The same document always gives the same
/// bytes.
pub fn render_docx(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &DocxOptions) -> Vec<u8> {
    write_package(analysis, pdf, opts).0
}

/// Writes an analysed document as a `.docx` file, like [`render_docx`], and
/// returns each of its pictures as a file in `image_dir` as well
/// (`image_dir/001-fig.png`, ...): the pictures stay embedded in the
/// document.
pub fn render_docx_with_images(
    analysis: &Analysis,
    pdf: Option<&PdfFile>,
    opts: &DocxOptions,
    image_dir: &str,
) -> DocxOutput {
    let (docx, media) = write_package(analysis, pdf, opts);
    let files = media
        .files
        .into_iter()
        .zip(media.hints)
        .enumerate()
        .map(|(i, ((name, data), hint))| {
            let ext = if name.ends_with(".jpeg") { "jpg" } else { "png" };
            (format!("{image_dir}/{:03}-{hint}.{ext}", i + 1), data)
        })
        .collect();
    DocxOutput { docx, files }
}

/// Writes the `.docx` package and returns its bytes with the pictures in it.
fn write_package(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &DocxOptions) -> (Vec<u8>, Media) {
    let doc = &analysis.doc;
    let mut body = body::Body::new(doc, opts);
    body.write(doc);
    let pictures = Pictures::new(pdf, doc);
    let drawn = crate::common::parallel::map(
        body.images.len(),
        opts.jobs,
        || pictures.renderer(),
        |renderer, i| pictures.region(renderer.as_ref(), &body.images[i].spec, opts.scale),
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
    // A document with added notes has a footnotes part too.
    let mut content_types = format!("{XML_DECL}{CONTENT_TYPES}");
    let mut rels = document_rels(&media.rels, &links);
    if !body.notes.is_empty() {
        content_types = content_types.replace("</Types>", &format!("{FOOTNOTES_TYPE}</Types>"));
        rels = rels.replace("</Relationships>", &format!("{FOOTNOTES_REL}</Relationships>"));
    }
    let mut zip = ZipWriter::default();
    let mut text = |name: &str, data: &str| zip.add(name, data.as_bytes(), Compression::Deflated);
    text("[Content_Types].xml", &content_types);
    text("_rels/.rels", &format!("{XML_DECL}{PACKAGE_RELS}"));
    text(
        "docProps/core.xml",
        &core_props(&doc_title(doc), &doc.meta, crate::render::source_label(doc)),
    );
    text("docProps/app.xml", &format!("{XML_DECL}{APP_PROPS}"));
    text("word/document.xml", &document);
    text("word/styles.xml", &format!("{XML_DECL}{STYLES}"));
    text("word/settings.xml", &format!("{XML_DECL}{SETTINGS}"));
    text("word/_rels/document.xml.rels", &rels);
    if !body.notes.is_empty() {
        text("word/footnotes.xml", &footnotes_xml(&body.notes));
    }
    for (name, data) in &media.files {
        zip.add(name, data, Compression::Stored);
    }
    (zip.finish(), media)
}

/// Replaces the picture placeholders of `xml` with the drawn pictures
/// (`drawn[i]` for `images[i]`: data and whether it is a JPEG), adding
/// their files and relationships to `media`. A picture that could not be
/// drawn leaves nothing.
///
/// # Panics
/// When `xml` holds a placeholder that is not closed or whose index is not
/// a number (it was not written by the body).
fn fill_images(xml: &str, images: &[PendingImage], drawn: Vec<Option<(Vec<u8>, bool)>>, media: &mut Media) -> String {
    let mut drawn: Vec<Option<(Vec<u8>, bool)>> = drawn;
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find(MARK) {
        out.push_str(&rest[..start]);
        let after = &rest[start + MARK.len_utf8()..];
        let end = after.find(MARK).expect("placeholders are closed");
        let index: usize = after[..end].parse().expect("placeholder index");
        if let Some((data, jpeg)) = drawn[index].take() {
            let number = media.files.len() + 1;
            let ext = if jpeg { "jpeg" } else { "png" };
            let id = format!("rIdImg{number}");
            out.push_str(&drawing_xml(number, &id, &images[index]));
            media.rels.push((id, format!("media/image{number}.{ext}")));
            media.files.push((format!("word/media/image{number}.{ext}"), data));
            media.hints.push(images[index].hint);
        }
        rest = &after[end + MARK.len_utf8()..];
    }
    out.push_str(rest);
    out
}

/// The run that shows picture number `number` (relationship `rel_id`) in
/// line with the text.
fn drawing_xml(number: usize, rel_id: &str, image: &PendingImage) -> String {
    let (cx, cy) = (image.width_emu, image.height_emu);
    format!(
        "<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
<wp:docPr id=\"{number}\" name=\"Picture {number}\" descr=\"{alt}\"/>\
<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect=\"1\"/></wp:cNvGraphicFramePr>\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic>\
<pic:nvPicPr><pic:cNvPr id=\"{number}\" name=\"image{number}\"/><pic:cNvPicPr/></pic:nvPicPr>\
<pic:blipFill><a:blip r:embed=\"{rel_id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr>\
</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>",
        alt = xml::esc(&image.alt)
    )
}
