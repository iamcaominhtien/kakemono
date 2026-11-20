//! The Symbol font of Windows, which older Word documents use for Greek
//! letters and mathematical signs (`w:sym`, and text in the Symbol font):
//! its character codes mapped to Unicode.

/// Unicode of the Symbol font's codes 0x20 to 0x7E. Most are the ASCII
/// character of the same code, except letters (Greek) and a few signs.
const LOW: &str = " !∀#∃%&∋()∗+,−./0123456789:;<=>?≅ΑΒΧΔΕΦΓΗΙϑΚΛΜΝΟΠΘΡΣΤΥςΩΞΨΖ[∴]⊥_‾αβχδεφγηιϕκλμνοπθρστυϖωξψζ{|}∼";
/// Unicode of the Symbol font's codes 0xA0 to 0xFE.
const HIGH: &str = "€ϒ′≤⁄∞ƒ♣♦♥♠↔←↑→↓°±″≥×∝∂•÷≠≡≈…⏐⎯↵ℵℑℜ℘⊗⊕∅∩∪⊃⊇⊄⊂⊆∈∉∠∇®©™∏√⋅¬∧∨⇔⇐⇑⇒⇓◊〈®©™∑⎛⎜⎝⎡⎢⎣⎧⎨⎩⎪ 〉∫⌠⎮⌡⎞⎟⎠⎤⎥⎦⎫⎬⎭";

/// The Unicode character of code `code` of the Symbol font, or `None` for
/// a code it does not use.
pub(crate) fn symbol_font_char(code: u8) -> Option<char> {
    match code {
        0x20..=0x7E => LOW.chars().nth(usize::from(code - 0x20)),
        0xA0..=0xFE => HIGH.chars().nth(usize::from(code - 0xA0)).filter(|&ch| ch != ' '),
        _ => None,
    }
}

/// `text` written in the Symbol font, in Unicode (its characters read as
/// Symbol codes, also from the private use area F0xx).
pub(crate) fn symbol_font_text(text: &str) -> String {
    text.chars()
        .map(|ch| {
            let code = u32::from(ch);
            let code = if (0xF020..=0xF0FE).contains(&code) {
                code - 0xF000
            } else {
                code
            };
            u8::try_from(code).ok().and_then(symbol_font_char).unwrap_or(ch)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Greek letters and signs come out as Unicode.
    #[test]
    fn greek_and_signs() {
        assert_eq!(symbol_font_char(b'a'), Some('α'));
        assert_eq!(symbol_font_char(b'W'), Some('Ω'));
        assert_eq!(symbol_font_char(0xA3), Some('≤'));
        assert_eq!(symbol_font_char(0xE5), Some('∑'));
        assert_eq!(symbol_font_text("\u{F070}r"), "πρ");
    }
}
