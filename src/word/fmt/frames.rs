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
        group_inside(block);
    }
    if !blocks
        .iter()
        .any(|block| matches!(block, Block::Para(para) if para.props.frame.is_some()))
    {
        return;
    }
    let mut grouping = Grouping {
        out: Vec::with_capacity(blocks.len()),
        waiting: Vec::new(),
        current: None,
    };
    for block in std::mem::take(blocks) {
        grouping.step(block);
    }
    let Grouping {
        mut out,
        mut waiting,
        current,
    } = grouping;
    if let Some((open, members)) = current {
        waiting.push(shape(open, members));
    }
    put_waiting(&mut out, waiting);
    *blocks = out;
}

/// The state of the grouping of a list of blocks.
struct Grouping {
    /// The blocks kept.
    out: Vec<Block>,
    /// Shapes waiting for the paragraph they are anchored to.
    waiting: Vec<Inline>,
    /// The frame being gathered, with its paragraphs.
    current: Option<(Frame, Vec<Block>)>,
}

impl Grouping {
    /// Takes the next block: a framed paragraph joins its frame, another
    /// paragraph takes the frames waiting before it.
    fn step(&mut self, block: Block) {
        let frame = match &block {
            Block::Para(para) => para.props.frame.clone(),
            Block::Table(_) => None,
        };
        if let Some((open, members)) = self.current.take() {
            if frame.as_ref() == Some(&open) {
                self.current = Some((open, members));
            } else {
                self.waiting.push(shape(open, members));
            }
        }
        match (frame, block) {
            (Some(frame), Block::Para(mut para)) => {
                para.props.frame = None;
                match self.current.as_mut() {
                    Some((_, members)) => members.push(Block::Para(para)),
                    None => self.current = Some((frame, vec![Block::Para(para)])),
                }
            }
            (_, Block::Para(mut para)) => {
                if !self.waiting.is_empty() {
                    let rest = std::mem::take(&mut para.inlines);
                    para.inlines = std::mem::take(&mut self.waiting);
                    para.inlines.extend(rest);
                }
                self.out.push(Block::Para(para));
            }
            (_, table) => self.out.push(table),
        }
    }
}

/// Groups the framed paragraphs inside `block`: in its cells (where Word
/// ignores frames) and its text boxes.
fn group_inside(block: &mut Block) {
    match block {
        Block::Table(table) => {
            for cell in table.rows.iter_mut().flat_map(|row| row.cells.iter_mut()) {
                // Word ignores the frames of paragraphs in a table: they
                // stay in the text of their cell.
                for block in &mut cell.blocks {
                    if let Block::Para(para) = block {
                        para.props.frame = None;
                    }
                }
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

/// Puts the shapes that no paragraph follows in the last paragraph of
/// `out`, else in a new empty one.
fn put_waiting(out: &mut Vec<Block>, waiting: Vec<Inline>) {
    if waiting.is_empty() {
        return;
    }
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
