//! Formatting properties as a `.docx` writes them (`w:rPr`, `w:pPr`,
//! `w:tblPr`, `w:tcPr`), each one unset until a level of the style
//! hierarchy sets it, and their merging into resolved properties.

use super::model::{
    Align, Border, BorderStyle, LineSpacing, ParaBorders, ParaProps, Rgb, RunFmt, TabKind, TabStop, Underline, VAlign,
    VertAlign, Width,
};
use crate::word::xml::{WORD_NS, attr, child, children, toggle, val};
use roxmltree::Node;

/// Twips (twentieths of a point) per point.
pub(crate) const TWIPS_PER_PT: f32 = 20.0;
/// Word font sizes are in half points.
const HALF_POINTS_PER_PT: f32 = 2.0;
/// Border widths are in eighths of a point.
const EIGHTHS_PER_PT: f32 = 8.0;
/// Auto line spacing is in 240ths of a line.
const LINE_UNITS: f32 = 240.0;
/// Percentages of tables are in fiftieths of a percent.
const PCT_UNITS: f32 = 5000.0;
/// Spacing in lines (`w:beforeLines`) is in hundredths of a line, of this
/// height in points.
const LINE_PITCH_PT: f32 = 12.0;

/// The fonts of the document theme (`theme1.xml`).
#[derive(Debug, Clone, Default)]
pub(crate) struct ThemeFonts {
    /// Latin font of headings.
    pub(crate) major: String,
    /// Latin font of body text.
    pub(crate) minor: String,
    /// East Asian font of headings.
    pub(crate) major_ea: String,
    /// East Asian font of body text.
    pub(crate) minor_ea: String,
    /// Complex-script font of headings.
    pub(crate) major_cs: String,
    /// Complex-script font of body text.
    pub(crate) minor_cs: String,
}

impl ThemeFonts {
    /// The font a theme reference (`minorHAnsi`, `majorEastAsia`) names.
    fn get(&self, name: &str) -> Option<&str> {
        let font = match name {
            "majorAscii" | "majorHAnsi" => &self.major,
            "minorAscii" | "minorHAnsi" => &self.minor,
            "majorEastAsia" => &self.major_ea,
            "minorEastAsia" => &self.minor_ea,
            "majorBidi" => &self.major_cs,
            "minorBidi" => &self.minor_cs,
            _ => return None,
        };
        (!font.is_empty()).then_some(font.as_str())
    }
}

/// Text properties, each `None` when not set at this level.
#[derive(Debug, Clone, Default)]
pub(crate) struct RawRun {
    pub(crate) font: Option<String>,
    pub(crate) font_hansi: Option<String>,
    pub(crate) font_ea: Option<String>,
    pub(crate) font_cs: Option<String>,
    pub(crate) size: Option<f32>,
    pub(crate) size_cs: Option<f32>,
    pub(crate) bold: Option<bool>,
    pub(crate) italic: Option<bool>,
    pub(crate) underline: Option<Option<Underline>>,
    pub(crate) underline_color: Option<Option<Rgb>>,
    pub(crate) strike: Option<bool>,
    pub(crate) dstrike: Option<bool>,
    pub(crate) color: Option<Option<Rgb>>,
    pub(crate) highlight: Option<Option<Rgb>>,
    pub(crate) shading: Option<Option<Rgb>>,
    pub(crate) vert: Option<VertAlign>,
    pub(crate) position: Option<f32>,
    pub(crate) caps: Option<bool>,
    pub(crate) small_caps: Option<bool>,
    pub(crate) spacing: Option<f32>,
    pub(crate) scale: Option<f32>,
    pub(crate) kern: Option<Option<f32>>,
    pub(crate) hidden: Option<bool>,
    pub(crate) rtl: Option<bool>,
    pub(crate) cs: Option<bool>,
}

/// Overwrites each field of `$into` that `$over` sets.
macro_rules! overlay {
    ($into:expr, $over:expr, $($field:ident),*) => {
        $(if $over.$field.is_some() { $into.$field = $over.$field.clone(); })*
    };
}

impl RawRun {
    /// The properties set in a `w:rPr` element.
    pub(crate) fn parse(props: Node<'_, '_>, theme: &ThemeFonts) -> Self {
        let mut raw = RawRun::default();
        if let Some(fonts) = child(props, WORD_NS, "rFonts") {
            let pick = |name: &str, theme_name: &str| {
                attr(fonts, WORD_NS, theme_name)
                    .and_then(|theme_ref| theme.get(theme_ref))
                    .or_else(|| attr(fonts, WORD_NS, name))
                    .map(str::to_string)
            };
            raw.font = pick("ascii", "asciiTheme");
            raw.font_hansi = pick("hAnsi", "hAnsiTheme");
            raw.font_ea = pick("eastAsia", "eastAsiaTheme");
            raw.font_cs = pick("cs", "cstheme");
        }
        raw.size = half_points(props, "sz");
        raw.size_cs = half_points(props, "szCs");
        raw.bold = toggle(props, "b");
        raw.italic = toggle(props, "i");
        if let Some(u) = child(props, WORD_NS, "u") {
            raw.underline = Some(match attr(u, WORD_NS, "val").unwrap_or("single") {
                "none" => None,
                "words" => Some(Underline::Words),
                "double" => Some(Underline::Double),
                "thick" => Some(Underline::Thick),
                value if value.starts_with("dotted") || value.starts_with("dotDash") || value.starts_with("dotDot") => {
                    Some(Underline::Dotted)
                }
                value if value.starts_with("dash") => Some(Underline::Dashed),
                value if value.starts_with("wav") => Some(Underline::Wave),
                _ => Some(Underline::Single),
            });
            raw.underline_color = attr(u, WORD_NS, "color").map(parse_color);
        }
        raw.strike = toggle(props, "strike");
        raw.dstrike = toggle(props, "dstrike");
        raw.color = val(props, "color").map(parse_color);
        raw.highlight = val(props, "highlight").map(highlight_color);
        raw.shading = child(props, WORD_NS, "shd").map(shading);
        raw.vert = val(props, "vertAlign").map(|align| match align {
            "superscript" => VertAlign::Super,
            "subscript" => VertAlign::Sub,
            _ => VertAlign::Baseline,
        });
        raw.position = half_points(props, "position").or_else(|| {
            // A negative position is not caught by `half_points`.
            val(props, "position")
                .and_then(|value| value.parse::<f32>().ok())
                .map(|half| half / HALF_POINTS_PER_PT)
        });
        raw.caps = toggle(props, "caps");
        raw.small_caps = toggle(props, "smallCaps");
        raw.spacing = val(props, "spacing")
            .and_then(|value| value.parse::<f32>().ok())
            .map(|twips| twips / TWIPS_PER_PT);
        raw.scale = val(props, "w").and_then(|value| value.trim_end_matches('%').parse().ok());
        raw.kern = val(props, "kern")
            .and_then(|value| value.parse::<f32>().ok())
            .map(|half| (half > 0.0).then_some(half / HALF_POINTS_PER_PT));
        raw.hidden = toggle(props, "vanish").or_else(|| toggle(props, "specVanish"));
        raw.rtl = toggle(props, "rtl");
        raw.cs = toggle(props, "cs");
        raw
    }

    /// These properties with those set in `over` replacing them.
    pub(crate) fn overlay(&mut self, over: &RawRun) {
        overlay!(
            self,
            over,
            font,
            font_hansi,
            font_ea,
            font_cs,
            size,
            size_cs,
            bold,
            italic,
            underline,
            underline_color,
            strike,
            dstrike,
            color,
            highlight,
            shading,
            vert,
            position,
            caps,
            small_caps,
            spacing,
            scale,
            kern,
            hidden,
            rtl,
            cs
        );
    }

    /// The resolved properties: these over Word's defaults.
    pub(crate) fn resolve(&self) -> RunFmt {
        let mut fmt = RunFmt::default();
        let font = self
            .font
            .clone()
            .or_else(|| self.font_hansi.clone())
            .unwrap_or_else(|| fmt.font.clone());
        fmt.font_hansi = self.font_hansi.clone().unwrap_or_else(|| font.clone());
        fmt.font_east_asia = self.font_ea.clone().unwrap_or_else(|| font.clone());
        fmt.font_cs = self.font_cs.clone().unwrap_or_else(|| font.clone());
        fmt.font = font;
        fmt.size = self.size.unwrap_or(fmt.size);
        fmt.size_cs = self.size_cs.or(self.size).unwrap_or(fmt.size);
        fmt.bold = self.bold.unwrap_or(false);
        fmt.italic = self.italic.unwrap_or(false);
        fmt.underline = self.underline.flatten();
        fmt.underline_color = self.underline_color.flatten();
        fmt.strike = self.strike.unwrap_or(false);
        fmt.dstrike = self.dstrike.unwrap_or(false);
        fmt.color = self.color.flatten();
        fmt.highlight = self.highlight.flatten();
        fmt.shading = self.shading.flatten();
        fmt.vert = self.vert.unwrap_or_default();
        fmt.position = self.position.unwrap_or(0.0);
        fmt.caps = self.caps.unwrap_or(false);
        fmt.small_caps = self.small_caps.unwrap_or(false);
        fmt.spacing = self.spacing.unwrap_or(0.0);
        fmt.scale = self.scale.unwrap_or(100.0);
        fmt.kern_from = self.kern.flatten();
        fmt.hidden = self.hidden.unwrap_or(false);
        fmt.rtl = self.rtl.unwrap_or(false);
        fmt.cs = self.cs.unwrap_or(false);
        fmt
    }
}

/// A tab stop as a level sets it: `None` kind clears the stop of a lower
/// level at that position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RawTab {
    pos: f32,
    kind: Option<TabKind>,
    leader: Option<char>,
}

/// Paragraph properties, each `None` when not set at this level.
#[derive(Debug, Clone, Default)]
pub(crate) struct RawPara {
    pub(crate) align: Option<Align>,
    pub(crate) left: Option<f32>,
    pub(crate) right: Option<f32>,
    pub(crate) first_line: Option<f32>,
    pub(crate) before: Option<f32>,
    pub(crate) after: Option<f32>,
    pub(crate) before_auto: Option<bool>,
    pub(crate) after_auto: Option<bool>,
    pub(crate) line: Option<LineSpacing>,
    pub(crate) contextual: Option<bool>,
    pub(crate) keep_next: Option<bool>,
    pub(crate) keep_lines: Option<bool>,
    pub(crate) page_break_before: Option<bool>,
    pub(crate) widow_control: Option<bool>,
    pub(crate) tabs: Vec<RawTab>,
    pub(crate) border_top: Option<Option<Border>>,
    pub(crate) border_bottom: Option<Option<Border>>,
    pub(crate) border_left: Option<Option<Border>>,
    pub(crate) border_right: Option<Option<Border>>,
    pub(crate) border_between: Option<Option<Border>>,
    pub(crate) shading: Option<Option<Rgb>>,
    pub(crate) outline: Option<Option<u8>>,
    pub(crate) bidi: Option<bool>,
    pub(crate) snap_to_grid: Option<bool>,
    pub(crate) suppress_hyphens: Option<bool>,
    /// List numbering: (`numId`, level); a `numId` of "0" removes it.
    pub(crate) numbering: Option<(String, Option<u8>)>,
}

impl RawPara {
    /// The properties set in a `w:pPr` element.
    pub(crate) fn parse(props: Node<'_, '_>) -> Self {
        let mut raw = RawPara {
            align: val(props, "jc").map(parse_align),
            ..Default::default()
        };
        if let Some(ind) = child(props, WORD_NS, "ind") {
            let twips = |name: &str| {
                attr(ind, WORD_NS, name)
                    .and_then(|value| value.parse::<f32>().ok())
                    .map(|value| value / TWIPS_PER_PT)
            };
            raw.left = twips("left").or_else(|| twips("start"));
            raw.right = twips("right").or_else(|| twips("end"));
            raw.first_line = twips("hanging").map(|hanging| -hanging).or_else(|| twips("firstLine"));
        }
        if let Some(spacing) = child(props, WORD_NS, "spacing") {
            let number = |name: &str| attr(spacing, WORD_NS, name).and_then(|value| value.parse::<f32>().ok());
            raw.before = number("beforeLines")
                .filter(|lines| *lines != 0.0)
                .map(|lines| lines / 100.0 * LINE_PITCH_PT)
                .or_else(|| number("before").map(|twips| twips / TWIPS_PER_PT));
            raw.after = number("afterLines")
                .filter(|lines| *lines != 0.0)
                .map(|lines| lines / 100.0 * LINE_PITCH_PT)
                .or_else(|| number("after").map(|twips| twips / TWIPS_PER_PT));
            raw.before_auto = attr(spacing, WORD_NS, "beforeAutospacing").map(is_on);
            raw.after_auto = attr(spacing, WORD_NS, "afterAutospacing").map(is_on);
            if let Some(line) = number("line") {
                raw.line = Some(match attr(spacing, WORD_NS, "lineRule").unwrap_or("auto") {
                    "exact" => LineSpacing::Exact(line.abs() / TWIPS_PER_PT),
                    "atLeast" => LineSpacing::AtLeast(line / TWIPS_PER_PT),
                    _ => LineSpacing::Auto(line / LINE_UNITS),
                });
            }
        }
        raw.contextual = toggle(props, "contextualSpacing");
        raw.keep_next = toggle(props, "keepNext");
        raw.keep_lines = toggle(props, "keepLines");
        raw.page_break_before = toggle(props, "pageBreakBefore");
        raw.widow_control = toggle(props, "widowControl");
        if let Some(tabs) = child(props, WORD_NS, "tabs") {
            for tab in children(tabs, WORD_NS, "tab") {
                let Some(pos) = attr(tab, WORD_NS, "pos").and_then(|pos| pos.parse::<f32>().ok()) else {
                    continue;
                };
                let kind = match attr(tab, WORD_NS, "val").unwrap_or("left") {
                    "clear" => None,
                    "center" => Some(TabKind::Center),
                    "right" | "end" => Some(TabKind::Right),
                    "decimal" => Some(TabKind::Decimal),
                    "bar" => Some(TabKind::Bar),
                    _ => Some(TabKind::Left),
                };
                let leader = match attr(tab, WORD_NS, "leader") {
                    Some("dot") => Some('.'),
                    Some("hyphen") => Some('-'),
                    Some("underscore" | "heavy") => Some('_'),
                    Some("middleDot") => Some('·'),
                    _ => None,
                };
                raw.tabs.push(RawTab {
                    pos: pos / TWIPS_PER_PT,
                    kind,
                    leader,
                });
            }
        }
        if let Some(borders) = child(props, WORD_NS, "pBdr") {
            let side = |name: &str| child(borders, WORD_NS, name).map(parse_border);
            raw.border_top = side("top");
            raw.border_bottom = side("bottom");
            raw.border_left = side("left").or_else(|| side("start"));
            raw.border_right = side("right").or_else(|| side("end"));
            raw.border_between = side("between");
        }
        raw.shading = child(props, WORD_NS, "shd").map(shading);
        raw.outline = val(props, "outlineLvl")
            .and_then(|level| level.parse::<u8>().ok())
            .map(|level| (level < 9).then_some(level));
        raw.bidi = toggle(props, "bidi");
        raw.snap_to_grid = toggle(props, "snapToGrid");
        raw.suppress_hyphens = toggle(props, "suppressAutoHyphens");
        if let Some(num) = child(props, WORD_NS, "numPr") {
            let level = val(num, "ilvl").and_then(|level| level.parse().ok());
            if let Some(id) = val(num, "numId") {
                raw.numbering = Some((id.to_string(), level));
            } else if let Some(level) = level {
                raw.numbering = Some((String::new(), Some(level)));
            }
        }
        raw
    }

    /// These properties with those set in `over` replacing them; tab stops
    /// are added (a cleared stop removes the one at its position).
    pub(crate) fn overlay(&mut self, over: &RawPara) {
        overlay!(
            self,
            over,
            align,
            left,
            right,
            first_line,
            before,
            after,
            before_auto,
            after_auto,
            line,
            contextual,
            keep_next,
            keep_lines,
            page_break_before,
            widow_control,
            border_top,
            border_bottom,
            border_left,
            border_right,
            border_between,
            shading,
            outline,
            bidi,
            snap_to_grid,
            suppress_hyphens
        );
        for tab in &over.tabs {
            self.tabs.retain(|old| (old.pos - tab.pos).abs() > 0.01);
            self.tabs.push(*tab);
        }
        if let Some((id, level)) = &over.numbering {
            let (old_id, old_level) = self.numbering.take().unwrap_or_default();
            let id = if id.is_empty() { old_id } else { id.clone() };
            self.numbering = Some((id, level.or(old_level)));
        }
    }

    /// The resolved properties: these over Word's defaults.
    pub(crate) fn resolve(&self) -> ParaProps {
        let mut tabs: Vec<TabStop> = self
            .tabs
            .iter()
            .filter_map(|tab| {
                Some(TabStop {
                    pos: tab.pos,
                    kind: tab.kind?,
                    leader: tab.leader,
                })
            })
            .collect();
        tabs.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        ParaProps {
            align: self.align.unwrap_or_default(),
            left: self.left.unwrap_or(0.0),
            right: self.right.unwrap_or(0.0),
            first_line: self.first_line.unwrap_or(0.0),
            before: self.before.unwrap_or(0.0),
            after: self.after.unwrap_or(0.0),
            before_auto: self.before_auto.unwrap_or(false),
            after_auto: self.after_auto.unwrap_or(false),
            line: self.line.unwrap_or_default(),
            contextual: self.contextual.unwrap_or(false),
            keep_next: self.keep_next.unwrap_or(false),
            keep_lines: self.keep_lines.unwrap_or(false),
            page_break_before: self.page_break_before.unwrap_or(false),
            widow_control: self.widow_control.unwrap_or(true),
            tabs,
            borders: ParaBorders {
                top: self.border_top.flatten(),
                bottom: self.border_bottom.flatten(),
                left: self.border_left.flatten(),
                right: self.border_right.flatten(),
                between: self.border_between.flatten(),
            },
            shading: self.shading.flatten(),
            outline: self.outline.flatten(),
            bidi: self.bidi.unwrap_or(false),
            snap_to_grid: self.snap_to_grid.unwrap_or(true),
            suppress_hyphens: self.suppress_hyphens.unwrap_or(false),
        }
    }
}

/// Table properties, each `None` when not set at this level.
#[derive(Debug, Clone, Default)]
pub(crate) struct RawTable {
    pub(crate) width: Option<Width>,
    pub(crate) indent: Option<f32>,
    pub(crate) align: Option<Align>,
    pub(crate) fixed: Option<bool>,
    pub(crate) cell_margins: [Option<f32>; 4],
    pub(crate) borders: [Option<Option<Border>>; 6],
    pub(crate) cell_spacing: Option<f32>,
}

impl RawTable {
    /// The properties set in a `w:tblPr` element.
    pub(crate) fn parse(props: Node<'_, '_>) -> Self {
        let mut raw = RawTable {
            width: child(props, WORD_NS, "tblW").map(parse_width),
            indent: child(props, WORD_NS, "tblInd").map(|ind| match parse_width(ind) {
                Width::Pt(pt) => pt,
                _ => 0.0,
            }),
            align: val(props, "jc").map(parse_align),
            fixed: child(props, WORD_NS, "tblLayout").map(|layout| attr(layout, WORD_NS, "type") == Some("fixed")),
            cell_spacing: child(props, WORD_NS, "tblCellSpacing").and_then(|spacing| match parse_width(spacing) {
                Width::Pt(pt) => Some(pt),
                _ => None,
            }),
            ..Default::default()
        };
        if let Some(margins) = child(props, WORD_NS, "tblCellMar") {
            raw.cell_margins = parse_margins(margins);
        }
        if let Some(borders) = child(props, WORD_NS, "tblBorders") {
            let side = |names: &[&str]| {
                names
                    .iter()
                    .find_map(|name| child(borders, WORD_NS, name))
                    .map(parse_border)
            };
            raw.borders = [
                side(&["top"]),
                side(&["left", "start"]),
                side(&["bottom"]),
                side(&["right", "end"]),
                side(&["insideH"]),
                side(&["insideV"]),
            ];
        }
        raw
    }

    /// These properties with those set in `over` replacing them.
    pub(crate) fn overlay(&mut self, over: &RawTable) {
        overlay!(self, over, width, indent, align, fixed, cell_spacing);
        for (mine, theirs) in self.cell_margins.iter_mut().zip(over.cell_margins) {
            if theirs.is_some() {
                *mine = theirs;
            }
        }
        for (mine, theirs) in self.borders.iter_mut().zip(over.borders) {
            if theirs.is_some() {
                *mine = theirs;
            }
        }
    }
}

/// Cell properties, each `None` when not set at this level.
#[derive(Debug, Clone, Default)]
pub(crate) struct RawCell {
    pub(crate) width: Option<Width>,
    pub(crate) borders: [Option<Option<Border>>; 6],
    pub(crate) shading: Option<Option<Rgb>>,
    pub(crate) margins: [Option<f32>; 4],
    pub(crate) valign: Option<VAlign>,
}

impl RawCell {
    /// The properties set in a `w:tcPr` element.
    pub(crate) fn parse(props: Node<'_, '_>) -> Self {
        let mut raw = RawCell {
            width: child(props, WORD_NS, "tcW").map(parse_width),
            shading: child(props, WORD_NS, "shd").map(shading),
            valign: val(props, "vAlign").map(|align| match align {
                "center" => VAlign::Center,
                "bottom" => VAlign::Bottom,
                _ => VAlign::Top,
            }),
            ..Default::default()
        };
        if let Some(margins) = child(props, WORD_NS, "tcMar") {
            raw.margins = parse_margins(margins);
        }
        if let Some(borders) = child(props, WORD_NS, "tcBorders") {
            let side = |names: &[&str]| {
                names
                    .iter()
                    .find_map(|name| child(borders, WORD_NS, name))
                    .map(parse_border)
            };
            raw.borders = [
                side(&["top"]),
                side(&["left", "start"]),
                side(&["bottom"]),
                side(&["right", "end"]),
                side(&["insideH"]),
                side(&["insideV"]),
            ];
        }
        raw
    }

    /// These properties with those set in `over` replacing them.
    pub(crate) fn overlay(&mut self, over: &RawCell) {
        overlay!(self, over, width, shading, valign);
        for (mine, theirs) in self.margins.iter_mut().zip(over.margins) {
            if theirs.is_some() {
                *mine = theirs;
            }
        }
        for (mine, theirs) in self.borders.iter_mut().zip(over.borders) {
            if theirs.is_some() {
                *mine = theirs;
            }
        }
    }
}

/// Whether an on/off attribute value is on.
pub(crate) fn is_on(value: &str) -> bool {
    !matches!(value, "0" | "false" | "off")
}

/// A `w:jc` value.
fn parse_align(value: &str) -> Align {
    match value {
        "center" => Align::Center,
        "right" | "end" => Align::Right,
        "both" | "lowKashida" | "mediumKashida" | "highKashida" | "thaiDistribute" => Align::Both,
        "distribute" => Align::Distribute,
        _ => Align::Left,
    }
}

/// The value in half points of the property `name`, in points.
fn half_points(props: Node<'_, '_>, name: &str) -> Option<f32> {
    val(props, name)
        .and_then(|value| value.parse::<f32>().ok())
        .map(|half| half / HALF_POINTS_PER_PT)
        .filter(|size| *size > 0.0)
}

/// A colour (`RRGGBB`); `None` for `auto`.
pub(crate) fn parse_color(value: &str) -> Option<Rgb> {
    if value.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&value[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The colour of a highlight name.
fn highlight_color(name: &str) -> Option<Rgb> {
    Some(match name {
        "yellow" => [255, 255, 0],
        "green" => [0, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "blue" => [0, 0, 255],
        "red" => [255, 0, 0],
        "darkBlue" => [0, 0, 128],
        "darkCyan" => [0, 128, 128],
        "darkGreen" => [0, 128, 0],
        "darkMagenta" => [128, 0, 128],
        "darkRed" => [128, 0, 0],
        "darkYellow" => [128, 128, 0],
        "darkGray" => [128, 128, 128],
        "lightGray" => [192, 192, 192],
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        _ => return None,
    })
}

/// The background colour of a `w:shd`; `None` for none.
pub(crate) fn shading(node: Node<'_, '_>) -> Option<Rgb> {
    let pattern = attr(node, WORD_NS, "val").unwrap_or("clear");
    let fill = attr(node, WORD_NS, "fill").and_then(parse_color);
    let color = attr(node, WORD_NS, "color").and_then(parse_color);
    match pattern {
        "nil" => None,
        "solid" => color.or(Some([0, 0, 0])),
        "clear" => fill,
        pct if pct.starts_with("pct") => {
            // A pattern of `color` dots over `fill`: their mix.
            let share = pct[3..].parse::<f32>().unwrap_or(0.0) / 100.0;
            let back = fill.unwrap_or([255, 255, 255]);
            let front = color.unwrap_or([0, 0, 0]);
            Some(std::array::from_fn(|i| {
                (f32::from(back[i]) * (1.0 - share) + f32::from(front[i]) * share).round() as u8
            }))
        }
        _ => fill,
    }
}

/// A border element (`w:top`, `w:insideH`); `None` for no border.
pub(crate) fn parse_border(node: Node<'_, '_>) -> Option<Border> {
    let style = match attr(node, WORD_NS, "val").unwrap_or("single") {
        "nil" | "none" => return None,
        "double" | "triple" | "thinThickSmallGap" | "thickThinSmallGap" | "thinThickMediumGap"
        | "thickThinMediumGap" | "thinThickLargeGap" | "thickThinLargeGap" => BorderStyle::Double,
        "dotted" => BorderStyle::Dotted,
        value if value.starts_with("dash") || value.starts_with("dotDash") => BorderStyle::Dashed,
        _ => BorderStyle::Single,
    };
    let number = |name: &str| attr(node, WORD_NS, name).and_then(|value| value.parse::<f32>().ok());
    let width = number("sz").map_or(0.5, |eighths| (eighths / EIGHTHS_PER_PT).max(0.25));
    Some(Border {
        width,
        space: number("space").unwrap_or(0.0),
        color: attr(node, WORD_NS, "color").and_then(parse_color).unwrap_or([0, 0, 0]),
        style,
    })
}

/// A width element (`w:tblW`, `w:tcW`, `w:tblInd`).
pub(crate) fn parse_width(node: Node<'_, '_>) -> Width {
    let value = attr(node, WORD_NS, "w").unwrap_or("0");
    match attr(node, WORD_NS, "type").unwrap_or("dxa") {
        "auto" | "nil" => Width::Auto,
        "pct" => {
            if let Some(percent) = value.strip_suffix('%') {
                Width::Pct(percent.parse::<f32>().unwrap_or(0.0) / 100.0)
            } else {
                Width::Pct(value.parse::<f32>().unwrap_or(0.0) / PCT_UNITS)
            }
        }
        _ => {
            let twips = value.parse::<f32>().unwrap_or(0.0);
            if twips == 0.0 {
                Width::Auto
            } else {
                Width::Pt(twips / TWIPS_PER_PT)
            }
        }
    }
}

/// Cell margins (`w:tblCellMar`, `w:tcMar`): left, top, right, bottom.
fn parse_margins(node: Node<'_, '_>) -> [Option<f32>; 4] {
    let side = |names: &[&str]| {
        names.iter().find_map(|name| {
            child(node, WORD_NS, name).map(|side| match parse_width(side) {
                Width::Pt(pt) => pt,
                _ => 0.0,
            })
        })
    };
    [
        side(&["left", "start"]),
        side(&["top"]),
        side(&["right", "end"]),
        side(&["bottom"]),
    ]
}

/// The theme fonts in `theme1.xml`.
pub(crate) fn theme_fonts(root: Node<'_, '_>) -> ThemeFonts {
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let mut fonts = ThemeFonts::default();
    let Some(scheme) = root
        .descendants()
        .find(|node| crate::word::xml::is(*node, A, "fontScheme"))
    else {
        return fonts;
    };
    let typeface = |group: &str, kind: &str| {
        child(scheme, A, group)
            .and_then(|group| child(group, A, kind))
            .and_then(|font| font.attribute("typeface"))
            .unwrap_or_default()
            .to_string()
    };
    fonts.major = typeface("majorFont", "latin");
    fonts.minor = typeface("minorFont", "latin");
    fonts.major_ea = typeface("majorFont", "ea");
    fonts.minor_ea = typeface("minorFont", "ea");
    fonts.major_cs = typeface("majorFont", "cs");
    fonts.minor_cs = typeface("minorFont", "cs");
    fonts
}
