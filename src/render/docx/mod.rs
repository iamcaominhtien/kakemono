//! Word output: the document as a `.docx` file (Office Open XML), built
//! from the same [`Document`] as the HTML modes. Headings, lists, captions
//! and references get Word styles, rebuilt tables become Word tables,
//! formulas whose structure is known become Office Math equations, and
//! figures (and the other formulas) are pictures of their area of the page.

mod body;
mod omml;
mod parts;
mod table;
mod xml;
mod zip;

use super::assets::MARK;
use super::{Equations, doc_title};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::ImageSpec;
use parts::{
    APP_PROPS, CONTENT_TYPES, DOCUMENT_NAMESPACES, PACKAGE_RELS, SETTINGS, STYLES, XML_DECL, core_props, document_rels,
};
use zip::{Compression, ZipWriter};

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
}

/// The parts of the package made from the drawn pictures.
#[derive(Default)]
struct Media {
    /// The files: (path in the package, data).
    files: Vec<(String, Vec<u8>)>,
    /// Their relationships: (id, target relative to `word/`).
    rels: Vec<(String, String)>,
}

/// Writes an analysed document as a `.docx` file and returns its bytes.
///
/// The pictures are drawn from `pdf` on `opts.jobs` threads; one that
/// cannot be drawn is left out. The same document always gives the same
/// bytes.
pub fn render_docx(analysis: &Analysis, pdf: &PdfFile, opts: &DocxOptions) -> Vec<u8> {
    let doc = &analysis.doc;
    let mut body = body::Body::new(doc, opts);
    body.write(doc);
    let drawn = crate::common::parallel::map(
        body.images.len(),
        opts.jobs,
        || pdf.renderer(),
        |renderer, i| {
            let spec = &body.images[i].spec;
            renderer.region_picture(spec.page, spec.rect, opts.scale, spec.transparent)
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
    text("[Content_Types].xml", &format!("{XML_DECL}{CONTENT_TYPES}"));
    text("_rels/.rels", &format!("{XML_DECL}{PACKAGE_RELS}"));
    text("docProps/core.xml", &core_props(&doc_title(doc), &doc.meta));
    text("docProps/app.xml", &format!("{XML_DECL}{APP_PROPS}"));
    text("word/document.xml", &document);
    text("word/styles.xml", &format!("{XML_DECL}{STYLES}"));
    text("word/settings.xml", &format!("{XML_DECL}{SETTINGS}"));
    text("word/_rels/document.xml.rels", &document_rels(&media.rels, &links));
    for (name, data) in &media.files {
        zip.add(name, data, Compression::Stored);
    }
    zip.finish()
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
