//! The order in which the text formats (LaTeX, Typst) write the blocks of a
//! document, and the front matter they write alike: runs of code
//! paragraphs become one listing, the title blocks one title, the author
//! blocks cards that stand side by side; the heading of the abstract goes
//! with the abstract, an equation takes back its number when it was read as
//! a paragraph of its own, and the link to the arXiv page follows the front
//! matter.

use super::{code_run, is_abstract_text, plain, stray_equation_number};
use crate::model::{Block, BlockKind, Span};

/// Longest line (in characters) of an author group set as a card beside
/// the others; a group with a longer line is a paragraph of its own.
const AUTHOR_CARD_MAX_CHARS: usize = 60;

/// One thing to write.
pub(crate) enum Step<'a> {
    /// The link to the arXiv page of the paper, after the front matter.
    Source,
    /// A run of code paragraphs, written as one listing.
    Code(&'a [&'a Block]),
    /// A run of title blocks, written as one title.
    Title(&'a [&'a Block]),
    /// A run of author groups.
    Authors(&'a [&'a Block]),
    /// The heading of the abstract, followed by the abstract.
    AbstractHeading(&'a Block),
    /// A displayed formula and its number, read as a paragraph after it.
    NumberedEquation(&'a Block, String),
    /// A run of entries of a table of contents.
    Contents(&'a [&'a Block]),
    /// Any other block.
    Block(&'a Block),
}

/// The steps that write `blocks`. `source` tells whether there is a link to
/// the source to write after the front matter.
pub(crate) fn steps<'a>(blocks: &'a [&'a Block], source: bool) -> Vec<Step<'a>> {
    let run = |start: usize, same: fn(&BlockKind) -> bool| {
        start + blocks[start..].iter().take_while(|block| same(&block.kind)).count()
    };
    let mut out = Vec::new();
    let mut idx = 0;
    let mut after_front = false;
    while idx < blocks.len() {
        let block = blocks[idx];
        if !after_front && !matches!(block.kind, BlockKind::Title | BlockKind::Authors | BlockKind::Note) {
            if source {
                out.push(Step::Source);
            }
            after_front = true;
        }
        let (step, end) = if let Some(end) = code_run(blocks, idx) {
            (Step::Code(&blocks[idx..end]), end)
        } else {
            match &block.kind {
                BlockKind::Title => {
                    let end = run(idx, |kind| matches!(kind, BlockKind::Title));
                    (Step::Title(&blocks[idx..end]), end)
                }
                BlockKind::Authors => {
                    let end = run(idx, |kind| matches!(kind, BlockKind::Authors));
                    (Step::Authors(&blocks[idx..end]), end)
                }
                BlockKind::ContentsEntry { .. } => {
                    let end = run(idx, |kind| matches!(kind, BlockKind::ContentsEntry { .. }));
                    (Step::Contents(&blocks[idx..end]), end)
                }
                BlockKind::Heading { .. }
                    if is_abstract_text(&block.spans)
                        && blocks
                            .get(idx + 1)
                            .is_some_and(|next| matches!(next.kind, BlockKind::Paragraph { in_abstract: true })) =>
                {
                    (Step::AbstractHeading(block), idx + 1)
                }
                BlockKind::Equation { number: None, .. }
                    if let Some(number) = blocks.get(idx + 1).and_then(|next| stray_equation_number(next)) =>
                {
                    (Step::NumberedEquation(block, number), idx + 2)
                }
                _ => (Step::Block(block), idx + 1),
            }
        };
        out.push(step);
        idx = end;
    }
    if !after_front && source {
        out.push(Step::Source);
    }
    out
}

/// The spans of a run of title blocks as one line.
pub(crate) fn title_spans(run: &[&Block]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for block in run {
        if !spans.is_empty() {
            spans.push(Span::plain(" "));
        }
        spans.extend(block.spans.iter().cloned());
    }
    spans
}

/// The rows of an author group: its lines, or its text as one line.
pub(crate) fn author_rows(block: &Block) -> &[Vec<Span>] {
    if block.rows.is_empty() {
        std::slice::from_ref(&block.spans)
    } else {
        &block.rows[..]
    }
}

/// The lines of an author group, written by `text`, each then given to
/// `line` with its index (the name is the first); empty lines are left
/// out.
pub(crate) fn author_lines(
    block: &Block,
    text: impl Fn(&[Span]) -> String,
    line: impl Fn(usize, String) -> String,
) -> Vec<String> {
    author_rows(block)
        .iter()
        .enumerate()
        .map(|(i, row)| line(i, text(row)))
        .filter(|text| !text.is_empty())
        .collect()
}

/// How a text format writes the author groups.
pub(crate) trait AuthorMarkup {
    /// Writes the label of a block that some text links to.
    fn anchor(&mut self, block: &Block);
    /// The lines of an author group (see [`author_lines`]).
    fn lines(&self, block: &Block) -> Vec<String>;
    /// An author group as a card.
    fn card(&self, lines: &[String]) -> String;
    /// Writes cards side by side.
    fn cards(&mut self, cards: &[String]);
    /// Writes an author group too long for a card as a paragraph.
    fn byline(&mut self, lines: &[String]);
}

/// Writes a run of author groups: each group whose lines are all short is
/// a card, and the cards stand side by side; a group with a longer line (a
/// list of many authors) is a paragraph of its own.
pub(crate) fn write_authors(markup: &mut impl AuthorMarkup, run: &[&Block]) {
    let mut cards: Vec<String> = Vec::new();
    for block in run {
        markup.anchor(block);
        let lines = markup.lines(block);
        if lines.is_empty() {
            continue;
        }
        if author_rows(block)
            .iter()
            .all(|row| plain(row).chars().count() <= AUTHOR_CARD_MAX_CHARS)
        {
            cards.push(markup.card(&lines));
        } else {
            if !cards.is_empty() {
                markup.cards(&cards);
                cards.clear();
            }
            markup.byline(&lines);
        }
    }
    if !cards.is_empty() {
        markup.cards(&cards);
    }
}
