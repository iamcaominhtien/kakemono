//! The rules that read a deck as a document beyond what its placeholders
//! say: a title found in a text box, a title slide that its layout does
//! not name, text repeated at the edge of many slides (page furniture),
//! and the reading order of a slide's shapes.

use super::build::{Piece, SlideContent, TextInfo, title_text};
use super::order::Frame;
use super::{FURNITURE_MIN_SLIDES, PowerPointConfig};
use crate::word::ir::{Item, Para};
use std::collections::{HashMap, HashSet};

/// Leaves out of `slides` the text boxes whose text is page furniture: a
/// short text near the top or bottom edge, the same on many of the slides
/// (a classification banner, a copyright line, a running title). `height_pt`
/// is the slides' height.
pub(crate) fn remove_furniture(slides: &mut [SlideContent], height_pt: f32, config: &PowerPointConfig) {
    let edge_pt = height_pt * config.furniture_edge_ratio;
    let is_candidate = |piece: &Piece| {
        let (Some(text), Some(frame)) = (&piece.text, piece.frame) else {
            return false;
        };
        let near_edge = frame.y + frame.height <= edge_pt || frame.y >= height_pt - edge_pt;
        near_edge && !text.plain.is_empty() && text.words() <= config.furniture_max_words
    };
    let mut counts: HashMap<String, usize> = HashMap::new();
    for slide in slides.iter() {
        let texts: HashSet<&str> = slide
            .pieces
            .iter()
            .filter(|piece| is_candidate(piece))
            .filter_map(|piece| piece.text.as_ref().map(|text| text.plain.as_str()))
            .collect();
        for text in texts {
            *counts.entry(text.to_string()).or_default() += 1;
        }
    }
    let needed = ((slides.len() as f32 * config.furniture_slides_ratio).ceil() as usize).max(FURNITURE_MIN_SLIDES);
    let repeated: HashSet<String> = counts
        .into_iter()
        .filter(|(_, count)| *count >= needed)
        .map(|(text, _)| text)
        .collect();
    if repeated.is_empty() {
        return;
    }
    for slide in slides {
        slide.pieces.retain(|piece| {
            !(is_candidate(piece) && piece.text.as_ref().is_some_and(|text| repeated.contains(&text.plain)))
        });
    }
}

/// Gives `slide` a title when it has none, from its text boxes
/// (`PowerPointConfig::title_*`), the first that applies of:
/// - the first paragraph of a text box near the top of the slide
///   (`height_pt` high), clearly larger than the box's other paragraphs
///   and the slide's other text (a title alone, or a title and its
///   subtitle in one box): the rest of the box stays as text;
/// - a text box near the top, short, in one or two paragraphs without
///   bullets, whose text is clearly the largest of the slide (a title in
///   two paragraphs of one size);
/// - the one text box of a slide that has no other (a section divider),
///   short and large, wherever it lies.
pub(crate) fn infer_title(slide: &mut SlideContent, height_pt: f32, config: &PowerPointConfig) {
    if slide.title.is_some() {
        return;
    }
    let top_pt = height_pt * config.title_top_ratio;
    // The largest text of the slide's pieces but `skip`.
    let others = |skip: usize| {
        slide
            .pieces
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != skip)
            .filter_map(|(_, piece)| piece.text.as_ref().map(|text| text.size_pt))
            .fold(0.0_f32, f32::max)
    };
    let short = |text: &TextInfo| {
        !text.listed
            && text.paragraphs <= 2
            && text.words() <= config.title_max_words
            && text.size_pt >= config.title_min_size_pt
    };
    let texts: Vec<(usize, &TextInfo, Option<Frame>)> = slide
        .pieces
        .iter()
        .enumerate()
        .filter_map(|(index, piece)| Some((index, piece.text.as_ref()?, piece.frame)))
        .collect();
    let near_top = |frame: Option<Frame>| frame.is_some_and(|frame| frame.y <= top_pt);
    let by_size = |a: &&(usize, &TextInfo, Option<Frame>), b: &&(usize, &TextInfo, Option<Frame>)| {
        let y = |frame: Option<Frame>| frame.map_or(0.0, |frame| frame.y);
        a.1.size_pt.total_cmp(&b.1.size_pt).then(y(b.2).total_cmp(&y(a.2)))
    };
    let first = texts
        .iter()
        .filter(|(index, text, frame)| {
            let Some((size, words)) = text.first else {
                return false;
            };
            near_top(*frame)
                && words <= config.title_max_words
                && size >= config.title_min_size_pt
                && size >= text.rest_size_pt.max(others(*index)) * config.title_size_ratio
                && matches!(slide.pieces[*index].items.first(), Some(Item::Para(para)) if para.list.is_none())
        })
        .max_by(by_size)
        .map(|(index, _, _)| *index);
    if let Some(index) = first {
        let first = slide.pieces[index].items.remove(0);
        slide.pieces[index].text = None;
        // A box left empty is no shape to read by its place.
        if slide.pieces[index].items.is_empty() {
            slide.pieces.remove(index);
        }
        slide.title = Some(title_text(paragraphs_of(vec![first])));
        return;
    }
    let whole = texts
        .iter()
        .filter(|(index, text, frame)| {
            near_top(*frame) && short(text) && text.size_pt >= others(*index) * config.title_size_ratio
        })
        .max_by(by_size)
        .map(|(index, _, _)| *index);
    if let Some(index) = whole {
        let piece = slide.pieces.remove(index);
        slide.title = Some(title_text(paragraphs_of(piece.items)));
        return;
    }
    if let [(index, text, _)] = texts[..]
        && short(text)
    {
        let piece = slide.pieces.remove(index);
        slide.title = Some(title_text(paragraphs_of(piece.items)));
    }
}

/// The paragraphs of `items`, tables left out.
fn paragraphs_of(items: Vec<Item>) -> Vec<Para> {
    items
        .into_iter()
        .filter_map(|item| match item {
            Item::Para(para) => Some(para),
            Item::Table(_) => None,
        })
        .collect()
}

/// Whether the first slide `slide` is a title slide although neither its
/// title (`ctrTitle`) nor its layout says so: it has a title and, besides
/// it, no list, table or chart, and little text
/// (`PowerPointConfig::title_slide_max_words`).
pub(crate) fn looks_like_title_slide(slide: &SlideContent, config: &PowerPointConfig) -> bool {
    if slide.title.is_none() {
        return false;
    }
    let mut words = slide
        .subtitle
        .iter()
        .map(|para| super::text::inlines_text(&para.inlines).split_whitespace().count())
        .sum::<usize>();
    for item in slide.pieces.iter().flat_map(|piece| &piece.items) {
        match item {
            Item::Table(_) => return false,
            Item::Para(para) => {
                if para.list.is_some()
                    || para
                        .inlines
                        .iter()
                        .any(|inline| matches!(inline, crate::word::ir::Inline::Chart(_)))
                {
                    return false;
                }
                words += super::text::inlines_text(&para.inlines).split_whitespace().count();
            }
        }
    }
    words <= config.title_slide_max_words
}
