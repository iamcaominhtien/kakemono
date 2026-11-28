//! A paragraph broken into lines the way Word breaks them: words placed
//! one after the other until one does not fit, tabs to their stops, lines
//! justified by widening the spaces, line heights from the font metrics
//! (`usWinAscent`, `usWinDescent` and the external leading) and the line
//! spacing rule.

use super::display::{Glyph, Item, TextRun, offset};
use super::fonts::{FaceId, FontBook};
use super::metrics::MetricFace;
use super::shape::{Shaped, Shaper};
use crate::word::fmt::model::{
    Align, Anchor, BreakKind, FDoc, FieldKind, Inline, LineSpacing, NumFormat, NumberMark, NumberSuffix, Para,
    ParaProps, Rgb, RunFmt, Shape, TabKind, TabStop, Underline, VertAlign,
};
use std::collections::HashMap;
use std::rc::Rc;

/// Superscripts and subscripts are set at this share of the font size.
const SCRIPT_SIZE_RATIO: f32 = 2.0 / 3.0;
/// Superscripts are raised by this share of the font size.
const SUPER_RISE_RATIO: f32 = 1.0 / 3.0;
/// Subscripts are lowered by this share of the font size.
const SUB_DROP_RATIO: f32 = 0.14;
/// Small capitals are set at this share of the font size.
const SMALL_CAPS_RATIO: f32 = 0.8;
/// A tab that reaches no stop before the end of the line moves to the next
/// line; this much past the end is still accepted, in points.
const TAB_OVERFLOW_PT: f32 = 0.5;
/// What Word shows for a reference to a bookmark that does not exist.
const MISSING_BOOKMARK: &str = "Error! Bookmark not defined.";
/// Resolution at which Word measures the cells of leader characters, in
/// dots per inch.
const LAYOUT_DPI: f32 = 600.0;
/// Points per inch.
const PT_PER_INCH: f32 = 72.0;
/// The baseline of a line of exact height, as a share of the height.
const EXACT_BASELINE_RATIO: f32 = 0.8;

/// The baseline of a line raised to a minimum height `value`: Word splits
/// the added height evenly above and below the text.
fn atleast_baseline(value: f32, ascent: f32, descent: f32, leading: f32) -> f32 {
    let natural = ascent + descent + leading;
    leading + ascent + (value - natural) / 2.0
}

/// Share of the width of its spaces a justified line may lose to fit one
/// more word.
const JUSTIFY_SHRINK_RATIO: f32 = 0.25;

/// Share of its own width a word may stick out of a justified line, whose
/// spaces are then squeezed, and still stay on it (measured on Word's PDFs:
/// with the limit on the spaces, it gives Word's choice for 9 words in 10).
const JUSTIFY_OVERHANG_RATIO: f32 = 0.45;
/// Lines fit when their text exceeds the width by less than this, in points
/// (rounding).
const FIT_TOLERANCE_PT: f32 = 0.01;

/// How a piece of text is set: face, faked bold and italic, size, small
/// capitals, and the metrics of a missing font.
type PieceKey = (FaceId, bool, bool, f32, bool, Option<Metric>);

/// The face choice for a character: [`PieceKey`] without small capitals.
type FaceChoice = (FaceId, bool, bool, f32, Option<Metric>);

/// Built-in metrics of a font, told apart by their address (there is one
/// of each in the table).
#[derive(Debug, Clone, Copy)]
struct Metric(&'static super::metrics::MetricFace);

impl PartialEq for Metric {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}

/// Font slot of a run for East Asian characters (`w:eastAsia`).
const SLOT_EAST_ASIA: usize = 0;
/// Font slot for complex scripts and right-to-left text (`w:cs`).
const SLOT_COMPLEX: usize = 1;
/// Font slot for ASCII characters (`w:ascii`).
const SLOT_ASCII: usize = 2;
/// Font slot for the other characters (`w:hAnsi`).
const SLOT_HANSI: usize = 3;
/// Number of font slots of a run.
const SLOTS: usize = 4;

/// A font slot of a run, resolved: what its face choice is for every
/// character, before the character's own coverage.
#[derive(Debug, Clone, Copy)]
struct SlotFace {
    /// The face of its family.
    face: FaceId,
    /// Bold is faked on that face.
    fake_bold: bool,
    /// Italics are faked on that face.
    fake_italic: bool,
    /// Font size, in points.
    size: f32,
    /// Its family is not installed (only a stand-in is).
    missing: bool,
    /// Built-in metrics of its family, if any.
    metric: Option<&'static super::metrics::MetricFace>,
}

/// The font slot that `ch` takes its font from in a run of format `fmt`,
/// as Word chooses it.
fn font_slot(ch: char, fmt: &RunFmt) -> usize {
    if is_east_asian(ch) {
        SLOT_EAST_ASIA
    } else if fmt.cs || fmt.rtl || is_complex(ch) {
        SLOT_COMPLEX
    } else if ch.is_ascii() {
        SLOT_ASCII
    } else {
        SLOT_HANSI
    }
}

/// Narrowest and widest a stand-in face's glyphs are drawn to fit the
/// widths of the font they stand for.
const MIN_STAND_IN_SCALE: f32 = 0.75;
/// See [`MIN_STAND_IN_SCALE`].
const MAX_STAND_IN_SCALE: f32 = 1.25;

/// What a paragraph's layout needs to know about its surroundings.
pub struct Ctx<'a> {
    /// The fonts.
    pub book: &'a mut FontBook,
    /// The shaper.
    pub shaper: &'a mut Shaper,
    /// The document.
    pub doc: &'a FDoc,
    /// Number of each note, by (endnote, id).
    pub notes: &'a HashMap<(bool, String), String>,
    /// Number of the note being laid out, for its own mark.
    pub own_note: Option<String>,
    /// Page number to show, when known.
    pub page: Option<(u32, NumFormat)>,
    /// Number of pages, when known.
    pub page_count: Option<u32>,
    /// Number of pages of the section, when known.
    pub section_pages: Option<u32>,
    /// Faces that cover each character, cached.
    pub coverage: &'a mut HashMap<(FaceId, char), bool>,
    /// The page number of each bookmark, once the document has been laid
    /// out (`None` on the first pass).
    pub page_refs: Option<&'a HashMap<String, String>>,
    /// Distance from the left edge of the page to the start of the lines
    /// (leader characters are placed on a grid that starts at the page's
    /// edge).
    pub origin: f32,
}

/// A laid-out line of a paragraph.
#[derive(Debug, Clone, Default)]
pub struct Line {
    /// Height of the line.
    pub height: f32,
    /// Baseline, from the top of the line.
    pub baseline: f32,
    /// Natural ascent, descent and leading of its text.
    pub metrics: (f32, f32, f32),
    /// Height that must fit above the bottom margin: Word lets the space
    /// that multiple spacing adds below the text run into the margin.
    pub fit_height: f32,
    /// What it draws, relative to the left edge of the column and the top
    /// of the line.
    pub items: Vec<Item>,
    /// It ends with a page break.
    pub page_break: bool,
    /// It ends with a column break.
    pub column_break: bool,
    /// It holds pictures or shapes and no text.
    pub pictures_only: bool,
    /// The notes it refers to: (endnote, id).
    pub notes: Vec<(bool, String)>,
    /// Bookmarks that start on it.
    pub bookmarks: Vec<String>,
    /// Inline objects that are drawn after placement: shapes in the line,
    /// with their x position.
    pub shapes: Vec<(f32, Shape)>,
}

/// A floating object anchored in a paragraph.
#[derive(Debug, Clone)]
pub struct Float {
    /// Index of the line it is anchored on.
    pub line: usize,
    /// Horizontal position of the anchor in its line.
    pub x: f32,
    /// What it is.
    pub kind: FloatKind,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Its placement.
    pub anchor: Anchor,
}

/// What floats.
#[derive(Debug, Clone)]
pub enum FloatKind {
    /// A picture: its index (none when it cannot be drawn) and cropping.
    Image(Option<usize>, [f32; 4]),
    /// A shape.
    Shape(Box<Shape>),
}

/// A paragraph laid out at a width.
#[derive(Debug, Clone, Default)]
pub struct ParaLayout {
    /// Its lines.
    pub lines: Vec<Line>,
    /// Its floating objects.
    pub floats: Vec<Float>,
}

/// A run of glyphs of one face and format, measured.
#[derive(Debug, Clone)]
struct Piece {
    face: FaceId,
    size: f32,
    fake_bold: bool,
    fake_italic: bool,
    scale: f32,
    /// The format of its run, shared by the pieces of the run (a long
    /// paragraph holds a piece for each word while it is laid out).
    fmt: Rc<RunFmt>,
    glyphs: Vec<Glyph>,
    text: String,
    width: f32,
    /// Raise above the baseline.
    rise: f32,
    ascent: f32,
    descent: f32,
    leading: f32,
    /// Number of space characters (for justification).
    spaces: usize,
    /// Underline position and thickness, strike position.
    lines: (f32, f32, f32),
}

/// Something placed on a line.
#[derive(Debug, Clone)]
enum Atom {
    /// A word: its text pieces, the width of its body and of its
    /// trailing spaces.
    Word {
        pieces: Vec<Piece>,
        body: f32,
        trailing: f32,
    },
    /// A tab.
    Tab { fmt: RunFmt },
    /// A break.
    Break(BreakKind),
    /// A picture in the line.
    Image {
        index: Option<usize>,
        width: f32,
        height: f32,
        crop: [f32; 4],
        fmt: RunFmt,
    },
    /// A shape in the line.
    Shape(Box<Shape>),
    /// A floating object: placed apart, anchored here.
    Float(Float),
    /// A note reference (as a word, with its note id).
    NoteRef { word: Box<Atom>, endnote: bool, id: String },
    /// A bookmark.
    Bookmark(String),
}

/// How a line ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineEnd {
    /// The next word did not fit.
    Wrap,
    /// A break.
    Break(BreakKind),
    /// The paragraph ends.
    Last,
}

/// An atom placed on the line being built.
#[derive(Debug, Clone)]
struct Placed {
    atom: Atom,
    x: f32,
    /// Extra width given to each space for justification.
    space_extra: f32,
    /// The tab stop this atom is aligned on, when it follows a tab: the
    /// segment from the tab to the next one is moved to it.
    tab: Option<usize>,
}

impl Ctx<'_> {
    /// The face and size used for `ch` in a run of format `fmt`.
    fn face_for(&mut self, ch: char, fmt: &RunFmt) -> FaceChoice {
        let slot = self.slot_face(font_slot(ch, fmt), fmt);
        self.face_in_slot(ch, fmt, &slot)
    }

    /// The face and size for `ch` in a run of format `fmt` whose font for
    /// `ch` (its slot, [`font_slot`]) is `slot`: the slot's face when it
    /// has the character, a fallback face otherwise, and the built-in
    /// metrics that measure it.
    fn face_in_slot(&mut self, ch: char, fmt: &RunFmt, slot: &SlotFace) -> FaceChoice {
        let (face, fake_bold, fake_italic) = if ch.is_whitespace() || ch.is_control() || self.covered(slot.face, ch) {
            (slot.face, slot.fake_bold, slot.fake_italic)
        } else {
            self.book
                .fallbacks()
                .into_iter()
                .find(|&fallback| self.covered(fallback, ch))
                .map_or((slot.face, slot.fake_bold, slot.fake_italic), |fallback| {
                    (fallback, fmt.bold, fmt.italic)
                })
        };
        // A missing font with known metrics: measured with them. A face
        // known by its vertical metrics alone applies to a stand-in too
        // (Caladea has Cambria's widths, not its line height).
        let metric = slot.metric.filter(|metric| {
            metric.vertical_only() || slot.missing && (ch.is_whitespace() || metric.advance(ch).is_some())
        });
        (face, fake_bold, fake_italic, slot.size, metric.map(Metric))
    }

    /// What does not depend on the character in the font slot `slot` of
    /// format `fmt`: its face, size, whether its family is missing and
    /// its built-in metrics. Resolving them per character was a quarter
    /// of the layout's time.
    fn slot_face(&mut self, slot: usize, fmt: &RunFmt) -> SlotFace {
        let (family, size) = match slot {
            SLOT_EAST_ASIA => (&fmt.font_east_asia, fmt.size),
            SLOT_COMPLEX => (&fmt.font_cs, fmt.size_cs),
            SLOT_ASCII => (&fmt.font, fmt.size),
            _ => (&fmt.font_hansi, fmt.size),
        };
        let style = self.book.style(family, fmt.bold, fmt.italic);
        SlotFace {
            face: style.face,
            fake_bold: style.fake_bold,
            fake_italic: style.fake_italic,
            size,
            missing: style.missing,
            metric: style.metric,
        }
    }

    /// Whether face `face` covers `ch`, cached.
    fn covered(&mut self, face: FaceId, ch: char) -> bool {
        if let Some(&covered) = self.coverage.get(&(face, ch)) {
            return covered;
        }
        let covered = self.book.covers(face, ch);
        self.coverage.insert((face, ch), covered);
        covered
    }

    /// Text in format `fmt` as pieces (split where the face changes).
    fn pieces(&mut self, text: &str, fmt: &RunFmt) -> Vec<Piece> {
        self.shared_pieces(text, &Rc::new(fmt.clone()))
    }

    /// [`Ctx::pieces`] with a format the pieces share.
    fn shared_pieces(&mut self, text: &str, fmt: &Rc<RunFmt>) -> Vec<Piece> {
        let text = transform_case(text, fmt);
        let mut pieces = Vec::new();
        let mut start = 0;
        let mut current: Option<PieceKey> = None;
        // The font slots of `fmt`, resolved when first used.
        let mut slots: [Option<SlotFace>; SLOTS] = [None; SLOTS];
        for (at, ch) in text.char_indices() {
            let small = fmt.small_caps && !fmt.caps && ch.is_lowercase();
            let index = font_slot(ch, fmt);
            let slot = match slots[index] {
                Some(slot) => slot,
                None => *slots[index].insert(self.slot_face(index, fmt)),
            };
            let (face, fake_bold, fake_italic, size, metric) = self.face_in_slot(ch, fmt, &slot);
            let key = (face, fake_bold, fake_italic, size, small, metric);
            match current {
                Some(old) if old == key => {}
                Some(old) => {
                    pieces.push(self.piece(&text[start..at], fmt, old));
                    start = at;
                    current = Some(key);
                }
                None => current = Some(key),
            }
        }
        if let Some(key) = current {
            pieces.push(self.piece(&text[start..], fmt, key));
        }
        pieces
    }

    /// One piece: text in one face.
    fn piece(&mut self, text: &str, fmt: &Rc<RunFmt>, key: PieceKey) -> Piece {
        let (face, fake_bold, fake_italic, base_size, small, metric) = key;
        let metric = metric.map(|metric| metric.0);
        let (shown, mut size) = if small {
            (text.to_uppercase(), base_size * SMALL_CAPS_RATIO)
        } else {
            (text.to_string(), base_size)
        };
        let mut rise = fmt.position;
        match fmt.vert {
            VertAlign::Super => {
                rise += base_size * SUPER_RISE_RATIO;
                size *= SCRIPT_SIZE_RATIO;
            }
            VertAlign::Sub => {
                rise -= base_size * SUB_DROP_RATIO;
                size *= SCRIPT_SIZE_RATIO;
            }
            VertAlign::Baseline => {}
        }
        let kern = fmt.kern_from.is_some_and(|from| size >= from);
        let shaped = self.shaper.shape(self.book, face, &shown, kern, fmt.rtl);
        let scale = fmt.scale / 100.0;
        let (glyphs, x, spaces, drawn_width) = place_glyphs(&shaped, &shown, metric, size, scale, fmt.spacing);
        let mut metrics = self.book.get(face).metrics;
        let mut draw_scale = scale;
        if let Some(metric) = metric {
            metrics.ascent = metric.ascent;
            metrics.descent = metric.descent;
            metrics.leading = metric.leading;
            // The stand-in's glyphs are narrowed or widened to the widths.
            let measured_width = (x - fmt.spacing * glyphs.len() as f32) / (size * scale);
            if drawn_width > 0.0 && measured_width > 0.0 {
                draw_scale = scale * (measured_width / drawn_width).clamp(MIN_STAND_IN_SCALE, MAX_STAND_IN_SCALE);
            }
        }
        Piece {
            face,
            size,
            fake_bold,
            fake_italic,
            scale: draw_scale,
            fmt: Rc::clone(fmt),
            glyphs,
            text: shown,
            width: x,
            rise,
            ascent: metrics.ascent * size + rise.max(0.0),
            descent: metrics.descent * size + (-rise).max(0.0),
            leading: metrics.leading * size,
            spaces,
            lines: (
                metrics.underline_pos * size,
                metrics.underline_thickness * size,
                metrics.strike_pos * size,
            ),
        }
    }

    /// The single line height parts (ascent, descent, leading) of format
    /// `fmt`, for empty lines and line heights.
    pub(crate) fn line_metrics(&mut self, fmt: &RunFmt) -> (f32, f32, f32) {
        let ch = 'x';
        let (face, _, _, size, metric) = self.face_for(ch, fmt);
        if let Some(Metric(metric)) = metric {
            return (metric.ascent * size, metric.descent * size, metric.leading * size);
        }
        let metrics = self.book.get(face).metrics;
        (metrics.ascent * size, metrics.descent * size, metrics.leading * size)
    }
}

/// Text with capitals applied.
fn transform_case(text: &str, fmt: &RunFmt) -> String {
    if fmt.caps {
        text.to_uppercase()
    } else {
        text.to_string()
    }
}

/// Whether `ch` is set in the East Asian font.
fn is_east_asian(ch: char) -> bool {
    matches!(u32::from(ch),
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x2FF0..=0x303F | 0x3040..=0x30FF | 0x3100..=0x312F
        | 0x3130..=0x318F | 0x3190..=0x31FF | 0x3200..=0x4DBF | 0x4E00..=0x9FFF | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// Whether `ch` is in a complex script (set in the complex-script font).
fn is_complex(ch: char) -> bool {
    matches!(u32::from(ch),
        0x0590..=0x08FF | 0x0900..=0x0DFF | 0x0E00..=0x0EFF | 0x0F00..=0x0FFF | 0x1000..=0x109F
        | 0x1780..=0x17FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Whether a line may break after `ch` when `next` follows.
fn breaks_after(ch: char, next: Option<char>) -> bool {
    match ch {
        ' ' | '\u{200B}' | '\t' => !matches!(next, Some(' ')),
        '-' | '\u{2010}' | '\u{2012}' | '\u{2013}' | '\u{2014}' => {
            next.is_some_and(|next| next.is_alphanumeric() || is_east_asian(next))
        }
        '\u{00AD}' => true,
        _ => {
            (is_east_asian(ch) || next.is_some_and(is_east_asian))
                && !NO_LINE_END.contains(ch)
                && !next.is_some_and(|next| NO_LINE_START.contains(next))
        }
    }
}

/// The letters a line may not end with, by Word's standard East Asian line
/// breaking rules (kinsoku): opening brackets and quotes, currency signs.
const NO_LINE_END: &str = "$([{£¥‘“〈《「『【〔＄（［｛｢￡￥";

/// The letters a line may not start with, by the same rules: closing
/// brackets and quotes, punctuation, iteration marks, sound marks.
const NO_LINE_START: &str = "!%),.:;?]}¢°’”‰′″℃、。々〉》」』】〕゛゜ゝゞ・ヽヾ！％），．：；？］｝｡｣､･ﾞﾟ￠";

/// The words of a stretch of text: (start, end of the body, end with the
/// trailing spaces), as byte offsets.
fn words(text: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut start = 0;
    let mut at = 0;
    while at < chars.len() {
        let (pos, ch) = chars[at];
        let next = chars.get(at + 1).map(|&(_, next)| next);
        if ch == ' ' {
            // A word's body ends at its first space; the spaces follow.
            let body_end = pos;
            let mut end = at;
            while end < chars.len() && chars[end].1 == ' ' {
                end += 1;
            }
            let end_byte = chars.get(end).map_or(text.len(), |&(pos, _)| pos);
            out.push((start, body_end, end_byte));
            start = end_byte;
            at = end;
            continue;
        }
        if breaks_after(ch, next) {
            let end = pos + ch.len_utf8();
            out.push((start, end, end));
            start = end;
        }
        at += 1;
    }
    if start < text.len() {
        out.push((start, text.len(), text.len()));
    }
    out
}

/// Lays out paragraph `para` in a column of width `width`; `first` tells
/// whether it is the paragraph's first fragment (for a first-line indent).
pub fn layout(ctx: &mut Ctx<'_>, para: &Para, width: f32) -> ParaLayout {
    let atoms = atoms(ctx, para);
    let props = &para.props;
    let mut builder = Builder {
        lines: Vec::new(),
        floats: Vec::new(),
        current: Vec::new(),
        x: 0.0,
        tab_stops: props.tabs.clone(),
        pending_bookmarks: Vec::new(),
        width,
    };
    // The number or bullet starts the first line, or, after a break that
    // starts the paragraph, the line after it.
    builder.x = props.left + props.first_line;
    let leading_break = atoms
        .iter()
        .find(|atom| !matches!(atom, Atom::Bookmark(_)))
        .is_some_and(|atom| matches!(atom, Atom::Break(BreakKind::Page | BreakKind::Column)));
    let mut number = para.number.as_ref();
    if !leading_break && let Some(number) = number.take() {
        place_number(ctx, para, number, &mut builder);
    }
    let mut first_line = true;
    for atom in atoms {
        let is_break = matches!(atom, Atom::Break(_));
        match atom {
            Atom::Float(mut float) => {
                float.line = builder.lines.len();
                float.x = builder.x;
                builder.floats.push(float);
            }
            Atom::Bookmark(name) => builder.pending_bookmarks.push(name),
            Atom::Break(kind) => {
                builder.finish_line(ctx, para, first_line, LineEnd::Break(kind));
                first_line = false;
                builder.x = line_bounds(props, width, false).0;
            }
            Atom::Tab { ref fmt } => place_tab(ctx, para, &mut builder, fmt.clone(), &mut first_line),
            atom => place_atom(ctx, para, &mut builder, atom, &mut first_line),
        }
        if is_break && let Some(number) = number.take() {
            builder.x = props.left + props.first_line;
            place_number(ctx, para, number, &mut builder);
        }
    }
    // The mark of a paragraph that ends with a page or column break stays
    // on the break's line, unless the document splits them (Word 97-2003
    // files, `w:splitPgBreakAndParaMark`).
    let ends_with_break = builder
        .lines
        .last()
        .is_some_and(|line| line.page_break || line.column_break)
        && !builder.has_content();
    if !ends_with_break || ctx.doc.split_break_and_mark {
        builder.finish_line(ctx, para, first_line, LineEnd::Last);
    }
    ParaLayout {
        lines: builder.lines,
        floats: builder.floats,
    }
}

/// The widths of paragraph `para`'s content, without indents: set on one
/// line (the widest of its lines between line breaks), of its widest word
/// (the narrowest it can be set without cutting a word), and of all its
/// lines together; and the number of its line breaks.
pub(crate) fn measure(ctx: &mut Ctx<'_>, para: &Para) -> (f32, f32, f32, usize) {
    let (mut natural, mut line, mut widest, mut trailing, mut total) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
    let mut breaks = 0;
    for atom in atoms(ctx, para) {
        if matches!(atom, Atom::Break(_)) {
            natural = natural.max(line - trailing);
            total += line - trailing;
            breaks += 1;
            (line, trailing) = (0.0, 0.0);
            continue;
        }
        let (body, after) = atom_width(&atom);
        line += body + after;
        widest = widest.max(body);
        if body + after > 0.0 {
            trailing = after;
        }
    }
    (natural.max(line - trailing), widest, total + line - trailing, breaks)
}

/// Widths of an atom: its body and its trailing spaces.
fn atom_width(atom: &Atom) -> (f32, f32) {
    match atom {
        Atom::Word { body, trailing, .. } => (*body, *trailing),
        Atom::Image { width, .. } => (*width, 0.0),
        Atom::Shape(shape) => (shape.width, 0.0),
        Atom::NoteRef { word, .. } => atom_width(word),
        _ => (0.0, 0.0),
    }
}

/// Cuts a word so that its first part fits in `room` (at least one
/// character is kept).
fn split_word(atom: Atom, room: f32) -> (Atom, Option<Atom>) {
    let Atom::Word { pieces, .. } = atom else {
        return (atom, None);
    };
    let mut head: Vec<Piece> = Vec::new();
    let mut tail: Vec<Piece> = Vec::new();
    let mut used = 0.0;
    let mut cut = false;
    for piece in pieces {
        if cut {
            tail.push(piece);
            continue;
        }
        if used + piece.width <= room {
            used += piece.width;
            head.push(piece);
            continue;
        }
        // Cut inside this piece, at a glyph.
        let mut keep = 0;
        let mut width = 0.0;
        for glyph in &piece.glyphs {
            if used + width + glyph.advance > room && (keep > 0 || !head.is_empty()) {
                break;
            }
            width += glyph.advance;
            keep += 1;
        }
        let (first, second) = cut_piece(piece, keep);
        if let Some(first) = first {
            head.push(first);
        }
        if let Some(second) = second {
            tail.push(second);
        }
        cut = true;
    }
    let make = |pieces: Vec<Piece>| {
        let width: f32 = pieces.iter().map(|piece| piece.width).sum();
        Atom::Word {
            pieces,
            body: width,
            trailing: 0.0,
        }
    };
    let tail = (!tail.is_empty()).then(|| make(tail));
    (make(head), tail)
}

/// A piece cut after its first `keep` glyphs.
fn cut_piece(piece: Piece, keep: usize) -> (Option<Piece>, Option<Piece>) {
    if keep == 0 {
        return (None, Some(piece));
    }
    // Glyphs out of text order (right-to-left text) are not cut.
    let in_order = piece.glyphs.windows(2).all(|pair| pair[0].text.0 <= pair[1].text.0);
    if keep >= piece.glyphs.len() || !in_order {
        return (Some(piece), None);
    }
    let split = piece.glyphs[keep].text.0;
    let split_byte = split as usize;
    let mut first = piece.clone();
    first.glyphs.truncate(keep);
    first.text = piece.text[..split_byte].to_string();
    first.width = first.glyphs.iter().map(|glyph| glyph.advance).sum();
    let mut second = piece.clone();
    let offset = second.glyphs[keep].x;
    second.glyphs = piece.glyphs[keep..]
        .iter()
        .map(|glyph| Glyph {
            x: glyph.x - offset,
            text: (glyph.text.0 - split, glyph.text.1 - split),
            ..*glyph
        })
        .collect();
    second.text = piece.text[split_byte..].to_string();
    second.width = second.glyphs.iter().map(|glyph| glyph.advance).sum();
    (Some(first), Some(second))
}

/// The next tab stop after `x`: a custom stop, the hanging indent (first
/// line), or a default stop.
fn next_stop(x: f32, tabs: &[TabStop], left: f32, first_line: f32, default_tab: f32, first: bool) -> Option<TabStop> {
    const EPS: f32 = 0.01;
    let custom = tabs
        .iter()
        .find(|stop| stop.pos > x + EPS && stop.kind != TabKind::Bar)
        .copied();
    // The hanging indent acts as a stop on the first line.
    let hanging = (first && first_line < 0.0 && left > x + EPS).then_some(TabStop {
        pos: left,
        kind: TabKind::Left,
        leader: None,
    });
    let candidate = match (custom, hanging) {
        (Some(custom), Some(hanging)) if hanging.pos < custom.pos => Some(hanging),
        (Some(custom), _) => Some(custom),
        (None, hanging) => hanging,
    };
    if candidate.is_some() {
        return candidate;
    }
    if default_tab <= 0.0 {
        return None;
    }
    // Default stops, past the last custom one.
    let last_custom = tabs.iter().map(|stop| stop.pos).fold(f32::MIN, f32::max);
    let from = x.max(last_custom);
    let pos = ((from + EPS) / default_tab).floor() * default_tab + default_tab;
    Some(TabStop {
        pos,
        kind: TabKind::Left,
        leader: None,
    })
}

/// The atoms of a paragraph.
fn atoms(ctx: &mut Ctx<'_>, para: &Para) -> Vec<Atom> {
    let mut atoms = Vec::new();
    for name in &para.bookmarks {
        atoms.push(Atom::Bookmark(name.clone()));
    }
    // Consecutive text runs are broken into words together.
    let mut stretch: Vec<(String, RunFmt)> = Vec::new();
    for inline in &para.inlines {
        match inline {
            Inline::Text { text, fmt } => {
                if !fmt.hidden {
                    stretch.push((text.clone(), fmt.clone()));
                }
                continue;
            }
            Inline::Field { kind, fmt, format } => {
                stretch.push((field_value(ctx, kind, format.as_ref()), fmt.clone()));
                continue;
            }
            _ => {}
        }
        flush_stretch(ctx, &mut stretch, &mut atoms);
        match inline {
            Inline::Tab { fmt } => atoms.push(Atom::Tab { fmt: fmt.clone() }),
            Inline::Break { kind, .. } => atoms.push(Atom::Break(*kind)),
            Inline::Image { .. } | Inline::Shape(_) => atoms.extend(object_atom(inline)),
            Inline::NoteRef { endnote, id, fmt } => {
                let number = ctx.notes.get(&(*endnote, id.clone())).cloned().unwrap_or_default();
                atoms.push(Atom::NoteRef {
                    word: Box::new(number_word(ctx, &number, fmt)),
                    endnote: *endnote,
                    id: id.clone(),
                });
            }
            Inline::NoteNumber { fmt } => {
                let number = ctx.own_note.clone().unwrap_or_default();
                atoms.push(number_word(ctx, &number, fmt));
            }
            Inline::Math { node, fmt, display } => {
                let text = crate::word::omml::plain_text(node);
                let mut fmt = fmt.clone();
                fmt.italic = true;
                fmt.font = "Cambria Math".into();
                fmt.font_hansi = "Cambria Math".into();
                if *display {
                    atoms.push(Atom::Break(BreakKind::Line));
                }
                stretch.push((text, fmt));
                flush_stretch(ctx, &mut stretch, &mut atoms);
                if *display {
                    atoms.push(Atom::Break(BreakKind::Line));
                }
            }
            Inline::Text { .. } | Inline::Field { .. } => {}
        }
    }
    flush_stretch(ctx, &mut stretch, &mut atoms);
    atoms
}

/// The text a field shows: the page number (in `format`, or else the
/// section's), the number of pages or of the section's pages; "1" when it
/// is not known.
fn field_value(ctx: &Ctx<'_>, kind: &FieldKind, format: Option<&NumFormat>) -> String {
    let value = match kind {
        FieldKind::PageRef(name) => ctx
            .page_refs
            .map(|refs| refs.get(name).cloned().unwrap_or_else(|| MISSING_BOOKMARK.to_string())),
        FieldKind::Page => ctx.page.clone().map(|(number, section_format)| {
            crate::word::fmt::model::format_page(number, format.unwrap_or(&section_format))
        }),
        FieldKind::NumPages => ctx.page_count.map(|count| count.to_string()),
        FieldKind::SectionPages => ctx.section_pages.map(|count| count.to_string()),
    };
    value.unwrap_or_else(|| "1".into())
}

/// The atom of a picture or shape: a float when it is anchored, else an
/// atom in the line. `None` for any other inline.
fn object_atom(inline: &Inline) -> Option<Atom> {
    match inline {
        Inline::Image {
            index,
            width,
            height,
            anchor,
            fmt,
            crop,
            ..
        } => Some(match anchor {
            Some(anchor) => Atom::Float(Float {
                line: 0,
                x: 0.0,
                kind: FloatKind::Image(*index, *crop),
                width: *width,
                height: *height,
                anchor: anchor.clone(),
            }),
            None => Atom::Image {
                index: *index,
                width: *width,
                height: *height,
                crop: *crop,
                fmt: fmt.clone(),
            },
        }),
        Inline::Shape(shape) => Some(match &shape.anchor {
            Some(anchor) => Atom::Float(Float {
                line: 0,
                x: 0.0,
                kind: FloatKind::Shape(shape.clone()),
                width: shape.width,
                height: shape.height,
                anchor: anchor.clone(),
            }),
            None => Atom::Shape(shape.clone()),
        }),
        _ => None,
    }
}

/// A note number `number` in format `fmt`, as one word.
fn number_word(ctx: &mut Ctx<'_>, number: &str, fmt: &RunFmt) -> Atom {
    let pieces = ctx.pieces(number, fmt);
    let width: f32 = pieces.iter().map(|piece| piece.width).sum();
    Atom::Word {
        pieces,
        body: width,
        trailing: 0.0,
    }
}

/// Breaks the collected text runs into word atoms.
fn flush_stretch(ctx: &mut Ctx<'_>, stretch: &mut Vec<(String, RunFmt)>, atoms: &mut Vec<Atom>) {
    if stretch.is_empty() {
        return;
    }
    let runs: Vec<(String, Rc<RunFmt>)> = std::mem::take(stretch)
        .into_iter()
        .map(|(text, fmt)| (text, Rc::new(fmt)))
        .collect();
    let mut text = String::new();
    let mut bounds = Vec::new();
    for (piece, _) in &runs {
        bounds.push(text.len());
        text.push_str(piece);
    }
    bounds.push(text.len());
    // The first run that ends after the start of the word: the words go
    // forward, and each only looks at the runs it overlaps (a paragraph
    // of 100,000 links).
    let mut first_run = 0;
    for (start, body_end, end) in words(&text) {
        while first_run < runs.len() && bounds[first_run + 1] <= start {
            first_run += 1;
        }
        let word = Stretch {
            text: &text,
            runs: &runs[first_run..],
            bounds: &bounds[first_run..],
        };
        atoms.push(word_atom(ctx, &word, (start, body_end, end)));
    }
}

/// The text of a stretch of runs, from a run on: the runs, and where each
/// starts in the text (and the last ends).
struct Stretch<'s> {
    text: &'s str,
    runs: &'s [(String, Rc<RunFmt>)],
    bounds: &'s [usize],
}

/// The word of `stretch` from `start` to `end` (its body ending at
/// `body_end`): its pieces in the formats of the runs it overlaps, the
/// first of which is the first of `stretch`.
fn word_atom(ctx: &mut Ctx<'_>, stretch: &Stretch<'_>, (start, body_end, end): (usize, usize, usize)) -> Atom {
    let mut pieces = Vec::new();
    let mut body = 0.0;
    let mut trailing = 0.0;
    for (at, (_, fmt)) in stretch.runs.iter().enumerate() {
        let (run_start, run_end) = (stretch.bounds[at], stretch.bounds[at + 1]);
        if run_start >= end {
            break;
        }
        for (part_start, part_end, is_body) in [(start, body_end, true), (body_end, end, false)] {
            let from = part_start.max(run_start);
            let to = part_end.min(run_end);
            if from >= to {
                continue;
            }
            let part = stretch.text[from..to].replace('\u{00AD}', "");
            if part.is_empty() {
                continue;
            }
            for piece in ctx.shared_pieces(&part, fmt) {
                if is_body {
                    body += piece.width;
                } else {
                    trailing += piece.width;
                }
                pieces.push(piece);
            }
        }
    }
    Atom::Word { pieces, body, trailing }
}

/// The left and right edges of a line of a paragraph with `props` in a
/// column of `width`; `first` is whether it is the first line.
fn line_bounds(props: &ParaProps, width: f32, first: bool) -> (f32, f32) {
    let left = if first {
        props.left + props.first_line
    } else {
        props.left
    };
    (left, width - props.right)
}

/// Places the number or bullet of `para` at the start of its first line,
/// followed by its tab, space or nothing.
fn place_number(ctx: &mut Ctx<'_>, para: &Para, number: &NumberMark, builder: &mut Builder) {
    let props = &para.props;
    let first_indent = props.left + props.first_line;
    let pieces = ctx.pieces(&number.text, &number.fmt);
    let number_width: f32 = pieces.iter().map(|piece| piece.width).sum();
    let start = match number.align {
        Align::Center => first_indent - number_width / 2.0,
        Align::Right => first_indent - number_width,
        _ => first_indent,
    };
    builder.current.push(Placed {
        atom: Atom::Word {
            pieces,
            body: number_width,
            trailing: 0.0,
        },
        x: start,
        space_extra: 0.0,
        tab: None,
    });
    builder.x = start + number_width;
    match number.suffix {
        NumberSuffix::Tab => {
            let stop = next_stop(
                builder.x,
                &props.tabs,
                props.left,
                props.first_line,
                ctx.doc.default_tab,
                true,
            );
            builder.x = stop.map_or(builder.x, |stop| stop.pos.max(builder.x));
        }
        NumberSuffix::Space => {
            let space = ctx.pieces(" ", &number.fmt);
            builder.x += space.iter().map(|piece| piece.width).sum::<f32>();
        }
        NumberSuffix::Nothing => {}
    }
}

/// Places a tab of format `fmt`: it moves to the next tab stop, or, when no
/// stop is left on the line, starts the next line. `first_line` is whether
/// the line being built is the paragraph's first, and is cleared when a line
/// ends.
fn place_tab(ctx: &mut Ctx<'_>, para: &Para, builder: &mut Builder, fmt: RunFmt, first_line: &mut bool) {
    let props = &para.props;
    let (_, right) = line_bounds(props, builder.width, *first_line);
    let stop = next_stop(
        builder.x,
        &props.tabs,
        props.left,
        if *first_line { props.first_line } else { 0.0 },
        ctx.doc.default_tab,
        *first_line,
    );
    let stop = stop.filter(|stop| stop.pos <= right + TAB_OVERFLOW_PT || stop.kind != TabKind::Left);
    match stop {
        Some(stop) => {
            builder.close_tab_segment();
            let index = builder
                .tab_stops
                .iter()
                .position(|known| *known == stop)
                .unwrap_or_else(|| {
                    builder.tab_stops.push(stop);
                    builder.tab_stops.len() - 1
                });
            let start = builder.x;
            builder.current.push(Placed {
                atom: Atom::Tab { fmt },
                x: start,
                space_extra: 0.0,
                tab: Some(index),
            });
            if stop.kind == TabKind::Left || stop.kind == TabKind::Bar {
                builder.x = stop.pos.max(start);
            }
        }
        None => {
            // No stop left on this line: the tab starts the next.
            builder.finish_line(ctx, para, *first_line, LineEnd::Wrap);
            *first_line = false;
            builder.x = line_bounds(props, builder.width, false).0;
            let stop = next_stop(builder.x, &props.tabs, props.left, 0.0, ctx.doc.default_tab, false);
            if let Some(stop) = stop {
                builder.x = stop.pos;
            }
        }
    }
}

/// Places a word, picture or other atom: on the current line when it fits,
/// else on a new line; a word longer than a whole line is cut. `first_line`
/// is whether the line being built is the paragraph's first, and is cleared
/// when a line ends.
fn place_atom(ctx: &mut Ctx<'_>, para: &Para, builder: &mut Builder, atom: Atom, first_line: &mut bool) {
    let props = &para.props;
    let (body, trailing) = atom_width(&atom);
    let (_, right) = line_bounds(props, builder.width, *first_line);
    // Word squeezes the spaces of a justified line to fit one more word,
    // when most of the word is inside the line.
    let shrink = if matches!(props.align, Align::Both | Align::Distribute) && ctx.doc.compat_mode >= 15 {
        (builder.space_width() * justify_shrink()).min(body * JUSTIFY_OVERHANG_RATIO)
    } else {
        0.0
    };
    let fits = builder.x + body - shrink <= right + FIT_TOLERANCE_PT;
    if !fits
        && matches!(atom, Atom::Word { .. })
        && builder.current.len() > 1
        && let Some(Placed {
            atom: Atom::Tab { fmt },
            x,
            ..
        }) = builder.current.last().cloned()
    {
        // A word that does not fit after a tab takes the tab to the next
        // line with it.
        builder.current.pop();
        builder.x = x;
        builder.finish_line(ctx, para, *first_line, LineEnd::Wrap);
        *first_line = false;
        builder.x = line_bounds(props, builder.width, false).0;
        place_tab(ctx, para, builder, fmt, first_line);
        let (_, right) = line_bounds(props, builder.width, *first_line);
        if builder.x + body > right + FIT_TOLERANCE_PT {
            cut_word(ctx, para, builder, atom, first_line);
        } else {
            builder.push(atom, body + trailing);
        }
        return;
    }
    if !fits && builder.has_content() {
        builder.finish_line(ctx, para, *first_line, LineEnd::Wrap);
        *first_line = false;
        builder.x = line_bounds(props, builder.width, false).0;
    }
    let (_, right) = line_bounds(props, builder.width, *first_line);
    if builder.x + body > right + FIT_TOLERANCE_PT && !builder.has_content() {
        cut_word(ctx, para, builder, atom, first_line);
        return;
    }
    builder.push(atom, body + trailing);
}

/// Places a word longer than the room left on the line, cut at the right
/// edge of each line it runs over.
fn cut_word(ctx: &mut Ctx<'_>, para: &Para, builder: &mut Builder, atom: Atom, first_line: &mut bool) {
    let props = &para.props;
    let mut rest = Some(atom);
    while let Some(atom) = rest.take() {
        let (_, right) = line_bounds(props, builder.width, *first_line);
        let room = right - builder.x;
        let (head, tail) = split_word(atom, room);
        let (head_body, head_trailing) = atom_width(&head);
        builder.push(head, head_body + head_trailing);
        if let Some(tail) = tail {
            builder.finish_line(ctx, para, *first_line, LineEnd::Wrap);
            *first_line = false;
            builder.x = line_bounds(props, builder.width, false).0;
            rest = Some(tail);
        }
    }
}

/// The shaped glyphs of `shown` placed one after another, at `size` and
/// horizontal `scale`: `metric` (the widths of the font Word used) gives the
/// advances when it is known, and `spacing` is added after each glyph. Returns the
/// glyphs, their total advance, the number of spaces and the width of the
/// glyphs as the face draws them, in em.
fn place_glyphs(
    shaped: &Shaped,
    shown: &str,
    metric: Option<&MetricFace>,
    size: f32,
    scale: f32,
    spacing: f32,
) -> (Vec<Glyph>, f32, usize, f32) {
    let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
    let mut x = 0.0;
    let mut spaces = 0;
    // The clusters in order, to find where each ends (the next one after
    // it) by a binary search: a scan of every cluster for each glyph was
    // quadratic, minutes for a cell of 32,000 characters. Right-to-left
    // text has its clusters in falling order, hence the sort.
    let mut clusters: Vec<usize> = shaped.glyphs.iter().map(|glyph| glyph.cluster).collect();
    clusters.sort_unstable();
    clusters.dedup();
    let mut drawn_width = 0.0;
    for (at, glyph) in shaped.glyphs.iter().enumerate() {
        let start = glyph.cluster;
        // The width of the font Word used, when it is known.
        let first_of_cluster = at == 0 || shaped.glyphs[at - 1].cluster != start;
        let measured = metric.and_then(|metric| {
            let ch = shown[start..].chars().next()?;
            if first_of_cluster {
                metric.advance(ch)
            } else {
                Some(0.0)
            }
        });
        drawn_width += glyph.advance;
        let advance = measured.unwrap_or(glyph.advance) * size * scale + spacing;
        let end = clusters
            .get(clusters.partition_point(|&cluster| cluster <= start))
            .copied()
            .unwrap_or(shown.len());
        if shown[start..end].starts_with(' ') {
            spaces += 1;
        }
        glyphs.push(Glyph {
            id: glyph.id,
            x,
            advance,
            dx: glyph.dx * size * scale,
            dy: glyph.dy * size,
            text: (offset(start), offset(end)),
        });
        x += advance;
    }
    (glyphs, x, spaces, drawn_width)
}

/// Builds lines.
struct Builder {
    lines: Vec<Line>,
    floats: Vec<Float>,
    current: Vec<Placed>,
    x: f32,
    tab_stops: Vec<TabStop>,
    pending_bookmarks: Vec<String>,
    width: f32,
}

/// Share of the width of its spaces a justified line may lose to fit a
/// word (`KAKEMONO_JUSTIFY_SHRINK` overrides it, for tuning).
fn justify_shrink() -> f32 {
    static SHRINK: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SHRINK.get_or_init(|| {
        std::env::var("KAKEMONO_JUSTIFY_SHRINK")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(JUSTIFY_SHRINK_RATIO)
    })
}

impl Builder {
    /// Width of the spaces between the words on the line so far (after the
    /// last tab).
    fn space_width(&self) -> f32 {
        let start = self
            .current
            .iter()
            .rposition(|placed| matches!(placed.atom, Atom::Tab { .. }))
            .map_or(0, |at| at + 1);
        self.current[start..]
            .iter()
            .map(|placed| match &placed.atom {
                Atom::Word { trailing, .. } => *trailing,
                _ => 0.0,
            })
            .sum()
    }

    /// Whether the current line has something on it.
    fn has_content(&self) -> bool {
        self.current
            .iter()
            .any(|placed| !matches!(placed.atom, Atom::Bookmark(_)))
    }

    /// Adds an atom at the pen position.
    fn push(&mut self, atom: Atom, advance: f32) {
        self.current.push(Placed {
            atom,
            x: self.x,
            space_extra: 0.0,
            tab: None,
        });
        self.x += advance;
    }

    /// Aligns the text after the last right, centre or decimal tab on its
    /// stop.
    fn close_tab_segment(&mut self) {
        let Some(tab_at) = self
            .current
            .iter()
            .rposition(|placed| matches!(placed.atom, Atom::Tab { .. }))
        else {
            return;
        };
        let Some(stop_index) = self.current[tab_at].tab else {
            return;
        };
        let stop = self.tab_stops[stop_index];
        if stop.kind == TabKind::Left || stop.kind == TabKind::Bar {
            return;
        }
        let start = self.current[tab_at].x;
        let segment: Vec<&Placed> = self.current[tab_at + 1..].iter().collect();
        let width: f32 = segment
            .iter()
            .map(|placed| {
                let (body, trailing) = atom_width(&placed.atom);
                body + trailing
            })
            .sum();
        let body_width = segment
            .last()
            .map_or(width, |placed| width - atom_width(&placed.atom).1);
        let anchor = match stop.kind {
            TabKind::Right => stop.pos - body_width,
            TabKind::Center => stop.pos - body_width / 2.0,
            TabKind::Decimal => stop.pos - decimal_offset(&segment).unwrap_or(body_width),
            _ => start,
        };
        let shift = (anchor - start).max(0.0);
        for placed in &mut self.current[tab_at + 1..] {
            placed.x += shift;
        }
        self.x += shift;
    }

    /// Ends the current line.
    fn finish_line(&mut self, ctx: &mut Ctx<'_>, para: &Para, first: bool, end: LineEnd) {
        self.close_tab_segment();
        let props = &para.props;
        let (left, right) = if first {
            (props.left + props.first_line, self.width - props.right)
        } else {
            (props.left, self.width - props.right)
        };
        let mut placed = std::mem::take(&mut self.current);
        // Drop trailing spaces from the measure.
        let content_end = placed
            .iter()
            .rev()
            .find_map(|item| {
                let (body, _) = atom_width(&item.atom);
                (body > 0.0 || matches!(item.atom, Atom::Image { .. } | Atom::Shape(_))).then_some(item.x + body)
            })
            .unwrap_or(left);
        let last_tab = placed
            .iter()
            .rposition(|item| matches!(item.atom, Atom::Tab { .. }))
            .map_or(0, |at| at + 1);
        let free = right - content_end;
        let align = if props.bidi {
            match props.align {
                Align::Left => Align::Right,
                Align::Right => Align::Left,
                other => other,
            }
        } else {
            props.align
        };
        // Justified lines are widened, except the paragraph's last (a line
        // ended by a line break is widened too, as Word does).
        let justify = match align {
            Align::Both => {
                end == LineEnd::Wrap || end == LineEnd::Break(BreakKind::Line) && !ctx.doc.no_expand_shift_return
            }
            Align::Distribute => true,
            _ => false,
        };
        match align {
            Align::Center | Align::Right if free > 0.0 => {
                let shift = if align == Align::Center { free / 2.0 } else { free };
                for item in &mut placed[last_tab..] {
                    item.x += shift;
                }
            }
            _ => {}
        }
        // A justified line that squeezed in a word is squeezed even when it
        // is the last.
        let justify = justify || free < 0.0 && matches!(align, Align::Both | Align::Distribute);
        if justify && free.abs() > f32::EPSILON {
            justify_spaces(&mut placed, last_tab, free);
        }
        let line = self.make_line(ctx, para, placed, end);
        self.lines.push(line);
    }

    /// Turns placed atoms into a line with its height and items.
    fn make_line(&mut self, ctx: &mut Ctx<'_>, para: &Para, placed: Vec<Placed>, end: LineEnd) -> Line {
        let mut parts = self.line_parts(ctx, &placed);
        // The paragraph mark's font counts only on a line with nothing else.
        if !parts.text_seen && parts.images.is_empty() && parts.shapes.is_empty() {
            let (mark_ascent, mark_descent, mark_leading) = ctx.line_metrics(&para.mark);
            parts.ascent = parts.ascent.max(mark_ascent);
            parts.descent = parts.descent.max(mark_descent);
            parts.leading = parts.leading.max(mark_leading);
        }
        let (height, baseline) = parts.height_and_baseline(para.props.line);
        let LineParts {
            ascent,
            descent,
            leading,
            mut items,
            notes,
            shapes,
            text_seen,
            images,
            ..
        } = parts;
        // Items were placed with y = 0 on the baseline: move them.
        for item in &mut items {
            shift_item(item, baseline);
        }
        let has_images = !images.is_empty();
        for (x, index, width, height, crop) in images {
            if let Some(index) = index {
                items.push(Item::Image {
                    index,
                    x,
                    y: baseline - height,
                    width,
                    height,
                    crop,
                });
            }
        }
        let only_pictures = !text_seen && (has_images || !shapes.is_empty());
        let break_only = !text_seen
            && !has_images
            && shapes.is_empty()
            && matches!(end, LineEnd::Break(BreakKind::Page | BreakKind::Column));
        let fit_height = match para.props.line {
            // A line holding only a page break stays on its page below the
            // margin rather than leaving a blank page.
            _ if break_only => 0.0,
            LineSpacing::Auto(_) if only_pictures => baseline,
            LineSpacing::Auto(_) => (baseline + descent).min(height),
            _ => height,
        };
        Line {
            height,
            baseline,
            fit_height,
            metrics: (ascent, descent, leading),
            items,
            page_break: end == LineEnd::Break(BreakKind::Page),
            column_break: end == LineEnd::Break(BreakKind::Column),
            pictures_only: only_pictures,
            notes,
            bookmarks: std::mem::take(&mut self.pending_bookmarks),
            shapes,
        }
    }

    /// Gathers what the placed atoms of a line hold: their text items (with
    /// y = 0 on the baseline), tab leaders, notes, pictures and shapes, and
    /// the tallest ascent, descent and leading of their text.
    fn line_parts(&self, ctx: &mut Ctx<'_>, placed: &[Placed]) -> LineParts {
        let mut parts = LineParts::default();
        // A tab's font counts only on a line without text.
        let mut tab_metrics = (0.0f32, 0.0f32, 0.0f32);
        for item in placed {
            match &item.atom {
                Atom::Word { pieces, .. } => {
                    parts.text_seen = true;
                    parts.emit_pieces(pieces, item.x, item.space_extra);
                }
                Atom::NoteRef { word, endnote, id } => {
                    parts.notes.push((*endnote, id.clone()));
                    if let Atom::Word { pieces, .. } = word.as_ref() {
                        parts.emit_pieces(pieces, item.x, 0.0);
                    }
                }
                Atom::Image {
                    index,
                    width,
                    height,
                    crop,
                    fmt,
                } => {
                    let (_, fmt_descent, _) = ctx.line_metrics(fmt);
                    parts.picture_ascent = parts.picture_ascent.max(*height);
                    parts.picture_descent = parts.picture_descent.max(fmt_descent);
                    parts.images.push((item.x, *index, *width, *height, *crop));
                }
                Atom::Shape(shape) => {
                    parts.picture_ascent = parts.picture_ascent.max(shape.height);
                    parts.shapes.push((item.x, shape.as_ref().clone()));
                }
                Atom::Tab { fmt } => {
                    let (tab_ascent, tab_descent, tab_leading) = ctx.line_metrics(fmt);
                    tab_metrics.0 = tab_metrics.0.max(tab_ascent);
                    tab_metrics.1 = tab_metrics.1.max(tab_descent);
                    tab_metrics.2 = tab_metrics.2.max(tab_leading);
                    if let Some(stop) = item.tab.map(|index| self.tab_stops[index])
                        && let Some(leader) = stop.leader
                    {
                        let next = placed
                            .iter()
                            .skip_while(|other| !std::ptr::eq(*other, item))
                            .nth(1)
                            .map_or(stop.pos, |next| next.x);
                        emit_leader(ctx, leader, fmt, item.x, next, &mut parts.items);
                    }
                }
                _ => {}
            }
        }
        if parts.ascent == 0.0 {
            parts.ascent = tab_metrics.0;
            parts.descent = tab_metrics.1;
            parts.leading = tab_metrics.2;
        }
        parts
    }
}

/// A picture in a line: x, index (`None` for one that is not stored),
/// width, height and the share cut off each side.
type LineImage = (f32, Option<usize>, f32, f32, [f32; 4]);

/// Whether `font` is a symbol font (Symbol, Wingdings).
fn is_symbol_font(font: &str) -> bool {
    let font = font.to_ascii_lowercase();
    font == "symbol" || font.starts_with("wingdings")
}

/// What the atoms of a line hold, gathered before the line's height is
/// known.
#[derive(Default)]
struct LineParts {
    /// Tallest ascent of the text.
    ascent: f32,
    /// Deepest descent of the text.
    descent: f32,
    /// Largest leading of the text.
    leading: f32,
    /// Items, placed with y = 0 on the baseline.
    items: Vec<Item>,
    /// The notes the line refers to: (endnote, id).
    notes: Vec<(bool, String)>,
    /// Shapes in the line, with their x.
    shapes: Vec<(f32, Shape)>,
    /// Whether the line has text.
    text_seen: bool,
    /// Tallest picture or shape.
    picture_ascent: f32,
    /// Deepest descent of the pictures' formats.
    picture_descent: f32,
    /// Pictures in the line.
    images: Vec<LineImage>,
}

impl LineParts {
    /// Emits the pieces of a word at `x`, with `space_extra` added to each
    /// space, and takes in their ascent, descent and leading.
    fn emit_pieces(&mut self, pieces: &[Piece], x: f32, space_extra: f32) {
        let mut x = x;
        for piece in pieces {
            // The leading sits above the ascent of each font: the line rises
            // to the highest of the two together.
            let top = (self.leading + self.ascent).max(piece.leading + piece.ascent);
            self.leading = self.leading.max(piece.leading);
            self.ascent = top - self.leading;
            // Word does not lower the line for a symbol font's descent (a
            // bullet in Symbol or Wingdings).
            if !is_symbol_font(&piece.fmt.font) {
                self.descent = self.descent.max(piece.descent);
            }
            x = emit_piece(piece, x, space_extra, &mut self.items);
        }
    }

    /// The height and baseline of the line with line spacing `spacing`.
    /// Pictures taller than the text raise the ascent (and, on a line
    /// without text, clear the descent and leading).
    fn height_and_baseline(&mut self, spacing: LineSpacing) -> (f32, f32) {
        // Line spacing multiplies the height of the text, not that of
        // pictures: a line of pictures alone is as tall as they are.
        let text_height = self.ascent + self.descent + self.leading;
        let picture_height = if self.text_seen {
            self.picture_ascent + self.descent.max(self.picture_descent) + self.leading
        } else {
            self.picture_ascent
        };
        let auto_height = |multiple: f32| (text_height * multiple).max(picture_height);
        if self.picture_ascent > self.ascent {
            if self.text_seen {
                self.descent = self.descent.max(self.picture_descent);
            } else {
                self.descent = 0.0;
                self.leading = 0.0;
            }
            self.ascent = self.picture_ascent;
        }
        let (ascent, descent, leading) = (self.ascent, self.descent, self.leading);
        let natural = ascent + descent + leading;
        let height = match spacing {
            LineSpacing::Auto(multiple) => auto_height(multiple),
            LineSpacing::Exact(value) => value,
            LineSpacing::AtLeast(value) => natural.max(value),
        };
        // Word puts the text at the top of a line made taller by multiple
        // spacing (below its external leading), and its baseline at 80% of
        // an exact height.
        let baseline = match spacing {
            LineSpacing::Auto(_) => leading + ascent,
            LineSpacing::Exact(value) => value * EXACT_BASELINE_RATIO,
            LineSpacing::AtLeast(value) if value > natural => atleast_baseline(value, ascent, descent, leading),
            LineSpacing::AtLeast(_) => leading + ascent,
        };
        (height, baseline)
    }
}

/// Widens (or, when `free` is negative, narrows) the spaces between the
/// words after the last tab (from `last_tab`), not the trailing ones, so
/// that the line takes `free` more width.
fn justify_spaces(placed: &mut [Placed], last_tab: usize, free: f32) {
    let mut spaces = 0usize;
    let end_index = placed.len();
    let mut last_word = None;
    for (at, item) in placed.iter().enumerate().skip(last_tab) {
        if let Atom::Word { body, .. } = &item.atom
            && *body > 0.0
        {
            last_word = Some(at);
        }
    }
    for item in placed[last_tab..last_word.unwrap_or(end_index)].iter() {
        if let Atom::Word { pieces, .. } = &item.atom {
            spaces += pieces.iter().map(|piece| piece.spaces).sum::<usize>();
        }
    }
    if spaces > 0 {
        let extra = free / spaces as f32;
        let mut shift = 0.0;
        for (at, item) in placed.iter_mut().enumerate().skip(last_tab) {
            item.x += shift;
            if Some(at) >= last_word {
                continue;
            }
            if let Atom::Word { pieces, .. } = &item.atom {
                let count: usize = pieces.iter().map(|piece| piece.spaces).sum();
                item.space_extra = extra;
                shift += extra * count as f32;
            }
        }
    }
}

/// The width before the decimal point in a segment after a decimal tab.
fn decimal_offset(segment: &[&Placed]) -> Option<f32> {
    for placed in segment {
        if let Atom::Word { pieces, .. } = &placed.atom {
            let mut x = placed.x;
            let start = segment.first().map_or(placed.x, |first| first.x);
            for piece in pieces {
                for glyph in &piece.glyphs {
                    if glyph_text(piece, glyph).contains(['.', ',']) {
                        return Some(x + glyph.x - start);
                    }
                }
                x += piece.width;
            }
        }
    }
    None
}

/// The text a glyph of `piece` stands for (empty when its range is not in
/// the piece's text).
fn glyph_text<'a>(piece: &'a Piece, glyph: &Glyph) -> &'a str {
    piece.text.get(glyph.range()).unwrap_or("")
}

/// Moves an item placed on baseline 0 to `baseline`.
fn shift_item(item: &mut Item, baseline: f32) {
    match item {
        Item::Text(run) => run.y += baseline,
        Item::Rect { y, .. } | Item::Image { y, .. } | Item::Link { y, .. } | Item::Turn { y, .. } => *y += baseline,
        Item::EndTurn => {}
        Item::Line { y1, y2, .. } => {
            *y1 += baseline;
            *y2 += baseline;
        }
    }
}

/// The items of `text` set in `fmt`, the pen starting at `x` on baseline
/// `y` (text that is not part of a paragraph: the lines of a metafile),
/// and the width of the text.
pub(super) fn text_items(ctx: &mut Ctx<'_>, text: &str, fmt: &RunFmt, x: f32, y: f32) -> (Vec<Item>, f32) {
    let mut items = Vec::new();
    let mut pen = x;
    for piece in ctx.pieces(text, fmt) {
        pen = emit_piece(&piece, pen, 0.0, &mut items);
    }
    for item in &mut items {
        shift_item(item, y);
    }
    (items, pen - x)
}

/// Draws a piece at `x` on baseline 0 (with justification `extra` per
/// space), with its underline, strike, highlight and link; returns the
/// pen position after it.
fn emit_piece(piece: &Piece, x: f32, extra: f32, items: &mut Vec<Item>) -> f32 {
    let mut glyphs = Vec::with_capacity(piece.glyphs.len());
    let mut shift = 0.0;
    for glyph in &piece.glyphs {
        let is_space = glyph_text(piece, glyph).starts_with(' ');
        glyphs.push(Glyph {
            x: glyph.x + shift,
            advance: glyph.advance + if is_space { extra } else { 0.0 },
            ..*glyph
        });
        if is_space {
            shift += extra;
        }
    }
    let width = piece.width + shift;
    let fmt = &piece.fmt;
    let top = -piece.ascent;
    let height = piece.ascent + piece.descent;
    if let Some(color) = fmt.highlight.or(fmt.shading) {
        items.push(Item::Rect {
            x,
            y: top + piece.rise.max(0.0),
            width,
            height: height - piece.rise.abs(),
            color,
        });
    }
    let color = fmt.color.unwrap_or([0, 0, 0]);
    items.push(Item::Text(TextRun {
        face: piece.face,
        size: piece.size,
        x,
        y: -piece.rise,
        color,
        glyphs,
        text: piece.text.clone(),
        fake_bold: piece.fake_bold,
        fake_italic: piece.fake_italic,
        scale: piece.scale,
    }));
    if let Some(underline) = fmt.underline {
        emit_underline(piece, underline, x, width, color, items);
    }
    if fmt.strike || fmt.dstrike {
        emit_strike(piece, x, width, color, items);
    }
    if let Some(href) = &fmt.href {
        items.push(Item::Link {
            x,
            y: top,
            width,
            height,
            target: href.clone(),
        });
    }
    x + width
}

/// Draws the underline of style `underline` under a piece drawn at `x`,
/// `width` wide, in the underline colour or else in the text's `color`. A
/// words-only underline is not drawn under spaces.
fn emit_underline(piece: &Piece, underline: Underline, x: f32, width: f32, color: Rgb, items: &mut Vec<Item>) {
    let (under_pos, thickness, _) = piece.lines;
    let visible = underline != Underline::Words || !piece.text.trim().is_empty();
    if !visible {
        return;
    }
    let y = under_pos - piece.rise + thickness / 2.0;
    let line_color = piece.fmt.underline_color.unwrap_or(color);
    let line_width = if underline == Underline::Thick {
        thickness * 2.0
    } else {
        thickness
    };
    let dash = match underline {
        Underline::Dotted => Some(line_width),
        Underline::Dashed => Some(line_width * 4.0),
        _ => None,
    };
    items.push(Item::Line {
        x1: x,
        y1: y,
        x2: x + width,
        y2: y,
        width: line_width,
        color: line_color,
        dash,
    });
    if underline == Underline::Double {
        items.push(Item::Line {
            x1: x,
            y1: y + line_width * 2.0,
            x2: x + width,
            y2: y + line_width * 2.0,
            width: line_width,
            color: line_color,
            dash: None,
        });
    }
}

/// Draws the strikethrough (single or double) of a piece drawn at `x`,
/// `width` wide, in colour `color`.
fn emit_strike(piece: &Piece, x: f32, width: f32, color: Rgb, items: &mut Vec<Item>) {
    let (_, thickness, strike_pos) = piece.lines;
    let y = -strike_pos - piece.rise;
    items.push(Item::Line {
        x1: x,
        y1: y,
        x2: x + width,
        y2: y,
        width: thickness,
        color,
        dash: None,
    });
    if piece.fmt.dstrike {
        items.push(Item::Line {
            x1: x,
            y1: y - thickness * 2.0,
            x2: x + width,
            y2: y - thickness * 2.0,
            width: thickness,
            color,
            dash: None,
        });
    }
}

/// Draws a tab leader from `from` to `to` on baseline 0.
fn emit_leader(ctx: &mut Ctx<'_>, leader: char, fmt: &RunFmt, from: f32, to: f32, items: &mut Vec<Item>) {
    let mut plain = fmt.clone();
    plain.underline = None;
    plain.href = None;
    let pieces = ctx.pieces(&leader.to_string(), &plain);
    let Some(piece) = pieces.into_iter().next() else { return };
    if piece.width <= 0.0 {
        return;
    }
    // Leader characters fill the cells of a grid that starts at the left
    // edge of the page, as wide as the character rounded to Word's layout
    // resolution.
    let pitch = (piece.width * LAYOUT_DPI / PT_PER_INCH).round().max(1.0) * PT_PER_INCH / LAYOUT_DPI;
    let origin = ctx.origin;
    let mut x = ((from + origin) / pitch - FIT_TOLERANCE_PT).ceil() * pitch - origin;
    let mut glyphs = Vec::new();
    let mut text = String::new();
    let start = x;
    while x + pitch <= to + FIT_TOLERANCE_PT {
        for glyph in &piece.glyphs {
            glyphs.push(Glyph {
                x: x - start + glyph.x,
                text: (offset(text.len()), offset(text.len() + leader.len_utf8())),
                ..*glyph
            });
        }
        text.push(leader);
        x += pitch;
    }
    if glyphs.is_empty() {
        return;
    }
    items.push(Item::Text(TextRun {
        face: piece.face,
        size: piece.size,
        x: start,
        y: -piece.rise,
        color: fmt.color.unwrap_or([0, 0, 0]),
        glyphs,
        text,
        fake_bold: piece.fake_bold,
        fake_italic: piece.fake_italic,
        scale: piece.scale,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_lines_keep_brackets_and_punctuation_to_their_letters() {
        // Between two Japanese letters a line may break...
        assert!(breaks_after('予', Some('定')));
        // ...but not after an opening bracket, nor before a closing one or
        // a full stop.
        assert!(!breaks_after('(', Some('予')));
        assert!(!breaks_after('（', Some('予')));
        assert!(!breaks_after('日', Some('）')));
        assert!(!breaks_after('日', Some('。')));
        assert!(breaks_after('。', Some('次')));
    }
}
