//! Text shaping: the glyphs and advances of a piece of text in a face, with
//! Word's defaults (no ligatures; kerning only when the run asks for it).

use super::fonts::{FaceId, FontBook};
use rustybuzz::{Direction, Feature, UnicodeBuffer, ttf_parser::Tag};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// A face as rustybuzz reads it, `None` when the font cannot be read.
type MaybeFace<'a> = Option<rustybuzz::Face<'a>>;

self_cell::self_cell!(
    /// A face read for shaping, with the font data it borrows: reading a
    /// face parses its substitution and positioning tables, which for a
    /// font of a complex script (Devanagari) costs far more than shaping
    /// a word, so each face is read once.
    struct ParsedFace {
        owner: Arc<Vec<u8>>,
        #[covariant]
        dependent: MaybeFace,
    }
);

/// A glyph, in multiples of the font size.
#[derive(Debug, Clone, Copy)]
pub struct ShapedGlyph {
    /// Glyph id.
    pub id: u16,
    /// Advance.
    pub advance: f32,
    /// Horizontal offset.
    pub dx: f32,
    /// Vertical offset (up).
    pub dy: f32,
    /// Byte offset of its cluster in the text.
    pub cluster: usize,
}

/// Shaped text, in multiples of the font size.
#[derive(Debug, Clone)]
pub struct Shaped {
    /// The glyphs, in visual order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Sum of the advances.
    pub width: f32,
}

/// Shapes text, keeping the results and the faces it has read.
#[derive(Default)]
pub struct Shaper {
    /// Shaped text, by face, text, kerning and direction.
    cache: HashMap<(FaceId, String, bool, bool), Rc<Shaped>>,
    /// The faces read so far.
    faces: HashMap<FaceId, ParsedFace>,
}

impl Shaper {
    /// The glyphs of `text` in face `face`, kerned when `kern` is set, right
    /// to left when `rtl` is set.
    pub fn shape(&mut self, book: &FontBook, face: FaceId, text: &str, kern: bool, rtl: bool) -> Rc<Shaped> {
        let key = (face, text.to_string(), kern, rtl);
        if let Some(shaped) = self.cache.get(&key) {
            return shaped.clone();
        }
        let font = book.get(face);
        let parsed = self.faces.entry(face).or_insert_with(|| {
            ParsedFace::new(font.data.clone(), |data| rustybuzz::Face::from_slice(data, font.index))
        });
        let shaped = Rc::new(match parsed.borrow_dependent() {
            Some(hb) => shape(hb, font.metrics.upem, text, kern, rtl),
            None => Shaped {
                glyphs: Vec::new(),
                width: 0.0,
            },
        });
        self.cache.insert(key, shaped.clone());
        shaped
    }
}

/// Shapes `text` in the face `hb`, of `upem` units per em.
fn shape(hb: &rustybuzz::Face<'_>, upem: f32, text: &str, kern: bool, rtl: bool) -> Shaped {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    if rtl {
        buffer.set_direction(Direction::RightToLeft);
    }
    let off = |tag: &[u8; 4]| Feature::new(Tag::from_bytes(tag), 0, ..);
    let mut features = vec![off(b"liga"), off(b"clig")];
    if !kern {
        features.push(off(b"kern"));
    }
    let output = rustybuzz::shape(hb, &features, buffer);
    let mut glyphs = Vec::with_capacity(output.len());
    let mut width = 0.0;
    for (info, pos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        let advance = pos.x_advance as f32 / upem;
        width += advance;
        glyphs.push(ShapedGlyph {
            id: info.glyph_id as u16,
            advance,
            dx: pos.x_offset as f32 / upem,
            dy: pos.y_offset as f32 / upem,
            cluster: info.cluster as usize,
        });
    }
    Shaped { glyphs, width }
}
