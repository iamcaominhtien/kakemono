//! The SVG text of a chart: elements written with escaped text and
//! numbers that are always finite, and the measure of text.

use super::super::Rgb;
use std::fmt::Write as _;

/// The fonts of every text of a chart: CJK fonts first, so that Japanese
/// and Chinese labels have glyphs, then the system's sans-serif.
const FONT_FAMILY: &str = "'Noto Sans CJK JP', 'Hiragino Sans', 'Yu Gothic', Meiryo, sans-serif";
/// The chart area's colour: white, as Excel's default chart area.
const SURFACE: &str = "#ffffff";
/// The width of a Latin character, as a ratio of the font size (a
/// little more than most sans-serif fonts, so that text is not
/// underestimated).
const NARROW_CHAR_RATIO: f32 = 0.58;
/// The width of a CJK or full-width character, as a ratio of the font
/// size.
const WIDE_CHAR_RATIO: f32 = 1.0;
/// What ends a text cut to fit.
const ELLIPSIS: char = '…';

/// How a text is written.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TextStyle {
    /// The font size, in points.
    pub(super) size: f32,
    /// The text colour.
    pub(super) fill: &'static str,
    /// `start`, `middle` or `end`.
    pub(super) anchor: &'static str,
    /// Bold.
    pub(super) bold: bool,
    /// The rotation about the anchor point, in degrees (negative turns
    /// counter-clockwise).
    pub(super) rotate: f32,
}

/// An SVG document being written.
pub(super) struct Svg {
    /// The text so far.
    out: String,
}

impl Svg {
    /// An SVG of `width` by `height` points, its surface painted.
    pub(super) fn new(width: f32, height: f32) -> Self {
        let (width, height) = (num(width), num(height));
        let mut out = String::new();
        let _ = write!(
            out,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}pt" height="{height}pt" viewBox="0 0 {width} {height}" font-family="{FONT_FAMILY}"><rect class="surface" x="0" y="0" width="{width}" height="{height}" fill="{SURFACE}"/>"#
        );
        Self { out }
    }

    /// A rectangle of the class `class`; `attrs` are more attributes
    /// (fill, stroke), written as they are.
    pub(super) fn rect(&mut self, class: &str, x: f32, y: f32, width: f32, height: f32, attrs: &str) {
        let _ = write!(
            self.out,
            r#"<rect class="{class}" x="{}" y="{}" width="{}" height="{}" {attrs}/>"#,
            num(x),
            num(y),
            num(width.max(0.0)),
            num(height.max(0.0))
        );
    }

    /// A straight line of the class `class`, in the colour `stroke`, `width`
    /// points wide.
    pub(super) fn line(&mut self, class: &str, from: (f32, f32), to: (f32, f32), stroke: &str, width: f32) {
        let _ = write!(
            self.out,
            r#"<line class="{class}" x1="{}" y1="{}" x2="{}" y2="{}" stroke="{stroke}" stroke-width="{}"/>"#,
            num(from.0),
            num(from.1),
            num(to.0),
            num(to.1),
            num(width)
        );
    }

    /// A path of the class `class` with the data `data`; `attrs` are more
    /// attributes, written as they are.
    pub(super) fn path(&mut self, class: &str, data: &str, attrs: &str) {
        if data.is_empty() {
            return;
        }
        let _ = write!(self.out, r#"<path class="{class}" d="{data}" {attrs}/>"#);
    }

    /// A marker: a dot of the colour `color` with a ring of the surface,
    /// so that it stays apart from the lines it sits on.
    pub(super) fn marker(&mut self, at: (f32, f32), radius: f32, color: Rgb) {
        let _ = write!(
            self.out,
            r#"<circle class="marker" cx="{}" cy="{}" r="{}" fill="{}" stroke="{SURFACE}" stroke-width="1"/>"#,
            num(at.0),
            num(at.1),
            num(radius),
            hex(color)
        );
    }

    /// The text `content` of the class `class` at `at` (its baseline).
    pub(super) fn text(&mut self, class: &str, at: (f32, f32), style: TextStyle, content: &str) {
        let (x, y) = (num(at.0), num(at.1));
        let _ = write!(
            self.out,
            r#"<text class="{class}" x="{x}" y="{y}" font-size="{}" fill="{}" text-anchor="{}""#,
            num(style.size),
            style.fill,
            style.anchor
        );
        if style.bold {
            self.out.push_str(r#" font-weight="bold""#);
        }
        if style.rotate != 0.0 {
            let _ = write!(self.out, r#" transform="rotate({} {x} {y})""#, num(style.rotate));
        }
        let _ = write!(self.out, ">{}</text>", escape(content));
    }

    /// The finished SVG.
    pub(super) fn finish(mut self) -> String {
        self.out.push_str("</svg>");
        self.out
    }
}

/// Path data being written: `M x y L x y ...`.
#[derive(Default)]
pub(super) struct PathData {
    /// The data so far.
    data: String,
    /// The next point starts a subpath.
    broken: bool,
}

impl PathData {
    /// Path data with nothing drawn yet.
    pub(super) fn new() -> Self {
        Self {
            data: String::new(),
            broken: true,
        }
    }

    /// A line to `at`, or a move when the path is broken.
    pub(super) fn point(&mut self, at: (f32, f32)) {
        let command = if self.broken { "M" } else { "L" };
        if !self.data.is_empty() {
            self.data.push(' ');
        }
        let _ = write!(self.data, "{command} {} {}", num(at.0), num(at.1));
        self.broken = false;
    }

    /// The next point starts a subpath (a gap in a line).
    pub(super) fn gap(&mut self) {
        self.broken = true;
    }

    /// Closes the current subpath.
    pub(super) fn close(&mut self) {
        if !self.data.is_empty() {
            self.data.push_str(" Z");
        }
        self.broken = true;
    }

    /// The data.
    pub(super) fn into_string(self) -> String {
        self.data
    }
}

/// `value` written with at most two decimals; 0 when it is not finite.
pub(super) fn num(value: f32) -> String {
    let value = if value.is_finite() { value } else { 0.0 };
    let text = format!("{value:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    match text {
        "-0" | "" => "0".to_string(),
        other => other.to_string(),
    }
}

/// `color` as `#rrggbb`.
pub(super) fn hex(color: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2])
}

/// `text` escaped for XML; the characters XML does not allow (control
/// characters) are left out, line breaks become spaces.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' | '\r' | '\t' => out.push(' '),
            '\u{FFFE}' | '\u{FFFF}' => {}
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

/// The width of `text` at the font size `size`, estimated.
pub(super) fn text_width(text: &str, size: f32) -> f32 {
    text.chars().map(|ch| char_width(ch, size)).sum()
}

/// `text` cut, with an ellipsis, to at most `max_width` at the font size
/// `size`.
pub(super) fn fit(text: &str, size: f32, max_width: f32) -> String {
    if text_width(text, size) <= max_width {
        return text.to_string();
    }
    let room = max_width - char_width(ELLIPSIS, size);
    let mut width = 0.0;
    let mut out = String::new();
    for ch in text.chars() {
        width += char_width(ch, size);
        if width > room {
            break;
        }
        out.push(ch);
    }
    out.push(ELLIPSIS);
    out
}

/// The estimated width of `ch` at the font size `size`.
fn char_width(ch: char, size: f32) -> f32 {
    let wide = matches!(u32::from(ch), 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x20000..=0x3FFFD);
    size * if wide { WIDE_CHAR_RATIO } else { NARROW_CHAR_RATIO }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_finite() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(1.256), "1.26");
        assert_eq!(num(-0.001), "0");
        assert_eq!(num(f32::NAN), "0");
        assert_eq!(num(f32::INFINITY), "0");
    }

    #[test]
    fn text_is_escaped_and_measured() {
        assert_eq!(escape("a<b>&\"c\u{1}\nd"), "a&lt;b&gt;&amp;&quot;c d");
        assert!(text_width("東京", 10.0) > text_width("ab", 10.0));
        let cut = fit("a long label that does not fit", 10.0, 40.0);
        assert!(cut.ends_with(ELLIPSIS) && text_width(&cut, 10.0) <= 40.0);
        assert_eq!(fit("ok", 10.0, 40.0), "ok");
    }
}
