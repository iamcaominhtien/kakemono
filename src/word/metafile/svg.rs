//! The SVG a metafile is written to.

use super::dc::{Font, Rgb, num};
use base64::Engine as _;

/// Thinnest line, in picture units (a device pixel).
const MIN_STROKE: f32 = 1.0;

/// The SVG elements drawn so far.
#[derive(Debug, Default)]
pub(super) struct Svg {
    /// The elements, in order.
    elements: Vec<String>,
}

impl Svg {
    /// Whether anything was drawn.
    pub(super) fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// A path, filled with `fill` and stroked with `stroke` (colour,
    /// width).
    pub(super) fn path(&mut self, data: &str, fill: Option<Rgb>, stroke: Option<(Rgb, f32)>, even_odd: bool) {
        if data.is_empty() || fill.is_none() && stroke.is_none() {
            return;
        }
        let fill = fill.map_or_else(|| "none".to_string(), hex);
        let stroke = match stroke {
            Some((color, width)) => format!(
                " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"",
                hex(color),
                num(width.max(MIN_STROKE))
            ),
            None => String::new(),
        };
        let rule = if even_odd { " fill-rule=\"evenodd\"" } else { "" };
        self.elements.push(format!(
            "<path d=\"{}\" fill=\"{fill}\"{rule}{stroke}/>",
            data.trim_end()
        ));
    }

    /// Text at its characters' positions (picture units).
    pub(super) fn text(
        &mut self,
        text: &str,
        positions: &[(f32, f32)],
        size: f32,
        font: &Font,
        color: Rgb,
        spaced: bool,
    ) {
        let Some(&(x, y)) = positions.first() else { return };
        let text: String = text.chars().map(|ch| super::wmf::symbol_char(&font.face, ch)).collect();
        let (xs, ys) = if spaced {
            let xs: Vec<String> = positions.iter().map(|&(x, _)| num(x)).collect();
            let ys: Vec<String> = positions.iter().map(|&(_, y)| num(y)).collect();
            (xs.join(" "), ys.join(" "))
        } else {
            (num(x), num(y))
        };
        let weight = if font.weight >= 600 {
            " font-weight=\"bold\""
        } else {
            ""
        };
        let style = if font.italic { " font-style=\"italic\"" } else { "" };
        let underline = if font.underline {
            " text-decoration=\"underline\""
        } else {
            ""
        };
        let rotate = if font.escapement == 0.0 {
            String::new()
        } else {
            format!(
                " transform=\"rotate({} {} {})\"",
                num(-font.escapement / 10.0),
                num(x),
                num(y)
            )
        };
        self.elements.push(format!(
            "<text x=\"{xs}\" y=\"{ys}\" font-family=\"{}\" font-size=\"{}\" fill=\"{}\"{weight}{style}{underline}{rotate} xml:space=\"preserve\">{}</text>",
            escape(&family_list(&font.face)),
            num(size),
            hex(color),
            escape(&text)
        ));
    }

    /// A bitmap (PNG) in the box (x, y, width, height), picture units.
    pub(super) fn image(&mut self, png: &[u8], bounds: [f32; 4]) {
        let [x, y, width, height] = bounds;
        let (x, width) = if width < 0.0 { (x + width, -width) } else { (x, width) };
        let (y, height) = if height < 0.0 {
            (y + height, -height)
        } else {
            (y, height)
        };
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        self.elements.push(format!(
            "<image x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" href=\"data:image/png;base64,{data}\"/>",
            num(x),
            num(y),
            num(width),
            num(height)
        ));
    }

    /// The SVG document: the area `view` (x, y, width, height) of the
    /// picture, shown at `size` (width, height) points.
    pub(super) fn finish(self, view: [f32; 4], size: (f32, f32)) -> String {
        let [x, y, width, height] = view;
        let mut out = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}pt\" height=\"{}pt\" viewBox=\"{} {} {} {}\" preserveAspectRatio=\"none\">",
            num(size.0),
            num(size.1),
            num(x),
            num(y),
            num(width),
            num(height)
        );
        for element in self.elements {
            out.push_str(&element);
        }
        out.push_str("</svg>");
        out
    }
}

/// `#rrggbb`.
fn hex([red, green, blue]: Rgb) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// The fonts to try for a face of a metafile: the face, its metric-compatible
/// open fonts, then a generic family.
fn family_list(face: &str) -> String {
    let lower = face.to_ascii_lowercase();
    let (open, generic) = match lower.as_str() {
        "times new roman" | "times" => ("Liberation Serif, Tinos", "serif"),
        "arial" | "helvetica" => ("Liberation Sans, Arimo", "sans-serif"),
        "courier new" | "courier" => ("Liberation Mono, Cousine", "monospace"),
        "calibri" => ("Carlito", "sans-serif"),
        "cambria" | "cambria math" => ("Caladea", "serif"),
        "symbol" | "mt extra" => ("DejaVu Serif", "serif"),
        _ => ("DejaVu Sans", "sans-serif"),
    };
    format!("'{}', {open}, {generic}", face.replace('\'', ""))
}

/// Text escaped for XML.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
