//! Word documents as input: a `.docx` (Office Open XML) or a `.doc` (Word
//! 97–2003) file read into a [`Document`], without the layout analysis a
//! PDF needs, since the file says what each paragraph is.
//!
//! Both readers fill the same intermediate form (`ir`), which `build` turns
//! into the document. A Word document has no pages: its pictures are files
//! stored in the [`Document`] ([`Document::pictures`]).

mod build;
pub(crate) mod doc;
pub(crate) mod docx;
pub mod fmt;
mod ir;
pub(crate) mod metafile;
pub(crate) mod mtef;
pub(crate) mod omml;
pub(crate) mod pictures;
mod styles;
mod symbols;
pub(crate) mod xml;
pub(crate) mod zip;

use crate::model::Document;
use anyhow::Result;

/// The kinds of Word files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WordFormat {
    /// Office Open XML (`.docx`), Word 2007 and later.
    Docx,
    /// The binary format of Word 97–2003 (`.doc`).
    Doc,
}

/// The kind of Word file `data` is, from its first bytes; `None` when it is
/// none.
pub fn detect(data: &[u8]) -> Option<WordFormat> {
    if docx::is_docx(data) {
        Some(WordFormat::Docx)
    } else if doc::is_doc(data) {
        Some(WordFormat::Doc)
    } else {
        None
    }
}

/// Reads a Word file of kind `format` into a document. `source` names
/// where it came from ([`Meta::source`](crate::model::Meta::source)).
///
/// # Errors
/// When the file is damaged or is not a Word document.
pub fn read(data: &[u8], format: WordFormat, source: Option<String>) -> Result<Document> {
    let word = match format {
        WordFormat::Docx => docx::read(data)?,
        WordFormat::Doc => doc::read(data)?,
    };
    Ok(build::build(word, source))
}

/// The `Equation Native` stream of an embedded OLE object (a compound
/// file), which holds a MathType or Equation Editor formula.
pub(crate) fn equation_native(ole: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut file = cfb::CompoundFile::open(std::io::Cursor::new(ole)).ok()?;
    let mut stream = file.open_stream("/Equation Native").ok()?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// Shows a formula alone in its paragraph (an embedded equation object,
/// with nothing but spaces and tabs around it) on a line of its own.
pub(crate) fn display_alone_formula(para: &mut ir::Para) {
    let mut formulas = para
        .inlines
        .iter()
        .filter(|inline| matches!(inline, ir::Inline::Math { .. }));
    let alone = formulas.next().is_some()
        && formulas.next().is_none()
        && para.inlines.iter().all(|inline| match inline {
            ir::Inline::Math { .. } => true,
            ir::Inline::Text(span) => span.text.trim().is_empty(),
            _ => false,
        });
    if alone {
        para.inlines.retain(|inline| matches!(inline, ir::Inline::Math { .. }));
        if let Some(ir::Inline::Math { display, .. }) = para.inlines.first_mut() {
            *display = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::paper::Paper;
    use crate::render::zip::{Compression, ZipWriter};

    /// The sample document with its picture's file replaced by `data`.
    fn sample_with_picture(data: &[u8]) -> Vec<u8> {
        let sample = std::fs::read("tests/data/word/sample.docx").unwrap();
        let zip = super::zip::ZipArchive::new(&sample).unwrap();
        let mut out = ZipWriter::default();
        for name in zip.names() {
            let contents = if name == "word/media/rId24.png" {
                data.to_vec()
            } else {
                zip.read(&name).unwrap().unwrap()
            };
            out.add(&name, &contents, Compression::Deflated);
        }
        out.finish()
    }

    #[test]
    fn a_picture_declaring_a_huge_size_is_bounded_in_text_and_layout() {
        // A PNG header that says 100000 × 100000: left out.
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(100_000u32.to_be_bytes());
        png.extend(100_000u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        let paper = Paper::from_bytes(sample_with_picture(&png)).unwrap();
        assert!(paper.analyze().doc.pictures.is_empty());
        assert!(paper.to_pdf().is_ok());
        // An SVG of 30000 × 30000 points: drawn smaller.
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="30000" height="30000"><rect width="10" height="10"/></svg>"#;
        let paper = Paper::from_bytes(sample_with_picture(svg)).unwrap();
        let doc = paper.analyze().doc;
        assert_eq!(doc.pictures.len(), 1);
        assert!(doc.pictures[0].data.starts_with(b"\x89PNG"));
        assert!(paper.to_pdf().is_ok());
    }
}
