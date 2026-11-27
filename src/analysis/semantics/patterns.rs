//! Regular expressions shared by the classification stages.
//!
//! Section numbers are captured as `num` and the title after them as
//! `rest`, so that the stages can read the level from the number.

use regex::Regex;
use std::sync::LazyLock;

/// The label of a chapter or part set as a heading of its own above the
/// chapter's title: "Chapter 1", "PART II", "CHAPTER 3:", "Appendix A",
/// "CHAPTER TWO", "Chapter Twenty-One"; the number is captured as `num`.
pub(crate) static CHAPTER_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?i:chapter|part|appendix|lesson|unit)\s+(?P<num>\d{{1,3}}|[IVXLC]{{1,6}}|[A-Z]|(?i:(?:{TENS})(?:[-\s](?:{UNITS}))?|{TEENS}|{UNITS}))[.:]?(?:\s|$)"
    ))
    .unwrap()
});

/// The number words from one to nine, in the order of their values.
const UNITS: &str = "one|two|three|four|five|six|seven|eight|nine";
/// The number words from ten to nineteen, in the order of their values.
const TEENS: &str = "ten|eleven|twelve|thirteen|fourteen|fifteen|sixteen|seventeen|eighteen|nineteen";
/// The tens from twenty to ninety, in the order of their values.
const TENS: &str = "twenty|thirty|forty|fifty|sixty|seventy|eighty|ninety";

/// The value of an English number word from one to ninety-nine ("Two",
/// "SEVENTEEN", "Twenty-One"), or `None`.
pub(crate) fn number_word_value(word: &str) -> Option<u32> {
    let word = word.to_lowercase();
    let value_in = |list: &str, word: &str| list.split('|').position(|item| item == word).map(|pos| pos as u32);
    let mut parts = word.split(['-', ' ']).filter(|part| !part.is_empty());
    let first = parts.next()?;
    let second = parts.next();
    if parts.next().is_some() {
        return None;
    }
    if let Some(tens) = value_in(TENS, first) {
        let units = match second {
            Some(unit) => value_in(UNITS, unit)? + 1,
            None => 0,
        };
        return Some(20 + 10 * tens + units);
    }
    if second.is_some() {
        return None;
    }
    value_in(TEENS, first)
        .map(|teen| 10 + teen)
        .or_else(|| value_in(UNITS, first).map(|unit| unit + 1))
}

/// The number of a block that is a chapter label and nothing else
/// ("CHAPTER TWO", "Chapter 3:"), in figures when it is written in words
/// ("2"), else as printed ("3", "IV", "A").
pub(crate) fn whole_chapter_label_number(text: &str) -> Option<String> {
    let text = text.trim();
    let captures = CHAPTER_LABEL.captures(text)?;
    if captures.get(0)?.end() < text.len() {
        return None;
    }
    let num = &captures["num"];
    Some(number_word_value(num).map_or_else(|| num.to_string(), |value| value.to_string()))
}

/// A line of dialogue in a language course: a speaker letter and a colon,
/// after an optional example label ("Ａ：すみません。", "［例］▶ Ｂ：はい").
pub(crate) static DIALOGUE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:［\s*例\s*］\s*▶?\s*)?[A-ZＡ-Ｚ]\s*[：:]\s*\S").unwrap());

/// Arabic section number and title: "3", "3.2", "3.2.1." followed by the
/// title (up to four levels of at most two digits each).
pub(crate) static HEADING_NUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>\d{1,2}(?:\.\d{1,2}){0,3})\.?\s+(?P<rest>\S.*)$").unwrap());
/// A chapter number set alone: figures ("12") or Roman numerals ("IV").
pub(crate) static BARE_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\d{1,3}|[IVXLC]{1,7})$").unwrap());
/// A section number of several parts at the start of a heading ("2.1.",
/// "3.5.1", "A.1"), which items of a list do not have.
pub(crate) static DOTTED_SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:Appendix\s+|APPENDIX\s+)?[A-Z0-9]{1,3}\.\d").unwrap());
/// Appendix label and title: "Appendix A", "A.1", "B:" followed by the
/// title; `word` captures the word "Appendix" when present.
pub(crate) static HEADING_APP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?P<word>Appendix\s+|APPENDIX\s+)?(?P<num>[A-Z](?:\.\d{1,2}){0,3})[.:]?\s+(?P<rest>\S.*)$").unwrap()
});
/// Roman section number with a period and a capitalised title ("IV. EXPERIMENTS").
pub(crate) static HEADING_ROMAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[IVX]{1,5})\.\s+(?P<rest>[A-Z].*)$").unwrap());
/// Letter with a period and a capitalised title ("A. Datasets"), the
/// subsection style of IEEE papers.
pub(crate) static HEADING_LETTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?P<num>[A-Z])\.\s+(?P<rest>[A-Z].*)$").unwrap());
/// The unnumbered heading "Introduction" (any case), which ends the author
/// block of a paper without section numbers.
pub(crate) static INTRO_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^introduction$").unwrap());
/// A theorem-like statement that starts with its label ("8.3 Lemma. ...",
/// "Theorem 2 (Main).", "Corollary 9.3:"): a paragraph with a run-in head,
/// never a section heading.
pub(crate) static THEOREM_LABEL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:(?:\d+(?:\.\d+)*\.?\s+)?(?:Theorem|Lemma|Corollary|Proposition|Definition|Remark|Claim|Conjecture|Fact|Observation|Notation|Assumption|Hypothesis|Axiom|Property)\b(?:\s+[A-Z]?\d+(?:\.\d+)*[a-z]?)?\s*[.(]|(?:Theorem|Lemma|Corollary|Proposition|Definition|Remark|Claim|Conjecture|Example)\b(?:\s+[A-Z]?\d+(?:\.\d+)*[a-z]?)?\s*[.:(])",
    )
    .unwrap()
});
/// A short capitalised label with a colon that introduces a note ("CW: ...",
/// "NOTE: ..."), not a heading.
pub(crate) static NOTE_LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{2,5}:\s+\S").unwrap());
/// An "Abstract" label run into the first paragraph ("Abstract—We …",
/// "ABSTRACT: …"), with the punctuation and spaces after it.
pub(crate) static ABSTRACT_INLINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:abstract)\s*[—–:.\-]+\s*").unwrap());
/// The heading of the bibliography, optionally numbered ("7 References",
/// "VI. BIBLIOGRAPHY"); the whole text must be the heading.
pub(crate) static REF_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:\d+\.?\s+|[ivx]+\.\s+)?(references|bibliography|references and notes|literature cited|works cited)$",
    )
    .unwrap()
});
/// Label at the start of a bibliography entry: `[label]` (captured as `a`,
/// e.g. `[12]` or `[Vas17]`) or "12." (captured as `b`).
pub(crate) static REF_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[(?P<a>[^\]]{1,24})\]|(?P<b>\d{1,3})\.(?:\s|$))").unwrap());
/// Unnumbered headings common to papers and books ("Introduction",
/// "Acknowledgments", "References"; the preface, contents and index of
/// Japanese, Chinese and Vietnamese books); such a text is a heading even
/// without a number, and never a title.
pub(crate) static KNOWN_HEADINGS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(abstract|references|bibliography|acknowledge?ments?|appendix|appendices|supplementary materials?|conclusions?|introduction|related work|contents|keywords|index terms|はじめに|まえがき|目次|索引|参考文献|あとがき|前言|序言|目录|lời nói đầu|lời giới thiệu|mục lục|tài liệu tham khảo)$").unwrap()
});

/// The heading of a book's table of contents ("Contents", "Table of
/// Contents", "目次", "目录", "Mục lục").
pub(crate) static CONTENTS_HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:(?:table of )?contents|目次|目录|mục lục)$").unwrap());
/// The end of an entry of a table of contents: leader dots (". . . ." or
/// "....", then the page number when there is one), or a page number in
/// arabic or lower-case roman figures after the title ("Preface v",
/// "1 Exploratory data analysis 1").
pub(crate) static CONTENTS_ENTRY_END: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:(?:\.\s?){4,}\s*(?:\d{1,4}|[ivxlc]{1,6})?|\s(?:\d{1,4}|[ivxlc]{1,6}))$").unwrap());

#[cfg(test)]
#[allow(clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    #[test]
    fn heading_patterns() {
        assert!(HEADING_NUM.is_match("3.2.1 Scaled Dot-Product Attention"));
        assert!(HEADING_ROMAN.is_match("IV. EXPERIMENTS"));
        assert!(REF_HEADING.is_match("References"));
        assert!(REF_HEADING.is_match("7 References"));
        assert!(KNOWN_HEADINGS.is_match("はじめに"));
        assert!(KNOWN_HEADINGS.is_match("Mục lục"));
        assert!(!REF_HEADING.is_match("References to prior work"));
        assert!(ABSTRACT_INLINE.is_match("Abstract—We propose"));
    }

    #[test]
    fn chapter_labels_in_words() {
        assert_eq!(whole_chapter_label_number("CHAPTER TWO").as_deref(), Some("2"));
        assert_eq!(whole_chapter_label_number("CHAPTER SEVENTEEN").as_deref(), Some("17"));
        assert_eq!(whole_chapter_label_number("Chapter Twenty-One").as_deref(), Some("21"));
        assert_eq!(whole_chapter_label_number("PART THREE").as_deref(), Some("3"));
        assert_eq!(whole_chapter_label_number("Chapter 4:").as_deref(), Some("4"));
        assert_eq!(whole_chapter_label_number("PART II").as_deref(), Some("II"));
        assert_eq!(whole_chapter_label_number("Chapter one of the book"), None);
        assert_eq!(whole_chapter_label_number("Chapter Twentyone"), None);
        assert!(CHAPTER_LABEL.is_match("CHAPTER SEVEN THE MIRROR"));
        assert_eq!(number_word_value("ninety-nine"), Some(99));
        assert_eq!(number_word_value("ten"), Some(10));
        assert_eq!(number_word_value("one two"), None);
    }

    #[test]
    fn contents_entries() {
        assert!(CONTENTS_HEADING.is_match("Table of Contents"));
        assert!(CONTENTS_HEADING.is_match("Mục lục"));
        assert!(!CONTENTS_HEADING.is_match("Contents of the box"));
        assert!(CONTENTS_ENTRY_END.is_match("1.1 What is a program? . . . . . . . ."));
        assert!(CONTENTS_ENTRY_END.is_match("0.1 How I wrote this book . . . . . vii"));
        assert!(CONTENTS_ENTRY_END.is_match("2.4 Script mode.......... 12"));
        assert!(CONTENTS_ENTRY_END.is_match("Preface v"));
        assert!(CONTENTS_ENTRY_END.is_match("1 Exploratory data analysis 1"));
        assert!(!CONTENTS_ENTRY_END.is_match("1 The way of the program"));
        assert!(!CONTENTS_ENTRY_END.is_match("Preface"));
    }

    #[test]
    fn theorem_labels() {
        assert!(THEOREM_LABEL.is_match("8.3 Lemma. Suppose we have a solution"));
        assert!(THEOREM_LABEL.is_match("9.3. Corollary. Under the same assumptions"));
        assert!(THEOREM_LABEL.is_match("Theorem 2 (Main). Let G be"));
        assert!(THEOREM_LABEL.is_match("Remark: this holds"));
        assert!(!THEOREM_LABEL.is_match("3 Definitions and notation"));
        assert!(!THEOREM_LABEL.is_match("5 Example Applications"));
        assert!(!THEOREM_LABEL.is_match("3 Example: Variational Auto-Encoder"));
        assert!(!THEOREM_LABEL.is_match("Lemmatization of words"));
    }

    #[test]
    fn note_labels() {
        assert!(NOTE_LABEL.is_match("CW: Section contains content related to bias"));
        assert!(!NOTE_LABEL.is_match("Setup: the datasets"));
        assert!(!NOTE_LABEL.is_match("RQ1: Does it work"));
    }
}
