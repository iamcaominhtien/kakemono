//! Text shaping: the glyphs and advances of a piece of text in a face, with
//! Word's defaults (no ligatures; kerning only when the run asks for it).

use super::fonts::{FaceId, FontBook};
use rustybuzz::{Direction, Feature, UnicodeBuffer, ttf_parser::Tag};
use std::collections::HashMap;
use std::rc::Rc;

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

/// Shapes text, keeping the results.
#[derive(Default)]
pub struct Shaper {
    cache: HashMap<(FaceId, String, bool, bool), Rc<Shaped>>,
}

impl Shaper {
    /// The glyphs of `text` in face `face`, kerned when `kern` is set, right
    /// to left when `rtl` is set.
    pub fn shape(&mut self, book: &FontBook, face: FaceId, text: &str, kern: bool, rtl: bool) -> Rc<Shaped> {
        let key = (face, text.to_string(), kern, rtl);
        if let Some(shaped) = self.cache.get(&key) {
            return shaped.clone();
        }
        let shaped = Rc::new(shape(book, face, text, kern, rtl));
        self.cache.insert(key, shaped.clone());
        shaped
    }
}

/// Shapes `text` in `face`.
fn shape(book: &FontBook, face: FaceId, text: &str, kern: bool, rtl: bool) -> Shaped {
    let font = book.get(face);
    let Some(hb) = rustybuzz::Face::from_slice(&font.data, font.index) else {
        return Shaped {
            glyphs: Vec::new(),
            width: 0.0,
        };
    };
    let upem = font.metrics.upem;
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
    let output = rustybuzz::shape(&hb, &features, buffer);
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
