//! The bibliographic fields of `meta` in the JSON output, read from the
//! front matter: the authors with their affiliations, e-mail addresses and
//! notes, the arXiv category and date, the DOI, the keywords and the
//! abstract as plain text. Like `citation`, these fields are read-only and
//! hold only what could be read with confidence.
//!
//! Authors come from the author blocks (one "card" each, see
//! [`crate::analysis::semantics`]). In a card, the first rows that read as
//! names are names; rows with an `@` are e-mail addresses
//! (`{a, b}@example.com` gives two); a row that starts with a raised mark
//! holds notes; a date gives the date of the paper; the other rows are the
//! affiliation of the names of the card. A raised mark after a name
//! (`Jane Doe¹`) points to a numbered affiliation (a row or footnote that
//! starts with that mark), or to a note when it is a symbol (`∗`). Names
//! printed side by side on one line without commas are told apart by the
//! gaps between them in the PDF.

use super::citation::{DOI, is_name, trim_id};
use super::plain_text::plain_of;
use crate::model::{Block, BlockKind, Document, Line, SUP, Span};
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;
use unicode_normalization::UnicodeNormalization;

/// A gap between two glyphs of a line wider than this (multiple of the
/// font size) separates two names.
const NAME_GAP_EM: f32 = 1.0;
/// A row of names read as one name of at least this many words is checked
/// for names printed side by side.
const SIDE_BY_SIDE_MIN_WORDS_COUNT: usize = 4;
/// Pages searched for the DOI and the keywords.
const FRONT_PAGES_COUNT: usize = 2;
/// Fewest letters of a name part that is matched against an address.
const EMAIL_MATCH_MIN_CHARS_COUNT: usize = 3;
/// Placeholder of a raised mark while a row of names is split.
const MARK_OPEN: char = '\u{2}';
/// End of the placeholder of a raised mark.
const MARK_CLOSE: char = '\u{3}';
/// Names of the months, by number from 1.
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// The arXiv stamp: "arXiv:1706.03762v7 [cs.CL] 2 Aug 2023", or without
/// a category ("arXiv:hep-th/9711200v3 22 Jan 1998").
static STAMP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\[\s*([a-z\-]+(?:\.[A-Za-z\-]+)?)\s*\]\s*)?\b(\d{1,2})\s*([A-Za-z]{3,9})\.?\s*(\d{4})").unwrap()
});
/// A date: "21 January 2016", "November 26, 2024", "December 1997".
static DATE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\b(\d{1,2})(?:st|nd|rd|th)?\s+([A-Za-z]{3,9})\.?,?\s+(\d{4})\b|\b([A-Za-z]{3,9})\.?\s+(?:(\d{1,2}),?\s+)?(\d{4})\b)").unwrap()
});
/// E-mail addresses with their user names in braces:
/// `{kahe, v-xiangz}@microsoft.com`.
static BRACED_EMAILS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{([^}]+)\}\s*@\s*([\w\-]+(?:\.[\w\-]+)+)").unwrap());
/// An e-mail address.
static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w.+\-]+@[\w\-]+(?:\.[\w\-]+)*\.[A-Za-z]{2,}").unwrap());
/// What separates two names in a row: a comma, "and", "&", a wide space.
static NAME_SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:,\s*(?:and\s+|&\s*)?|\s+and\s+|\s*&\s*|\u{2003}|;)\s*").unwrap());
/// "et al." after the last name.
static ET_AL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\s*\bet\.?\s+al\b\.?").unwrap());
/// The head of a list of keywords.
static KEYWORDS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:keywords|key\s*words|index\s+terms)\s*[:.—–-]*\s*").unwrap());
/// What separates two keywords.
static KEYWORD_SEPARATOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*(?:[,;·•]|\s[-–—]\s)\s*").unwrap());

/// The fields of `meta` read from the front matter.
#[derive(Debug, Default, Serialize)]
pub(crate) struct Front {
    /// arXiv category ("cs.CL"), from the arXiv stamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) arxiv_category: Option<String>,
    /// Date of the paper, "YYYY-MM-DD" or "YYYY-MM": of the arXiv version
    /// (from its stamp), or else the date printed with the authors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) date: Option<String>,
    /// DOI of the paper, printed on its first pages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) doi: Option<String>,
    /// The authors, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) authors: Vec<Author>,
    /// The affiliations the authors refer to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) affiliations: Vec<String>,
    /// The notes on authors the authors refer to ("Equal contribution").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) author_notes: Vec<String>,
    /// Keywords ("Index Terms").
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) keywords: Vec<String>,
    /// The abstract as plain text, one paragraph per line.
    #[serde(rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub(crate) abstract_text: Option<String>,
}

/// An author.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub(crate) struct Author {
    /// The name as printed.
    pub(crate) name: String,
    /// Indices into [`Front::affiliations`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) affiliations: Vec<usize>,
    /// E-mail addresses.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) emails: Vec<String>,
    /// Indices into [`Front::author_notes`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) notes: Vec<usize>,
    /// The raised marks after the name, until they are resolved.
    #[serde(skip)]
    marks: Vec<String>,
}

/// Reads the bibliographic fields of `doc`.
pub(crate) fn front_matter(doc: &Document) -> Front {
    let mut front = Front::default();
    if let Some(caps) = doc.meta.arxiv_stamp.as_deref().and_then(|stamp| STAMP.captures(stamp)) {
        front.arxiv_category = caps.get(1).map(|category| category.as_str().to_string());
        front.date = iso_date(caps[4].parse().ok(), month_number(&caps[3]), caps[2].parse().ok());
    }
    let mut people = People::default();
    for block in doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
    {
        people.card(block, &mut front.date);
    }
    let marked = marked_texts(doc, &people.mark_rows);
    people.resolve(marked);
    people.match_emails();
    front.authors = people.authors;
    front.affiliations = people.affiliations;
    front.author_notes = people.notes;
    front.doi = doi(doc);
    front.keywords = keywords(doc);
    let abstract_text = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Paragraph { in_abstract: true }))
        .map(|block| plain_of(&block.spans))
        .filter(|text| !KEYWORDS.is_match(text))
        .collect::<Vec<_>>()
        .join("\n");
    front.abstract_text = (!abstract_text.is_empty()).then_some(abstract_text);
    front
}

/// The authors being read.
#[derive(Default)]
struct People {
    /// The authors so far.
    authors: Vec<Author>,
    /// The affiliations so far.
    affiliations: Vec<String>,
    /// The notes so far.
    notes: Vec<String>,
    /// E-mail addresses not given to an author yet.
    loose_emails: Vec<String>,
    /// Rows of the author blocks that start with a raised mark: (mark,
    /// text).
    mark_rows: Vec<(String, String)>,
    /// The authors of the last author block, when it had no affiliation.
    unaffiliated: Option<std::ops::Range<usize>>,
}

impl People {
    /// Reads one author block.
    fn card(&mut self, block: &Block, date: &mut Option<String>) {
        let rows: Vec<&[Span]> = if block.rows.is_empty() {
            vec![&block.spans]
        } else {
            block.rows.iter().map(Vec::as_slice).collect()
        };
        let first = self.authors.len();
        let mut names_done = false;
        let mut names_continue = false;
        let mut affiliation_rows: Vec<String> = Vec::new();
        let mut emails: Vec<String> = Vec::new();
        for row in rows {
            let text = plain_of(row);
            if text.contains('@') {
                emails.extend(emails_in(&text));
                names_done = true;
                continue;
            }
            if let Some(found) = row_date(&text) {
                date.get_or_insert(found);
                names_done = true;
                continue;
            }
            if starts_with_mark(row) {
                self.mark_rows.extend(marked_row(row));
                names_done = true;
                continue;
            }
            // Names fill the first row, and the next ones while a row ends
            // with a comma or the next starts with "and".
            let continued = self.authors.len() == first || names_continue || text.starts_with("and ");
            if !names_done
                && continued
                && let Some(names) = names_row(row, &block.lines)
            {
                self.authors.extend(names);
                names_continue = text.ends_with(',');
                continue;
            }
            names_done = true;
            if is_affiliation(&text) {
                // The raw text, which keeps the wide spaces between
                // affiliations printed side by side.
                affiliation_rows.push(row.iter().map(|span| span.text.as_str()).collect());
            }
        }
        let names = first..self.authors.len();
        // The names of the block just before, when they had no affiliation.
        let unaffiliated = self.unaffiliated.take();
        if names.is_empty() {
            // "Juan Maldacena¹" and below, in a block of its own, "Lyman
            // Laboratory of Physics, Harvard University".
            if let Some(before) = unaffiliated
                && !affiliation_rows.is_empty()
            {
                let index = self.affiliation(&affiliation_rows.join(", "));
                for author in before {
                    self.authors[author].affiliations.push(index);
                }
            }
            self.loose_emails.extend(emails);
            return;
        }
        if affiliation_rows.is_empty() {
            self.unaffiliated = Some(names.clone());
        }
        // One affiliation per name, side by side ("OpenAI  OpenAI").
        let side_by_side = match affiliation_rows.as_slice() {
            [row] if names.len() > 1 => row_pieces(row, &block.lines).filter(|pieces| pieces.len() == names.len()),
            _ => None,
        };
        if let Some(pieces) = side_by_side {
            for (author, part) in names.clone().zip(pieces) {
                let index = self.affiliation(&part);
                self.authors[author].affiliations.push(index);
            }
        } else if !affiliation_rows.is_empty() {
            let index = self.affiliation(&affiliation_rows.join(", "));
            for author in names.clone() {
                self.authors[author].affiliations.push(index);
            }
        }
        if emails.len() == names.len() {
            for (author, email) in names.zip(emails) {
                self.authors[author].emails.push(email);
            }
        } else {
            self.loose_emails.extend(emails);
        }
    }

    /// The index of an affiliation, added when new.
    fn affiliation(&mut self, text: &str) -> usize {
        index_of(
            &mut self.affiliations,
            text.split_whitespace().collect::<Vec<_>>().join(" "),
        )
    }

    /// Gives each author the affiliations and notes their marks point to,
    /// and the e-mail addresses in them.
    fn resolve(&mut self, marked: Vec<(String, String)>) {
        for author in &mut self.authors {
            for mark in std::mem::take(&mut author.marks) {
                let Some((_, text)) = marked.iter().find(|(found, _)| *found == mark) else {
                    continue;
                };
                // An address in the note is the author's.
                let emails = emails_in(text);
                for email in &emails {
                    if !author.emails.contains(email) {
                        author.emails.push(email.clone());
                    }
                }
                let only_emails = EMAIL
                    .replace_all(text, "")
                    .trim_matches([' ', ',', ';', '.', ':'])
                    .is_empty();
                if only_emails && !emails.is_empty() {
                    continue;
                }
                if mark.chars().all(char::is_alphanumeric) {
                    let index = index_of(&mut self.affiliations, text.clone());
                    if !author.affiliations.contains(&index) {
                        author.affiliations.push(index);
                    }
                } else {
                    let index = index_of(&mut self.notes, text.clone());
                    if !author.notes.contains(&index) {
                        author.notes.push(index);
                    }
                }
            }
        }
    }

    /// Gives each loose e-mail address to the one author whose name it
    /// holds ("drob@mit.edu" is Daniel A. Roberts's), or to the only author.
    fn match_emails(&mut self) {
        for email in std::mem::take(&mut self.loose_emails) {
            let user = fold(email.split('@').next().unwrap_or(""));
            let matching: Vec<usize> = (0..self.authors.len())
                .filter(|&i| name_matches(&self.authors[i].name, &user))
                .collect();
            let author = match matching.as_slice() {
                [only] => Some(*only),
                [] if self.authors.len() == 1 => Some(0),
                _ => None,
            };
            if let Some(author) = author {
                self.authors[author].emails.push(email);
            }
        }
    }
}

/// The index of `text` in `list`, added when new.
fn index_of(list: &mut Vec<String>, text: String) -> usize {
    match list.iter().position(|found| *found == text) {
        Some(index) => index,
        None => {
            list.push(text);
            list.len() - 1
        }
    }
}

/// Whether an e-mail user name (`user`, folded) belongs to `name`: it holds
/// the surname or the first given name, or the first initial and the start
/// of the surname ("drob" for Daniel Roberts).
fn name_matches(name: &str, user: &str) -> bool {
    let words: Vec<String> = name
        .split_whitespace()
        .map(fold)
        .filter(|word| !word.is_empty())
        .collect();
    let (Some(first), Some(last)) = (words.first(), words.last()) else {
        return false;
    };
    let long = |word: &String| word.chars().count() >= EMAIL_MATCH_MIN_CHARS_COUNT;
    (long(last) && user.contains(last.as_str()))
        || (long(first) && user.contains(first.as_str()))
        || first.chars().next().is_some_and(|initial| {
            let stem: String = last.chars().take(EMAIL_MATCH_MIN_CHARS_COUNT).collect();
            long(&stem) && user.starts_with(&format!("{initial}{stem}"))
        })
}

/// Lower-case ASCII letters of `text`, accents removed.
fn fold(text: &str) -> String {
    text.nfd()
        .filter(char::is_ascii_alphabetic)
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

/// The authors of a row of names with the raised marks after each, or
/// `None` when the row does not read as names. `lines` are the lines of
/// the author block, to tell apart names printed side by side.
fn names_row(row: &[Span], lines: &[Line]) -> Option<Vec<Author>> {
    // The raised marks become placeholders, so that a comma between two
    // marks ("∗,†") does not separate names.
    let mut text = String::new();
    let mut marks: Vec<String> = Vec::new();
    for span in row {
        if span.style & SUP != 0 {
            marks.push(span.text.clone());
            text.push(MARK_OPEN);
            text.push_str(&(marks.len() - 1).to_string());
            text.push(MARK_CLOSE);
        } else {
            text.push_str(&span.text);
        }
    }
    let text = ET_AL.replace_all(&text, "");
    let text = text.trim().trim_start_matches("By ").trim_start_matches("by ");
    let mut authors = Vec::new();
    for piece in NAME_SEPARATOR.split(text) {
        let (name, piece_marks) = take_marks(piece, &marks);
        let name = name
            .trim()
            .trim_end_matches([',', '*', '∗', '†', '‡', '§', '¶', '⋆'])
            .trim()
            .to_string();
        if name.is_empty() {
            // Marks alone after a comma: they belong to the name before.
            if let Some(last) = authors.last_mut() {
                let last: &mut Author = last;
                last.marks.extend(piece_marks);
            }
            continue;
        }
        if name.split_whitespace().count() >= SIDE_BY_SIDE_MIN_WORDS_COUNT
            && let Some(names) = side_by_side(&name, lines)
        {
            authors.extend(names.into_iter().map(|name| Author {
                name,
                ..Author::default()
            }));
            continue;
        }
        if !is_name(&name) {
            return None;
        }
        authors.push(Author {
            name,
            marks: piece_marks,
            ..Author::default()
        });
    }
    (!authors.is_empty()).then_some(authors)
}

/// The text of a piece of a row of names without its mark placeholders,
/// and the marks they stand for (`∗,†` is two marks).
fn take_marks(piece: &str, marks: &[String]) -> (String, Vec<String>) {
    let mut name = String::new();
    let mut found = Vec::new();
    let mut rest = piece;
    while let Some(open) = rest.find(MARK_OPEN) {
        name.push_str(&rest[..open]);
        let after = &rest[open + MARK_OPEN.len_utf8()..];
        let close = after.find(MARK_CLOSE).unwrap_or(after.len());
        if let Some(mark) = after[..close].parse::<usize>().ok().and_then(|i| marks.get(i)) {
            found.extend(
                mark.split([',', ' '])
                    .map(str::trim)
                    .filter(|mark| !mark.is_empty())
                    .map(str::to_string),
            );
        }
        rest = after.get(close + MARK_CLOSE.len_utf8()..).unwrap_or("");
    }
    name.push_str(rest);
    (name, found)
}

/// The names of the line of `lines` whose text is `text`, told apart by the
/// wide gaps between them ("Kaiming He   Xiangyu Zhang   Shaoqing Ren"),
/// when each reads as a name.
fn side_by_side(text: &str, lines: &[Line]) -> Option<Vec<String>> {
    let line = line_of(text, lines)?;
    let names = gap_pieces(line);
    (names.len() > 1 && names.iter().all(|name| is_name(name))).then_some(names)
}

/// The parts of a row printed side by side: split at its wide spaces (two
/// lines on one baseline), or else at the wide gaps of its line. `None`
/// when the row is in one piece.
fn row_pieces(row: &str, lines: &[Line]) -> Option<Vec<String>> {
    let parts: Vec<String> = row
        .trim()
        .split('\u{2003}')
        .map(|part| part.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    if parts.len() > 1 {
        return Some(parts);
    }
    let pieces = gap_pieces(line_of(&parts[0], lines)?);
    (pieces.len() > 1).then_some(pieces)
}

/// The line of `lines` whose text (without raised glyphs) is `text`, all
/// spaces left out.
fn line_of<'a>(text: &str, lines: &'a [Line]) -> Option<&'a Line> {
    let compact: Vec<char> = text.chars().filter(|ch| !ch.is_whitespace()).collect();
    lines.iter().find(|line| {
        line.glyphs
            .iter()
            .filter(|glyph| glyph.script == 0)
            .flat_map(|glyph| glyph.text.chars())
            .filter(|ch| !ch.is_whitespace())
            .eq(compact.iter().copied())
    })
}

/// The text of a line cut at its wide gaps, without raised glyphs.
fn gap_pieces(line: &Line) -> Vec<String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut last_x1: Option<f32> = None;
    for glyph in line.glyphs.iter().filter(|glyph| glyph.script == 0) {
        let wide = last_x1.is_some_and(|x1| glyph.x0 - x1 > NAME_GAP_EM * glyph.size);
        if wide || pieces.is_empty() {
            pieces.push(String::new());
        } else if glyph.space_before {
            pieces.last_mut().unwrap().push(' ');
        }
        pieces.last_mut().unwrap().push_str(&glyph.text);
        last_x1 = Some(glyph.x1);
    }
    pieces.into_iter().map(|piece| piece.trim().to_string()).collect()
}

/// Whether a row of an author block that is not a name, an address or a
/// date is an affiliation: it starts with a capital or a digit ("based on
/// research in collaboration with" does not) and is not in brackets.
fn is_affiliation(text: &str) -> bool {
    text.starts_with(|ch: char| !ch.is_lowercase() && ch != '(') && text.chars().any(char::is_alphabetic)
}

/// Whether a row starts with a raised mark followed by text
/// ("¹Google Research", "∗equal technical contribution").
fn starts_with_mark(row: &[Span]) -> bool {
    let mut spans = row.iter().filter(|span| !span.text.trim().is_empty());
    spans.next().is_some_and(|span| span.style & SUP != 0) && spans.next().is_some_and(|span| span.style & SUP == 0)
}

/// The (mark, text) pairs of a row of marked texts
/// ("∗equal technical contribution, †equal advising").
fn marked_row(row: &[Span]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for span in row {
        if span.style & SUP != 0 && !span.text.trim().is_empty() {
            out.push((span.text.trim().to_string(), String::new()));
        } else if let Some((_, text)) = out.last_mut() {
            text.push_str(&span.text);
        }
    }
    out.into_iter()
        .map(|(mark, text)| (mark, text.trim().trim_end_matches([',', ';']).trim().to_string()))
        .filter(|(_, text)| !text.is_empty())
        .collect()
}

/// The texts that marks point to: the marked rows of the author blocks, and
/// the footnotes of the first page.
fn marked_texts(doc: &Document, rows: &[(String, String)]) -> Vec<(String, String)> {
    let mut marked = rows.to_vec();
    for block in doc.blocks.iter().filter(|block| block.page == 0) {
        if let BlockKind::Footnote { marker } = &block.kind
            && !marker.is_empty()
        {
            marked.push((marker.clone(), plain_of(&block.spans)));
        }
    }
    marked
}

/// The e-mail addresses in a row, those in braces made whole.
fn emails_in(text: &str) -> Vec<String> {
    let mut emails = Vec::new();
    let mut rest = text.to_string();
    for caps in BRACED_EMAILS.captures_iter(text) {
        for user in caps[1].split([',', ';']).map(str::trim).filter(|user| !user.is_empty()) {
            emails.push(format!("{user}@{}", &caps[2]));
        }
        rest = rest.replace(&caps[0], " ");
    }
    emails.extend(EMAIL.find_iter(&rest).map(|found| found.as_str().to_string()));
    emails
}

/// The date of a row that holds only a date ("December 1997"), or one of
/// "(Received ...; published 11 February 2016)" (the publication date).
fn row_date(text: &str) -> Option<String> {
    let lower = text.to_lowercase();
    let at = lower.find("published").unwrap_or(0);
    let caps = DATE.captures(&text[at..])?;
    let whole = caps.get(0).unwrap();
    let only_date = whole.as_str().len() + 2 >= text.trim().len();
    if !only_date && !lower.starts_with("(received") && at == 0 {
        return None;
    }
    let (day, month, year) = match caps.get(1) {
        Some(day) => (day.as_str().parse().ok(), month_number(&caps[2]), caps[3].parse().ok()),
        None => (
            caps.get(5).and_then(|day| day.as_str().parse().ok()),
            month_number(&caps[4]),
            caps[6].parse().ok(),
        ),
    };
    month?;
    iso_date(year, month, day)
}

/// The number of a month from its name or the first three letters of it.
fn month_number(name: &str) -> Option<u32> {
    let lower = name.to_lowercase();
    if lower.len() < 3 {
        return None;
    }
    MONTHS
        .iter()
        .position(|month| {
            month.starts_with(&lower) || (lower.len() >= 3 && month.starts_with(&lower[..3]) && lower.len() <= 4)
        })
        .map(|index| index as u32 + 1)
}

/// "YYYY-MM-DD", or "YYYY-MM" without a day.
fn iso_date(year: Option<u32>, month: Option<u32>, day: Option<u32>) -> Option<String> {
    let year = year?;
    let month = month?;
    Some(match day {
        Some(day) if (1..=31).contains(&day) => format!("{year:04}-{month:02}-{day:02}"),
        _ => format!("{year:04}-{month:02}"),
    })
}

/// The DOI printed on the first page, outside the references.
fn doi(doc: &Document) -> Option<String> {
    doc.blocks
        .iter()
        .filter(|block| block.page == 0 && !matches!(block.kind, BlockKind::Reference { .. }))
        .find_map(|block| {
            let link = block
                .spans
                .iter()
                .filter_map(|span| span.href.as_deref())
                .find_map(|href| href.find("doi.org/").map(|at| trim_id(&href[at + "doi.org/".len()..])));
            link.or_else(|| DOI.captures(&block.text()).map(|caps| trim_id(&caps[1])))
        })
}

/// The keywords: the words after "Keywords" or "Index Terms" at the start
/// of a block of the first pages.
fn keywords(doc: &Document) -> Vec<String> {
    doc.blocks
        .iter()
        .filter(|block| block.page < FRONT_PAGES_COUNT && !matches!(block.kind, BlockKind::Reference { .. }))
        .find_map(|block| {
            let text = plain_of(&block.spans);
            let head = KEYWORDS.find(&text)?;
            let words: Vec<String> = KEYWORD_SEPARATOR
                .split(&text[head.end()..])
                .map(|word| word.trim().trim_end_matches('.').trim().to_string())
                .filter(|word| !word.is_empty())
                .collect();
            (!words.is_empty()).then_some(words)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braced_addresses_are_made_whole() {
        assert_eq!(
            emails_in("{kahe, v-xiangz}@microsoft.com"),
            ["kahe@microsoft.com", "v-xiangz@microsoft.com"]
        );
        assert_eq!(
            emails_in("drob@mit.edu, shoyaida@fb.com"),
            ["drob@mit.edu", "shoyaida@fb.com"]
        );
    }

    #[test]
    fn dates_of_rows() {
        assert_eq!(row_date("December 1997").as_deref(), Some("1997-12"));
        assert_eq!(row_date("November 26, 2024").as_deref(), Some("2024-11-26"));
        assert_eq!(
            row_date("(Received 21 January 2016; published 11 February 2016)").as_deref(),
            Some("2016-02-11")
        );
        assert_eq!(row_date("Google Research, Brain Team"), None);
        assert_eq!(row_date("University of California, Santa Barbara, CA 93106"), None);
    }

    #[test]
    fn addresses_hold_names() {
        assert!(name_matches("Daniel A. Roberts", "drob"));
        assert!(name_matches("Sho Yaida", "shoyaida"));
        assert!(name_matches("Łukasz Kaiser", "lukaszkaiser"));
        assert!(!name_matches("Boris Hanin", "drob"));
    }

    #[test]
    fn marks_after_names() {
        let mut sup = Span::plain("∗,†");
        sup.style = SUP;
        let mut one = Span::plain("1");
        one.style = SUP;
        let row = [
            Span::plain("Alexey Dosovitskiy"),
            sup,
            Span::plain(", Lucas Beyer"),
            one,
            Span::plain(", and Jane Doe"),
        ];
        let authors = names_row(&row, &[]).unwrap();
        let names: Vec<_> = authors.iter().map(|a| (a.name.as_str(), a.marks.clone())).collect();
        assert_eq!(
            names,
            [
                ("Alexey Dosovitskiy", vec!["∗".to_string(), "†".to_string()]),
                ("Lucas Beyer", vec!["1".to_string()]),
                ("Jane Doe", vec![]),
            ]
        );
    }
}
