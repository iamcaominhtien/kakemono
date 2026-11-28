//! The look of a slide's shapes: the theme its master takes (colours
//! through the colour map, fonts, the fill and line styles that a shape's
//! style refers to), and the fill and outline of a shape or background.

use crate::chart::{Rgb, Theme, drawing_color as color};
use crate::excel::drawing::{TextShape, outline};
use crate::powerpoint::package::EMU_PER_PT;
use crate::word::xml::{DRAWING_NS, child, children};
use roxmltree::Node;

/// The first index of a style's fill reference (`a:fillRef idx`) that
/// names a background fill (`a:bgFillStyleLst`) rather than a shape's.
const BACKGROUND_FILL_FROM: usize = 1001;
/// A share in DrawingML's units: 100 000 is all.
const SHARE_UNITS: f32 = 100_000.0;
/// What a DrawingML angle is written in: 60 000 is a degree.
const ANGLE_UNITS: f32 = 60_000.0;
/// The width of a line that names none, in points (PowerPoint's thinnest
/// theme line, 6350 EMU).
pub(super) const DEFAULT_LINE_PT: f32 = 0.5;

/// The fonts of a theme (`a:fontScheme`): for headings (major) and body
/// text (minor), each Latin, East Asian and complex script.
#[derive(Debug, Clone, Default)]
pub(super) struct Fonts {
    /// The major fonts: Latin, East Asian, complex script.
    major: [String; 3],
    /// The minor fonts.
    minor: [String; 3],
}

impl Fonts {
    /// The fonts of the theme part whose root is `theme`.
    pub(super) fn read(theme: Option<Node<'_, '_>>) -> Self {
        let scheme = theme.and_then(|theme| {
            theme
                .descendants()
                .find(|node| node.has_tag_name((DRAWING_NS, "fontScheme")))
        });
        let faces = |name: &str| {
            let fonts = scheme.and_then(|scheme| child(scheme, DRAWING_NS, name));
            ["latin", "ea", "cs"].map(|script| {
                fonts
                    .and_then(|fonts| child(fonts, DRAWING_NS, script))
                    .and_then(|font| font.attribute("typeface"))
                    .unwrap_or_default()
                    .to_string()
            })
        };
        Self {
            major: faces("majorFont"),
            minor: faces("minorFont"),
        }
    }

    /// The font a run's typeface `name` names: a theme font (`+mj-lt`,
    /// `+mn-ea`, ...) resolved, another as it is; `None` for an empty
    /// one, or a theme font the theme leaves empty.
    pub(super) fn resolve(&self, name: &str) -> Option<String> {
        let name = name.trim();
        let own = match name.strip_prefix('+') {
            Some(theme) => {
                let (kind, script) = theme.split_once('-')?;
                let fonts = if kind == "mj" { &self.major } else { &self.minor };
                let at = match script {
                    "ea" => 1,
                    "cs" => 2,
                    _ => 0,
                };
                fonts[at].clone()
            }
            None => name.to_string(),
        };
        (!own.is_empty()).then_some(own)
    }

    /// The theme font of `kind` (`major` or `minor`) for the script `at`
    /// (0 Latin, 1 East Asian, 2 complex), `None` when the theme has none.
    pub(super) fn theme(&self, major: bool, at: usize) -> Option<String> {
        let fonts = if major { &self.major } else { &self.minor };
        Some(fonts[at].clone()).filter(|font| !font.is_empty())
    }
}

/// The theme of a slide: its colours through the colour map of its
/// master (and of its layout and own, when they override it), its fonts,
/// and its fill and line styles (`a:fmtScheme`).
pub(super) struct Look<'a, 'i> {
    /// The colours.
    pub(super) theme: Theme,
    /// The fonts.
    pub(super) fonts: Fonts,
    /// The theme's format scheme.
    pub(super) format: Option<Node<'a, 'i>>,
}

/// A stop of a gradient.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Stop {
    /// Where it is, from 0 to 1.
    pub(super) at: f32,
    /// Its colour.
    pub(super) rgb: Rgb,
    /// Its opacity.
    pub(super) opacity: f32,
}

/// A gradient fill (`a:gradFill`).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Gradient {
    /// Its stops, in order.
    pub(super) stops: Vec<Stop>,
    /// The direction of a linear one, in degrees clockwise from left to
    /// right.
    pub(super) angle_deg: f32,
    /// It spreads from the centre (`a:path`).
    pub(super) radial: bool,
}

impl Gradient {
    /// The SVG definition of the gradient, with the id `id`, for a fill of
    /// the shape's box.
    pub(super) fn svg(&self, id: &str) -> String {
        let stops: String = self
            .stops
            .iter()
            .map(|stop| {
                format!(
                    "<stop offset=\"{:.4}\" stop-color=\"#{:02x}{:02x}{:02x}\" stop-opacity=\"{:.3}\"/>",
                    stop.at, stop.rgb[0], stop.rgb[1], stop.rgb[2], stop.opacity
                )
            })
            .collect();
        if self.radial {
            format!("<radialGradient id=\"{id}\" cx=\"0.5\" cy=\"0.5\" r=\"0.71\">{stops}</radialGradient>")
        } else {
            format!(
                "<linearGradient id=\"{id}\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"0\" gradientTransform=\"rotate({:.2} 0.5 0.5)\">{stops}</linearGradient>",
                self.angle_deg
            )
        }
    }
}

/// How a shape or background is filled.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Paint {
    /// A colour (a gradient as the average of its stops).
    Solid(Rgb),
    /// A colour seen through: its opacity, above 0 and below 1.
    Clear(Rgb, f32),
    /// A gradient: the average of its colours, and its stops.
    Gradient(Rgb, Box<Gradient>),
    /// The picture of the relationship of this id, stretched.
    Picture(String),
}

impl Look<'_, '_> {
    /// The colour of the colour element in `node`, `phClr` as
    /// `placeholder`.
    pub(super) fn color(&self, node: Node<'_, '_>, placeholder: Rgb) -> Option<Rgb> {
        color(node, &self.theme, placeholder)
    }

    /// The colour of the first colour element among the children of
    /// `holder` (an `a:tcTxStyle`, whose font reference comes first).
    pub(super) fn first_color(&self, holder: Node<'_, '_>) -> Option<Rgb> {
        holder
            .children()
            .filter(|node| node.is_element() && node.tag_name().namespace() == Some(DRAWING_NS))
            .find_map(|node| crate::chart::color_element(node, &self.theme, [0; 3]))
    }

    /// The fill that `holder` (a `p:spPr`, `p:bgPr`, `a:tcPr`, an `a:rPr`)
    /// says it has: `Some(None)` when it says none (`a:noFill`), `None`
    /// when it says nothing. A pattern is its foreground.
    pub(super) fn fill(&self, holder: Node<'_, '_>, placeholder: Rgb) -> Option<Option<Paint>> {
        holder.children().filter(Node::is_element).find_map(|node| {
            if node.tag_name().namespace() != Some(DRAWING_NS) {
                return None;
            }
            Some(match node.tag_name().name() {
                "noFill" => None,
                "solidFill" => self.color(node, placeholder).and_then(|rgb| seen(rgb, opacity(node))),
                "gradFill" => self.gradient(node, placeholder),
                "pattFill" => child(node, DRAWING_NS, "fgClr")
                    .and_then(|fg| self.color(fg, placeholder))
                    .map(Paint::Solid),
                "blipFill" => child(node, DRAWING_NS, "blip")
                    .and_then(|blip| {
                        crate::word::xml::attr(blip, crate::word::xml::REL_NS, "embed")
                            .or_else(|| crate::word::xml::attr(blip, crate::word::xml::REL_NS, "link"))
                    })
                    .map(|id| Paint::Picture(id.to_string())),
                _ => return None,
            })
        })
    }

    /// The average colour of the stops of the gradient `grad`.
    fn gradient(&self, grad: Node<'_, '_>, placeholder: Rgb) -> Option<Paint> {
        let mut stops: Vec<Stop> = child(grad, DRAWING_NS, "gsLst")
            .into_iter()
            .flat_map(|list| children(list, DRAWING_NS, "gs"))
            .filter_map(|stop| {
                let at = stop
                    .attribute("pos")
                    .and_then(|pos| pos.trim().parse::<f32>().ok())
                    .filter(|pos| pos.is_finite())
                    .map_or(0.0, |pos| (pos / SHARE_UNITS).clamp(0.0, 1.0));
                Some(Stop {
                    at,
                    rgb: self.color(stop, placeholder)?,
                    opacity: opacity(stop),
                })
            })
            .collect();
        if stops.is_empty() {
            return None;
        }
        stops.sort_by(|a, b| a.at.total_cmp(&b.at));
        let count = stops.len() as f32;
        let channel = |at: usize| {
            let sum: f32 = stops.iter().map(|stop| f32::from(stop.rgb[at])).sum();
            (sum / count).round().clamp(0.0, 255.0) as u8
        };
        let average = [channel(0), channel(1), channel(2)];
        if stops.iter().all(|stop| stop.opacity <= 0.0) {
            return None;
        }
        let linear = child(grad, DRAWING_NS, "lin");
        let angle_deg = linear
            .and_then(|lin| lin.attribute("ang"))
            .and_then(|angle| angle.trim().parse::<f32>().ok())
            .filter(|angle| angle.is_finite())
            .map_or(0.0, |angle| angle / ANGLE_UNITS);
        Some(Paint::Gradient(
            average,
            Box::new(Gradient {
                stops,
                angle_deg,
                radial: linear.is_none() && child(grad, DRAWING_NS, "path").is_some(),
            }),
        ))
    }

    /// The fill a style reference `reference` (`a:fillRef`, or the
    /// `p:bgRef` of a background) names in the theme's format scheme, in
    /// the reference's colour; `None` for index 0 or a style not there.
    pub(super) fn fill_ref(&self, reference: Node<'_, '_>) -> Option<Paint> {
        let index: usize = reference.attribute("idx")?.trim().parse().ok()?;
        let placeholder = self.color(reference, [0; 3]).unwrap_or_default();
        let (list, at) = if index >= BACKGROUND_FILL_FROM {
            ("bgFillStyleLst", index - BACKGROUND_FILL_FROM)
        } else {
            ("fillStyleLst", index.checked_sub(1)?)
        };
        let styles = child(self.format?, DRAWING_NS, list)?;
        let style = styles.children().filter(Node::is_element).nth(at)?;
        self.one_fill(style, placeholder)
    }

    /// The fill of the fill element `fill` (`a:solidFill`, ...).
    fn one_fill(&self, fill: Node<'_, '_>, placeholder: Rgb) -> Option<Paint> {
        match fill.tag_name().name() {
            "solidFill" => self.color(fill, placeholder).and_then(|rgb| seen(rgb, opacity(fill))),
            "gradFill" => self.gradient(fill, placeholder),
            "pattFill" => child(fill, DRAWING_NS, "fgClr")
                .and_then(|fg| self.color(fg, placeholder))
                .map(Paint::Solid),
            _ => None,
        }
    }

    /// The colour and width (points) of the line a style reference
    /// `reference` (`a:lnRef`) names in the theme's line styles.
    pub(super) fn line_ref(&self, reference: Node<'_, '_>) -> Option<(Rgb, Option<f32>)> {
        let index: usize = reference.attribute("idx")?.trim().parse().ok()?;
        let placeholder = self.color(reference, [0; 3]).unwrap_or_default();
        let styles = child(self.format?, DRAWING_NS, "lnStyleLst")?;
        let line = children(styles, DRAWING_NS, "ln").nth(index.checked_sub(1)?)?;
        let rgb = match self.fill(line, placeholder) {
            Some(Some(Paint::Solid(rgb) | Paint::Clear(rgb, _) | Paint::Gradient(rgb, _))) => rgb,
            Some(_) => return None,
            None => placeholder,
        };
        Some((rgb, width_pt(line)))
    }

    /// The outline of a shape (`p:sp`, `p:cxnSp`) for drawing with
    /// `excel::view::shape_svg`: its geometry from the properties
    /// `geometry` (its own, or its placeholder's), its turn, flips and line
    /// from its own properties `own`, else from `geometry`, else its style's
    /// line, and its fill `fill` (a colour; a picture fill is drawn apart).
    pub(super) fn outline(
        &self,
        own: Option<Node<'_, '_>>,
        geometry: Option<Node<'_, '_>>,
        style: Option<Node<'_, '_>>,
        fill: Option<Rgb>,
    ) -> TextShape {
        let mut shape = TextShape {
            fill,
            ..TextShape::default()
        };
        if let Some(props) = geometry {
            outline(&mut shape, props);
        }
        if let Some(props) = own {
            let mut turned = TextShape::default();
            outline(&mut turned, props);
            shape.rotation_deg = turned.rotation_deg;
            shape.flip_h = turned.flip_h;
            shape.flip_v = turned.flip_v;
            shape.line_width_pt = turned.line_width_pt.or(shape.line_width_pt);
        }
        let said = [own, geometry]
            .into_iter()
            .flatten()
            .filter_map(|props| child(props, DRAWING_NS, "ln"))
            .find_map(|line| self.fill(line, [0; 3]));
        let from_style = style
            .and_then(|style| child(style, DRAWING_NS, "lnRef"))
            .and_then(|reference| self.line_ref(reference));
        shape.line = match said {
            Some(Some(paint)) => paint.rgb(),
            Some(None) => None,
            None => from_style.map(|(rgb, _)| rgb),
        };
        if shape.line_width_pt.is_none() {
            shape.line_width_pt = from_style.and_then(|(_, width)| width).or(Some(DEFAULT_LINE_PT));
        }
        shape
    }
}

impl Paint {
    /// Its colour: a gradient's average; `None` for a picture.
    pub(super) fn rgb(&self) -> Option<Rgb> {
        match self {
            Self::Solid(rgb) | Self::Clear(rgb, _) | Self::Gradient(rgb, _) => Some(*rgb),
            Self::Picture(_) => None,
        }
    }
}

/// A colour `rgb` of the opacity `opacity`: `None` when it is not seen.
fn seen(rgb: Rgb, opacity: f32) -> Option<Paint> {
    if opacity <= 0.0 {
        None
    } else if opacity >= 1.0 {
        Some(Paint::Solid(rgb))
    } else {
        Some(Paint::Clear(rgb, opacity))
    }
}

/// The opacity of the colour of the fill `fill` (its colour element's
/// `a:alpha`); 1 when it says none.
fn opacity(fill: Node<'_, '_>) -> f32 {
    fill.children()
        .filter(Node::is_element)
        .find_map(|element| child(element, DRAWING_NS, "alpha"))
        .and_then(|alpha| alpha.attribute("val"))
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map_or(1.0, |value| (value / SHARE_UNITS).clamp(0.0, 1.0))
}

/// The width of the line `line` (`a:ln w`), in points.
pub(super) fn width_pt(line: Node<'_, '_>) -> Option<f32> {
    line.attribute("w")
        .and_then(|width| width.trim().parse::<f32>().ok())
        .map(|emu| emu / EMU_PER_PT)
        .filter(|width| width.is_finite() && *width >= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::docx::parse;

    #[test]
    fn theme_fonts_resolve() {
        let xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme name="x"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface="Yu Gothic"/><a:cs typeface=""/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;
        let doc = parse(xml).expect("xml");
        let fonts = Fonts::read(Some(doc.root_element()));
        assert_eq!(fonts.resolve("+mj-lt").as_deref(), Some("Calibri Light"));
        assert_eq!(fonts.resolve("+mn-ea").as_deref(), Some("Yu Gothic"));
        assert_eq!(fonts.resolve("+mj-ea"), None);
        assert_eq!(fonts.resolve("Arial").as_deref(), Some("Arial"));
    }

    #[test]
    fn colour_map_maps_tx1_to_lt1() {
        let xml = r#"<p:clrMap xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" bg1="dk1" tx1="lt1" bg2="dk2" tx2="lt2"/>"#;
        let doc = parse(xml).expect("xml");
        let theme = Theme::default().with_map(doc.root_element());
        assert_eq!(theme.scheme("tx1"), Some(theme.light1));
        assert_eq!(theme.scheme("bg1"), Some(theme.dark1));
        assert_eq!(theme.scheme("dk1"), Some(theme.dark1));
    }
}
