//! Added notes ([`Span::note`], the `<note>` tag of the editable
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
use crate::model::{Block, BlockKind, SUP, Span};
use std::collections::HashMap;

/// Prefix of the anchor of a note.
const NOTE_ANCHOR: &str = "an-";
/// Prefix of the anchor of the call of a note, which the note links back
/// to.
const CALL_ANCHOR: &str = "anref-";

/// Style of the notes in the flowing HTML modes and in EPUB, added to the
/// page only when it has notes: a small aside with a rule on its left.
pub(crate) const NOTES_CSS: &str = "aside.an{font-size:.85em;line-height:1.45;margin:.3em 0 1em;padding-left:.75em;\
border-left:2px solid rgba(128,128,128,.45)}\naside.an p{margin:.15em 0;text-indent:0}\n\
sup.an-ref{line-height:0}\na.an-call,a.an-back{text-decoration:none;font-weight:600}\n";

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

/// Whether the text of `block` holds an added note.
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
/// (`#an-` + `scope` + number) as its link. `scope` keeps the anchors of
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

/// A copy of `analysis` whose footnotes are added notes of their calls
/// ([`Span::note`]), for the outputs that set a note at the foot of the
/// page of its call (LaTeX, Typst, Word), or `None` when no footnote can
/// be attached.
///
/// A footnote block (of a Word document, the note of an EPUB book, the
/// note at the foot of a PDF page) is attached to its call when it has a
/// mark and exactly one call: a raised link to it in the text of a
/// paragraph or a list item, which the note follows (after only other
/// notes, or on the same page or the next). The call becomes the note and
/// the block goes. Other footnotes stay blocks where they are: an endnote,
/// a note called from a heading, a caption or a table, or from more than
/// one place.
pub(crate) fn footnotes_attached(analysis: &Analysis) -> Option<Analysis> {
    let blocks = &analysis.doc.blocks;
    let paged = !analysis.doc.pages.is_empty();
    let notes: HashMap<&str, usize> = blocks
        .iter()
        .enumerate()
        .filter_map(|(idx, block)| match (&block.kind, block.anchor.as_deref()) {
            (BlockKind::Footnote { marker }, Some(anchor)) if !marker.is_empty() && !has_notes(block) => {
                Some((anchor, idx))
            }
            _ => None,
        })
        .collect();
    if notes.is_empty() {
        return None;
    }
    // Every link to a note, wherever it is: (block, span) of each.
    let mut links: HashMap<&str, Vec<(usize, Option<usize>)>> = HashMap::new();
    for (idx, block) in blocks.iter().enumerate() {
        let text = std::ptr::from_ref(block.spans.as_slice());
        for spans in span_lists(block) {
            let in_text = std::ptr::eq(spans, text);
            for (span_idx, span) in spans.iter().enumerate() {
                if let Some(anchor) = span.href.as_deref().and_then(|href| href.strip_prefix('#'))
                    && notes.contains_key(anchor)
                {
                    links
                        .entry(anchor)
                        .or_default()
                        .push((idx, in_text.then_some(span_idx)));
                }
            }
        }
    }
    let follows = |call: usize, note: usize| {
        note > call
            && (blocks[call + 1..note]
                .iter()
                .all(|block| matches!(block.kind, BlockKind::Footnote { .. }))
                || paged && blocks[note].page <= blocks[call].page + 1)
    };
    let attached: Vec<(usize, usize, usize)> = links
        .iter()
        .filter_map(|(anchor, links)| {
            let [(call, Some(span))] = links.as_slice() else {
                return None;
            };
            let note = notes[anchor];
            let callable = matches!(
                blocks[*call].kind,
                BlockKind::Paragraph { .. } | BlockKind::ListItem { .. } | BlockKind::ListContinuation
            );
            (callable && blocks[*call].spans[*span].style & SUP != 0 && follows(*call, note))
                .then_some((*call, *span, note))
        })
        .collect();
    if attached.is_empty() {
        return None;
    }
    let mut doc = analysis.doc.clone();
    let mut gone = vec![false; doc.blocks.len()];
    // From the last call of a block, so that the spans added after a call
    // do not move the calls before it.
    let mut attached = attached;
    attached.sort_unstable_by_key(|&(call, span, _)| std::cmp::Reverse((call, span)));
    for (call, span, note) in attached {
        let body = std::mem::take(&mut doc.blocks[note].spans);
        attach(&mut doc.blocks[call].spans, span, body);
        gone[note] = true;
    }
    let mut idx = 0;
    doc.blocks.retain(|_| {
        idx += 1;
        !gone[idx - 1]
    });
    Some(Analysis {
        doc,
        loose: analysis.loose.clone(),
        rotated: analysis.rotated.clone(),
    })
}

/// Makes the call `spans[call]` of a footnote the note `body`: the mark
/// goes, as do the spaces before it (the note sticks to the word it
/// follows), and a space after it stays.
fn attach(spans: &mut Vec<Span>, call: usize, body: Vec<Span>) {
    let spaced = spans[call].text.ends_with(char::is_whitespace);
    let span = &mut spans[call];
    span.text.clear();
    span.href = None;
    span.style &= !SUP;
    span.note = Some(body);
    if spaced {
        match spans
            .get_mut(call + 1)
            .filter(|next| next.note.is_none() && next.math.is_none())
        {
            Some(next) if !next.text.starts_with(char::is_whitespace) => next.text.insert(0, ' '),
            Some(_) => {}
            None => spans.insert(call + 1, Span::plain(" ")),
        }
    }
    if let Some(before) = call.checked_sub(1).map(|idx| &mut spans[idx])
        && before.note.is_none()
    {
        before.text.truncate(before.text.trim_end().len());
    }
}

/// The HTML of the call of a numbered note: its number, raised, linking to
/// the note. Empty for a note that was not numbered.
pub(crate) fn call_html(span: &Span) -> String {
    let Some(target) = note_anchor(span) else {
        return String::new();
    };
    format!(
        "<sup class=\"an-ref\"><a class=\"an-call\" id=\"{}\" href=\"#{}\" role=\"doc-noteref\">{}</a></sup>",
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
                "<p id=\"{}\"><a class=\"an-back\" href=\"#{}\" role=\"doc-backlink\">{}</a> {}</p>",
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
    format!("<aside class=\"an\" role=\"note\">{}</aside>\n", items.concat())
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
                "<aside class=\"an\" epub:type=\"footnote\" role=\"doc-footnote\" id=\"{}\"><p>\
<a class=\"an-back\" href=\"#{}\" role=\"doc-backlink\">{}</a> {}</p></aside>\n",
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
        assert_eq!(blocks[2].spans[0].href.as_deref(), Some("#an-c2-3"));
        let html = call_html(&blocks[0].spans[1]);
        assert_eq!(
            html,
            "<sup class=\"an-ref\"><a class=\"an-call\" id=\"anref-c2-1\" href=\"#an-c2-1\" role=\"doc-noteref\">1</a></sup>"
        );
        let aside = aside_html(&[&blocks[0]], false);
        assert!(aside.starts_with("<aside class=\"an\""), "{aside}");
        assert!(
            aside.contains("<p id=\"an-c2-2\"><a class=\"an-back\" href=\"#anref-c2-2\""),
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

    fn call(marker: &str) -> Span {
        Span {
            href: Some(format!("#fn-{}", marker.trim())),
            ..Span::styled(marker, SUP)
        }
    }

    fn footnote(marker: &str, text: &str) -> Block {
        let mut block = Block::new(0, BlockKind::Footnote { marker: marker.into() });
        block.spans = vec![Span::plain(text)];
        block.anchor = Some(format!("fn-{marker}"));
        block
    }

    fn analysis(blocks: Vec<Block>) -> Analysis {
        Analysis {
            doc: crate::model::Document {
                meta: Default::default(),
                pages: Vec::new(),
                body_size: 10.0,
                leading: 12.0,
                body_font: String::new(),
                math_font: String::new(),
                blocks,
                pictures: Vec::new(),
            },
            loose: Vec::new(),
            rotated: Vec::new(),
        }
    }

    #[test]
    fn footnotes_attach_to_their_single_call() {
        let mut heading = Block::new(0, BlockKind::Heading { level: 1 });
        heading.spans = vec![Span::plain("Title"), call("3")];
        let blocks = vec![
            paragraph(vec![
                Span::plain("a "),
                call("1"),
                Span::plain(" b"),
                call("2 "),
                Span::plain("c"),
            ]),
            footnote("1", "one"),
            footnote("2", "two"),
            heading,
            footnote("3", "three"),
            paragraph(vec![Span::plain("d"), call("4"), Span::plain(", e"), call("4")]),
            footnote("4", "four"),
            paragraph(vec![Span::plain("f"), call("5")]),
            paragraph(vec![Span::plain("g")]),
            footnote("5", "an endnote"),
        ];
        let attached = footnotes_attached(&analysis(blocks)).unwrap().doc.blocks;
        let texts: Vec<String> = attached[0]
            .spans
            .iter()
            .map(|span| match &span.note {
                Some(note) => format!("[{}]", note[0].text),
                None => span.text.clone(),
            })
            .collect();
        assert_eq!(texts.concat(), "a[one] b[two] c");
        // Called from a heading, twice, or far from its call: still a block.
        let kept: Vec<&str> = attached
            .iter()
            .filter_map(|block| match &block.kind {
                BlockKind::Footnote { marker } => Some(marker.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(kept, ["3", "4", "5"]);
        assert!(footnotes_attached(&analysis(vec![paragraph(vec![Span::plain("x")])])).is_none());
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
