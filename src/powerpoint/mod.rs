//! PowerPoint presentations as input: a `.pptx` file (Office Open XML
//! PresentationML; also `.pptm`, `.ppsx`, `.ppsm`, `.potx`, the same
//! package) read into a [`Document`], without the layout analysis a PDF
//! needs, since the slides say what their text is.
//!
//! The slides are read into the same intermediate form as a Word document
//! (`word::ir`), which `word::build` turns into the document, as for an
//! EPUB book. Each visible slide becomes a section, headed by its title.
//! Like a Word document, a deck read so has no pages.
//!
//! - `package`: the presentation part: slide size, slides in order, hidden
//!   slides, sections.

mod package;

use crate::model::{Document, Span};
use crate::word::ir::{Inline, Item, Para, WordDoc};
use crate::word::zip::ZipArchive;
use anyhow::Result;
use package::Presentation;

/// The size of PowerPoint's default body text, in points
/// (`p:defaultTextStyle`, level 1).
const DEFAULT_BODY_SIZE_PT: f32 = 18.0;
/// PowerPoint's default font of the body text (the theme's minor font of
/// the default theme before Office 2023).
const DEFAULT_BODY_FONT: &str = "Calibri";

/// Whether `data` is a PowerPoint presentation: a ZIP package whose main
/// part is a presentation.
pub fn detect(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
        && ZipArchive::new(data)
            .ok()
            .is_some_and(|zip| package::presentation_part(&zip).is_some())
}

/// Reads a presentation into a document. `source` names where it came from
/// ([`Meta::source`](crate::model::Meta::source)).
///
/// # Errors
/// When the archive is damaged or has no presentation part.
pub fn read(data: &[u8], source: Option<String>) -> Result<Document> {
    let zip = ZipArchive::new(data)?;
    let presentation = Presentation::read(&zip)?;
    let mut word = WordDoc {
        title: crate::word::docx::document_title(&zip)?,
        body_size_pt: DEFAULT_BODY_SIZE_PT,
        body_font: DEFAULT_BODY_FONT.to_string(),
        ..Default::default()
    };
    // With sections, each section's name heads its slides, one level up,
    // before the first of them that is shown; an unnamed section has none.
    let slide_level = if presentation.sections.is_empty() { 1 } else { 2 };
    let mut section = None;
    for slide in presentation.visible() {
        let current = presentation.section_of(&slide.id);
        if current != section {
            section = current;
            let name = current.map_or("", |index| presentation.sections[index].name.trim());
            if !name.is_empty() {
                word.body.push(Item::Para(heading(name, 1)));
            }
        }
        word.body
            .push(Item::Para(heading(&format!("Slide {}", slide.number), slide_level)));
    }
    Ok(crate::word::build_document(word, source))
}

/// A heading of `level` (1 for the outermost) with the text `text`.
fn heading(text: &str, level: u8) -> Para {
    Para {
        style: format!("heading {level}"),
        inlines: vec![Inline::Text(Span::plain(text))],
        ..Default::default()
    }
}
