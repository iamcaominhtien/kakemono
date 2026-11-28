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
    to_pdf_with(doc, &mut book)
}

/// Lays out a formatted document with the fonts of `book` and writes it as
/// a PDF.
///
/// # Errors
/// When the PDF cannot be written.
fn to_pdf_with(doc: &FDoc, book: &mut fonts::FontBook) -> Result<Vec<u8>> {
    if flow::is_long(doc) {
        return to_pdf_page_by_page(doc, book);
    }
    let laid = flow::layout(doc, book);
    pdf::write(laid.pages, &laid.destinations, book, &doc.images, doc.title.as_deref())
}

/// Lays out a long document and writes each page into the PDF as soon as
/// it is drawn, so that its pages are not all held at once. The PDF is the
/// same as [`to_pdf_with`] writes. What the pages need of later ones (the
/// page numbers of `PAGEREF`, the page counts of headers and footers, the
/// bookmarks that links point to) is found by a pass that only counts,
/// before the pass that writes; the pass that writes is the first one when
/// it needs none of it, and is then the only pass of a document without
/// `PAGEREF`.
///
/// # Errors
/// When the PDF cannot be written.
fn to_pdf_page_by_page(doc: &FDoc, book: &mut fonts::FontBook) -> Result<Vec<u8>> {
    let title = doc.title.as_deref();
    let refs =
        flow::refers_to_pages(doc).then(|| flow::layout_into(doc, book, None, None, &mut flow::Discard).page_refs);
    let mut writer = pdf::Writer::before_bookmarks(&doc.images);
    let first = flow::layout_into(doc, book, refs.as_ref(), None, &mut writer);
    if !first.missed_counts && !writer.linked_inside() {
        return writer.finish(title);
    }
    drop(writer);
    let mut writer = pdf::Writer::new(&doc.images, &first.destinations);
    flow::layout_into(doc, book, refs.as_ref(), Some(&first.counts), &mut writer);
    writer.finish(title)
}

#[cfg(test)]
mod tests {
    use super::{flow, fonts, pdf, to_pdf_page_by_page};
    use crate::word::WordFormat;
    use crate::word::fmt::model::{Block, FDoc, FieldKind, Inline, Para, RunFmt, VMerge};
    use crate::word::fmt::read;

    /// A sample Word document of `tests/data/word/`.
    fn sample(name: &str) -> FDoc {
        let path = format!("{}/tests/data/word/{name}", env!("CARGO_MANIFEST_DIR"));
        read(&std::fs::read(path).expect("the sample"), WordFormat::Docx).expect("it reads")
    }

    /// The PDF of `doc` laid out with its pages held, then written.
    fn held_pdf(doc: &FDoc) -> Vec<u8> {
        let mut book = fonts::FontBook::new(&doc.fonts, &doc.font_kinds);
        let laid = flow::layout(doc, &mut book);
        pdf::write(laid.pages, &laid.destinations, &book, &doc.images, doc.title.as_deref()).expect("a PDF")
    }

    /// The PDF of `doc` written page by page.
    fn page_by_page_pdf(doc: &FDoc) -> Vec<u8> {
        let mut book = fonts::FontBook::new(&doc.fonts, &doc.font_kinds);
        to_pdf_page_by_page(doc, &mut book).expect("a PDF")
    }

    /// A field of `kind` in the text.
    fn field(kind: FieldKind) -> Inline {
        Inline::Field {
            kind,
            fmt: RunFmt::default(),
            format: None,
        }
    }

    /// `doc` many pages long: its body 40 times over, its tables with the
    /// first cell of their body merged down, and a footer.
    fn lengthen(doc: &mut FDoc, footer: Vec<Inline>) {
        for block in &mut doc.sections[0].blocks {
            if let Block::Table(table) = block {
                for (at, row) in table.rows.iter_mut().enumerate().skip(1) {
                    row.cells[0].vmerge = if at == 1 { VMerge::Restart } else { VMerge::Continue };
                }
            }
        }
        let body = doc.sections[0].blocks.clone();
        for _ in 0..40 {
            doc.sections[0].blocks.extend(body.iter().cloned());
        }
        doc.parts.push(vec![Block::Para(Box::new(Para {
            inlines: footer,
            ..Para::default()
        }))]);
        let part = doc.parts.len() - 1;
        for section in &mut doc.sections {
            section.footers.default = Some(part);
        }
    }

    #[test]
    fn a_document_written_page_by_page_is_the_same_pdf() {
        // Links to its bookmarks, a footer with the page count and a
        // PAGEREF: three passes.
        let mut doc = sample("sample.docx");
        let text = |text: &str| Inline::Text {
            text: text.into(),
            fmt: RunFmt::default(),
        };
        lengthen(
            &mut doc,
            vec![
                text("Page "),
                field(FieldKind::Page),
                text(" of "),
                field(FieldKind::NumPages),
                text(", introduction on page "),
                field(FieldKind::PageRef("sec-intro".into())),
            ],
        );
        let held = held_pdf(&doc);
        assert!(held.len() > 10_000);
        assert!(held == page_by_page_pdf(&doc), "the PDFs differ");
        // No link, no page count: one pass.
        let mut doc = sample("lists.docx");
        lengthen(&mut doc, vec![text("Page "), field(FieldKind::Page)]);
        assert!(held_pdf(&doc) == page_by_page_pdf(&doc), "the PDFs differ");
    }
}
