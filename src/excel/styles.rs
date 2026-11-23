//! The cell styles of a workbook (`xl/styles.xml`): for each cell format
//! (`cellXfs`), the number format, whether its font is bold or italic, and
//! whether it centres across the selection (`centerContinuous`); and its
//! look (font, fill, borders, alignment), which the sheet view draws.

use crate::word::docx::parse;
use anyhow::{Context, Result};
use roxmltree::Node;
use std::collections::HashMap;

use super::numfmt::{NumFmt, builtin};
use super::workbook::is_sml;
use crate::chart::{Rgb, Theme, with_luminance};

/// The cell formats of a workbook, by the index a cell's `s` gives.
#[derive(Debug, Clone, Default)]
pub(crate) struct Styles {
    /// The cell formats (`cellXfs`), in order.
    xfs: Vec<Xf>,
    /// The look of each cell format, in the same order.
    looks: Vec<Look>,
    /// The colours of the legacy palette (`indexedColors`), when the file
    /// changes them; Excel's own otherwise.
    pub(crate) palette: Option<Vec<Rgb>>,
}

/// A colour of the styles part, resolved with the workbook's theme and
/// palette when it is drawn ([`Color::rgb`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Color {
    /// A colour given as RGB.
    Rgb(Rgb),
    /// A colour of the theme, in Excel's order (light 1, dark 1, light 2,
    /// dark 2, the six accents, the two hyperlink colours), lightened
    /// (tint above 0) or darkened (below 0) by its tint, -1 to 1.
    Theme(u8, f32),
    /// A colour of the legacy palette (`indexed`).
    Indexed(u8),
}

/// How a cell looks: its font, fill, borders and alignment.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Look {
    /// The font.
    pub(crate) font: Font,
    /// The colour of the fill; `None` for no fill.
    pub(crate) fill: Option<Color>,
    /// The borders: left, right, top, bottom.
    pub(crate) borders: [Option<Edge>; 4],
    /// The horizontal alignment.
    pub(crate) h_align: HAlign,
    /// The vertical alignment.
    pub(crate) v_align: VAlign,
    /// The text wraps in the cell.
    pub(crate) wrap: bool,
    /// The indent, in steps of three spaces' width.
    pub(crate) indent: u8,
    /// The text is turned by this many degrees counter-clockwise (90 is
    /// upwards, -90 downwards); `None` when it is not turned.
    pub(crate) rotation: Option<i16>,
    /// The letters are stacked one under the other (`textRotation` 255).
    pub(crate) stacked: bool,
}

/// The font of a cell.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Font {
    /// The family; `None` for the application's default.
    pub(crate) name: Option<String>,
    /// The size in points; `None` for the default.
    pub(crate) size_pt: Option<f32>,
    /// The colour; `None` for automatic (black).
    pub(crate) color: Option<Color>,
    /// Bold.
    pub(crate) bold: bool,
    /// Italic.
    pub(crate) italic: bool,
    /// Underlined.
    pub(crate) underline: bool,
    /// Struck through.
    pub(crate) strike: bool,
}

/// One border of a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Edge {
    /// The line.
    pub(crate) line: Line,
    /// Its width in pixels at 100 % zoom.
    pub(crate) width_px: f32,
    /// Its colour; `None` for automatic (black).
    pub(crate) color: Option<Color>,
}

/// How a border is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line {
    /// A solid line.
    Solid,
    /// A dashed line.
    Dashed,
    /// A dotted line.
    Dotted,
    /// Two thin lines.
    Double,
}

/// The horizontal alignment of a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum HAlign {
    /// By the kind of value: numbers and dates right, text left, logical
    /// and error values centred.
    #[default]
    General,
    /// Left.
    Left,
    /// Centred, also across the selection.
    Center,
    /// Right.
    Right,
    /// Justified (also `distributed` and `fill`).
    Justify,
}

/// The vertical alignment of a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum VAlign {
    /// At the top.
    Top,
    /// In the middle.
    Middle,
    /// At the bottom, Excel's default.
    #[default]
    Bottom,
}

/// The colours of Excel's legacy palette (`indexed` 0 to 63).
const PALETTE: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000, 0xFFFFFF, 0xFF0000,
    0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080,
    0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080,
    0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99,
    0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699,
    0x969696, 0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333,
];
/// The palette index of the system's text colour.
const SYSTEM_FOREGROUND: u8 = 64;

impl Color {
    /// The colour as RGB: a theme colour of `theme` with its tint, a
    /// palette colour of `palette` (Excel's when `None`); `None` for the
    /// system's background colour or an index past the palette.
    pub(crate) fn rgb(self, theme: &Theme, palette: Option<&[Rgb]>) -> Option<Rgb> {
        match self {
            Color::Rgb(rgb) => Some(rgb),
            Color::Theme(index, tint) => {
                let base = match index {
                    0 => theme.light1,
                    1 => theme.dark1,
                    2 => theme.light2,
                    3 => theme.dark2,
                    4..=9 => theme.accents[usize::from(index - 4)],
                    10 => theme.hyperlink,
                    11 => theme.followed_hyperlink,
                    _ => return None,
                };
                Some(tinted(base, tint))
            }
            Color::Indexed(SYSTEM_FOREGROUND) => Some([0; 3]),
            Color::Indexed(index) => {
                if let Some(rgb) = palette.and_then(|palette| palette.get(usize::from(index))) {
                    return Some(*rgb);
                }
                let value = *PALETTE.get(usize::from(index))?;
                Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
            }
        }
    }
}

/// `rgb` lightened (`tint` above 0) or darkened (below 0) as Excel does:
/// its luminance moved that share of the way to white or black.
fn tinted(rgb: Rgb, tint: f32) -> Rgb {
    if tint == 0.0 || !tint.is_finite() {
        return rgb;
    }
    let tint = tint.clamp(-1.0, 1.0);
    with_luminance(rgb, |lum| {
        if tint < 0.0 {
            lum * (1.0 + tint)
        } else {
            lum * (1.0 - tint) + tint
        }
    })
}

/// A cell format: what of it shows in the text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Xf {
    /// The number format.
    pub(crate) num_fmt: NumFmt,
    /// The font is bold.
    pub(crate) bold: bool,
    /// The font is italic.
    pub(crate) italic: bool,
    /// The text is centred across the selection (`centerContinuous`): over
    /// the empty cells to its right that have the same alignment.
    pub(crate) center_continuous: bool,
}

/// The format of a cell without a style.
static DEFAULT_XF: Xf = Xf {
    num_fmt: NumFmt::General,
    bold: false,
    italic: false,
    center_continuous: false,
};

/// The look of a cell without a style.
static DEFAULT_LOOK: Look = Look {
    font: Font {
        name: None,
        size_pt: None,
        color: None,
        bold: false,
        italic: false,
        underline: false,
        strike: false,
    },
    fill: None,
    borders: [None; 4],
    h_align: HAlign::General,
    v_align: VAlign::Bottom,
    wrap: false,
    indent: 0,
    rotation: None,
    stacked: false,
};

impl Styles {
    /// The cell format of index `index` (a cell's `s`); the default one
    /// when there is none.
    pub(crate) fn get(&self, index: usize) -> &Xf {
        self.xfs.get(index).unwrap_or(&DEFAULT_XF)
    }

    /// The look of the cell format of index `index`; the default one when
    /// there is none.
    pub(crate) fn look(&self, index: usize) -> &Look {
        self.looks.get(index).unwrap_or(&DEFAULT_LOOK)
    }
}

/// The cell formats of the styles part `xml`.
///
/// # Errors
/// When `xml` is not well-formed.
pub(crate) fn read(xml: &str) -> Result<Styles> {
    let doc = parse(xml).context("reading the styles")?;
    let root = doc.root_element();
    let child = |name: &str| root.children().find(|node| is_sml(*node, name));
    // Custom number formats, by id; they may also replace a built-in one.
    let custom: HashMap<u32, String> = items(child("numFmts"), "numFmt")
        .filter_map(|node| {
            Some((
                node.attribute("numFmtId")?.parse().ok()?,
                node.attribute("formatCode")?.to_string(),
            ))
        })
        .collect();
    let fonts: Vec<Font> = items(child("fonts"), "font").map(font).collect();
    let fills: Vec<Option<Color>> = items(child("fills"), "fill").map(fill).collect();
    let borders: Vec<[Option<Edge>; 4]> = items(child("borders"), "border").map(border).collect();
    let palette = child("colors")
        .and_then(|colors| colors.children().find(|node| is_sml(*node, "indexedColors")))
        .map(|list| {
            items(Some(list), "rgbColor")
                .map(|node| node.attribute("rgb").and_then(argb).unwrap_or([0; 3]))
                .collect()
        });
    let mut looks = Vec::new();
    let xfs = items(child("cellXfs"), "xf")
        .map(|xf| {
            let id = |name| {
                xf.attribute(name)
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0)
            };
            let num_fmt_id = id("numFmtId");
            let num_fmt = match custom.get(&num_fmt_id) {
                Some(code) if code.eq_ignore_ascii_case("general") => NumFmt::General,
                Some(code) => NumFmt::Code(code.clone()),
                None => builtin(num_fmt_id),
            };
            let pick = |name| usize::try_from(id(name)).ok();
            let font = pick("fontId")
                .and_then(|index| fonts.get(index))
                .cloned()
                .unwrap_or_default();
            let (bold, italic) = (font.bold, font.italic);
            let alignment = xf.children().find(|node| is_sml(*node, "alignment"));
            let center_continuous =
                alignment.is_some_and(|alignment| alignment.attribute("horizontal") == Some("centerContinuous"));
            looks.push(look(
                font,
                pick("fillId").and_then(|index| fills.get(index)).copied().flatten(),
                pick("borderId")
                    .and_then(|index| borders.get(index))
                    .copied()
                    .unwrap_or_default(),
                alignment,
            ));
            Xf {
                num_fmt,
                bold,
                italic,
                center_continuous,
            }
        })
        .collect();
    Ok(Styles { xfs, looks, palette })
}

/// The look of a cell format with `font`, `fill`, `borders` and the
/// `alignment` element.
fn look(font: Font, fill: Option<Color>, borders: [Option<Edge>; 4], alignment: Option<Node<'_, '_>>) -> Look {
    let attr = |name| alignment.and_then(|node| node.attribute(name));
    let h_align = match attr("horizontal") {
        Some("left") => HAlign::Left,
        Some("center" | "centerContinuous") => HAlign::Center,
        Some("right") => HAlign::Right,
        Some("justify" | "distributed" | "fill") => HAlign::Justify,
        _ => HAlign::General,
    };
    let v_align = match attr("vertical") {
        Some("top") => VAlign::Top,
        Some("center" | "justify" | "distributed") => VAlign::Middle,
        _ => VAlign::Bottom,
    };
    let rotation = attr("textRotation").and_then(|value| value.parse::<i16>().ok());
    Look {
        font,
        fill,
        borders,
        h_align,
        v_align,
        wrap: is_on(attr("wrapText")),
        indent: attr("indent")
            .and_then(|value| value.parse::<u8>().ok())
            .unwrap_or(0)
            .min(MAX_INDENT),
        // 1 to 90 turn upwards; 91 to 180 downwards, by the value less 90.
        rotation: match rotation {
            Some(degrees @ 1..=90) => Some(degrees),
            Some(degrees @ 91..=180) => Some(90 - degrees),
            _ => None,
        },
        stacked: rotation == Some(STACKED_ROTATION),
    }
}

/// The largest indent kept, in steps.
const MAX_INDENT: u8 = 15;
/// The `textRotation` of stacked letters.
const STACKED_ROTATION: i16 = 255;
/// Font sizes kept, in points: smaller or larger ones are clamped.
const FONT_SIZE_RANGE_PT: (f32, f32) = (1.0, 409.0);

/// Whether a boolean attribute is on (`1`, `true`).
fn is_on(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true" | "on"))
}

/// The colour of a colour element (`color`, `fgColor`, `bgColor`):
/// `rgb`, `theme` with its `tint`, or `indexed`; `None` for an automatic
/// one or none.
fn color(node: Node<'_, '_>) -> Option<Color> {
    if let Some(rgb) = node.attribute("rgb") {
        return argb(rgb).map(Color::Rgb);
    }
    if let Some(theme) = node.attribute("theme").and_then(|value| value.parse::<u8>().ok()) {
        let tint = node
            .attribute("tint")
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|tint| tint.is_finite())
            .unwrap_or(0.0);
        return Some(Color::Theme(theme, tint));
    }
    node.attribute("indexed")
        .and_then(|value| value.parse::<u8>().ok())
        .map(Color::Indexed)
}

/// The RGB of an `AARRGGBB` (or `RRGGBB`) value.
fn argb(value: &str) -> Option<Rgb> {
    let value = value.trim();
    let hex = if value.len() == 8 { value.get(2..)? } else { value };
    let number = u32::from_str_radix(hex, 16).ok().filter(|_| hex.len() == 6)?;
    Some([(number >> 16) as u8, (number >> 8) as u8, number as u8])
}

/// The child element `name` of `node`.
fn child_of<'a, 'i>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|child| is_sml(*child, name))
}

/// A font (`font`).
fn font(node: Node<'_, '_>) -> Font {
    let on =
        |name| child_of(node, name).is_some_and(|node| !matches!(node.attribute("val"), Some("0" | "false" | "off")));
    let value = |name| child_of(node, name).and_then(|node| node.attribute("val"));
    Font {
        name: value("name").map(str::to_string).filter(|name| !name.trim().is_empty()),
        size_pt: value("sz")
            .and_then(|size| size.parse::<f32>().ok())
            .filter(|size| size.is_finite())
            .map(|size| size.clamp(FONT_SIZE_RANGE_PT.0, FONT_SIZE_RANGE_PT.1)),
        color: child_of(node, "color").and_then(color),
        bold: on("b"),
        italic: on("i"),
        underline: child_of(node, "u").is_some_and(|node| node.attribute("val") != Some("none")),
        strike: on("strike"),
    }
}

/// The colour of a fill (`fill`): a solid or shaded pattern's colour, or
/// a gradient's first stop; `None` for no fill or the sparse grey
/// patterns.
fn fill(node: Node<'_, '_>) -> Option<Color> {
    if let Some(pattern) = child_of(node, "patternFill") {
        let kind = pattern.attribute("patternType").unwrap_or("none");
        if matches!(kind, "none" | "gray125" | "gray0625") {
            return None;
        }
        let fg = child_of(pattern, "fgColor").and_then(color);
        return fg.or_else(|| child_of(pattern, "bgColor").and_then(color));
    }
    let gradient = child_of(node, "gradientFill")?;
    let stop = child_of(gradient, "stop")?;
    child_of(stop, "color").and_then(color)
}

/// The four borders (left, right, top, bottom) of a `border`; `start` and
/// `end` stand for left and right.
fn border(node: Node<'_, '_>) -> [Option<Edge>; 4] {
    let edge = |names: &[&str]| names.iter().find_map(|name| child_of(node, name)).and_then(edge);
    [
        edge(&["left", "start"]),
        edge(&["right", "end"]),
        edge(&["top"]),
        edge(&["bottom"]),
    ]
}

/// One border: its style (`style`) and colour; `None` for no line.
fn edge(node: Node<'_, '_>) -> Option<Edge> {
    let (line, width_px) = match node.attribute("style")? {
        "thin" => (Line::Solid, 1.0),
        "hair" => (Line::Dotted, 1.0),
        "dotted" => (Line::Dotted, 1.0),
        "dashed" | "dashDot" | "dashDotDot" => (Line::Dashed, 1.0),
        "medium" => (Line::Solid, 2.0),
        "mediumDashed" | "mediumDashDot" | "mediumDashDotDot" | "slantDashDot" => (Line::Dashed, 2.0),
        "thick" => (Line::Solid, 3.0),
        "double" => (Line::Double, 3.0),
        _ => return None,
    };
    Some(Edge {
        line,
        width_px,
        color: child_of(node, "color").and_then(color),
    })
}

/// The child elements `name` of `list`, none when there is no `list`.
fn items<'a, 'input>(list: Option<Node<'a, 'input>>, name: &'static str) -> impl Iterator<Item = Node<'a, 'input>> {
    list.into_iter()
        .flat_map(|list| list.children())
        .filter(move |node| is_sml(*node, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `body` as a styles part.
    fn styles(body: &str) -> Styles {
        read(&format!(
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{body}</styleSheet>"#
        ))
        .unwrap()
    }

    #[test]
    fn cell_formats_have_their_number_format_and_font() {
        let styles = styles(concat!(
            r#"<numFmts count="2"><numFmt numFmtId="164" formatCode="&quot;¥&quot;#,##0"/><numFmt numFmtId="3" formatCode="0.0"/></numFmts>"#,
            r#"<fonts count="3"><font><sz val="11"/></font><font><b/><i val="1"/></font><font><b val="0"/><i/></font></fonts>"#,
            r#"<cellXfs count="5"><xf numFmtId="0" fontId="0"/><xf numFmtId="164" fontId="1"/><xf numFmtId="14" fontId="2"/>"#,
            r#"<xf numFmtId="3" fontId="9"><alignment horizontal="centerContinuous"/></xf><xf numFmtId="7"><alignment horizontal="center"/></xf></cellXfs>"#,
        ));
        assert_eq!(*styles.get(0), Xf::default());
        let xf = styles.get(1);
        assert_eq!(xf.num_fmt, NumFmt::Code("\"¥\"#,##0".into()));
        assert!(xf.bold && xf.italic && !xf.center_continuous);
        let xf = styles.get(2);
        assert_eq!(xf.num_fmt, builtin(14));
        assert!(!xf.bold && xf.italic);
        // A custom code replaces the built-in one of its id; a missing font
        // is the default one.
        let xf = styles.get(3);
        assert_eq!(xf.num_fmt, NumFmt::Code("0.0".into()));
        assert!(xf.center_continuous && !xf.bold);
        assert!(!styles.get(4).center_continuous);
        assert_eq!(styles.get(4).num_fmt, builtin(7));
    }

    #[test]
    fn an_index_out_of_range_is_the_default_format() {
        let styles = styles(r#"<cellXfs count="1"><xf numFmtId="10"/></cellXfs>"#);
        assert_eq!(styles.get(0).num_fmt, builtin(10));
        assert_eq!(*styles.get(7), Xf::default());
        assert_eq!(*Styles::default().get(0), Xf::default());
    }

    #[test]
    fn strict_namespace_is_read() {
        let xml = r#"<styleSheet xmlns="http://purl.oclc.org/ooxml/spreadsheetml/main"><fonts><font><b/></font></fonts><cellXfs><xf numFmtId="9" fontId="0"/></cellXfs></styleSheet>"#;
        let styles = read(xml).unwrap();
        assert!(styles.get(0).bold);
        assert_eq!(styles.get(0).num_fmt, builtin(9));
    }

    #[test]
    fn cell_formats_have_their_look() {
        let styles = styles(concat!(
            r#"<colors><indexedColors><rgbColor rgb="FF112233"/></indexedColors></colors>"#,
            r#"<fonts><font><sz val="11"/><name val="Calibri"/></font><font><u/><strike/><sz val="9"/><color theme="1"/><name val="ＭＳ ゴシック"/></font></fonts>"#,
            r#"<fills><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor theme="4" tint="0.5"/></patternFill></fill><fill><gradientFill><stop position="0"><color indexed="0"/></stop></gradientFill></fill></fills>"#,
            r#"<borders><border/><border><start style="medium"><color rgb="FF00FF00"/></start><top style="double"/><bottom style="hair"/><right style="none"/></border></borders>"#,
            r#"<cellXfs><xf/><xf fontId="1" fillId="2" borderId="1"><alignment horizontal="right" vertical="center" wrapText="1" indent="2" textRotation="135"/></xf><xf fillId="1"/><xf fillId="3"><alignment textRotation="255"/></xf></cellXfs>"#,
        ));
        assert_eq!(
            *styles.look(0),
            Look {
                font: Font {
                    name: Some("Calibri".into()),
                    size_pt: Some(11.0),
                    ..Font::default()
                },
                ..Look::default()
            }
        );
        let look = styles.look(1);
        assert_eq!(look.font.name.as_deref(), Some("ＭＳ ゴシック"));
        assert!(look.font.underline && look.font.strike && !look.font.bold);
        assert_eq!(look.font.color, Some(Color::Theme(1, 0.0)));
        assert_eq!(look.fill, Some(Color::Theme(4, 0.5)));
        assert_eq!(
            look.borders[0].map(|edge| (edge.line, edge.width_px)),
            Some((Line::Solid, 2.0))
        );
        assert_eq!(look.borders[1], None);
        assert_eq!(look.borders[2].map(|edge| edge.line), Some(Line::Double));
        assert_eq!(look.borders[3].map(|edge| edge.line), Some(Line::Dotted));
        assert_eq!(
            (look.h_align, look.v_align, look.wrap, look.indent),
            (HAlign::Right, VAlign::Middle, true, 2)
        );
        // 135 turns downwards by 45 degrees.
        assert_eq!((look.rotation, look.stacked), (Some(-45), false));
        // The sparse grey pattern is no fill; a gradient is its first stop.
        assert_eq!(styles.look(2).fill, None);
        assert_eq!(styles.look(3).fill, Some(Color::Indexed(0)));
        assert!(styles.look(3).stacked);
        // Colours resolved with the theme and the file's palette.
        let theme = Theme::default();
        assert_eq!(Color::Theme(1, 0.0).rgb(&theme, None), Some([0, 0, 0]));
        assert_eq!(Color::Theme(4, 0.5).rgb(&theme, None), Some([0xA2, 0xB9, 0xE2]));
        assert_eq!(Color::Theme(4, -0.5).rgb(&theme, None), Some([0x20, 0x38, 0x64]));
        assert_eq!(
            Color::Indexed(0).rgb(&theme, styles.palette.as_deref()),
            Some([0x11, 0x22, 0x33])
        );
        assert_eq!(Color::Indexed(10).rgb(&theme, None), Some([255, 0, 0]));
        assert_eq!(Color::Indexed(64).rgb(&theme, None), Some([0, 0, 0]));
        assert_eq!(Color::Indexed(65).rgb(&theme, None), None);
    }

    #[test]
    fn damaged_styles_are_an_error() {
        assert!(read("<styleSheet").is_err());
    }
}
