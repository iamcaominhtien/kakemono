//! A paragraph broken into lines the way Word breaks them: words placed
//! one after the other until one does not fit, tabs to their stops, lines
//! justified by widening the spaces, line heights from the font metrics
//! (`usWinAscent`, `usWinDescent` and the external leading) and the line
//! spacing rule.

use super::display::{Glyph, Item, TextRun};
use super::fonts::{FaceId, FontBook};
use super::shape::Shaper;
use crate::word::fmt::model::{
    Align, Anchor, BreakKind, FDoc, FieldKind, Inline, LineSpacing, NumFormat, NumberSuffix, Para, RunFmt, Shape,
    TabKind, TabStop, Underline, VertAlign,
};
use std::collections::HashMap;

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
/// Lines fit when their text exceeds the width by less than this, in points
/// (rounding).
const FIT_TOLERANCE_PT: f32 = 0.01;

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
    fmt: RunFmt,
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
    /// A word: its text pieces, the width of its trailing spaces, and
    /// whether it ends with a soft hyphen.
    Word {
        pieces: Vec<Piece>,
        body: f32,
        trailing: f32,
        soft_hyphen: bool,
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
    fn face_for(&mut self, ch: char, fmt: &RunFmt) -> (FaceId, bool, bool, f32) {
        let (family, size) = if is_east_asian(ch) {
            (&fmt.font_east_asia, fmt.size)
        } else if fmt.cs || fmt.rtl || is_complex(ch) {
            (&fmt.font_cs, fmt.size_cs)
        } else if ch.is_ascii() {
            (&fmt.font, fmt.size)
        } else {
            (&fmt.font_hansi, fmt.size)
        };
        let (face, fake_bold, fake_italic) = self.book.face(family, fmt.bold, fmt.italic);
        if ch.is_whitespace() || ch.is_control() || self.covered(face, ch) {
            return (face, fake_bold, fake_italic, size);
        }
        for fallback in self.book.fallbacks() {
            if self.covered(fallback, ch) {
                return (fallback, fmt.bold, fmt.italic, size);
            }
        }
        (face, fake_bold, fake_italic, size)
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
        let text = transform_case(text, fmt);
        let mut pieces = Vec::new();
        let mut start = 0;
        let mut current: Option<(FaceId, bool, bool, f32, bool)> = None;
        for (at, ch) in text.char_indices() {
            let small = fmt.small_caps && !fmt.caps && ch.is_lowercase();
            let (face, fake_bold, fake_italic, size) = self.face_for(ch, fmt);
            let key = (face, fake_bold, fake_italic, size, small);
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
    fn piece(&mut self, text: &str, fmt: &RunFmt, key: (FaceId, bool, bool, f32, bool)) -> Piece {
        let (face, fake_bold, fake_italic, base_size, small) = key;
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
        let mut glyphs = Vec::with_capacity(shaped.glyphs.len());
        let mut x = 0.0;
        let mut spaces = 0;
        let clusters: Vec<usize> = shaped.glyphs.iter().map(|glyph| glyph.cluster).collect();
        for (at, glyph) in shaped.glyphs.iter().enumerate() {
            let advance = glyph.advance * size * scale + fmt.spacing;
            let start = glyph.cluster;
            let end = clusters
                .iter()
                .copied()
                .filter(|&cluster| cluster > start)
                .min()
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
                text: (start, end),
            });
            let _ = at;
            x += advance;
        }
        let metrics = self.book.get(face).metrics;
        Piece {
            face,
            size,
            fake_bold,
            fake_italic,
            scale,
            fmt: fmt.clone(),
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
    fn line_metrics(&mut self, fmt: &RunFmt) -> (f32, f32, f32) {
        let ch = 'x';
        let (face, _, _, size) = self.face_for(ch, fmt);
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
        _ => is_east_asian(ch) || next.is_some_and(is_east_asian),
    }
}

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
    // The number or bullet starts the first line.
    let first_indent = props.left + props.first_line;
    builder.x = first_indent;
    if let Some(number) = &para.number {
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
                soft_hyphen: false,
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
    let line_width = |first: bool| {
        let left = if first {
            props.left + props.first_line
        } else {
            props.left
        };
        (left, width - props.right)
    };
    let mut first_line = true;
    for atom in atoms {
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
                builder.x = line_width(false).0;
            }
            Atom::Tab { ref fmt } => {
                let fmt = fmt.clone();
                let (_, right) = line_width(first_line);
                let stop = next_stop(
                    builder.x,
                    &props.tabs,
                    props.left,
                    if first_line { props.first_line } else { 0.0 },
                    ctx.doc.default_tab,
                    first_line,
                );
                let stop = stop.filter(|stop| stop.pos <= right + TAB_OVERFLOW_PT || stop.kind != TabKind::Left);
                match stop {
                    Some(stop) => {
                        builder.close_tab_segment();
                        let index = builder.tab_stops.iter().position(|s| *s == stop).unwrap_or_else(|| {
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
                        builder.finish_line(ctx, para, first_line, LineEnd::Wrap);
                        first_line = false;
                        builder.x = line_width(false).0;
                        let stop = next_stop(builder.x, &props.tabs, props.left, 0.0, ctx.doc.default_tab, false);
                        if let Some(stop) = stop {
                            builder.x = stop.pos;
                        }
                    }
                }
            }
            atom => {
                let (body, trailing) = atom_width(&atom);
                let (left, right) = line_width(first_line);
                // Word squeezes the spaces of a justified line to fit one
                // more word.
                let shrink = if matches!(props.align, Align::Both | Align::Distribute) && ctx.doc.compat_mode >= 15 {
                    builder.space_width() * justify_shrink()
                } else {
                    0.0
                };
                let fits = builder.x + body - shrink <= right + FIT_TOLERANCE_PT;
                if !fits && builder.has_content() {
                    builder.finish_line(ctx, para, first_line, LineEnd::Wrap);
                    first_line = false;
                    builder.x = line_width(false).0;
                }
                let (_, right) = line_width(first_line);
                if builder.x + body > right + FIT_TOLERANCE_PT && !builder.has_content() {
                    // A word longer than the line: cut it.
                    let mut rest = Some(atom);
                    while let Some(atom) = rest.take() {
                        let room = right - builder.x;
                        let (head, tail) = split_word(atom, room);
                        let (head_body, head_trailing) = atom_width(&head);
                        builder.push(head, head_body + head_trailing);
                        if let Some(tail) = tail {
                            builder.finish_line(ctx, para, first_line, LineEnd::Wrap);
                            first_line = false;
                            builder.x = line_width(false).0;
                            rest = Some(tail);
                        }
                    }
                    continue;
                }
                let _ = left;
                builder.push(atom, body + trailing);
            }
        }
    }
    // Since Word 2013, the mark of a paragraph that ends with a page or
    // column break stays on the break's line.
    let ends_with_break = builder
        .lines
        .last()
        .is_some_and(|line| line.page_break || line.column_break);
    if !(ends_with_break && !builder.has_content() && ctx.doc.compat_mode >= 15) {
        builder.finish_line(ctx, para, first_line, LineEnd::Last);
    }
    ParaLayout {
        lines: builder.lines,
        floats: builder.floats,
    }
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
            soft_hyphen: false,
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
    if keep >= piece.glyphs.len() {
        return (Some(piece), None);
    }
    let split_byte = piece.glyphs[keep].text.0;
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
            text: (glyph.text.0 - split_byte, glyph.text.1 - split_byte),
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
                let value = match kind {
                    FieldKind::Page => ctx.page.clone().map(|(number, section_format)| {
                        crate::word::fmt::model::format_page(number, format.as_ref().unwrap_or(&section_format))
                    }),
                    FieldKind::NumPages => ctx.page_count.map(|count| count.to_string()),
                    FieldKind::SectionPages => ctx.section_pages.map(|count| count.to_string()),
                };
                stretch.push((value.unwrap_or_else(|| "1".into()), fmt.clone()));
                continue;
            }
            _ => {}
        }
        flush_stretch(ctx, &mut stretch, &mut atoms);
        match inline {
            Inline::Tab { fmt } => atoms.push(Atom::Tab { fmt: fmt.clone() }),
            Inline::Break { kind, .. } => atoms.push(Atom::Break(*kind)),
            Inline::Image {
                index,
                width,
                height,
                anchor,
                fmt,
                crop,
                ..
            } => match anchor {
                Some(anchor) => atoms.push(Atom::Float(Float {
                    line: 0,
                    x: 0.0,
                    kind: FloatKind::Image(*index, *crop),
                    width: *width,
                    height: *height,
                    anchor: anchor.clone(),
                })),
                None => atoms.push(Atom::Image {
                    index: *index,
                    width: *width,
                    height: *height,
                    crop: *crop,
                    fmt: fmt.clone(),
                }),
            },
            Inline::Shape(shape) => match &shape.anchor {
                Some(anchor) => atoms.push(Atom::Float(Float {
                    line: 0,
                    x: 0.0,
                    kind: FloatKind::Shape(shape.clone()),
                    width: shape.width,
                    height: shape.height,
                    anchor: anchor.clone(),
                })),
                None => atoms.push(Atom::Shape(shape.clone())),
            },
            Inline::NoteRef { endnote, id, fmt } => {
                let number = ctx.notes.get(&(*endnote, id.clone())).cloned().unwrap_or_default();
                let pieces = ctx.pieces(&number, fmt);
                let width: f32 = pieces.iter().map(|piece| piece.width).sum();
                atoms.push(Atom::NoteRef {
                    word: Box::new(Atom::Word {
                        pieces,
                        body: width,
                        trailing: 0.0,
                        soft_hyphen: false,
                    }),
                    endnote: *endnote,
                    id: id.clone(),
                });
            }
            Inline::NoteNumber { fmt } => {
                let number = ctx.own_note.clone().unwrap_or_default();
                let pieces = ctx.pieces(&number, fmt);
                let width: f32 = pieces.iter().map(|piece| piece.width).sum();
                atoms.push(Atom::Word {
                    pieces,
                    body: width,
                    trailing: 0.0,
                    soft_hyphen: false,
                });
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

/// Breaks the collected text runs into word atoms.
fn flush_stretch(ctx: &mut Ctx<'_>, stretch: &mut Vec<(String, RunFmt)>, atoms: &mut Vec<Atom>) {
    if stretch.is_empty() {
        return;
    }
    let runs = std::mem::take(stretch);
    let mut text = String::new();
    let mut bounds = Vec::new();
    for (piece, _) in &runs {
        bounds.push(text.len());
        text.push_str(piece);
    }
    bounds.push(text.len());
    for (start, body_end, end) in words(&text) {
        let mut pieces = Vec::new();
        let mut body = 0.0;
        let mut trailing = 0.0;
        for (at, (_, fmt)) in runs.iter().enumerate() {
            let (run_start, run_end) = (bounds[at], bounds[at + 1]);
            for (part_start, part_end, is_body) in [(start, body_end, true), (body_end, end, false)] {
                let from = part_start.max(run_start);
                let to = part_end.min(run_end);
                if from >= to {
                    continue;
                }
                let part = &text[from..to];
                let part = part.replace('\u{00AD}', "");
                if part.is_empty() {
                    continue;
                }
                for piece in ctx.pieces(&part, fmt) {
                    if is_body {
                        body += piece.width;
                    } else {
                        trailing += piece.width;
                    }
                    pieces.push(piece);
                }
            }
        }
        let soft_hyphen = text[start..body_end].ends_with('\u{00AD}');
        atoms.push(Atom::Word {
            pieces,
            body,
            trailing,
            soft_hyphen,
        });
    }
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
    std::env::var("KAKEMONO_JUSTIFY_SHRINK")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(JUSTIFY_SHRINK_RATIO)
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
            // Spaces between words after the last tab, not the trailing ones.
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
        let line = self.make_line(ctx, para, placed, end);
        self.lines.push(line);
    }

    /// Turns placed atoms into a line with its height and items.
    fn make_line(&mut self, ctx: &mut Ctx<'_>, para: &Para, placed: Vec<Placed>, end: LineEnd) -> Line {
        let mut ascent: f32 = 0.0;
        let mut descent: f32 = 0.0;
        let mut leading: f32 = 0.0;
        let mut items = Vec::new();
        let mut notes = Vec::new();
        let mut shapes = Vec::new();
        let mut text_seen = false;
        let mut images = Vec::new();
        for item in &placed {
            match &item.atom {
                Atom::Word { pieces, .. } => {
                    text_seen = true;
                    let mut x = item.x;
                    for piece in pieces {
                        ascent = ascent.max(piece.ascent);
                        descent = descent.max(piece.descent);
                        leading = leading.max(piece.leading);
                        x = emit_piece(piece, x, item.space_extra, &mut items);
                    }
                }
                Atom::NoteRef { word, endnote, id } => {
                    notes.push((*endnote, id.clone()));
                    if let Atom::Word { pieces, .. } = word.as_ref() {
                        let mut x = item.x;
                        for piece in pieces {
                            ascent = ascent.max(piece.ascent);
                            descent = descent.max(piece.descent);
                            leading = leading.max(piece.leading);
                            x = emit_piece(piece, x, 0.0, &mut items);
                        }
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
                    ascent = ascent.max(*height);
                    descent = descent.max(fmt_descent);
                    images.push((item.x, *index, *width, *height, *crop));
                }
                Atom::Shape(shape) => {
                    ascent = ascent.max(shape.height);
                    shapes.push((item.x, shape.as_ref().clone()));
                }
                Atom::Tab { fmt } => {
                    let (a, d, l) = ctx.line_metrics(fmt);
                    ascent = ascent.max(a);
                    descent = descent.max(d);
                    leading = leading.max(l);
                    if let Some(stop) = item.tab.map(|index| self.tab_stops[index])
                        && let Some(leader) = stop.leader
                    {
                        let next = placed
                            .iter()
                            .skip_while(|other| !std::ptr::eq(*other, item))
                            .nth(1)
                            .map_or(stop.pos, |next| next.x);
                        emit_leader(ctx, leader, fmt, item.x, next, &mut items);
                    }
                }
                _ => {}
            }
        }
        // The paragraph mark's font counts on the last line, and alone on an
        // empty one.
        let is_last_line = end == LineEnd::Last;
        if is_last_line || !text_seen && images.is_empty() {
            let (a, d, l) = ctx.line_metrics(&para.mark);
            if !text_seen && images.is_empty() && shapes.is_empty() {
                ascent = ascent.max(a);
                descent = descent.max(d);
                leading = leading.max(l);
            } else if is_last_line {
                ascent = ascent.max(a);
                descent = descent.max(d);
                leading = leading.max(l);
            }
        }
        let natural = ascent + descent + leading;
        let height = match para.props.line {
            LineSpacing::Auto(multiple) => natural * multiple,
            LineSpacing::Exact(value) => value,
            LineSpacing::AtLeast(value) => natural.max(value),
        };
        // Word puts the text at the top of a line made taller by multiple
        // spacing (below its external leading), and its baseline at 80% of
        // an exact height.
        let baseline = match para.props.line {
            LineSpacing::Auto(_) => leading + ascent,
            LineSpacing::Exact(value) => value * EXACT_BASELINE_RATIO,
            LineSpacing::AtLeast(value) if value > natural => atleast_baseline(value, ascent, descent, leading),
            LineSpacing::AtLeast(_) => leading + ascent,
        };
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
                    w: width,
                    h: height,
                    crop,
                });
            }
        }
        let shapes = shapes.into_iter().map(|(x, shape)| (x, shape)).collect::<Vec<_>>();
        let only_pictures = !text_seen && (has_images || !shapes.is_empty());
        let fit_height = match para.props.line {
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
            notes,
            bookmarks: std::mem::take(&mut self.pending_bookmarks),
            shapes,
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
                    if piece.text[glyph.text.0..glyph.text.1].contains(['.', ',']) {
                        return Some(x + glyph.x - start);
                    }
                }
                x += piece.width;
            }
        }
    }
    None
}

/// Moves an item placed on baseline 0 to `baseline`.
fn shift_item(item: &mut Item, baseline: f32) {
    match item {
        Item::Text(run) => run.y += baseline,
        Item::Rect { y, .. } | Item::Image { y, .. } | Item::Link { y, .. } => *y += baseline,
        Item::Line { y1, y2, .. } => {
            *y1 += baseline;
            *y2 += baseline;
        }
    }
}

/// Draws a piece at `x` on baseline 0 (with justification `extra` per
/// space), with its underline, strike, highlight and link; returns the
/// pen position after it.
fn emit_piece(piece: &Piece, x: f32, extra: f32, items: &mut Vec<Item>) -> f32 {
    let mut glyphs = Vec::with_capacity(piece.glyphs.len());
    let mut shift = 0.0;
    for glyph in &piece.glyphs {
        let is_space = piece.text[glyph.text.0..glyph.text.1].starts_with(' ');
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
            w: width,
            h: height - piece.rise.abs(),
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
    let (under_pos, thickness, strike_pos) = piece.lines;
    if let Some(underline) = fmt.underline {
        let visible = underline != Underline::Words || !piece.text.trim().is_empty();
        if visible {
            let y = under_pos - piece.rise + thickness / 2.0;
            let line_color = fmt.underline_color.unwrap_or(color);
            let w = if underline == Underline::Thick {
                thickness * 2.0
            } else {
                thickness
            };
            let dash = match underline {
                Underline::Dotted => Some(w),
                Underline::Dashed => Some(w * 4.0),
                _ => None,
            };
            items.push(Item::Line {
                x1: x,
                y1: y,
                x2: x + width,
                y2: y,
                width: w,
                color: line_color,
                dash,
            });
            if underline == Underline::Double {
                items.push(Item::Line {
                    x1: x,
                    y1: y + w * 2.0,
                    x2: x + width,
                    y2: y + w * 2.0,
                    width: w,
                    color: line_color,
                    dash: None,
                });
            }
        }
    }
    if fmt.strike || fmt.dstrike {
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
        if fmt.dstrike {
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
    if let Some(href) = &fmt.href {
        items.push(Item::Link {
            x,
            y: top,
            w: width,
            h: height,
            target: href.clone(),
        });
    }
    x + width
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
    // Leader characters sit on a grid of their width, as in Word.
    let mut x = (from / piece.width).ceil() * piece.width;
    let mut glyphs = Vec::new();
    let mut text = String::new();
    let start = x;
    while x + piece.width <= to - piece.width * 0.5 {
        for glyph in &piece.glyphs {
            glyphs.push(Glyph {
                x: x - start + glyph.x,
                text: (text.len(), text.len() + leader.len_utf8()),
                ..*glyph
            });
        }
        text.push(leader);
        x += piece.width;
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
