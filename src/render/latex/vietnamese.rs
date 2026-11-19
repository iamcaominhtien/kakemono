//! Vietnamese letters in pdfLaTeX. The T1 fonts of pdfLaTeX lack the
//! letters with a horn (ơ, ư) and those with two accents (ấ, ặ, ữ), but
//! the T5 encoding of the vntex package has them all. A document that holds
//! such letters loads T5 next to T1, and each letter it uses is declared to
//! be set in T5, wherever it is (headings, tables, code listings): the rest
//! of the text stays in T1, which has the letters of the other languages
//! (Polish ą, say) that T5 lacks. XeLaTeX and LuaLaTeX need none of this.

use unicode_normalization::UnicodeNormalization;

/// The command that sets its argument in the T5 encoding, defined in the
/// preamble of a document with Vietnamese letters.
pub(crate) const T5_COMMAND: &str = "\\kakemonovn";

/// The LaTeX of a Vietnamese letter that the T1 fonts lack, in the commands
/// of the T5 encoding (as vntex's `t5enc.dfu` writes it), or `None` for
/// any other character.
pub(crate) fn t5_letter(ch: char) -> Option<String> {
    let horn_or_extended = matches!(ch, 'Ơ' | 'ơ' | 'Ư' | 'ư') || ('\u{1EA0}'..='\u{1EF9}').contains(&ch);
    if !horn_or_extended {
        return None;
    }
    let mut parts = ch.nfd();
    let base = parts.next()?;
    if !base.is_ascii_alphabetic() {
        return None;
    }
    let mut letter = base.to_string();
    let mut tone: Option<&str> = None;
    for mark in parts {
        match mark {
            // Marks that make a letter of its own in T5.
            '\u{302}' | '\u{306}' | '\u{31B}' => letter = modified_letter(base, mark)?,
            '\u{300}' => tone = Some("\\`"),
            '\u{301}' => tone = Some("\\'"),
            '\u{303}' => tone = Some("\\~"),
            '\u{309}' => tone = Some("\\h"),
            '\u{323}' => tone = Some("\\d"),
            _ => return None,
        }
    }
    // An accent above an i takes the dotless i.
    if letter == "i" && tone.is_some_and(|tone| tone != "\\d") {
        letter = "\\i".to_string();
    }
    Some(match tone {
        Some(tone) => format!("{tone}{{{letter}}}"),
        None => letter,
    })
}

/// The T5 command of a letter with a circumflex, breve or horn (`mark`).
fn modified_letter(base: char, mark: char) -> Option<String> {
    let name = match mark {
        '\u{302}' => "circumflex",
        '\u{306}' => "breve",
        _ => "horn",
    };
    let lower = base.to_ascii_lowercase();
    let known = match name {
        "circumflex" => matches!(lower, 'a' | 'e' | 'o'),
        "breve" => lower == 'a',
        _ => matches!(lower, 'o' | 'u'),
    };
    if !known {
        return None;
    }
    let command = format!("{lower}{name}");
    Some(if base.is_ascii_uppercase() {
        format!("\\{}", command.to_ascii_uppercase())
    } else {
        format!("\\{command}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_are_written_as_vntex_writes_them() {
        assert_eq!(t5_letter('ơ').as_deref(), Some("\\ohorn"));
        assert_eq!(t5_letter('Ư').as_deref(), Some("\\UHORN"));
        assert_eq!(t5_letter('ạ').as_deref(), Some("\\d{a}"));
        assert_eq!(t5_letter('ặ').as_deref(), Some("\\d{\\abreve}"));
        assert_eq!(t5_letter('ấ').as_deref(), Some("\\'{\\acircumflex}"));
        assert_eq!(t5_letter('Ầ').as_deref(), Some("\\`{\\ACIRCUMFLEX}"));
        assert_eq!(t5_letter('ớ').as_deref(), Some("\\'{\\ohorn}"));
        assert_eq!(t5_letter('ỉ').as_deref(), Some("\\h{\\i}"));
        assert_eq!(t5_letter('ị').as_deref(), Some("\\d{i}"));
        assert_eq!(t5_letter('ỹ').as_deref(), Some("\\~{y}"));
        assert_eq!(t5_letter('ả').as_deref(), Some("\\h{a}"));
    }

    #[test]
    fn other_characters_are_not_vietnamese() {
        for ch in ['a', 'é', 'ă', 'đ', 'ą', 'ß', 'α', 'ḁ', 'Ẁ'] {
            assert_eq!(t5_letter(ch), None, "{ch}");
        }
        // Every letter of the Vietnamese block is written.
        for ch in '\u{1EA0}'..='\u{1EF9}' {
            assert!(t5_letter(ch).is_some(), "{ch} U+{:04X}", u32::from(ch));
        }
    }
}
