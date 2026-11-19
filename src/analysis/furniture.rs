//! Removal of page furniture: running heads and feet, page numbers, margin
//! line numbers and the arXiv margin stamp.
//!
//! Furniture is recognised by repetition across pages (the same text, with
//! digits ignored, in the head or foot band of several pages), by shape
//! (page numbers, columns of margin line numbers) and by position (outside
//! the block of running text). Removed lines are not thrown away: they are
//! returned per page so that the position-preserving renderers can still
//! draw them.

use super::page::PageData;
use crate::common::geom::{Rect, weighted_mode};
use crate::model::{Line, LineGlyph};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// A page id of a journal page: digits joined by a hyphen ("061102-3",
/// "e2014-12").
static PAGE_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]?\d{2,}-\d{1,4}$").unwrap());

/// The foot a preprint class prints on its first page: "Preprint submitted
/// to Physics Letters B" (elsarticle), "Preprint. Under review." (NeurIPS).
static PREPRINT_FOOT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?i:preprint submitted to\b|preprint\. under review)").unwrap());

/// A printer's position mark: underscores, then an optional signed number
/// ("___-1", "___0", "___+1").
static PRINTER_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^_+[-+]?\d*$").unwrap());

/// Characters that separate a page number from the title in a running
/// head ("Preface | xv").
const RUNNING_HEAD_SEPARATORS: &str = "|\u{2502}\u{00B7}\u{2022}";
/// A line that is nothing but a page number: arabic or roman, optionally
/// "page N", "N of M" or "N/M", with up to three decorating non-word
/// characters on each side ("- 3 -", "[iv]").
static PAGE_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\W{0,3}(page\s+)?(\d{1,4}|[ivxlc]{1,7})(\s*(of|/)\s*\d{1,4})?\W{0,3}$").unwrap()
});
/// An arXiv identifier as printed in the margin stamp, old style
/// (`hep-th/9901001`, `math.AG/0101001`) or new style (`1706.03762`), with
/// an optional version. Group 1 is the identifier, group 2 the version.
static ARXIV_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"arXiv:\s*([a-z\-]+(?:\.[A-Z]{2})?/\d{7}|\d{4}\.\d{4,5})(v\d+)?").unwrap());

/// Tuning parameters of furniture removal. Part of [`LayoutConfig`](crate::config::LayoutConfig).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FurnitureConfig {
    /// A line whose bottom edge is above this fraction of the page height
    /// is in the head band, where running heads are looked for.
    pub head_band_ratio: f32,
    /// A line whose top edge is below this fraction of the page height is
    /// in the foot band, where running feet and page numbers are looked for.
    pub foot_band_ratio: f32,
    /// A horizontal rule at least this fraction of the page wide, outside
    /// the main text ...
    pub head_rule_min_width_ratio: f32,
    /// ... and at most this far (in points) from a removed running head or
    /// foot, belongs to it and is removed too.
    pub head_rule_max_gap_pt: f32,
    /// Shortest normalised text (in bytes, after digits and whitespace are folded) of
    /// a line that is counted as a possible running head. Shorter texts
    /// (a lone page number, "#") repeat by chance.
    pub repeat_min_text_bytes: usize,
    /// Documents with at most this many pages use
    /// [`short_doc_min_repeat_count`](Self::short_doc_min_repeat_count).
    pub short_doc_max_pages_count: usize,
    /// How many pages a head/foot line must appear on to be a running head
    /// in a short document.
    pub short_doc_min_repeat_count: usize,
    /// How many pages a head/foot line must appear on to be a running head
    /// in a longer document. Three rules out a title repeated on the next
    /// page by chance.
    pub min_repeat_count: usize,
    /// Lines with at least this many glyphs (and not repeated) make up the
    /// block of running text whose extent separates content from margins.
    pub body_line_min_glyphs_count: usize,
    /// Longest digit-only line that can be a margin line number.
    pub line_number_max_glyphs_count: usize,
    /// How far (in points) a margin line number may reach into the text
    /// block and still count as outside it.
    pub line_number_margin_pt: f32,
    /// Width (in points) of the bins in which margin line numbers are
    /// grouped by right edge; numbers of one column are right-aligned to
    /// within this.
    pub line_number_bin_pt: f32,
    /// Fewest numbers sharing a right edge for them to be a column of margin
    /// line numbers (review copies number every line, so there are many).
    pub line_number_min_count: usize,
    /// Smallest share of the steps between successive margin numbers (top
    /// to bottom) that are the same step: line numbers count up evenly
    /// (1, 2, 3 or 5, 10, 15), the page numbers of a table of contents do
    /// not.
    pub line_number_min_even_ratio: f32,
    /// How far (in points) a candidate may overlap the top or bottom of the
    /// text block and still count as outside it.
    pub body_edge_tolerance_pt: f32,
    /// Longest line (in glyphs) that is taken for a page number.
    pub page_number_max_glyphs_count: usize,
    /// Baseline distance (in points) below which another line counts as on
    /// the same row as a candidate, and so does not crowd it.
    pub crowded_min_distance_pt: f32,
    /// Only text of at least this share of a candidate's size crowds it:
    /// the small tick labels of a figure right below a running head do
    /// not make it part of the text...
    pub crowded_min_size_ratio: f32,
    /// ... when the candidate is set in at least this size (points): tiny
    /// labels inside a picture all crowd each other.
    pub crowded_min_head_size_pt: f32,
    /// Baseline distance (in font sizes of the larger line) below which a
    /// non-candidate line crowds a candidate, making it content: a normal
    /// line pitch is about 1.2 font sizes, and a table set in open rows
    /// (such as the header of a table of numbers) reaches about 1.5.
    pub crowded_max_distance_em: f32,
    /// A number followed on its row, within this many font sizes, by text
    /// that is not furniture is a heading's number, not a page number. A
    /// number set farther apart at either end of the top or bottom row of a
    /// page is the page number of a book's running head ("46   Chapter 4.
    /// Cumulative distribution functions").
    pub heading_number_max_gap_em: f32,
    /// A roman page number in a running head ("vi Preface") is at most
    /// this many pages below the page's position in the file (covers before
    /// the front matter).
    pub roman_number_max_offset_count: i64,
    /// The offset of printed page numbers to page indexes is the one at
    /// least this many pages ...
    pub number_offset_min_pages: usize,
    /// ... and this fraction of the pages agree on.
    pub number_offset_min_ratio: f32,
    /// A printer's mark ("___-1") is at least this many font sizes away
    /// from the words beside it on its row.
    pub printer_mark_min_gap_em: f32,
    /// Lines in the top or bottom part of the page this high (a fraction
    /// of the page height) are also compared by position: a book with wide
    /// margins sets its running feet above the foot band.
    pub placed_band_ratio: f32,
    /// Shortest normalised text (in bytes) of a line compared by position:
    /// a long line repeated at the same place is a running head or foot
    /// ("Draft (2024-01-15) of ...", a copyright notice), a short one may
    /// repeat by chance.
    pub placed_min_text_bytes: usize,
    /// Baselines within this distance (points) are at the same place.
    pub placed_tolerance_pt: f32,
    /// A shorter line in the bottom part of the page is compared by
    /// position too ("Pride and Prejudice", a book's running foot set above
    /// a wide bottom margin) when its text has at least this many bytes ...
    pub placed_short_min_text_bytes: usize,
    /// ... it is set smaller than the text of the document (at most this
    /// share of its size; a chapter heading is larger) ...
    pub placed_short_max_size_ratio: f32,
    /// ... and it occurs at the same place on at least this share of the
    /// pages (a running foot alternating between two texts is on half of
    /// them).
    pub placed_short_min_pages_ratio: f32,
}

impl Default for FurnitureConfig {
    fn default() -> Self {
        Self {
            head_band_ratio: 0.11,
            foot_band_ratio: 0.89,
            head_rule_min_width_ratio: 0.4,
            head_rule_max_gap_pt: 12.0,
            repeat_min_text_bytes: 3,
            short_doc_max_pages_count: 5,
            short_doc_min_repeat_count: 2,
            min_repeat_count: 3,
            body_line_min_glyphs_count: 25,
            line_number_max_glyphs_count: 4,
            line_number_margin_pt: 2.0,
            line_number_bin_pt: 3.0,
            line_number_min_count: 8,
            line_number_min_even_ratio: 0.7,
            body_edge_tolerance_pt: 1.0,
            page_number_max_glyphs_count: 12,
            crowded_min_distance_pt: 2.0,
            crowded_min_size_ratio: 0.7,
            crowded_min_head_size_pt: 5.0,
            crowded_max_distance_em: 1.75,
            heading_number_max_gap_em: 3.0,
            roman_number_max_offset_count: 4,
            number_offset_min_pages: 3,
            number_offset_min_ratio: 0.3,
            printer_mark_min_gap_em: 3.0,
            placed_band_ratio: 0.2,
            placed_min_text_bytes: 30,
            placed_tolerance_pt: 2.0,
            placed_short_min_text_bytes: 8,
            placed_short_max_size_ratio: 0.9,
            placed_short_min_pages_ratio: 0.2,
        }
    }
}

/// Result of the furniture pass.
#[derive(Debug, Default)]
pub struct Furniture {
    /// arXiv identifier (with version) read from the margin stamp, if any.
    pub arxiv_id: Option<String>,
    /// Full text of the arXiv margin stamp, if any.
    pub arxiv_stamp: Option<String>,
    /// The removed lines of each page (they are still drawn by the
    /// position-preserving renderers).
    pub lines: Vec<Vec<Line>>,
}

/// Width (points) of the bins in which line sizes are counted to find the
/// size of the text of the document.
const BODY_SIZE_STEP_PT: f32 = 0.5;

/// The margin band of a page in which a line lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Band {
    /// Top of the page (running heads).
    Head,
    /// Bottom of the page (running feet, page numbers).
    Foot,
}

/// Key under which head/foot lines are counted across pages: the
/// normalised text and the band it was found in.
type RepeatKey = (String, Band);

/// Key under which long lines are counted by position: the normalised text
/// and the baseline in steps of
/// [`placed_tolerance_pt`](FurnitureConfig::placed_tolerance_pt).
type PlacedKey = (String, i32);

/// How many pages each head/foot line occurs on, and how many it takes to
/// count as a running head.
struct RepeatCounts {
    /// Number of pages on which each key occurs (at most once per page).
    counts: HashMap<RepeatKey, usize>,
    /// Number of pages from which on a key is a running head or foot.
    min_repeat: usize,
    /// Printed page number minus the 1-based page index, as most pages
    /// show it (a document whose cover page is unnumbered starts at -1).
    number_offset: i64,
    /// Number of pages on which each long line of the top or bottom part
    /// of the page occurs at the same place (see [`PlacedKey`]).
    placed: HashMap<PlacedKey, usize>,
    /// Number of pages from which on a short line of the bottom part of the
    /// page repeated at the same place is a running foot (see
    /// [`placed_short_min_pages_ratio`](FurnitureConfig::placed_short_min_pages_ratio)).
    short_min_repeat: usize,
    /// Font size of the text of the document (points).
    body_size: f32,
}

impl RepeatCounts {
    /// Whether the line with text `text` in band `band` is repeated on
    /// enough pages to be a running head or foot.
    fn is_repeated(&self, text: &str, band: Band) -> bool {
        self.counts
            .get(&(normalize_furniture(text), band))
            .copied()
            .unwrap_or(0)
            >= self.min_repeat
    }
}

/// Extent of the running text of a page.
struct BodyExtent {
    /// Top of the highest body line.
    top: f32,
    /// Bottom of the lowest body line.
    bottom: f32,
    /// Left edge of the leftmost body line.
    left: f32,
    /// Right edge of the rightmost body line.
    right: f32,
}

/// Removes running headers/footers, page numbers, margin line numbers and the
/// arXiv margin stamp from the pages.
///
/// The removed lines of each page are returned in [`Furniture::lines`] (one
/// entry per page, in page order), together with the arXiv stamp and the
/// identifier read from it.
pub fn remove_furniture(pages: &mut [PageData], cfg: &FurnitureConfig) -> Furniture {
    let mut out = Furniture::default();
    find_arxiv_stamp(pages, &mut out);

    let n_pages = pages.len();
    let body_size = weighted_mode(
        pages
            .iter()
            .flat_map(|page| &page.lines)
            .map(|line| (line.size, line.glyphs.len() as f32)),
        BODY_SIZE_STEP_PT,
        0.0,
    );
    let min_repeat = if n_pages <= cfg.short_doc_max_pages_count {
        cfg.short_doc_min_repeat_count
    } else {
        cfg.min_repeat_count
    };
    let repeats = RepeatCounts {
        counts: count_head_foot_lines(pages, cfg),
        min_repeat,
        number_offset: page_number_offset(pages, cfg),
        placed: count_placed_lines(pages, body_size, cfg),
        short_min_repeat: min_repeat.max((cfg.placed_short_min_pages_ratio * n_pages as f32).ceil() as usize),
        body_size,
    };
    for page in pages.iter_mut() {
        let removed = remove_page_furniture(page, &repeats, cfg);
        out.lines.push(removed);
    }
    out
}

/// Reduces a line's text to the form in which running heads are compared:
/// whitespace removed, lower case, and every run of digits replaced by one
/// `#` (so that "Page 3" and "Page 14" match).
fn normalize_furniture(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            if !out.ends_with('#') {
                out.push('#');
            }
        } else if !ch.is_whitespace() {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

/// Finds the arXiv stamp (rotated text in the left margin, usually of the
/// first page) and stores the first one found, and the identifier read from
/// it, in `out`.
fn find_arxiv_stamp(pages: &[PageData], out: &mut Furniture) {
    for page in pages.iter() {
        for rotated in &page.rotated {
            let compact: String = rotated.text.chars().filter(|ch| !ch.is_whitespace()).collect();
            if compact.starts_with("arXiv:") && out.arxiv_stamp.is_none() {
                out.arxiv_stamp = Some(rotated.text.clone());
                // The text as printed first: without its spaces, the
                // version runs into the date ("v1 3 Dec" would be "v13").
                if let Some(caps) = ARXIV_ID.captures(&rotated.text).or_else(|| ARXIV_ID.captures(&compact)) {
                    out.arxiv_id = Some(format!(
                        "{}{}",
                        &caps[1],
                        caps.get(2).map(|version| version.as_str()).unwrap_or("")
                    ));
                }
            }
        }
    }
}

/// The margin band `line` lies in, or `None` when it is in the middle of the
/// page.
fn band(page: &PageData, line: &Line, cfg: &FurnitureConfig) -> Option<Band> {
    if line.bbox.y1 < cfg.head_band_ratio * page.height {
        Some(Band::Head)
    } else if line.bbox.y0 > cfg.foot_band_ratio * page.height {
        Some(Band::Foot)
    } else {
        None
    }
}

/// The key of `line` for the count by position, when it is long enough and
/// in the top or bottom part of the page (see
/// [`placed_band_ratio`](FurnitureConfig::placed_band_ratio)), with whether
/// it is a short line of the bottom part (see
/// [`placed_short_min_text_bytes`](FurnitureConfig::placed_short_min_text_bytes)),
/// which must repeat on more pages. `body_size` is the size of the text of
/// the document.
fn placed_key(page: &PageData, line: &Line, body_size: f32, cfg: &FurnitureConfig) -> Option<(PlacedKey, bool)> {
    let band = cfg.placed_band_ratio * page.height;
    let top = line.bbox.y1 < band;
    let bottom = line.bbox.y0 > page.height - band;
    let text = normalize_furniture(&line.text());
    let long = (top || bottom) && text.len() >= cfg.placed_min_text_bytes;
    let short = bottom
        && text.len() >= cfg.placed_short_min_text_bytes
        && line.size <= cfg.placed_short_max_size_ratio * body_size;
    // A short foot is compared without the page number at either end:
    // "Free eBooks at Planet eBook.com" and "… eBook.com 13" are one foot.
    let text = if long { text } else { text.trim_matches('#').to_string() };
    (long || short).then(|| ((text, (line.baseline / cfg.placed_tolerance_pt).round() as i32), !long))
}

/// Counts, for every long line of the top or bottom part of the pages, on
/// how many pages it occurs at the same place.
fn count_placed_lines(pages: &[PageData], body_size: f32, cfg: &FurnitureConfig) -> HashMap<PlacedKey, usize> {
    let mut counts: HashMap<PlacedKey, usize> = HashMap::new();
    for page in pages {
        let keys: HashSet<PlacedKey> = page
            .lines
            .iter()
            .filter_map(|line| placed_key(page, line, body_size, cfg).map(|(key, _)| key))
            .collect();
        for key in keys {
            *counts.entry(key).or_default() += 1;
        }
    }
    counts
}

/// Whether `line` is a long line repeated at the same place (within one
/// step of tolerance) on enough pages to be a running head or foot.
fn is_placed_repeat(page: &PageData, line: &Line, repeats: &RepeatCounts, cfg: &FurnitureConfig) -> bool {
    placed_key(page, line, repeats.body_size, cfg).is_some_and(|((text, step), short)| {
        let count: usize = (step - 1..=step + 1)
            .map(|near| repeats.placed.get(&(text.clone(), near)).copied().unwrap_or(0))
            .sum();
        count
            >= if short {
                repeats.short_min_repeat
            } else {
                repeats.min_repeat
            }
    })
}

/// Takes out of the page the long lines repeated at the same place on
/// other pages (see [`is_placed_repeat`]) that stand alone on their row,
/// and returns them with their index among the page's lines. A repeated
/// line beside other text on its row is part of it: the journal name in
/// the head of a first page, the header of a column of a table.
fn take_placed_repeats(page: &mut PageData, repeats: &RepeatCounts, cfg: &FurnitureConfig) -> Vec<(usize, Line)> {
    let placed: Vec<bool> = page
        .lines
        .iter()
        .map(|line| is_placed_repeat(page, line, repeats, cfg))
        .collect();
    // A page number beside a running foot does not make it text; it goes
    // with the foot.
    let number: Vec<bool> = page
        .lines
        .iter()
        .map(|line| {
            // Spaces apart: the digits of a page number may be set apart.
            let text: String = line.text().split_whitespace().collect();
            line.glyphs.len() <= cfg.page_number_max_glyphs_count && PAGE_NUMBER.is_match(&text)
        })
        .collect();
    let same_row = |a: &Line, b: &Line| (a.baseline - b.baseline).abs() <= cfg.crowded_min_distance_pt;
    let mut flags: Vec<bool> = page
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            placed[i]
                && !page
                    .lines
                    .iter()
                    .enumerate()
                    .any(|(j, other)| j != i && !placed[j] && !number[j] && same_row(other, line))
        })
        .collect();
    for j in 0..page.lines.len() {
        if number[j] && !flags[j] {
            flags[j] = (0..page.lines.len()).any(|i| flags[i] && same_row(&page.lines[i], &page.lines[j]));
        }
    }
    let mut taken = Vec::new();
    let mut kept = Vec::with_capacity(page.lines.len());
    for (i, (line, take)) in std::mem::take(&mut page.lines).into_iter().zip(flags).enumerate() {
        if take {
            taken.push((i, line));
        } else {
            kept.push(line);
        }
    }
    page.lines = kept;
    taken
}

/// Counts, for every normalised head/foot text, on how many pages it occurs.
fn count_head_foot_lines(pages: &[PageData], cfg: &FurnitureConfig) -> HashMap<RepeatKey, usize> {
    let mut counts: HashMap<RepeatKey, usize> = HashMap::new();
    for page in pages.iter() {
        let mut seen = HashSet::new();
        for line in &page.lines {
            if let Some(line_band) = band(page, line, cfg) {
                let key = (normalize_furniture(&line.text()), line_band);
                if key.0.len() >= cfg.repeat_min_text_bytes && seen.insert(key.clone()) {
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
    }
    counts
}

/// Removes the furniture lines of one page and returns them in their
/// original order.
fn remove_page_furniture(page: &mut PageData, repeats: &RepeatCounts, cfg: &FurnitureConfig) -> Vec<Line> {
    // Long lines repeated at the same place go first: the page's own
    // number may then be its last line.
    let placed = take_placed_repeats(page, repeats, cfg);
    let outer = body_extent(page, repeats, &numbered_heads(page, repeats.number_offset, cfg), cfg);
    let marks = take_printer_marks(page, &outer, cfg);
    let numbered = numbered_heads(page, repeats.number_offset, cfg);
    let body = body_extent(page, repeats, &numbered, cfg);
    let mut drop = margin_line_numbers(page, &body, cfg);
    mark_running_heads(page, &body, repeats, &numbered, cfg, &mut drop);

    let mut kept = Vec::with_capacity(page.lines.len());
    let mut removed = Vec::new();
    // The removed lines keep the order they had on the page: the placed
    // repeats go back between the others at their original index.
    let mut placed = placed.into_iter().peekable();
    let mut original = 0;
    for (i, line) in std::mem::take(&mut page.lines).into_iter().enumerate() {
        while placed.peek().is_some_and(|(idx, _)| *idx == original) {
            removed.extend(placed.next().map(|(_, line)| line));
            original += 1;
        }
        original += 1;
        if drop[i] {
            removed.push(line);
        } else {
            kept.push(line);
        }
    }
    removed.extend(placed.map(|(_, line)| line));
    page.lines = kept;
    drop_head_rules(page, &removed, &body, cfg);
    removed.extend(marks);
    removed
}

/// Takes the printer's marks out of the lines of a page and returns them:
/// the position marks a layout program prints in the margin ("___-1",
/// "___0", "___1"), which share a baseline with lines of the text and were
/// read as part of them. A mark is a word of underscores (cut by the
/// page's crop box, it may keep one only), with an optional signed number,
/// beside the text block `body` and set apart from the rest of its line by
/// more than [`printer_mark_min_gap_em`](FurnitureConfig::printer_mark_min_gap_em).
fn take_printer_marks(page: &mut PageData, body: &BodyExtent, cfg: &FurnitureConfig) -> Vec<Line> {
    let mut marks = Vec::new();
    let mut kept = Vec::with_capacity(page.lines.len());
    for line in std::mem::take(&mut page.lines) {
        let words = word_ranges(&line);
        let is_mark = |range: &std::ops::Range<usize>| {
            let text: String = line.glyphs[range.clone()]
                .iter()
                .map(|glyph| glyph.text.as_str())
                .collect();
            PRINTER_MARK.is_match(&text)
        };
        let apart = |i: usize| {
            let range = &words[i];
            let gap_before = i
                .checked_sub(1)
                .map(|prev| line.glyphs[range.start].x0 - line.glyphs[words[prev].end - 1].x1);
            let gap_after = words
                .get(i + 1)
                .map(|next| line.glyphs[next.start].x0 - line.glyphs[range.end - 1].x1);
            let far = |gap: Option<f32>| gap.is_none_or(|gap| gap > cfg.printer_mark_min_gap_em * line.size);
            far(gap_before) && far(gap_after)
        };
        // Beside the text block, not in it: a row of underscores in the
        // text is a blank to fill in, or a rule in a listing.
        let beside = |range: &std::ops::Range<usize>| {
            line.glyphs[range.start].x0 >= body.right - cfg.body_edge_tolerance_pt
                || line.glyphs[range.end - 1].x1 <= body.left + cfg.body_edge_tolerance_pt
        };
        let mark_words: Vec<usize> = (0..words.len())
            .filter(|&i| is_mark(&words[i]) && beside(&words[i]) && apart(i))
            .collect();
        if mark_words.is_empty() {
            kept.push(line);
            continue;
        }
        let in_mark = |idx: usize| mark_words.iter().any(|&i| words[i].contains(&idx));
        let (mark, rest): (Vec<_>, Vec<_>) = line
            .glyphs
            .iter()
            .cloned()
            .enumerate()
            .partition(|(idx, _)| in_mark(*idx));
        marks.push(with_glyphs(&line, mark.into_iter().map(|(_, glyph)| glyph).collect()));
        let rest: Vec<LineGlyph> = rest.into_iter().map(|(_, glyph)| glyph).collect();
        if !rest.is_empty() {
            kept.push(with_glyphs(&line, rest));
        }
    }
    page.lines = kept;
    marks
}

/// The glyph index ranges of the words of a line (split at word spaces).
fn word_ranges(line: &Line) -> Vec<std::ops::Range<usize>> {
    let mut words: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, glyph) in line.glyphs.iter().enumerate() {
        match words.last_mut() {
            Some(word) if !glyph.space_before => word.end = i + 1,
            _ => words.push(i..i + 1),
        }
    }
    words
}

/// `line` with only `glyphs` (a part of its glyphs, in order), its boxes
/// fitted to them.
fn with_glyphs(line: &Line, mut glyphs: Vec<LineGlyph>) -> Line {
    if let Some(first) = glyphs.first_mut() {
        first.space_before = false;
    }
    let x0 = glyphs.first().map_or(line.bbox.x0, |glyph| glyph.x0);
    let x1 = glyphs.last().map_or(line.bbox.x1, |glyph| glyph.x1);
    let ink = glyphs.iter().fold(Rect::EMPTY, |acc, glyph| acc.union(&glyph.ink));
    Line {
        bbox: Rect::new(x0, line.bbox.y0, x1, line.bbox.y1),
        ink,
        glyphs,
        ..line.clone()
    }
}

/// Removes the rules that underline a running head (or top a running foot):
/// long horizontal rules next to a removed line and outside the main text.
/// Left on the page, such a rule joins the figure below it and makes the
/// figure as wide as the page.
fn drop_head_rules(page: &mut PageData, removed: &[Line], body: &BodyExtent, cfg: &FurnitureConfig) {
    let width = page.width;
    page.paths.retain(|path| {
        let rule = &path.bbox;
        let head_rule = path.is_hrule()
            && rule.width() >= cfg.head_rule_min_width_ratio * width
            && (rule.y1 <= body.top || rule.y0 >= body.bottom)
            && removed.iter().any(|line| {
                let gap = if rule.cy() > line.bbox.cy() {
                    rule.y0 - line.bbox.y1
                } else {
                    line.bbox.y0 - rule.y1
                };
                gap <= cfg.head_rule_max_gap_pt && rule.x_overlap(&line.bbox) > 0.0
            });
        !head_rule
    });
}

/// Extent of the main text: the long lines that are not running heads. When
/// there are none, the whole page.
fn body_extent(page: &PageData, repeats: &RepeatCounts, numbered: &[bool], cfg: &FurnitureConfig) -> BodyExtent {
    let is_repeated =
        |line: &Line| band(page, line, cfg).is_some_and(|line_band| repeats.is_repeated(&line.text(), line_band));
    // The lines of a numbered running head are not text either.
    let lines: Vec<&Line> = page
        .lines
        .iter()
        .zip(numbered)
        .filter(|(_, head)| !**head)
        .map(|(line, _)| line)
        .collect();
    // A first page's foot may set a journal line on the row of the page id
    // that the other pages repeat alone ("0031-9007/16/116(6)/061102(16)
    // 061102-1 Published by ..."): that row is not text.
    let id_rows: Vec<f32> = page
        .lines
        .iter()
        .filter(|line| {
            let text = line.text();
            PAGE_ID.is_match(text.trim())
                && band(page, line, cfg).is_some_and(|line_band| repeats.is_repeated(&text, line_band))
        })
        .map(|line| line.baseline)
        .collect();
    let body: Vec<&Line> = lines
        .iter()
        .copied()
        .filter(|line| line.char_count() >= cfg.body_line_min_glyphs_count && !is_repeated(line))
        .filter(|line| !PREPRINT_FOOT.is_match(line.text().trim()))
        .filter(|line| {
            !id_rows
                .iter()
                .any(|&row| (row - line.baseline).abs() <= cfg.crowded_min_distance_pt)
        })
        .collect();
    // A page with no long line (a lone "Appendix" heading): its other text,
    // without page numbers, is the text block.
    let short: Vec<&Line> = lines
        .iter()
        .copied()
        .filter(|line| !PAGE_NUMBER.is_match(line.text().trim()) && !is_repeated(line))
        .collect();
    let (top, bottom) = if body.is_empty() && short.is_empty() {
        (0.0, page.height)
    } else if body.is_empty() {
        (
            short.iter().map(|line| line.bbox.y0).fold(f32::INFINITY, f32::min),
            short.iter().map(|line| line.bbox.y1).fold(f32::NEG_INFINITY, f32::max),
        )
    } else {
        (
            body.iter().map(|line| line.bbox.y0).fold(f32::INFINITY, f32::min),
            body.iter().map(|line| line.bbox.y1).fold(f32::NEG_INFINITY, f32::max),
        )
    };
    let (left, right) = if body.is_empty() {
        (0.0, page.width)
    } else {
        (
            body.iter().map(|line| line.bbox.x0).fold(f32::INFINITY, f32::min),
            body.iter().map(|line| line.bbox.x1).fold(f32::NEG_INFINITY, f32::max),
        )
    };
    BodyExtent {
        top,
        bottom,
        left,
        right,
    }
}

/// Margin line numbers (review mode): many short numeric lines sharing a
/// right edge outside the text block. Returns one flag per line of the page,
/// set for the line numbers.
fn margin_line_numbers(page: &PageData, body: &BodyExtent, cfg: &FurnitureConfig) -> Vec<bool> {
    let mut numeric: HashMap<i32, Vec<usize>> = HashMap::new();
    for (i, line) in page.lines.iter().enumerate() {
        if line.char_count() <= cfg.line_number_max_glyphs_count
            && line
                .glyphs
                .iter()
                .all(|glyph| glyph.text.chars().all(|ch| ch.is_ascii_digit()))
            && (line.bbox.x1 < body.left + cfg.line_number_margin_pt
                || line.bbox.x0 > body.right - cfg.line_number_margin_pt)
        {
            numeric
                .entry((line.bbox.x1 / cfg.line_number_bin_pt).round() as i32)
                .or_default()
                .push(i);
        }
    }
    let mut drop: Vec<bool> = vec![false; page.lines.len()];
    for idxs in numeric.values() {
        if idxs.len() >= cfg.line_number_min_count && counts_evenly(page, idxs, cfg) {
            for &i in idxs {
                drop[i] = true;
            }
        }
    }
    drop
}

/// Whether the numeric lines `idxs` of the page, read top to bottom, count
/// up by the same step most of the time (see
/// [`FurnitureConfig::line_number_min_even_ratio`]).
fn counts_evenly(page: &PageData, idxs: &[usize], cfg: &FurnitureConfig) -> bool {
    let mut numbers: Vec<(f32, i64)> = idxs
        .iter()
        .filter_map(|&i| Some((page.lines[i].baseline, page.lines[i].text().trim().parse().ok()?)))
        .collect();
    numbers.sort_by(|a, b| a.0.total_cmp(&b.0));
    let steps: Vec<i64> = numbers.windows(2).map(|pair| pair[1].1 - pair[0].1).collect();
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for &step in steps.iter().filter(|&&step| step > 0) {
        *counts.entry(step).or_default() += 1;
    }
    let most = counts.values().copied().max().unwrap_or(0);
    most as f32 >= cfg.line_number_min_even_ratio * steps.len() as f32
}

/// Sets the `drop` flag of the running heads, running feet and page numbers
/// of the page: lines in a margin band, above or below the text block, that
/// are repeated or look like a page number, and that stand apart from other
/// text.
fn mark_running_heads(
    page: &PageData,
    body: &BodyExtent,
    repeats: &RepeatCounts,
    numbered: &[bool],
    cfg: &FurnitureConfig,
    drop: &mut [bool],
) {
    let mut candidate: Vec<bool> = page
        .lines
        .iter()
        .zip(numbered)
        .map(|(line, &head)| head || is_furniture_candidate(page, line, body, repeats, cfg))
        .collect();
    // A number right before other text on its row is the number of a
    // heading at the top of the page ("1   Introduction"), not a page number.
    let heading_numbers: Vec<bool> = page
        .lines
        .iter()
        .map(|line| {
            PAGE_NUMBER.is_match(line.text().trim())
                && page.lines.iter().zip(&candidate).any(|(other, &other_candidate)| {
                    !other_candidate
                        && (other.baseline - line.baseline).abs() <= cfg.crowded_min_distance_pt
                        && other.bbox.x0 >= line.bbox.x1
                        && other.bbox.x0 - line.bbox.x1 <= cfg.heading_number_max_gap_em * line.size
                })
        })
        .collect();
    for (flag, numbered) in candidate.iter_mut().zip(heading_numbers) {
        *flag &= !numbered;
    }
    let dropped = drop_apart(page, &candidate, cfg);
    // The other pieces of the row of a dropped page id (see `body_extent`)
    // are furniture too.
    let id_rows: Vec<f32> = page
        .lines
        .iter()
        .zip(&dropped)
        .filter(|(line, is_dropped)| {
            let text = line.text();
            **is_dropped && (PAGE_ID.is_match(text.trim()) || PREPRINT_FOOT.is_match(text.trim()))
        })
        .map(|(line, _)| line.baseline)
        .collect();
    let with_mates: Vec<bool> = page
        .lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            dropped[i]
                || id_rows
                    .iter()
                    .any(|&row| (row - line.baseline).abs() <= cfg.crowded_min_distance_pt)
        })
        .collect();
    // A row that ends with the page's own number is a running head even
    // with text right below it (a margin caption level with a figure).
    for ((flag, keep_out), &head) in drop.iter_mut().zip(drop_apart(page, &with_mates, cfg)).zip(numbered) {
        *flag |= keep_out || head;
    }
}

/// The candidates that stand apart from the content: running heads do. A
/// candidate with other text right above or below it is content itself
/// (for instance the header row of a table at the top of a page); the much
/// smaller labels of a figure do not count (see
/// [`FurnitureConfig::crowded_min_size_ratio`]).
fn drop_apart(page: &PageData, candidate: &[bool], cfg: &FurnitureConfig) -> Vec<bool> {
    page.lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            candidate[i]
                && !page.lines.iter().enumerate().any(|(j, other)| {
                    let distance = (other.baseline - line.baseline).abs();
                    !candidate[j]
                        && (other.size >= cfg.crowded_min_size_ratio * line.size
                            || line.size < cfg.crowded_min_head_size_pt)
                        && distance > cfg.crowded_min_distance_pt
                        && distance < cfg.crowded_max_distance_em * line.size.max(other.size)
                })
        })
        .collect()
}

/// Whether `line` is a candidate for removal: a repeated line or a page
/// number in a margin band, outside the text block.
fn is_furniture_candidate(
    page: &PageData,
    line: &Line,
    body: &BodyExtent,
    repeats: &RepeatCounts,
    cfg: &FurnitureConfig,
) -> bool {
    let text = line.text();
    let outside = line.bbox.y1 <= body.top + cfg.body_edge_tolerance_pt
        || line.bbox.y0 >= body.bottom - cfg.body_edge_tolerance_pt;
    let is_page_no = PAGE_NUMBER.is_match(text.trim()) && line.char_count() <= cfg.page_number_max_glyphs_count;
    match band(page, line, cfg) {
        Some(line_band) => (repeats.is_repeated(&text, line_band) || is_page_no) && outside,
        // Wide margins (LNCS, a preprint in plain TeX) put the page number
        // below the head band or above the foot band: there it must be the
        // page's own number, on the first or last line of the page.
        None => {
            (is_page_no
                && outside
                && is_own_number(page, &text, repeats.number_offset)
                && is_extreme_line(page, line, cfg)
                && alone_on_row(page, line, cfg))
                || (PREPRINT_FOOT.is_match(text.trim())
                    && outside
                    && line.bbox.y0 >= body.bottom - cfg.body_edge_tolerance_pt)
        }
    }
}

/// Whether `text` is the page's number: its 1-based index plus the
/// document's `offset`, or a roman numeral (front matter, numbered apart)
/// no greater than the page's 1-based index.
fn is_own_number(page: &PageData, text: &str, offset: i64) -> bool {
    let text = text.trim();
    match text.parse::<i64>() {
        Ok(number) => number == page.index as i64 + 1 + offset,
        Err(_) => roman_value(&text.to_lowercase()).is_some_and(|number| number <= page.index as i64 + 1),
    }
}

/// The offset between printed page numbers and page indexes that most pages
/// agree on: lone numbers on the top or bottom line of a page, alone on
/// their row. 0 unless enough pages agree.
fn page_number_offset(pages: &[PageData], cfg: &FurnitureConfig) -> i64 {
    let mut votes: HashMap<i64, usize> = HashMap::new();
    for page in pages {
        let offsets: HashSet<i64> = page
            .lines
            .iter()
            .filter(|line| is_extreme_line(page, line, cfg) && alone_on_row(page, line, cfg))
            .filter_map(|line| line.text().trim().parse::<i64>().ok())
            .chain(
                edge_rows(page, cfg)
                    .into_iter()
                    .filter_map(|row| row_edge_number(page, &row, cfg)),
            )
            .map(|number| number - (page.index as i64 + 1))
            .collect();
        for offset in offsets {
            *votes.entry(offset).or_default() += 1;
        }
    }
    let needed = cfg
        .number_offset_min_pages
        .max((pages.len() as f32 * cfg.number_offset_min_ratio).ceil() as usize);
    votes
        .into_iter()
        .filter(|&(_, count)| count >= needed)
        .max_by_key(|&(offset, count)| (count, -offset.abs()))
        .map_or(0, |(offset, _)| offset)
}

/// The top and the bottom row of a page: the indexes of the lines on the
/// baseline of its highest line, and of its lowest.
fn edge_rows(page: &PageData, cfg: &FurnitureConfig) -> Vec<Vec<usize>> {
    let row_of = |baseline: f32| -> Vec<usize> {
        (0..page.lines.len())
            .filter(|&i| (page.lines[i].baseline - baseline).abs() <= cfg.crowded_min_distance_pt)
            .collect()
    };
    let top = page.lines.iter().min_by(|a, b| a.bbox.y0.total_cmp(&b.bbox.y0));
    let bottom = page.lines.iter().max_by(|a, b| a.bbox.y1.total_cmp(&b.bbox.y1));
    let mut rows = Vec::new();
    if let Some(line) = top {
        rows.push(row_of(line.baseline));
    }
    if let Some(line) = bottom {
        let row = row_of(line.baseline);
        if !rows.contains(&row) {
            rows.push(row);
        }
    }
    rows
}

/// The number at the start or the end of a row of text, set apart from the
/// rest of the row by more than
/// [`heading_number_max_gap_em`](FurnitureConfig::heading_number_max_gap_em):
/// the page number of a running head. `None` when there is none.
fn row_edge_number(page: &PageData, row: &[usize], cfg: &FurnitureConfig) -> Option<i64> {
    row_edge_token(page, row, cfg, |ch| ch.is_ascii_digit())?.parse().ok()
}

/// The value of a lowercase roman page number ("vi", "xxii") set apart at
/// either end of the row `row` (front matter), as [`row_edge_number`].
fn row_edge_roman(page: &PageData, row: &[usize], cfg: &FurnitureConfig) -> Option<i64> {
    roman_value(&row_edge_token(page, row, cfg, |ch| {
        matches!(ch, 'i' | 'v' | 'x' | 'l')
    })?)
}

/// The value of the lowercase roman numeral `text`, when it is one written
/// in the usual subtractive form.
pub(crate) fn roman_value(text: &str) -> Option<i64> {
    let digit = |ch: char| match ch {
        'i' => Some(1),
        'v' => Some(5),
        'x' => Some(10),
        'l' => Some(50),
        _ => None,
    };
    let values: Vec<i64> = text.chars().map(digit).collect::<Option<_>>()?;
    let mut total = 0;
    for (idx, value) in values.iter().enumerate() {
        if values.get(idx + 1).is_some_and(|next| next > value) {
            total -= value;
        } else {
            total += value;
        }
    }
    (total > 0 && to_roman(total) == text).then_some(total)
}

/// `value` (1 to 89) as a lowercase roman numeral.
fn to_roman(mut value: i64) -> String {
    let mut out = String::new();
    for (step, numeral) in [
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while value >= step {
            out.push_str(numeral);
            value -= step;
        }
    }
    out
}

/// The run of glyphs accepted by `accept` at either end of the row `row`,
/// set apart from the rest of the row by more than
/// `heading_number_max_gap_em`, or by a separator (see
/// [`RUNNING_HEAD_SEPARATORS`]).
fn row_edge_token(
    page: &PageData,
    row: &[usize],
    cfg: &FurnitureConfig,
    accept: impl Fn(char) -> bool,
) -> Option<String> {
    let mut glyphs: Vec<&LineGlyph> = row.iter().flat_map(|&i| page.lines[i].glyphs.iter()).collect();
    glyphs.retain(|glyph| !glyph.text.trim().is_empty());
    glyphs.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    let accepted = |glyph: &&&LineGlyph| glyph.text.chars().all(&accept);
    let lead = glyphs.iter().take_while(accepted).count();
    let trail = glyphs.iter().rev().take_while(accepted).count();
    let token = |part: &[&LineGlyph]| part.iter().map(|glyph| glyph.text.as_str()).collect::<String>();
    // A number next to a separator ("Preface | xv", "96 | Chapter 2") is
    // set apart by it, however close.
    let separator = |glyph: &LineGlyph| glyph.text.chars().all(|ch| RUNNING_HEAD_SEPARATORS.contains(ch));
    let apart = |left: &LineGlyph, right: &LineGlyph| {
        right.x0 - left.x1 > cfg.heading_number_max_gap_em * left.size || separator(left) || separator(right)
    };
    if lead > 0 && lead < glyphs.len() && apart(glyphs[lead - 1], glyphs[lead]) {
        return Some(token(&glyphs[..lead]));
    }
    let first = glyphs.len() - trail;
    if trail > 0 && first > 0 && apart(glyphs[first - 1], glyphs[first]) {
        return Some(token(&glyphs[first..]));
    }
    None
}

/// The lines of a book's running head that carries the page number
/// (see [`row_edge_number`]): the top or bottom row of the page when the
/// number set apart at its end is the page's own number. One flag per line.
fn numbered_heads(page: &PageData, offset: i64, cfg: &FurnitureConfig) -> Vec<bool> {
    let mut heads = vec![false; page.lines.len()];
    for row in edge_rows(page, cfg) {
        let page_number = page.index as i64 + 1;
        let arabic = row_edge_number(page, &row, cfg) == Some(page_number + offset);
        // Front matter counts its pages in roman numerals from the first
        // page, or from just after the cover.
        let roman = row_edge_roman(page, &row, cfg)
            .is_some_and(|value| value <= page_number && page_number - value <= cfg.roman_number_max_offset_count);
        if arabic || roman {
            for i in row {
                heads[i] = true;
            }
        }
    }
    heads
}

/// Whether no other line shares the row of `line` (a footnote's number
/// sits next to its text; a page number stands alone).
fn alone_on_row(page: &PageData, line: &Line, cfg: &FurnitureConfig) -> bool {
    page.lines
        .iter()
        .filter(|other| (other.baseline - line.baseline).abs() <= cfg.crowded_min_distance_pt)
        .count()
        == 1
}

/// Whether `line` is the top or the bottom line of the page.
fn is_extreme_line(page: &PageData, line: &Line, cfg: &FurnitureConfig) -> bool {
    let tol = cfg.body_edge_tolerance_pt;
    page.lines.iter().all(|other| other.bbox.y0 >= line.bbox.y0 - tol)
        || page.lines.iter().all(|other| other.bbox.y1 <= line.bbox.y1 + tol)
}

#[cfg(test)]
mod tests {
    use super::{FurnitureConfig, PageData, remove_furniture, roman_value};
    use crate::analysis::lines::LinesConfig;
    use crate::backend::{Dir, Glyph, RawPage};
    use crate::common::geom::Rect;

    /// The glyphs of `text` from `x` on baseline `y` in `size` pt type.
    fn words(text: &str, x: f32, y: f32, size: f32) -> Vec<Glyph> {
        let adv = 0.5 * size;
        text.chars()
            .enumerate()
            .map(|(i, ch)| {
                let x = x + i as f32 * adv;
                Glyph {
                    text: ch.to_string(),
                    x,
                    y,
                    adv,
                    size,
                    font: 0,
                    dir: Dir::Horizontal,
                    angle: 0.0,
                    invisible: false,
                    color: [0; 3],
                    ink: Rect::new(x, y - 0.7 * size, x + adv, y),
                }
            })
            .collect()
    }

    /// Page `index` of a small book (320 × 480 pt): ten lines of 10 pt text
    /// and, at 87 % of the height (above the foot band), a running foot in
    /// 7 pt alternating between the title and a notice, with the page
    /// number apart at the other end of its row.
    fn book_page(index: usize) -> PageData {
        let mut glyphs = Vec::new();
        for row in 0..10 {
            let y = 60.0 + 30.0 * row as f32;
            glyphs.extend(words("these are the words of a line of running text", 30.0, y, 10.0));
        }
        let (foot, number_x) = if index.is_multiple_of(2) {
            ("Pride and Prejudice", 30.0)
        } else {
            ("Free eBooks at Planet eBook.com", 280.0)
        };
        let foot_x = if index.is_multiple_of(2) { 200.0 } else { 30.0 };
        glyphs.extend(words(foot, foot_x, 418.0, 7.0));
        glyphs.extend(words(&(index + 1).to_string(), number_x, 418.0, 7.0));
        let raw = RawPage {
            index,
            width: 320.0,
            height: 480.0,
            glyphs,
            ..Default::default()
        };
        PageData::from_raw(raw, &LinesConfig::default())
    }

    #[test]
    fn short_running_feet_above_the_foot_band_are_removed() {
        let mut pages: Vec<PageData> = (0..10).map(book_page).collect();
        remove_furniture(&mut pages, &FurnitureConfig::default());
        for page in &pages {
            let text: String = page.lines.iter().map(|line| line.text() + " ").collect();
            assert!(
                !text.contains("Pride") && !text.contains("eBook"),
                "page {}: {text}",
                page.index
            );
            assert!(!text.chars().any(|ch| ch.is_ascii_digit()), "page number left: {text}");
            assert_eq!(text.matches("running").count(), 10, "the text stays: {text}");
        }
    }

    #[test]
    fn roman_page_numbers_are_read() {
        assert_eq!(roman_value("vi"), Some(6));
        assert_eq!(roman_value("xxii"), Some(22));
        assert_eq!(roman_value("xlix"), Some(49));
        // Not written the usual way, or not a numeral.
        assert_eq!(roman_value("iiii"), None);
        assert_eq!(roman_value("vx"), None);
        assert_eq!(roman_value("vim"), None);
    }
}
