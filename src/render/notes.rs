//! Translator's notes ([`Span::note`], the `<tn>` tag of the editable
//! JSON) in the outputs: their numbering, and the HTML of their calls and
//! of the notes themselves, shared by the flowing modes and EPUB.
//!
//! The outputs that number the notes themselves (HTML, Markdown, EPUB)
//! number a copy of the document before writing it ([`numbered_analysis`],
//! [`number_blocks`]): the call of each note gets its number as its text
//! and the anchor of the note as its link. A document without notes is
//! written as it is, so that its outputs do not change.

use super::escape::{esc, text_html};
use crate::analysis::pipeline::Analysis;
use crate::model::{Block, BlockKind, Span};

/// Prefix of the anchor of a note.
const NOTE_ANCHOR: &str = "tn-";
/// Prefix of the anchor of the call of a note, which the note links back
/// to.
const CALL_ANCHOR: &str = "tnref-";

/// Style of the notes in the flowing HTML modes and in EPUB, added to the
/// page only when it has notes: a small aside with a rule on its left.
pub(crate) const NOTES_CSS: &str = "aside.tn{font-size:.85em;line-height:1.45;margin:.3em 0 1em;padding-left:.75em;\
border-left:2px solid rgba(128,128,128,.45)}\naside.tn p{margin:.15em 0;text-indent:0}\n\
sup.tn-ref{line-height:0}\na.tn-call,a.tn-back{text-decoration:none;font-weight:600}\n";

/// A piece of a run of spans: text spans, or the call of a note.
pub(crate) enum Segment<'a> {
    /// Consecutive spans that are not notes.
    Text(&'a [Span]),
    /// The call of a note (its span).
    Note(&'a Span),
}

/// `spans` cut at the calls of notes, so that the spans of text on either
/// side are never joined with each other or with a call.
pub(crate) fn segments(spans: &[Span]) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (idx, span) in spans.iter().enumerate() {
        if span.note.is_none() {
            continue;
        }
        if start < idx {
            out.push(Segment::Text(&spans[start..idx]));
        }
        out.push(Segment::Note(span));
        start = idx + 1;
    }
    if start < spans.len() {
        out.push(Segment::Text(&spans[start..]));
    }
    out
}

/// Whether the text of `block` holds a translator's note.
pub(crate) fn has_notes(block: &Block) -> bool {
    span_lists(block)
        .iter()
        .any(|spans| spans.iter().any(|span| span.note.is_some()))
}

/// A copy of `analysis` whose notes are numbered from 1 in reading order
/// (see [`number_blocks`]), or `None` when it has no note.
pub(crate) fn numbered_analysis(analysis: &Analysis) -> Option<Analysis> {
    if !analysis.doc.blocks.iter().any(has_notes) {
        return None;
    }
    let mut doc = analysis.doc.clone();
    number_blocks(&mut doc.blocks, "");
    Some(Analysis {
        doc,
        loose: analysis.loose.clone(),
        rotated: analysis.rotated.clone(),
    })
}

/// Numbers the notes of `blocks` from 1, in reading order: the call of
/// each note gets its number as its text, and the anchor of the note
/// (`#tn-` + `scope` + number) as its link. `scope` keeps the anchors of
/// notes numbered apart (the chapters of a book) different.
pub(crate) fn number_blocks(blocks: &mut [Block], scope: &str) {
    let mut number = 0;
    for block in blocks {
        for spans in span_lists_mut(block) {
            for span in spans.iter_mut().filter(|span| span.note.is_some()) {
                number += 1;
                span.text = number.to_string();
                span.href = Some(format!("#{NOTE_ANCHOR}{scope}{number}"));
            }
        }
    }
}

/// The HTML of the call of a numbered note: its number, raised, linking to
/// the note. Empty for a note that was not numbered.
pub(crate) fn call_html(span: &Span) -> String {
    let Some(target) = note_anchor(span) else {
        return String::new();
    };
    format!(
        "<sup class=\"tn-ref\"><a class=\"tn-call\" id=\"{}\" href=\"#{}\" role=\"doc-noteref\">{}</a></sup>",
        esc(&call_anchor(target)),
        esc(target),
        esc(&span.text)
    )
}

/// The numbered notes of `blocks`, listed in an aside (to be written after
/// the blocks), each with its number linking back to its call. Formulas in
/// the notes are MathML when `math`. Empty when the blocks have no note.
pub(crate) fn aside_html(blocks: &[&Block], math: bool) -> String {
    let items: Vec<String> = calls(blocks)
        .into_iter()
        .filter_map(|call| {
            let target = note_anchor(call)?;
            Some(format!(
                "<p id=\"{}\"><a class=\"tn-back\" href=\"#{}\" role=\"doc-backlink\">{}</a> {}</p>",
                esc(target),
                esc(&call_anchor(target)),
                esc(&call.text),
                text_html(call.note.as_deref().unwrap_or_default(), math)
            ))
        })
        .collect();
    if items.is_empty() {
        return String::new();
    }
    format!("<aside class=\"tn\" role=\"note\">{}</aside>\n", items.concat())
}

/// The numbered notes of `blocks` as EPUB 3 footnotes: one `aside` per
/// note, which reading systems show in a popup from its call. Formulas in
/// the notes are MathML when `math`.
pub(crate) fn epub_asides(blocks: &[&Block], math: bool) -> String {
    calls(blocks)
        .into_iter()
        .filter_map(|call| {
            let target = note_anchor(call)?;
            Some(format!(
                "<aside class=\"tn\" epub:type=\"footnote\" role=\"doc-footnote\" id=\"{}\"><p>\
<a class=\"tn-back\" href=\"#{}\" role=\"doc-backlink\">{}</a> {}</p></aside>\n",
                esc(target),
                esc(&call_anchor(target)),
                esc(&call.text),
                text_html(call.note.as_deref().unwrap_or_default(), math)
            ))
        })
        .collect()
}

/// The calls of the notes of `blocks`, in reading order.
pub(crate) fn calls<'a>(blocks: &[&'a Block]) -> Vec<&'a Span> {
    blocks
        .iter()
        .flat_map(|block| span_lists(block))
        .flat_map(|spans| spans.iter().filter(|span| span.note.is_some()))
        .collect()
}

/// The anchor of the note a numbered call links to (without `#`).
fn note_anchor(span: &Span) -> Option<&str> {
    span.href
        .as_deref()
        .and_then(|href| href.strip_prefix('#'))
        .filter(|anchor| anchor.starts_with(NOTE_ANCHOR))
}

/// The anchor of the call of the note whose anchor is `target`.
fn call_anchor(target: &str) -> String {
    format!("{CALL_ANCHOR}{}", &target[NOTE_ANCHOR.len()..])
}

/// The runs of text of a block that the outputs write, in reading order:
/// the rows of an author group (its `spans` are made from them), or its
/// text, then the cells of its table.
fn span_lists(block: &Block) -> Vec<&[Span]> {
    let mut lists: Vec<&[Span]> = if matches!(block.kind, BlockKind::Authors) && !block.rows.is_empty() {
        block.rows.iter().map(Vec::as_slice).collect()
    } else {
        vec![&block.spans]
    };
    if let Some(table) = &block.table {
        lists.extend(table.rows.iter().flatten().map(|cell| cell.spans.as_slice()));
    }
    lists
}

/// [`span_lists`], to change.
fn span_lists_mut(block: &mut Block) -> Vec<&mut Vec<Span>> {
    let mut lists: Vec<&mut Vec<Span>> = if matches!(block.kind, BlockKind::Authors) && !block.rows.is_empty() {
        block.rows.iter_mut().collect()
    } else {
        vec![&mut block.spans]
    };
    if let Some(table) = &mut block.table {
        lists.extend(table.rows.iter_mut().flatten().map(|cell| &mut cell.spans));
    }
    lists
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{BlockKind, ITALIC};

    fn note(text: &str) -> Span {
        Span {
            note: Some(vec![Span::plain(text)]),
            ..Span::plain("")
        }
    }

    fn paragraph(spans: Vec<Span>) -> Block {
        let mut block = Block::new(0, BlockKind::Paragraph { in_abstract: false });
        block.spans = spans;
        block
    }

    #[test]
    fn notes_are_numbered_in_reading_order() {
        let mut blocks = vec![
            paragraph(vec![Span::plain("a"), note("one"), Span::plain("b"), note("two")]),
            paragraph(vec![Span::plain("c")]),
            paragraph(vec![note("three")]),
        ];
        assert!(has_notes(&blocks[0]) && !has_notes(&blocks[1]));
        number_blocks(&mut blocks, "c2-");
        assert_eq!(blocks[0].spans[3].text, "2");
        assert_eq!(blocks[2].spans[0].href.as_deref(), Some("#tn-c2-3"));
        let html = call_html(&blocks[0].spans[1]);
        assert_eq!(
            html,
            "<sup class=\"tn-ref\"><a class=\"tn-call\" id=\"tnref-c2-1\" href=\"#tn-c2-1\" role=\"doc-noteref\">1</a></sup>"
        );
        let aside = aside_html(&[&blocks[0]], false);
        assert!(aside.starts_with("<aside class=\"tn\""), "{aside}");
        assert!(
            aside.contains("<p id=\"tn-c2-2\"><a class=\"tn-back\" href=\"#tnref-c2-2\""),
            "{aside}"
        );
        assert!(aside.contains("</a> two</p>"), "{aside}");
        assert!(aside_html(&[&blocks[1]], false).is_empty());
        assert_eq!(
            epub_asides(&[&blocks[2]], false)
                .matches("epub:type=\"footnote\"")
                .count(),
            1
        );
    }

    #[test]
    fn segments_cut_at_notes() {
        let spans = vec![Span::styled("a", ITALIC), note("x"), Span::plain("b"), Span::plain("c")];
        let segments = segments(&spans);
        assert_eq!(segments.len(), 3);
        assert!(matches!(segments[1], Segment::Note(_)));
        assert!(matches!(segments[2], Segment::Text(text) if text.len() == 2));
    }
}
