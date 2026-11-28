//! The content of a Word document as both readers give it: paragraphs
//! with their style, list numbering and runs, tables, notes and pictures.
//! The `.docx` reader (`docx`) and the `.doc` reader (`doc`) fill it in;
//! `build` turns it into a [`Document`](crate::model::Document).

use crate::chart::Placed;
use crate::model::{MathNode, Picture, Span};

/// A Word document read from its file.
#[derive(Debug, Default)]
pub(crate) struct WordDoc {
    /// The body, in order.
    pub(crate) body: Vec<Item>,
    /// The footnotes and endnotes, by their id in the file.
    pub(crate) notes: Vec<Note>,
    /// The pictures the runs refer to, as PNG or JPEG.
    pub(crate) pictures: Vec<Picture>,
    /// The title in the document properties, if any.
    pub(crate) title: Option<String>,
    /// Font size of the normal text, in points.
    pub(crate) body_size_pt: f32,
    /// Font of the normal text.
    pub(crate) body_font: String,
}

/// A part of the body: a paragraph or a table.
#[derive(Debug)]
pub(crate) enum Item {
    /// A paragraph.
    Para(Para),
    /// A table.
    Table(Table),
}

/// A paragraph.
#[derive(Debug, Default)]
pub(crate) struct Para {
    /// Name of its paragraph style, as Word shows it ("heading 1",
    /// "Caption"); empty for the default style.
    pub(crate) style: String,
    /// Outline level (0 for a heading of level 1), from the paragraph or
    /// its style.
    pub(crate) outline: Option<u8>,
    /// List numbering, when the paragraph is an item of a list.
    pub(crate) list: Option<ListMark>,
    /// The content, in order.
    pub(crate) inlines: Vec<Inline>,
    /// Names of the bookmarks that start in the paragraph.
    pub(crate) bookmarks: Vec<String>,
}

/// The list numbering of a paragraph.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ListMark {
    /// Nesting level (0 for the outermost).
    pub(crate) level: u8,
    /// The marker as Word draws it ("1.", "(a)", "iv."); `None` for a
    /// bullet.
    pub(crate) marker: Option<String>,
}

/// A piece of the content of a paragraph.
#[derive(Debug)]
pub(crate) enum Inline {
    /// Styled text. A link inside the document has an `href` of
    /// `#bookmark`, with the name of a Word bookmark.
    Text(Span),
    /// A formula (Office Math).
    Math {
        /// Its structure.
        node: MathNode,
        /// It is displayed on a line of its own (`m:oMathPara`).
        display: bool,
    },
    /// A picture.
    Picture {
        /// Index into [`WordDoc::pictures`]; `None` when its format cannot
        /// be shown (a Windows metafile).
        index: Option<usize>,
        /// Width in the document, in points.
        width_pt: f32,
        /// Height in the document, in points.
        height_pt: f32,
        /// Its description, if any.
        alt: String,
    },
    /// A chart: read and drawn, with the index of its picture in
    /// [`WordDoc::pictures`] when it was drawn.
    Chart(Box<(Placed, Option<usize>)>),
    /// The mark of a footnote or endnote.
    NoteRef {
        /// It is an endnote.
        endnote: bool,
        /// The id of the note in the file.
        id: String,
    },
}

/// A footnote or endnote.
#[derive(Debug)]
pub(crate) struct Note {
    /// It is an endnote.
    pub(crate) endnote: bool,
    /// Its id in the file.
    pub(crate) id: String,
    /// Its paragraphs.
    pub(crate) body: Vec<Item>,
}

/// A table.
#[derive(Debug, Default)]
pub(crate) struct Table {
    /// Its rows, top to bottom.
    pub(crate) rows: Vec<Row>,
}

/// A row of a table.
#[derive(Debug, Default)]
pub(crate) struct Row {
    /// Its cells, left to right.
    pub(crate) cells: Vec<Cell>,
    /// The row is a header row (repeated on every page).
    pub(crate) header: bool,
}

/// A cell of a table.
#[derive(Debug, Default)]
pub(crate) struct Cell {
    /// Number of grid columns it spans.
    pub(crate) span: usize,
    /// How it merges with the cell above.
    pub(crate) merge: VMerge,
    /// Its content.
    pub(crate) body: Vec<Item>,
}

/// Vertical merging of a cell with the cells above and below.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VMerge {
    /// Not merged.
    #[default]
    None,
    /// The first cell of a merged column of cells.
    Restart,
    /// Merged into the cell above.
    Continue,
}

impl Para {
    /// The plain text of the paragraph (formulas as their plain text).
    pub(crate) fn text(&self) -> String {
        self.inlines
            .iter()
            .map(|inline| match inline {
                Inline::Text(span) => span.text.clone(),
                Inline::Math { node, .. } => super::omml::plain_text(node),
                _ => String::new(),
            })
            .collect()
    }
}
