//! The named character references of HTML (`&nbsp;`, `&eacute;`), which an
//! XML parser does not know without the XHTML DTD: they are written as
//! numeric references before a content document that uses them is parsed.

use regex::{Captures, Regex};
use std::sync::LazyLock;

/// A named character reference.
static NAMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&([A-Za-z][A-Za-z0-9]*);").unwrap());

/// The references XML itself defines.
const XML_NAMES: [&str; 5] = ["amp", "lt", "gt", "quot", "apos"];

/// The Latin-1 references, for U+00A0 to U+00FF in order.
const LATIN1: [&str; 96] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf", "laquo", "not", "shy",
    "reg", "macr", "deg", "plusmn", "sup2", "sup3", "acute", "micro", "para", "middot", "cedil", "sup1", "ordm",
    "raquo", "frac14", "frac12", "frac34", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring", "AElig",
    "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml", "ETH", "Ntilde", "Ograve",
    "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave", "Uacute", "Ucirc", "Uuml", "Yacute", "THORN",
    "szlig", "agrave", "aacute", "acirc", "atilde", "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc",
    "euml", "igrave", "iacute", "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml",
    "divide", "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml",
];

/// The capital Greek letters, from U+0391 (U+03A2 has none).
const GREEK_UPPER: [&str; 25] = [
    "Alpha", "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota", "Kappa", "Lambda", "Mu", "Nu", "Xi",
    "Omicron", "Pi", "Rho", "", "Sigma", "Tau", "Upsilon", "Phi", "Chi", "Psi", "Omega",
];

/// The small Greek letters, from U+03B1.
const GREEK_LOWER: [&str; 25] = [
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu", "nu", "xi",
    "omicron", "pi", "rho", "sigmaf", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
];

/// The other references of HTML 4: punctuation, arrows, mathematical
/// symbols.
const OTHERS: [(&str, u32); 62] = [
    ("OElig", 0x152),
    ("oelig", 0x153),
    ("Scaron", 0x160),
    ("scaron", 0x161),
    ("Yuml", 0x178),
    ("fnof", 0x192),
    ("circ", 0x2C6),
    ("tilde", 0x2DC),
    ("thetasym", 0x3D1),
    ("upsih", 0x3D2),
    ("piv", 0x3D6),
    ("ensp", 0x2002),
    ("emsp", 0x2003),
    ("thinsp", 0x2009),
    ("zwnj", 0x200C),
    ("zwj", 0x200D),
    ("lrm", 0x200E),
    ("rlm", 0x200F),
    ("ndash", 0x2013),
    ("mdash", 0x2014),
    ("lsquo", 0x2018),
    ("rsquo", 0x2019),
    ("sbquo", 0x201A),
    ("ldquo", 0x201C),
    ("rdquo", 0x201D),
    ("bdquo", 0x201E),
    ("dagger", 0x2020),
    ("Dagger", 0x2021),
    ("bull", 0x2022),
    ("hellip", 0x2026),
    ("permil", 0x2030),
    ("prime", 0x2032),
    ("Prime", 0x2033),
    ("lsaquo", 0x2039),
    ("rsaquo", 0x203A),
    ("oline", 0x203E),
    ("frasl", 0x2044),
    ("euro", 0x20AC),
    ("trade", 0x2122),
    ("larr", 0x2190),
    ("uarr", 0x2191),
    ("rarr", 0x2192),
    ("darr", 0x2193),
    ("harr", 0x2194),
    ("rArr", 0x21D2),
    ("hArr", 0x21D4),
    ("forall", 0x2200),
    ("part", 0x2202),
    ("exist", 0x2203),
    ("nabla", 0x2207),
    ("isin", 0x2208),
    ("sum", 0x2211),
    ("minus", 0x2212),
    ("radic", 0x221A),
    ("infin", 0x221E),
    ("int", 0x222B),
    ("sim", 0x223C),
    ("asymp", 0x2248),
    ("ne", 0x2260),
    ("equiv", 0x2261),
    ("le", 0x2264),
    ("ge", 0x2265),
];

/// `text` with every named reference of HTML written as a numeric one; an
/// unknown name is kept as text (`&amp;name;`).
pub(crate) fn to_numeric(text: &str) -> String {
    NAMED
        .replace_all(text, |captures: &Captures<'_>| {
            let name = &captures[1];
            if XML_NAMES.contains(&name) {
                return captures[0].to_string();
            }
            match code_point(name) {
                Some(code) => format!("&#{code};"),
                None => format!("&amp;{name};"),
            }
        })
        .into_owned()
}

/// The code point of the reference `name`, if HTML defines it.
fn code_point(name: &str) -> Option<u32> {
    let index = |names: &[&str]| names.iter().position(|known| *known == name);
    if let Some(at) = index(&LATIN1) {
        return Some(0xA0 + at as u32);
    }
    if let Some(at) = index(&GREEK_UPPER) {
        return Some(0x391 + at as u32);
    }
    if let Some(at) = index(&GREEK_LOWER) {
        return Some(0x3B1 + at as u32);
    }
    OTHERS.iter().find(|(known, _)| *known == name).map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::to_numeric;

    #[test]
    fn html_names_become_numbers() {
        assert_eq!(
            to_numeric("a&nbsp;b &eacute; &amp; &Omega;&mdash;&bogus;"),
            "a&#160;b &#233; &amp; &#937;&#8212;&amp;bogus;"
        );
    }
}
