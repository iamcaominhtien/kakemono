//! Tables of contents: pages whose lines mostly end with a page number set
//! apart from their title (by leader dots or a wide gap), the numbers in
//! increasing order. Each entry becomes a node of its own
//! ([`Class::ContentsEntry`]) with its nesting level and page label, so that
//! it is never read as a heading ("1. Introduction ... 1" looks like a
//! numbered section) nor run together with the next entries as a paragraph.

use std::collections::HashSet;

use super::{Class, Node, NodeKind, SemanticsConfig};
use crate::analysis::furniture::roman_value;
use crate::common::geom::Rect;
use crate::model::{Line, LineGlyph, Span};

/// Characters of the leaders between a title and its page number.
const LEADER_CHARS: &[char] = &['.', '\u{2026}', '\u{00B7}', '\u{2024}'];

/// Tuning parameters of table of contents detection. Part of
/// [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContentsConfig {
    /// Fewest entry lines (see [`entry_page`]) a page needs to be a page of
    /// contents.
    pub page_min_entries_count: usize,
    /// Smallest share of the unclassified lines of a page that must be
    /// entry lines.
    pub page_min_entries_ratio: f32,
    /// Smallest share of consecutive entries of a page whose page numbers
    /// do not go down (a table of numbers is not in order).
    pub page_min_ordered_ratio: f32,
    /// A page number is set apart from its title by a gap of at least this
    /// many font sizes, when no leader dots run up to it.
    pub number_min_gap_em: f32,
    /// Fewest leader dots before a page number.
    pub leader_min_dots_count: usize,
    /// The start of an entry's title that wrapped into a block of its own
    /// has at most this many lines...
    pub wrap_max_lines_count: usize,
    /// ... stands at most this many font sizes above the rest of the entry...
    pub wrap_max_gap_em: f32,
    /// ... in the same size, within this (points)...
    pub wrap_size_tolerance_pt: f32,
    /// ... and fills at least this share of the row up to the page number
    /// (a short line is a part title, not the start of an entry).
    pub wrap_min_fill_ratio: f32,
    /// Longest page number, in digits.
    pub number_max_digits_count: usize,
    /// A page number set as a line of its own belongs to the line before it
    /// when their baselines differ by at most this (font sizes).
    pub row_max_offset_em: f32,
    /// Entries whose indents differ by at most this (points) are on the
    /// same level.
    pub indent_tolerance_pt: f32,
    /// An indent shared by fewer than this share of the entries is not a
    /// level of its own (a two-digit chapter number hanging half an em to
    /// the left of the one-digit ones)...
    pub level_min_entries_ratio: f32,
    /// ... and joins the nearest level within this distance (points).
    pub indent_merge_max_pt: f32,
    /// Deepest level given to an entry.
    pub max_level: u8,
}

impl Default for ContentsConfig {
    fn default() -> Self {
        Self {
            page_min_entries_count: 4,
            page_min_entries_ratio: 0.6,
            page_min_ordered_ratio: 0.8,
            number_min_gap_em: 2.0,
            leader_min_dots_count: 3,
            wrap_max_lines_count: 2,
            wrap_max_gap_em: 1.6,
            wrap_size_tolerance_pt: 0.3,
            wrap_min_fill_ratio: 0.6,
            number_max_digits_count: 4,
            row_max_offset_em: 0.3,
            indent_tolerance_pt: 3.0,
            level_min_entries_ratio: 0.03,
            indent_merge_max_pt: 10.0,
            max_level: 4,
        }
    }
}

/// A page label read at the end of an entry line, as an order key: roman
/// numbers (front matter) come before arabic ones.
#[derive(Debug, Clone, PartialEq)]
struct PageLabel {
    /// The label as printed ("xi", "47").
    text: String,
    /// Sort key.
    key: i64,
}

/// An entry found on a page of contents.
struct Entry {
    /// Index of its node.
    idx: usize,
    /// Its page.
    page: usize,
    /// Left edge of its first line.
    x0: f32,
}

/// Offset that sorts roman page numbers before every arabic one.
const ROMAN_KEY_OFFSET: i64 = -100_000;

/// Marks the entries of the tables of contents among the unclassified text
/// nodes: splits their blocks into one node per entry (the lines up to one
/// that ends with a page number) and gives each its level, from its indent.
pub(crate) fn mark_contents(nodes: &mut Vec<Node>, cfg: &SemanticsConfig) {
    let cfg = &cfg.contents;
    let pages = contents_pages(nodes, cfg);
    if pages.is_empty() {
        return;
    }
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    let mut entries: Vec<Entry> = Vec::new();
    for node in nodes.drain(..) {
        let on_contents_page = pages.contains(&node.loc.page) && node.class == Class::Unknown;
        match &node.kind {
            NodeKind::Text(block) if on_contents_page => {
                let block = block.clone();
                split_entries(node, &block.lines, cfg, &mut out, &mut entries);
            }
            _ => out.push(node),
        }
    }
    let levels = indent_levels(&entries, cfg);
    for (entry, level) in entries.into_iter().zip(levels) {
        if let Class::ContentsEntry { level: slot, .. } = &mut out[entry.idx].class {
            *slot = level;
        }
    }
    *nodes = out;
}

/// The pages of contents: those with enough entry lines, in order, and the
/// pages right after one that still read as entries.
fn contents_pages(nodes: &[Node], cfg: &ContentsConfig) -> HashSet<usize> {
    // Per page: number of unclassified lines, and the labels of the entry lines.
    let mut stats: std::collections::BTreeMap<usize, (usize, Vec<PageLabel>)> = std::collections::BTreeMap::new();
    for node in nodes.iter().filter(|node| node.class == Class::Unknown) {
        let NodeKind::Text(block) = &node.kind else { continue };
        let labels = entry_labels(&block.lines, cfg);
        // A page number set as a line of its own counts with its title.
        let bare = (1..block.lines.len())
            .filter(|&i| labels[i].is_some() && is_bare_number(&block.lines[i]))
            .count();
        let entry = stats.entry(node.loc.page).or_default();
        entry.0 += block.lines.len() - bare;
        entry.1.extend(labels.into_iter().flatten());
    }
    let reads_as_contents = |lines: usize, labels: &[PageLabel], min_entries: usize| {
        let ordered = labels.windows(2).filter(|pair| pair[0].key <= pair[1].key).count();
        labels.len() >= min_entries
            && labels.len() as f32 >= cfg.page_min_entries_ratio * lines as f32
            && ordered as f32 >= cfg.page_min_ordered_ratio * labels.len().saturating_sub(1) as f32
    };
    let mut pages = HashSet::new();
    let mut previous: Option<usize> = None;
    for (&page, (lines, labels)) in &stats {
        let continues = previous.is_some_and(|prev| prev + 1 == page);
        let min_entries = if continues { 1 } else { cfg.page_min_entries_count };
        if reads_as_contents(*lines, labels, min_entries) {
            pages.insert(page);
            previous = Some(page);
        } else {
            previous = None;
        }
    }
    pages
}

/// Splits the text node `node` (with lines `lines`) of a page of contents
/// into its entries: each entry runs up to a line that ends with a page
/// number. Lines left after the last entry stay unclassified. The new
/// entries are recorded in `entries`.
fn split_entries(node: Node, lines: &[Line], cfg: &ContentsConfig, out: &mut Vec<Node>, entries: &mut Vec<Entry>) {
    let NodeKind::Text(block) = &node.kind else {
        unreachable!("only text nodes are split")
    };
    let mut start = 0;
    let mut pieces: Vec<(std::ops::Range<usize>, Option<PageLabel>)> = Vec::new();
    for (i, label) in entry_labels(lines, cfg).into_iter().enumerate() {
        if let Some(label) = label {
            pieces.push((start..i + 1, Some(label)));
            start = i + 1;
        }
    }
    if start < lines.len() {
        pieces.push((start..lines.len(), None));
    }
    for (offset, (range, label)) in pieces.into_iter().enumerate() {
        let mut part = block.clone();
        part.lines = lines[range.clone()].to_vec();
        part.bbox = part
            .lines
            .iter()
            .map(|line| line.bbox)
            .reduce(|a, b| a.union(&b))
            .unwrap_or(Rect::EMPTY);
        if offset == 0
            && label.is_some()
            && let Some(title) = dangling_title(out, &node, &part.lines[0], cfg)
        {
            part.lines.splice(0..0, title);
            part.bbox = part.bbox.union(&part.lines[0].bbox);
        }
        let class = match &label {
            Some(label) => {
                entries.push(Entry {
                    idx: out.len(),
                    page: node.loc.page,
                    x0: part.lines[0].bbox.x0,
                });
                Class::ContentsEntry {
                    level: 1,
                    page: label.text.clone(),
                }
            }
            None => Class::Unknown,
        };
        out.push(Node {
            kind: NodeKind::Text(part),
            loc: node.loc,
            class,
            tail: node.tail,
            leaf_left: node.leaf_left,
            leaf_right: node.leaf_right,
            strip_prefix: if offset == 0 { node.strip_prefix } else { 0 },
        });
    }
}

/// The sort key of a page label: its value, roman numbers first. `None`
/// when it is not a page number.
fn page_key(text: &str, cfg: &ContentsConfig) -> Option<i64> {
    if text.chars().all(|ch| ch.is_ascii_digit()) {
        if text.len() > cfg.number_max_digits_count {
            return None;
        }
        text.parse().ok()
    } else {
        Some(roman_value(text)? + ROMAN_KEY_OFFSET)
    }
}

/// The lines of the start of an entry title that wrapped into a block of
/// its own, taken off the end of `out`: the last node, still unclassified,
/// on the page of `node`, whose last line stands right above `first` (the
/// first line of an entry), in the same size, and fills most of the row.
fn dangling_title(out: &mut Vec<Node>, node: &Node, first: &Line, cfg: &ContentsConfig) -> Option<Vec<Line>> {
    let prev = out.last()?;
    let NodeKind::Text(block) = &prev.kind else { return None };
    let last = block.lines.last()?;
    let wraps = prev.class == Class::Unknown
        && prev.loc.page == node.loc.page
        && block.lines.len() <= cfg.wrap_max_lines_count
        && first.baseline > last.baseline
        && first.baseline - last.baseline <= cfg.wrap_max_gap_em * first.size
        && (first.size - last.size).abs() <= cfg.wrap_size_tolerance_pt
        && last.bbox.width() >= cfg.wrap_min_fill_ratio * (first.bbox.x1 - last.bbox.x0);
    if !wraps {
        return None;
    }
    let NodeKind::Text(block) = out.pop()?.kind else {
        return None;
    };
    Some(block.lines)
}

/// The page label ending each of `lines`, if any (see [`entry_page`]).
fn entry_labels(lines: &[Line], cfg: &ContentsConfig) -> Vec<Option<PageLabel>> {
    (0..lines.len())
        .map(|i| entry_page(&lines[i], i.checked_sub(1).map(|prev| &lines[prev]), cfg))
        .collect()
}

/// Whether a line is nothing but a page number.
fn is_bare_number(line: &Line) -> bool {
    let text = line.text();
    let text = text.trim();
    !text.is_empty() && (text.chars().all(|ch| ch.is_ascii_digit()) || text.chars().all(|ch| "ivxlc".contains(ch)))
}

/// The page number at the end of a line of contents: digits (or a roman
/// numeral) set apart from a title before them by leader dots or a wide gap.
/// A line that is only a number is the page number of the line before it
/// (`prev`), when that one stands on the same baseline, well to its left
/// (the numbers were cut off as a column of their own).
fn entry_page(line: &Line, prev: Option<&Line>, cfg: &ContentsConfig) -> Option<PageLabel> {
    if is_bare_number(line) {
        let prev = prev?;
        let text = line.text().trim().to_string();
        let key = page_key(&text, cfg)?;
        let same_row = (line.baseline - prev.baseline).abs() <= cfg.row_max_offset_em * line.size;
        let apart = line.bbox.x0 - prev.bbox.x1 >= cfg.number_min_gap_em * line.size;
        return (same_row && apart && prev.text().chars().any(char::is_alphabetic)).then_some(PageLabel { text, key });
    }
    let glyphs: Vec<&LineGlyph> = line
        .glyphs
        .iter()
        .filter(|glyph| !glyph.text.trim().is_empty())
        .collect();
    // Digits, or else roman numerals: never both ("Model 218" is not "l218").
    let run = |accept: fn(char) -> bool| {
        glyphs
            .iter()
            .rev()
            .take_while(|glyph| glyph.text.chars().all(accept))
            .count()
    };
    let digits = run(|ch| ch.is_ascii_digit());
    let number_len = if digits > 0 {
        digits
    } else {
        run(|ch| "ivxlc".contains(ch))
    };
    if number_len == 0 || number_len == glyphs.len() {
        return None;
    }
    let first = glyphs.len() - number_len;
    let text: String = glyphs[first..].iter().map(|glyph| glyph.text.as_str()).collect();
    let key = page_key(&text, cfg)?;
    let before = glyphs[first - 1];
    let leaders = glyphs[..first]
        .iter()
        .rev()
        .take_while(|glyph| glyph.text.chars().all(|ch| LEADER_CHARS.contains(&ch)))
        .map(|glyph| glyph.text.chars().count())
        .sum::<usize>();
    let title_len = first
        - glyphs[..first]
            .iter()
            .rev()
            .take_while(|glyph| glyph.text.chars().all(|ch| LEADER_CHARS.contains(&ch)))
            .count();
    let apart = leaders >= cfg.leader_min_dots_count
        || glyphs[first].x0 - before.x1 >= cfg.number_min_gap_em * before.size.max(glyphs[first].size);
    (apart && title_len > 0 && before.text != ",").then_some(PageLabel { text, key })
}

/// Clusters of nearby values (sorted, each within `tolerance` of the
/// previous one): (mean, count) per cluster, left to right.
fn clusters(mut values: Vec<f32>, tolerance: f32) -> Vec<(f32, usize)> {
    values.sort_by(f32::total_cmp);
    let mut groups: Vec<Vec<f32>> = Vec::new();
    for value in values {
        match groups.last_mut() {
            Some(group) if value - group[group.len() - 1] <= tolerance => group.push(value),
            _ => groups.push(vec![value]),
        }
    }
    groups
        .into_iter()
        .map(|group| (group.iter().sum::<f32>() / group.len() as f32, group.len()))
        .collect()
}

/// The level of each entry: the rank of its indent among the indents of
/// all entries, the smallest first. Pages are lined up on the page with the
/// most entries (left and right pages have different margins);
/// an indent that only a few entries have (a two-digit chapter number that
/// hangs further left) joins the nearest common one.
fn indent_levels(entries: &[Entry], cfg: &ContentsConfig) -> Vec<u8> {
    let mut by_page: std::collections::BTreeMap<usize, Vec<f32>> = std::collections::BTreeMap::new();
    for entry in entries {
        by_page.entry(entry.page).or_default().push(entry.x0);
    }
    // The page with the most entries is the template; every other page is
    // shifted by the amount that puts most of its entries on the
    // template's indents.
    let template: Vec<f32> = by_page
        .values()
        .max_by_key(|xs| xs.len())
        .map(|xs| {
            clusters(xs.clone(), cfg.indent_tolerance_pt)
                .iter()
                .map(|group| group.0)
                .collect()
        })
        .unwrap_or_default();
    let fits = |xs: &[f32], shift: f32| {
        xs.iter()
            .filter(|&&x| {
                template
                    .iter()
                    .any(|indent| (x + shift - indent).abs() <= cfg.indent_tolerance_pt)
            })
            .count()
    };
    let anchor: std::collections::HashMap<usize, f32> = by_page
        .into_iter()
        .map(|(page, xs)| {
            let own: Vec<f32> = clusters(xs.clone(), cfg.indent_tolerance_pt)
                .iter()
                .map(|group| group.0)
                .collect();
            let shift = template
                .iter()
                .flat_map(|indent| own.iter().map(move |x| indent - x))
                .chain(std::iter::once(0.0))
                .max_by(|a, b| fits(&xs, *a).cmp(&fits(&xs, *b)).then(b.abs().total_cmp(&a.abs())))
                .unwrap_or(0.0);
            (page, -shift)
        })
        .collect();
    let indent_of = |entry: &Entry| entry.x0 - anchor[&entry.page];
    let groups = clusters(entries.iter().map(indent_of).collect(), cfg.indent_tolerance_pt);
    let min_count = (cfg.level_min_entries_ratio * entries.len() as f32).ceil() as usize;
    let common: Vec<f32> = groups
        .iter()
        .filter(|group| group.1 >= min_count)
        .map(|group| group.0)
        .collect();
    // Each group's level: its own rank when common, else that of the
    // nearest common group within reach.
    let mut steps: Vec<(f32, f32)> = Vec::new();
    for &(mean, count) in &groups {
        let target = if count >= min_count {
            Some(mean)
        } else {
            common
                .iter()
                .copied()
                .filter(|common| (common - mean).abs() <= cfg.indent_merge_max_pt)
                .min_by(|a, b| (a - mean).abs().total_cmp(&(b - mean).abs()))
        };
        steps.push((mean, target.unwrap_or(mean)));
    }
    let mut ranks: Vec<f32> = steps.iter().map(|step| step.1).collect();
    ranks.sort_by(f32::total_cmp);
    ranks.dedup();
    entries
        .iter()
        .map(|entry| {
            let indent = indent_of(entry);
            let target = steps
                .iter()
                .min_by(|a, b| (a.0 - indent).abs().total_cmp(&(b.0 - indent).abs()))
                .map_or(indent, |step| step.1);
            let rank = ranks.iter().filter(|&&rank| rank <= target).count();
            (rank.max(1) as u8).min(cfg.max_level)
        })
        .collect()
}

/// Removes the page number and the leaders before it from the end of the
/// text of an entry.
pub(crate) fn strip_page_label(spans: &mut Vec<Span>, label: &str) {
    let mut left = label.chars().count();
    while left > 0 {
        let Some(last) = spans.last_mut() else { return };
        let trimmed = last.text.trim_end().to_string();
        let len = trimmed.chars().count();
        if len <= left {
            left -= len;
            spans.pop();
        } else {
            last.text = trimmed.chars().take(len - left).collect();
            left = 0;
        }
    }
    while let Some(last) = spans.last_mut() {
        let kept = last
            .text
            .trim_end_matches(|ch: char| ch.is_whitespace() || LEADER_CHARS.contains(&ch))
            .to_string();
        if kept.is_empty() {
            spans.pop();
        } else {
            last.text = kept;
            break;
        }
    }
}

/// The words (see [`title_key`]) of the title of an entry whose text (page
/// number included) is `text`.
pub(crate) fn entry_key(text: &str, page: &str) -> String {
    let title = text.trim_end();
    let title = title.strip_suffix(page).unwrap_or(title);
    title_key(title.trim_end_matches(|ch: char| ch.is_whitespace() || LEADER_CHARS.contains(&ch)))
}

/// Levels of the entries of the tables of contents of `nodes`, by the
/// words of their titles (see [`title_key`]).
pub(crate) fn entry_levels(nodes: &[Node]) -> std::collections::HashMap<String, u8> {
    let mut levels = std::collections::HashMap::new();
    for node in nodes {
        if let (Class::ContentsEntry { level, page }, NodeKind::Text(block)) = (&node.class, &node.kind) {
            let key = entry_key(&super::block_text(&block.lines), page);
            if !key.is_empty() {
                levels.entry(key).or_insert(*level);
            }
        }
    }
    levels
}

/// Words of a title, for matching an entry with its heading: lower case,
/// letters and digits only, without a leading "chapter N" or section number.
pub(crate) fn title_key(text: &str) -> String {
    let words: Vec<String> = text
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut start = 0;
    if words
        .first()
        .is_some_and(|word| word == "chapter" || word == "part" || word == "appendix")
    {
        start = 1;
    }
    while words
        .get(start)
        .is_some_and(|word| word.chars().all(|ch| ch.is_ascii_digit()))
    {
        start += 1;
    }
    words[start.min(words.len())..].join(" ")
}

#[cfg(test)]
mod tests {
    use super::{strip_page_label, title_key};
    use crate::model::Span;

    #[test]
    fn page_label_and_leaders_are_stripped() {
        let mut spans = vec![Span::plain("Preface. . . . . . . "), Span::plain("xi")];
        strip_page_label(&mut spans, "xi");
        assert_eq!(spans, vec![Span::plain("Preface")]);
        let mut spans = vec![Span::plain("The Rise of AI Engineering 2")];
        strip_page_label(&mut spans, "2");
        assert_eq!(spans, vec![Span::plain("The Rise of AI Engineering")]);
    }

    #[test]
    fn titles_match_without_chapter_numbers() {
        assert_eq!(title_key("1. Introduction to AI"), "introduction to ai");
        assert_eq!(title_key("CHAPTER 1 Introduction to AI"), "introduction to ai");
        assert_eq!(title_key("Bits-per-Character"), "bits per character");
    }
}
