//! The `citation` field of a reference in the JSON output: its authors,
//! title, venue, year, volume, pages and identifiers, read from its text.
//!
//! The styles of reference lists differ between fields and publishers, so
//! the reader knows a few shapes and gives only what it is sure of:
//!
//! - a quoted title (IEEE): `A. Author and B. Author, “Title,” in Venue, 2017.`
//! - the year after the authors (ACL, APA, astronomy):
//!   `A. Author. 2016. Title. In Venue.`, `Author, A. (2015). Title.`,
//!   `Author, A., & Author, B. 2015, MNRAS, 454, 1958`
//! - sentences (most of the others): `A. Author, B. Author. Title. Venue, 2015.`,
//!   or a book title in italics after the authors: `A. Author, Title (Publisher, 1983).`
//! - initials glued to the name, without a period after the authors
//!   (`R.S.Hamilton A compactness property ... Amer. Jour. Math. 117 (1995), 545-572.`)
//!
//! Authors are written given names first ("Graves, Alex" is "Alex Graves").
//! The venue is the first text in italics after the title (or the words
//! before the volume, when nothing is in italics). A field the reader is not
//! sure of is left out, and a reference of which it understood nothing has
//! no `citation`. The field is read-only: it is not read back by
//! [`super::apply`].

use crate::model::{BOLD, ITALIC, Span};
use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

/// Most words of an author's name.
const NAME_MAX_WORDS_COUNT: usize = 6;
/// Most words of a surname written before the initials ("Dalla Vecchia,
/// C.").
const SURNAME_MAX_WORDS_COUNT: usize = 3;
/// Most bytes of an initial or a run of them ("U.-L.", "J.-P.", "A.").
const MAX_INITIAL_BYTES: usize = 6;
/// Most words of a venue that is not in italics.
const VENUE_MAX_WORDS_COUNT: usize = 15;
/// Fewest words of a quoted title.
const QUOTED_TITLE_MIN_WORDS_COUNT: usize = 2;
/// Years that can be the year of a reference.
const YEARS: std::ops::RangeInclusive<u16> = 1600..=2100;
/// Lower-case words that are part of a name ("Ludwig van Beethoven").
const NAME_PARTICLES: &[&str] = &[
    "van", "von", "der", "den", "de", "del", "della", "da", "das", "di", "do", "dos", "du", "la", "le", "ten", "ter",
    "bin", "ibn", "al", "el", "y", "zu",
];
/// Names of months, which are not venues ("Mar. 2023").
const MONTHS: &[&str] = &[
    "jan",
    "january",
    "feb",
    "february",
    "mar",
    "march",
    "apr",
    "april",
    "may",
    "jun",
    "june",
    "jul",
    "july",
    "aug",
    "august",
    "sep",
    "sept",
    "september",
    "oct",
    "october",
    "nov",
    "november",
    "dec",
    "december",
];
/// Words that a period follows without ending a sentence.
const ABBREVIATIONS: &[&str] = &["al", "Jr", "Sr", "St", "vs", "Dr", "Mr", "Mrs", "Ms", "Prof"];

/// A label at the start of the text: "[12]", "(3)", "12.", "[GRK17]"; or a
/// bullet.
static LEADING_LABEL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[[^\]]{1,24}\]|\(\d{1,4}\)|\d{1,4}\.|[•·∙▪◦‣-])\s*").unwrap());
/// A DOI.
pub(crate) static DOI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(10\.\d{4,9}/[^\s<>]+)").unwrap());
/// An arXiv identifier written as such (`arXiv:1706.03762v2`,
/// `arXiv preprint arXiv:hep-th/9711200`), or in the CoRR form
/// `abs/1409.0473`.
static ARXIV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\barxiv\s*:?\s*|\babs/)(\d{4}\.\d{4,5}(?:v\d+)?|[a-z][a-z\-]*(?:\.[a-z]{2})?/\d{7}(?:v\d+)?)")
        .unwrap()
});
/// An arXiv identifier in the address of an arXiv page.
static ARXIV_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)arxiv\.org/(?:abs|pdf)/(?:arxiv:)?(\d{4}\.\d{4,5}(?:v\d+)?|[a-z][a-z\-]*(?:\.[a-z]{2})?/\d{7}(?:v\d+)?)",
    )
    .unwrap()
});
/// A web address written in the text.
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^\s<>]+").unwrap());
/// A year, with the letter that tells apart two works of one year
/// ("2018b"), possibly in brackets.
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(?\b(\d{4})[a-z]?\b\)?").unwrap());
/// Authors written with initials, at the start of the text, as when no
/// period follows them ("R.S.Hamilton", "H.-D. Cao, B.Chow").
static GLUED_AUTHORS: LazyLock<Regex> = LazyLock::new(|| {
    let name = r"\p{Lu}\.(?:\s?-?\p{Lu}\.)*\s?\p{Lu}[\p{L}\p{M}’'\-]+";
    Regex::new(&format!(r"^{name}(?:\s*(?:,|\band\b|&)\s*{name})*")).unwrap()
});
/// One word of a name: an initial ("A.", "J.-P.", "A", "d."), a word with a
/// capital ("Abdel-rahman", "D’Hoker", "Łukasz"), initials glued to a
/// name ("R.S.Hamilton"), a suffix.
static NAME_WORD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:\p{Lu}\.(?:-?\p{Lu}\.)*|\p{Ll}\.|Md\.|(?:\p{Lu}\.)+\p{Lu}[\p{L}\p{M}’'\-]+|\p{Lu}|\p{Lu}[\p{L}\p{M}’'\-]*|[dl][’']\p{Lu}[\p{L}\p{M}’'\-]*|Jr\.?|Sr\.?|II|III|IV)$")
        .unwrap()
});
/// A word that starts with an initial: "A.", "J.-P.", "R.S.Hamilton", or a
/// capital alone ("R Kallosh").
static INITIAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Lu}(?:\.|$)").unwrap());
/// "et al." at the end of a list of authors, or "and others".
static ET_AL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i),?\s*(?:\bet\.?\s+al\b\.?|\band\s+others\b)").unwrap());
/// The words between the authors: "and", "&", with or without a comma.
static AND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",?\s+(?:and|&)\s+|;\s+").unwrap());
/// The year that ends a title ("Title, 2018, arXiv:...").
static TITLE_YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r",\s*\(?(?:1[6-9]|20)\d{2}[a-z]?\b").unwrap());
/// "vol. 12", "volume 12".
static VOLUME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bvol(?:ume)?\.?\s*(\d+)").unwrap());
/// "pp. 12–34", "pages 12–34", "p. 12".
static PAGES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:pp?\.|pages?)\s*(\d+(?:\s*[–—-]+\s*\d+)?)").unwrap());
/// "12:2121–2159", "19(2):313–330": volume, issue, pages.
static VOLUME_PAGES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s,])(\d+)\s*(?:\(\d+\))?\s*:\s*(\d+(?:\s*[–—-]+\s*\d+)?)\b").unwrap());
/// "117 (1995), 545-572", "(1995) 469–505", "B305 (1988) 483": volume,
/// year, pages.
static YEAR_PAGES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:\b([A-Z]?\d+)\s*)?\((?:1[6-9]|20)\d{2}\)\s*,?\s*(\d+(?:\s*[–—-]+\s*\d+)?)\b").unwrap()
});
/// ", 454, 1958" (astronomy) or " 116, 061102" (physics) after the venue:
/// volume and first page.
static COMMA_VOLUME_PAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*,?\s*(\d+)\s*,\s*([A-Z]?\d+)\b").unwrap());

/// What a reference cites.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub(crate) struct Citation {
    /// The authors, given names first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) authors: Vec<String>,
    /// The list of authors ends with "et al.".
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(crate) et_al: bool,
    /// Title of the work.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// Where it was published: journal, proceedings, publisher.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) venue: Option<String>,
    /// Year of publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) year: Option<u16>,
    /// Volume of the journal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) volume: Option<String>,
    /// Pages ("2121–2159") or article number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) pages: Option<String>,
    /// DOI, without `https://doi.org/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) doi: Option<String>,
    /// arXiv identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) arxiv: Option<String>,
    /// Another web address: a link of the reference or one written in it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) url: Option<String>,
}

/// The text of a reference: its characters with white space made single
/// spaces, the style of each byte, and the external links.
struct Text {
    /// The text.
    src: String,
    /// The style bits of each byte of `src`.
    style: Vec<u8>,
    /// External links of the spans, in order.
    links: Vec<String>,
}

impl Text {
    /// The text of `spans`.
    fn new(spans: &[Span]) -> Self {
        let mut text = Text {
            src: String::new(),
            style: Vec::new(),
            links: Vec::new(),
        };
        for span in spans {
            if let Some(href) = &span.href
                && !href.starts_with('#')
                && !text.links.contains(href)
            {
                text.links.push(href.clone());
            }
            for ch in span.text.chars() {
                if ch == crate::render::assets::MARK {
                    continue;
                }
                if ch.is_whitespace() {
                    if text.src.is_empty() || text.src.ends_with(' ') {
                        continue;
                    }
                    text.push(' ', span.style);
                } else {
                    text.push(ch, span.style);
                }
            }
        }
        while text.src.ends_with(' ') {
            text.src.pop();
            text.style.pop();
        }
        text
    }

    /// Appends a character of style `style`.
    fn push(&mut self, ch: char, style: u8) {
        self.src.push(ch);
        self.style.extend(std::iter::repeat_n(style, ch.len_utf8()));
    }

    /// The text from byte `start` on.
    fn from(&self, start: usize) -> Self {
        Text {
            src: self.src[start..].to_string(),
            style: self.style[start..].to_vec(),
            links: self.links.clone(),
        }
    }

    /// The byte ranges in a style of `flag`, spaces between two of them
    /// joining them, without spaces at their ends.
    fn runs(&self, flag: u8) -> Vec<(usize, usize)> {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for (i, ch) in self.src.char_indices() {
            if self.style[i] & flag == 0 || ch == ' ' {
                continue;
            }
            let end = i + ch.len_utf8();
            match runs.last_mut() {
                Some(last) if self.src[last.1..i].trim().is_empty() => last.1 = end,
                _ => runs.push((i, end)),
            }
        }
        runs
    }
}

/// What a reference cites, read from its spans; `None` when nothing could
/// be read from it.
pub(crate) fn parse(spans: &[Span]) -> Option<Citation> {
    let full = Text::new(spans);
    let label_len = LEADING_LABEL.find(&full.src).map_or(0, |found| found.end());
    let text = full.from(label_len);
    let src = text.src.as_str();
    let mut cite = Citation::default();
    let ids = identifiers(&text, &mut cite);
    let italic = text.runs(ITALIC);

    // Where the authors end, and the title (when it is known) starts.
    let mut title_start = None;
    let mut year_after_authors = None;
    if let Some((open, inner, close)) = quoted_title(src) {
        set_authors(&mut cite, &src[..open]);
        cite.title = Some(clean(inner));
        read_rest(&mut cite, &text, &italic, close, &ids);
    } else if let Some((authors_end, year, delimiter, after)) = year_after(src) {
        set_authors(&mut cite, &src[..authors_end]);
        year_after_authors = Some(year);
        if delimiter == '.' {
            title_start = Some(after);
        } else {
            // "2015, MNRAS, 454, 1958": no title.
            read_venue_after_year(&mut cite, src, after);
        }
    } else {
        match authors_end(src, &italic) {
            Some((end, AuthorsEnd::Sentence | AuthorsEnd::BeforeItalic)) => {
                set_authors(&mut cite, &src[..end]);
                title_start = Some(end);
            }
            // "D.Bakry, M.Emery Diffusions hypercontractives. Seminaire":
            // authors without a period after them.
            Some((end, _))
                if let Some(glued) = GLUED_AUTHORS.find(src)
                    && glued.end() > end
                    && set_authors(&mut cite, glued.as_str()) =>
            {
                title_start = Some(glued.end());
            }
            Some((end, AuthorsEnd::Comma)) => {
                // "B. P. Abbott et al., Phys. Rev. Lett. 116, 061102 (2016)":
                // no title.
                set_authors(&mut cite, &src[..end]);
                read_rest(&mut cite, &text, &italic, end, &ids);
            }
            None => {
                if let Some(glued) = GLUED_AUTHORS.find(src)
                    && set_authors(&mut cite, glued.as_str())
                {
                    title_start = Some(glued.end());
                }
            }
        }
    }
    if let Some(start) = title_start {
        let start = skip_punctuation(src, start);
        let title_end = read_title(&mut cite, src, start, &italic);
        read_rest(&mut cite, &text, &italic, title_end, &ids);
    }
    cite.year = year_after_authors
        .or_else(|| last_year(src, &ids))
        .or_else(|| cite.arxiv.as_deref().and_then(arxiv_year));
    let found = !cite.authors.is_empty() || cite.title.is_some() || cite.doi.is_some() || cite.arxiv.is_some();
    found.then_some(cite)
}

/// Reads the DOI, arXiv identifier and web address, from the links and the
/// text. Returns the byte ranges of the text they take, where no year is.
fn identifiers(text: &Text, cite: &mut Citation) -> Vec<(usize, usize)> {
    let mut taken = Vec::new();
    for link in &text.links {
        if let Some(at) = link.find("doi.org/") {
            cite.doi.get_or_insert_with(|| trim_id(&link[at + "doi.org/".len()..]));
        } else if let Some(caps) = ARXIV_URL.captures(link) {
            cite.arxiv.get_or_insert_with(|| caps[1].to_string());
        } else {
            cite.url.get_or_insert_with(|| link.clone());
        }
    }
    if let Some(caps) = DOI.captures(&text.src) {
        let found = caps.get(1).unwrap();
        cite.doi.get_or_insert_with(|| trim_id(found.as_str()));
        taken.push((found.start(), found.end()));
    }
    for caps in ARXIV.captures_iter(&text.src) {
        let found = caps.get(1).unwrap();
        cite.arxiv.get_or_insert_with(|| found.as_str().to_string());
        taken.push((found.start(), found.end()));
    }
    for found in URL.find_iter(&text.src) {
        taken.push((found.start(), found.end()));
        let url = trim_id(found.as_str());
        if cite.url.is_none() && !url.contains("doi.org/") && !url.contains("arxiv.org/") {
            cite.url = Some(url);
        }
    }
    taken
}

/// An identifier without the punctuation that follows it in a sentence.
pub(crate) fn trim_id(id: &str) -> String {
    id.trim_end_matches(['.', ',', ';', ':', ')', ']']).to_string()
}

/// A title in quotation marks: the byte offsets of the opening mark and of
/// the end of the closing one, and the text between them.
fn quoted_title(src: &str) -> Option<(usize, &str, usize)> {
    for (open_mark, close_mark) in [('“', '”'), ('"', '"')] {
        let Some(open) = src.find(open_mark) else {
            continue;
        };
        let inner_start = open + open_mark.len_utf8();
        let Some(len) = src[inner_start..].find(close_mark) else {
            continue;
        };
        let inner = &src[inner_start..inner_start + len];
        if inner.split_whitespace().count() >= QUOTED_TITLE_MIN_WORDS_COUNT {
            return Some((open, inner, inner_start + len + close_mark.len_utf8()));
        }
    }
    None
}

/// A year right after the list of authors: where the authors end, the
/// year, the punctuation after it and where the text after it starts.
fn year_after(src: &str) -> Option<(usize, u16, char, usize)> {
    let found = YEAR.captures_iter(src).find(|caps| {
        let year = &caps[1];
        year.parse::<u16>().is_ok_and(|year| YEARS.contains(&year))
    })?;
    let whole = found.get(0).unwrap();
    let authors = src[..whole.start()].trim_end_matches([' ', ',']);
    if authors.is_empty() || split_names(authors).is_none() {
        return None;
    }
    let rest = &src[whole.end()..];
    let delimiter = rest.chars().next().filter(|ch| matches!(ch, '.' | ',' | ':' | ';'))?;
    let year = found[1].parse().ok()?;
    Some((authors.len(), year, delimiter, whole.end() + delimiter.len_utf8()))
}

/// What ends a list of authors that nothing else marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthorsEnd {
    /// The end of a sentence: the title follows.
    Sentence,
    /// A comma before text in italics: the title of a book.
    BeforeItalic,
    /// A comma before what is not a name: the venue follows, without a
    /// title.
    Comma,
}

/// Where the list of authors ends when nothing marks it: before a comma
/// and text in italics (a book title); at a period, the last one before
/// which the text reads as names (a period after an initial too:
/// "Zeiler, Matthew D. Adadelta"); or else at the last comma of the first
/// sentence before which the text reads as names. `None` when no start of
/// the text reads as names.
fn authors_end(src: &str, italic: &[(usize, usize)]) -> Option<(usize, AuthorsEnd)> {
    let sentence = sentence_end(src, 0).unwrap_or(src.len());
    if let Some(&(start, _)) = italic.first()
        && start < sentence
        && let Some(before) = src[..start].trim_end().strip_suffix(',')
        && split_names(before).is_some()
    {
        return Some((before.len(), AuthorsEnd::BeforeItalic));
    }
    let bytes = src.as_bytes();
    let mut periods = src
        .match_indices('.')
        .map(|(at, _)| at + 1)
        .filter(|&end| bytes.get(end).is_none_or(|&next| next == b' '));
    if let Some(end) = periods.rfind(|&end| split_names(&src[..end]).is_some()) {
        return Some((end, AuthorsEnd::Sentence));
    }
    src[..sentence]
        .rmatch_indices(',')
        .map(|(at, _)| at)
        .find(|&at| split_names(&src[..at]).is_some())
        .map(|at| (at, AuthorsEnd::Comma))
}

/// The byte offset of the punctuation that ends the sentence starting at
/// `from`: a period, question or exclamation mark followed by a space (or
/// the end), the period not after an initial or an abbreviation.
fn sentence_end(src: &str, from: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    for (i, ch) in src[from..].char_indices() {
        let at = from + i;
        if !matches!(ch, '.' | '?' | '!') || bytes.get(at + 1).is_some_and(|&next| next != b' ') {
            continue;
        }
        if ch == '.' {
            let word: String = src[..at]
                .chars()
                .rev()
                .take_while(|ch| ch.is_alphabetic())
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            if word.chars().count() == 1 || ABBREVIATIONS.contains(&word.as_str()) {
                continue;
            }
        }
        return Some(at);
    }
    None
}

/// The offset after the punctuation and spaces at `at`.
fn skip_punctuation(src: &str, at: usize) -> usize {
    at + src[at..].len() - src[at..].trim_start_matches(['.', ',', ':', ';', ' ']).len()
}

/// Reads the title starting at `start`: the text in italics there (a book),
/// or the sentence, up to text in italics or a year after a comma. Returns
/// where the title ends.
fn read_title(cite: &mut Citation, src: &str, start: usize, italic: &[(usize, usize)]) -> usize {
    if let Some(&(_, end)) = italic.iter().find(|&&(run_start, _)| run_start == start) {
        cite.title = Some(clean(&src[start..end]));
        return end;
    }
    let mut end = sentence_end(src, start).map_or(src.len(), |at| {
        // A question mark is part of the title.
        if src[at..].starts_with(['?', '!']) { at + 1 } else { at }
    });
    if let Some(&(run_start, _)) = italic
        .iter()
        .find(|&&(run_start, _)| run_start > start && run_start < end)
    {
        end = run_start;
    }
    if let Some(found) = TITLE_YEAR.find(&src[start..end]) {
        end = start + found.start();
    }
    let title = clean(
        src[start..end]
            .trim_end()
            .trim_end_matches(" In")
            .trim_end_matches(" in"),
    );
    // "R. Schilling (unpublished)": a remark, not a title.
    if !title.is_empty() && !title.starts_with('(') {
        cite.title = Some(title);
    }
    end
}

/// Reads the venue, volume and pages after the title (which ends at `from`).
fn read_rest(cite: &mut Citation, text: &Text, italic: &[(usize, usize)], from: usize, ids: &[(usize, usize)]) {
    let src = text.src.as_str();
    let rest = &src[from..];
    let venue_end = if let Some(&(start, end)) = italic.iter().find(|&&(start, _)| start >= from) {
        let venue = venue_text(&src[start..end], cite.arxiv.is_some());
        if !venue.is_empty() {
            cite.venue = Some(venue);
        }
        end
    } else {
        plain_venue(cite, src, from)
    };
    // A bold number is a volume ("Network 6 no. 3").
    let bold = text.runs(BOLD);
    if let Some(&(start, end)) = bold.iter().find(|&&(start, _)| start >= from)
        && src[start..end].chars().all(|ch| ch.is_ascii_digit())
    {
        cite.volume = Some(src[start..end].to_string());
    }
    if let Some(caps) = VOLUME.captures(rest) {
        cite.volume.get_or_insert_with(|| caps[1].to_string());
    }
    if let Some(caps) = PAGES.captures(rest) {
        cite.pages = Some(pages(&caps[1]));
    }
    let outside_ids = |at: usize| !ids.iter().any(|&(start, end)| at >= start && at < end);
    if let Some(caps) = VOLUME_PAGES
        .captures_iter(rest)
        .find(|caps| outside_ids(from + caps.get(1).unwrap().start()))
    {
        cite.volume.get_or_insert_with(|| caps[1].to_string());
        cite.pages.get_or_insert_with(|| pages(&caps[2]));
    }
    if let Some(caps) = YEAR_PAGES.captures(rest) {
        if let Some(volume) = caps.get(1) {
            cite.volume.get_or_insert_with(|| volume.as_str().to_string());
        }
        cite.pages.get_or_insert_with(|| pages(&caps[2]));
    }
    if venue_end <= src.len()
        && let Some(caps) = COMMA_VOLUME_PAGE.captures(&src[venue_end..])
        // "Venue, 19, 2018": the second number is the year.
        && !(caps[2].len() == 4 && caps[2].parse().is_ok_and(|year: u16| YEARS.contains(&year)))
    {
        cite.volume.get_or_insert_with(|| caps[1].to_string());
        cite.pages.get_or_insert_with(|| caps[2].to_string());
    }
}

/// "2015, MNRAS, 454, 1958": the venue after the year, then the volume and
/// the first page.
fn read_venue_after_year(cite: &mut Citation, src: &str, after: usize) {
    let rest = &src[after..];
    let venue = rest.split(',').next().unwrap_or("").trim();
    if !venue.is_empty()
        && venue.chars().any(char::is_alphabetic)
        && venue.split_whitespace().count() <= VENUE_MAX_WORDS_COUNT
    {
        cite.venue = Some(venue_text(venue, cite.arxiv.is_some()));
        let venue_end = after + rest.find(venue).unwrap_or(0) + venue.len();
        if let Some(caps) = COMMA_VOLUME_PAGE.captures(&src[venue_end..]) {
            cite.volume = Some(caps[1].to_string());
            cite.pages = Some(caps[2].to_string());
        }
    }
}

/// The venue when no text is in italics: the words after the title up to
/// the first number, bracket or "pp." ("Amer. Jour. Math. 117 (1995)").
/// Returns where it ends.
fn plain_venue(cite: &mut Citation, src: &str, from: usize) -> usize {
    let start = skip_punctuation(src, from);
    let rest = src[start..]
        .strip_prefix("In ")
        .or_else(|| src[start..].strip_prefix("in "))
        .unwrap_or(&src[start..]);
    let start = src.len() - rest.len();
    let mut end = start;
    let mut words = 0;
    for word in rest.split(' ') {
        // "Nucl. Phys. B305 (1988)": a volume with a letter.
        let volume = word.len() > 1
            && word.starts_with(|ch: char| ch.is_ascii_uppercase())
            && word[1..].chars().all(|ch| ch.is_ascii_digit());
        let stop = volume
            || word.starts_with(|ch: char| ch.is_ascii_digit() || ch == '(' || ch == '[')
            || ["vol.", "pp.", "pages", "p.", "URL", "arXiv", "doi:", "DOI:"].contains(&word)
            || word.starts_with("http");
        if stop || word.is_empty() {
            break;
        }
        end += word.len() + 1;
        words += 1;
        if word.ends_with(',') {
            break;
        }
    }
    let end = end.min(src.len());
    let venue = clean(&src[start..end]);
    let first_capital = venue.starts_with(|ch: char| ch.is_uppercase());
    let month = MONTHS.contains(&venue.to_lowercase().as_str());
    if first_capital && !month && words <= VENUE_MAX_WORDS_COUNT {
        cite.venue = Some(venue);
    }
    src[..end].trim_end_matches([' ', ',']).len()
}

/// The venue from text in italics: without an arXiv identifier in it, and
/// "arXiv" for an arXiv preprint.
fn venue_text(text: &str, has_arxiv: bool) -> String {
    let without_id = ARXIV.replace_all(text, "");
    let venue = clean(&without_id);
    let lower = venue.to_lowercase();
    if has_arxiv && (venue.is_empty() || lower == "arxiv" || lower == "arxiv preprint" || lower == "arxiv e-prints") {
        "arXiv".to_string()
    } else {
        venue
    }
}

/// The year an arXiv identifier was given: "1706.03762" is of 2017,
/// "hep-th/9711200" of 1997. A reference with no year written is dated so.
fn arxiv_year(id: &str) -> Option<u16> {
    let digits = match id.rsplit_once('/') {
        Some((_, number)) => number,
        None => id,
    };
    let yy: u16 = digits.get(..2)?.parse().ok()?;
    // Old identifiers began in 1991, new ones in 2007.
    Some(if yy >= 91 { 1900 + yy } else { 2000 + yy })
}

/// Pages without the spaces around the dash.
fn pages(text: &str) -> String {
    text.split_whitespace().collect()
}

/// The last year in `src`, outside identifiers and page ranges.
fn last_year(src: &str, ids: &[(usize, usize)]) -> Option<u16> {
    YEAR.captures_iter(src)
        .filter_map(|caps| {
            let found = caps.get(1).unwrap();
            let inside_id = ids
                .iter()
                .any(|&(start, end)| found.start() >= start && found.start() < end);
            let before = src[..found.start()].chars().next_back();
            let after = src[found.end()..].chars().next();
            let in_range = matches!(before, Some('–' | '—' | '-' | ':' | '/' | '.'))
                || matches!(after, Some('–' | '—' | '-' | '/'))
                || (after == Some('.') && src[found.end() + 1..].starts_with(|ch: char| ch.is_ascii_digit()));
            let year: u16 = found.as_str().parse().ok()?;
            (!inside_id && !in_range && YEARS.contains(&year)).then_some(year)
        })
        .last()
}

/// Sets the authors of `cite` from the text of the list, if it reads as
/// one. Returns whether it did.
fn set_authors(cite: &mut Citation, text: &str) -> bool {
    match split_names(text) {
        Some((authors, et_al)) => {
            cite.authors = authors;
            cite.et_al = et_al;
            true
        }
        None => false,
    }
}

/// The names of a list of authors, given names first, and whether it ends
/// with "et al."; `None` when the text is not a list of names.
pub(crate) fn split_names(text: &str) -> Option<(Vec<String>, bool)> {
    let et_al = ET_AL.is_match(text);
    let text = ET_AL.replace_all(text, "");
    let text = AND.replace_all(text.trim(), ", ");
    let text = text.trim().trim_end_matches([',', ' ']);
    // A period after the last name ends the list, unless it ends an initial.
    let last_word = text.rsplit([' ', '.']).find(|word| !word.is_empty()).unwrap_or("");
    let text = if last_word.chars().filter(|ch| ch.is_alphabetic()).count() > 1
        && !text.ends_with("Jr.")
        && !text.ends_with("Sr.")
    {
        text.trim_end_matches('.')
    } else {
        text
    };
    let pieces: Vec<&str> = text
        .split(',')
        .map(str::trim)
        .filter(|piece| !piece.is_empty())
        .collect();
    if pieces.is_empty() {
        return None;
    }
    let names: Vec<String> = if is_inverted(&pieces) {
        pieces
            .chunks(2)
            .map(|pair| format!("{} {}", pair[1], pair[0]))
            .collect()
    } else {
        pieces.iter().map(|piece| (*piece).to_string()).collect()
    };
    // Single words are names only alone ("OpenAI") or with initials glued
    // to them ("B.Chow"): in a list they are words of a venue ("B. P.
    // Abbott, Phys").
    // "L. Blanchet, Living Rev. Relativity": a list of names that start
    // with initials does not end with one that does not ("Living Rev" is
    // a journal); "Diederik P. Kingma and Jimmy Ba" starts with none.
    let initialed = |name: &String| {
        name.split_whitespace()
            .next()
            .is_some_and(|word| INITIAL.is_match(word))
    };
    if let Some((last, others)) = names.split_last()
        && !others.is_empty()
        && others.iter().all(initialed)
        && !initialed(last)
    {
        return None;
    }
    let single_ok = names.len() == 1;
    names
        .iter()
        .all(|name| is_name(name) && (single_ok || name.split_whitespace().count() > 1 || name.contains('.')))
        .then_some((names, et_al))
}

/// Whether the pieces between commas are surnames and given names in turn
/// ("Graves, Alex, Mohamed, Abdel-rahman", "Mead, A. J., Peacock, J. A.").
fn is_inverted(pieces: &[&str]) -> bool {
    if !pieces.len().is_multiple_of(2) {
        return false;
    }
    // Given names as initials only ("J. M.") allow surnames of several
    // words ("Zorrilla Matilla, J. M.", "Van Waerbeke, L.").
    let initials = pieces.chunks(2).all(|pair| {
        pair[1]
            .split_whitespace()
            .all(|word| INITIAL.is_match(word) && word.len() <= MAX_INITIAL_BYTES)
    });
    pieces.chunks(2).all(|pair| {
        let surname: Vec<&str> = pair[0].split_whitespace().collect();
        let surname_ok = surname.split_last().is_some_and(|(_, rest)| {
            rest.iter().all(|word| NAME_PARTICLES.contains(word))
                || (initials
                    && surname.len() <= SURNAME_MAX_WORDS_COUNT
                    && rest.iter().all(|word| word.starts_with(char::is_uppercase)))
        });
        surname_ok && !pair[0].contains('.')
    })
}

/// Whether `name` reads as the name of a person or a group: a few words,
/// each an initial, a word with a capital or a particle, with a capital.
pub(crate) fn is_name(name: &str) -> bool {
    let words: Vec<&str> = name.split_whitespace().collect();
    !words.is_empty()
        && words.len() <= NAME_MAX_WORDS_COUNT
        && words.iter().any(|word| word.starts_with(char::is_uppercase))
        // Initials alone are not a name ("B. P." of "B. P. Abbott").
        && words
            .iter()
            .any(|word| !word.ends_with('.') && word.chars().filter(|ch| ch.is_alphabetic()).count() > 1)
        && words
            .iter()
            .all(|word| NAME_WORD.is_match(word) || NAME_PARTICLES.contains(word))
}

/// Text without the punctuation and spaces at its ends.
fn clean(text: &str) -> String {
    text.trim()
        .trim_end_matches([',', '.', ';', ':', ' '])
        .trim_start_matches([',', '.', ';', ':', ' '])
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spans from text where `_..._` is in italics and `*...*` in bold.
    fn spans(text: &str) -> Vec<Span> {
        let mut out = Vec::new();
        let mut style = 0;
        let mut current = String::new();
        for ch in text.chars() {
            let flag = match ch {
                '_' => ITALIC,
                '*' => BOLD,
                _ => {
                    current.push(ch);
                    continue;
                }
            };
            let mut span = Span::plain(std::mem::take(&mut current));
            span.style = style;
            out.push(span);
            style ^= flag;
        }
        let mut span = Span::plain(current);
        span.style = style;
        out.push(span);
        out
    }

    fn cite(text: &str) -> Citation {
        parse(&spans(text)).expect(text)
    }

    #[test]
    fn ieee_with_a_quoted_title() {
        let got = cite(
            "[8] P. F. Felzenszwalb, R. B. Girshick, D. McAllester, and D. Ramanan, “Object detection with discriminatively trained part-based models,” _IEEE Transactions on Pattern Analysis and Machine Intelligence (TPAMI)_, 2010.",
        );
        assert_eq!(
            got.authors,
            ["P. F. Felzenszwalb", "R. B. Girshick", "D. McAllester", "D. Ramanan"]
        );
        assert_eq!(
            got.title.as_deref(),
            Some("Object detection with discriminatively trained part-based models")
        );
        assert_eq!(
            got.venue.as_deref(),
            Some("IEEE Transactions on Pattern Analysis and Machine Intelligence (TPAMI)")
        );
        assert_eq!(got.year, Some(2010));
    }

    #[test]
    fn surnames_first_are_turned_round() {
        let got = cite(
            "Duchi, John, Hazan, Elad, and Singer, Yoram. Adaptive subgradient methods for online learning and stochastic optimization. _The Journal of Machine Learning Research_, 12:2121–2159, 2011.",
        );
        assert_eq!(got.authors, ["John Duchi", "Elad Hazan", "Yoram Singer"]);
        assert_eq!(
            got.title.as_deref(),
            Some("Adaptive subgradient methods for online learning and stochastic optimization")
        );
        assert_eq!(got.venue.as_deref(), Some("The Journal of Machine Learning Research"));
        assert_eq!(
            (got.volume.as_deref(), got.pages.as_deref()),
            (Some("12"), Some("2121–2159"))
        );
        assert_eq!(got.year, Some(2011));
        let got = cite("Kingma, Diederik P and Welling, Max. Auto-Encoding Variational Bayes. In _ICLR_, 2013.");
        assert_eq!(got.authors, ["Diederik P Kingma", "Max Welling"]);
        assert_eq!(got.title.as_deref(), Some("Auto-Encoding Variational Bayes"));
        assert_eq!(got.venue.as_deref(), Some("ICLR"));
    }

    #[test]
    fn year_after_the_authors() {
        let got = cite(
            "Pranav Rajpurkar, Jian Zhang, Konstantin Lopyrev, and Percy Liang. 2016. Squad: 100,000+ questions for machine comprehension of text. In _Proceedings of EMNLP_, pages 2383–2392.",
        );
        assert_eq!(got.authors.len(), 4);
        assert_eq!(got.year, Some(2016));
        assert_eq!(
            got.title.as_deref(),
            Some("Squad: 100,000+ questions for machine comprehension of text")
        );
        assert_eq!(got.venue.as_deref(), Some("Proceedings of EMNLP"));
        assert_eq!(got.pages.as_deref(), Some("2383–2392"));
        let got =
            cite("Mead, A. J., Peacock, J. A., Heymans, C., Joudaki, S., & Heavens, A. F. 2015, MNRAS, 454, 1958");
        assert_eq!(
            got.authors,
            [
                "A. J. Mead",
                "J. A. Peacock",
                "C. Heymans",
                "S. Joudaki",
                "A. F. Heavens"
            ]
        );
        assert_eq!(got.year, Some(2015));
        assert_eq!(got.title, None);
        assert_eq!(got.venue.as_deref(), Some("MNRAS"));
        assert_eq!(
            (got.volume.as_deref(), got.pages.as_deref()),
            (Some("454"), Some("1958"))
        );
        let got = cite("Duffy, A. R., Schaye, J., Kay, S. T., & Dalla Vecchia, C. 2008, MNRAS, 390, L64");
        assert_eq!(
            got.authors,
            ["A. R. Duffy", "J. Schaye", "S. T. Kay", "C. Dalla Vecchia"]
        );
        assert_eq!(got.venue.as_deref(), Some("MNRAS"));
        let got = cite("Harnois-Déraps, J. & Pen, U.-L. 2013, MNRAS, 431, 3349");
        assert_eq!(got.authors, ["J. Harnois-Déraps", "U.-L. Pen"]);
        let got = cite("Hildebrandt, H., Viola, M., Heymans, C., et al. 2017, MNRAS, 465, 1454");
        assert!(got.et_al);
        assert_eq!(got.authors, ["H. Hildebrandt", "M. Viola", "C. Heymans"]);
    }

    #[test]
    fn arxiv_preprints() {
        let got =
            cite("[42] R. K. Srivastava, K. Greff, and J. Schmidhuber. Highway networks. _arXiv:1505.00387_, 2015.");
        assert_eq!(got.title.as_deref(), Some("Highway networks"));
        assert_eq!(got.arxiv.as_deref(), Some("1505.00387"));
        assert_eq!(got.venue.as_deref(), Some("arXiv"));
        assert_eq!(got.year, Some(2015));
        let got = cite(
            "Jacob Devlin, Ming-Wei Chang, Kenton Lee, and Kristina Toutanova. Bert: Pre-training of deep bidirectional transformers for language understanding, 2018, arXiv:1810.04805. 2",
        );
        assert_eq!(
            got.title.as_deref(),
            Some("Bert: Pre-training of deep bidirectional transformers for language understanding")
        );
        assert_eq!(got.year, Some(2018));
        assert_eq!(got.arxiv.as_deref(), Some("1810.04805"));
        let got = cite(
            "Dzmitry Bahdanau, Kyunghyun Cho, and Yoshua Bengio. Neural machine translation. _CoRR_, abs/1409.0473, 2014.",
        );
        assert_eq!(got.venue.as_deref(), Some("CoRR"));
        assert_eq!(got.arxiv.as_deref(), Some("1409.0473"));
        assert_eq!(got.year, Some(2014));
        // No year written: the year of the arXiv identifier.
        let got =
            cite("[23] D. Hendrycks and K. Gimpel, “Gaussian Error Linear Units (GELUs),” arXiv:1606.08415 [cs.LG].");
        assert_eq!(got.year, Some(2016));
        assert_eq!(arxiv_year("math.DG/0211065"), Some(2002));
        assert_eq!(arxiv_year("hep-th/9711200"), Some(1997));
    }

    #[test]
    fn books_and_glued_initials() {
        let got = cite(
            "• C. Nash and S. Sen, _Topology and Geometry for Physicists _(Academic Press, 1983) [***]. Includes homotopy.",
        );
        assert_eq!(got.authors, ["C. Nash", "S. Sen"]);
        assert_eq!(got.title.as_deref(), Some("Topology and Geometry for Physicists"));
        assert_eq!(got.year, Some(1983));
        let got = cite(
            "[H 9] R.S.Hamilton A compactness property for solutions of the Ricci flow. Amer. Jour. Math. 117 (1995), 545-572.",
        );
        assert_eq!(got.authors, ["R.S.Hamilton"]);
        assert_eq!(
            got.title.as_deref(),
            Some("A compactness property for solutions of the Ricci flow")
        );
        assert_eq!(got.venue.as_deref(), Some("Amer. Jour. Math"));
        assert_eq!(
            (got.volume.as_deref(), got.pages.as_deref()),
            (Some("117"), Some("545-572"))
        );
        assert_eq!(got.year, Some(1995));
    }

    #[test]
    fn identifiers_come_from_links_too() {
        let mut spans = spans("B. P. Abbott et al., Phys. Rev. Lett. 116, 061102 (2016).");
        spans[0].href = Some("http://dx.doi.org/10.1103/PhysRevLett.116.061102".into());
        let got = parse(&spans).unwrap();
        assert_eq!(got.doi.as_deref(), Some("10.1103/PhysRevLett.116.061102"));
        assert_eq!(got.year, Some(2016));
        assert_eq!(
            (got.authors.as_slice(), got.et_al),
            (&["B. P. Abbott".to_string()][..], true)
        );
        assert_eq!(got.title, None);
        assert_eq!(got.venue.as_deref(), Some("Phys. Rev. Lett"));
        assert_eq!(got.volume.as_deref(), Some("116"));
        let got = cite("[11] H. Nicolai, E. Sezgin and Y. Tanii, Nucl. Phys. B305 (1988) 483.");
        assert_eq!(got.authors, ["H. Nicolai", "E. Sezgin", "Y. Tanii"]);
        assert_eq!(got.venue.as_deref(), Some("Nucl. Phys"));
        assert_eq!(
            (got.volume.as_deref(), got.pages.as_deref(), got.year),
            (Some("B305"), Some("483"), Some(1988))
        );
        let got = cite("[12] L. Blanchet, Living Rev. Relativity 17, 2 (2014).");
        assert_eq!(got.authors, ["L. Blanchet"]);
        assert_eq!(got.title, None);
        assert_eq!(got.venue.as_deref(), Some("Living Rev. Relativity"));
        assert_eq!((got.volume.as_deref(), got.pages.as_deref()), (Some("17"), Some("2")));
        let got = cite("[58] A. Chamseddine, S. Ferrara, G. Gibbons and R Kallosh, Phys. Rev. D55 (1997) 3647.");
        assert_eq!(got.authors.len(), 4);
        let got = cite("Diederik P. Kingma and Jimmy Ba. Adam: A method for stochastic optimization. In ICLR, 2015.");
        assert_eq!(got.authors, ["Diederik P. Kingma", "Jimmy Ba"]);
        let got = cite("[50] R. Schilling (unpublished).");
        assert_eq!(
            (got.authors.as_slice(), got.title),
            (&["R. Schilling".to_string()][..], None)
        );
    }

    #[test]
    fn prose_is_not_a_reference() {
        assert_eq!(
            parse(&spans(
                "squared exponential form, which specifies the covariance between the functions as"
            )),
            None
        );
    }
}
