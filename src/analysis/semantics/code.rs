//! Code listings: lines set in a monospaced font, kept line by line with
//! their indentation instead of being reflowed as a paragraph.
//!
//! A text block whose lines are all monospaced is a listing; a block that
//! mixes prose and listing lines is split where the font changes. Listings
//! that the block stage cut into pieces (each line of an indented program
//! can start a block of its own) are joined again.

use super::{Class, Node, NodeKind, SemanticsConfig};
use crate::backend::FontInfo;
use crate::common::geom::{Rect, median};
use crate::model::style::MONO;
use crate::model::{Line, Span};

/// Tuning parameters of code listing detection. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CodeConfig {
    /// A line with at least this share of monospaced glyphs is a line of
    /// code.
    pub line_mono_min_ratio: f32,
    /// Fewest consecutive code lines that are split off a block of prose
    /// (one monospaced line in a paragraph is a long identifier or URL).
    pub split_min_lines_count: usize,
    /// Two listings of the same column whose baselines are at most this
    /// many font sizes apart are one listing (a blank line between two
    /// lines of a program is about two).
    pub merge_max_gap_em: f32,
    /// Monospaced lines of which at least this share end a sentence or run
    /// on into the next line like one are running text, not code.
    pub prose_min_ratio: f32,
    /// Fewest characters (spaces aside) of a listing.
    pub min_chars_count: usize,
}

impl Default for CodeConfig {
    fn default() -> Self {
        Self {
            line_mono_min_ratio: 0.8,
            split_min_lines_count: 2,
            merge_max_gap_em: 2.6,
            prose_min_ratio: 0.5,
            min_chars_count: 10,
        }
    }
}

/// Marks the code listings among the unclassified text nodes (class
/// [`Class::Code`]): splits the monospaced lines off blocks of prose, joins
/// the pieces of one listing, then gives back to the prose the pieces that
/// do not read as code (see [`is_listing`]).
pub(crate) fn mark_code(nodes: &mut Vec<Node>, fonts: &[FontInfo], cfg: &SemanticsConfig) {
    let cfg = &cfg.code;
    // One pass that builds the new list: removing and inserting in the
    // middle of a long book's nodes would move all those after, each time.
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for node in nodes.drain(..) {
        let pieces = match &node.kind {
            NodeKind::Text(block) if node.class == Class::Unknown => code_runs(&block.lines, fonts, cfg),
            _ => Vec::new(),
        };
        if pieces.is_empty() {
            out.push(node);
        } else {
            split_node(node, &pieces, &mut out);
        }
    }
    *nodes = out;
    join_listings(nodes, cfg);
    for node in nodes.iter_mut() {
        if node.class == Class::Code
            && let NodeKind::Text(block) = &node.kind
            && !is_listing(&block.lines, cfg)
        {
            node.class = Class::Unknown;
        }
    }
}

/// Whether `line` is set in a monospaced font. A line holding a web
/// address is not code: URLs are often set in a typewriter font, in
/// running text and in references.
fn is_code_line(line: &Line, fonts: &[FontInfo], cfg: &CodeConfig) -> bool {
    if line.glyphs.is_empty() {
        return false;
    }
    let text = line.text();
    if text.contains("://") || text.trim_start().starts_with("www.") {
        return false;
    }
    line.fixed_pitch || line.font_frac(fonts, |font| font.mono) >= cfg.line_mono_min_ratio
}

/// How the lines of a block divide into prose and code: consecutive ranges
/// of line indexes, each with whether it is code. Empty when the block has
/// no code, or is all prose after the short code runs are folded into it.
fn code_runs(lines: &[Line], fonts: &[FontInfo], cfg: &CodeConfig) -> Vec<(std::ops::Range<usize>, bool)> {
    let flags: Vec<bool> = lines.iter().map(|line| is_code_line(line, fonts, cfg)).collect();
    if flags.iter().all(|&code| code) {
        return vec![(0..lines.len(), true)];
    }
    let mut runs: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
    for (i, &code) in flags.iter().enumerate() {
        // A short run of code lines inside prose stays prose.
        match runs.last_mut() {
            Some((range, last)) if *last == code => range.end = i + 1,
            _ => runs.push((i..i + 1, code)),
        }
    }
    for run in &mut runs {
        if run.1 && run.0.len() < cfg.split_min_lines_count {
            run.1 = false;
        }
    }
    // Merge the neighbouring prose runs again.
    let mut merged: Vec<(std::ops::Range<usize>, bool)> = Vec::new();
    for run in runs {
        match merged.last_mut() {
            Some((range, last)) if *last == run.1 => range.end = run.0.end,
            _ => merged.push(run),
        }
    }
    if merged.iter().any(|run| run.1) {
        merged
    } else {
        Vec::new()
    }
}

/// Whether monospaced lines are running text (a chat transcript in a
/// novel, a typewritten letter) rather than code: most of them end a
/// sentence (".", "?" or "!") or run on into the next line like one (a
/// letter or a comma before a line that starts in lower case). Lines of
/// code end with brackets, colons and semicolons, or start the next
/// statement.
fn reads_as_prose(lines: &[Line], cfg: &CodeConfig) -> bool {
    let texts: Vec<String> = lines.iter().map(|line| line.text().trim().to_string()).collect();
    let prose = texts
        .iter()
        .enumerate()
        .filter(|(i, text)| {
            let last = text.chars().last();
            let sentence = matches!(last, Some('.' | '?' | '!')) && text.chars().any(char::is_alphabetic);
            let run_on = last.is_some_and(|ch| ch.is_alphabetic() || ch == ',')
                && texts
                    .get(i + 1)
                    .and_then(|next| next.chars().next())
                    .is_some_and(char::is_lowercase);
            // A line of a chat or a quoted mail: "> " and words.
            let quoted = text.strip_prefix("> ").is_some_and(|rest| {
                rest.split_whitespace().all(|word| {
                    word.trim_end_matches([',', '.', '?', '!', '\''])
                        .chars()
                        .all(char::is_alphabetic)
                })
            });
            (sentence || run_on || quoted) && !text.starts_with(">>>")
        })
        .count();
    prose as f32 >= cfg.prose_min_ratio * texts.len() as f32
}

/// Whether lines hold enough to be a listing: some letters, and at least
/// [`CodeConfig::min_chars_count`] characters (a lone axis label or key
/// name set in a typewriter font is not a program).
fn has_substance(lines: &[Line], cfg: &CodeConfig) -> bool {
    let chars: Vec<char> = lines
        .iter()
        .flat_map(|line| line.glyphs.iter())
        .flat_map(|glyph| glyph.text.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect();
    chars.len() >= cfg.min_chars_count && chars.iter().any(|ch| ch.is_alphabetic())
}

/// Whether lines that are all monospaced make a listing.
fn is_listing(lines: &[Line], cfg: &CodeConfig) -> bool {
    has_substance(lines, cfg) && !reads_as_prose(lines, cfg)
}

/// Replaces node `idx` by one node per piece, in order; code pieces get
/// class [`Class::Code`].
fn split_node(node: Node, pieces: &[(std::ops::Range<usize>, bool)], out: &mut Vec<Node>) {
    let NodeKind::Text(block) = &node.kind else {
        unreachable!("only text nodes are split")
    };
    for (offset, (range, code)) in pieces.iter().enumerate() {
        let mut part = block.clone();
        part.lines = block.lines[range.clone()].to_vec();
        part.bbox = lines_bbox(&part.lines);
        out.push(Node {
            kind: NodeKind::Text(part),
            loc: node.loc,
            class: if *code { Class::Code } else { Class::Unknown },
            tail: node.tail,
            leaf_left: node.leaf_left,
            leaf_right: node.leaf_right,
            strip_prefix: if offset == 0 { node.strip_prefix } else { 0 },
        });
    }
}

/// Joins consecutive listings of the same page and column whose lines are
/// close enough to be one program.
fn join_listings(nodes: &mut Vec<Node>, cfg: &CodeConfig) {
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    for node in nodes.drain(..) {
        if let Some(prev) = out.last_mut()
            && continues_listing(prev, &node, cfg)
            && let NodeKind::Text(more) = node.kind
        {
            if let NodeKind::Text(block) = &mut prev.kind {
                block.lines.extend(more.lines);
                block.bbox = lines_bbox(&block.lines);
            }
            prev.tail = node.tail;
            continue;
        }
        out.push(node);
    }
    *nodes = out;
}

/// Whether the listing `next` goes on from the listing `prev`: the next
/// block of the same page and column, close below it.
fn continues_listing(prev: &Node, next: &Node, cfg: &CodeConfig) -> bool {
    if let (Class::Code, Class::Code) = (&prev.class, &next.class)
        && prev.loc.page == next.loc.page
        && prev.tail == (next.loc.page, next.loc.leaf)
        && let (NodeKind::Text(first), NodeKind::Text(second)) = (&prev.kind, &next.kind)
        && let (Some(last), Some(following)) = (first.lines.last(), second.lines.first())
    {
        following.baseline > last.baseline
            && following.baseline - last.baseline <= cfg.merge_max_gap_em * last.size.max(following.size)
    } else {
        false
    }
}

/// Union of the line boxes.
fn lines_bbox(lines: &[Line]) -> Rect {
    lines.iter().fold(Rect::EMPTY, |acc, line| acc.union(&line.bbox))
}

/// The rows of a listing, one per line, with the glyphs placed on the
/// character grid of the monospaced font so that indentation and aligned
/// columns survive: the grid starts at the leftmost glyph and its step is
/// the median glyph advance.
pub(crate) fn code_rows(lines: &[Line], fonts: &[FontInfo]) -> Vec<Vec<Span>> {
    let advances: Vec<f32> = lines
        .iter()
        .flat_map(|line| line.glyphs.iter())
        .filter(|glyph| glyph.text.chars().count() == 1)
        .map(|glyph| glyph.x1 - glyph.x0)
        .filter(|advance| *advance > 0.0)
        .collect();
    let left = lines
        .iter()
        .flat_map(|line| line.glyphs.first())
        .map(|glyph| glyph.x0)
        .fold(f32::INFINITY, f32::min);
    let size = lines.first().map_or(1.0, |line| line.size);
    let step = median(&advances, size * 0.5);
    lines
        .iter()
        .map(|line| {
            let mut spans: Vec<Span> = Vec::new();
            let mut column = 0usize;
            for glyph in &line.glyphs {
                // Only the start of a word is placed on the grid: a letter in
                // a proportional font would otherwise be pushed apart from
                // its word.
                let mut text = String::new();
                let starts_word = column == 0 || glyph.space_before;
                if starts_word {
                    let target = ((glyph.x0 - left) / step).round().max(0.0) as usize;
                    let spaces = if column == 0 {
                        target
                    } else {
                        target.saturating_sub(column).max(1)
                    };
                    text.extend(std::iter::repeat_n(' ', spaces));
                    column += spaces;
                }
                text.push_str(&glyph.text);
                column += glyph.text.chars().count();
                let style =
                    (line.style_of(glyph, fonts) | MONO) & !(crate::model::style::SUP | crate::model::style::SUB);
                match spans.last_mut() {
                    Some(last) if last.style == style => last.text.push_str(&text),
                    _ => spans.push(Span {
                        text,
                        style,
                        href: None,
                        color: [0, 0, 0],
                        math: None,
                        note: None,
                    }),
                }
            }
            spans
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineGlyph;

    /// A line of a monospaced font (index 0), six points per character;
    /// leading spaces indent it.
    fn mono_line(text: &str, baseline: f32) -> Line {
        let mut glyphs = Vec::new();
        let mut after_space = false;
        for (i, ch) in text.chars().enumerate() {
            let x = i as f32 * 6.0;
            if ch == ' ' {
                after_space = !glyphs.is_empty();
                continue;
            }
            glyphs.push(LineGlyph {
                text: ch.to_string(),
                x0: x,
                x1: x + 6.0,
                y: baseline,
                size: 10.0,
                font: 0,
                script: 0,
                space_before: after_space,
                drawn_space: false,
                link: None,
                ink: Rect::new(x, baseline - 7.0, x + 6.0, baseline),
                ox: x,
                oy: baseline,
                extra: Vec::new(),
                color: [0, 0, 0],
            });
            after_space = false;
        }
        let (x0, x1) = (glyphs[0].x0, glyphs.last().unwrap().x1);
        Line {
            page: 0,
            bbox: Rect::new(x0, baseline - 7.0, x1, baseline + 2.0),
            ink: Rect::new(x0, baseline - 7.0, x1, baseline),
            baseline,
            size: 10.0,
            font: 0,
            glyphs,
            fixed_pitch: false,
        }
    }

    fn lines(texts: &[&str]) -> Vec<Line> {
        texts
            .iter()
            .enumerate()
            .map(|(i, text)| mono_line(text, 12.0 * (i + 1) as f32))
            .collect()
    }

    fn mono_fonts() -> Vec<FontInfo> {
        vec![FontInfo {
            mono: true,
            ..Default::default()
        }]
    }

    #[test]
    fn listing_rows_keep_their_indentation() {
        let code = lines(&["(define (square x)", "  (* x x))"]);
        let rows: Vec<String> = code_rows(&code, &mono_fonts())
            .iter()
            .map(|row| row.iter().map(|span| span.text.as_str()).collect())
            .collect();
        assert_eq!(rows, ["(define (square x)", "  (* x x))"]);
    }

    #[test]
    fn chat_and_sentences_in_a_typewriter_font_are_prose() {
        let cfg = CodeConfig::default();
        assert!(reads_as_prose(
            &lines(&["> I know you can do it", "> So you're in?"]),
            &cfg
        ));
        assert!(reads_as_prose(
            &lines(&["The game is afoot, something big", "is going down."]),
            &cfg
        ));
        assert!(!reads_as_prose(&lines(&["(define (square x)", "  (* x x))"]), &cfg));
        assert!(!reads_as_prose(
            &lines(&[">>> minutes = 105", ">>> minutes / 60", "1.75"]),
            &cfg
        ));
    }

    #[test]
    fn lone_labels_are_not_listings() {
        let cfg = CodeConfig::default();
        assert!(!is_listing(&lines(&["-50"]), &cfg));
        assert!(!is_listing(&lines(&["0.03", "0.025"]), &cfg));
        assert!(is_listing(&lines(&["for i in range(10):", "    print(i)"]), &cfg));
    }
}
