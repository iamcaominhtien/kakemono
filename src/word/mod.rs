//! Word documents as input: a `.docx` (Office Open XML) or a `.doc` (Word
//! 97–2003) file read into a [`Document`], without the layout analysis a
//! PDF needs, since the file says what each paragraph is.
//!
//! Both readers fill the same intermediate form (`ir`), which `build` turns
//! into the document. A Word document has no pages: its pictures are files
//! stored in the [`Document`] ([`Document::pictures`]).

mod build;
pub(crate) mod doc;
mod docx;
pub mod fmt;
mod ir;
pub(crate) mod omml;
mod pictures;
mod styles;
mod symbols;
mod xml;
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
