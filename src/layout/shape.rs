//! Text shaping: the glyphs and advances of a piece of text in a face, with
//! Word's defaults (no ligatures; kerning only when the run asks for it).

use super::fonts::{FaceId, FontBook};
use rustybuzz::{Direction, Feature, Script, ShapePlan, UnicodeBuffer, script, ttf_parser::Tag};
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

/// What a shaping plan depends on: the face, the direction and script of
/// the text, and whether it is kerned.
type PlanKey = (FaceId, Direction, Option<Script>, bool);

/// Shapes text, keeping the results, the faces it has read and the
/// shaping plans it has made.
#[derive(Default)]
pub struct Shaper {
    /// Shaped text, by face, text, kerning and direction.
    cache: HashMap<(FaceId, String, bool, bool), Rc<Shaped>>,
    /// The faces read so far.
    faces: HashMap<FaceId, ParsedFace>,
    /// The shaping plans made so far: a plan (the features and lookups to
    /// apply) is the same for every text of a face in one script, and
    /// making it costs about as much as shaping a word.
    plans: HashMap<PlanKey, ShapePlan>,
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
            Some(hb) => {
                let buffer = buffer(text, rtl);
                // As `rustybuzz::shape` makes it: a text of digits and
                // punctuation only has no script.
                let script = Some(buffer.script()).filter(|found| *found != script::UNKNOWN);
                let plan = self
                    .plans
                    .entry((face, buffer.direction(), script, kern))
                    .or_insert_with(|| ShapePlan::new(hb, buffer.direction(), script, None, &features(kern)));
                glyphs(rustybuzz::shape_with_plan(hb, plan, buffer), font.metrics.upem)
            }
            None => Shaped {
                glyphs: Vec::new(),
                width: 0.0,
            },
        });
        self.cache.insert(key, shaped.clone());
        shaped
    }
}

/// A buffer of `text`, with its script and direction (right to left when
/// `rtl` is set).
fn buffer(text: &str, rtl: bool) -> UnicodeBuffer {
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
    buffer.guess_segment_properties();
    if rtl {
        buffer.set_direction(Direction::RightToLeft);
    }
    buffer
}

/// Word's defaults: no ligatures, and no kerning unless `kern` is set.
fn features(kern: bool) -> Vec<Feature> {
    let off = |tag: &[u8; 4]| Feature::new(Tag::from_bytes(tag), 0, ..);
    let mut features = vec![off(b"liga"), off(b"clig")];
    if !kern {
        features.push(off(b"kern"));
    }
    features
}

/// The glyphs of the shaped `output`, in a face of `upem` units per em.
fn glyphs(output: rustybuzz::GlyphBuffer, upem: f32) -> Shaped {
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
