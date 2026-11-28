//! The presentation part of a deck (`ppt/presentation.xml`): the size of
//! its slides, the slides in order with the ones hidden, and the sections
//! PowerPoint groups them in.

use crate::word::docx::{parse, read_rels};
use crate::word::xml::{REL_NS, attr, child, children};
use crate::word::zip::ZipArchive;
use anyhow::{Context, Result};
use roxmltree::Node;
use std::collections::HashMap;

/// Namespace of PresentationML (`p:`).
pub(crate) const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
/// Namespace of the PowerPoint 2010 extensions (`p14:`), which hold the
/// sections.
const P14_NS: &str = "http://schemas.microsoft.com/office/powerpoint/2010/main";
/// EMUs (English Metric Units) in a point: 12 700.
pub(crate) const EMU_PER_PT: f32 = 12_700.0;
/// The type of the package relationship to the main part.
const OFFICE_DOCUMENT_REL: &str = "officeDocument";
/// The part of the content types of a package.
const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
/// The usual name of the presentation part.
const PRESENTATION_NAME: &str = "presentation.xml";
/// PowerPoint's default slide (4:3, 10 × 7.5 inches), in points, when a
/// deck names none.
const DEFAULT_SLIDE_PT: (f32, f32) = (720.0, 540.0);
/// The largest side of a slide PowerPoint allows (56 inches), in points: a
/// larger size in a file is clamped to it.
const MAX_SLIDE_SIDE_PT: f32 = 4_032.0;
/// The most slides read: a deck with more is read up to this many.
const MAX_SLIDES: usize = 5_000;

/// What the presentation part says about the deck.
#[derive(Debug, Default)]
pub(crate) struct Presentation {
    /// The path of the presentation part in the package.
    pub(crate) part: String,
    /// The size of a slide (width, height), in points.
    pub(crate) size_pt: (f32, f32),
    /// The slides, in the order of the deck, hidden ones included.
    pub(crate) slides: Vec<SlideRef>,
    /// The sections, in order; empty when the deck has none.
    pub(crate) sections: Vec<Section>,
}

/// A slide of the deck.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SlideRef {
    /// Its number in the deck (from 1, hidden slides counted), the number
    /// PowerPoint shows.
    pub(crate) number: usize,
    /// Its id in the presentation (`p:sldId id`), which sections name.
    pub(crate) id: String,
    /// The path of its part in the package.
    pub(crate) part: String,
    /// It is hidden (`show="0"`): left out of every output.
    pub(crate) hidden: bool,
}

/// A section of the deck (`p14:section`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Section {
    /// Its name.
    pub(crate) name: String,
    /// The ids of its slides (`p:sldId id`), in order.
    pub(crate) slide_ids: Vec<String>,
}

impl Presentation {
    /// Reads the presentation part of `zip`.
    ///
    /// # Errors
    /// When the package has no presentation part, or it is damaged.
    pub(crate) fn read(zip: &ZipArchive<'_>) -> Result<Self> {
        let part = presentation_part(zip).context("no presentation in the package")?;
        let xml = zip.read_text(&part)?.context("no presentation in the package")?;
        let doc = parse(&xml).with_context(|| format!("reading {part}"))?;
        let root = doc.root_element();
        let rels: HashMap<String, String> = read_rels(zip, &part)?
            .into_iter()
            .filter(|(_, _, rel)| !rel.external)
            .map(|(id, _, rel)| (id, rel.target))
            .collect();
        Ok(Self {
            slides: slide_list(root, &rels, zip),
            sections: sections(root),
            size_pt: slide_size(root),
            part,
        })
    }

    /// The visible slides, in order.
    pub(crate) fn visible(&self) -> impl Iterator<Item = &SlideRef> {
        self.slides.iter().filter(|slide| !slide.hidden)
    }

    /// The index in [`Presentation::sections`] of the section that holds
    /// the slide `id`, if any.
    pub(crate) fn section_of(&self, id: &str) -> Option<usize> {
        self.sections
            .iter()
            .position(|section| section.slide_ids.iter().any(|slide| slide == id))
    }
}

/// The path of the presentation part: the main part of the package when its
/// content type is a presentation's (a deck, a slide show, a template), or
/// when it is named `presentation.xml`; `None` otherwise.
pub(crate) fn presentation_part(zip: &ZipArchive<'_>) -> Option<String> {
    let target = read_rels(zip, "")
        .ok()?
        .into_iter()
        .find(|(_, kind, rel)| !rel.external && kind == OFFICE_DOCUMENT_REL)
        .map(|(_, _, rel)| rel.target)
        .filter(|target| zip.contains(target))?;
    let is_presentation = content_type(zip, &target).map_or_else(
        || target.rsplit('/').next() == Some(PRESENTATION_NAME),
        |kind| kind.contains(".presentationml.") && kind.ends_with(".main+xml"),
    );
    is_presentation.then_some(target)
}

/// The content type the package declares for `part` (an `Override`), if any.
fn content_type(zip: &ZipArchive<'_>, part: &str) -> Option<String> {
    let xml = zip.read_text(CONTENT_TYPES_PART).ok()??;
    let doc = parse(&xml).ok()?;
    let name = format!("/{part}");
    doc.root_element()
        .children()
        .filter(|node| node.tag_name().name() == "Override")
        .find(|node| {
            node.attribute("PartName")
                .is_some_and(|value| value.eq_ignore_ascii_case(&name))
        })
        .and_then(|node| node.attribute("ContentType"))
        .map(str::to_string)
}

/// The size of a slide (`p:sldSz`), in points, within PowerPoint's limits.
fn slide_size(root: Node<'_, '_>) -> (f32, f32) {
    let Some(size) = child(root, P_NS, "sldSz") else {
        return DEFAULT_SLIDE_PT;
    };
    let side = |name: &str, default: f32| {
        size.attribute(name)
            .and_then(|value| value.parse::<f32>().ok())
            .map(|emu| emu / EMU_PER_PT)
            .filter(|pt| pt.is_finite() && *pt > 0.0)
            .map_or(default, |pt| pt.min(MAX_SLIDE_SIDE_PT))
    };
    (side("cx", DEFAULT_SLIDE_PT.0), side("cy", DEFAULT_SLIDE_PT.1))
}

/// The slides of `p:sldIdLst`, in order, each with its part (through the
/// relationships `rels`) and whether it is hidden; a slide whose part is
/// missing is left out (its number still counts).
fn slide_list(root: Node<'_, '_>, rels: &HashMap<String, String>, zip: &ZipArchive<'_>) -> Vec<SlideRef> {
    let Some(list) = child(root, P_NS, "sldIdLst") else {
        return Vec::new();
    };
    let mut slides = Vec::new();
    for (index, entry) in children(list, P_NS, "sldId").take(MAX_SLIDES).enumerate() {
        let Some(part) = attr(entry, REL_NS, "id").and_then(|rel| rels.get(rel)) else {
            continue;
        };
        let Some(xml) = zip.read_text(part).ok().flatten() else {
            continue;
        };
        // `show` is on the slide's own root element.
        let hidden = parse(&xml).is_ok_and(|doc| doc.root_element().attribute("show") == Some("0"));
        slides.push(SlideRef {
            number: index + 1,
            id: entry.attribute("id").unwrap_or_default().to_string(),
            part: part.clone(),
            hidden,
        });
    }
    slides
}

/// The sections of the deck (`p14:sectionLst`, in the presentation's
/// extensions).
fn sections(root: Node<'_, '_>) -> Vec<Section> {
    root.descendants()
        .filter(|node| node.tag_name().name() == "section" && node.tag_name().namespace() == Some(P14_NS))
        .map(|section| Section {
            name: section.attribute("name").unwrap_or_default().to_string(),
            slide_ids: section
                .descendants()
                .filter(|node| node.tag_name().name() == "sldId" && node.tag_name().namespace() == Some(P14_NS))
                .filter_map(|node| node.attribute("id").map(str::to_string))
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root_of(xml: &str, test: impl FnOnce(Node<'_, '_>)) {
        let doc = parse(xml).unwrap();
        test(doc.root_element());
    }

    #[test]
    fn slide_size_is_in_points_and_clamped() {
        let ns = format!("xmlns:p=\"{P_NS}\"");
        root_of(
            &format!("<p:presentation {ns}><p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>"),
            |root| assert_eq!(slide_size(root), (960.0, 540.0)),
        );
        root_of(
            &format!("<p:presentation {ns}><p:sldSz cx=\"999999999999\" cy=\"-5\"/></p:presentation>"),
            |root| assert_eq!(slide_size(root), (MAX_SLIDE_SIDE_PT, DEFAULT_SLIDE_PT.1)),
        );
        root_of(&format!("<p:presentation {ns}/>"), |root| {
            assert_eq!(slide_size(root), DEFAULT_SLIDE_PT);
        });
    }

    #[test]
    fn sections_list_their_slides() {
        let xml = format!(
            "<p:presentation xmlns:p=\"{P_NS}\" xmlns:p14=\"{P14_NS}\"><p:extLst><p:ext><p14:sectionLst>\
             <p14:section name=\"Intro\"><p14:sldIdLst><p14:sldId id=\"256\"/><p14:sldId id=\"257\"/></p14:sldIdLst></p14:section>\
             <p14:section name=\"\"><p14:sldIdLst><p14:sldId id=\"258\"/></p14:sldIdLst></p14:section>\
             </p14:sectionLst></p:ext></p:extLst></p:presentation>"
        );
        root_of(&xml, |root| {
            let presentation = Presentation {
                sections: sections(root),
                ..Default::default()
            };
            assert_eq!(presentation.sections.len(), 2);
            assert_eq!(presentation.sections[0].name, "Intro");
            assert_eq!(presentation.section_of("257"), Some(0));
            assert_eq!(presentation.section_of("258"), Some(1));
            assert_eq!(presentation.section_of("300"), None);
        });
    }
}
