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
//! - `order`: the reading order of a slide's shapes, from where they lie.
//! - `rules`: titles found in text boxes, title slides, page furniture.

mod build;
mod inherit;
mod media;
mod order;
mod package;
mod rules;
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
/// The heading of a slide's speaker notes.
const NOTES_HEADING: &str = "Notes";
/// PowerPoint's default font of the body text (the theme's minor font of
/// the default theme before Office 2023).
const DEFAULT_BODY_FONT: &str = "Calibri";

/// The thresholds of the rules that read a deck as a document. Every
/// field's name ends with its unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PowerPointConfig {
    /// A slide without a title placeholder takes as its title a text box
    /// whose top edge lies within this share of the slide's height from
    /// its top.
    pub title_top_ratio: f32,
    /// ... whose text is at least this large, in points.
    pub title_min_size_pt: f32,
    /// ... and this many times larger than any other text of the slide
    /// (1.1: a 22 pt title over 20 pt text; decks often set their body in
    /// 24 pt under a 28 pt title).
    pub title_size_ratio: f32,
    /// ... and which holds at most this many words, in one or two
    /// paragraphs without bullets.
    pub title_max_words: usize,
    /// The first slide is a title slide, even when its layout does not say
    /// so, when it has a title and, besides it, no list, table or chart and
    /// at most this many words.
    pub title_slide_max_words: usize,
    /// A text box of at most this many words, near the top or bottom edge
    /// of the slide, with the same text on many slides, is page furniture
    /// (a banner, a copyright line): left out.
    pub furniture_max_words: usize,
    /// ... near the edge: within this share of the slide's height from its
    /// top or bottom.
    pub furniture_edge_ratio: f32,
    /// ... on many slides: on at least this share of the visible slides.
    pub furniture_slides_ratio: f32,
    /// Two shapes overlapping by at most this much, in points, are still
    /// read one after the other, as columns or rows, by their position.
    pub reading_overlap_pt: f32,
}

impl Default for PowerPointConfig {
    fn default() -> Self {
        Self {
            title_top_ratio: 0.25,
            title_min_size_pt: 20.0,
            title_size_ratio: 1.1,
            title_max_words: 30,
            title_slide_max_words: 60,
            furniture_max_words: 12,
            furniture_edge_ratio: 0.15,
            furniture_slides_ratio: 0.5,
            reading_overlap_pt: 2.0,
        }
    }
}

/// The fewest slides a text must be repeated on to be page furniture,
/// whatever the share: a text on 1 of 2 slides is half of them.
const FURNITURE_MIN_SLIDES: usize = 3;

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
    let config = PowerPointConfig::default();
    let height_pt = presentation.size_pt.1;
    let shown: Vec<&SlideRef> = presentation.visible().collect();
    let mut slides = Vec::with_capacity(shown.len());
    for slide in &shown {
        slides.push(parts.slide(slide, defaults, &mut media, config.reading_overlap_pt)?);
    }
    // Titles first: a title repeated on every slide is no furniture.
    for content in &mut slides {
        rules::infer_title(content, height_pt, &config);
    }
    rules::remove_furniture(&mut slides, height_pt, &config);
    // With sections, each section's name heads its slides, one level up,
    // before the first of them that is shown; an unnamed section has none.
    let slide_level = if presentation.sections.is_empty() { 1 } else { 2 };
    let mut section = None;
    for (index, (slide, content)) in shown.iter().zip(slides).enumerate() {
        let notes = parts.notes(slide);
        if index == 0 && (content.title_slide || rules::looks_like_title_slide(&content, &config)) {
            front_matter(content, &config, &mut word.body);
            push_notes(notes, slide_level, &mut word.body);
            continue;
        }
        if content.is_empty() && notes.is_empty() {
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
        word.body
            .extend(build::ordered_items(content.pieces, config.reading_overlap_pt));
        let notes_level = if repeats_section { slide_level } else { slide_level + 1 };
        push_notes(notes, notes_level, &mut word.body);
    }
    word.pictures = media.pictures;
    Ok(crate::word::build_document(word, source))
}

/// Adds the title slide `content` to `body`: its title as the document's
/// title, its subtitle and its other text (authors, a date) as the front
/// matter, in reading order; its pictures (logos) after them.
fn front_matter(content: SlideContent, config: &PowerPointConfig, body: &mut Vec<Item>) {
    if let Some(title) = content.title {
        body.push(Item::Para(Para {
            style: "Title".to_string(),
            inlines: title,
            ..Default::default()
        }));
    }
    let as_subtitle = |para: Para| {
        Item::Para(Para {
            style: "Subtitle".to_string(),
            ..para
        })
    };
    body.extend(content.subtitle.into_iter().map(as_subtitle));
    let mut pictures = Vec::new();
    for item in build::ordered_items(content.pieces, config.reading_overlap_pt) {
        match item {
            Item::Para(para) if !text::inlines_text(&para.inlines).trim().is_empty() => body.push(as_subtitle(para)),
            other => pictures.push(other),
        }
    }
    body.extend(pictures);
}

/// Adds a slide's speaker notes `notes` to `body`, under a heading
/// "Notes" of `level` (one below the slide's own), so that their place in
/// the document, and the section path of their chunks, is their slide's.
fn push_notes(notes: Vec<Para>, level: u8, body: &mut Vec<Item>) {
    if notes.is_empty() {
        return;
    }
    body.push(Item::Para(heading(
        vec![Inline::Text(Span::plain(NOTES_HEADING))],
        level,
    )));
    body.extend(notes.into_iter().map(Item::Para));
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
    /// style `defaults` of the presentation; its groups' shapes are read by
    /// their position, shapes overlapping by `overlap_pt` told apart.
    ///
    /// # Errors
    /// When the slide's part is not well-formed XML.
    fn slide(
        &mut self,
        slide: &SlideRef,
        defaults: Option<Node<'_, '_>>,
        media: &mut Media<'_>,
        overlap_pt: f32,
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
        let mut content = shape_tree(&slide_doc).map_or_else(SlideContent::default, |tree| {
            build::slide(tree, &inheritance, &context, media, overlap_pt)
        });
        // A slide of the title slide layout is a title slide.
        content.title_slide |= layout_doc
            .as_ref()
            .is_some_and(|doc| doc.root_element().attribute("type") == Some("title"));
        Ok(content)
    }
}

impl Parts<'_> {
    /// The text of the speaker notes of `slide` (the body placeholder of
    /// its notes page), which takes its list styles from the notes master
    /// (`p:notesStyle`); empty when it has none.
    fn notes(&mut self, slide: &SlideRef) -> Vec<Para> {
        let Some(part) = self.related(&slide.part, "notesSlide") else {
            return Vec::new();
        };
        let Some(xml) = self.text(&part) else {
            return Vec::new();
        };
        let master_xml = self.related(&part, "notesMaster").and_then(|master| self.text(&master));
        let (Ok(doc), master_doc) = (parse(&xml), master_xml.as_deref().map(parse)) else {
            return Vec::new();
        };
        let master_doc = master_doc.and_then(Result::ok);
        let inheritance = Inheritance {
            master: master_doc.as_ref().and_then(shape_tree),
            defaults: master_doc
                .as_ref()
                .and_then(|doc| child(doc.root_element(), P_NS, "notesStyle")),
            ..Default::default()
        };
        let rels: HashMap<String, Rel> = read_rels(self.zip, &part)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _, rel)| (id, rel))
            .collect();
        let context = TextContext {
            number: slide.number,
            rels: &rels,
        };
        let Some(tree) = shape_tree(&doc) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for shape in tree.descendants().filter(|node| node.tag_name().name() == "sp") {
            let Some(ph) = inherit::placeholder(shape) else {
                continue;
            };
            if ph.kind != "body" {
                continue;
            }
            if let Some(body) = child(shape, P_NS, "txBody") {
                out.extend(text::paragraphs(body, &inheritance.list_styles(shape), &context));
            }
        }
        out
    }
}

/// The shape tree of a slide, layout or master (`p:cSld/p:spTree`).
fn shape_tree<'a, 'input>(doc: &'a Xml<'input>) -> Option<Node<'a, 'input>> {
    child(doc.root_element(), P_NS, "cSld").and_then(|slide| child(slide, P_NS, "spTree"))
}
