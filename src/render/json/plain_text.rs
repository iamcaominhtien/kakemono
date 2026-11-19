//! The `plain` field of the JSON output: the text of a block without
//! markup, for programs that want words rather than styles (search indexes,
//! embeddings, language models). Inline formulas are written as LaTeX
//! between `$` signs and displayed ones between `$$`, as in Markdown, so
//! that the text of a block reads as one string. The field is read-only:
//! [`super::apply`] reads `text` back, never `plain`.

use crate::model::{Block, BlockKind, Span};
use crate::render::inline_math::{Piece, has_math, split};
use crate::render::latex::math::latex;

/// The plain text of spans: their text with runs of white space made one
/// space, and formulas as `$LaTeX$`.
pub(crate) fn plain_of(spans: &[Span]) -> String {
    let mut out = String::new();
    if has_math(spans) {
        for piece in split(spans) {
            match piece {
                Piece::Text(spans) => spans.iter().for_each(|span| out.push_str(&span.text)),
                Piece::Math(node, _, _) => {
                    // The spaces around the formula are in the text pieces.
                    out.push('$');
                    out.push_str(latex(&node).trim());
                    out.push('$');
                }
            }
        }
    } else {
        spans.iter().for_each(|span| out.push_str(&span.text));
    }
    squeeze(&out)
}

/// The plain text of a whole block: its rows one per line for an author
/// group or a code listing, `$$LaTeX$$` (or the text of the PDF when the
/// structure is not known) for a displayed formula, the caption for a
/// figure, the text otherwise. `None` for a block without text.
pub(crate) fn block_plain(block: &Block) -> Option<String> {
    let text = match &block.kind {
        BlockKind::Authors if !block.rows.is_empty() => block
            .rows
            .iter()
            .map(|row| plain_of(row))
            .collect::<Vec<_>>()
            .join("\n"),
        BlockKind::Code => block
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>()
                    .replace(super::super::assets::MARK, "")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        BlockKind::Equation { .. } => match &block.math {
            Some(node) => format!("$${}$$", latex(node).trim()),
            None => squeeze(&block.text()),
        },
        _ => plain_of(&block.spans),
    };
    (!text.is_empty()).then_some(text)
}

/// `text` with every run of white space made one space, without the
/// placeholder mark of the image store, trimmed.
fn squeeze(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split(|ch: char| ch.is_whitespace() || ch == super::super::assets::MARK) {
        if word.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_space_is_one_space() {
        let spans = [Span::plain("  Deep\u{2003}residual \n learning ")];
        assert_eq!(plain_of(&spans), "Deep residual learning");
    }

    #[test]
    fn styles_and_links_are_dropped() {
        let mut bold = Span::plain("Adam");
        bold.style = crate::model::BOLD;
        let mut link = Span::plain("[12]");
        link.href = Some("#ref-12".into());
        let spans = [bold, Span::plain(" works well "), link];
        assert_eq!(plain_of(&spans), "Adam works well [12]");
    }
}
