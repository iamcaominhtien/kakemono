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
use std::collections::HashMap;

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
}

impl Default for BodyConfig {
    fn default() -> Self {
        Self {
            heading_max_lines_count: 3,
            heading_min_chars_count: 2,
            heading_max_chars_count: 160,
            bold_min_ratio: 0.9,
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
    super::contents::mark_contents(nodes, cfg);
    super::code::mark_code(nodes, fonts, cfg);
    split_run_in_headings(nodes, fonts, cfg);
    let candidates = find_heading_candidates(nodes, stats, fonts, cfg);
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
fn find_heading_candidates(
    nodes: &[Node],
    stats: &DocStats,
    fonts: &[FontInfo],
    cfg: &SemanticsConfig,
) -> Vec<HeadingCandidate> {
    let mut candidates: Vec<HeadingCandidate> = Vec::new();
    let mut seen_refs = false;
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
        // A numbered sentence not set in bold ("4. Use the modified version
        // of do_twice to call print_twice twice.") is an item of a list, not
        // a section.
        let sentence = !features.bold
            && features.text.ends_with('.')
            && features.words > cfg.body.sentence_max_heading_words_count;
        let number = heading_number(&features, seen_refs, &cfg.body).filter(|_| !sentence);
        let is_heading = number.is_some()
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
    let bold = text_frac(&block.lines, fonts, |font| font.bold) >= body.bold_min_ratio;
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
    // capital letter (or is all caps).
    if rest.chars().next().is_some_and(|ch| ch.is_uppercase()) && (features.strong || features.weak) {
        Some(dotted_number(&captures["num"]))
    } else {
        None
    }
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
/// (ICLR sets them in small capitals at the body size). A bare letter
/// ("A Proofs") is only accepted after the bibliography heading.
fn appendix_number_rule(features: &HeadingFeatures<'_>, seen_refs: bool) -> Option<HeadingNumber> {
    let captures = HEADING_APP.captures(features.text).filter(|captures| {
        (seen_refs || captures.name("word").is_some() || captures["num"].contains('.'))
            && (features.strong || features.caps)
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
fn text_frac(lines: &[Line], fonts: &[FontInfo], pred: impl Fn(&FontInfo) -> bool) -> f32 {
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
    fn few_repeats_are_kept() {
        let cfg = BodyConfig::default();
        let candidates = vec![candidate("Setup", None, false), candidate("Setup", None, false)];
        assert_eq!(drop_repeated_labels(candidates, &cfg).len(), 2);
    }
}
