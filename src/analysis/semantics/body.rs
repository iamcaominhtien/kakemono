//! Classification of body blocks: headings, paragraphs, list items, references.
//!
//! Runs after [`classify_front_matter`] on the
//! nodes it left [`Class::Unknown`], in two passes:
//!
//! 1. **Headings.** Short text blocks are tested against a sequence of
//!    rules (numbered section, Roman numeral, appendix letter, italic letter,
//!    known heading word, styled short line). Numbered headings get their
//!    level from the number; the others are ranked by font size against the
//!    numbered ones.
//! 2. **Running text.** The remaining text blocks become abstract
//!    paragraphs, references (after a "References" heading), list items or
//!    plain paragraphs, depending on the last heading seen.

use super::patterns::*;
use super::run_in::split_run_in_headings;
use super::*;
use crate::analysis::blocks::LIST_LABEL;
use crate::analysis::page::DocStats;
use crate::backend::FontInfo;
use crate::common::geom::median;
use regex::Captures;
use std::collections::{HashMap, HashSet};

/// Deepest heading level in the output; deeper numbered sections
/// ("1.2.3.4.5.6") are clamped to it.
const MAX_HEADING_LEVEL: u8 = 5;

/// Resolution of the font sizes used as keys when counting characters per
/// size: sizes are compared in tenths of a point.
const SIZE_KEY_PER_PT: f32 = 10.0;

/// Tuning parameters of body classification. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyConfig {
    /// Blocks with more lines than this are never headings.
    pub heading_max_lines_count: usize,
    /// Fewest characters of a heading (a single character is a stray label).
    pub heading_min_chars_count: usize,
    /// Most characters of a heading, numbered or not.
    pub heading_max_chars_count: usize,
    /// Share of bold glyphs from which a block counts as bold.
    pub bold_min_ratio: f32,
    /// A block with at least this share of bold glyphs counts as bold too
    /// when its bold and italic glyphs together reach `bold_min_ratio` (a
    /// bold heading with a word in italics).
    pub bold_with_italic_min_ratio: f32,
    /// Share of italic glyphs from which a block counts as italic.
    pub italic_min_ratio: f32,
    /// Share of capitals from which a block counts as set in capitals.
    pub caps_min_upper_ratio: f32,
    /// A block at least this much larger than the body text (points) is
    /// "strong", like a bold one.
    pub strong_min_size_above_body_pt: f32,
    /// Most words of an italic "A. Title" heading; longer italic text with a
    /// letter label is a list item or a run-in paragraph.
    pub letter_heading_max_words_count: usize,
    /// A block that ends with a period and has more words than this is a
    /// sentence, not a heading, numbered or not.
    pub sentence_max_heading_words_count: usize,
    /// The last line of a block reaching to within this distance (points) of
    /// the block's right edge is a full line, as in running text.
    pub full_line_right_tolerance_pt: f32,
    /// Fewest lines a block needs before a full last line marks it as prose.
    pub full_line_min_lines_count: usize,
    /// Most words of a known heading word ("Introduction", "References")
    /// accepted without a number; longer text merely starts with it.
    pub known_heading_max_words_count: usize,
    /// Most lines of an unnumbered heading recognised by its style alone.
    pub styled_heading_max_lines_count: usize,
    /// Most characters of an unnumbered heading recognised by its style alone.
    pub styled_heading_max_chars_count: usize,
    /// Most words of an unnumbered heading recognised by its style alone.
    pub styled_heading_max_words_count: usize,
    /// Size of level-1 headings above the body size (points), assumed when
    /// the document has no numbered level-1 heading to measure.
    pub default_h1_size_above_body_pt: f32,
    /// An unnumbered heading within this distance (points) below the typical
    /// size of a level is given that level.
    pub level_size_tolerance_pt: f32,
    /// Fewest unnumbered headings that the table of contents must list for
    /// its levels to be used.
    pub contents_min_headings_count: usize,
    /// An unnumbered level-2 heading must be at least this much larger than
    /// the body text (points); smaller ones are level 3.
    pub h2_min_size_above_body_pt: f32,
    /// An unnumbered, unknown heading text that occurs at least this many
    /// times in the document is a repeated label ("GPT-4 (early) response"),
    /// not a heading.
    pub repeated_label_min_count: usize,
    /// An unnumbered heading whose centre is within this distance (points)
    /// of the centre of its column, on a page after the first, is the title
    /// block of an embedded document (a system card after a report), not a
    /// section heading.
    pub centered_title_tolerance_pt: f32,
    /// The abstract ends at the latest this many pages after its heading.
    pub abstract_max_pages_count: usize,
    /// Last page index (0-based) on which an inline "Abstract—" label starts
    /// the abstract; later it is ordinary text.
    pub inline_abstract_max_page_idx: usize,
    /// A paragraph after a list item whose left edge is within this
    /// distance (em) of the item's text (its second line) continues the
    /// item.
    pub list_continuation_max_offset_em: f32,
    /// ... provided the item's text is indented by at least this much (em)
    /// from its label.
    pub list_continuation_min_indent_em: f32,
    /// A sentence of a language course is mostly in CJK characters (at
    /// least this share), and its translation mostly in Latin letters.
    pub translated_min_script_ratio: f32,
    /// A bibliography without a heading (Physical Review Letters) is a run
    /// of at least this many paragraphs labelled `[1]`, `[2]`, ... in
    /// increasing order.
    pub unheaded_refs_min_entries_count: usize,
}

impl Default for BodyConfig {
    fn default() -> Self {
        Self {
            heading_max_lines_count: 3,
            heading_min_chars_count: 2,
            heading_max_chars_count: 160,
            bold_min_ratio: 0.9,
            bold_with_italic_min_ratio: 0.5,
            italic_min_ratio: 0.7,
            caps_min_upper_ratio: 0.8,
            strong_min_size_above_body_pt: 0.7,
            letter_heading_max_words_count: 12,
            sentence_max_heading_words_count: 6,
            full_line_right_tolerance_pt: 2.0,
            full_line_min_lines_count: 3,
            known_heading_max_words_count: 4,
            styled_heading_max_lines_count: 2,
            styled_heading_max_chars_count: 120,
            styled_heading_max_words_count: 16,
            default_h1_size_above_body_pt: 1.5,
            level_size_tolerance_pt: 0.3,
            contents_min_headings_count: 3,
            h2_min_size_above_body_pt: 0.3,
            repeated_label_min_count: 5,
            centered_title_tolerance_pt: 4.0,
            abstract_max_pages_count: 1,
            inline_abstract_max_page_idx: 1,
            list_continuation_max_offset_em: 0.3,
            list_continuation_min_indent_em: 0.5,
            translated_min_script_ratio: 0.6,
            unheaded_refs_min_entries_count: 5,
        }
    }
}

/// A block accepted as a heading by pass 1, before its level is final.
struct HeadingCandidate {
    /// Index of the node.
    idx: usize,
    /// Level given by the section number; `None` for unnumbered headings.
    level: Option<u8>,
    /// The section number as printed.
    number: Option<String>,
    /// Font size of the block.
    size: f32,
    /// The text is a known heading word ("Introduction", "References").
    known: bool,
    /// The block's text, trimmed.
    text: String,
}

/// Measurements of a short text block that the heading rules look at.
struct HeadingFeatures<'a> {
    /// The block's text, trimmed.
    text: &'a str,
    /// Number of characters of `text`.
    nchars: usize,
    /// Number of words of `text`.
    words: usize,
    /// Median font size of the block.
    size: f32,
    /// Set in a bold font.
    bold: bool,
    /// Bold, or clearly larger than the body text.
    strong: bool,
    /// Mostly italic.
    italic: bool,
    /// Set in capitals or small caps.
    caps: bool,
    /// Italic or in capitals: a weaker heading style than `strong`.
    weak: bool,
    /// The text is a known heading word.
    known: bool,
}

/// Section number and level read from a heading's label.
struct HeadingNumber {
    /// Level implied by the number.
    level: u8,
    /// The number as printed.
    number: String,
}

/// State of pass 2: the section the running text is in.
struct FlowState {
    /// Inside the abstract.
    in_abstract: bool,
    /// Inside the bibliography.
    in_refs: bool,
    /// Page on which the abstract started.
    abstract_page: usize,
}

/// Classifies the body: cuts run-in headings off their paragraphs, marks headings (with level and number), then
/// paragraphs (inside or outside the abstract), references and list items.
/// Only nodes still [`Class::Unknown`] are classified; non-text nodes are
/// left alone.
pub(crate) fn classify_body(nodes: &mut Vec<Node>, stats: &DocStats, fonts: &[FontInfo], cfg: &SemanticsConfig) {
    super::contents::mark_contents(nodes, fonts, cfg);
    super::code::mark_code(nodes, fonts, cfg);
    split_run_in_headings(nodes, fonts, cfg);
    let mut candidates = find_heading_candidates(nodes, stats, fonts, cfg);
    add_bare_chapter_numbers(nodes, &mut candidates, fonts, &cfg.body);
    assign_heading_levels(nodes, candidates, stats, &cfg.body);
    unmark_contents_entries(nodes);
    classify_running_text(nodes, &cfg.body);
    mark_list_continuations(nodes, &cfg.body);
}

/// Turns the entries of a book's table of contents that were taken for
/// headings ("1 The way of the program", "Preface v": bold, short, often
/// numbered) back into plain text, so that they do not fill the outline of
/// the document. The table of contents is the run of text blocks after a
/// "Contents" heading that are entries: blocks ending in leader dots or a
/// page number (see [`CONTENTS_ENTRY_END`]), and headings followed by such
/// a block (a chapter whose page number was not read). The run ends at the
/// first other block, or at a chapter label set alone ("Chapter 1"), which
/// opens the first chapter.
fn unmark_contents_entries(nodes: &mut [Node]) {
    let text_of = |node: &Node| node.text_block().map(|block| block_text(&block.lines));
    // A chapter label set alone ("Chapter 1") ends in a number too, but
    // starts the first chapter after the table of contents.
    let is_entry = |node: &Node| {
        text_of(node).is_some_and(|text| {
            let text = text.trim();
            CONTENTS_ENTRY_END.is_match(text) && CHAPTER_LABEL.find(text).is_none_or(|label| label.end() < text.len())
        })
    };
    let mut idx = 0;
    while idx < nodes.len() {
        let starts = matches!(nodes[idx].class, Class::Heading { .. })
            && text_of(&nodes[idx]).is_some_and(|text| CONTENTS_HEADING.is_match(text.trim()));
        idx += 1;
        if !starts {
            continue;
        }
        while idx < nodes.len() {
            let heading = matches!(nodes[idx].class, Class::Heading { .. });
            let entry = is_entry(&nodes[idx]) || (heading && nodes.get(idx + 1).is_some_and(is_entry));
            if !entry {
                break;
            }
            if heading {
                nodes[idx].class = Class::Unknown;
            }
            idx += 1;
        }
    }
}

/// Marks the paragraphs that continue a list item: a paragraph right after
/// an item (or another continuation), in the same column, whose lines start
/// where the item's text does, to the right of the item's label.
fn mark_list_continuations(nodes: &mut [Node], cfg: &BodyConfig) {
    // The left edge of the text of the list item being continued.
    let mut item_text_left: Option<f32> = None;
    for idx in 0..nodes.len() {
        let Some(block) = nodes[idx].text_block() else {
            continue;
        };
        let em = block.lines.first().map_or(0.0, |line| line.size);
        match nodes[idx].class {
            Class::ListItem { .. } => {
                item_text_left = match block.lines.as_slice() {
                    [first, second, ..]
                        if second.bbox.x0 - first.bbox.x0 >= cfg.list_continuation_min_indent_em * em =>
                    {
                        Some(second.bbox.x0)
                    }
                    _ => None,
                };
                continue;
            }
            Class::ListContinuation => continue,
            Class::Paragraph { in_abstract: false } => {}
            _ => {
                item_text_left = None;
                continue;
            }
        }
        let same_column = idx > 0 && nodes[idx - 1].loc.page == nodes[idx].loc.page;
        let aligned =
            item_text_left.is_some_and(|left| (block.left - left).abs() <= cfg.list_continuation_max_offset_em * em);
        if same_column && aligned {
            nodes[idx].class = Class::ListContinuation;
        } else {
            item_text_left = None;
        }
    }
}

/// Whether the text block `nodes[idx]`, of text `text`, is a sentence of a
/// language course rather than a heading: a line of dialogue ("Ａ：はい、
/// 何ですか？"), or an unnumbered sentence written mostly in CJK characters
/// that ends with a full stop, or with other punctuation and is followed by
/// its translation (a block mostly in Latin letters, in a smaller size).
fn is_translated_sentence(nodes: &[Node], idx: usize, text: &str, cfg: &BodyConfig) -> bool {
    if DIALOGUE_LINE.is_match(text) {
        return true;
    }
    // A numbered task ("2 自分の近況を話しましょう。") stays a heading.
    let numbered = text
        .trim_start_matches(['(', '（', ' '])
        .starts_with(|ch: char| ch.is_ascii_digit());
    let ends_sentence = !numbered && text.ends_with(['。', '？', '！', '?', '!']);
    if !ends_sentence || script_ratio(text, is_cjk) < cfg.translated_min_script_ratio {
        return false;
    }
    // A full stop ends a sentence: headings do not end with one.
    if text.ends_with('。') {
        return true;
    }
    let (Some(block), Some(next)) = (nodes[idx].text_block(), nodes.get(idx + 1)) else {
        return false;
    };
    let Some(next_block) = next.text_block().filter(|_| next.loc.page == nodes[idx].loc.page) else {
        return false;
    };
    let next_text = block_text(&next_block.lines);
    script_ratio(&next_text, |ch| ch.is_ascii_alphabetic()) >= cfg.translated_min_script_ratio
        && block_size(&next_block.lines) < block_size(&block.lines)
}

/// The share of the non-space characters of `text` that `pred` accepts.
fn script_ratio(text: &str, pred: impl Fn(char) -> bool) -> f32 {
    // Blanks to fill in ("____") are not part of the script.
    let chars: Vec<char> = text.chars().filter(|ch| !ch.is_whitespace() && *ch != '_').collect();
    if chars.is_empty() {
        return 0.0;
    }
    chars.iter().filter(|&&ch| pred(ch)).count() as f32 / chars.len() as f32
}

/// A Chinese or Japanese character (kana, kanji) or CJK punctuation.
fn is_cjk(ch: char) -> bool {
    matches!(ch, '\u{3000}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}' | '\u{FF00}'..='\u{FFEF}')
}

/// Pass 1: the unclassified text blocks that look like headings, in
/// document order. Once the bibliography heading has been seen, appendix
/// labels without the word "Appendix" ("A Proofs") are accepted too.
/// Closing quotes and brackets that may follow the punctuation ending a
/// sentence ("… the wrong way to go?”").
const CLOSING_QUOTES: &[char] = &['\u{201D}', '\u{2019}', '"', '\'', ')', ']'];

fn find_heading_candidates(
    nodes: &[Node],
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) -> Vec<HeadingCandidate> {
    let mut candidates: Vec<HeadingCandidate> = Vec::new();
    let mut seen_refs = false;
    // The numbers of the numbered headings found so far.
    let mut numbers: HashSet<String> = HashSet::new();
    // The node of the last chapter label set alone ("CHAPTER TWO"), whose
    // title may follow on a line of its own.
    let mut last_label: Option<usize> = None;
    let page_body_sizes = page_body_sizes(nodes, stats);
    for (idx, node) in nodes.iter().enumerate() {
        if node.class != Class::Unknown {
            continue;
        }
        let Some(block) = node.text_block() else { continue };
        let text = block_text(&block.lines);
        let body_size = page_body_sizes.get(node.loc.page).copied().unwrap_or(stats.body_size);
        let Some(features) = heading_features(block, text.trim(), body_size, fonts, cfg) else {
            continue;
        };
        if THEOREM_LABEL.is_match(features.text) || is_translated_sentence(nodes, idx, features.text, &cfg.body) {
            continue;
        }
        let sentence = is_list_sentence(&features, &cfg.body);
        let label = chapter_label_number(&features, block.lines.len());
        let label_title = idx > 0
            && last_label == Some(idx - 1)
            && nodes[idx - 1].loc.page == node.loc.page
            && is_chapter_title(&features, block.lines.len(), &cfg.body);
        if label.is_some() {
            last_label = Some(idx);
        }
        let number = label
            .or_else(|| heading_number(&features, seen_refs, &cfg.body).filter(|_| !sentence))
            .or_else(|| child_number(&features, block.lines.len(), &numbers, &cfg.body));
        if let Some(found) = &number {
            numbers.insert(found.number.clone());
        }
        let is_heading = number.is_some()
            || label_title
            || is_known_heading(&features, &cfg.body)
            || is_styled_heading(&features, block, &cfg.body);
        if !is_heading || (number.is_none() && !features.known && is_embedded_title(node, block, &features, &cfg.body))
        {
            continue;
        }
        if REF_HEADING.is_match(features.text) {
            seen_refs = true;
        }
        let (level, number) = match number {
            Some(found) => (Some(found.level), Some(found.number)),
            None => (None, None),
        };
        candidates.push(HeadingCandidate {
            idx,
            level,
            number,
            size: features.size,
            known: features.known,
            text: features.text.to_string(),
        });
    }
    drop_repeated_labels(candidates, &cfg.body)
}

/// Adds to the `candidates` the chapters headed by their number alone ("2"
/// between "1 ROGER" and "3 ANDOVER", the chapters of a detective story
/// told in the third person): an unclassified block of one line holding
/// only an Arabic or Roman number, of the size and weight of the numbered
/// level-1 headings, and in sequence with them: the chapter before it has
/// the number before, or the chapter after it the number after (counting
/// the bare chapters already accepted, so that "24", "25", "26" follow
/// "23"). A number some chapter already has, or out of sequence (a page
/// number, a label), stays text.
fn add_bare_chapter_numbers(
    nodes: &[Node],
    candidates: &mut Vec<HeadingCandidate>,
    fonts: &[FontInfo],
    cfg: &BodyConfig,
) {
    let bold = |idx: usize| {
        nodes[idx]
            .text_block()
            .is_some_and(|block| text_frac(&block.lines, fonts, |font| font.bold) >= cfg.bold_min_ratio)
    };
    // (node, value, size, bold) of the numbered chapters, in order.
    let mut chapters: Vec<(usize, u32, f32, bool)> = candidates
        .iter()
        .filter(|candidate| candidate.level == Some(1))
        .filter_map(|candidate| {
            let value = chapter_value(candidate.number.as_deref()?)?;
            Some((candidate.idx, value, candidate.size, bold(candidate.idx)))
        })
        .collect();
    if chapters.is_empty() {
        return;
    }
    let taken: HashSet<usize> = candidates.iter().map(|candidate| candidate.idx).collect();
    let mut bare: Vec<(usize, u32, f32, String)> = nodes
        .iter()
        .enumerate()
        .filter(|(idx, node)| node.class == Class::Unknown && !taken.contains(idx))
        .filter_map(|(idx, node)| {
            let block = node.text_block().filter(|block| block.lines.len() == 1)?;
            let text = block_text(&block.lines).trim().to_string();
            let value = BARE_NUMBER.is_match(&text).then(|| chapter_value(&text)).flatten()?;
            Some((idx, value, block_size(&block.lines), text))
        })
        .collect();
    // Accept the bare numbers next in sequence until none is left to add.
    loop {
        let found = bare.iter().position(|&(idx, value, size, _)| {
            let before = chapters.iter().rev().find(|chapter| chapter.0 < idx);
            let after = chapters.iter().find(|chapter| chapter.0 > idx);
            let follows = |chapter: Option<&(usize, u32, f32, bool)>, expected: u32| {
                chapter.is_some_and(|&(_, number, chapter_size, chapter_bold)| {
                    number == expected
                        && (size - chapter_size).abs() <= cfg.level_size_tolerance_pt
                        && bold(idx) == chapter_bold
                })
            };
            !chapters.iter().any(|chapter| chapter.1 == value)
                && (follows(before, value.wrapping_sub(1)) || follows(after, value + 1))
        });
        let Some(found) = found else { break };
        let (idx, value, size, text) = bare.remove(found);
        let at = chapters.partition_point(|chapter| chapter.0 < idx);
        chapters.insert(at, (idx, value, size, bold(idx)));
        candidates.push(HeadingCandidate {
            idx,
            level: Some(1),
            number: Some(text.clone()),
            size,
            known: false,
            text,
        });
    }
    candidates.sort_by_key(|candidate| candidate.idx);
}

/// The value of a chapter number in figures ("12") or Roman numerals
/// ("IV"), or `None` for a section number of several parts or a word.
fn chapter_value(number: &str) -> Option<u32> {
    if let Ok(value) = number.parse() {
        return Some(value);
    }
    let digit = |ch: char| match ch {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        _ => None,
    };
    let digits: Vec<u32> = number.chars().map(digit).collect::<Option<_>>()?;
    // A numeral before a larger one is subtracted ("IV", "XC").
    let value = digits.iter().enumerate().fold(0i64, |sum, (pos, &numeral)| {
        if digits.get(pos + 1).is_some_and(|&next| next > numeral) {
            sum - i64::from(numeral)
        } else {
            sum + i64::from(numeral)
        }
    });
    u32::try_from(value).ok().filter(|&value| value > 0)
}

/// Whether a block that reads as numbered is an item of a list rather than
/// a numbered heading: a sentence not set in bold ("4. Use the modified
/// version of do_twice to call print_twice twice.", "1. In a print
/// statement, what happens if you leave out one of the parentheses?"); a
/// short one set only in italics ("2. Turn left 60 degrees."); and one so
/// set that is long or stops on a lowercase word, cut by a formula or code
/// after it ("3. Add methods to"), unless its number has several parts
/// ("2.1. 2-point weak lensing model"). Headings in capitals ("4.2 COMPARISON
/// TO STATE OF THE ART") and in bold ("1.1 What is a program?") stay.
fn is_list_sentence(features: &HeadingFeatures<'_>, cfg: &BodyConfig) -> bool {
    let long = features.words > cfg.sentence_max_heading_words_count;
    let ends_sentence = features
        .text
        .trim_end_matches(CLOSING_QUOTES)
        .ends_with(['.', '?', '!', ':']);
    let unfinished = features
        .text
        .split_whitespace()
        .last()
        .is_some_and(|word| word.starts_with(|ch: char| ch.is_lowercase()));
    let only_italic = !features.strong && !features.caps;
    // "4.1. Simulation-based vs. analytical model: a comparison",
    // "Appendix A.1: Emulation strategy": a heading in italics may be long
    // or end on a lowercase word when its number has several parts or the
    // word "Appendix".
    let section = DOTTED_SECTION.is_match(features.text);
    let appendix = features.text.starts_with("Appendix ") || features.text.starts_with("APPENDIX ");
    !features.bold
        && !appendix
        && ((ends_sentence && (long || only_italic)) || (only_italic && !section && (long || unfinished)))
}

/// The body font size of each page: the size carrying most characters of
/// the unclassified text blocks of the page (and of its entries of
/// contents, set in running text size), but never below the document's
/// body size. A system card set in 11 pt inside a report set in 10 pt has
/// body text of 11 pt, which must not count as "larger than the body".
fn page_body_sizes(nodes: &[Node], stats: &DocStats) -> Vec<f32> {
    let pages = nodes.iter().map(|node| node.loc.page + 1).max().unwrap_or(0);
    // Characters per size, in tenths of a point, for each page.
    let mut chars: Vec<HashMap<i32, usize>> = vec![HashMap::new(); pages];
    for node in nodes
        .iter()
        .filter(|node| matches!(node.class, Class::Unknown | Class::ContentsEntry { .. }))
    {
        let Some(block) = node.text_block() else { continue };
        for line in &block.lines {
            *chars[node.loc.page]
                .entry((line.size * SIZE_KEY_PER_PT).round() as i32)
                .or_default() += line.glyphs.len();
        }
    }
    chars
        .iter()
        .map(|sizes| {
            let dominant = sizes
                .iter()
                .max_by_key(|&(key, count)| (*count, *key))
                .map(|(key, _)| *key as f32 / SIZE_KEY_PER_PT)
                .unwrap_or(0.0);
            dominant.max(stats.body_size)
        })
        .collect()
}

/// Whether the (unnumbered) heading block is the title block of a document
/// embedded in the paper: on a page after the first, centred in its column
/// (narrower than it) and large without being bold. Bold centred headings
/// are ordinary section headings (APS style).
fn is_embedded_title(node: &Node, block: &TextBlock, features: &HeadingFeatures<'_>, cfg: &BodyConfig) -> bool {
    let tolerance = cfg.centered_title_tolerance_pt;
    let column_center = (node.leaf_left + node.leaf_right) * 0.5;
    node.loc.page > 0
        && !features.bold
        && (block.bbox.cx() - column_center).abs() <= tolerance
        && block.bbox.x0 - node.leaf_left > tolerance
}

/// Removes the unnumbered candidates whose text repeats at least
/// [`BodyConfig::repeated_label_min_count`] times: a label that the document
/// uses over and over is part of a template, not a section.
fn drop_repeated_labels(candidates: Vec<HeadingCandidate>, cfg: &BodyConfig) -> Vec<HeadingCandidate> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.number.is_none() && !candidate.known)
    {
        *counts.entry(candidate.text.clone()).or_default() += 1;
    }
    candidates
        .into_iter()
        .filter(|candidate| {
            candidate.number.is_some()
                || candidate.known
                || counts.get(&candidate.text).copied().unwrap_or(0) < cfg.repeated_label_min_count
        })
        .collect()
}

/// Measures a block for the heading rules. Returns `None` for blocks too
/// long (in lines or characters) or too short to be a heading.
fn heading_features<'a>(
    block: &TextBlock,
    text: &'a str,
    body_size: f32,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) -> Option<HeadingFeatures<'a>> {
    let body = &cfg.body;
    let nchars = text.chars().count();
    if block.lines.len() > body.heading_max_lines_count
        || !(body.heading_min_chars_count..=body.heading_max_chars_count).contains(&nchars)
    {
        return None;
    }
    let size = block_size(&block.lines);
    // A bold heading may have a word in italics ("Appendix A: The
    // *cosmo*-SLICS Emulator").
    let bold_frac = text_frac(&block.lines, fonts, |font| font.bold);
    let bold = bold_frac >= body.bold_min_ratio
        || (bold_frac >= body.bold_with_italic_min_ratio
            && text_frac(&block.lines, fonts, |font| font.bold || font.italic) >= body.bold_min_ratio);
    let italic = text_frac(&block.lines, fonts, |font| font.italic) >= body.italic_min_ratio;
    let caps = upper_ratio(text, cfg) >= body.caps_min_upper_ratio || small_caps(&block.lines, cfg);
    let strong = bold || size >= body_size + body.strong_min_size_above_body_pt;
    Some(HeadingFeatures {
        text,
        nchars,
        words: text.split_whitespace().count(),
        size,
        bold,
        strong,
        italic,
        caps,
        weak: italic || caps,
        known: KNOWN_HEADINGS.is_match(text.trim_end_matches(['.', ':'])),
    })
}

/// The section number of a heading, from the first numbering rule whose
/// pattern matches. A text that looks like "2 layers" (Arabic number but
/// lowercase title) gets no number and is not tried against the other
/// patterns.
fn heading_number(features: &HeadingFeatures<'_>, seen_refs: bool, cfg: &BodyConfig) -> Option<HeadingNumber> {
    if let Some(captures) = HEADING_NUM.captures(features.text) {
        return arabic_number_rule(&captures, features);
    }
    roman_number_rule(features)
        .or_else(|| appendix_number_rule(features, seen_refs))
        .or_else(|| letter_number_rule(features, cfg))
}

/// "3.2 Attention": an Arabic number; the level is the number of parts. The
/// title must start with a capital (or be all caps) and the block must be
/// styled as a heading.
fn arabic_number_rule(captures: &Captures<'_>, features: &HeadingFeatures<'_>) -> Option<HeadingNumber> {
    let rest = &captures["rest"];
    // "2 layers" is not a heading; a heading title starts with a
    // capital letter (or is all caps), or with a number after a section
    // number of several parts ("2.1. 2-point weak lensing model").
    let multi_part = captures["num"].contains('.');
    let title_start = rest
        .chars()
        .next()
        .is_some_and(|ch| ch.is_uppercase() || (multi_part && ch.is_ascii_digit()));
    if title_start && (features.strong || features.weak) {
        Some(dotted_number(&captures["num"]))
    } else {
        None
    }
}

/// "4.1.1. Diagonal elements" set like the body text, under the heading
/// "4.1": a block of one line (`lines_count`) numbered as a child of a
/// heading found before, with a short title starting with a capital, is a
/// heading too (A&A sets its third level in the plain sans-serif font).
fn child_number(
    features: &HeadingFeatures<'_>,
    lines_count: usize,
    numbers: &HashSet<String>,
    cfg: &BodyConfig,
) -> Option<HeadingNumber> {
    let captures = HEADING_NUM.captures(features.text)?;
    let (parent, _) = captures["num"].rsplit_once('.')?;
    let short = features.words <= cfg.letter_heading_max_words_count;
    let capital = captures["rest"].starts_with(char::is_uppercase);
    let ends_sentence = features.text.ends_with(['.', ':']);
    (lines_count == 1 && short && capital && !ends_sentence && numbers.contains(parent))
        .then(|| dotted_number(&captures["num"]))
}

/// "CHAPTER TWO", "Chapter 3": a chapter label set alone on one line, in
/// capitals or otherwise styled (even in the body font, as in a novel made
/// in Word), is a heading of level 1; a number in words is given in figures.
fn chapter_label_number(features: &HeadingFeatures<'_>, lines_count: usize) -> Option<HeadingNumber> {
    whole_chapter_label_number(features.text)
        .filter(|_| lines_count == 1 && (features.strong || features.weak))
        .map(|number| HeadingNumber { level: 1, number })
}

/// Whether a block right after a chapter label set alone is the chapter's
/// title ("THE VANISHING GLASS" under "CHAPTER TWO"): short, in capitals or
/// strong, not a sentence.
fn is_chapter_title(features: &HeadingFeatures<'_>, lines_count: usize, cfg: &BodyConfig) -> bool {
    lines_count <= cfg.styled_heading_max_lines_count
        && features.words <= cfg.letter_heading_max_words_count
        && (features.caps || features.strong)
        && !features
            .text
            .trim_end_matches(CLOSING_QUOTES)
            .ends_with(['.', ',', ';', ':'])
        && whole_chapter_label_number(features.text).is_none()
}

/// "IV. EXPERIMENTS": a Roman numeral, always level 1; the block must be in
/// capitals or strong.
fn roman_number_rule(features: &HeadingFeatures<'_>) -> Option<HeadingNumber> {
    let captures = HEADING_ROMAN
        .captures(features.text)
        .filter(|_| features.caps || features.strong)?;
    Some(HeadingNumber {
        level: 1,
        number: captures["num"].to_string(),
    })
}

/// "Appendix A", "A.1 Proofs": an appendix letter, strong or in capitals
/// (ICLR sets them in small capitals at the body size), or in italics
/// after the word "Appendix". A bare letter ("A Proofs") is only accepted
/// after the bibliography heading.
fn appendix_number_rule(features: &HeadingFeatures<'_>, seen_refs: bool) -> Option<HeadingNumber> {
    let captures = HEADING_APP.captures(features.text).filter(|captures| {
        let word = captures.name("word").is_some();
        // With the word "Appendix", italics are a heading style too, as
        // for "3.2 Attention" ("Appendix A.1: Emulation strategy").
        (seen_refs || word || captures["num"].contains('.'))
            && (features.strong || features.caps || (word && features.weak))
    })?;
    Some(dotted_number(&captures["num"]))
}

/// "A. Datasets" in italics (a level-2 run of IEEE style): short, italic and
/// not strong.
fn letter_number_rule(features: &HeadingFeatures<'_>, cfg: &BodyConfig) -> Option<HeadingNumber> {
    let captures = HEADING_LETTER
        .captures(features.text)
        .filter(|_| features.italic && !features.strong && features.words <= cfg.letter_heading_max_words_count)?;
    Some(HeadingNumber {
        level: 2,
        number: captures["num"].to_string(),
    })
}

/// A dotted section number ("3", "3.2", "A.1") with the level given by its
/// number of parts.
fn dotted_number(num: &str) -> HeadingNumber {
    HeadingNumber {
        level: 1 + num.matches('.').count() as u8,
        number: num.to_string(),
    }
}

/// A known heading word ("Introduction", "Acknowledgments"), strong or in
/// capitals, with few words.
fn is_known_heading(features: &HeadingFeatures<'_>, cfg: &BodyConfig) -> bool {
    features.known && (features.strong || features.caps) && features.words <= cfg.known_heading_max_words_count
}

/// An unnumbered heading recognised by its style: strong, short, not a
/// sentence or a lead-in ending in a colon, not a list item, not a
/// continuation (lowercase start) or a note ("CW: ...") and not ending in a
/// full line of running text.
fn is_styled_heading(features: &HeadingFeatures<'_>, block: &TextBlock, cfg: &BodyConfig) -> bool {
    let text = features.text;
    let ends_sentence = text.ends_with(['.', ':']) && features.words > cfg.sentence_max_heading_words_count;
    let last_full = block.lines.len() > 1 && {
        let last = block.lines.last().unwrap();
        last.bbox.x1 > block.right - cfg.full_line_right_tolerance_pt
            && block.lines.len() >= cfg.full_line_min_lines_count
    };
    // Numbered headings were accepted before this rule is tried.
    let list_like = LIST_LABEL.is_match(text);
    features.strong
        && block.lines.len() <= cfg.styled_heading_max_lines_count
        && features.nchars <= cfg.styled_heading_max_chars_count
        && !ends_sentence
        && !last_full
        && !list_like
        && !first_alpha_lower(text)
        && !NOTE_LABEL.is_match(text)
        && features.words <= cfg.styled_heading_max_words_count
        && text.chars().any(|ch| ch.is_alphabetic())
}

/// Marks the candidates as headings. Numbered ones keep the level of their
/// number; unnumbered ones are ranked by size against the median size of
/// the numbered level-1 and level-2 headings (known heading words are
/// always level 1).
fn assign_heading_levels(nodes: &mut [Node], candidates: Vec<HeadingCandidate>, stats: &DocStats, cfg: &BodyConfig) {
    // Level-1 size, to rank unnumbered headings.
    let h1_sizes: Vec<f32> = candidates
        .iter()
        .filter(|candidate| candidate.level == Some(1))
        .map(|candidate| candidate.size)
        .collect();
    let h1_size = if h1_sizes.is_empty() {
        stats.body_size + cfg.default_h1_size_above_body_pt
    } else {
        median(&h1_sizes, stats.body_size + cfg.default_h1_size_above_body_pt)
    };
    let h2_sizes: Vec<f32> = candidates
        .iter()
        .filter(|candidate| candidate.level == Some(2))
        .map(|candidate| candidate.size)
        .collect();
    let h2_size = median(&h2_sizes, stats.body_size);
    let by_contents = contents_levels(nodes, &candidates, cfg);
    for candidate in candidates {
        let level = candidate.level.unwrap_or_else(|| match &by_contents {
            Some(levels) => levels.level_of(&candidate, cfg),
            None => unnumbered_level(&candidate, h1_size, h2_size, stats, cfg),
        });
        nodes[candidate.idx].class = Class::Heading {
            level: level.min(MAX_HEADING_LEVEL),
            number: candidate.number,
        };
    }
}

/// Heading levels taken from a table of contents: the level of the entry
/// each heading names; a heading it does not list takes the level of the
/// listed headings of its size (see [`ContentsLevels::level_of`]).
struct ContentsLevels {
    /// Level of each entry, by the words of its title.
    entries: HashMap<String, u8>,
    /// (level, median size of the headings of that level that it lists).
    sizes: Vec<(u8, f32)>,
}

impl ContentsLevels {
    /// The level of the unnumbered heading `candidate`.
    fn level_of(&self, candidate: &HeadingCandidate, cfg: &BodyConfig) -> u8 {
        if let Some(&level) = self.entries.get(&super::contents::title_key(&candidate.text)) {
            return level;
        }
        // The first listed level whose headings are not larger: a heading
        // the contents leave out ("Table of Contents" itself, the sections
        // of a preface) takes the level of its size; one smaller than every
        // listed level is below them all (worked examples, exercise labels).
        self.sizes
            .iter()
            .find(|&&(_, size)| size <= candidate.size + cfg.level_size_tolerance_pt)
            .map_or(self.sizes[self.sizes.len() - 1].0 + 1, |&(level, _)| level)
    }
}

/// The levels given by the document's table of contents, when it lists at
/// least [`BodyConfig::contents_min_headings_count`] of the candidates.
fn contents_levels(nodes: &[Node], candidates: &[HeadingCandidate], cfg: &BodyConfig) -> Option<ContentsLevels> {
    let entries = super::contents::entry_levels(nodes);
    if entries.is_empty() {
        return None;
    }
    let mut by_level: std::collections::BTreeMap<u8, Vec<f32>> = std::collections::BTreeMap::new();
    for candidate in candidates {
        if let Some(&level) = entries.get(&super::contents::title_key(&candidate.text)) {
            by_level.entry(level).or_default().push(candidate.size);
        }
    }
    let listed: usize = by_level.values().map(Vec::len).sum();
    if listed < cfg.contents_min_headings_count {
        return None;
    }
    let sizes = by_level
        .into_iter()
        .map(|(level, sizes)| (level, median(&sizes, 0.0)))
        .collect();
    Some(ContentsLevels { entries, sizes })
}

/// Level of an unnumbered heading from its size: 1 near the level-1 size
/// (or for a known heading word), 2 near the level-2 size and larger than
/// the body text, 3 otherwise.
fn unnumbered_level(
    candidate: &HeadingCandidate,
    h1_size: f32,
    h2_size: f32,
    stats: &DocStats,
    cfg: &BodyConfig,
) -> u8 {
    if candidate.known || candidate.size >= h1_size - cfg.level_size_tolerance_pt {
        1
    } else if candidate.size >= h2_size - cfg.level_size_tolerance_pt
        && candidate.size > stats.body_size + cfg.h2_min_size_above_body_pt
    {
        2
    } else {
        3
    }
}

/// Pass 2: classifies the remaining text blocks in document order; each
/// heading sets whether the following text is abstract or bibliography.
fn classify_running_text(nodes: &mut [Node], cfg: &BodyConfig) {
    let mut state = FlowState {
        in_abstract: false,
        in_refs: false,
        abstract_page: 0,
    };
    for node in nodes.iter_mut() {
        let NodeKind::Text(block) = &node.kind else { continue };
        if !matches!(node.class, Class::Heading { .. } | Class::Unknown) {
            continue;
        }
        let text = block_text(&block.lines);
        let trimmed = text.trim();
        if let Class::Heading { .. } = node.class {
            state.enter_heading(trimmed, node.loc.page);
            continue;
        }
        node.class = state.classify(node, trimmed, cfg);
    }
    unheaded_references(nodes, cfg);
}

/// Finds a bibliography without a heading: paragraphs that start with
/// `[1]`, `[2]`, ... in increasing order, at least
/// [`BodyConfig::unheaded_refs_min_entries_count`] of them in a row (floats
/// and footnotes between them do not break the run), become references.
fn unheaded_references(nodes: &mut [Node], cfg: &BodyConfig) {
    let mut i = 0;
    while i < nodes.len() {
        if paragraph_label(&nodes[i]) != Some(1) {
            i += 1;
            continue;
        }
        let mut run = vec![i];
        let mut last = 1;
        for (j, node) in nodes.iter().enumerate().skip(i + 1) {
            if !matches!(node.class, Class::Paragraph { .. }) {
                continue;
            }
            match paragraph_label(node) {
                Some(label) if label > last => {
                    run.push(j);
                    last = label;
                }
                _ => break,
            }
        }
        if run.len() >= cfg.unheaded_refs_min_entries_count {
            for &j in &run {
                nodes[j].class = Class::Reference;
            }
        }
        i = run.last().map_or(i, |&end| end) + 1;
    }
}

/// The number of a paragraph that starts with a bracketed number (`[12]`).
fn paragraph_label(node: &Node) -> Option<u32> {
    if !matches!(node.class, Class::Paragraph { in_abstract: false }) {
        return None;
    }
    let NodeKind::Text(block) = &node.kind else { return None };
    let first = block.lines.first()?.text();
    let rest = first.trim_start().strip_prefix('[')?;
    let (number, _) = rest.split_once(']')?;
    number.parse().ok()
}

impl FlowState {
    /// Starts a new section at a heading with the (trimmed) text `heading`
    /// on page `page`.
    fn enter_heading(&mut self, heading: &str, page: usize) {
        self.in_abstract = heading.trim_end_matches(['.', ':']).eq_ignore_ascii_case("abstract");
        self.abstract_page = page;
        self.in_refs = REF_HEADING.is_match(heading);
    }

    /// The class of an unclassified text block with the (trimmed) text
    /// `trimmed`, from the first rule that applies: inline abstract label,
    /// bibliography entry, list item, paragraph. May set the node's
    /// `strip_prefix` (inline abstract label).
    fn classify(&mut self, node: &mut Node, trimmed: &str, cfg: &BodyConfig) -> Class {
        if self.in_abstract && node.loc.page > self.abstract_page + cfg.abstract_max_pages_count {
            self.in_abstract = false;
        }
        if let Some(class) = self.inline_abstract_rule(node, trimmed, cfg) {
            return class;
        }
        if self.in_refs {
            return Class::Reference;
        }
        if let Some(class) = list_item_rule(trimmed) {
            return class;
        }
        Class::Paragraph {
            in_abstract: self.in_abstract,
        }
    }

    /// "Abstract—We propose …" on one of the first pages: starts the
    /// abstract and drops the label from the text.
    fn inline_abstract_rule(&mut self, node: &mut Node, trimmed: &str, cfg: &BodyConfig) -> Option<Class> {
        let label = ABSTRACT_INLINE.find(trimmed)?;
        if node.loc.page > cfg.inline_abstract_max_page_idx {
            return None;
        }
        node.strip_prefix = trimmed[..label.end()].chars().count();
        self.in_abstract = true;
        self.abstract_page = node.loc.page;
        Some(Class::Paragraph { in_abstract: true })
    }
}

/// Fraction (0–1) of the text glyphs of `lines` whose font satisfies `pred`.
/// Only glyphs of ASCII letters and numerals in text fonts count: arrows,
/// brackets, Greek letters and math symbols of a heading such as
/// "5. H→ γγ channel" come from other fonts and say nothing about the style
/// of the heading. A block without any such glyph counts all its letters and
/// numerals instead.
pub(super) fn text_frac(lines: &[Line], fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool) -> f32 {
    let count = |ascii_only: bool| {
        let (mut matching, mut total) = (0usize, 0usize);
        for glyph in lines.iter().flat_map(|line| line.glyphs.iter()) {
            let font = &fonts[glyph.font as usize];
            let counts = glyph.text.chars().any(|ch| {
                if ascii_only {
                    ch.is_ascii_alphanumeric()
                } else {
                    ch.is_alphanumeric()
                }
            });
            if font.math || !counts {
                continue;
            }
            total += 1;
            matching += usize::from(pred(font));
        }
        (matching, total)
    };
    let (mut matching, mut total) = count(true);
    if total == 0 {
        (matching, total) = count(false);
    }
    if total == 0 {
        0.0
    } else {
        matching as f32 / total as f32
    }
}

/// A block starting with a list label ("•", "1.", "(a)"): a list item; a
/// bullet when the label is a single symbol. Labels in brackets (`[3]`) are
/// citations, not list labels.
fn list_item_rule(trimmed: &str) -> Option<Class> {
    let label = LIST_LABEL.find(trimmed)?.as_str().trim();
    let bullet = label.chars().count() == 1 && !label.chars().next().unwrap().is_alphanumeric();
    (!label.starts_with('[')).then_some(Class::ListItem { bullet })
}

#[cfg(test)]
#[allow(clippy::missing_docs_in_private_items)]
mod tests {
    use super::*;

    #[test]
    fn chapter_numbers_in_figures_or_roman_numerals() {
        assert_eq!(chapter_value("12"), Some(12));
        assert_eq!(chapter_value("IV"), Some(4));
        assert_eq!(chapter_value("XIX"), Some(19));
        assert_eq!(chapter_value("3.2"), None);
        assert_eq!(chapter_value("A"), None);
    }

    fn candidate(text: &str, number: Option<&str>, known: bool) -> HeadingCandidate {
        HeadingCandidate {
            idx: 0,
            level: number.map(|_| 1),
            number: number.map(str::to_string),
            size: 12.0,
            known,
            text: text.to_string(),
        }
    }

    /// The features of a heading candidate `text`, set in italics (or in
    /// bold, or in capitals).
    fn features(text: &str, bold: bool, caps: bool) -> HeadingFeatures<'_> {
        HeadingFeatures {
            text,
            nchars: text.chars().count(),
            words: text.split_whitespace().count(),
            size: 10.0,
            bold,
            strong: bold,
            italic: !bold && !caps,
            caps,
            weak: !bold,
            known: false,
        }
    }

    #[test]
    fn numbered_sentences_are_list_items() {
        let cfg = BodyConfig::default();
        for item in [
            "1. In a print statement, what happens if you leave out one of the parentheses, or both?",
            "2. Turn left 60 degrees.",
            "3. Add methods to",
            "2. What is the order of growth of bubble sort, and why is it \u{201C}the wrong way to go?\u{201D}",
        ] {
            assert!(is_list_sentence(&features(item, false, false), &cfg), "{item}");
        }
        for heading in [
            ("1.1 What is a program?", true, false),
            ("4.2 COMPARISON TO STATE OF THE ART", false, true),
            ("3.2 Attention", false, false),
        ] {
            assert!(
                !is_list_sentence(&features(heading.0, heading.1, heading.2), &cfg),
                "{}",
                heading.0
            );
        }
    }

    #[test]
    fn repeated_unnumbered_labels_are_not_headings() {
        let cfg = BodyConfig::default();
        let mut candidates: Vec<HeadingCandidate> = (0..cfg.repeated_label_min_count)
            .map(|_| candidate("GPT-4 (early) response", None, false))
            .collect();
        candidates.push(candidate("Results", None, false));
        candidates.push(candidate("1 Results", Some("1"), false));
        let kept = drop_repeated_labels(candidates, &cfg);
        let texts: Vec<&str> = kept.iter().map(|kept_candidate| kept_candidate.text.as_str()).collect();
        assert_eq!(texts, ["Results", "1 Results"]);
    }

    #[test]
    fn repeated_known_and_numbered_headings_stay() {
        let cfg = BodyConfig::default();
        let mut candidates = Vec::new();
        for _ in 0..cfg.repeated_label_min_count {
            candidates.push(candidate("Abstract", None, true));
            candidates.push(candidate("A.1 Prompt", Some("A.1"), false));
        }
        assert_eq!(
            drop_repeated_labels(candidates, &cfg).len(),
            2 * cfg.repeated_label_min_count
        );
    }

    #[test]
    fn italic_section_headings_of_several_parts() {
        let cfg = BodyConfig::default();
        let number = |text: &str| {
            let italic = features(text, false, false);
            let sentence = is_list_sentence(&italic, &cfg);
            heading_number(&italic, false, &cfg)
                .filter(|_| !sentence)
                .map(|found| found.number)
        };
        // A&A sets its second level in italics, in sentence case.
        assert_eq!(number("2.1. 2-point weak lensing model").as_deref(), Some("2.1"));
        assert_eq!(
            number("4.1. Simulation-based vs. analytical model: a comparison").as_deref(),
            Some("4.1")
        );
        assert_eq!(number("Appendix A.1: Emulation strategy").as_deref(), Some("A.1"));
        // Items of a list stay items.
        assert_eq!(number("3. Add methods to"), None);
        assert_eq!(number("2 layers of attention"), None);
    }

    #[test]
    fn children_of_numbered_headings() {
        let cfg = BodyConfig::default();
        let numbers: HashSet<String> = ["4.1".to_string()].into();
        let child = |text: &str| child_number(&features(text, false, false), 1, &numbers, &cfg).map(|n| n.level);
        assert_eq!(child("4.1.1. Diagonal elements"), Some(3));
        assert_eq!(child("4.2.1. Diagonal elements"), None);
        assert_eq!(child("4.1.1. the value is set to zero."), None);
    }

    #[test]
    fn few_repeats_are_kept() {
        let cfg = BodyConfig::default();
        let candidates = vec![candidate("Setup", None, false), candidate("Setup", None, false)];
        assert_eq!(drop_repeated_labels(candidates, &cfg).len(), 2);
    }
}
