//! MathType and Equation Editor formulas (MTEF, versions 3 and 5), stored
//! in the `Equation Native` stream of an embedded OLE object, read into
//! the structure of a formula ([`MathNode`]). Word shows such a formula as
//! a picture (its preview); this reading gives the text outputs the formula
//! itself.

use crate::model::MathNode;

/// Size of the header of the `Equation Native` stream (`EQNOLEFILEHDR`).
const OLE_HEADER_LEN: usize = 28;
/// Record types.
const END: u8 = 0;
/// A line: a list of objects.
const LINE: u8 = 1;
/// A character.
const CHAR: u8 = 2;
/// A template (fraction, root, fence, ...) with its slots.
const TMPL: u8 = 3;
/// Lines stacked vertically.
const PILE: u8 = 4;
/// A matrix.
const MATRIX: u8 = 5;
/// An embellishment of a character (dot, prime, hat, ...).
const EMBELL: u8 = 6;
/// Tab stops.
const RULER: u8 = 7;
/// Version 3: a font; version 5: the style of a typeface.
const FONT_OR_STYLE_DEF: u8 = 8;
/// A size.
const SIZE: u8 = 9;
/// The sizes that need no data: full, sub, sub2, sym, subsym.
const SIZE_FULL: u8 = 10;
/// Last of the sizes that need no data.
const SIZE_SUBSYM: u8 = 14;
/// A colour.
const COLOR: u8 = 15;
/// A colour definition.
const COLOR_DEF: u8 = 16;
/// A font definition.
const FONT_DEF: u8 = 17;
/// Equation preferences.
const EQN_PREFS: u8 = 18;
/// An encoding definition.
const ENCODING_DEF: u8 = 19;
/// Records from this type on carry their length.
const FUTURE: u8 = 100;
/// Option: a nudge follows.
const OPT_NUDGE: u8 = 0x08;
/// Option of a character: embellishments follow.
const OPT_CHAR_EMBELL: u8 = 0x01;
/// Option of a character (version 5): an 8-bit font position follows.
const OPT_CHAR_ENC_CHAR_8: u8 = 0x04;
/// Option of a character (version 5): a 16-bit font position follows.
const OPT_CHAR_ENC_CHAR_16: u8 = 0x10;
/// Option of a character (version 5): there is no character code.
const OPT_CHAR_ENC_NO_MTCODE: u8 = 0x20;
/// Option of a line: it is empty.
const OPT_LINE_NULL: u8 = 0x01;
/// Option of a line or pile: a ruler follows.
const OPT_LP_RULER: u8 = 0x02;
/// Option of a line: its spacing follows.
const OPT_LINE_LSPACE: u8 = 0x04;
/// Typeface of running text.
const FACE_TEXT: u8 = 1;
/// Typeface of function names (upright).
const FACE_FUNCTION: u8 = 2;
/// Typeface of numbers.
const FACE_NUMBER: u8 = 8;
/// Typeface of vectors and matrices (bold).
const FACE_VECTOR: u8 = 7;
/// Size of an em space, for the spacing characters of the MathType fonts.
const THIN_SPACE_EM: f32 = 0.17;

/// The formula of an `Equation Native` stream; `None` when it cannot be
/// read.
pub(crate) fn equation(native: &[u8]) -> Option<MathNode> {
    let header_len = usize::from(u16::from_le_bytes(native.get(0..2)?.try_into().ok()?));
    let start = if header_len == OLE_HEADER_LEN { header_len } else { 0 };
    let mut reader = Reader {
        bytes: native.get(start..)?,
        pos: 0,
        version: 0,
    };
    reader.header()?;
    let nodes = reader.objects()?;
    let node = MathNode::row(nodes);
    (!is_empty(&node)).then_some(node)
}

/// Whether a formula shows nothing.
fn is_empty(node: &MathNode) -> bool {
    match node {
        MathNode::Row(nodes) => nodes.iter().all(is_empty),
        MathNode::Space(_) => true,
        _ => false,
    }
}

/// One object of a line, before characters are grouped.
enum Object {
    /// A character, with its typeface and embellishments.
    Char {
        /// The character.
        ch: char,
        /// Its typeface.
        face: u8,
        /// Its embellishments.
        embells: Vec<u8>,
    },
    /// A formula part (template, pile, matrix).
    Node(MathNode),
    /// A template that attaches scripts to what comes before it.
    Scripts {
        /// The lowered script.
        sub: Option<MathNode>,
        /// The raised script.
        sup: Option<MathNode>,
    },
}

/// Reads MTEF records.
struct Reader<'a> {
    /// The MTEF data.
    bytes: &'a [u8],
    /// Where the next record starts.
    pos: usize,
    /// The MTEF version (3 or 5).
    version: u8,
}

impl Reader<'_> {
    /// The next byte.
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }

    /// The next little-endian 16-bit value.
    fn word(&mut self) -> Option<u16> {
        Some(u16::from(self.byte()?) | (u16::from(self.byte()?) << 8))
    }

    /// Skips a null-terminated string.
    fn skip_string(&mut self) -> Option<()> {
        while self.byte()? != 0 {}
        Some(())
    }

    /// Reads the MTEF header.
    fn header(&mut self) -> Option<()> {
        self.version = self.byte()?;
        if !matches!(self.version, 3 | 5) {
            return None;
        }
        // Platform, product, version, subversion.
        for _ in 0..4 {
            self.byte()?;
        }
        if self.version == 5 {
            self.skip_string()?;
            // Equation options.
            self.byte()?;
        }
        Some(())
    }

    /// The type and options of the next record (`None` at the end of the
    /// data).
    fn tag(&mut self) -> Option<(u8, u8)> {
        let tag = self.byte()?;
        if self.version == 3 {
            return Some((tag & 0x0F, tag >> 4));
        }
        // Records that have options in version 5.
        let options = if (LINE..=RULER).contains(&tag) && tag != RULER {
            self.byte()?
        } else {
            0
        };
        Some((tag, options))
    }

    /// Skips a nudge when `options` announce one.
    fn nudge(&mut self, options: u8) -> Option<()> {
        if options & OPT_NUDGE != 0 {
            let (dx, dy) = (self.byte()?, self.byte()?);
            if dx == 128 && dy == 128 {
                self.word()?;
                self.word()?;
            }
        }
        Some(())
    }

    /// The objects of a list, up to its END record, as formula nodes.
    fn objects(&mut self) -> Option<Vec<MathNode>> {
        let mut objects = Vec::new();
        while let Some((kind, options)) = self.tag() {
            match kind {
                END => break,
                LINE => {
                    if let Some(line) = self.line(options)? {
                        objects.push(Object::Node(line));
                    }
                }
                CHAR => {
                    if let Some(object) = self.character(options)? {
                        objects.push(object);
                    }
                }
                TMPL => objects.push(self.template(options)?),
                PILE => objects.push(Object::Node(self.pile(options)?)),
                MATRIX => objects.push(Object::Node(self.matrix(options)?)),
                _ => self.skip_record(kind, options)?,
            }
        }
        Some(group(objects))
    }

    /// A line: `None` for an empty one.
    fn line(&mut self, options: u8) -> Option<Option<MathNode>> {
        self.nudge(options)?;
        if self.version == 5 && options & OPT_LINE_LSPACE != 0 {
            self.word()?;
        }
        if self.version == 3 && options & OPT_LINE_LSPACE != 0 {
            self.word()?;
        }
        if options & OPT_LP_RULER != 0 {
            self.ruler_record()?;
        }
        if options & OPT_LINE_NULL != 0 {
            return Some(None);
        }
        Some(Some(MathNode::row(self.objects()?)))
    }

    /// Skips a ruler record (its tag included).
    fn ruler_record(&mut self) -> Option<()> {
        self.tag()?;
        self.ruler()
    }

    /// Skips the body of a ruler: its tab stops.
    fn ruler(&mut self) -> Option<()> {
        let stops = self.byte()?;
        for _ in 0..stops {
            self.byte()?;
            self.word()?;
        }
        Some(())
    }

    /// A character, with its embellishments.
    fn character(&mut self, options: u8) -> Option<Option<Object>> {
        self.nudge(options)?;
        let face = self.byte()?.wrapping_sub(128);
        let code = if self.version == 3 {
            Some(self.word()?)
        } else {
            let code = if options & OPT_CHAR_ENC_NO_MTCODE == 0 {
                Some(self.word()?)
            } else {
                None
            };
            if options & OPT_CHAR_ENC_CHAR_8 != 0 {
                self.byte()?;
            }
            if options & OPT_CHAR_ENC_CHAR_16 != 0 {
                self.word()?;
            }
            code
        };
        let has_embells = if self.version == 3 {
            options & 0x02 != 0
        } else {
            options & OPT_CHAR_EMBELL != 0
        };
        let mut embells = Vec::new();
        if has_embells {
            while let Some((kind, embell_options)) = self.tag() {
                if kind == END {
                    break;
                }
                if kind != EMBELL {
                    self.skip_record(kind, embell_options)?;
                    continue;
                }
                self.nudge(embell_options)?;
                embells.push(self.byte()?);
            }
        }
        Some(code.and_then(char_of).map(|ch| Object::Char { ch, face, embells }))
    }

    /// A template: its selector, variation and slots.
    fn template(&mut self, options: u8) -> Option<Object> {
        self.nudge(options)?;
        let selector = self.byte()?;
        let mut variation = u16::from(self.byte()?);
        if self.version == 5 && variation & 0x80 != 0 {
            variation = (variation & 0x7F) | (u16::from(self.byte()?) << 8);
        }
        // Template-specific options.
        self.byte()?;
        // The slots (lines and piles) and the template's characters.
        let mut slots: Vec<Option<MathNode>> = Vec::new();
        let mut chars = Vec::new();
        while let Some((kind, slot_options)) = self.tag() {
            match kind {
                END => break,
                LINE => slots.push(self.line(slot_options)?),
                PILE => slots.push(Some(self.pile(slot_options)?)),
                CHAR => {
                    if let Some(Object::Char { ch, .. }) = self.character(slot_options)? {
                        chars.push(ch);
                    }
                }
                TMPL => {
                    let inner = self.template(slot_options)?;
                    slots.push(Some(
                        group(vec![inner])
                            .into_iter()
                            .next()
                            .unwrap_or(MathNode::Row(Vec::new())),
                    ));
                }
                MATRIX => slots.push(Some(self.matrix(slot_options)?)),
                _ => self.skip_record(kind, slot_options)?,
            }
        }
        if self.version == 3 {
            let (selector, brackets) = v3_selector(selector);
            if chars.is_empty()
                && let Some((left, right)) = brackets
            {
                chars.extend([left, right]);
            }
            return Some(build_template(selector, variation, slots, &chars));
        }
        Some(build_template(selector, variation, slots, &chars))
    }

    /// Lines stacked vertically.
    fn pile(&mut self, options: u8) -> Option<MathNode> {
        self.nudge(options)?;
        let halign = self.byte()?;
        // Vertical alignment.
        self.byte()?;
        if options & OPT_LP_RULER != 0 {
            self.ruler_record()?;
        }
        let mut lines = Vec::new();
        while let Some((kind, line_options)) = self.tag() {
            match kind {
                END => break,
                LINE => lines.push(self.line(line_options)?.unwrap_or(MathNode::Row(Vec::new()))),
                _ => self.skip_record(kind, line_options)?,
            }
        }
        if lines.len() == 1 {
            return lines.pop();
        }
        let align = match halign {
            1 => "left",
            3 => "right",
            _ => "center",
        };
        Some(MathNode::Table {
            rows: lines.into_iter().map(|line| vec![line]).collect(),
            align: vec![align],
        })
    }

    /// A matrix.
    fn matrix(&mut self, options: u8) -> Option<MathNode> {
        self.nudge(options)?;
        // Vertical alignment, horizontal and vertical justification.
        for _ in 0..3 {
            self.byte()?;
        }
        let rows = usize::from(self.byte()?);
        let cols = usize::from(self.byte()?);
        // Partition lines: two bits per row and column boundary.
        for count in [rows, cols] {
            for _ in 0..(2 * (count + 1)).div_ceil(8) {
                self.byte()?;
            }
        }
        let mut cells = Vec::new();
        while let Some((kind, cell_options)) = self.tag() {
            match kind {
                END => break,
                LINE => cells.push(self.line(cell_options)?.unwrap_or(MathNode::Row(Vec::new()))),
                _ => self.skip_record(kind, cell_options)?,
            }
        }
        let cols = cols.max(1);
        Some(MathNode::Table {
            rows: cells.chunks(cols).map(<[MathNode]>::to_vec).collect(),
            align: vec!["center"; cols],
        })
    }

    /// Skips a record that holds no part of the formula.
    fn skip_record(&mut self, kind: u8, options: u8) -> Option<()> {
        match kind {
            EMBELL => {
                self.nudge(options)?;
                self.byte()?;
            }
            RULER => self.ruler()?,
            FONT_OR_STYLE_DEF if self.version == 3 => {
                self.byte()?;
                self.byte()?;
                self.skip_string()?;
            }
            FONT_OR_STYLE_DEF => {
                let font = self.byte()?;
                if font != 0 {
                    self.byte()?;
                }
            }
            SIZE => {
                let first = self.byte()?;
                match first {
                    101 => {
                        self.word()?;
                    }
                    100 => {
                        self.byte()?;
                        self.word()?;
                    }
                    _ => {
                        self.byte()?;
                    }
                }
            }
            SIZE_FULL..=SIZE_SUBSYM => {}
            COLOR => {
                self.byte()?;
            }
            COLOR_DEF => {
                let color_options = self.byte()?;
                let components = if color_options & 0x01 != 0 { 4 } else { 3 };
                for _ in 0..components {
                    self.word()?;
                }
                if color_options & 0x04 != 0 {
                    self.skip_string()?;
                }
            }
            FONT_DEF => {
                self.byte()?;
                self.skip_string()?;
            }
            EQN_PREFS => self.preferences()?,
            ENCODING_DEF => self.skip_string()?,
            kind if kind >= FUTURE => {
                let len = usize::from(self.word()?);
                self.pos += len;
            }
            _ => return None,
        }
        Some(())
    }

    /// Skips the equation preferences: sizes, spaces and styles.
    fn preferences(&mut self) -> Option<()> {
        // Options.
        self.byte()?;
        for _ in 0..2 {
            let count = self.byte()?;
            self.dimensions(count)?;
        }
        let styles = self.byte()?;
        for _ in 0..styles {
            if self.byte()? != 0 {
                self.byte()?;
            }
        }
        Some(())
    }

    /// Skips `count` dimensions written as nibbles (a unit, then digits,
    /// each value ended by the nibble 0xF).
    fn dimensions(&mut self, count: u8) -> Option<()> {
        let mut left = count;
        let mut high = true;
        let mut byte = 0;
        // Each value starts with its unit nibble, which is never 0xF.
        let mut in_value = false;
        while left > 0 {
            if high {
                byte = self.byte()?;
            }
            let nibble = if high { byte >> 4 } else { byte & 0x0F };
            high = !high;
            if !in_value {
                in_value = true;
            } else if nibble == 0x0F {
                in_value = false;
                left -= 1;
            }
        }
        Some(())
    }
}

/// The character of an MTEF character code (Unicode, with the private
/// codes of the MathType fonts mapped where they have a Unicode twin).
fn char_of(code: u16) -> Option<char> {
    let mapped = match code {
        // Spaces of the MathType fonts.
        0xEF01..=0xEF08 | 0xEB00..=0xEB0F => 0x2009,
        0xF8E7 => 0x2212,
        0xEC00 => 0x2192,
        0xEC01 => 0x2190,
        other => other,
    };
    char::from_u32(u32::from(mapped)).filter(|ch| !ch.is_control())
}

/// Makes formula nodes of a line's objects: letters of a function name,
/// digits of a number and runs of text are grouped, embellishments and
/// scripts attached to what they follow.
fn group(objects: Vec<Object>) -> Vec<MathNode> {
    let mut out: Vec<MathNode> = Vec::new();
    // A run of characters being grouped: (kind, text).
    let mut run: Option<(u8, String)> = None;
    let finish = |run: &mut Option<(u8, String)>, out: &mut Vec<MathNode>| {
        if let Some((kind, text)) = run.take() {
            out.push(match kind {
                FACE_NUMBER => MathNode::Number(text),
                FACE_FUNCTION => MathNode::Ident { text, upright: true },
                _ => MathNode::Text(text),
            });
        }
    };
    for object in objects {
        match object {
            Object::Char { ch, face, embells } => {
                let groupable = match face {
                    FACE_NUMBER => ch.is_ascii_digit() || ch == '.',
                    FACE_FUNCTION => ch.is_alphabetic(),
                    FACE_TEXT => !ch.is_whitespace() || run.is_some(),
                    _ => false,
                };
                if groupable && embells.is_empty() {
                    match &mut run {
                        Some((kind, text)) if *kind == face => text.push(ch),
                        _ => {
                            finish(&mut run, &mut out);
                            run = Some((face, ch.to_string()));
                        }
                    }
                    continue;
                }
                finish(&mut run, &mut out);
                let node = char_node(ch, face);
                out.push(embellish(node, &embells));
            }
            Object::Node(node) => {
                finish(&mut run, &mut out);
                out.push(node);
            }
            Object::Scripts { sub, sup } => {
                finish(&mut run, &mut out);
                let base = out.pop().unwrap_or(MathNode::Row(Vec::new()));
                out.push(MathNode::Scripts {
                    base: Box::new(base),
                    sub: sub.map(Box::new),
                    sup: sup.map(Box::new),
                });
            }
        }
    }
    finish(&mut run, &mut out);
    out
}

/// The node of a single character of typeface `face`.
fn char_node(ch: char, face: u8) -> MathNode {
    if ch == '\u{2009}' || ch == ' ' {
        return MathNode::Space(THIN_SPACE_EM);
    }
    if ch.is_ascii_digit() {
        return MathNode::Number(ch.to_string());
    }
    if ch.is_alphabetic() {
        let text = if face == FACE_VECTOR {
            bold(ch).to_string()
        } else {
            ch.to_string()
        };
        return MathNode::Ident {
            text,
            upright: face == FACE_FUNCTION || face == FACE_TEXT,
        };
    }
    MathNode::Operator {
        text: ch.to_string(),
        height_em: None,
    }
}

/// The bold mathematical letter of `ch`, for vectors.
fn bold(ch: char) -> char {
    let offset = match ch {
        'A'..='Z' => 0x1D400 + (u32::from(ch) - u32::from('A')),
        'a'..='z' => 0x1D41A + (u32::from(ch) - u32::from('a')),
        _ => return ch,
    };
    char::from_u32(offset).unwrap_or(ch)
}

/// `node` with the embellishments `embells` (dots, primes, accents).
fn embellish(node: MathNode, embells: &[u8]) -> MathNode {
    let mut node = node;
    for &embell in embells {
        let (mark, above) = match embell {
            2 => ("\u{2D9}", true),
            3 => ("\u{A8}", true),
            4 => ("\u{20DB}", true),
            5 => ("\u{2032}", false),
            6 => ("\u{2033}", false),
            7 => ("\u{2035}", false),
            8 => ("~", true),
            9 => ("^", true),
            11 => ("\u{2192}", true),
            12 => ("\u{2190}", true),
            13 => ("\u{2194}", true),
            16 | 17 => ("\u{AF}", true),
            18 => ("\u{2034}", false),
            _ => continue,
        };
        let mark = Box::new(MathNode::Operator {
            text: mark.to_string(),
            height_em: None,
        });
        node = if above {
            MathNode::UnderOver {
                base: Box::new(node),
                under: None,
                over: Some(mark),
                accent: true,
            }
        } else {
            MathNode::Scripts {
                base: Box::new(node),
                sub: None,
                sup: Some(mark),
            }
        };
    }
    node
}

/// An operator node.
fn op(text: &str) -> MathNode {
    MathNode::Operator {
        text: text.to_string(),
        height_em: None,
    }
}

/// The slots of a template, taken in order (an empty slot is `None`).
type Slots = std::vec::IntoIter<Option<MathNode>>;

/// The next slot of a template, or an empty row.
fn next_slot(slots: &mut Slots) -> MathNode {
    slots.next().flatten().unwrap_or_else(|| MathNode::Row(Vec::new()))
}

/// The formula of a template, from its selector, variation, slots and
/// characters.
fn build_template(selector: u8, variation: u16, slots: Vec<Option<MathNode>>, chars: &[char]) -> Object {
    let mut slots = slots.into_iter();
    let node = match selector {
        // Fences: angle, paren, brace, bracket, bar, double bar, floor,
        // ceiling, open bracket, interval.
        0..=9 => fence_template(selector, variation, &mut slots, chars),
        10 => {
            let base = next_slot(&mut slots);
            match slots.next().flatten() {
                Some(index) if variation & 1 == 1 => MathNode::Root {
                    base: Box::new(base),
                    index: Box::new(index),
                },
                _ => MathNode::Sqrt(Box::new(base)),
            }
        }
        11 => MathNode::Frac {
            num: Box::new(next_slot(&mut slots)),
            den: Box::new(next_slot(&mut slots)),
        },
        12 => accent(next_slot(&mut slots), "_", false),
        13 => accent(next_slot(&mut slots), "\u{AF}", true),
        14 => {
            let over = slots.next().flatten();
            let under = slots.next().flatten();
            MathNode::UnderOver {
                base: Box::new(op("\u{2192}")),
                under: under.map(Box::new),
                over: over.map(Box::new),
                accent: false,
            }
        }
        15..=23 => operator_template(selector, &mut slots, chars),
        24 | 25 => brace_template(selector, &mut slots),
        26 => {
            let dividend = next_slot(&mut slots);
            let mut row = vec![op(")"), dividend];
            if let Some(quotient) = slots.next().flatten() {
                row.insert(0, quotient);
            }
            MathNode::Row(row)
        }
        27..=29 => {
            let sub = slots.next().flatten();
            let sup = slots.next().flatten();
            return Object::Scripts { sub, sup };
        }
        30 => {
            let mut row = vec![op("\u{27E8}"), next_slot(&mut slots)];
            if let Some(right) = slots.next().flatten() {
                row.push(op("|"));
                row.push(right);
            }
            row.push(op("\u{27E9}"));
            MathNode::Row(row)
        }
        31 => accent(next_slot(&mut slots), "\u{2192}", true),
        32 => accent(next_slot(&mut slots), "~", true),
        33 => accent(next_slot(&mut slots), "^", true),
        34 => accent(next_slot(&mut slots), "\u{2322}", true),
        _ => next_slot(&mut slots),
    };
    Object::Node(node)
}

/// `base` with `mark` above it (or below when `above` is false), drawn as
/// an accent.
fn accent(base: MathNode, mark: &str, above: bool) -> MathNode {
    let mark = Some(Box::new(op(mark)));
    MathNode::UnderOver {
        base: Box::new(base),
        under: if above { None } else { mark.clone() },
        over: if above { mark } else { None },
        accent: true,
    }
}

/// A fence template: the delimiters its variation keeps around its slot.
fn fence_template(selector: u8, variation: u16, slots: &mut Slots, chars: &[char]) -> MathNode {
    let (left, right) = fence(selector, chars);
    let mut row = Vec::new();
    // Variation bits: left fence (1), right fence (2).
    if variation & 0x01 != 0 || variation == 0 {
        row.push(op(left));
    }
    row.push(next_slot(slots));
    if variation & 0x02 != 0 || variation == 0 {
        row.push(op(right));
    }
    MathNode::Row(row)
}

/// A big operator template: integrals, sums and the like with their
/// operand (15 to 20), alone (21, 22), and limits (23).
fn operator_template(selector: u8, slots: &mut Slots, chars: &[char]) -> MathNode {
    if selector >= 21 {
        let main = if selector == 23 { slots.next().flatten() } else { None };
        let lower = slots.next().flatten();
        let upper = slots.next().flatten();
        let base = main.unwrap_or_else(|| op(&big_operator(selector, chars)));
        return MathNode::UnderOver {
            base: Box::new(base),
            under: lower.map(Box::new),
            over: upper.map(Box::new),
            accent: false,
        };
    }
    let operand = next_slot(slots);
    let lower = slots.next().flatten().map(Box::new);
    let upper = slots.next().flatten().map(Box::new);
    let base = Box::new(op(&big_operator(selector, chars)));
    let operator = if selector == 15 {
        MathNode::Scripts {
            base,
            sub: lower,
            sup: upper,
        }
    } else {
        MathNode::UnderOver {
            base,
            under: lower,
            over: upper,
            accent: false,
        }
    };
    MathNode::Row(vec![operator, operand])
}

/// A horizontal brace (24) or bracket (25) over its slot, with a label.
fn brace_template(selector: u8, slots: &mut Slots) -> MathNode {
    let main = next_slot(slots);
    let label = slots.next().flatten();
    let brace = op(if selector == 24 { "\u{23DE}" } else { "\u{23B4}" });
    let braced = MathNode::UnderOver {
        base: Box::new(main),
        under: None,
        over: Some(Box::new(brace)),
        accent: false,
    };
    match label {
        Some(label) => MathNode::UnderOver {
            base: Box::new(braced),
            under: None,
            over: Some(Box::new(label)),
            accent: false,
        },
        None => braced,
    }
}

/// The template of version 5 that template `selector` of version 3 is
/// (version 3 numbers them differently), with the brackets of the
/// intervals it names.
fn v3_selector(selector: u8) -> (u8, Option<(char, char)>) {
    match selector {
        0..=7 => (selector, None),
        8 => (9, Some(('[', '['))),
        9 => (9, Some((']', ']'))),
        10 => (9, Some((']', '['))),
        11 => (9, Some(('[', ')'))),
        12 => (9, Some(('(', ']'))),
        13 => (10, None),
        14 | 41 => (11, None),
        15 | 44 => (29, None),
        16 => (12, None),
        17 => (13, None),
        18..=20 => (14, None),
        21..=26 => (15, None),
        27 => (24, None),
        28 => (25, None),
        29 | 30 => (16, None),
        31 | 32 => (17, None),
        33 | 34 => (18, None),
        35 | 36 => (19, None),
        37 | 38 => (20, None),
        39 => (23, None),
        40 => (26, None),
        42 => (21, None),
        43 => (22, None),
        45 => (30, None),
        46..=49 => (selector - 15, None),
        _ => (selector.saturating_add(100), None),
    }
}

/// The left and right delimiters of fence template `selector`, from its
/// characters when it has them.
fn fence(selector: u8, chars: &[char]) -> (&'static str, &'static str) {
    let default = match selector {
        0 => ("\u{27E8}", "\u{27E9}"),
        1 => ("(", ")"),
        2 => ("{", "}"),
        3 => ("[", "]"),
        4 => ("|", "|"),
        5 => ("\u{2016}", "\u{2016}"),
        6 => ("\u{230A}", "\u{230B}"),
        7 => ("\u{2308}", "\u{2309}"),
        8 => ("\u{27E6}", "\u{27E7}"),
        _ => ("(", ")"),
    };
    if selector != 9 {
        return default;
    }
    // An interval: its brackets are its characters.
    let side = |ch: Option<&char>, fallback: &'static str| match ch {
        Some('(') => "(",
        Some(')') => ")",
        Some('[') => "[",
        Some(']') => "]",
        _ => fallback,
    };
    (side(chars.first(), "("), side(chars.get(1), ")"))
}

/// The symbol of big operator template `selector`, from its character when
/// it has one.
fn big_operator(selector: u8, chars: &[char]) -> String {
    if let Some(ch) = chars.iter().find(|ch| !ch.is_whitespace()) {
        return ch.to_string();
    }
    match selector {
        15 | 21 => "\u{222B}",
        17 => "\u{220F}",
        18 => "\u{2210}",
        19 => "\u{22C3}",
        20 => "\u{22C2}",
        23 => "lim",
        _ => "\u{2211}",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prints the start of the `Equation Native` stream of the OLE object
    /// `KAKEMONO_OLE` and its formula (run by hand: `cargo test
    /// ole_formula -- --ignored --nocapture`).
    #[test]
    #[ignore = "a tool for looking at equation objects"]
    fn ole_formula() {
        let path = std::env::var("KAKEMONO_OLE").expect("KAKEMONO_OLE names a file");
        let ole = std::fs::read(path).expect("the object is readable");
        let native = crate::word::equation_native(&ole).expect("an Equation Native stream");
        let head: Vec<String> = native.iter().take(160).map(|byte| format!("{byte:02x}")).collect();
        println!("{}", head.join(" "));
        println!("{:?}", equation(&native));
    }

    /// MTEF 5 data: header, then `records`.
    fn mtef5(records: &[u8]) -> Vec<u8> {
        let mut data = vec![5, 1, 1, 6, 0];
        data.extend_from_slice(b"DSMT6\0");
        data.push(0);
        data.extend_from_slice(records);
        data
    }

    /// A character record of version 5: typeface and code.
    fn ch(face: u8, code: char) -> Vec<u8> {
        let code = code as u16;
        vec![CHAR, 0, face + 128, (code & 0xFF) as u8, (code >> 8) as u8]
    }

    /// x² over 2 reads as a fraction with a superscript in its numerator.
    #[test]
    fn fraction_with_a_square() {
        let mut records = vec![LINE, 0];
        records.extend_from_slice(&[TMPL, 0, 11, 0, 0]);
        // Numerator: x then a superscript template with 2.
        records.extend_from_slice(&[LINE, 0]);
        records.extend(ch(3, 'x'));
        records.extend_from_slice(&[TMPL, 0, 28, 0, 0, LINE, OPT_LINE_NULL, LINE, 0]);
        records.extend(ch(8, '2'));
        records.extend_from_slice(&[END, END, END]);
        // Denominator: 2.
        records.extend_from_slice(&[LINE, 0]);
        records.extend(ch(8, '2'));
        records.extend_from_slice(&[END, END, END]);
        let node = equation(&mtef5(&records)).expect("a formula");
        let MathNode::Frac { num, den } = node else {
            panic!("not a fraction: {node:?}")
        };
        assert!(matches!(*den, MathNode::Number(ref text) if text == "2"));
        let MathNode::Scripts { base, sup, sub } = *num else {
            panic!("no script")
        };
        assert!(sub.is_none());
        assert!(matches!(*base, MathNode::Ident { ref text, .. } if text == "x"));
        assert!(matches!(sup.as_deref(), Some(MathNode::Number(text)) if text == "2"));
    }

    /// Letters of a function name and digits of a number are grouped.
    #[test]
    fn function_names_and_numbers_are_grouped() {
        let mut records = vec![LINE, 0];
        for letter in "sin".chars() {
            records.extend(ch(FACE_FUNCTION, letter));
        }
        records.extend(ch(3, 'x'));
        records.extend(ch(6, '='));
        for digit in "0.5".chars() {
            records.extend(ch(FACE_NUMBER, digit));
        }
        records.extend_from_slice(&[END, END]);
        let MathNode::Row(nodes) = equation(&mtef5(&records)).expect("a formula") else {
            panic!("not a row")
        };
        assert!(matches!(&nodes[0], MathNode::Ident { text, upright: true } if text == "sin"));
        assert!(matches!(&nodes[3], MathNode::Number(text) if text == "0.5"));
    }

    /// Version 3 numbers its templates its own way: 17 is an overline
    /// (here over "1,2", as in "i = 1,2" for a range).
    #[test]
    fn version_3_overline() {
        let mut data = vec![0x1C, 0, 0, 0, 2, 0];
        data.resize(OLE_HEADER_LEN, 0);
        data.extend_from_slice(&[3, 1, 1, 3, 1, 0x0A, 0x01, 0x03, 0x11, 0, 0, 0x01]);
        data.extend_from_slice(&[0x02, 0x88, b'1', 0, 0x02, 0x82, b',', 0, 0x02, 0x88, b'2', 0]);
        data.extend_from_slice(&[0, 0, 0, 0]);
        let node = equation(&data).expect("a formula");
        let MathNode::UnderOver {
            over: Some(mark),
            accent: true,
            ..
        } = node
        else {
            panic!("not an overline: {node:?}")
        };
        assert!(matches!(*mark, MathNode::Operator { ref text, .. } if text == "\u{AF}"));
    }
}
