//! Symbol fonts whose glyphs come without a Unicode mapping: their
//! characters are read by character code from the font's known encoding.
//! The tables are only used for glyphs that the PDF leaves unmapped.

use serde::{Deserialize, Serialize};

/// A symbol font with a known encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolFont {
    /// LaTeX's `lasy` (`latexsym`): the box that ends a proof, ◇, ⇝, ...
    Lasy,
    /// A math symbol font in TeX's OMS encoding (the encoding of `cmsy`),
    /// such as the `Symbol` fonts of MathDesign.
    Oms,
    /// The `MathDesignSymbolA` fonts: blackboard bold capitals at their
    /// letters (ℝ at "R") and AMS symbols.
    MathDesignA,
    /// A math italic font in TeX's OML encoding (the encoding of `cmmi`).
    Oml,
    /// ZapfDingbats (card suits; a heart marks hard exercises).
    Dingbats,
    /// `stmaryrd` (the short arrows: an edge u→v).
    Stmary,
    /// A math extension font in TeX's OMX encoding (the encoding of
    /// `cmex`): big delimiters, big operators and the pieces of both.
    Omx,
    /// LibreOffice's OpenSymbol, whose bullets come at the private-use
    /// code points of Wingdings.
    OpenSymbol,
}

impl SymbolFont {
    /// The symbol font whose PostScript name (lower case, without subset
    /// prefix) is `lower`, if it is one.
    pub fn from_name(lower: &str) -> Option<Self> {
        if tex_font(lower, &["lasy"]) {
            Some(Self::Lasy)
        } else if lower.starts_with("mathdesign") && lower.contains("symbola") {
            Some(Self::MathDesignA)
        } else if (lower.starts_with("mathdesign") && lower.contains("-symbol-")) || tex_font(lower, &["cmsy", "cmbsy"])
        {
            Some(Self::Oms)
        } else if (lower.starts_with("mathdesign") && lower.contains("-extension-")) || tex_font(lower, &["cmex"]) {
            Some(Self::Omx)
        } else if lower.starts_with("opensymbol") {
            Some(Self::OpenSymbol)
        } else if lower.starts_with("mathdesign") && lower.contains("mathitalic") {
            Some(Self::Oml)
        } else if lower == "dingbats" || lower.starts_with("zapfdingbats") {
            Some(Self::Dingbats)
        } else if tex_font(lower, &["stmary"]) {
            Some(Self::Stmary)
        } else {
            None
        }
    }

    /// The text of a glyph that the PDF maps to the private-use character
    /// `ch`, when the font gives that character a known meaning.
    pub fn private_text(self, ch: char) -> Option<&'static str> {
        match (self, ch) {
            // Wingdings' "l", the round bullet of word processors.
            (Self::OpenSymbol, '\u{F06C}') => Some("\u{25CF}"),
            _ => None,
        }
    }

    /// The text of the glyph of character code `code`, or `None` when the
    /// table does not know it.
    pub fn text(self, code: u32) -> Option<String> {
        let code = u8::try_from(code).ok()?;
        match self {
            Self::Lasy => lasy(code).map(String::from),
            Self::Oms => oms(code),
            Self::MathDesignA => mathdesign_a(code),
            Self::Oml => (code == 0x60).then(|| "\u{2113}".to_string()),
            Self::Dingbats => dingbats(code).map(String::from),
            Self::Stmary => ["\u{2190}", "\u{2192}", "\u{2191}", "\u{2193}"]
                .get(usize::from(code))
                .map(|arrow| arrow.to_string()),
            Self::Omx => OMX
                .get(usize::from(code))
                .filter(|text| !text.is_empty())
                .map(|text| text.to_string()),
            Self::OpenSymbol => None,
        }
    }
}

/// Whether `lower` is one of the TeX fonts `stems` at a design size
/// (`cmsy10`, `lasy7`).
fn tex_font(lower: &str, stems: &[&str]) -> bool {
    stems.iter().any(|stem| {
        lower
            .strip_prefix(stem)
            .is_some_and(|size| !size.is_empty() && size.chars().all(|ch| ch.is_ascii_digit()))
    })
}

/// TeX's OMX encoding (`cmex`). Sized delimiters read as the delimiter; the
/// extension pieces read as the Unicode bracket pieces; the pieces that are
/// only drawn (the stem of a radical, arrow and brace tips) have no text.
#[rustfmt::skip]
const OMX: [&str; 128] = [
    "(", ")", "[", "]", "\u{230A}", "\u{230B}", "\u{2308}", "\u{2309}",
    "{", "}", "\u{27E8}", "\u{27E9}", "|", "\u{2016}", "/", "\\",
    "(", ")", "(", ")", "[", "]", "\u{230A}", "\u{230B}",
    "\u{2308}", "\u{2309}", "{", "}", "\u{27E8}", "\u{27E9}", "/", "\\",
    "(", ")", "[", "]", "\u{230A}", "\u{230B}", "\u{2308}", "\u{2309}",
    "{", "}", "\u{27E8}", "\u{27E9}", "/", "\\", "/", "\\",
    "\u{239B}", "\u{239E}", "\u{23A1}", "\u{23A4}", "\u{23A3}", "\u{23A6}", "\u{23A2}", "\u{23A5}",
    "\u{23A7}", "\u{23AB}", "\u{23A9}", "\u{23AD}", "\u{23A8}", "\u{23AC}", "\u{23AA}", "",
    "\u{239D}", "\u{23A0}", "\u{239C}", "\u{239F}", "\u{27E8}", "\u{27E9}", "\u{2A06}", "\u{2A06}",
    "\u{222E}", "\u{222E}", "\u{2A00}", "\u{2A00}", "\u{2A01}", "\u{2A01}", "\u{2A02}", "\u{2A02}",
    "\u{2211}", "\u{220F}", "\u{222B}", "\u{22C3}", "\u{22C2}", "\u{2A04}", "\u{22C0}", "\u{22C1}",
    "\u{2211}", "\u{220F}", "\u{222B}", "\u{22C3}", "\u{22C2}", "\u{2A04}", "\u{22C0}", "\u{22C1}",
    "\u{2210}", "\u{2210}", "^", "^", "^", "~", "~", "~",
    "[", "]", "\u{230A}", "\u{230B}", "\u{2308}", "\u{2309}", "{", "}",
    "\u{221A}", "\u{221A}", "\u{221A}", "\u{221A}", "\u{221A}", "", "", "",
    "", "", "", "", "", "", "", "",
];

/// `lasy`, the encoding of `latexsym.sty`.
fn lasy(code: u8) -> Option<&'static str> {
    Some(match code {
        0x01 => "\u{25C1}",
        0x02 => "\u{22B4}",
        0x03 => "\u{25B7}",
        0x04 => "\u{22B5}",
        0x30 => "\u{2127}",
        0x31 => "\u{22C8}",
        0x32 => "\u{25A1}",
        0x33 => "\u{25C7}",
        0x3B => "\u{219D}",
        0x3C => "\u{228F}",
        0x3D => "\u{2290}",
        _ => return None,
    })
}

/// The text of the stem that OMS draws before an arrow to make ↦: the
/// extraction joins it with the arrow that follows.
pub const MAPSTO_STEM: &str = "\u{21A6}";

/// TeX's OMS encoding (`cmsy`). The negation slash, which is drawn over
/// other symbols, has no text of its own.
#[rustfmt::skip]
const OMS: [&str; 128] = [
    "\u{2212}", "\u{22C5}", "\u{D7}", "\u{2217}", "\u{F7}", "\u{22C4}", "\u{B1}", "\u{2213}",
    "\u{2295}", "\u{2296}", "\u{2297}", "\u{2298}", "\u{2299}", "\u{25EF}", "\u{2218}", "\u{2219}",
    "\u{224D}", "\u{2261}", "\u{2286}", "\u{2287}", "\u{2264}", "\u{2265}", "\u{2AAF}", "\u{2AB0}",
    "\u{223C}", "\u{2248}", "\u{2282}", "\u{2283}", "\u{226A}", "\u{226B}", "\u{227A}", "\u{227B}",
    "\u{2190}", "\u{2192}", "\u{2191}", "\u{2193}", "\u{2194}", "\u{2197}", "\u{2198}", "\u{2243}",
    "\u{21D0}", "\u{21D2}", "\u{21D1}", "\u{21D3}", "\u{21D4}", "\u{2196}", "\u{2199}", "\u{221D}",
    "\u{2032}", "\u{221E}", "\u{2208}", "\u{220B}", "\u{25B3}", "\u{25BD}", "", MAPSTO_STEM,
    "\u{2200}", "\u{2203}", "\u{AC}", "\u{2205}", "\u{211C}", "\u{2111}", "\u{22A4}", "\u{22A5}",
    "\u{2135}", "", "", "", "", "", "", "",
    "", "", "", "", "", "", "", "",
    "", "", "", "", "", "", "", "",
    "", "", "", "\u{222A}", "\u{2229}", "\u{228E}", "\u{2227}", "\u{2228}",
    "\u{22A2}", "\u{22A3}", "\u{230A}", "\u{230B}", "\u{2308}", "\u{2309}", "{", "}",
    "\u{27E8}", "\u{27E9}", "|", "\u{2016}", "\u{2195}", "\u{21D5}", "\u{2216}", "\u{2240}",
    "\u{221A}", "\u{2A3F}", "\u{2207}", "\u{222B}", "\u{2294}", "\u{2293}", "\u{2291}", "\u{2292}",
    "\u{A7}", "\u{2020}", "\u{2021}", "\u{B6}", "\u{2663}", "\u{2662}", "\u{2661}", "\u{2660}",
];

/// The OMS symbol of `code`: the table, and script capitals at the letters.
fn oms(code: u8) -> Option<String> {
    if code.is_ascii_uppercase() {
        return Some(styled_capital(code, SCRIPT_CAPITALS, 0x1D49C).to_string());
    }
    OMS.get(usize::from(code))
        .filter(|text| !text.is_empty())
        .map(|text| text.to_string())
}

/// `MathDesignSymbolA`: the symbols seen in its fonts, and blackboard bold
/// capitals at the letters.
fn mathdesign_a(code: u8) -> Option<String> {
    if code.is_ascii_uppercase() {
        return Some(styled_capital(code, DOUBLE_STRUCK_CAPITALS, 0x1D538).to_string());
    }
    let text = match code {
        63 => "\u{2205}",
        131 => "\u{25A1}",
        132 => "\u{25A0}",
        135 => "\u{29EB}",
        160 => "\u{21DD}",
        191 => "\u{2277}",
        194 => "\u{22B3}",
        195 => "\u{22B2}",
        198 => "\u{2605}",
        200 => "\u{25BC}",
        201 => "\u{25B6}",
        202 => "\u{25C0}",
        206 => "\u{25B2}",
        220 => "\u{2220}",
        250 => "\u{2720}",
        _ => return None,
    };
    Some(text.to_string())
}

/// ZapfDingbats: the card suits.
fn dingbats(code: u8) -> Option<&'static str> {
    Some(match code {
        168 => "\u{2663}",
        169 => "\u{2666}",
        170 => "\u{2665}",
        171 => "\u{2660}",
        _ => return None,
    })
}

/// Capitals of the Letterlike Symbols block that replace the holes of the
/// mathematical script alphabet (B, E, F, H, I, L, M, R).
const SCRIPT_CAPITALS: &[(u8, char)] = &[
    (b'B', '\u{212C}'),
    (b'E', '\u{2130}'),
    (b'F', '\u{2131}'),
    (b'H', '\u{210B}'),
    (b'I', '\u{2110}'),
    (b'L', '\u{2112}'),
    (b'M', '\u{2133}'),
    (b'R', '\u{211B}'),
];

/// The same for the double-struck alphabet (C, H, N, P, Q, R, Z).
const DOUBLE_STRUCK_CAPITALS: &[(u8, char)] = &[
    (b'C', '\u{2102}'),
    (b'H', '\u{210D}'),
    (b'N', '\u{2115}'),
    (b'P', '\u{2119}'),
    (b'Q', '\u{211A}'),
    (b'R', '\u{211D}'),
    (b'Z', '\u{2124}'),
];

/// The capital `letter` of a mathematical alphabet starting at `base`
/// (its "A"), or its letterlike replacement.
fn styled_capital(letter: u8, holes: &[(u8, char)], base: u32) -> char {
    holes
        .iter()
        .find(|(hole, _)| *hole == letter)
        .map(|(_, ch)| *ch)
        .or_else(|| char::from_u32(base + u32::from(letter - b'A')))
        .unwrap_or('\u{FFFD}')
}

#[cfg(test)]
mod tests {
    use super::SymbolFont;

    #[test]
    fn symbol_fonts_are_read_by_code() {
        let oms = SymbolFont::from_name("mathdesign-ch-regular-symbol-10").unwrap();
        assert_eq!(oms.text(0x30).as_deref(), Some("\u{2032}"));
        assert_eq!(oms.text(u32::from(b'L')).as_deref(), Some("\u{2112}"));
        let bbb = SymbolFont::from_name("mathdesign-ch-regular-mathdesignsymbola-10").unwrap();
        assert_eq!(bbb.text(u32::from(b'R')).as_deref(), Some("\u{211D}"));
        assert_eq!(bbb.text(u32::from(b'A')).as_deref(), Some("\u{1D538}"));
        assert_eq!(
            SymbolFont::from_name("lasy10").unwrap().text(0x32).as_deref(),
            Some("\u{25A1}")
        );
        assert_eq!(
            SymbolFont::from_name("dingbats").unwrap().text(170).as_deref(),
            Some("\u{2665}")
        );
        assert_eq!(
            SymbolFont::from_name("stmary10").unwrap().text(1).as_deref(),
            Some("\u{2192}")
        );
        assert_eq!(SymbolFont::from_name("lasyb10x"), None);
        assert_eq!(SymbolFont::from_name("cmsy10"), Some(SymbolFont::Oms));
        let omx = SymbolFont::from_name("mathdesign-ch-regular-extension-10").unwrap();
        assert_eq!(omx.text(0).as_deref(), Some("("));
        assert_eq!(omx.text(0x58).as_deref(), Some("\u{2211}"));
        assert_eq!(omx.text(0x7E), None);
        let bullets = SymbolFont::from_name("opensymbol").unwrap();
        assert_eq!(bullets.private_text('\u{F06C}'), Some("\u{25CF}"));
    }
}
