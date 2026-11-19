//! Automatic text colour: Word draws text whose colour is automatic in
//! black, or in white on a dark background (a shaded cell, paragraph, run or
//! text box). This pass gives such text its colour.

use super::model::{Block, FDoc, Inline, Rgb, RunFmt};

/// Brightness (0 to 255) below which a background is dark.
const DARK_BRIGHTNESS: f32 = 128.0;
/// White.
const WHITE: Rgb = [255, 255, 255];

/// Colours the automatic text of `doc` that lies on a dark background.
pub(super) fn apply(doc: &mut FDoc) {
    for section in &mut doc.sections {
        blocks(&mut section.blocks, None);
    }
    for part in &mut doc.parts {
        blocks(part, None);
    }
    for (_, notes) in doc.footnotes.iter_mut().chain(doc.endnotes.iter_mut()) {
        blocks(notes, None);
    }
}

/// Whether `color` is dark.
fn is_dark(color: Rgb) -> bool {
    let [red, green, blue] = color.map(f32::from);
    0.299 * red + 0.587 * green + 0.114 * blue < DARK_BRIGHTNESS
}

/// Colours the automatic text of `blocks`, on `background`.
fn blocks(list: &mut [Block], background: Option<Rgb>) {
    for block in list {
        match block {
            Block::Para(para) => {
                let background = para.props.shading.or(background);
                if let Some(number) = &mut para.number {
                    run(&mut number.fmt, background);
                }
                run(&mut para.mark, background);
                for inline in &mut para.inlines {
                    inline_fmt(inline, background);
                }
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        blocks(&mut cell.blocks, cell.shading.or(background));
                    }
                }
            }
        }
    }
}

/// Colours the automatic text of `inline`, on `background`.
fn inline_fmt(inline: &mut Inline, background: Option<Rgb>) {
    match inline {
        Inline::Text { fmt, .. }
        | Inline::Tab { fmt }
        | Inline::Break { fmt, .. }
        | Inline::Field { fmt, .. }
        | Inline::Image { fmt, .. }
        | Inline::NoteRef { fmt, .. }
        | Inline::NoteNumber { fmt }
        | Inline::Math { fmt, .. } => run(fmt, background),
        Inline::Shape(shape) => {
            // A text box has its own background: its fill, or else none.
            let fill = shape.fill;
            blocks(&mut shape.blocks, fill);
        }
    }
}

/// Gives the run `fmt` white when its colour is automatic and it lies on a
/// dark background (its own highlight or shading, else `background`).
fn run(fmt: &mut RunFmt, background: Option<Rgb>) {
    if fmt.color.is_none() && fmt.highlight.or(fmt.shading).or(background).is_some_and(is_dark) {
        fmt.color = Some(WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Cell, Para, Row, Section, Table};
    use super::*;

    /// A paragraph in the given cell shading, with one run of automatic text.
    fn doc_with_cell(shading: Rgb) -> FDoc {
        let para = Para {
            inlines: vec![Inline::Text {
                text: "Section A".into(),
                fmt: RunFmt::default(),
            }],
            ..Default::default()
        };
        let cell = Cell {
            shading: Some(shading),
            blocks: vec![Block::Para(Box::new(para))],
            ..Default::default()
        };
        let table = Table {
            rows: vec![Row {
                cells: vec![cell],
                ..Default::default()
            }],
            ..Default::default()
        };
        FDoc {
            sections: vec![Section {
                blocks: vec![Block::Table(table)],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    /// The colour of the text of the first cell.
    fn text_color(doc: &FDoc) -> Option<Rgb> {
        let Block::Table(table) = &doc.sections[0].blocks[0] else {
            return None;
        };
        let Block::Para(para) = &table.rows[0].cells[0].blocks[0] else {
            return None;
        };
        let Inline::Text { fmt, .. } = &para.inlines[0] else {
            return None;
        };
        fmt.color
    }

    /// Automatic text is white in a black cell and stays automatic in a
    /// light one.
    #[test]
    fn automatic_text_is_white_on_dark_cells() {
        let mut dark = doc_with_cell([0, 0, 0]);
        apply(&mut dark);
        assert_eq!(text_color(&dark), Some(WHITE));
        let mut light = doc_with_cell([230, 230, 230]);
        apply(&mut light);
        assert_eq!(text_color(&light), None);
    }
}
