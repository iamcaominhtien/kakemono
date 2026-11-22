//! A Word document ([`WordDoc`]) turned into a [`Document`]: paragraph
//! styles and numbering give the kinds of the blocks (title, headings,
//! lists, captions, references, ...), pictures and tables become floats,
//! notes follow the block that calls them, and cross-references are linked
//! as in a PDF.

use super::ir::{Inline, Item, Para, Table as WordTable, VMerge, WordDoc};
use super::omml::plain_text;
use super::pictures::{FALLBACK_PICTURE_PT, bounded_size, pixel_size_pt};
use crate::analysis::semantics::AssembleConfig;
use crate::analysis::semantics::assemble::{link_blocks, link_contents, reference_label, slug_marker};
use crate::analysis::semantics::patterns::{HEADING_LETTER, HEADING_NUM, HEADING_ROMAN, REF_HEADING};
use crate::analysis::semantics::text::{slug, unique_anchor};
use crate::common::geom::Rect;
use crate::common::patterns::CAP_LABEL;
use crate::model::{
    BOLD, Block, BlockKind, Document, FloatKind, ImageSpec, MATH, MathNode, Meta, Picture, Span, Table, TableCell,
};
use regex::Regex;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// Ratio of the line spacing of running text to its font size, for a
/// document that gives no lines.
const LEADING_RATIO: f32 = 1.2;
/// The math font named for a document with Office Math (Word's own).
const OFFICE_MATH_FONT: &str = "Cambria Math";

/// The number of an equation as it is set next to it: "(3)", "(2.1a)".
static EQUATION_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\(\s*[0-9A-Za-z][0-9A-Za-z.\-]*\s*\)$").unwrap());
/// The heading of an abstract.
static ABSTRACT_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(abstract|summary)[.:]?$").unwrap());
/// The page number at the end of a contents entry.
static CONTENTS_PAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\.*\s*(\d{1,4}|[ivxlc]{1,6})\s*$").unwrap());

/// What a paragraph is, from its style, numbering and content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// The title.
    Title,
    /// A subtitle or a note of the front matter.
    Note,
    /// Authors, affiliations, addresses.
    Authors,
    /// A heading of this level.
    Heading(u8),
    /// The caption of a figure or table.
    Caption,
    /// A footnote written as a paragraph (with a footnote style).
    FootnoteText,
    /// An entry of the bibliography.
    Reference,
    /// A line of a code listing.
    Code,
    /// An entry of a table of contents, at this level.
    Contents(u8),
    /// An item of a list.
    ListItem,
    /// Running text.
    Paragraph,
}

/// A caption read but not yet given to its float.
#[derive(Debug)]
struct PendingCaption {
    /// Its text.
    spans: Vec<Span>,
    /// Its label ("Figure 2").
    label: Option<String>,
}

/// Builds the blocks of a document, in order.
struct Builder<'a> {
    /// The notes of the document, by (endnote, id).
    notes: HashMap<(bool, String), &'a [Item]>,
    /// The number given to each note, in the order they are called.
    note_numbers: HashMap<(bool, String), String>,
    /// Footnotes counted so far.
    footnotes: usize,
    /// Endnotes counted so far.
    endnotes: usize,
    /// Notes called in the block being built, written after it.
    pending_notes: Vec<(bool, String)>,
    /// Endnotes, written at the end of the document.
    endnote_blocks: Vec<Block>,
    /// The blocks so far.
    blocks: Vec<Block>,
    /// Anchors given so far.
    anchors: HashSet<String>,
    /// For each Word bookmark, the index of the block it is in.
    bookmarks: HashMap<String, usize>,
    /// Bookmarks of empty paragraphs, given to the next block.
    loose_bookmarks: Vec<String>,
    /// A caption waiting for the float after it.
    caption: Option<PendingCaption>,
    /// The paragraphs read now belong to the abstract.
    in_abstract: bool,
    /// The paragraphs read now are the bibliography.
    in_references: bool,
    /// Code lines being gathered into one listing.
    code: Vec<Vec<Span>>,
    /// The pictures of the document, by index.
    pictures: &'a [Picture],
    /// Sizes of the pictures in points (96 pixels per inch), by index,
    /// found when first needed: only a picture whose document gives it no
    /// size needs its own, and an SVG is parsed to know it.
    picture_sizes: Vec<OnceCell<Option<(f32, f32)>>>,
}

/// Builds the document of a Word file.
pub(crate) fn build(word: WordDoc, source: Option<String>) -> Document {
    let notes = word
        .notes
        .iter()
        .map(|note| ((note.endnote, note.id.clone()), note.body.as_slice()))
        .collect();
    let picture_sizes = word.pictures.iter().map(|_| OnceCell::new()).collect();
    let mut builder = Builder {
        notes,
        note_numbers: HashMap::new(),
        footnotes: 0,
        endnotes: 0,
        pending_notes: Vec::new(),
        endnote_blocks: Vec::new(),
        blocks: Vec::new(),
        anchors: HashSet::new(),
        bookmarks: HashMap::new(),
        loose_bookmarks: Vec::new(),
        caption: None,
        in_abstract: false,
        in_references: false,
        code: Vec::new(),
        pictures: &word.pictures,
        picture_sizes,
    };
    builder.items(&word.body);
    builder.flush_code();
    builder.flush_caption();
    let endnotes = std::mem::take(&mut builder.endnote_blocks);
    builder.blocks.extend(endnotes);
    let mut blocks = builder.finish();
    link_blocks(&mut blocks, &AssembleConfig::default());
    link_contents(&mut blocks);
    let has_math = blocks.iter().any(|block| {
        block.math.is_some()
            || block.spans.iter().any(|span| span.math.is_some())
            || block.table.as_ref().is_some_and(|table| {
                table
                    .rows
                    .iter()
                    .flatten()
                    .any(|cell| cell.spans.iter().any(|span| span.math.is_some()))
            })
    });
    let title = blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Title))
        .map(Block::text)
        .collect::<Vec<_>>()
        .join(" ");
    let title = Some(title.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|title| !title.is_empty())
        .or(word.title);
    Document {
        meta: Meta {
            title,
            source,
            ..Default::default()
        },
        pages: Vec::new(),
        body_size: word.body_size_pt,
        leading: word.body_size_pt * LEADING_RATIO,
        body_font: word.body_font,
        math_font: if has_math {
            OFFICE_MATH_FONT.to_string()
        } else {
            String::new()
        },
        blocks,
        pictures: word.pictures,
    }
}

impl Builder<'_> {
    /// Adds the blocks of the paragraphs and tables `items`.
    fn items(&mut self, items: &[Item]) {
        for (idx, item) in items.iter().enumerate() {
            match item {
                Item::Para(para) => {
                    let next_is_float = items[idx + 1..]
                        .iter()
                        .find(|item| !matches!(item, Item::Para(para) if is_empty(para)))
                        .is_some_and(|item| match item {
                            Item::Table(_) => true,
                            Item::Para(para) => is_picture_only(para),
                        });
                    self.paragraph(para, next_is_float);
                }
                Item::Table(table) => {
                    self.flush_code();
                    self.table(table);
                }
            }
        }
    }

    /// Adds the blocks of a paragraph. `next_is_float` says whether a table
    /// or a picture comes next, which a caption may belong to.
    fn paragraph(&mut self, para: &Para, next_is_float: bool) {
        if is_empty(para) {
            self.loose_bookmarks.extend(para.bookmarks.iter().cloned());
            return;
        }
        let role = self.role(para);
        if role != Role::Code {
            self.flush_code();
        }
        match role {
            Role::Caption => return self.caption_para(para, next_is_float),
            Role::Code => {
                let spans = self.spans(para, false);
                self.code.extend(split_lines(spans));
                self.mark_bookmarks(para);
                return;
            }
            _ => {}
        }
        // The paragraph is cut at its displayed formulas and pictures.
        let mut text: Vec<&Inline> = Vec::new();
        let mut first = true;
        for inline in &para.inlines {
            match inline {
                Inline::Math { display: true, .. } | Inline::Picture { .. } => {
                    if has_text(&text) {
                        self.text_block(para, role, &text, first);
                        first = false;
                    }
                    text.clear();
                    self.object(inline);
                }
                _ => text.push(inline),
            }
        }
        if has_text(&text) {
            self.text_block(para, role, &text, first);
        }
        self.mark_bookmarks(para);
    }

    /// What a paragraph is, from its style, numbering, text and the section
    /// it is in. Headings also open and close the abstract and the
    /// bibliography.
    fn role(&mut self, para: &Para) -> Role {
        let key = style_key(&para.style);
        let text = para.text();
        let text = text.trim();
        let heading_level = para
            .outline
            .map(|level| level + 1)
            .or_else(|| heading_number(&key))
            .filter(|_| !key.starts_with("toc"));
        let role = if key == "title" {
            Role::Title
        } else if key == "subtitle" {
            Role::Note
        } else if let Some(level) = heading_level {
            Role::Heading(level)
        } else if key.contains("author") || key.contains("affiliation") || key == "address" || key == "email" {
            Role::Authors
        } else if key == "abstractheading" || key == "abstracttitle" {
            Role::Heading(1)
        } else if key.contains("caption") && !is_picture_only(para) {
            Role::Caption
        } else if key == "footnotetext" || key == "endnotetext" {
            Role::FootnoteText
        } else if let Some(level) = key.strip_prefix("toc") {
            Role::Contents(level.parse().unwrap_or(1))
        } else if key.contains("bibliograph") || key == "references" || key == "reference" || self.in_references {
            Role::Reference
        } else if is_code(para, &key) {
            Role::Code
        } else if key == "note" {
            Role::Note
        } else if para.list.is_some() {
            Role::ListItem
        } else {
            Role::Paragraph
        };
        if let Role::Heading(_) = role {
            self.in_abstract = ABSTRACT_HEADING.is_match(text);
            self.in_references = REF_HEADING.is_match(text);
        } else if key == "abstract" {
            self.in_abstract = true;
        }
        role
    }

    /// Adds the block of the text `inlines` of a paragraph of role `role`.
    /// `first` is false for the text after a displayed formula or picture
    /// in the same paragraph.
    fn text_block(&mut self, para: &Para, role: Role, inlines: &[&Inline], first: bool) {
        let marker = para
            .list
            .as_ref()
            .and_then(|list| list.marker.clone())
            .filter(|_| first);
        let strip_bold = matches!(role, Role::Title | Role::Heading(_));
        let mut spans = self.inline_spans(inlines, strip_bold);
        if !first {
            let block = self.block(
                BlockKind::Paragraph {
                    in_abstract: self.in_abstract,
                },
                spans,
            );
            self.push(block);
            return;
        }
        let kind = match role {
            Role::Title => BlockKind::Title,
            Role::Note => BlockKind::Note,
            Role::Authors => {
                let rows = split_lines(spans.clone());
                let mut block = self.block(BlockKind::Authors, join_lines(spans));
                block.rows = rows;
                return self.push(block);
            }
            Role::Heading(level) => {
                if let Some(marker) = &marker {
                    spans.insert(0, Span::plain(format!("{marker} ")));
                }
                let text = text_of(&spans);
                let mut block = self.block(BlockKind::Heading { level: level.min(6) }, spans);
                let base = match section_number(text.trim()) {
                    Some(number) => format!("sec-{number}"),
                    None => format!("sec-{}", slug(&text)),
                };
                block.anchor = Some(self.unique_anchor(base));
                return self.push(block);
            }
            Role::FootnoteText => return self.footnote_para(spans),
            Role::Reference => {
                if let Some(marker) = &marker {
                    spans.insert(0, Span::plain(format!("{marker} ")));
                }
                let label = reference_label(&text_of(&spans));
                let anchor = label
                    .as_ref()
                    .map(|label| self.unique_anchor(format!("ref-{}", slug(label))));
                let mut block = self.block(BlockKind::Reference { label }, spans);
                block.anchor = anchor;
                return self.push(block);
            }
            Role::Contents(level) => {
                let text = text_of(&spans);
                let page = CONTENTS_PAGE
                    .captures(&text)
                    .map(|captures| captures[1].to_string())
                    .unwrap_or_default();
                if !page.is_empty() {
                    let keep = text.len() - CONTENTS_PAGE.find(&text).map_or(0, |found| found.len());
                    truncate_spans(&mut spans, keep);
                }
                BlockKind::ContentsEntry { level, page }
            }
            Role::ListItem => {
                if let Some(marker) = &marker {
                    spans.insert(0, Span::plain(format!("{marker} ")));
                }
                BlockKind::ListItem {
                    marker,
                    level: para.list.as_ref().map_or(0, |list| list.level),
                }
            }
            Role::Paragraph | Role::Caption | Role::Code => BlockKind::Paragraph {
                in_abstract: self.in_abstract,
            },
        };
        let block = self.block(kind, spans);
        self.push(block);
    }

    /// Adds a displayed formula or a picture of a paragraph.
    fn object(&mut self, inline: &Inline) {
        match inline {
            Inline::Math { node, .. } => {
                let mut block = self.block(
                    BlockKind::Equation {
                        image: None,
                        number: None,
                    },
                    Vec::new(),
                );
                block.spans = vec![Span::plain(plain_text(node))];
                block.math = Some(node.clone());
                self.push(block);
            }
            Inline::Picture {
                index,
                width_pt,
                height_pt,
                alt,
            } => {
                let image = index.map(|index| self.picture_spec(index, *width_pt, *height_pt));
                if image.is_none() && self.caption.is_none() {
                    return;
                }
                let mut block = self.block(
                    BlockKind::Figure {
                        kind: FloatKind::Graphic,
                        image,
                        label: None,
                    },
                    Vec::new(),
                );
                if !alt.is_empty() {
                    block.rows = vec![vec![Span::plain(alt.clone())]];
                }
                self.push_float(block);
            }
            _ => {}
        }
    }

    /// The picture `index` at its size in the document (or its own size).
    fn picture_spec(&self, index: usize, width_pt: f32, height_pt: f32) -> ImageSpec {
        let (width, height) = bounded_size((width_pt, height_pt))
            .or_else(|| {
                let picture = self.pictures.get(index)?;
                *self.picture_sizes.get(index)?.get_or_init(|| pixel_size_pt(picture))
            })
            .unwrap_or(FALLBACK_PICTURE_PT);
        ImageSpec {
            page: 0,
            rect: Rect {
                x0: 0.0,
                y0: 0.0,
                x1: width,
                y1: height,
            },
            transparent: false,
            embedded: Some(index),
        }
    }

    /// Handles a caption: given to the float before it when that float has
    /// none and nothing comes after; kept for the float after it otherwise;
    /// a paragraph when there is no float either side.
    fn caption_para(&mut self, para: &Para, next_is_float: bool) {
        self.flush_caption();
        let spans = self.spans(para, false);
        let label = caption_label(&text_of(&spans));
        let previous_free = self
            .blocks
            .last()
            .is_some_and(|block| matches!(block.kind, BlockKind::Figure { label: None, .. }) && block.spans.is_empty());
        let wants_next = next_is_float && !(previous_free && !label_is_table(label.as_deref()) && is_figure_last(self));
        if previous_free && !wants_next {
            let index = self.blocks.len() - 1;
            self.caption_float(index, spans, label, false);
        } else if next_is_float {
            self.caption = Some(PendingCaption { spans, label });
        } else {
            let block = self.block(BlockKind::Paragraph { in_abstract: false }, spans);
            self.push(block);
        }
        self.mark_bookmarks(para);
    }

    /// Writes a caption kept for a float that did not come as a paragraph.
    fn flush_caption(&mut self) {
        if let Some(caption) = self.caption.take() {
            let block = self.block(BlockKind::Paragraph { in_abstract: false }, caption.spans);
            self.push(block);
        }
    }

    /// Gives the caption `spans` (labelled `label`) to the float block at
    /// `index`: its kind and anchor follow the label.
    fn caption_float(&mut self, index: usize, spans: Vec<Span>, label: Option<String>, above: bool) {
        let kind = match label.as_deref() {
            Some(label) if label_is_table(Some(label)) => FloatKind::Table,
            Some(label)
                if label.starts_with("Algorithm") || label.starts_with("ALGORITHM") || label.starts_with("Listing") =>
            {
                FloatKind::Algorithm
            }
            Some(_) => FloatKind::Figure,
            None => FloatKind::Graphic,
        };
        let anchor = label.as_ref().map(|label| {
            let num = label.rsplit(' ').next().unwrap_or("");
            let prefix = match kind {
                FloatKind::Table => "tab",
                FloatKind::Algorithm => "alg",
                _ => "fig",
            };
            self.unique_anchor(format!("{prefix}-{num}"))
        });
        let block = &mut self.blocks[index];
        if let BlockKind::Figure {
            kind: float_kind,
            label: float_label,
            ..
        } = &mut block.kind
        {
            *float_kind = if block.table.is_some() && kind == FloatKind::Graphic {
                FloatKind::Table
            } else {
                kind
            };
            *float_label = label;
        }
        block.spans = spans;
        block.caption_above = above;
        if block.anchor.is_none() {
            block.anchor = anchor;
        }
    }

    /// Adds a float block, with the caption waiting before it.
    fn push_float(&mut self, block: Block) {
        self.push(block);
        if let Some(caption) = self.caption.take() {
            let index = self.blocks.len() - 1;
            self.caption_float(index, caption.spans, caption.label, true);
        }
    }

    /// Adds a footnote written as a paragraph: its mark is the raised text
    /// it starts with.
    fn footnote_para(&mut self, mut spans: Vec<Span>) {
        let mut marker = String::new();
        while spans.first().is_some_and(|span| span.style & crate::model::SUP != 0) {
            marker.push_str(spans.remove(0).text.trim());
        }
        trim_spans(&mut spans);
        let anchor = (!marker.is_empty()).then(|| self.unique_anchor(format!("fn-{}", slug_marker(&marker))));
        let mut block = self.block(BlockKind::Footnote { marker }, spans);
        block.anchor = anchor;
        self.push(block);
    }

    /// Adds a table: an equation with its number when it is the usual
    /// layout table of a numbered equation, its content when it has a
    /// single cell, a table float otherwise.
    fn table(&mut self, table: &WordTable) {
        if let Some((node, number)) = numbered_equation(table) {
            let mut block = self.block(
                BlockKind::Equation {
                    image: None,
                    number: Some(number),
                },
                Vec::new(),
            );
            block.spans = vec![Span::plain(plain_text(&node))];
            block.math = Some(node);
            self.push(block);
            return;
        }
        if let [row] = table.rows.as_slice()
            && let [cell] = row.cells.as_slice()
        {
            return self.items(&cell.body);
        }
        let built = self.rebuild_table(table);
        let mut block = self.block(
            BlockKind::Figure {
                kind: FloatKind::Table,
                image: None,
                label: None,
            },
            Vec::new(),
        );
        block.table = Some(built);
        self.push_float(block);
    }

    /// The table of the document as a rebuilt [`Table`]: its grid, spans,
    /// header rows and rules.
    fn rebuild_table(&mut self, table: &WordTable) -> Table {
        let mut rows: Vec<Vec<TableCell>> = Vec::new();
        // For each column, the row and position of the cell that a merge
        // continues.
        let mut open: HashMap<usize, (usize, usize)> = HashMap::new();
        for (row_idx, row) in table.rows.iter().enumerate() {
            let mut cells = Vec::new();
            let mut col = 0;
            for cell in &row.cells {
                if cell.merge == VMerge::Continue
                    && let Some(&(start_row, pos)) = open.get(&col)
                    && let Some(start) = rows.get_mut(start_row).and_then(|cells| cells.get_mut(pos))
                {
                    start.rowspan += 1;
                    col += cell.span;
                    continue;
                }
                let spans = self.cell_spans(&cell.body);
                if cell.merge == VMerge::Restart {
                    open.insert(col, (row_idx, cells.len()));
                } else {
                    open.remove(&col);
                }
                cells.push(TableCell {
                    col,
                    colspan: cell.span,
                    rowspan: 1,
                    spans,
                });
                col += cell.span;
            }
            rows.push(cells);
        }
        let n_cols = table
            .rows
            .iter()
            .map(|row| row.cells.iter().map(|cell| cell.span).sum::<usize>())
            .max()
            .unwrap_or(0)
            .max(1);
        let marked = table.rows.iter().take_while(|row| row.header).count();
        let header_rows = if marked > 0 {
            marked
        } else {
            usize::from(rows.len() > 1)
        };
        Table::ruled(rows, n_cols, header_rows, vec!["left"; n_cols])
    }

    /// The text of a table cell: its paragraphs one after another.
    fn cell_spans(&mut self, items: &[Item]) -> Vec<Span> {
        let mut spans: Vec<Span> = Vec::new();
        for item in items {
            let Item::Para(para) = item else { continue };
            if is_empty(para) {
                continue;
            }
            if !spans.is_empty() {
                spans.push(Span::plain(" "));
            }
            let inlines: Vec<&Inline> = para.inlines.iter().collect();
            let mut more = self.inline_spans(&inlines, false);
            if let Some(marker) = para.list.as_ref().and_then(|list| list.marker.as_ref()) {
                more.insert(0, Span::plain(format!("{marker} ")));
            }
            spans.extend(more);
            self.mark_bookmarks(para);
        }
        merge_spans(spans)
    }

    /// The spans of a whole paragraph.
    fn spans(&mut self, para: &Para, strip_bold: bool) -> Vec<Span> {
        let inlines: Vec<&Inline> = para.inlines.iter().collect();
        self.inline_spans(&inlines, strip_bold)
    }

    /// The spans of `inlines`: text, formulas in running text, note marks
    /// (the notes are written after the block). `strip_bold` drops the bold
    /// of a heading's text.
    fn inline_spans(&mut self, inlines: &[&Inline], strip_bold: bool) -> Vec<Span> {
        let mut spans = Vec::new();
        for inline in inlines {
            match inline {
                Inline::Text(span) => {
                    let mut span = span.clone();
                    if strip_bold {
                        span.style &= !BOLD;
                    }
                    span.text = without_controls(&span.text);
                    span.href = span
                        .href
                        .map(|href| without_controls(&href))
                        .filter(|href| !href.is_empty());
                    spans.push(span);
                }
                Inline::Math { node, .. } => spans.push(math_span(node)),
                Inline::NoteRef { endnote, id } => {
                    let marker = self.note_marker(*endnote, id);
                    let mut span = Span::styled(marker.clone(), crate::model::SUP);
                    span.href = Some(format!("#fn-{}", slug_marker(&marker)));
                    spans.push(span);
                    self.pending_notes.push((*endnote, id.clone()));
                }
                Inline::Picture { .. } => {}
            }
        }
        let mut spans = merge_spans(spans);
        trim_spans(&mut spans);
        spans
    }

    /// The mark of a note, numbered in the order the notes are called:
    /// 1, 2, ... for footnotes, i, ii, ... for endnotes.
    fn note_marker(&mut self, endnote: bool, id: &str) -> String {
        let key = (endnote, id.to_string());
        if let Some(marker) = self.note_numbers.get(&key) {
            return marker.clone();
        }
        let marker = if endnote {
            self.endnotes += 1;
            roman_lower(self.endnotes)
        } else {
            self.footnotes += 1;
            self.footnotes.to_string()
        };
        self.note_numbers.insert(key, marker.clone());
        marker
    }

    /// A new block of kind `kind` with the text `spans`.
    fn block(&self, kind: BlockKind, spans: Vec<Span>) -> Block {
        let mut block = Block::new(0, kind);
        block.spans = spans;
        block
    }

    /// Adds a block, then the footnotes it calls (endnotes wait for the end
    /// of the document).
    fn push(&mut self, mut block: Block) {
        let bookmarks = std::mem::take(&mut self.loose_bookmarks);
        let index = self.blocks.len();
        for name in bookmarks {
            self.bookmarks.entry(name).or_insert(index);
        }
        if matches!(block.kind, BlockKind::Paragraph { .. }) {
            block.kind = BlockKind::Paragraph {
                in_abstract: self.in_abstract,
            };
        }
        self.blocks.push(block);
        for (endnote, id) in std::mem::take(&mut self.pending_notes) {
            let Some(body) = self.notes.get(&(endnote, id.clone())).copied() else {
                continue;
            };
            let marker = self.note_numbers.get(&(endnote, id)).cloned().unwrap_or_default();
            let mut spans = Vec::new();
            for item in body {
                let Item::Para(para) = item else { continue };
                if !spans.is_empty() {
                    spans.push(Span::plain(" "));
                }
                let inlines: Vec<&Inline> = para
                    .inlines
                    .iter()
                    .filter(|inline| !matches!(inline, Inline::NoteRef { .. }))
                    .collect();
                spans.extend(self.inline_spans(&inlines, false));
            }
            let mut note = self.block(BlockKind::Footnote { marker: marker.clone() }, merge_spans(spans));
            note.anchor = Some(self.unique_anchor(format!("fn-{}", slug_marker(&marker))));
            if endnote {
                self.endnote_blocks.push(note);
            } else {
                self.blocks.push(note);
            }
        }
    }

    /// Records the bookmarks of a paragraph as pointing to the last block.
    fn mark_bookmarks(&mut self, para: &Para) {
        let Some(index) = self.blocks.len().checked_sub(1) else {
            self.loose_bookmarks.extend(para.bookmarks.iter().cloned());
            return;
        };
        for name in &para.bookmarks {
            self.bookmarks.entry(name.clone()).or_insert(index);
        }
    }

    /// Writes the code lines gathered so far as one listing.
    fn flush_code(&mut self) {
        if self.code.is_empty() {
            return;
        }
        let rows = std::mem::take(&mut self.code);
        let mut block = self.block(BlockKind::Code, Vec::new());
        block.spans = rows.join(&Span::plain("\n"));
        block.rows = rows;
        self.push(block);
    }

    /// An anchor based on `base` that no block has yet.
    fn unique_anchor(&mut self, base: String) -> String {
        unique_anchor(&mut self.anchors, base)
    }

    /// The blocks, numbered, with the links to Word bookmarks pointed at
    /// the blocks that hold them (blocks without an anchor get one), and
    /// links to bookmarks that do not exist dropped.
    fn finish(mut self) -> Vec<Block> {
        let mut wanted: HashSet<String> = HashSet::new();
        for block in &self.blocks {
            for span in all_spans(block) {
                if let Some(target) = span.href.as_deref().and_then(|href| href.strip_prefix('#')) {
                    wanted.insert(target.to_string());
                }
            }
        }
        let mut targets: HashMap<String, String> = HashMap::new();
        let mut names: Vec<&String> = wanted.iter().collect();
        names.sort();
        for name in names {
            if let Some(&index) = self.bookmarks.get(name) {
                let anchor = match self.blocks[index].anchor.clone() {
                    Some(anchor) => anchor,
                    None => {
                        let anchor = self.unique_anchor(format!("bm-{}", slug(name)));
                        self.blocks[index].anchor = Some(anchor.clone());
                        anchor
                    }
                };
                targets.insert(name.clone(), anchor);
            }
        }
        let anchors = self.anchors.clone();
        for (id, block) in self.blocks.iter_mut().enumerate() {
            block.id = id;
            for span in all_spans_mut(block) {
                let Some(target) = span.href.as_deref().and_then(|href| href.strip_prefix('#')) else {
                    continue;
                };
                span.href = match targets.get(target) {
                    Some(anchor) => Some(format!("#{anchor}")),
                    None if anchors.contains(target) => span.href.take(),
                    None => None,
                };
            }
        }
        self.blocks
    }
}

/// The span of a formula in running text.
fn math_span(node: &MathNode) -> Span {
    let mut span = Span::styled(plain_text(node), MATH);
    span.math = Some(Box::new(node.clone()));
    span
}

/// A style name reduced to lowercase letters and digits ("Heading 1" →
/// "heading1"), to compare names the way Word's built-in styles are
/// spelled in any document.
fn style_key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

/// The level of a style named `heading N`.
fn heading_number(key: &str) -> Option<u8> {
    key.strip_prefix("heading")?
        .parse::<u8>()
        .ok()
        .filter(|level| (1..=9).contains(level))
}

/// The section number at the start of a heading ("2.1", "IV", "A"), as the
/// PDF analysis anchors it.
fn section_number(text: &str) -> Option<String> {
    [&*HEADING_NUM, &*HEADING_ROMAN, &*HEADING_LETTER]
        .iter()
        .find_map(|pattern| pattern.captures(text).map(|captures| captures["num"].to_string()))
}

/// Whether a paragraph is a line of code: its style says so, or all its
/// text is in a monospaced font.
fn is_code(para: &Para, key: &str) -> bool {
    if matches!(
        key,
        "sourcecode" | "code" | "htmlpreformatted" | "verbatim" | "codeblock"
    ) {
        return true;
    }
    // "Plain Text" is monospaced unless a document changes its font (some
    // templates use it for their body text): it is code only when its text
    // is monospaced, like any other paragraph.
    let mut any = false;
    for inline in &para.inlines {
        match inline {
            Inline::Text(span) if span.text.trim().is_empty() => {}
            Inline::Text(span) if span.style & crate::model::MONO != 0 => any = true,
            _ => return false,
        }
    }
    any && para.list.is_none()
}

/// Whether a paragraph has nothing to show.
fn is_empty(para: &Para) -> bool {
    para.inlines.iter().all(|inline| match inline {
        Inline::Text(span) => span.text.trim().is_empty(),
        Inline::Picture { index, .. } => index.is_none(),
        _ => false,
    })
}

/// Whether a paragraph holds only pictures.
fn is_picture_only(para: &Para) -> bool {
    let mut any = false;
    for inline in &para.inlines {
        match inline {
            Inline::Text(span) if span.text.trim().is_empty() => {}
            Inline::Picture { .. } => any = true,
            _ => return false,
        }
    }
    any
}

/// Whether the last block is a picture (not a table): a figure caption
/// below it belongs to it.
fn is_figure_last(builder: &Builder<'_>) -> bool {
    builder.blocks.last().is_some_and(|block| block.table.is_none())
}

/// Whether some of `inlines` is text to show.
fn has_text(inlines: &[&Inline]) -> bool {
    inlines.iter().any(|inline| match inline {
        Inline::Text(span) => !span.text.trim().is_empty(),
        Inline::Math { .. } | Inline::NoteRef { .. } => true,
        Inline::Picture { .. } => false,
    })
}

/// The label of a caption ("Figure 2", "Table 1"), without the
/// punctuation after it.
fn caption_label(text: &str) -> Option<String> {
    let found = CAP_LABEL.find(text.trim_start())?;
    let label = found
        .as_str()
        .trim()
        .trim_end_matches([':', '.', '|', '—', '–', '-'])
        .trim();
    Some(label.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Whether a caption label names a table.
fn label_is_table(label: Option<&str>) -> bool {
    label.is_some_and(|label| label.starts_with("Tab") || label.starts_with("TAB"))
}

/// The formula and number of a table that only lays out a numbered
/// equation: one row, one cell with a formula and nothing else, one with a
/// number such as "(3)", the others empty.
fn numbered_equation(table: &WordTable) -> Option<(MathNode, String)> {
    let [row] = table.rows.as_slice() else { return None };
    let mut formula = None;
    let mut number = None;
    for cell in &row.cells {
        for item in &cell.body {
            let Item::Para(para) = item else { return None };
            if is_empty(para) {
                continue;
            }
            let maths: Vec<&MathNode> = para
                .inlines
                .iter()
                .filter_map(|inline| match inline {
                    Inline::Math { node, .. } => Some(node),
                    _ => None,
                })
                .collect();
            let text = para.text();
            if let [node] = maths.as_slice()
                && para.inlines.iter().all(|inline| {
                    matches!(inline, Inline::Math { .. })
                        || matches!(inline, Inline::Text(span) if span.text.trim().is_empty())
                })
                && formula.is_none()
            {
                formula = Some((*node).clone());
            } else if maths.is_empty() && EQUATION_NUMBER.is_match(text.trim()) && number.is_none() {
                number = Some(text.trim().to_string());
            } else {
                return None;
            }
        }
    }
    Some((formula?, number?))
}

/// `text` without control characters other than line breaks: the
/// outputs use some of them as markers.
fn without_controls(text: &str) -> String {
    text.chars().filter(|&ch| ch == '\n' || !ch.is_control()).collect()
}

/// The text of spans.
fn text_of(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// Every span of a block: its text, its rows and its table cells.
fn all_spans(block: &Block) -> impl Iterator<Item = &Span> {
    block.spans.iter().chain(block.rows.iter().flatten()).chain(
        block
            .table
            .iter()
            .flat_map(|table| table.rows.iter().flatten().flat_map(|cell| cell.spans.iter())),
    )
}

/// Every span of a block, to change.
fn all_spans_mut(block: &mut Block) -> Vec<&mut Span> {
    let mut out: Vec<&mut Span> = block.spans.iter_mut().collect();
    out.extend(block.rows.iter_mut().flatten());
    if let Some(table) = &mut block.table {
        out.extend(table.rows.iter_mut().flatten().flat_map(|cell| cell.spans.iter_mut()));
    }
    out
}

/// Spans with neighbours of the same style, colour and link joined, and
/// line breaks kept as `\n`.
fn merge_spans(spans: Vec<Span>) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.text.is_empty() && span.math.is_none() {
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.math.is_none()
            && span.math.is_none()
            && last.style == span.style
            && last.color == span.color
            && last.href == span.href
        {
            last.text.push_str(&span.text);
            continue;
        }
        out.push(span);
    }
    out
}

/// Removes the spaces at both ends of the text of `spans`.
fn trim_spans(spans: &mut Vec<Span>) {
    while let Some(first) = spans.first_mut().filter(|span| span.math.is_none()) {
        let trimmed = first.text.trim_start().to_string();
        if trimmed.is_empty() {
            spans.remove(0);
        } else {
            first.text = trimmed;
            break;
        }
    }
    while let Some(last) = spans.last_mut().filter(|span| span.math.is_none()) {
        let trimmed = last.text.trim_end().to_string();
        if trimmed.is_empty() {
            spans.pop();
        } else {
            last.text = trimmed;
            break;
        }
    }
}

/// Keeps the first `keep` bytes of the text of `spans`.
fn truncate_spans(spans: &mut Vec<Span>, keep: usize) {
    let mut left = keep;
    let mut out = Vec::new();
    for mut span in spans.drain(..) {
        if left == 0 {
            break;
        }
        if span.text.len() > left {
            let mut cut = left;
            while !span.text.is_char_boundary(cut) {
                cut -= 1;
            }
            span.text.truncate(cut);
        }
        left -= span.text.len().min(left);
        out.push(span);
    }
    *spans = out;
    trim_spans(spans);
}

/// The lines of spans that hold line breaks (`\n`), each trimmed.
fn split_lines(spans: Vec<Span>) -> Vec<Vec<Span>> {
    let mut lines = vec![Vec::new()];
    for span in spans {
        if span.math.is_some() || !span.text.contains('\n') {
            lines.last_mut().expect("one line at least").push(span);
            continue;
        }
        for (i, piece) in span.text.split('\n').enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if !piece.is_empty() {
                let mut part = span.clone();
                part.text = piece.to_string();
                lines.last_mut().expect("one line at least").push(part);
            }
        }
    }
    lines.retain(|line| {
        line.iter()
            .any(|span| !span.text.trim().is_empty() || span.math.is_some())
    });
    lines
}

/// Lines put back together into one text, separated by spaces.
fn join_lines(spans: Vec<Span>) -> Vec<Span> {
    spans
        .into_iter()
        .map(|mut span| {
            if span.math.is_none() {
                span.text = span.text.replace('\n', " ");
            }
            span
        })
        .collect()
}

/// A number in lowercase Roman numerals (the marks of endnotes).
fn roman_lower(number: usize) -> String {
    const NUMERALS: &[(usize, &str)] = &[
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut left = number;
    let mut out = String::new();
    for &(value, numeral) in NUMERALS {
        while left >= value {
            out.push_str(numeral);
            left -= value;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_keys_and_labels() {
        assert_eq!(style_key("Heading 2"), "heading2");
        assert_eq!(heading_number("heading2"), Some(2));
        assert_eq!(caption_label("Figure 3: The detector.").as_deref(), Some("Figure 3"));
        assert_eq!(caption_label("Table 1. Results").as_deref(), Some("Table 1"));
        assert_eq!(caption_label("The detector"), None);
        assert_eq!(section_number("2.1 Methods").as_deref(), Some("2.1"));
        assert_eq!(roman_lower(14), "xiv");
    }
}
