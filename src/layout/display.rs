//! The laid-out pages: what is drawn where, in points from the top left
//! corner of the page.

use super::fonts::FaceId;
use crate::word::fmt::model::Rgb;

/// A laid-out page.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// What is drawn, in painting order.
    pub items: Vec<Item>,
}

impl Page {
    /// Frees the room its vectors hold spare (a page is held until the
    /// PDF is written).
    pub fn compact(&mut self) {
        self.items.shrink_to_fit();
        for item in &mut self.items {
            if let Item::Text(run) = item {
                run.glyphs.shrink_to_fit();
                run.text.shrink_to_fit();
            }
        }
    }
}

/// Something drawn on a page.
#[derive(Debug, Clone)]
pub enum Item {
    /// Glyphs of one face.
    Text(TextRun),
    /// A filled rectangle.
    Rect {
        /// Left.
        x: f32,
        /// Top.
        y: f32,
        /// Width.
        width: f32,
        /// Height.
        height: f32,
        /// Colour.
        color: Rgb,
    },
    /// A straight line.
    Line {
        /// Start.
        x1: f32,
        /// Start.
        y1: f32,
        /// End.
        x2: f32,
        /// End.
        y2: f32,
        /// Width.
        width: f32,
        /// Colour.
        color: Rgb,
        /// Dash length, for a dashed line.
        dash: Option<f32>,
    },
    /// A picture.
    Image {
        /// Index into the document's pictures.
        index: usize,
        /// Left.
        x: f32,
        /// Top.
        y: f32,
        /// Width.
        width: f32,
        /// Height.
        height: f32,
        /// Share cut off each side: left, top, right, bottom.
        crop: [f32; 4],
    },
    /// The items until the matching [`Item::EndTurn`] are drawn turned
    /// by `angle_deg` (clockwise) about the point `x`, `y` (vertical text
    /// of a slide).
    Turn {
        /// The angle, in degrees, clockwise.
        angle_deg: f32,
        /// The point turned about.
        x: f32,
        /// The point turned about.
        y: f32,
    },
    /// Ends the last [`Item::Turn`].
    EndTurn,
    /// A link area.
    Link {
        /// Left.
        x: f32,
        /// Top.
        y: f32,
        /// Width.
        width: f32,
        /// Height.
        height: f32,
        /// A web address, or `#bookmark`.
        target: String,
    },
}

/// Glyphs of one face at one size on one baseline.
#[derive(Debug, Clone)]
pub struct TextRun {
    /// The face.
    pub face: FaceId,
    /// Font size.
    pub size: f32,
    /// Pen position at the start.
    pub x: f32,
    /// Baseline.
    pub y: f32,
    /// Colour.
    pub color: Rgb,
    /// The glyphs.
    pub glyphs: Vec<Glyph>,
    /// The text they show.
    pub text: String,
    /// Bold is faked by stroking.
    pub fake_bold: bool,
    /// Italic is faked by slanting.
    pub fake_italic: bool,
    /// Horizontal scale (1 for none).
    pub scale: f32,
}

/// A glyph of a run.
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    /// Glyph id.
    pub id: u16,
    /// Pen position, from the start of the run.
    pub x: f32,
    /// Advance.
    pub advance: f32,
    /// Offset from the pen position.
    pub dx: f32,
    /// Offset above the baseline.
    pub dy: f32,
    /// Byte range of its text in [`TextRun::text`] (32 bits: a page
    /// holds millions of glyphs in a long document).
    pub text: (u32, u32),
}

impl Glyph {
    /// The byte range of its text in [`TextRun::text`].
    #[must_use]
    pub fn range(&self) -> std::ops::Range<usize> {
        self.text.0 as usize..self.text.1 as usize
    }
}

/// A byte offset in a run's text, as a glyph holds it.
#[must_use]
pub fn offset(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

/// A bookmark: where it lands.
#[derive(Debug, Clone)]
pub struct Destination {
    /// The bookmark's name.
    pub name: String,
    /// Page index.
    pub page: usize,
    /// Top of its line.
    pub y: f32,
}
