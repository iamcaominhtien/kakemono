//! The Word layout engine: a Word document ([`FDoc`]) laid out in pages as
//! Word lays it out (fonts with Word's metrics, line breaking, line
//! spacing, page breaking, tables, headers and footers, footnotes), and
//! written as a PDF. The page modes of the HTML output use that PDF. A
//! workbook is laid out by the same engine (`workbook`).

pub mod display;
pub mod flow;
pub mod fonts;
pub mod html;
pub mod lines;
pub mod metrics;
pub mod para;
pub mod pdf;
pub mod shape;
pub mod workbook;

use crate::word::fmt::FDoc;
use anyhow::Result;

/// Lays out a Word document and writes it as a PDF.
///
/// # Errors
/// When the PDF cannot be written.
pub fn to_pdf(doc: &FDoc) -> Result<Vec<u8>> {
    // For debugging: the formatted document as read.
    if std::env::var_os("KAKEMONO_DUMP_FORMAT").is_some() {
        eprintln!("{doc:#?}");
    }
    let mut book = fonts::FontBook::new(&doc.fonts, &doc.font_kinds);
    book.require_font()?;
    to_pdf_with(doc, &mut book)
}

/// Lays out a formatted document with the fonts of `book` and writes it as
/// a PDF.
///
/// # Errors
/// When the PDF cannot be written.
fn to_pdf_with(doc: &FDoc, book: &mut fonts::FontBook) -> Result<Vec<u8>> {
    let laid = flow::layout(doc, book);
    pdf::write(laid.pages, &laid.destinations, book, &doc.images, doc.title.as_deref())
}
