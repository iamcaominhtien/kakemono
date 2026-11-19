//! Frames (`w:framePr`, `sprmPPc`): the paragraphs of a frame are taken out
//! of the text and placed like a text box, anchored to the paragraph that
//! follows them.

use super::model::{Block, FDoc, Frame, Inline, Para, Shape};

/// Turns every run of framed paragraphs of the document into a floating
/// shape.
pub(crate) fn group(doc: &mut FDoc) {
    for section in &mut doc.sections {
        group_blocks(&mut section.blocks);
    }
    for part in &mut doc.parts {
        group_blocks(part);
    }
    for (_, blocks) in doc.footnotes.iter_mut().chain(doc.endnotes.iter_mut()) {
        group_blocks(blocks);
    }
}

/// Groups the framed paragraphs of `blocks`, and of the cells and text boxes
/// in them.
fn group_blocks(blocks: &mut Vec<Block>) {
    for block in blocks.iter_mut() {
        match block {
            Block::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|row| row.cells.iter_mut()) {
                    group_blocks(&mut cell.blocks);
                }
            }
            Block::Para(para) => {
                for inline in &mut para.inlines {
                    if let Inline::Shape(shape) = inline {
                        group_blocks(&mut shape.blocks);
                    }
                }
            }
        }
    }
    if !blocks
        .iter()
        .any(|block| matches!(block, Block::Para(para) if para.props.frame.is_some()))
    {
        return;
    }
    let mut out: Vec<Block> = Vec::with_capacity(blocks.len());
    // Shapes waiting for the paragraph they are anchored to.
    let mut waiting: Vec<Inline> = Vec::new();
    let mut current: Option<(Frame, Vec<Block>)> = None;
    for block in std::mem::take(blocks) {
        let frame = match &block {
            Block::Para(para) => para.props.frame.clone(),
            Block::Table(_) => None,
        };
        if let Some((open, members)) = current.take() {
            if frame.as_ref() == Some(&open) {
                current = Some((open, members));
            } else {
                waiting.push(shape(open, members));
            }
        }
        match (frame, block) {
            (Some(frame), Block::Para(mut para)) => {
                para.props.frame = None;
                match current.as_mut() {
                    Some((_, members)) => members.push(Block::Para(para)),
                    None => current = Some((frame, vec![Block::Para(para)])),
                }
            }
            (_, Block::Para(mut para)) => {
                if !waiting.is_empty() {
                    let rest = std::mem::take(&mut para.inlines);
                    para.inlines = std::mem::take(&mut waiting);
                    para.inlines.extend(rest);
                }
                out.push(Block::Para(para));
            }
            (_, table) => out.push(table),
        }
    }
    if let Some((open, members)) = current {
        waiting.push(shape(open, members));
    }
    if !waiting.is_empty() {
        // No paragraph follows: the last one before holds them, else an
        // empty one.
        match out.iter_mut().rev().find_map(|block| match block {
            Block::Para(para) => Some(para),
            Block::Table(_) => None,
        }) {
            Some(para) => para.inlines.extend(waiting),
            None => out.push(Block::Para(Box::new(Para {
                inlines: waiting,
                ..Default::default()
            }))),
        }
    }
    *blocks = out;
}

/// The floating shape of a frame holding `blocks`.
fn shape(frame: Frame, blocks: Vec<Block>) -> Inline {
    let fmt = match blocks.first() {
        Some(Block::Para(para)) => para.mark.clone(),
        _ => Default::default(),
    };
    Inline::Shape(Box::new(Shape {
        width: frame.width,
        height: frame.height,
        anchor: Some(frame.anchor),
        fill: None,
        line: None,
        blocks,
        insets: [0.0; 4],
        pictures: Vec::new(),
        auto_height: !frame.exact,
        fmt,
    }))
}
