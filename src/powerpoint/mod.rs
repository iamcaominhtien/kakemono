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
//! - `package`: the presentation part: slides in order, hidden slides,
//!   sections.
//! - `inherit`: placeholders matched with their layout's and master's, and
//!   the list styles a shape's text takes its properties from.
//! - `text`: a text body → paragraphs and list items.
//! - `build`: a slide's shapes → its title and content, in reading order.
//! - `media`: pictures, charts and the text of SmartArt diagrams.

mod build;
mod inherit;
mod media;
mod package;
mod text;

use crate::model::{Document, Span};
use crate::word::docx::{Rel, parse, read_rels};
use crate::word::ir::{Inline, Item, Para, WordDoc};
use crate::word::xml::child;
use crate::word::zip::ZipArchive;
use anyhow::{Context, Result};
use build::SlideContent;
use inherit::Inheritance;
use media::Media;
use package::{P_NS, Presentation, SlideRef};
use roxmltree::{Document as Xml, Node};
use std::collections::HashMap;
use text::TextContext;

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
/// A first slide with a centred title (a title slide) gives the document's
/// title and its subtitle; every other visible slide is a section headed by
/// its title ("Slide N" when it has none), under the name of its
/// PowerPoint section when the deck has sections.
///
/// # Errors
/// When the archive is damaged or has no presentation part.
pub fn read(data: &[u8], source: Option<String>) -> Result<Document> {
    let zip = ZipArchive::new(data)?;
    let presentation = Presentation::read(&zip)?;
    let mut parts = Parts::new(&zip);
    let presentation_xml = parts.text(&presentation.part).unwrap_or_default();
    let presentation_doc = parse(&presentation_xml).with_context(|| format!("reading {}", presentation.part))?;
    let defaults = child(presentation_doc.root_element(), P_NS, "defaultTextStyle");
    // Charts take their colours from the deck's theme.
    let theme = parts
        .related(&presentation.part, "theme")
        .and_then(|part| zip.read(&part).ok().flatten());
    let mut media = Media::new(&zip, theme.as_deref());
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
    for (index, slide) in presentation.visible().enumerate() {
        let content = parts.slide(slide, defaults, &mut media)?;
        if index == 0 && content.centred_title {
            front_matter(content, &mut word.body);
            continue;
        }
        if content.is_empty() {
            continue;
        }
        let current = presentation.section_of(&slide.id);
        let mut section_name = "";
        if current != section {
            section = current;
            section_name = current.map_or("", |index| presentation.sections[index].name.trim());
            if !section_name.is_empty() {
                word.body
                    .push(Item::Para(heading(vec![Inline::Text(Span::plain(section_name))], 1)));
            }
        }
        let title = content
            .title
            .unwrap_or_else(|| vec![Inline::Text(Span::plain(format!("Slide {}", slide.number)))]);
        // A section's header slide often repeats its name: one heading is
        // enough.
        let repeats_section = !section_name.is_empty()
            && text::inlines_text(&title)
                .split_whitespace()
                .eq(section_name.split_whitespace());
        if !repeats_section {
            word.body.push(Item::Para(heading(title, slide_level)));
        }
        word.body.extend(content.subtitle.into_iter().map(Item::Para));
        word.body.extend(content.body);
    }
    word.pictures = media.pictures;
    Ok(crate::word::build_document(word, source))
}

/// Adds the title slide `content` to `body`: its title as the document's
/// title, its subtitle and its other text as the front matter.
fn front_matter(content: SlideContent, body: &mut Vec<Item>) {
    if let Some(title) = content.title {
        body.push(Item::Para(Para {
            style: "Title".to_string(),
            inlines: title,
            ..Default::default()
        }));
    }
    for para in content.subtitle {
        body.push(Item::Para(Para {
            style: "Subtitle".to_string(),
            ..para
        }));
    }
    body.extend(content.body);
}

/// A heading of `level` (1 for the outermost) with the text `inlines`.
fn heading(inlines: Vec<Inline>, level: u8) -> Para {
    Para {
        style: format!("heading {level}"),
        inlines,
        ..Default::default()
    }
}

/// The parts of a deck, read once each (a layout and a master serve many
/// slides).
struct Parts<'z> {
    /// The package.
    zip: &'z ZipArchive<'z>,
    /// The text of the parts read, by path; `None` for one missing.
    texts: HashMap<String, Option<String>>,
}

impl<'z> Parts<'z> {
    /// The parts of `zip`.
    fn new(zip: &'z ZipArchive<'z>) -> Self {
        Self {
            zip,
            texts: HashMap::new(),
        }
    }

    /// The text of the part `path`, when there is one.
    fn text(&mut self, path: &str) -> Option<String> {
        self.texts
            .entry(path.to_string())
            .or_insert_with(|| self.zip.read_text(path).ok().flatten())
            .clone()
    }

    /// The part related to `part` by a relationship of type `kind`
    /// (`slideLayout`, `slideMaster`).
    fn related(&self, part: &str, kind: &str) -> Option<String> {
        read_rels(self.zip, part)
            .ok()?
            .into_iter()
            .find(|(_, own, rel)| own == kind && !rel.external)
            .map(|(_, _, rel)| rel.target)
    }

    /// The content of the slide `slide`, whose text takes the default text
    /// style `defaults` of the presentation.
    ///
    /// # Errors
    /// When the slide's part is not well-formed XML.
    fn slide(
        &mut self,
        slide: &SlideRef,
        defaults: Option<Node<'_, '_>>,
        media: &mut Media<'_>,
    ) -> Result<SlideContent> {
        let Some(slide_xml) = self.text(&slide.part) else {
            return Ok(SlideContent::default());
        };
        let layout_part = self.related(&slide.part, "slideLayout");
        let master_part = layout_part
            .as_deref()
            .and_then(|part| self.related(part, "slideMaster"));
        let layout_xml = layout_part.and_then(|part| self.text(&part));
        let master_xml = master_part.and_then(|part| self.text(&part));
        let slide_doc = parse(&slide_xml).with_context(|| format!("reading {}", slide.part))?;
        // A damaged layout or master is read as none.
        let layout_doc = layout_xml.as_deref().and_then(|xml| parse(xml).ok());
        let master_doc = master_xml.as_deref().and_then(|xml| parse(xml).ok());
        let inheritance = Inheritance {
            layout: layout_doc.as_ref().and_then(shape_tree),
            master: master_doc.as_ref().and_then(shape_tree),
            styles: master_doc
                .as_ref()
                .and_then(|doc| child(doc.root_element(), P_NS, "txStyles")),
            defaults,
        };
        let rels: HashMap<String, Rel> = read_rels(self.zip, &slide.part)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _, rel)| (id, rel))
            .collect();
        let context = TextContext {
            number: slide.number,
            rels: &rels,
        };
        Ok(shape_tree(&slide_doc).map_or_else(SlideContent::default, |tree| {
            build::slide(tree, &inheritance, &context, media)
        }))
    }
}

/// The shape tree of a slide, layout or master (`p:cSld/p:spTree`).
fn shape_tree<'a, 'input>(doc: &'a Xml<'input>) -> Option<Node<'a, 'input>> {
    child(doc.root_element(), P_NS, "cSld").and_then(|slide| child(slide, P_NS, "spTree"))
}
