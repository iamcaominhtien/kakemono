//! The colour scheme of a theme part (`a:clrScheme` of
//! `xl/theme/theme1.xml` or `word/theme/theme1.xml`), and DrawingML colours
//! resolved with it.

use super::Rgb;
use crate::word::docx::parse;
use crate::word::xml::{DRAWING_NS, is};
use roxmltree::Node;

/// What a DrawingML percentage is written in: 100 000 is 100 %.
const PERCENT_UNIT: f32 = 100_000.0;
/// The luminance changes Office gives the accents on each round of six
/// series after the first (`cs:variation` of its default chart colours):
/// the luminance multiplied by the first number, then the second added.
const VARIATIONS: [(f32, f32); 9] = [
    (1.0, 0.0),
    (0.6, 0.0),
    (0.8, 0.2),
    (0.8, 0.0),
    (0.6, 0.4),
    (0.5, 0.0),
    (0.7, 0.3),
    (0.7, 0.0),
    (0.5, 0.5),
];

/// The colour scheme of a document's theme: what `a:schemeClr` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Theme {
    /// Dark 1 (`dk1`, the text colour `tx1`).
    pub(crate) dark1: Rgb,
    /// Light 1 (`lt1`, the background `bg1`).
    pub(crate) light1: Rgb,
    /// Dark 2 (`dk2`, `tx2`).
    pub(crate) dark2: Rgb,
    /// Light 2 (`lt2`, `bg2`).
    pub(crate) light2: Rgb,
    /// Accent 1 to 6: the colours of the series, in order.
    pub(crate) accents: [Rgb; 6],
    /// Hyperlinks.
    pub(crate) hyperlink: Rgb,
    /// Followed hyperlinks.
    pub(crate) followed_hyperlink: Rgb,
    /// The slots that `tx1`, `bg1`, `tx2` and `bg2` name, as indices into
    /// dark 1, light 1, dark 2, light 2: a slide master's colour map
    /// (`p:clrMap`), which a dark deck turns round.
    pub(crate) map: [u8; 4],
}

impl Default for Theme {
    /// The Office theme of Office 2013 to 2021.
    fn default() -> Self {
        Self {
            dark1: [0, 0, 0],
            light1: [255, 255, 255],
            dark2: [0x44, 0x54, 0x6A],
            light2: [0xE7, 0xE6, 0xE6],
            accents: [
                [0x44, 0x72, 0xC4],
                [0xED, 0x7D, 0x31],
                [0xA5, 0xA5, 0xA5],
                [0xFF, 0xC0, 0x00],
                [0x5B, 0x9B, 0xD5],
                [0x70, 0xAD, 0x47],
            ],
            hyperlink: [0x05, 0x63, 0xC1],
            followed_hyperlink: [0x95, 0x4F, 0x72],
            map: [0, 1, 2, 3],
        }
    }
}

impl Theme {
    /// The colour scheme of the theme part `xml`; the Office theme when
    /// there is no part, and its colour for each one the part lacks or
    /// damages.
    pub(crate) fn read(xml: Option<&[u8]>) -> Self {
        let mut theme = Self::default();
        let Some(text) = xml.and_then(|bytes| std::str::from_utf8(bytes).ok()) else {
            return theme;
        };
        let Ok(doc) = parse(text.trim_start_matches('\u{feff}')) else {
            return theme;
        };
        let Some(scheme) = doc.descendants().find(|node| is(*node, DRAWING_NS, "clrScheme")) else {
            return theme;
        };
        for slot in scheme
            .children()
            .filter(|node| node.tag_name().namespace() == Some(DRAWING_NS))
        {
            let Some(rgb) = color(slot, &theme, [0; 3]) else {
                continue;
            };
            let target = match slot.tag_name().name() {
                "dk1" => &mut theme.dark1,
                "lt1" => &mut theme.light1,
                "dk2" => &mut theme.dark2,
                "lt2" => &mut theme.light2,
                "hlink" => &mut theme.hyperlink,
                "folHlink" => &mut theme.followed_hyperlink,
                name => match accent_number(name) {
                    Some(number) => &mut theme.accents[number],
                    None => continue,
                },
            };
            *target = rgb;
        }
        theme
    }

    /// The colour the scheme names `name` (`accent1`, `tx1`, `bg2`, ...);
    /// `None` for a name it does not know (`phClr`).
    pub(crate) fn scheme(&self, name: &str) -> Option<Rgb> {
        let slots = [self.dark1, self.light1, self.dark2, self.light2];
        let mapped = |at: usize| slots[usize::from(self.map[at]) % slots.len()];
        Some(match name {
            "dk1" => self.dark1,
            "lt1" => self.light1,
            "dk2" => self.dark2,
            "lt2" => self.light2,
            "tx1" => mapped(0),
            "bg1" => mapped(1),
            "tx2" => mapped(2),
            "bg2" => mapped(3),
            "hlink" => self.hyperlink,
            "folHlink" => self.followed_hyperlink,
            other => self.accents[accent_number(other)?],
        })
    }

    /// The theme with the colour map `clr_map` (a `p:clrMap` or
    /// `a:overrideClrMapping`: `tx1="dk1" bg1="lt1" ...`); a name it does
    /// not map keeps its slot.
    pub(crate) fn with_map(mut self, clr_map: Node<'_, '_>) -> Self {
        for (at, name) in ["tx1", "bg1", "tx2", "bg2"].iter().enumerate() {
            let slot = match clr_map.attribute(*name) {
                Some("dk1") => 0,
                Some("lt1") => 1,
                Some("dk2") => 2,
                Some("lt2") => 3,
                _ => continue,
            };
            self.map[at] = slot;
        }
        self
    }

    /// The automatic colour of the series (or point) `index`, as Office
    /// gives it: the six accents in turn, darker or lighter on each later
    /// round.
    pub(crate) fn automatic(&self, index: usize) -> Rgb {
        let accent = self.accents[index % self.accents.len()];
        let (lum_mod, lum_off) = VARIATIONS[(index / self.accents.len()) % VARIATIONS.len()];
        if index < self.accents.len() {
            return accent;
        }
        with_luminance(accent, |lum| lum * lum_mod + lum_off)
    }
}

/// The colour of the DrawingML fill or line `node` (an `a:solidFill`, an
/// `a:ln`, a `c:spPr`, ...): its colour element with its transforms
/// (`lumMod`, `lumOff`, `tint`, `shade`), a scheme colour resolved with
/// `theme`, `phClr` as `placeholder`. `None` when it has no colour element.
pub(crate) fn color(node: Node<'_, '_>, theme: &Theme, placeholder: Rgb) -> Option<Rgb> {
    let element = node
        .children()
        .find(|item| item.is_element() && item.tag_name().namespace() == Some(DRAWING_NS))?;
    color_element(element, theme, placeholder)
}

/// The colour of the colour element `element` itself (`a:srgbClr`,
/// `a:schemeClr`, ...), as [`color`] reads it; `None` for another element.
pub(crate) fn color_element(element: Node<'_, '_>, theme: &Theme, placeholder: Rgb) -> Option<Rgb> {
    let base = match element.tag_name().name() {
        "srgbClr" => hex(element.attribute("val")?)?,
        "sysClr" => hex(element.attribute("lastClr")?)?,
        "schemeClr" => match element.attribute("val")? {
            "phClr" => placeholder,
            name => theme.scheme(name)?,
        },
        "prstClr" => preset(element.attribute("val")?)?,
        "scrgbClr" => {
            let channel = |name| percent(element, name).map(|value| linear_to_srgb(value.clamp(0.0, 1.0)));
            [channel("r")?, channel("g")?, channel("b")?]
        }
        _ => return None,
    };
    Some(transformed(base, element))
}

/// The number (0 to 5) of the accent `name` (`accent1` to `accent6`).
fn accent_number(name: &str) -> Option<usize> {
    let number: usize = name.strip_prefix("accent")?.parse().ok()?;
    (1..=6).contains(&number).then(|| number - 1)
}

/// A colour written `RRGGBB`.
fn hex(text: &str) -> Option<Rgb> {
    if text.len() != 6 || !text.is_ascii() {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&text[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The common preset colours (`a:prstClr`); the others are not known.
fn preset(name: &str) -> Option<Rgb> {
    Some(match name {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "cyan" => [0, 255, 255],
        "magenta" => [255, 0, 255],
        "gray" | "grey" => [128, 128, 128],
        "orange" => [255, 165, 0],
        _ => return None,
    })
}

/// The value of the attribute `name` of `node`, a DrawingML percentage
/// (`50000` is 50 %), as a ratio.
fn percent(node: Node<'_, '_>, name: &str) -> Option<f32> {
    let value: f32 = node.attribute(name)?.parse().ok()?;
    value.is_finite().then_some(value / PERCENT_UNIT)
}

/// `base` with the transforms of the colour element `element` applied in
/// order; the ones not known (alpha, hue, saturation) are left out.
fn transformed(base: Rgb, element: Node<'_, '_>) -> Rgb {
    element
        .children()
        .filter(|item| item.is_element() && item.tag_name().namespace() == Some(DRAWING_NS))
        .fold(base, |rgb, transform| {
            let Some(ratio) = percent(transform, "val") else {
                return rgb;
            };
            match transform.tag_name().name() {
                "lumMod" => with_luminance(rgb, |lum| lum * ratio),
                "lumOff" => with_luminance(rgb, |lum| lum + ratio),
                // Office darkens and lightens in linear light: a 20 % tint
                // of accent 1 is PowerPoint's pale table band (E9EBF5).
                "shade" => rgb.map(|channel| linear_to_srgb(srgb_to_linear(channel) * ratio.clamp(0.0, 1.0))),
                "tint" => rgb.map(|channel| {
                    let ratio = ratio.clamp(0.0, 1.0);
                    linear_to_srgb(srgb_to_linear(channel) * ratio + (1.0 - ratio))
                }),
                _ => rgb,
            }
        })
}

/// `rgb` with its HSL luminance changed by `change`, clamped to 0..1.
pub(crate) fn with_luminance(rgb: Rgb, change: impl Fn(f32) -> f32) -> Rgb {
    let [red, green, blue] = rgb.map(|channel| f32::from(channel) / 255.0);
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lum = (max + min) / 2.0;
    let delta = max - min;
    let (hue, sat) = if delta == 0.0 {
        (0.0, 0.0)
    } else {
        let sat = delta / (1.0 - (2.0 * lum - 1.0).abs());
        let hue = if max == red {
            ((green - blue) / delta).rem_euclid(6.0)
        } else if max == green {
            (blue - red) / delta + 2.0
        } else {
            (red - green) / delta + 4.0
        };
        (hue, sat)
    };
    let lum = change(lum);
    let lum = if lum.is_finite() { lum.clamp(0.0, 1.0) } else { 0.0 };
    let chroma = (1.0 - (2.0 * lum - 1.0).abs()) * sat;
    let second = chroma * (1.0 - (hue.rem_euclid(2.0) - 1.0).abs());
    let (r1, g1, b1) = match hue {
        sector if sector < 1.0 => (chroma, second, 0.0),
        sector if sector < 2.0 => (second, chroma, 0.0),
        sector if sector < 3.0 => (0.0, chroma, second),
        sector if sector < 4.0 => (0.0, second, chroma),
        sector if sector < 5.0 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = lum - chroma / 2.0;
    [r1, g1, b1].map(|channel| channel_of((channel + base) * 255.0))
}

/// A channel value rounded into 0..=255.
fn channel_of(value: f32) -> u8 {
    value.round().clamp(0.0, 255.0) as u8
}

/// An sRGB channel as linear light (0..1).
fn srgb_to_linear(channel: u8) -> f32 {
    let value = f32::from(channel) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

/// A linear-light channel (0..1) of `a:scrgbClr` as an sRGB channel.
fn linear_to_srgb(value: f32) -> u8 {
    let encoded = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    channel_of(encoded * 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The colour of the first `a:solidFill` of `xml`.
    fn fill(xml: &str, theme: &Theme) -> Option<Rgb> {
        let doc = roxmltree::Document::parse(xml).unwrap();
        let node = doc.descendants().find(|node| node.has_tag_name("solidFill")).unwrap();
        color(node, theme, [1, 2, 3])
    }

    /// A theme part whose accent 2 is pure red.
    const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="T"><a:dk1><a:sysClr val="windowText" lastClr="111111"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:accent1><a:srgbClr val="5B9BD5"/></a:accent1><a:accent2><a:srgbClr val="FF0000"/></a:accent2><a:accent3><a:srgbClr val="zz"/></a:accent3></a:clrScheme></a:themeElements></a:theme>"#;

    /// `<a:solidFill>` around `inner`.
    fn solid(inner: &str) -> String {
        format!(r#"<a:solidFill xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">{inner}</a:solidFill>"#)
    }

    #[test]
    fn theme_part_gives_its_colours_and_office_ones_for_the_rest() {
        let theme = Theme::read(Some(THEME.as_bytes()));
        assert_eq!(theme.accents[0], [0x5B, 0x9B, 0xD5]);
        assert_eq!(theme.accents[1], [255, 0, 0]);
        assert_eq!(theme.dark1, [0x11, 0x11, 0x11]);
        // Damaged or missing: the Office colour.
        assert_eq!(theme.accents[2], Theme::default().accents[2]);
        assert_eq!(theme.dark2, Theme::default().dark2);
        assert_eq!(Theme::read(Some(b"<not xml")), Theme::default());
        assert_eq!(Theme::read(None), Theme::default());
    }

    #[test]
    fn scheme_names_and_their_aliases() {
        let theme = Theme::default();
        assert_eq!(theme.scheme("accent4"), Some([0xFF, 0xC0, 0x00]));
        assert_eq!(theme.scheme("tx1"), Some([0, 0, 0]));
        assert_eq!(theme.scheme("bg1"), Some([255, 255, 255]));
        assert_eq!(theme.scheme("tx2"), Some(theme.dark2));
        assert_eq!(theme.scheme("phClr"), None);
    }

    #[test]
    fn automatic_colours_cycle_the_accents_then_darken() {
        let theme = Theme::default();
        assert_eq!(theme.automatic(0), theme.accents[0]);
        assert_eq!(theme.automatic(5), theme.accents[5]);
        // The second round: lumMod 60 %, darker than the accent.
        let darker = theme.automatic(6);
        assert_ne!(darker, theme.accents[0]);
        assert!(
            darker.iter().map(|&channel| u32::from(channel)).sum::<u32>()
                < theme.accents[0].iter().map(|&channel| u32::from(channel)).sum()
        );
        // Any index has a colour.
        let _ = theme.automatic(usize::MAX);
    }

    #[test]
    fn drawing_colours_with_their_transforms() {
        let theme = Theme::default();
        assert_eq!(
            fill(&solid(r#"<a:srgbClr val="1A2B3C"/>"#), &theme),
            Some([0x1A, 0x2B, 0x3C])
        );
        assert_eq!(
            fill(&solid(r#"<a:sysClr val="windowText" lastClr="000000"/>"#), &theme),
            Some([0, 0, 0])
        );
        assert_eq!(
            fill(&solid(r#"<a:schemeClr val="accent2"/>"#), &theme),
            Some(theme.accents[1])
        );
        assert_eq!(fill(&solid(r#"<a:schemeClr val="phClr"/>"#), &theme), Some([1, 2, 3]));
        assert_eq!(fill(&solid(r#"<a:prstClr val="red"/>"#), &theme), Some([255, 0, 0]));
        // White at 50 % luminance is mid grey; black lightened by 25 % too.
        assert_eq!(
            fill(
                &solid(r#"<a:srgbClr val="FFFFFF"><a:lumMod val="50000"/></a:srgbClr>"#),
                &theme
            ),
            Some([128, 128, 128])
        );
        assert_eq!(
            fill(
                &solid(r#"<a:srgbClr val="000000"><a:lumMod val="50000"/><a:lumOff val="50000"/></a:srgbClr>"#),
                &theme
            ),
            Some([128, 128, 128])
        );
        assert_eq!(
            fill(
                &solid(r#"<a:srgbClr val="FF0000"><a:shade val="50000"/></a:srgbClr>"#),
                &theme
            ),
            // Half the light, not half the value.
            Some([188, 0, 0])
        );
        assert_eq!(
            fill(
                &solid(r#"<a:srgbClr val="000000"><a:tint val="50000"/></a:srgbClr>"#),
                &theme
            ),
            Some([188, 188, 188])
        );
        // A damaged value is no colour; absurd transforms stay in range.
        assert_eq!(fill(&solid(r#"<a:srgbClr val="xyz"/>"#), &theme), None);
        assert_eq!(
            fill(
                &solid(r#"<a:srgbClr val="808080"><a:lumMod val="99999999"/></a:srgbClr>"#),
                &theme
            ),
            Some([255, 255, 255])
        );
    }
}
