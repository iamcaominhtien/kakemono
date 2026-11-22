//! Styled text as one string with HTML-like tags, the form in which the
//! JSON output gives the text of a block, and the form it is read back in
//! when an edited JSON file is applied ([`super::apply`]).
//!
//! The text of a block is a list of [`Span`]s. Written as a single string it
//! can be edited (or translated) as a whole, word order included, while its
//! styles stay attached to the words they cover:
//!
//! ```text
//! The <i>chirp mass</i> <m id="1">\mathcal{M}</m> was measured [<a href="#ref-16">16</a>].
//! ```
//!
//! | Tag | Meaning |
//! | --- | --- |
//! | `<b>`, `<i>`, `<code>` | bold, italic, monospaced |
//! | `<sup>`, `<sub>` | raised, lowered |
//! | `<sf>`, `<sc>`, `<mf>` | sans-serif, small capitals, math font |
//! | `<a href="…">` | a link |
//! | `<font color="#rrggbb">` | coloured text |
//! | `<m id="n">…</m>` | the n-th formula of the block, with its LaTeX |
//! | `<tn>…</tn>` | a translator's note attached at that point of the text |
//!
//! A translator's note (`“<i>Mon ami</i><tn>Tiếng Pháp: <i>bạn tôi</i>.
//! (ND)</tn> Hastings!”`) holds styled text and links, but no formula and
//! no other note; it does not take the styles open around it. Its text is
//! printed exactly as it is given; the outputs number the notes and show
//! them as footnotes (see [`crate::model::Span::note`]).
//!
//! A formula is not text: reading the markup back puts the original spans
//! of formula `n` where `<m id="n">` stands, whatever is between the tags
//! (or `<m id="n"/>`), so that editing the text around a formula never
//! breaks it. `&amp;`, `&lt;`, `&gt;` and `&quot;` stand for `&`, `<`, `>`
//! and `"`; a `&` that starts no entity is kept as it is.

use super::super::inline_math::{self, Piece};
use super::super::latex::math::latex;
use crate::model::{BOLD, ITALIC, MATH, MONO, SANS, SMALL_CAPS, SUB, SUP, Span};

/// The tags of the style flags, from the outermost to the innermost.
const STYLE_TAGS: [(u8, &str); 8] = [
    (BOLD, "b"),
    (ITALIC, "i"),
    (MONO, "code"),
    (SANS, "sf"),
    (SMALL_CAPS, "sc"),
    (MATH, "mf"),
    (SUP, "sup"),
    (SUB, "sub"),
];

/// Text with markup, and the formulas its `<m>` tags stand for.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Markup {
    /// The text with its tags.
    pub(crate) text: String,
    /// The spans of each formula: `<m id="n">` stands for `formulas[n - 1]`.
    pub(crate) formulas: Vec<Vec<Span>>,
}

/// Something that wraps a run of text: a link, a colour or a style flag.
#[derive(Debug, Clone, PartialEq)]
enum Wrap {
    /// A link to this target.
    Link(String),
    /// Text in this colour.
    Color([u8; 3]),
    /// One style flag.
    Style(u8),
}

impl Wrap {
    /// The opening tag.
    fn open(&self) -> String {
        match self {
            Self::Link(href) => format!("<a href=\"{}\">", escape(href)),
            Self::Color([red, green, blue]) => format!("<font color=\"#{red:02x}{green:02x}{blue:02x}\">"),
            Self::Style(flag) => format!("<{}>", style_tag(*flag)),
        }
    }

    /// The closing tag.
    fn close(&self) -> String {
        match self {
            Self::Link(_) => "</a>".to_string(),
            Self::Color(_) => "</font>".to_string(),
            Self::Style(flag) => format!("</{}>", style_tag(*flag)),
        }
    }

    /// The name of the tag, which its closing tag repeats.
    fn name(&self) -> &'static str {
        match self {
            Self::Link(_) => "a",
            Self::Color(_) => "font",
            Self::Style(flag) => style_tag(*flag),
        }
    }
}

/// The spans read back from a text with markup.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Parsed {
    /// The styled text.
    pub(crate) spans: Vec<Span>,
    /// The ids of the formulas the text holds, in order (a formula may be
    /// missing, or appear twice).
    pub(crate) formulas_used: Vec<usize>,
    /// Tags that were not closed, or closed without being opened: the text
    /// was still read, but its styles may not be the intended ones.
    pub(crate) warnings: Vec<String>,
}

/// The text of `spans` with markup. Formulas in running text (see
/// [`inline_math`]) become `<m>` tags holding their LaTeX.
pub(crate) fn to_markup(spans: &[Span]) -> Markup {
    let pieces = if inline_math::has_math(spans) {
        inline_math::split(spans)
    } else {
        vec![Piece::Text(spans.to_vec())]
    };
    let mut writer = Writer::default();
    let mut formulas = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Text(spans) => spans.iter().for_each(|span| writer.span(span)),
            Piece::Math(node, _, spans) => {
                writer.close_all();
                formulas.push(spans);
                writer
                    .out
                    .push_str(&format!("<m id=\"{}\">{}</m>", formulas.len(), escape(&latex(&node))));
            }
        }
    }
    writer.close_all();
    Markup {
        text: writer.out,
        formulas,
    }
}

/// Reads a text with markup back into spans. `formulas` are the spans the
/// `<m>` tags stand for (see [`Markup::formulas`]).
///
/// # Errors
/// A message naming the problem when the text has an unknown tag, a tag
/// that is not closed by `>`, or a formula id that does not exist.
pub(crate) fn from_markup(text: &str, formulas: &[Vec<Span>]) -> Result<Parsed, String> {
    let mut reader = Reader::default();
    let mut rest = text;
    while let Some(pos) = rest.find(['<', '&']) {
        reader.text(&rest[..pos]);
        rest = &rest[pos..];
        if rest.starts_with('&') {
            let (ch, len) = entity(rest);
            reader.text(&ch);
            rest = &rest[len..];
            continue;
        }
        // A '<' that starts no tag ("x < y" written without its entity)
        // is text.
        if !rest[1..].starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '/') {
            reader.text("<");
            rest = &rest[1..];
            continue;
        }
        let end = rest
            .find('>')
            .ok_or_else(|| format!("a tag is not closed by '>': {}", excerpt(rest)))?;
        let tag = Tag::parse(&rest[1..end])?;
        rest = &rest[end + 1..];
        if let Tag::Formula { id, closed } = tag {
            let spans = formulas
                .get(id.wrapping_sub(1))
                .ok_or_else(|| format!("there is no formula {id} in this block"))?;
            if reader.note.is_some() {
                reader.warnings.push(format!(
                    "a translator's note cannot hold a formula: <m id=\"{id}\"> left out"
                ));
            } else {
                reader.formula(id, spans);
            }
            // The LaTeX between the tags is only there to be read.
            if !closed {
                match rest.find("</m>") {
                    Some(pos) => rest = &rest[pos + "</m>".len()..],
                    None => reader.warnings.push(format!("<m id=\"{id}\"> is not closed")),
                }
            }
            continue;
        }
        reader.tag(tag);
    }
    reader.text(rest);
    Ok(reader.finish())
}

/// The tag name of a style flag.
fn style_tag(flag: u8) -> &'static str {
    STYLE_TAGS
        .iter()
        .find(|(bit, _)| *bit == flag)
        .map_or("span", |(_, name)| name)
}

/// What wraps a span, from the outermost to the innermost.
fn wraps_of(span: &Span) -> Vec<Wrap> {
    let mut wraps = Vec::new();
    if let Some(href) = &span.href {
        wraps.push(Wrap::Link(href.clone()));
    }
    if span.color != [0; 3] {
        wraps.push(Wrap::Color(span.color));
    }
    for (flag, _) in STYLE_TAGS {
        if span.style & flag != 0 {
            wraps.push(Wrap::Style(flag));
        }
    }
    wraps
}

/// Escapes `&`, `<`, `>` and `"`.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// The character an entity at the start of `text` stands for, and the
/// length of the entity; a `&` that starts no entity stands for itself.
fn entity(text: &str) -> (String, usize) {
    const NAMED: [(&str, char); 5] = [("amp", '&'), ("lt", '<'), ("gt", '>'), ("quot", '"'), ("apos", '\'')];
    let Some(end) = text[1..]
        .find(';')
        .map(|pos| pos + 1)
        .filter(|&end| end <= MAX_ENTITY_LEN)
    else {
        return ("&".to_string(), 1);
    };
    let name = &text[1..end];
    let ch = NAMED
        .iter()
        .find(|(entity, _)| *entity == name)
        .map(|&(_, ch)| ch)
        .or_else(|| {
            let code = match name.strip_prefix('#') {
                Some(hex) if hex.starts_with(['x', 'X']) => u32::from_str_radix(&hex[1..], 16).ok(),
                Some(dec) => dec.parse().ok(),
                None => None,
            };
            code.and_then(char::from_u32)
        });
    match ch {
        Some(ch) => (ch.to_string(), end + 1),
        None => ("&".to_string(), 1),
    }
}

/// The tag of a translator's note.
const NOTE_TAG: &str = "tn";

/// Longest entity read (`&#x10FFFF;` without the `;`).
const MAX_ENTITY_LEN: usize = 10;

/// The start of `text`, to show where a problem is.
fn excerpt(text: &str) -> String {
    text.chars().take(40).collect()
}

/// Writes spans with the fewest tags: a wrap shared by consecutive spans
/// stays open over them.
#[derive(Default)]
struct Writer {
    /// The text written so far.
    out: String,
    /// The wraps open now, outermost first.
    open: Vec<Wrap>,
}

impl Writer {
    /// Writes one span.
    fn span(&mut self, span: &Span) {
        if let Some(note) = &span.note {
            self.note(note);
            return;
        }
        if span.text.is_empty() {
            return;
        }
        let wanted = wraps_of(span);
        let kept = self
            .open
            .iter()
            .zip(&wanted)
            .take_while(|(open, want)| open == want)
            .count();
        while self.open.len() > kept {
            let wrap = self.open.pop().expect("more wraps open than kept");
            self.out.push_str(&wrap.close());
        }
        for wrap in &wanted[kept..] {
            self.out.push_str(&wrap.open());
        }
        self.open = wanted;
        self.out.push_str(&escape(&span.text));
    }

    /// Writes a translator's note: `<tn>`, its content with tags of its own
    /// (the wraps open around it are closed first), `</tn>`.
    fn note(&mut self, content: &[Span]) {
        self.close_all();
        let mut inner = Writer::default();
        content.iter().for_each(|span| inner.span(span));
        inner.close_all();
        self.out.push_str(&format!("<{NOTE_TAG}>{}</{NOTE_TAG}>", inner.out));
    }

    /// Closes every open wrap.
    fn close_all(&mut self) {
        while let Some(wrap) = self.open.pop() {
            self.out.push_str(&wrap.close());
        }
    }
}

/// A tag of the markup.
#[derive(Debug, Clone, PartialEq)]
enum Tag {
    /// An opening tag.
    Open(Wrap),
    /// A closing tag, by name.
    Close(String),
    /// The opening tag of a translator's note.
    Note,
    /// A formula: `<m id="n">` (`closed` false) or `<m id="n"/>`.
    Formula {
        /// Its number in the block, from 1.
        id: usize,
        /// The tag closes itself.
        closed: bool,
    },
}

impl Tag {
    /// Reads the inside of a tag (between `<` and `>`).
    fn parse(inner: &str) -> Result<Self, String> {
        let inner = inner.trim();
        if let Some(name) = inner.strip_prefix('/') {
            return Ok(Self::Close(name.trim().to_ascii_lowercase()));
        }
        let (body, closed) = match inner.strip_suffix('/') {
            Some(body) => (body.trim(), true),
            None => (inner, false),
        };
        let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let name = body[..name_end].to_ascii_lowercase();
        let attrs = &body[name_end..];
        match name.as_str() {
            NOTE_TAG => Ok(Self::Note),
            "m" => {
                let id = attribute(attrs, "id")
                    .and_then(|id| id.trim().parse().ok())
                    .ok_or_else(|| format!("<{inner}> has no formula id"))?;
                Ok(Self::Formula { id, closed })
            }
            "a" => {
                let href = attribute(attrs, "href").ok_or_else(|| format!("<{inner}> has no href"))?;
                Ok(Self::Open(Wrap::Link(href)))
            }
            "font" => {
                let color = attribute(attrs, "color")
                    .and_then(|color| parse_color(&color))
                    .ok_or_else(|| format!("<{inner}> has no colour #rrggbb"))?;
                Ok(Self::Open(Wrap::Color(color)))
            }
            _ => STYLE_TAGS
                .iter()
                .find(|(_, tag)| *tag == name)
                .map(|&(flag, _)| Self::Open(Wrap::Style(flag)))
                .ok_or_else(|| format!("unknown tag <{inner}>")),
        }
    }
}

/// The value of attribute `name` in the attributes of a tag (quoted with
/// `"` or `'`), with its entities read.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        let pos = rest.find(name)?;
        let before_ok = rest[..pos].chars().next_back().is_none_or(char::is_whitespace);
        let after = rest[pos + name.len()..].trim_start();
        if before_ok && let Some(value) = after.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next().filter(|ch| *ch == '"' || *ch == '\'')?;
            let end = value[1..].find(quote)? + 1;
            return Some(unescape(&value[1..end]));
        }
        rest = &rest[pos + name.len()..];
    }
}

/// Text with its entities read.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let (ch, len) = entity(&rest[pos..]);
        out.push_str(&ch);
        rest = &rest[pos + len..];
    }
    out.push_str(rest);
    out
}

/// An RGB colour written `#rrggbb`.
fn parse_color(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let part = |idx: usize| u8::from_str_radix(&hex[idx..idx + 2], 16).ok();
    Some([part(0)?, part(2)?, part(4)?])
}

/// Builds spans from text and tags.
#[derive(Default)]
struct Reader {
    /// The spans read so far.
    spans: Vec<Span>,
    /// The wraps open now, in the order they were opened.
    open: Vec<Wrap>,
    /// Whether the last span is a formula's, which text never joins.
    after_formula: bool,
    /// The formulas found, in order.
    formulas_used: Vec<usize>,
    /// Problems that did not stop the reading.
    warnings: Vec<String>,
    /// While a translator's note is read: the text around it, set aside
    /// until the note is closed.
    note: Option<Outside>,
    /// `<tn>` tags met inside a note, which hold no note of their own: as
    /// many `</tn>` are skipped.
    nested_notes: usize,
}

/// The state of the text around a translator's note, while the note is
/// read.
#[derive(Default)]
struct Outside {
    /// The spans before the note.
    spans: Vec<Span>,
    /// The wraps open around the note.
    open: Vec<Wrap>,
    /// Whether the last span before the note is a formula's.
    after_formula: bool,
}

impl Reader {
    /// Adds text, styled by the open wraps.
    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let mut span = Span::plain(text);
        for wrap in &self.open {
            match wrap {
                Wrap::Link(href) => span.href = Some(href.clone()),
                Wrap::Color(color) => span.color = *color,
                Wrap::Style(flag) => span.style |= flag,
            }
        }
        match self.spans.last_mut() {
            Some(last)
                if !self.after_formula
                    && last.note.is_none()
                    && last.style == span.style
                    && last.href == span.href
                    && last.color == span.color =>
            {
                last.text.push_str(text);
            }
            _ => self.spans.push(span),
        }
        self.after_formula = false;
    }

    /// Adds the spans of formula `id`.
    fn formula(&mut self, id: usize, spans: &[Span]) {
        self.spans.extend(spans.iter().cloned());
        self.formulas_used.push(id);
        self.after_formula = true;
    }

    /// Opens or closes a wrap.
    fn tag(&mut self, tag: Tag) {
        match tag {
            Tag::Open(wrap) => self.open.push(wrap),
            Tag::Note => self.open_note(),
            Tag::Close(name) if name == NOTE_TAG => self.close_note(),
            Tag::Close(name) => match self.open.iter().rposition(|wrap| wrap.name() == name) {
                Some(pos) => {
                    self.open.remove(pos);
                }
                None => self.warnings.push(format!("</{name}> closes no open tag")),
            },
            Tag::Formula { .. } => unreachable!("formulas are read by from_markup"),
        }
    }

    /// Starts a translator's note: the text read from here on is its
    /// content, without the styles open around it.
    fn open_note(&mut self) {
        if self.note.is_some() {
            self.warnings.push(format!(
                "a translator's note cannot hold another note: <{NOTE_TAG}> left out"
            ));
            self.nested_notes += 1;
            return;
        }
        self.note = Some(Outside {
            spans: std::mem::take(&mut self.spans),
            open: std::mem::take(&mut self.open),
            after_formula: std::mem::take(&mut self.after_formula),
        });
    }

    /// Ends a translator's note: its content becomes the note of a call
    /// placed after the text before it. An empty note is left out.
    fn close_note(&mut self) {
        if self.nested_notes > 0 {
            self.nested_notes -= 1;
            return;
        }
        let Some(outside) = self.note.take() else {
            self.warnings.push(format!("</{NOTE_TAG}> closes no open tag"));
            return;
        };
        for wrap in &self.open {
            self.warnings
                .push(format!("<{}> is not closed inside a translator's note", wrap.name()));
        }
        let content = std::mem::replace(&mut self.spans, outside.spans);
        self.open = outside.open;
        if content.iter().all(|span| span.text.trim().is_empty()) {
            self.warnings.push(format!("an empty <{NOTE_TAG}> was left out"));
            self.after_formula = outside.after_formula;
            return;
        }
        self.spans.push(Span {
            note: Some(content),
            ..Span::plain("")
        });
        self.after_formula = false;
    }

    /// The spans, with a warning for each tag left open.
    fn finish(mut self) -> Parsed {
        if self.note.is_some() {
            self.warnings.push(format!("<{NOTE_TAG}> is not closed"));
            self.close_note();
        }
        for wrap in &self.open {
            self.warnings.push(format!("<{}> is not closed", wrap.name()));
        }
        Parsed {
            spans: self.spans,
            formulas_used: self.formulas_used,
            warnings: self.warnings,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, style: u8) -> Span {
        Span {
            style,
            ..Span::plain(text)
        }
    }

    fn link(text: &str, href: &str) -> Span {
        Span {
            href: Some(href.to_string()),
            ..Span::plain(text)
        }
    }

    #[test]
    fn styles_become_tags_and_come_back() {
        let spans = vec![
            span("The ", 0),
            span("goal", ITALIC),
            span(" of ", 0),
            span("bold italic", BOLD | ITALIC),
            span(" bold", BOLD),
            span(" x<y & \"z\" ", 0),
            link("16", "#ref-16"),
        ];
        let markup = to_markup(&spans);
        assert_eq!(
            markup.text,
            "The <i>goal</i> of <b><i>bold italic</i> bold</b> x&lt;y &amp; &quot;z&quot; <a href=\"#ref-16\">16</a>"
        );
        let parsed = from_markup(&markup.text, &markup.formulas).unwrap();
        assert_eq!(parsed.spans, spans);
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn colour_and_rare_styles_come_back() {
        let spans = vec![
            Span {
                color: [0xff, 0, 0x10],
                ..span("red", SANS)
            },
            span(" small caps", SMALL_CAPS),
            span("2", SUP),
            span("i", SUB),
            span(" code", MONO),
        ];
        let markup = to_markup(&spans);
        let parsed = from_markup(&markup.text, &markup.formulas).unwrap();
        assert_eq!(parsed.spans, spans);
    }

    #[test]
    fn formulas_are_kept_whatever_the_text_around_them() {
        let spans = vec![
            span("the regret ", 0),
            span("f", MATH | ITALIC),
            span("t", SUB | MATH | ITALIC),
            span(" is small", 0),
        ];
        let markup = to_markup(&spans);
        assert_eq!(markup.formulas.len(), 1);
        assert!(markup.text.starts_with("the regret <m id=\"1\">"), "{}", markup.text);
        assert!(markup.text.ends_with("</m> is small"), "{}", markup.text);
        let parsed = from_markup(&markup.text, &markup.formulas).unwrap();
        assert_eq!(parsed.spans, spans);
        // Edited, with the formula moved and its LaTeX changed.
        let parsed = from_markup("độ hối tiếc <m id=\"1\">garbled</m> là nhỏ", &markup.formulas).unwrap();
        let text: String = parsed.spans.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(text, "độ hối tiếc ft là nhỏ");
        assert_eq!(parsed.formulas_used, vec![1]);
        let parsed = from_markup("<m id=\"1\"/> nhỏ", &markup.formulas).unwrap();
        assert_eq!(parsed.spans[0], markup.formulas[0][0]);
    }

    #[test]
    fn edited_markup_is_read_leniently() {
        // Tags closed in the wrong order, entities, a bare ampersand.
        let parsed = from_markup("<b><i>x</b></i> A &amp; B & C &#x3b1;", &[]).unwrap();
        assert_eq!(parsed.spans, vec![span("x", BOLD | ITALIC), span(" A & B & C α", 0)]);
        assert!(parsed.warnings.is_empty());
        let parsed = from_markup("x < y <= z", &[]).unwrap();
        assert_eq!(parsed.spans, vec![span("x < y <= z", 0)]);
        let parsed = from_markup("<B>x</b></i>", &[]).unwrap();
        assert_eq!(parsed.spans, vec![span("x", BOLD)]);
        assert_eq!(parsed.warnings.len(), 1);
        let parsed = from_markup("<a href='#sec-2'>x", &[]).unwrap();
        assert_eq!(parsed.spans, vec![link("x", "#sec-2")]);
        assert_eq!(parsed.warnings.len(), 1);
    }

    fn note(content: Vec<Span>) -> Span {
        Span {
            note: Some(content),
            ..Span::plain("")
        }
    }

    #[test]
    fn translator_notes_become_tags_and_come_back() {
        let spans = vec![
            span("“", 0),
            span("Mon ami", ITALIC),
            note(vec![
                span("Tiếng Pháp: ", 0),
                span("bạn tôi", ITALIC),
                span(". (ND)", 0),
            ]),
            span(" Hastings!”", 0),
        ];
        let markup = to_markup(&spans);
        assert_eq!(
            markup.text,
            "“<i>Mon ami</i><tn>Tiếng Pháp: <i>bạn tôi</i>. (ND)</tn> Hastings!”"
        );
        let parsed = from_markup(&markup.text, &markup.formulas).unwrap();
        assert_eq!(parsed.spans, spans);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
    }

    #[test]
    fn a_note_inside_italic_text_does_not_take_its_style() {
        let parsed = from_markup("<i>Mon ami<tn>bạn <b>tôi</b></tn> Hastings</i>!", &[]).unwrap();
        let expected = vec![
            span("Mon ami", ITALIC),
            note(vec![span("bạn ", 0), span("tôi", BOLD)]),
            span(" Hastings", ITALIC),
            span("!", 0),
        ];
        assert_eq!(parsed.spans, expected);
        assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
        // Written back, the italic text is closed around the note.
        assert_eq!(
            to_markup(&expected).text,
            "<i>Mon ami</i><tn>bạn <b>tôi</b></tn><i> Hastings</i>!"
        );
        // Text on both sides of a note is never joined with it.
        let parsed = from_markup("a<tn>b</tn>c", &[]).unwrap();
        assert_eq!(parsed.spans, vec![span("a", 0), note(vec![span("b", 0)]), span("c", 0)]);
    }

    #[test]
    fn faulty_notes_are_read_with_warnings() {
        // Not closed: the note runs to the end of the text.
        let parsed = from_markup("ami<tn>bạn tôi", &[]).unwrap();
        assert_eq!(parsed.spans, vec![span("ami", 0), note(vec![span("bạn tôi", 0)])]);
        assert_eq!(parsed.warnings, vec!["<tn> is not closed".to_string()]);
        // A note in a note, a formula in a note, a stray </tn>, an empty note.
        let formulas = vec![vec![span("x", MATH | ITALIC)]];
        let parsed = from_markup("a<tn>b<tn>c</tn> <m id=\"1\">x</m></tn></tn><tn> </tn>", &formulas).unwrap();
        assert_eq!(parsed.spans, vec![span("a", 0), note(vec![span("bc ", 0)])]);
        assert_eq!(parsed.warnings.len(), 4, "{:?}", parsed.warnings);
        assert!(parsed.formulas_used.is_empty());
        let parsed = from_markup("<tn><b>x</tn>", &[]).unwrap();
        assert_eq!(parsed.spans, vec![note(vec![span("x", BOLD)])]);
        assert_eq!(parsed.warnings.len(), 1, "{:?}", parsed.warnings);
    }

    #[test]
    fn broken_markup_is_an_error() {
        assert!(from_markup("x <blink>y</blink>", &[]).is_err());
        assert!(from_markup("x <b y", &[]).is_err());
        assert!(from_markup("x <m id=\"2\"/>", &[vec![span("y", MATH)]]).is_err());
        assert!(from_markup("x <m>", &[]).is_err());
        assert!(from_markup("<font color=\"red\">x</font>", &[]).is_err());
    }
}
