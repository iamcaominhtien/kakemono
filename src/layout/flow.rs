//! Pages: the paragraphs and tables of each section placed in its columns
//! and pages, with Word's rules for space between paragraphs, widows and
//! orphans, paragraphs kept together or with the next one, page breaks,
//! headers and footers, footnotes and floating objects.

use super::display::{Destination, Item, Page};
use super::fonts::FontBook;
use super::para::{self, Ctx, Float, FloatKind, Line, ParaLayout};
use super::shape::Shaper;
use crate::word::fmt::model::{
    Align, AnchorAlign, AnchorFrom, Block, Border, BorderStyle, Cell, FDoc, Inline, NumFormat, Para, ParaBorders, Rgb,
    Row, RunFmt, Section, SectionStart, Shape, Table, VAlign, VMerge, Width, Wrap,
};
use std::collections::HashMap;
use std::collections::hash_map::Entry;

/// Space Word puts before and after a paragraph with automatic spacing.
const AUTO_SPACING_PT: f32 = 14.0;
/// Length of the line that separates footnotes from the text.
const NOTE_SEPARATOR_LEN_PT: f32 = 144.0;
/// Width of the separator line.
const NOTE_SEPARATOR_WIDTH_PT: f32 = 0.5;
/// Fewest lines of a paragraph left at the bottom or top of a page with
/// widow control.
const WIDOW_LINES: usize = 2;
/// Smallest room, in points, under which nothing more is placed in a
/// column (rounding).
const EPS: f32 = 0.01;

/// Whether the placed lines are printed (`KAKEMONO_LAYOUT_TRACE`), for
/// calibration against Word's own PDFs.
/// A metafile's line of text is narrowed or widened to the width the
/// metafile gives it when ours differs by more than this share.
const METAFILE_FIT_RATIO: f32 = 0.02;

fn trace() -> bool {
    std::env::var_os("KAKEMONO_LAYOUT_TRACE").is_some()
}

/// The laid-out document.
pub struct Layout {
    /// The pages.
    pub pages: Vec<Page>,
    /// Where the bookmarks land.
    pub destinations: Vec<Destination>,
    /// The page number shown on each bookmark's page, by bookmark (for
    /// `PAGEREF` fields).
    pub page_refs: HashMap<String, String>,
}

/// Lays out a whole document: once, then again with the page numbers of
/// its bookmarks when it refers to them.
pub fn layout(doc: &FDoc, book: &mut FontBook) -> Layout {
    let first = layout_pass(doc, book, None);
    if !refers_to_pages(doc) {
        return first;
    }
    let refs = first.page_refs.clone();
    layout_pass(doc, book, Some(&refs))
}

/// Whether the document has a `PAGEREF` field.
fn refers_to_pages(doc: &FDoc) -> bool {
    let mut found = false;
    for section in &doc.sections {
        walk_inlines(&section.blocks, &mut |inline| {
            found |= matches!(
                inline,
                Inline::Field {
                    kind: crate::word::fmt::model::FieldKind::PageRef(_),
                    ..
                }
            );
        });
    }
    found
}

/// Lays out a whole document once, with the page numbers of bookmarks
/// from an earlier pass.
fn layout_pass(doc: &FDoc, book: &mut FontBook, page_refs: Option<&HashMap<String, String>>) -> Layout {
    let notes = number_notes(doc);
    let mut shaper = Shaper::default();
    let mut coverage = HashMap::new();
    let mut flow = Flow {
        doc,
        book,
        shaper: &mut shaper,
        coverage: &mut coverage,
        notes: &notes,
        page_refs,
        pages: Vec::new(),
        page_info: Vec::new(),
        destinations: Vec::new(),
        behind: HashMap::new(),
        state: State::default(),
    };
    for (at, section) in doc.sections.iter().enumerate() {
        flow.section(at, section);
    }
    flow.endnotes();
    flow.finish_page();
    flow.headers_and_footers();
    flow.metafile_texts();
    let mut refs = HashMap::new();
    for destination in &flow.destinations {
        if let Some(info) = flow.page_info.get(destination.page) {
            let format = &doc.sections[info.section].page_number_format;
            refs.entry(destination.name.clone())
                .or_insert_with(|| crate::word::fmt::model::format_page(info.number, format));
        }
    }
    Layout {
        pages: flow.pages,
        destinations: flow.destinations,
        page_refs: refs,
    }
}

/// The numbers of the notes, in the order the text refers to them.
fn number_notes(doc: &FDoc) -> HashMap<(bool, String), String> {
    let mut numbers = HashMap::new();
    let mut counts = [0u32; 2];
    let mut visit = |inline: &Inline| {
        if let Inline::NoteRef { endnote, id, .. } = inline {
            let key = (*endnote, id.clone());
            if let Entry::Vacant(slot_entry) = numbers.entry(key) {
                let slot = usize::from(*endnote);
                counts[slot] += 1;
                let format = if *endnote {
                    &doc.endnote_format
                } else {
                    &doc.footnote_format
                };
                slot_entry.insert(crate::word::fmt::model::format_page(counts[slot], format));
            }
        }
    };
    for section in &doc.sections {
        walk_inlines(&section.blocks, &mut visit);
    }
    numbers
}

/// Calls `visit` on every inline of `blocks`, in order.
fn walk_inlines(blocks: &[Block], visit: &mut dyn FnMut(&Inline)) {
    for block in blocks {
        match block {
            Block::Para(para) => {
                for inline in &para.inlines {
                    visit(inline);
                    if let Inline::Shape(shape) = inline {
                        walk_inlines(&shape.blocks, visit);
                    }
                }
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        walk_inlines(&cell.blocks, visit);
                    }
                }
            }
        }
    }
}

/// Per page: its section, its number, whether it is the section's first.
#[derive(Debug, Clone)]
struct PageInfo {
    section: usize,
    number: u32,
    first_of_section: bool,
}

/// Where the flow is.
#[derive(Debug, Clone, Default)]
struct State {
    /// The page being filled exists.
    open: bool,
    /// Index of the section being placed.
    section: usize,
    /// Its column being filled.
    column: usize,
    /// Left edge and width of each column of the current section.
    columns: Vec<(f32, f32)>,
    /// Top of the body area.
    top: f32,
    /// Bottom of the body area.
    bottom: f32,
    /// Next free position in the column.
    y: f32,
    /// Nothing has been placed in the column yet.
    fresh: bool,
    /// The column was started because the paragraph placed next did not
    /// fit in the previous one (not by a break): Word drops its space
    /// before.
    pushed: bool,
    /// The column is the first of a section's first page.
    section_top: bool,
    /// The space after the last paragraph of the section before.
    section_after: f32,
    /// The paragraph being placed is an empty one that ends a section
    /// before a new page: it stays on its page even when it does not fit.
    section_mark: bool,
    /// The paragraph being placed is the last of a section followed by a
    /// new page: a page break on its last line opens no page.
    break_ends_section: bool,
    /// The paragraph placed last ended with a page or column break.
    after_break: bool,
    /// Space after the last paragraph, with its style and whether it is
    /// contextual.
    pending_after: Option<(f32, String, bool)>,
    /// Footnotes on the page: their lines, in order.
    page_notes: Vec<Vec<Line>>,
    /// Height of the footnote area (with its separator).
    notes_height: f32,
    /// Number of the page being filled.
    number: u32,
    /// Areas text flows around on this page: (left, top, right, bottom),
    /// and whether text stays only above and below.
    exclusions: Vec<([f32; 4], bool)>,
    /// Style of the last paragraph placed.
    last_style: Option<String>,
    /// The text continues below this position (a floating object across
    /// the column).
    pending_push: Option<f32>,
    /// Space after the previous paragraph and before this one, for the
    /// trace.
    trace_spacing: (f32, f32),
    /// Borders and indents of the last paragraph placed: paragraphs with the
    /// same ones are framed together.
    last_borders: Option<(ParaBorders, f32, f32)>,
    /// Height the bottom border of the last paragraph adds below it, unless
    /// the next paragraph is in the same frame.
    pending_border: f32,
}

/// The layout of the body.
struct Flow<'a> {
    doc: &'a FDoc,
    book: &'a mut FontBook,
    shaper: &'a mut Shaper,
    coverage: &'a mut HashMap<(super::fonts::FaceId, char), bool>,
    notes: &'a HashMap<(bool, String), String>,
    page_refs: Option<&'a HashMap<String, String>>,
    pages: Vec<Page>,
    page_info: Vec<PageInfo>,
    destinations: Vec<Destination>,
    /// The number of items drawn behind the text at the start of each page,
    /// by page.
    behind: HashMap<usize, usize>,
    state: State,
}

impl Flow<'_> {
    /// The layout context.
    fn ctx(&mut self) -> Ctx<'_> {
        let origin = self.column_left();
        Ctx {
            book: self.book,
            shaper: self.shaper,
            doc: self.doc,
            notes: self.notes,
            page_refs: self.page_refs,
            own_note: None,
            page: None,
            page_count: None,
            section_pages: None,
            coverage: self.coverage,
            origin,
        }
    }

    /// Places a section.
    fn section(&mut self, at: usize, section: &Section) {
        // A continuous section on a page of another size starts a new page.
        let same_size = at > 0 && {
            let before = &self.doc.sections[self.state.section].page;
            (before.width - section.page.width).abs() < EPS && (before.height - section.page.height).abs() < EPS
        };
        let continuous = section.start == SectionStart::Continuous && self.state.open && same_size;
        if trace() {
            eprintln!("SECTION\t{at}\t{:?}\tpage {}", section.start, self.pages.len());
        }
        let previous_number = self.state.number;
        self.state.section_after = self.state.pending_after.as_ref().map_or(0.0, |(after, _, _)| *after);
        if !continuous {
            self.finish_page();
            if matches!(section.start, SectionStart::OddPage | SectionStart::EvenPage) {
                let next = section.page_number_start.unwrap_or(previous_number + 1);
                let want_odd = section.start == SectionStart::OddPage;
                if at > 0 && (next % 2 == 1) != want_odd {
                    // A blank page keeps the parity.
                    self.start_page(at, section, next.saturating_sub(1), true);
                    self.finish_page();
                }
            }
        }
        self.state.section = at;
        let page = &section.page;
        let left = page.left + page.gutter;
        let width = (page.width - page.left - page.right - page.gutter).max(1.0);
        self.state.columns = column_boxes(section, left, width);
        if continuous {
            // The columns of the new section start below what is there.
            self.state.column = 0;
            let y = self.state.y;
            self.state.fresh = false;
            let _ = y;
        } else {
            let number = section.page_number_start.unwrap_or(previous_number + 1);
            self.start_page(at, section, number, true);
        }
        // The empty paragraph that ends a section followed by a new page
        // holds only the section break: Word keeps it on its page.
        let next_page = self
            .doc
            .sections
            .get(at + 1)
            .is_some_and(|next| next.start != SectionStart::Continuous);
        // Empty paragraphs that end the document open no page: Word leaves
        // them below the last page.
        let last_section = at + 1 == self.doc.sections.len();
        let trailing = if last_section {
            section
                .blocks
                .iter()
                .rev()
                .take_while(|block| matches!(block, Block::Para(para) if para.inlines.is_empty()))
                .count()
        } else {
            0
        };
        if trailing > 0 && trailing < section.blocks.len() {
            let split = section.blocks.len() - trailing;
            self.blocks(&section.blocks[..split]);
            self.state.section_mark = true;
            self.blocks(&section.blocks[split..]);
            self.state.section_mark = false;
            return;
        }
        match section.blocks.split_last() {
            Some((Block::Para(last), rest)) if next_page && last.inlines.is_empty() => {
                self.blocks(rest);
                self.state.section_mark = true;
                self.blocks(std::slice::from_ref(&section.blocks[section.blocks.len() - 1]));
                self.state.section_mark = false;
            }
            Some((Block::Para(_), rest)) if next_page => {
                // A page break on the last line of the section's last
                // paragraph: the new section starts the new page.
                self.blocks(rest);
                self.state.break_ends_section = true;
                self.blocks(&section.blocks[section.blocks.len() - 1..]);
                self.state.break_ends_section = false;
            }
            Some((Block::Para(last), rest))
                if at + 1 < self.doc.sections.len() && last.inlines.is_empty() && !rest.is_empty() =>
            {
                // The empty paragraph that holds a continuous section break
                // takes no room.
                self.blocks(rest);
            }
            _ => self.blocks(&section.blocks),
        }
    }

    /// Opens a new page of section `at`, numbered `number`.
    fn start_page(&mut self, at: usize, section: &Section, number: u32, first_of_section: bool) {
        let page = &section.page;
        self.pages.push(Page {
            width: page.width,
            height: page.height,
            items: Vec::new(),
        });
        self.page_info.push(PageInfo {
            section: at,
            number,
            first_of_section,
        });
        self.state.open = true;
        self.state.number = number;
        self.state.column = 0;
        let (header_bottom, footer_top) = self.header_extent(at, number, first_of_section);
        self.state.top = page.top.abs().max(header_bottom);
        self.state.bottom = (page.height - page.bottom.abs()).min(footer_top);
        self.state.y = self.state.top;
        self.state.fresh = true;
        self.state.pushed = false;
        self.state.section_top = first_of_section;
        self.state.pending_after = None;
        self.state.page_notes.clear();
        self.state.notes_height = 0.0;
        self.state.exclusions.clear();
        self.state.pending_push = None;
    }

    /// Closes the current page: draws its footnotes.
    fn finish_page(&mut self) {
        if !self.state.open {
            return;
        }
        self.state.open = false;
        if self.state.page_notes.is_empty() {
            return;
        }
        let (x, _) = self.state.columns.first().copied().unwrap_or((72.0, 468.0));
        let mut y = self.state.bottom - self.state.notes_height;
        let separator = self.separator_height();
        let page = self.pages.len() - 1;
        let rule_y = y + separator / 2.0;
        self.pages[page].items.push(Item::Line {
            x1: x,
            y1: rule_y,
            x2: x + NOTE_SEPARATOR_LEN_PT,
            y2: rule_y,
            width: NOTE_SEPARATOR_WIDTH_PT,
            color: [0, 0, 0],
            dash: None,
        });
        y += separator;
        let notes = std::mem::take(&mut self.state.page_notes);
        for lines in notes {
            for line in lines {
                push_items(&mut self.pages[page].items, &line.items, x, y);
                y += line.height;
            }
        }
    }

    /// Height of the footnote separator: a single line of the normal font.
    fn separator_height(&mut self) -> f32 {
        if let Some(blocks) = &self.doc.footnote_separator {
            return self.blocks_height(blocks, 100.0, None);
        }
        let fmt = self
            .doc
            .footnotes
            .first()
            .and_then(|(_, blocks)| match blocks.first() {
                Some(Block::Para(para)) => Some(para.mark.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let para = Para {
            mark: fmt,
            ..Default::default()
        };
        let layout = para::layout(&mut self.ctx(), &para, 100.0);
        layout.lines.first().map_or(12.0, |line| line.height)
    }

    /// Bottom of the header and top of the footer of a page, laid out with
    /// a provisional page number.
    fn header_extent(&mut self, section: usize, number: u32, first: bool) -> (f32, f32) {
        let sect = &self.doc.sections[section];
        let page = sect.page;
        let width = (page.width - page.left - page.right - page.gutter).max(1.0);
        let header = self.header_part(section, number, first, true);
        let footer = self.header_part(section, number, first, false);
        let mut header_bottom = 0.0;
        // A header or footer that shows nothing (empty paragraphs, objects
        // floating apart) does not push the body.
        if let Some(part) = header.filter(|&part| shows_something(&self.doc.parts[part])) {
            let height = self.blocks_height(&self.doc.parts[part], width, Some(number));
            // The space after the header's last paragraph is part of it.
            let after = match self.doc.parts[part].last() {
                Some(Block::Para(para)) if para.props.after_auto => AUTO_SPACING_PT,
                Some(Block::Para(para)) => para.props.after,
                _ => 0.0,
            };
            header_bottom = page.header + height + after;
        }
        let mut footer_top = page.height;
        if let Some(part) = footer.filter(|&part| shows_something(&self.doc.parts[part])) {
            let height = self.blocks_height(&self.doc.parts[part], width, Some(number));
            footer_top = page.height - page.footer - height;
        }
        (header_bottom, footer_top)
    }

    /// Draws the lines of text of the metafile pictures placed on every
    /// page as text over them, as Word's PDFs do (the picture drawn is
    /// the one without them).
    fn metafile_texts(&mut self) {
        for at in 0..self.pages.len() {
            let placed: Vec<(usize, [f32; 4], [f32; 4])> = self.pages[at]
                .items
                .iter()
                .filter_map(|item| match *item {
                    Item::Image {
                        index,
                        x,
                        y,
                        width,
                        height,
                        crop,
                    } if self.doc.images.get(index).is_some_and(|image| image.bare.is_some()) => {
                        Some((index, [x, y, width, height], crop))
                    }
                    _ => None,
                })
                .collect();
            for (index, frame, crop) in placed {
                let items = self.metafile_items(index, frame, crop);
                self.pages[at].items.extend(items);
            }
        }
    }

    /// The text items of the metafile `index` drawn in `frame` (x, y,
    /// width, height) with `crop` cut off its sides.
    fn metafile_items(&mut self, index: usize, frame: [f32; 4], crop: [f32; 4]) -> Vec<Item> {
        let [x, y, width, height] = frame;
        let [left, top, right, bottom] = crop;
        let shown_width = (1.0 - left - right).max(EPS);
        let shown_height = (1.0 - top - bottom).max(EPS);
        let doc = self.doc;
        let mut out = Vec::new();
        for text in &doc.images[index].texts {
            let fx = (text.x - left) / shown_width;
            let fy = (text.y - top) / shown_height;
            if !(0.0..=1.0).contains(&fx) || !(0.0..=1.0).contains(&fy) {
                continue;
            }
            let size = text.size / shown_height * height;
            if size <= EPS {
                continue;
            }
            let fmt = RunFmt {
                font: text.face.clone(),
                font_hansi: text.face.clone(),
                font_cs: text.face.clone(),
                size,
                size_cs: size,
                bold: text.bold,
                italic: text.italic,
                color: Some(text.color),
                ..RunFmt::default()
            };
            let (px, py) = (x + fx * width, y + fy * height);
            let mut ctx = self.ctx();
            let (items, drawn) = para::text_items(&mut ctx, &text.text, &fmt, px, py);
            // As wide as the metafile sets it, within reason.
            let wanted = (text.end_x - text.x) / shown_width * width;
            let ratio = wanted / drawn.max(EPS);
            if drawn > EPS && wanted > EPS && (ratio - 1.0).abs() > METAFILE_FIT_RATIO && ratio > 0.5 && ratio < 2.0 {
                let scaled = RunFmt {
                    scale: 100.0 * ratio,
                    ..fmt
                };
                out.extend(para::text_items(&mut ctx, &text.text, &scaled, px, py).0);
            } else {
                out.extend(items);
            }
        }
        out
    }

    /// The header (or footer) part of a page.
    fn header_part(&self, section: usize, number: u32, first: bool, header: bool) -> Option<usize> {
        let sect = &self.doc.sections[section];
        let set = if header { sect.headers } else { sect.footers };
        if first && sect.title_page {
            return set.first;
        }
        if self.doc.even_and_odd_headers && number.is_multiple_of(2) {
            return set.even;
        }
        set.default
    }

    /// Height of blocks laid out at `width` (headers, footers, cells).
    fn blocks_height(&mut self, blocks: &[Block], width: f32, page: Option<u32>) -> f32 {
        let mut sub = SubFlow::new(width);
        self.sub_blocks(&mut sub, blocks, page, None);
        sub.y
    }

    /// Draws the headers and footers of every page, now that the page
    /// count is known.
    fn headers_and_footers(&mut self) {
        let count = self.pages.len() as u32;
        for at in 0..self.pages.len() {
            let info = self.page_info[at].clone();
            let sect = &self.doc.sections[info.section];
            let page = sect.page;
            let left = page.left + page.gutter;
            let width = (page.width - page.left - page.right - page.gutter).max(1.0);
            let section_pages = self
                .page_info
                .iter()
                .filter(|other| other.section == info.section)
                .count() as u32;
            let format = sect.page_number_format.clone();
            for header in [true, false] {
                let Some(part) = self.header_part(info.section, info.number, info.first_of_section, header) else {
                    continue;
                };
                let mut sub = SubFlow::new(width);
                sub.page_count = Some(count);
                sub.section_pages = Some(section_pages);
                sub.format = format.clone();
                let blocks = &self.doc.parts[part];
                self.sub_blocks(&mut sub, blocks, Some(info.number), None);
                let y = if header {
                    page.header
                } else {
                    page.height - page.footer - sub.y
                };
                let mut items = Vec::new();
                push_items(&mut items, &sub.items, left, y);
                self.put_sub_floats(&sub.floats, info.section, at, left, y, width);
                // Headers are drawn behind the body.
                let body = std::mem::take(&mut self.pages[at].items);
                self.pages[at].items = items;
                self.pages[at].items.extend(body);
            }
        }
    }

    /// Places blocks in the body.
    fn blocks(&mut self, blocks: &[Block]) {
        // The current chain of paragraphs kept with the next was moved to a
        // new column: its later paragraphs stay with it.
        let mut chain_moved = false;
        for block in blocks.iter().enumerate() {
            let (at, block) = block;
            match block {
                Block::Para(para) => {
                    // A paragraph kept with the next: the chain must start on
                    // a page where it fits (checked again at each of its
                    // paragraphs, as long as it was not moved). One holding a
                    // page break, or with a page break before it, starts the
                    // new page itself.
                    if para.props.keep_next
                        && !chain_moved
                        && !self.state.fresh
                        && !self.state.section_mark
                        && !breaks_page(para)
                        && !para.props.page_break_before
                    {
                        let need = self.chain_height(&blocks[at..]);
                        let room = self.state.bottom - self.state.notes_height - self.state.y;
                        let page_room = self.state.bottom - self.state.top;
                        if need > room + EPS && need <= page_room {
                            self.next_column();
                            self.state.pushed = true;
                            chain_moved = true;
                        }
                    }
                    if !para.props.keep_next {
                        chain_moved = false;
                    }
                    self.paragraph(para);
                }
                Block::Table(table) => {
                    chain_moved = false;
                    self.table(table);
                }
            }
        }
    }

    /// Height of a chain of paragraphs kept with the next, with the first
    /// two lines of the paragraph that ends it.
    fn chain_height(&mut self, blocks: &[Block]) -> f32 {
        let width = self.column_width();
        let mut height = 0.0;
        for block in blocks {
            match block {
                Block::Para(para) => {
                    let layout = para::layout(&mut self.ctx(), para, width);
                    let before = self.before_space(para);
                    if breaks_page(para) {
                        // The chain ends at a page break: what comes before
                        // it stays on this page.
                        height += before + layout.lines.first().map_or(0.0, |line| line.height);
                        break;
                    }
                    if para.props.keep_next {
                        height += before + layout.lines.iter().map(|line| line.height).sum::<f32>() + para.props.after;
                    } else {
                        height += before
                            + layout
                                .lines
                                .iter()
                                .take(WIDOW_LINES)
                                .map(|line| line.height)
                                .sum::<f32>();
                        break;
                    }
                }
                Block::Table(_) => break,
            }
        }
        height
    }

    /// Width of the current column.
    fn column_width(&self) -> f32 {
        self.state
            .columns
            .get(self.state.column)
            .map_or(468.0, |&(_, width)| width)
    }

    /// Where a line `height` high at `top` in the current column must go
    /// instead, below the objects of the page wrapped top and bottom; or
    /// `None` when it meets none.
    fn below_exclusions(&self, top: f32, height: f32) -> Option<f32> {
        let left = self.column_left();
        let right = left + self.column_width();
        let mut y = top;
        let mut moved = false;
        // Moving below one object can run into another.
        for _ in 0..self.state.exclusions.len() {
            let hit = self.state.exclusions.iter().find(|&&([x0, y0, x1, y1], top_bottom)| {
                top_bottom && x0 < right && x1 > left && y0 < y + height && y1 > y + EPS
            });
            match hit {
                Some(&([_, _, _, bottom], _)) => {
                    y = bottom;
                    moved = true;
                }
                None => break,
            }
        }
        moved.then_some(y)
    }

    /// Left edge of the current column.
    fn column_left(&self) -> f32 {
        self.state
            .columns
            .get(self.state.column)
            .map_or(72.0, |&(left, _)| left)
    }

    /// Moves to the next column, or the next page.
    fn next_column(&mut self) {
        if self.state.column + 1 < self.state.columns.len() {
            self.state.column += 1;
            self.state.y = self.state.top;
            self.state.fresh = true;
            self.state.pushed = false;
            self.state.section_top = false;
            self.state.pending_after = None;
            return;
        }
        self.next_page();
    }

    /// Moves to the next page of the current section.
    fn next_page(&mut self) {
        self.finish_page();
        let at = self.state.section;
        let number = self.state.number + 1;
        let section = &self.doc.sections[at];
        self.start_page(at, section, number, false);
    }

    /// The space before a paragraph, given what precedes it.
    fn before_space(&self, para: &Para) -> f32 {
        let before = if para.props.before_auto {
            AUTO_SPACING_PT
        } else {
            para.props.before
        };
        match &self.state.pending_after {
            Some((_, style, _)) if para.props.contextual && *style == para.style => 0.0,
            _ => before,
        }
    }

    /// Places a paragraph.
    fn paragraph(&mut self, para: &Para) {
        // The empty paragraph that holds a section break stays on its page.
        if para.props.page_break_before && !self.state.fresh && !self.state.section_mark {
            self.next_page();
        }
        let width = self.column_width();
        let layout = para::layout(&mut self.ctx(), para, width);
        let after_prev = match self.state.pending_after.take() {
            Some((after, style, contextual)) if !(contextual && style == para.style) => after,
            _ => 0.0,
        };
        let before = if para.props.before_auto {
            AUTO_SPACING_PT
        } else {
            para.props.before
        };
        // At the top of a section that starts a new page, the space before
        // is what it adds to the space after the last paragraph of the
        // section before (spent on the previous page): with the larger of
        // the two counted, none when that space after is as large.
        // At the top of a page opened by a break that ends the paragraph
        // before, Word drops the space before.
        let before = if self.state.fresh && self.state.after_break {
            0.0
        } else {
            before
        };
        let before = if self.state.fresh && self.state.section_top {
            let after = self.state.section_after;
            (self.spacing(after, before) - after).max(0.0)
        } else {
            before
        };
        let before = if para.props.contextual && self.state_style_matches(para) || self.state.fresh && self.state.pushed
        {
            0.0
        } else {
            before
        };
        self.state.trace_spacing = (after_prev, before);
        let mut gap = self.spacing(after_prev, before);
        // Borders: the top border (and its distance) is above the first
        // paragraph of a frame, the bottom one below the last.
        let borders = para.props.borders;
        let key = (borders, para.props.left, para.props.right);
        let framed = borders.top.is_some() || borders.bottom.is_some() || borders.between.is_some();
        let same_frame = framed && self.state.last_borders == Some(key);
        let pending_border = std::mem::take(&mut self.state.pending_border);
        if !same_frame {
            gap += pending_border;
            if let Some(top) = borders.top {
                gap += top.space + top.width;
            }
        } else if let Some(between) = borders.between {
            // A rule between two paragraphs of the group, with its space on
            // both sides.
            let y = self.state.y + gap + between.space + between.width / 2.0;
            let left = self.column_left() + para.props.left;
            let right = self.column_left() + self.column_width() - para.props.right;
            let mut rule = Vec::new();
            border_line(&mut rule, between, left, y, right, y);
            let page = self.pages.len() - 1;
            self.pages[page].items.extend(rule);
            gap += 2.0 * between.space + between.width;
        }
        self.state.last_borders = framed.then_some(key);
        self.state.pending_border = borders.bottom.map_or(0.0, |bottom| bottom.space + bottom.width);
        if let Some(push) = self.state.pending_push.take()
            && push > self.state.y
        {
            gap += push - self.state.y;
        }
        // A first line that would run into an object text does not flow
        // beside starts below it, after its space before.
        let first_height = layout.lines.first().map_or(0.0, |line| line.height);
        if let Some(below) = self.below_exclusions(self.state.y + gap, first_height) {
            gap = below + before - self.state.y;
        }
        // A paragraph that ends with a page or column break leaves its
        // space after on the page it ends: the next column starts at its top.
        let ends_with_break = layout
            .lines
            .last()
            .is_some_and(|line| line.page_break || line.column_break);
        self.place_lines(para, layout, gap);
        let after = if para.props.after_auto {
            AUTO_SPACING_PT
        } else {
            para.props.after
        };
        self.state.pending_after = (!ends_with_break).then(|| (after, para.style.clone(), para.props.contextual));
        self.state.after_break = ends_with_break;
    }

    /// The space between two paragraphs: the larger of the space after the
    /// first and before the second, or their sum in documents that ask for
    /// it.
    fn spacing(&self, after: f32, before: f32) -> f32 {
        if self.doc.add_paragraph_spacing {
            after + before
        } else {
            after.max(before)
        }
    }

    /// How many of `lines`, from `at`, fit in the rest of the column after a
    /// space of `gap`, with the notes they refer to; and the end of the
    /// chunk they belong to (a page or column break ends a chunk). Returns
    /// (lines that fit, chunk end).
    fn lines_that_fit(&mut self, lines: &[Line], at: usize, gap: f32) -> (usize, usize) {
        let room_bottom = self.state.bottom - self.state.notes_height;
        let mut y = self.state.y + gap;
        let mut fit = 0;
        let mut notes_extra = 0.0;
        let chunk_end = lines[at..]
            .iter()
            .position(|line| line.page_break || line.column_break)
            .map_or(lines.len(), |offset| at + offset + 1);
        if self.state.section_mark {
            return (chunk_end - at, chunk_end);
        }
        for line in &lines[at..chunk_end] {
            let extra = self.notes_height_for(line);
            if y + line.fit_height > room_bottom - notes_extra - extra + EPS {
                break;
            }
            notes_extra += extra;
            y += line.height;
            fit += 1;
        }
        (fit, chunk_end)
    }

    /// How many of the `fit` lines that fit (of `remaining` in the chunk,
    /// from line `at`) go in this column, after the paragraph's
    /// keep-lines-together and widow and orphan rules. 0 moves the lines to
    /// the next column; a fresh column takes at least one line.
    fn lines_to_take(&self, para: &Para, at: usize, fit: usize, remaining: usize) -> usize {
        let mut take = fit;
        if take < remaining {
            if para.props.keep_lines && at == 0 && !self.state.fresh {
                take = 0;
            } else if para.props.widow_control {
                if take == remaining - 1 && remaining > WIDOW_LINES {
                    take -= 1;
                }
                if take < WIDOW_LINES && (at == 0 && remaining > take) && remaining >= WIDOW_LINES && !self.state.fresh
                {
                    take = 0;
                }
            }
        }
        if take == 0 && self.state.fresh {
            take = fit.max(1);
        }
        take
    }

    /// Places one line of `para` at (`x`, `y`) on page `page`: its items,
    /// bookmarks, inline shapes and notes. `first` is whether it is the
    /// paragraph's first line.
    fn place_line(&mut self, para: &Para, line: &Line, first: bool, page: usize, x: f32, y: f32) {
        for bookmark in &line.bookmarks {
            self.destinations.push(Destination {
                name: bookmark.clone(),
                page,
                y,
            });
        }
        push_items(&mut self.pages[page].items, &line.items, x, y);
        if trace() {
            self.trace_line(para, line, first, page, y);
        }
        for (shape_x, shape) in &line.shapes {
            let top = y + line.baseline - shape.height;
            let page = self.pages.len() - 1;
            self.draw_shape(shape, page, x + shape_x, top, shape.width);
        }
        self.add_notes(line);
    }

    /// Prints a placed line for debugging (`trace()`).
    fn trace_line(&self, para: &Para, line: &Line, first: bool, page: usize, y: f32) {
        let text: String = line
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Text(run) => Some(run.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        let (after_prev, before) = if first { self.state.trace_spacing } else { (0.0, 0.0) };
        eprintln!(
            "TRACE\t{page}\t{y:.2}\t{:.2}\t{:.2}\t{:?}\t{}\t{}\t{after_prev:.2}\t{before:.2}\t{:.2},{:.2},{:.2}\t{}",
            line.baseline,
            line.height,
            para.props.line,
            para.style,
            first,
            line.metrics.0,
            line.metrics.1,
            line.metrics.2,
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }

    /// Whether the previous paragraph has the same style as `para`.
    fn state_style_matches(&self, para: &Para) -> bool {
        self.state.last_style.as_deref() == Some(para.style.as_str())
    }

    /// Places the lines of a paragraph, breaking columns and pages where
    /// they do not fit.
    fn place_lines(&mut self, para: &Para, layout: ParaLayout, gap: f32) {
        self.state.last_style = Some(para.style.clone());
        let lines = layout.lines;
        let mut gap = gap;
        if self.state.section_mark {
            gap = 0.0;
        }
        let count = lines.len();
        let mut at = 0;
        let borders = para.props.borders;
        let shading = para.props.shading;
        // A paragraph that starts with a page break: the break's empty line
        // ends the page it is on, and the space before goes with the text
        // after it, on the new page, for what it adds to the space after
        // the paragraph before (as at the top of a section).
        if count > 1 && lines[0].page_break && lines[0].items.is_empty() && !self.state.fresh {
            let page = self.pages.len() - 1;
            let x = self.column_left();
            self.place_line(para, &lines[0], true, page, x, self.state.y);
            self.next_page();
            let (after, before) = self.state.trace_spacing;
            gap = (self.spacing(after, before) - after).max(0.0);
            at = 1;
        }
        while at < count {
            let (fit, chunk_end) = self.lines_that_fit(&lines, at, gap);
            let take = self.lines_to_take(para, at, fit, chunk_end - at);
            if take == 0 {
                self.next_column();
                gap = if at == 0 { self.top_gap(para) } else { 0.0 };
                continue;
            }
            let x = self.column_left();
            let start_y = self.state.y + gap;
            let mut y = start_y;
            let page = self.pages.len() - 1;
            for line in &lines[at..at + take] {
                let first = std::ptr::eq(line, &lines[0]);
                self.place_line(para, line, first, page, x, y);
                y += line.height;
            }
            // Floats anchored on the placed lines.
            for float in layout
                .floats
                .iter()
                .filter(|float| float.line >= at && float.line < at + take)
            {
                let line_y = start_y
                    + lines[at..float.line.min(at + take)]
                        .iter()
                        .map(|line| line.height)
                        .sum::<f32>();
                // A paragraph's top, for what is positioned from it, is above
                // its space before.
                let para_top = if at == 0 { start_y - gap } else { start_y };
                self.place_float(float, x, para_top, line_y);
            }
            // Borders and shading around the placed lines.
            self.para_frame(para, borders, shading, x, start_y, y, at == 0, at + take == count);
            self.state.y = y;
            self.state.fresh = false;
            self.state.pushed = false;
            at += take;
            gap = 0.0;
            match lines.get(at - 1) {
                Some(last) if last.page_break && self.state.break_ends_section && at == count => {}
                Some(last) if last.page_break => self.next_page(),
                Some(last) if last.column_break => self.next_column(),
                _ if at < count => self.next_column(),
                _ => {}
            }
        }
    }

    /// The gap above a paragraph pushed to a new column or page: Word drops
    /// its space before.
    fn top_gap(&self, _: &Para) -> f32 {
        0.0
    }

    /// Extra footnote height a line needs on this page.
    fn notes_height_for(&mut self, line: &Line) -> f32 {
        let mut height = 0.0;
        for (endnote, id) in &line.notes {
            if *endnote {
                continue;
            }
            if self.state.page_notes.is_empty() && height == 0.0 {
                height += self.separator_height();
            }
            height += self.note_lines(id).iter().map(|line| line.height).sum::<f32>();
        }
        height
    }

    /// The lines of footnote `id`.
    fn note_lines(&mut self, id: &str) -> Vec<Line> {
        let Some((_, blocks)) = self.doc.footnotes.iter().find(|(note_id, _)| note_id == id) else {
            return Vec::new();
        };
        let width = self.state.columns.first().map_or(468.0, |&(_, width)| width);
        let own = self.notes.get(&(false, id.to_string())).cloned();
        let mut sub = SubFlow::new(width);
        sub.own_note = own;
        self.sub_blocks(&mut sub, blocks, Some(self.state.number), None);
        vec![Line {
            height: sub.y,
            baseline: 0.0,
            items: sub.items,
            ..Default::default()
        }]
    }

    /// Adds the footnotes of a placed line to the page.
    fn add_notes(&mut self, line: &Line) {
        for (endnote, id) in &line.notes {
            if *endnote {
                continue;
            }
            if self.state.page_notes.is_empty() {
                self.state.notes_height += self.separator_height();
            }
            let lines = self.note_lines(id);
            self.state.notes_height += lines.iter().map(|line| line.height).sum::<f32>();
            self.state.page_notes.push(lines);
        }
    }

    /// Draws the borders and shading of the part of a paragraph between
    /// `top` and `bottom`.
    #[allow(clippy::too_many_arguments)]
    fn para_frame(
        &mut self,
        para: &Para,
        borders: ParaBorders,
        shading: Option<Rgb>,
        x: f32,
        top: f32,
        bottom: f32,
        first: bool,
        last: bool,
    ) {
        let width = self.column_width();
        let left = x + para.props.left;
        let right = x + width - para.props.right;
        let page = self.pages.len() - 1;
        let mut back = Vec::new();
        if let Some(color) = shading {
            back.push(Item::Rect {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
                color,
            });
        }
        let mut lines = Vec::new();
        if first && let Some(border) = borders.top {
            border_line(&mut lines, border, left, top - border.space, right, top - border.space);
        }
        if last && let Some(border) = borders.bottom {
            border_line(
                &mut lines,
                border,
                left,
                bottom + border.space,
                right,
                bottom + border.space,
            );
        }
        if let Some(border) = borders.left {
            border_line(
                &mut lines,
                border,
                left - border.space,
                top,
                left - border.space,
                bottom,
            );
        }
        if let Some(border) = borders.right {
            border_line(
                &mut lines,
                border,
                right + border.space,
                top,
                right + border.space,
                bottom,
            );
        }
        if !back.is_empty() {
            let items = &mut self.pages[page].items;
            // Shading goes behind the paragraph's text, placed just before.
            let insert_at = items.len().saturating_sub(0);
            let _ = insert_at;
            let mut rest = std::mem::take(items);
            let keep = rest.len();
            let _ = keep;
            items.append(&mut back);
            items.append(&mut rest);
        }
        self.pages[page].items.extend(lines);
    }

    /// Places a floating object anchored in a paragraph.
    fn place_float(&mut self, float: &Float, column_x: f32, para_top: f32, line_top: f32) {
        let width = self.column_width();
        let page_at = self.pages.len() - 1;
        self.put_float(
            float,
            self.state.section,
            page_at,
            (column_x, width),
            (para_top, line_top),
            true,
        );
    }

    /// Draws a floating object on page `page_at` of `section`, in a column
    /// at (x, width), anchored to a paragraph and line at (top, top);
    /// `wrap`: text of the body flows around it.
    fn put_float(
        &mut self,
        float: &Float,
        section: usize,
        page_at: usize,
        (column_x, width): (f32, f32),
        (para_top, line_top): (f32, f32),
        wrap: bool,
    ) {
        let page = self.doc.sections[section].page;
        let anchor = &float.anchor;
        let h_area = match anchor.horizontal.from {
            AnchorFrom::Page => (0.0, page.width),
            AnchorFrom::Margin => (page.left, page.width - page.right),
            AnchorFrom::Column => (column_x, column_x + width),
            AnchorFrom::Character => (column_x + float.x, column_x + float.x),
            AnchorFrom::StartMargin | AnchorFrom::InsideMargin => (0.0, page.left),
            AnchorFrom::EndMargin | AnchorFrom::OutsideMargin => (page.width - page.right, page.width),
            _ => (column_x, column_x + width),
        };
        let v_area = match anchor.vertical.from {
            AnchorFrom::Page => (0.0, page.height),
            AnchorFrom::Margin => (page.top, page.height - page.bottom),
            AnchorFrom::Paragraph => (para_top, para_top),
            AnchorFrom::Line => (line_top, line_top),
            AnchorFrom::StartMargin => (0.0, page.top),
            AnchorFrom::EndMargin => (page.height - page.bottom, page.height),
            _ => (para_top, para_top),
        };
        let place = |area: (f32, f32), align: Option<AnchorAlign>, offset: f32, size: f32| match align {
            Some(AnchorAlign::Center) => (area.0 + area.1 - size) / 2.0,
            Some(AnchorAlign::End | AnchorAlign::Outside) => area.1 - size,
            Some(AnchorAlign::Start | AnchorAlign::Inside) => area.0,
            None => area.0 + offset,
        };
        // A shape sized by its text (a frame) is measured first.
        let (float_width, float_height) = match &float.kind {
            FloatKind::Shape(shape) => self.shape_size(shape, width, page_at),
            FloatKind::Image(..) => (float.width, float.height),
        };
        let x = place(h_area, anchor.horizontal.align, anchor.horizontal.offset, float_width);
        let mut y = place(v_area, anchor.vertical.align, anchor.vertical.offset, float_height);
        // An object is kept on its page.
        if y + float_height > page.height {
            y = (page.height - float_height).max(0.0);
        }
        let y = y.max(0.0);
        match &float.kind {
            FloatKind::Image(index, crop) => {
                if let Some(index) = index {
                    let item = Item::Image {
                        index: *index,
                        x,
                        y,
                        width: float.width,
                        height: float.height,
                        crop: *crop,
                    };
                    let start = self.pages[page_at].items.len();
                    self.pages[page_at].items.push(item);
                    if anchor.behind {
                        self.move_behind(page_at, start);
                    }
                }
            }
            FloatKind::Shape(shape) => {
                let start = self.pages[page_at].items.len();
                self.draw_shape(shape, page_at, x, y, float_width);
                if anchor.behind {
                    self.move_behind(page_at, start);
                }
            }
        }
        if wrap && matches!(anchor.wrap, Wrap::Square | Wrap::Tight | Wrap::TopBottom) {
            let [top, bottom, left, right] = anchor.dist;
            self.state.exclusions.push((
                [x - left, y - top, x + float_width + right, y + float_height + bottom],
                anchor.wrap == Wrap::TopBottom,
            ));
            // Text below the object in the same column moves past it when
            // the object covers the whole column.
            let column_left = column_x;
            let column_right = column_x + width;
            let covers = x - left <= column_left + 1.0 && x + float_width + right >= column_right - 1.0;
            if (anchor.wrap == Wrap::TopBottom || covers)
                && y - top <= self.state.y + 1.0
                && y + float_height + bottom > self.state.y
            {
                // Handled by the caller's next lines: the cursor moves down.
                self.state.pending_push = Some(y + float_height + bottom);
            }
        }
    }

    /// Draws a shape (text box, rectangle) at `x`, `y`.
    fn draw_shape(&mut self, shape: &Shape, page: usize, x: f32, y: f32, width: f32) {
        let mut height = shape.height;
        let inner_width = (width - shape.insets[0] - shape.insets[2]).max(1.0);
        let mut sub = SubFlow::new(inner_width);
        if !shape.blocks.is_empty() {
            self.sub_blocks(&mut sub, &shape.blocks, Some(self.page_number(page)), None);
            if shape.auto_height {
                height = height.max(sub.y + shape.insets[1] + shape.insets[3]);
            }
            // A bookmark in the text box is on the page the box is on.
            for (name, dy) in std::mem::take(&mut sub.bookmarks) {
                self.destinations.push(Destination {
                    name,
                    page,
                    y: y + shape.insets[1] + dy,
                });
            }
        }
        if let Some(color) = shape.fill {
            self.pages[page].items.push(Item::Rect {
                x,
                y,
                width,
                height,
                color,
            });
        }
        for &(index, px, py, pw, ph) in &shape.pictures {
            self.pages[page].items.push(Item::Image {
                index,
                x: x + px,
                y: y + py,
                width: pw,
                height: ph,
                crop: [0.0; 4],
            });
        }
        if let Some(line) = shape.line {
            let mut lines = Vec::new();
            border_line(&mut lines, line, x, y, x + width, y);
            border_line(&mut lines, line, x, y + height, x + width, y + height);
            border_line(&mut lines, line, x, y, x, y + height);
            border_line(&mut lines, line, x + width, y, x + width, y + height);
            self.pages[page].items.extend(lines);
        }
        push_items(
            &mut self.pages[page].items,
            &sub.items,
            x + shape.insets[0],
            y + shape.insets[1],
        );
        let section = self.page_section(page);
        self.put_sub_floats(
            &sub.floats,
            section,
            page,
            x + shape.insets[0],
            y + shape.insets[1],
            inner_width,
        );
    }

    /// Moves the items of page `page` from `start` on behind the text,
    /// above what is already behind it.
    fn move_behind(&mut self, page: usize, start: usize) {
        let items = &mut self.pages[page].items;
        let moved: Vec<Item> = items.drain(start..).collect();
        let at = self.behind.entry(page).or_default();
        let count = moved.len();
        items.splice(*at..*at, moved);
        *at += count;
    }

    /// The number of page `page`.
    fn page_number(&self, page: usize) -> u32 {
        self.page_info.get(page).map_or(self.state.number, |info| info.number)
    }

    /// The section of page `page`.
    fn page_section(&self, page: usize) -> usize {
        self.page_info.get(page).map_or(self.state.section, |info| info.section)
    }

    /// Draws the floating objects of a sub-flow whose origin is at `x`, `y`
    /// of page `page`.
    fn put_sub_floats(&mut self, floats: &[SubFloat], section: usize, page: usize, x: f32, y: f32, width: f32) {
        for sub_float in floats {
            self.put_float(
                &sub_float.float,
                section,
                page,
                (x + sub_float.x, width),
                (y + sub_float.para_top, y + sub_float.line_top),
                false,
            );
        }
    }

    /// The size of a shape: a width of 0 is that of its text (a frame of
    /// automatic width), and an automatic height grows with the text.
    fn shape_size(&mut self, shape: &Shape, column_width: f32, page: usize) -> (f32, f32) {
        let number = Some(self.page_number(page));
        let insets = shape.insets[0] + shape.insets[2];
        let mut width = shape.width;
        if width <= 0.0 {
            let mut sub = SubFlow::new(column_width.max(1.0));
            self.sub_blocks(&mut sub, &shape.blocks, number, None);
            // The text from the frame's left edge (with its indents).
            let (_, right) = items_extent(&sub.items).unwrap_or((0.0, 0.0));
            width = (right.max(0.0) + insets + 1.0).max(1.0);
        }
        let mut height = shape.height;
        if shape.auto_height && !shape.blocks.is_empty() {
            let mut sub = SubFlow::new((width - insets).max(1.0));
            self.sub_blocks(&mut sub, &shape.blocks, number, None);
            height = height.max(sub.y + shape.insets[1] + shape.insets[3]);
        }
        (width, height)
    }

    /// Places a table in the body, row by row.
    fn table(&mut self, table: &Table) {
        let after_prev = self.state.pending_after.take().map_or(0.0, |(after, _, _)| after);
        self.state.y += after_prev + std::mem::take(&mut self.state.pending_border);
        self.state.last_borders = None;
        // A table floating below the text, as wide as the column: the text
        // goes on below it; it starts its offset lower.
        let below_text = table
            .props
            .float
            .as_ref()
            .filter(|anchor| anchor.vertical.from == AnchorFrom::Paragraph && anchor.vertical.align.is_none());
        if let Some(anchor) = below_text
            && !self.state.fresh
        {
            self.state.y += anchor.vertical.offset.max(0.0);
        }
        let width = self.column_width();
        let laid = self.lay_table(table, width);
        let header_rows: Vec<usize> = (0..laid.rows.len()).take_while(|&at| table.rows[at].header).collect();
        let mut at = 0;
        // Right after a page break (and the repeated header rows), a row is
        // placed even when it does not fit.
        let mut forced = false;
        let mut rest: Option<LaidRow> = None;
        while at < laid.rows.len() {
            let row = rest.take().unwrap_or_else(|| laid.rows[at].clone());
            let room = self.state.bottom - self.state.notes_height - self.state.y;
            if row.height <= room + EPS || forced && self.state.fresh {
                if row.height > room + EPS
                    && let Some((first, second)) = split_row(&row, room)
                {
                    self.draw_row(&first);
                    rest = Some(second);
                    self.page_break_in_table(&laid, &header_rows, at);
                    forced = true;
                    continue;
                }
                forced = false;
                self.draw_row(&row);
                at += 1;
                continue;
            }
            // The row does not fit: split it at a line, or move it on.
            if !table.rows[at].cant_split
                && let Some((first, second)) = split_row(&row, room)
            {
                self.draw_row(&first);
                rest = Some(second);
            } else {
                rest = Some(row);
            }
            self.page_break_in_table(&laid, &header_rows, at);
            forced = true;
        }
        if let Some(anchor) = below_text {
            self.state.y += anchor.dist[1];
        }
        self.state.last_style = None;
    }

    /// Moves a table to the next column or page, repeating its header rows.
    fn page_break_in_table(&mut self, laid: &LaidTable, header_rows: &[usize], at: usize) {
        self.next_column();
        if !header_rows.is_empty() && at >= header_rows.len() {
            for &header in header_rows {
                let row = laid.rows[header].clone();
                self.draw_row(&row);
            }
        }
        // The page counts as fresh for the row that follows.
        self.state.fresh = true;
    }

    /// Draws a laid-out row at the cursor.
    fn draw_row(&mut self, row: &LaidRow) {
        let x = self.column_left();
        let y = self.state.y;
        let page = self.pages.len() - 1;
        if trace() {
            let heights: Vec<String> = row
                .cells
                .iter()
                .map(|cell| format!("{:.2}", cell.content_height))
                .collect();
            eprintln!("ROWTRACE\t{page}\t{y:.2}\t{:.2}\t{}", row.height, heights.join(" "));
        }
        for cell in &row.cells {
            if let Some(color) = cell.shading {
                self.pages[page].items.push(Item::Rect {
                    x: x + cell.x,
                    y,
                    width: cell.width,
                    height: cell.height.max(row.height),
                    color,
                });
            }
        }
        for cell in &row.cells {
            let height = cell.height.max(row.height);
            let offset = match cell.valign {
                VAlign::Top => 0.0,
                VAlign::Center => (height - cell.content_height) / 2.0,
                VAlign::Bottom => height - cell.content_height,
            }
            .max(0.0);
            push_items(&mut self.pages[page].items, &cell.items, x + cell.x, y + offset);
            let section = self.state.section;
            self.put_sub_floats(&cell.floats, section, page, x + cell.x, y + offset, cell.width);
            for (bookmark, dy) in &cell.bookmarks {
                self.destinations.push(Destination {
                    name: bookmark.clone(),
                    page,
                    y: y + dy,
                });
            }
        }
        for cell in &row.cells {
            let height = cell.height.max(row.height);
            let (left, right) = (x + cell.x, x + cell.x + cell.width);
            let (top, bottom) = (y, y + height);
            let mut lines = Vec::new();
            let [b_top, b_left, b_bottom, b_right] = cell.borders;
            if let Some(border) = b_top.filter(|_| cell.draw_top) {
                border_line(&mut lines, border, left, top, right, top);
            }
            if let Some(border) = b_bottom.filter(|_| cell.draw_bottom) {
                border_line(&mut lines, border, left, bottom, right, bottom);
            }
            if let Some(border) = b_left {
                border_line(&mut lines, border, left, top, left, bottom);
            }
            if let Some(border) = b_right {
                border_line(&mut lines, border, right, top, right, bottom);
            }
            self.pages[page].items.extend(lines);
        }
        self.state.y += row.height;
        self.state.fresh = false;
    }

    /// Lays out a table at a column width: column positions, row heights and
    /// cell content.
    fn lay_table(&mut self, table: &Table, width: f32) -> LaidTable {
        let grid = table_grid(table, width);
        let left = self.table_left(table, width, &grid);
        let mut rows: Vec<LaidRow> = Vec::new();
        // Merged cells: the row where each column's merge started.
        let mut merge_start: HashMap<usize, (usize, usize)> = HashMap::new();
        for (row_at, row) in table.rows.iter().enumerate() {
            let mut column = row.grid_before;
            let mut cells = Vec::new();
            for cell in &row.cells {
                let span = cell.span.max(1);
                let cell_x = left + grid.iter().take(column).sum::<f32>();
                let cell_width: f32 = grid.iter().skip(column).take(span).sum();
                let mut laid = self.lay_cell(table, cell, cell_x, cell_width);
                if cell.vmerge == VMerge::Continue {
                    if let Some(&(start_row, start_cell)) = merge_start.get(&column)
                        && let Some(start) = rows.get_mut(start_row).and_then(|row| row.cells.get_mut(start_cell))
                    {
                        start.merged_rows += 1;
                        start.draw_bottom = false;
                    }
                } else if cell.vmerge == VMerge::Restart {
                    merge_start.insert(column, (row_at, cells.len()));
                    laid.height = 0.0;
                } else {
                    merge_start.remove(&column);
                }
                cells.push(laid);
                column += span;
            }
            let border = top_border(&cells);
            for cell in &mut cells {
                shift_cell(cell, border);
            }
            let height = laid_row_height(row, &cells);
            if matches!(row.height, Some((_, true))) {
                // Word leaves out what does not fit in a row of exact height
                // (but a merged cell spans more rows).
                for (laid, cell) in cells.iter_mut().zip(&row.cells) {
                    if cell.vmerge == VMerge::None {
                        clip_cell(laid, height);
                    }
                }
            }
            rows.push(LaidRow { height, cells });
        }
        grow_merged_rows(&mut rows);
        LaidTable { rows }
    }

    /// The left edge of a table in a column of `width`, from its alignment
    /// and indent; `grid` is the widths of its grid columns.
    fn table_left(&self, table: &Table, width: f32, grid: &[f32]) -> f32 {
        let total: f32 = grid.iter().sum();
        let first_margin = table
            .rows
            .first()
            .and_then(|row| row.cells.first())
            .map_or(table.props.cell_margins[0], |cell| cell_margin(cell, table, 0));
        let mut left = match table.props.align {
            Align::Center => (width - total) / 2.0,
            Align::Right => width - total,
            _ => table.props.indent,
        };
        if self.doc.compat_mode < 15 && table.props.align == Align::Left {
            // Before Word 2013, the text of the first cell is on the margin.
            left -= first_margin;
        }
        left
    }

    /// Lays out one cell at `cell_x`, `cell_width` wide. A cell that
    /// continues a vertical merge gets no content, top border or shading.
    fn lay_cell(&mut self, table: &Table, cell: &Cell, cell_x: f32, cell_width: f32) -> LaidCell {
        let margins = [
            cell_margin(cell, table, 0),
            cell_margin(cell, table, 1),
            cell_margin(cell, table, 2),
            cell_margin(cell, table, 3),
        ];
        let inner = (cell_width - margins[0] - margins[2]).max(1.0);
        let mut laid = LaidCell {
            x: cell_x,
            width: cell_width,
            height: 0.0,
            content_height: 0.0,
            items: Vec::new(),
            floats: Vec::new(),
            breaks: Vec::new(),
            top_margin: margins[1],
            bottom_margin: margins[3],
            bookmarks: Vec::new(),
            borders: cell.borders,
            shading: cell.shading,
            valign: cell.valign,
            draw_top: true,
            draw_bottom: true,
            merged_rows: 1,
        };
        if cell.vmerge == VMerge::Continue {
            laid.draw_top = false;
            laid.shading = None;
            laid.borders[0] = None;
            return laid;
        }
        let mut sub = SubFlow::new(inner);
        sub.in_cell = true;
        self.sub_blocks(&mut sub, &cell.blocks, Some(self.state.number), None);
        laid.content_height = sub.y + margins[1] + margins[3];
        laid.height = laid.content_height;
        laid.breaks = sub.breaks.iter().map(|y| y + margins[1]).collect();
        let mut items = Vec::new();
        push_items(&mut items, &sub.items, margins[0], margins[1]);
        laid.items = items;
        laid.floats = sub
            .floats
            .iter()
            .map(|float| float.moved(margins[0], margins[1]))
            .collect();
        laid.bookmarks = sub
            .bookmarks
            .into_iter()
            .map(|(name, y)| (name, y + margins[1]))
            .collect();
        laid
    }

    /// Lays out blocks in a sub-flow (a cell, a header, a note, a text
    /// box): no pages, only a growing height.
    fn sub_blocks(&mut self, sub: &mut SubFlow, blocks: &[Block], page: Option<u32>, _: Option<()>) {
        for block in blocks {
            match block {
                Block::Para(para) => self.sub_para(sub, para, page),
                Block::Table(table) => self.sub_table(sub, table),
            }
        }
        sub.close_frame();
        // The space after the last paragraph counts in a cell.
        if let Some((after, _, _)) = sub.pending_after.take()
            && sub.in_cell
        {
            sub.y += after;
        }
    }

    /// Lays out a paragraph in a sub-flow, with the space before it and its
    /// floats, bookmarks and shading. `page` is the page number its fields
    /// show.
    fn sub_para(&mut self, sub: &mut SubFlow, para: &Para, page: Option<u32>) {
        let mut ctx = self.ctx();
        ctx.page = page.map(|number| (number, sub.format.clone()));
        ctx.page_count = sub.page_count;
        ctx.section_pages = sub.section_pages;
        ctx.own_note = sub.own_note.clone();
        let layout = para::layout(&mut ctx, para, sub.width);
        let after_prev = match sub.pending_after.take() {
            Some((after, style, contextual)) if !(contextual && style == para.style) => after,
            _ => 0.0,
        };
        let before = if para.props.before_auto {
            if sub.y == 0.0 && sub.in_cell {
                0.0
            } else {
                AUTO_SPACING_PT
            }
        } else {
            para.props.before
        };
        let before = if para.props.contextual && sub.last_style.as_deref() == Some(para.style.as_str()) {
            0.0
        } else {
            before
        };
        sub.y += self.spacing(after_prev, before);
        sub.borders(para);
        let top = sub.y;
        for float in &layout.floats {
            let line_top = top
                + layout.lines[..float.line.min(layout.lines.len())]
                    .iter()
                    .map(|line| line.height)
                    .sum::<f32>();
            sub.floats.push(SubFloat {
                float: float.clone(),
                x: 0.0,
                para_top: top,
                line_top,
            });
        }
        for line in &layout.lines {
            for bookmark in &line.bookmarks {
                sub.bookmarks.push((bookmark.clone(), sub.y));
            }
            push_items(&mut sub.items, &line.items, 0.0, sub.y);
            sub.y += line.height;
            sub.breaks.push(sub.y);
        }
        if let Some(color) = para.props.shading {
            sub.items.insert(
                0,
                Item::Rect {
                    x: para.props.left,
                    y: top,
                    width: sub.width - para.props.left - para.props.right,
                    height: sub.y - top,
                    color,
                },
            );
        }
        let after = if para.props.after_auto {
            AUTO_SPACING_PT
        } else {
            para.props.after
        };
        sub.bottom_border(para);
        sub.pending_after = Some((after, para.style.clone(), para.props.contextual));
        sub.last_style = Some(para.style.clone());
    }

    /// Lays out a table in a sub-flow, row by row, with its shading and
    /// borders.
    fn sub_table(&mut self, sub: &mut SubFlow, table: &Table) {
        sub.close_frame();
        if let Some((after, _, _)) = sub.pending_after.take() {
            sub.y += after;
        }
        let laid = self.lay_table(table, sub.width);
        for row in &laid.rows {
            let mut row_items = Vec::new();
            for cell in &row.cells {
                let height = cell.height.max(row.height);
                if let Some(color) = cell.shading {
                    row_items.push(Item::Rect {
                        x: cell.x,
                        y: 0.0,
                        width: cell.width,
                        height,
                        color,
                    });
                }
                push_items(&mut row_items, &cell.items, cell.x, 0.0);
                sub.floats
                    .extend(cell.floats.iter().map(|float| float.moved(cell.x, sub.y)));
                let [b_top, b_left, b_bottom, b_right] = cell.borders;
                let (left, right) = (cell.x, cell.x + cell.width);
                if let Some(border) = b_top.filter(|_| cell.draw_top) {
                    border_line(&mut row_items, border, left, 0.0, right, 0.0);
                }
                if let Some(border) = b_bottom.filter(|_| cell.draw_bottom) {
                    border_line(&mut row_items, border, left, height, right, height);
                }
                if let Some(border) = b_left {
                    border_line(&mut row_items, border, left, 0.0, left, height);
                }
                if let Some(border) = b_right {
                    border_line(&mut row_items, border, right, 0.0, right, height);
                }
            }
            push_items(&mut sub.items, &row_items, 0.0, sub.y);
            sub.y += row.height;
            // The cell around the table can be cut between its rows.
            sub.breaks.push(sub.y);
        }
        sub.last_style = None;
    }

    /// Places the endnotes after the body.
    fn endnotes(&mut self) {
        if self.doc.endnotes.is_empty() {
            return;
        }
        let mut referenced: Vec<&(String, Vec<Block>)> = self
            .doc
            .endnotes
            .iter()
            .filter(|(id, _)| self.notes.contains_key(&(true, id.clone())))
            .collect();
        referenced.sort_by_key(|(id, _)| {
            self.notes
                .get(&(true, id.clone()))
                .map(|number| roman_value(number))
                .unwrap_or(0)
        });
        if referenced.is_empty() {
            return;
        }
        let x = self.column_left();
        let separator = self.separator_height();
        let page = self.pages.len() - 1;
        if let Some((after, _, _)) = self.state.pending_after.take() {
            self.state.y += after;
        }
        let rule_y = self.state.y + separator / 2.0;
        self.pages[page].items.push(Item::Line {
            x1: x,
            y1: rule_y,
            x2: x + NOTE_SEPARATOR_LEN_PT,
            y2: rule_y,
            width: NOTE_SEPARATOR_WIDTH_PT,
            color: [0, 0, 0],
            dash: None,
        });
        self.state.y += separator;
        for (id, blocks) in referenced {
            let own = self.notes.get(&(true, id.clone())).cloned();
            for block in blocks {
                if let Block::Para(para) = block {
                    let width = self.column_width();
                    let mut ctx = self.ctx();
                    ctx.own_note = own.clone();
                    let layout = para::layout(&mut ctx, para, width);
                    let after_prev = self.state.pending_after.take().map_or(0.0, |(after, _, _)| after);
                    let gap = self.spacing(after_prev, para.props.before);
                    self.place_lines(para, layout, gap);
                    self.state.pending_after = Some((para.props.after, para.style.clone(), para.props.contextual));
                }
            }
        }
    }
}

/// A laid-out table.
#[derive(Debug, Clone)]
struct LaidTable {
    rows: Vec<LaidRow>,
}

/// A laid-out row.
#[derive(Debug, Clone)]
struct LaidRow {
    height: f32,
    cells: Vec<LaidCell>,
}

/// A laid-out cell, relative to the table's left edge and the row's top.
#[derive(Debug, Clone)]
struct LaidCell {
    x: f32,
    width: f32,
    height: f32,
    content_height: f32,
    items: Vec<Item>,
    /// Floating objects anchored in it (from the cell's top left).
    floats: Vec<SubFloat>,
    /// Where the content may be cut across pages (from the top of the
    /// cell).
    breaks: Vec<f32>,
    /// Top margin.
    top_margin: f32,
    /// Bottom margin.
    bottom_margin: f32,
    bookmarks: Vec<(String, f32)>,
    borders: [Option<Border>; 4],
    shading: Option<Rgb>,
    valign: VAlign,
    draw_top: bool,
    draw_bottom: bool,
    merged_rows: usize,
}

/// Blocks laid out without pages.
struct SubFlow {
    width: f32,
    y: f32,
    items: Vec<Item>,
    /// Floating objects anchored in it.
    floats: Vec<SubFloat>,
    /// Bottoms of the lines placed: where the content may be cut across
    /// pages.
    breaks: Vec<f32>,
    bookmarks: Vec<(String, f32)>,
    pending_after: Option<(f32, String, bool)>,
    last_style: Option<String>,
    /// The borders and indents of the last paragraph, when it has borders.
    last_borders: Option<(ParaBorders, f32, f32)>,
    /// The bottom border of the last paragraph, with its left and right
    /// ends and the paragraph's bottom: drawn below it unless the next
    /// paragraph is of the same frame.
    pending_bottom: Option<(Border, f32, f32, f32)>,
    in_cell: bool,
    own_note: Option<String>,
    page_count: Option<u32>,
    section_pages: Option<u32>,
    format: NumFormat,
}

impl SubFlow {
    /// The top border of a paragraph that starts a frame, or the rule
    /// between two paragraphs of one; ends the frame of the paragraph
    /// before when this one is not part of it. In a header, a footer or a
    /// cell, Word draws them in the spacing around the text, without
    /// making room for them.
    fn borders(&mut self, para: &Para) {
        let borders = para.props.borders;
        let key = (borders, para.props.left, para.props.right);
        let framed = borders.top.is_some() || borders.bottom.is_some() || borders.between.is_some();
        let same_frame = framed && self.last_borders == Some(key);
        let (left, right) = (para.props.left, self.width - para.props.right);
        if same_frame {
            self.pending_bottom = None;
            if let Some(between) = borders.between {
                let y = self.y - between.space - between.width / 2.0;
                border_line(&mut self.items, between, left, y, right, y);
            }
        } else {
            self.close_frame();
            if let Some(top) = borders.top {
                let y = self.y - top.space - top.width / 2.0;
                border_line(&mut self.items, top, left, y, right, y);
            }
        }
        self.last_borders = framed.then_some(key);
    }

    /// Notes the bottom border of the paragraph just placed, drawn when its
    /// frame ends.
    fn bottom_border(&mut self, para: &Para) {
        self.pending_bottom = para
            .props
            .borders
            .bottom
            .map(|bottom| (bottom, para.props.left, self.width - para.props.right, self.y));
    }

    /// Draws the bottom border of the last frame, below its space.
    fn close_frame(&mut self) {
        if let Some((bottom, left, right, y)) = self.pending_bottom.take() {
            let y = y + bottom.space + bottom.width / 2.0;
            border_line(&mut self.items, bottom, left, y, right, y);
        }
        self.last_borders = None;
    }

    /// An empty sub-flow of width `width`.
    fn new(width: f32) -> Self {
        SubFlow {
            width,
            y: 0.0,
            items: Vec::new(),
            floats: Vec::new(),
            breaks: Vec::new(),
            bookmarks: Vec::new(),
            pending_after: None,
            last_style: None,
            last_borders: None,
            pending_bottom: None,
            in_cell: false,
            own_note: None,
            page_count: None,
            section_pages: None,
            format: NumFormat::Decimal,
        }
    }
}

/// Cuts a row so that its first part fits in `room`: each cell is cut at
/// its last line that fits. `None` when no cell has a line that fits.
fn split_row(row: &LaidRow, room: f32) -> Option<(LaidRow, LaidRow)> {
    let mut first = row.clone();
    let mut second = row.clone();
    let mut any = false;
    let mut rest_height: f32 = 0.0;
    for (at, cell) in row.cells.iter().enumerate() {
        let limit = room - cell.bottom_margin;
        let content_bottom = cell.content_height - cell.bottom_margin;
        let cut = if content_bottom <= limit + EPS {
            content_bottom
        } else {
            match cell
                .breaks
                .iter()
                .copied()
                .filter(|y| *y <= limit + EPS)
                .reduce(f32::max)
            {
                Some(cut) => {
                    any = true;
                    cut
                }
                None => cell.top_margin,
            }
        };
        let (head, tail): (Vec<Item>, Vec<Item>) =
            cell.items.iter().cloned().partition(|item| item_top(item) < cut - EPS);
        let (float_head, float_tail): (Vec<SubFloat>, Vec<SubFloat>) = cell
            .floats
            .iter()
            .cloned()
            .partition(|float| float.para_top < cut - EPS);
        first.cells[at].floats = float_head;
        second.cells[at].floats = float_tail
            .iter()
            .map(|float| float.moved(0.0, -(cut - cell.top_margin)))
            .collect();
        first.cells[at].items = head;
        first.cells[at].draw_bottom = false;
        first.cells[at].height = room;
        let shift = cut - cell.top_margin;
        let mut moved = Vec::new();
        push_items(&mut moved, &tail, 0.0, -shift);
        second.cells[at].items = moved;
        second.cells[at].breaks = cell
            .breaks
            .iter()
            .filter(|y| **y > cut + EPS)
            .map(|y| y - shift)
            .collect();
        second.cells[at].draw_top = false;
        let remaining = (cell.content_height - shift).max(cell.top_margin + cell.bottom_margin);
        second.cells[at].content_height = remaining;
        second.cells[at].height = remaining;
        rest_height = rest_height.max(remaining);
    }
    if !any {
        return None;
    }
    first.height = room;
    second.height = rest_height;
    Some((first, second))
}

/// Whether a paragraph holds a page break.
fn breaks_page(para: &Para) -> bool {
    para.inlines.iter().any(|inline| {
        matches!(
            inline,
            Inline::Break {
                kind: crate::word::fmt::model::BreakKind::Page,
                ..
            }
        )
    })
}

/// Leaves out of a cell what starts below `height`.
fn clip_cell(cell: &mut LaidCell, height: f32) {
    cell.items.retain(|item| item_top(item) < height);
    cell.breaks.retain(|&at| at < height);
    cell.content_height = cell.content_height.min(height);
    cell.height = cell.height.min(height);
}

/// The top of an item, to tell which part of a cut cell it is in.
fn item_top(item: &Item) -> f32 {
    match item {
        Item::Text(run) => run.y - run.size * 0.5,
        Item::Rect { y, .. } | Item::Image { y, .. } | Item::Link { y, .. } => *y,
        Item::Line { y1, y2, .. } => y1.min(*y2),
    }
}

/// The height of a laid-out row: its cells' content (below the row's top
/// border, which Word adds), or the row's set height (exact, or at least).
fn laid_row_height(row: &Row, cells: &[LaidCell]) -> f32 {
    let content: f32 = cells.iter().map(|cell| cell.height).fold(0.0, f32::max);
    match row.height {
        Some((value, true)) => value,
        // A minimum height is that of the cells' content, inside their
        // margins and the row's border (in the top margins).
        Some((value, false)) => {
            let margins = cells
                .iter()
                .map(|cell| cell.top_margin + cell.bottom_margin)
                .fold(0.0, f32::max);
            content.max(value + margins)
        }
        None => content,
    }
}

/// The width of the top border of a row: the widest of its cells'.
fn top_border(cells: &[LaidCell]) -> f32 {
    cells
        .iter()
        .filter(|cell| cell.draw_top)
        .filter_map(|cell| cell.borders[0].map(|border| border.width))
        .fold(0.0, f32::max)
}

/// Moves the content of a cell down by `by` (the row's top border, which
/// Word puts above the text).
fn shift_cell(cell: &mut LaidCell, by: f32) {
    if by <= 0.0 {
        return;
    }
    let items = std::mem::take(&mut cell.items);
    push_items(&mut cell.items, &items, 0.0, by);
    for float in &mut cell.floats {
        *float = float.moved(0.0, by);
    }
    for y in &mut cell.breaks {
        *y += by;
    }
    for (_, y) in &mut cell.bookmarks {
        *y += by;
    }
    cell.top_margin += by;
    cell.content_height += by;
    cell.height += by;
}

/// Grows the rows that vertically merged cells span so that they hold the
/// cells' content, and sets each merged cell's height to the rows it spans.
fn grow_merged_rows(rows: &mut [LaidRow]) {
    // The merges, as (first row, cell, rows), taken by the row they end
    // in: a merge that ends earlier grows its rows before the merges that
    // span them are measured.
    let mut merges: Vec<(usize, usize, usize)> = Vec::new();
    for (row_at, row) in rows.iter().enumerate() {
        for (cell_at, cell) in row.cells.iter().enumerate() {
            if cell.merged_rows > 1 {
                merges.push((row_at, cell_at, cell.merged_rows));
            }
        }
    }
    merges.sort_by_key(|&(row_at, _, merged)| (row_at + merged, std::cmp::Reverse(row_at)));
    for &(row_at, cell_at, merged) in &merges {
        let end = (row_at + merged).min(rows.len());
        let need = rows[row_at].cells[cell_at].content_height;
        let have: f32 = rows[row_at..end].iter().map(|row| row.height).sum();
        if need > have {
            rows[end - 1].height += need - have;
        }
    }
    for &(row_at, cell_at, merged) in &merges {
        let end = (row_at + merged).min(rows.len());
        rows[row_at].cells[cell_at].height = rows[row_at..end].iter().map(|row| row.height).sum();
    }
}

/// The margin of a cell on one side (0 left, 1 top, 2 right, 3 bottom):
/// its own, else the table's.
fn cell_margin(cell: &Cell, table: &Table, side: usize) -> f32 {
    let own = cell.margins[side];
    if own.is_nan() {
        table.props.cell_margins[side]
    } else {
        own
    }
}

/// The widths of the grid columns of a table: Word's last layout, else
/// from the cell widths, else equal.
fn table_grid(table: &Table, width: f32) -> Vec<f32> {
    let columns = table
        .rows
        .iter()
        .map(|row| row.grid_before + row.grid_after + row.cells.iter().map(|cell| cell.span.max(1)).sum::<usize>())
        .max()
        .unwrap_or(0);
    if table.grid.len() >= columns && table.grid.iter().sum::<f32>() > 0.0 {
        let mut grid = table.grid.clone();
        if let Width::Pct(share) = table.props.width
            && !table.props.fixed
        {
            // A table set to a share of the width is scaled to it.
            let total: f32 = grid.iter().sum();
            let want = share * width;
            if total > 0.0 && (want - total).abs() > 1.0 {
                for col in &mut grid {
                    *col *= want / total;
                }
            }
        }
        return grid;
    }
    let count = columns.max(1);
    vec![width / count as f32; count]
}

/// The left edge and width of each column of a section.
fn column_boxes(section: &Section, left: f32, width: f32) -> Vec<(f32, f32)> {
    let columns = &section.columns;
    if !columns.widths.is_empty() {
        let mut x = left;
        return columns
            .widths
            .iter()
            .map(|&(width, space)| {
                let column = (x, width);
                x += width + space;
                column
            })
            .collect();
    }
    let count = columns.count.max(1) as f32;
    let column_width = (width - columns.space * (count - 1.0)) / count;
    (0..columns.count.max(1))
        .map(|at| (left + at as f32 * (column_width + columns.space), column_width))
        .collect()
}

/// Adds `items` moved by (`dx`, `dy`) to `out`.
fn push_items(out: &mut Vec<Item>, items: &[Item], dx: f32, dy: f32) {
    for item in items {
        let mut item = item.clone();
        match &mut item {
            Item::Text(run) => {
                run.x += dx;
                run.y += dy;
            }
            Item::Rect { x, y, .. } | Item::Image { x, y, .. } | Item::Link { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
        }
        out.push(item);
    }
}

/// Draws a border line.
fn border_line(out: &mut Vec<Item>, border: Border, x1: f32, y1: f32, x2: f32, y2: f32) {
    let dash = match border.style {
        BorderStyle::Dotted => Some(border.width),
        BorderStyle::Dashed => Some(border.width * 3.0),
        _ => None,
    };
    if border.style == BorderStyle::Double {
        let gap = border.width;
        let (dx, dy) = if (y1 - y2).abs() < f32::EPSILON {
            (0.0, gap)
        } else {
            (gap, 0.0)
        };
        for sign in [-1.0, 1.0] {
            out.push(Item::Line {
                x1: x1 + dx * sign,
                y1: y1 + dy * sign,
                x2: x2 + dx * sign,
                y2: y2 + dy * sign,
                width: border.width / 2.0,
                color: border.color,
                dash: None,
            });
        }
        return;
    }
    out.push(Item::Line {
        x1,
        y1,
        x2,
        y2,
        width: border.width,
        color: border.color,
        dash,
    });
}

/// The value of a note number written in Roman numerals or digits, for
/// ordering endnotes.
fn roman_value(text: &str) -> u32 {
    if let Ok(number) = text.parse() {
        return number;
    }
    let value = |ch: char| match ch.to_ascii_lowercase() {
        'i' => 1,
        'v' => 5,
        'x' => 10,
        'l' => 50,
        'c' => 100,
        'd' => 500,
        'm' => 1000,
        _ => 0,
    };
    let digits: Vec<i64> = text.chars().map(value).collect();
    let mut total: i64 = 0;
    for (at, &digit) in digits.iter().enumerate() {
        if digits.get(at + 1).is_some_and(|&next| next > digit) {
            total -= digit;
        } else {
            total += digit;
        }
    }
    total.max(0) as u32
}

/// A floating object of a sub-flow, with the top of its paragraph and line
/// in the sub-flow.
#[derive(Debug, Clone)]
struct SubFloat {
    float: Float,
    /// Left of its column in the sub-flow.
    x: f32,
    para_top: f32,
    line_top: f32,
}

impl SubFloat {
    /// The same float, moved by `dx`, `dy`.
    fn moved(&self, dx: f32, dy: f32) -> Self {
        SubFloat {
            float: self.float.clone(),
            x: self.x + dx,
            para_top: self.para_top + dy,
            line_top: self.line_top + dy,
        }
    }
}

/// The leftmost and rightmost points of the text of `items`.
fn items_extent(items: &[Item]) -> Option<(f32, f32)> {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Text(run) => {
                let last = run.glyphs.last()?;
                Some((run.x, run.x + last.x + last.advance))
            }
            _ => None,
        })
        .reduce(|(left, right), (other_left, other_right)| (left.min(other_left), right.max(other_right)))
}

/// Whether `blocks` show anything in the flow: text, a table, a picture or
/// shape in the line (not empty paragraphs, nor objects floating apart).
fn shows_something(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Table(_) => true,
        Block::Para(para) => para.inlines.iter().any(|inline| match inline {
            Inline::Text { text, .. } => !text.trim().is_empty(),
            Inline::Tab { .. } | Inline::Break { .. } => false,
            Inline::Image { anchor, .. } => anchor.is_none(),
            Inline::Shape(shape) => shape.anchor.is_none(),
            _ => true,
        }),
    })
}
