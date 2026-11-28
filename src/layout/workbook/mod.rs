//! A document without pages (an Excel workbook) laid out in pages as a
//! document: its blocks are turned into the formatted form the Word layout
//! engine reads ([`FDoc`]), which then lays them out and writes the PDF.
//! The look is that of the LaTeX output: one sheet after another, each
//! starting with its heading; tables with rules above, under the header
//! and below, their cells wrapped in their columns, broken across pages
//! between rows with the header rows repeated; a table too wide for the
//! page set smaller, then on landscape pages, then cut into groups of
//! columns that each repeat the first ([`table`]); figures at their size,
//! made smaller to fit the page; numbered pages.

mod header;
pub(crate) mod print;
mod table;

use super::fonts::FontBook;
use super::para::{self, Ctx};
use super::shape::Shaper;
use crate::model::{Block as DocBlock, BlockKind, Document, ImageSpec, Picture, Span, style};
use crate::word::fmt::model::{
    Align, Block, FDoc, FImage, FieldKind, HeaderSet, Inline, LineSpacing, PageSetup, Para, ParaProps, RunFmt, Section,
    SectionStart, VertAlign,
};
use anyhow::Result;
use std::collections::HashMap;

/// Serif families for the text, the first one installed is taken (Times
/// New Roman's metric twin first, as most systems have it).
const SERIF_FAMILIES: &[&str] = &[
    "Liberation Serif",
    "Times New Roman",
    "Tinos",
    "DejaVu Serif",
    "Noto Serif",
];
/// Families for Japanese, Chinese and Korean text, the first one installed
/// is taken (serif designs first, as the LaTeX output's).
const CJK_FAMILIES: &[&str] = &[
    "Noto Serif CJK JP",
    "Source Han Serif JP",
    "IPAexMincho",
    "IPAMincho",
    "Yu Mincho",
    "MS Mincho",
    "Hiragino Mincho ProN",
    "Noto Sans CJK JP",
    "Source Han Sans JP",
    "IPAexGothic",
    "Noto Serif CJK SC",
    "Noto Sans CJK SC",
];
/// Monospaced families, for code text.
const MONO_FAMILIES: &[&str] = &["Liberation Mono", "Courier New", "DejaVu Sans Mono"];
/// Sans-serif families, for text a workbook sets without serifs.
const SANS_FAMILIES: &[&str] = &["Liberation Sans", "Arial", "DejaVu Sans"];
/// Colour of the text of a link (that of the LaTeX output's links).
const LINK_COLOR: [u8; 3] = [0, 0, 255];
/// Distance between default tab stops.
const DEFAULT_TAB_PT: f32 = 36.0;
/// The Word version whose layout rules apply (Word 2013 and later).
const COMPAT_MODE: u32 = 15;
/// Indent of each level of a list.
const LIST_INDENT_EM: f32 = 1.5;

/// How a workbook is laid out in pages ([`to_pdf`]): page, margins, font
/// sizes, and the limits of how a wide table is made to fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkbookPdfConfig {
    /// Width of the page when the workbook's print settings name no paper
    /// size: A4.
    pub page_width_pt: f32,
    /// Height of that page.
    pub page_height_pt: f32,
    /// Margin on each side of the page (2.5 cm, as the LaTeX output's).
    pub margin_pt: f32,
    /// Font size of the paragraphs.
    pub body_size_pt: f32,
    /// Font size of a sheet's heading (a level-1 heading); deeper levels
    /// are set at [`WorkbookPdfConfig::subheading_size_pt`].
    pub heading_size_pt: f32,
    /// Font size of the deeper headings.
    pub subheading_size_pt: f32,
    /// Font size of the captions of tables and figures.
    pub caption_size_pt: f32,
    /// Font size a table is set in when it fits the width of the text.
    pub table_size_pt: f32,
    /// The smallest font size a table too wide for the text is set in on
    /// portrait pages; a table that does not fit even then is set on
    /// landscape pages (from [`WorkbookPdfConfig::table_size_pt`] down to
    /// this size again), and one that does not fit there is cut into
    /// groups of columns at this size.
    pub min_table_size_pt: f32,
    /// How much smaller each next font size tried for a wide table is.
    pub table_size_step_pt: f32,
    /// Space left and right of the text of a cell, in em of the table's
    /// font size.
    pub cell_padding_em: f32,
    /// Space above and below the text of a cell, in em of the table's font
    /// size.
    pub cell_padding_top_em: f32,
    /// The widest a column is kept for its typical cell (the mean width of
    /// its body cells, or its widest word) when the table must wrap its
    /// text, in em: a short label stays on one line; a longer text wraps.
    pub typical_cell_max_em: f32,
    /// A column is at least as wide as the text of its header cells set on
    /// this many lines (up to [`WorkbookPdfConfig::typical_cell_max_em`]):
    /// a header over a column of short values is not set a word a line.
    pub header_max_lines: f32,
    /// A row is kept whole on a page when its height, guessed from its
    /// text, is at most this share of the height of the text; a taller row
    /// is broken across pages between lines.
    pub max_unsplit_row_ratio: f32,
    /// The narrowest a column is made, in em: the room for one CJK
    /// character.
    pub min_column_em: f32,
    /// Width of the rules above and below a table.
    pub heavy_rule_pt: f32,
    /// Width of the rule under a table's header.
    pub light_rule_pt: f32,
    /// A table of at most this many body rows is not broken across pages:
    /// it moves whole to the next page when it does not fit (and fits a
    /// page), as the LaTeX output's tables of up to 30 body rows.
    pub max_unbroken_body_rows: usize,
    /// Space above a table that has no caption above it, in em of the
    /// body's font size.
    pub table_space_em: f32,
    /// The header rows of a table are repeated at the top of each page
    /// only when they take at most this share of the height of the text.
    pub max_head_ratio: f32,
    /// A figure is made smaller to be at most this share of the height of
    /// the text (and at most as wide as the text).
    pub figure_max_height_ratio: f32,
    /// The most cells of tables laid out in one PDF: the rows of a table
    /// past them are left out, with a paragraph saying how many. Laying
    /// out takes some 2 to 3 KB a cell at its peak: the largest sheet of
    /// the corpus (2.1 million cells) took 6.7 GB whole, 1.9 GB cut to
    /// 500,000 cells (1,158 pages); an 18,000-row budget table (144,000
    /// cells) is kept whole.
    pub max_cells: usize,
    /// Space between paragraphs, and between a paragraph and a table or a
    /// figure, in em of the body's font size.
    pub paragraph_space_em: f32,
    /// How the workbook is laid out: printed as Excel prints its sheets
    /// (the default), or as a document.
    pub style: WorkbookPdfStyle,
    /// The most pages of a workbook printed as Excel prints it
    /// ([`WorkbookPdfStyle::Print`]): the pages after are left out. A page
    /// takes some 2 to 3 KB a cell it shows until the PDF is written.
    pub print_max_pages: usize,
}

/// How a workbook is laid out in pages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkbookPdfStyle {
    /// Each sheet as Excel prints it: its print area (or the cells it
    /// uses) cut into pages by its print settings, with the look of every
    /// cell and its charts, pictures and shapes where they are anchored.
    #[default]
    Print,
    /// The workbook as a document: each sheet after its heading, its
    /// tables with rules and their header rows repeated on each page, and
    /// its charts as figures with their data. The fields of
    /// [`WorkbookPdfConfig`] other than `style` and `print_max_pages` are
    /// this layout's.
    Document,
}

impl Default for WorkbookPdfConfig {
    fn default() -> Self {
        Self {
            page_width_pt: 595.28,
            page_height_pt: 841.89,
            margin_pt: 70.87,
            body_size_pt: 11.0,
            heading_size_pt: 14.4,
            subheading_size_pt: 12.0,
            caption_size_pt: 10.0,
            table_size_pt: 10.0,
            min_table_size_pt: 6.0,
            table_size_step_pt: 1.0,
            cell_padding_em: 0.4,
            cell_padding_top_em: 0.1,
            typical_cell_max_em: 10.0,
            header_max_lines: 6.0,
            max_unsplit_row_ratio: 0.25,
            min_column_em: 1.0,
            heavy_rule_pt: 0.8,
            light_rule_pt: 0.5,
            max_unbroken_body_rows: 30,
            table_space_em: 1.0,
            max_head_ratio: 0.25,
            figure_max_height_ratio: 0.8,
            paragraph_space_em: 0.5,
            max_cells: 500_000,
            style: WorkbookPdfStyle::Print,
            print_max_pages: 2_000,
        }
    }
}

/// The families text is set in, chosen among those installed.
#[derive(Debug, Clone)]
struct Families {
    /// Latin and other text.
    serif: String,
    /// CJK text.
    cjk: String,
    /// Monospaced text.
    mono: String,
    /// Sans-serif text.
    sans: String,
}

impl Families {
    /// The first family of each list that `book` has.
    fn choose(book: &FontBook) -> Self {
        let pick = |list: &[&str]| {
            list.iter()
                .find(|name| book.has_family(name))
                .unwrap_or(&list[0])
                .to_string()
        };
        let serif = pick(SERIF_FAMILIES);
        let cjk = CJK_FAMILIES
            .iter()
            .find(|name| book.has_family(name))
            .map_or_else(|| serif.clone(), ToString::to_string);
        Self {
            cjk,
            mono: pick(MONO_FAMILIES),
            sans: pick(SANS_FAMILIES),
            serif,
        }
    }

    /// A plain run of text at `size`.
    fn run(&self, size: f32) -> RunFmt {
        RunFmt {
            font: self.serif.clone(),
            font_hansi: self.serif.clone(),
            font_east_asia: self.cjk.clone(),
            font_cs: self.serif.clone(),
            size,
            size_cs: size,
            ..RunFmt::default()
        }
    }

    /// The run of a span at `size`: its style, colour and link.
    fn span_run(&self, span: &Span, size: f32) -> RunFmt {
        let mut fmt = self.run(size);
        let latin = if span.style & style::MONO != 0 {
            Some(&self.mono)
        } else if span.style & style::SANS != 0 {
            Some(&self.sans)
        } else {
            None
        };
        if let Some(family) = latin {
            fmt.font.clone_from(family);
            fmt.font_hansi.clone_from(family);
        }
        fmt.bold = span.style & style::BOLD != 0;
        fmt.italic = span.style & style::ITALIC != 0;
        fmt.small_caps = span.style & style::SMALL_CAPS != 0;
        fmt.vert = if span.style & style::SUP != 0 {
            VertAlign::Super
        } else if span.style & style::SUB != 0 {
            VertAlign::Sub
        } else {
            VertAlign::Baseline
        };
        if span.color != [0; 3] {
            fmt.color = Some(span.color);
        }
        if let Some(href) = &span.href {
            fmt.href = Some(href.clone());
            fmt.color = Some(LINK_COLOR);
        }
        fmt
    }

    /// The inlines of `spans` at `size`: a line feed is a line break, a
    /// tab a space.
    fn inlines(&self, spans: &[Span], size: f32) -> Vec<Inline> {
        let mut inlines = Vec::new();
        for span in spans {
            let fmt = self.span_run(span, size);
            for (at, line) in span.text.split('\n').enumerate() {
                if at > 0 {
                    inlines.push(Inline::Break {
                        kind: crate::word::fmt::model::BreakKind::Line,
                        fmt: fmt.clone(),
                    });
                }
                let line = line.replace(['\t', '\r'], " ");
                if !line.is_empty() {
                    inlines.push(Inline::Text {
                        text: line,
                        fmt: fmt.clone(),
                    });
                }
            }
        }
        inlines
    }
}

/// Measures text with the fonts of the layout.
struct Measurer {
    /// The fonts, handed on to the layout once the document is built.
    book: FontBook,
    /// The shaper of the measures.
    shaper: Shaper,
    /// An empty document, which paragraphs are measured in.
    doc: FDoc,
    /// No notes.
    notes: HashMap<(bool, String), String>,
    /// Faces that cover each character, cached.
    coverage: HashMap<(super::fonts::FaceId, char), bool>,
}

impl Measurer {
    /// The widths of `para` (see [`para::measure`]): on one line, of its
    /// widest word and of all its lines; and its number of line breaks.
    fn measure(&mut self, para: &Para) -> (f32, f32, f32, usize) {
        let mut ctx = Ctx {
            book: &mut self.book,
            shaper: &mut self.shaper,
            doc: &self.doc,
            notes: &self.notes,
            own_note: None,
            page: None,
            page_count: None,
            section_pages: None,
            coverage: &mut self.coverage,
            page_refs: None,
            origin: 0.0,
        };
        para::measure(&mut ctx, para)
    }
}

/// The page setups of the document: portrait and landscape.
#[derive(Debug, Clone, Copy)]
struct Pages {
    /// Portrait pages.
    portrait: PageSetup,
    /// The same pages turned.
    landscape: PageSetup,
}

impl Pages {
    /// Pages of `paper` (width, height, in either orientation) with the
    /// margins of `config`.
    fn new(paper: (f32, f32), config: &WorkbookPdfConfig) -> Self {
        let (short, long) = (paper.0.min(paper.1), paper.0.max(paper.1));
        let setup = |width: f32, height: f32| PageSetup {
            width,
            height,
            top: config.margin_pt,
            bottom: config.margin_pt,
            left: config.margin_pt,
            right: config.margin_pt,
            header: config.margin_pt / 2.0,
            footer: config.margin_pt / 2.0,
            gutter: 0.0,
        };
        Self {
            portrait: setup(short, long),
            landscape: setup(long, short),
        }
    }

    /// Width of the text on a page of `setup`.
    fn text_width(setup: &PageSetup) -> f32 {
        (setup.width - setup.left - setup.right).max(1.0)
    }

    /// Height of the text on a page of `setup`.
    fn text_height(setup: &PageSetup) -> f32 {
        (setup.height - setup.top - setup.bottom).max(1.0)
    }
}

/// Builds the sections of the formatted document.
struct Builder<'a> {
    /// The thresholds.
    config: &'a WorkbookPdfConfig,
    /// The families of the text.
    families: Families,
    /// Measures text.
    measurer: Measurer,
    /// The page setups.
    pages: Pages,
    /// The document's pictures.
    pictures: &'a [Picture],
    /// The sections done.
    sections: Vec<Section>,
    /// The blocks of the section being filled.
    blocks: Vec<Block>,
    /// The section being filled is on landscape pages.
    landscape: bool,
    /// How many more cells of tables may be laid out
    /// ([`WorkbookPdfConfig::max_cells`]).
    cells_left: usize,
}

/// Lays out `doc`, a document without pages, on pages of `paper` (width
/// and height in points, from the workbook's print settings; `None` for
/// those of `config`) and writes it as a PDF.
///
/// # Errors
/// When the PDF cannot be written.
pub fn to_pdf(doc: &Document, paper: Option<(f32, f32)>, config: &WorkbookPdfConfig) -> Result<Vec<u8>> {
    let mut book = FontBook::new(&[], &HashMap::new());
    book.require_font()?;
    let families = Families::choose(&book);
    let mut builder = Builder {
        config,
        families,
        measurer: Measurer {
            book,
            shaper: Shaper::default(),
            doc: FDoc::default(),
            notes: HashMap::new(),
            coverage: HashMap::new(),
        },
        pages: Pages::new(paper.unwrap_or((config.page_width_pt, config.page_height_pt)), config),
        pictures: &doc.pictures,
        sections: Vec::new(),
        blocks: Vec::new(),
        landscape: false,
        cells_left: config.max_cells,
    };
    for block in &doc.blocks {
        builder.block(block);
    }
    let (fdoc, mut book) = builder.finish(doc);
    super::to_pdf_with(&fdoc, &mut book)
}

impl Builder<'_> {
    /// Adds a block of the document.
    fn block(&mut self, block: &DocBlock) {
        match &block.kind {
            BlockKind::Heading { level } => {
                self.orientation(false);
                self.heading(block, *level);
            }
            BlockKind::Title => {
                self.orientation(false);
                let size = self.config.heading_size_pt * 1.2;
                let mut para = self.text_para(&block.spans, size, Align::Center);
                para.props.after = size;
                bold(&mut para);
                self.blocks.push(Block::Para(Box::new(para)));
            }
            BlockKind::Figure { image, .. } => self.figure(block, image.as_ref()),
            BlockKind::ListItem { marker, level } => {
                let mut spans = vec![Span::plain(format!("{} ", marker.as_deref().unwrap_or("•")))];
                spans.extend(block.spans.iter().cloned());
                self.indented(&spans, *level + 1);
            }
            BlockKind::ListContinuation => self.indented(&block.spans, 1),
            BlockKind::Code | BlockKind::Authors => {
                let rows: Vec<Vec<Span>> = if block.rows.is_empty() {
                    vec![block.spans.clone()]
                } else {
                    block.rows.clone()
                };
                let mut spans = Vec::new();
                for (at, row) in rows.into_iter().enumerate() {
                    if at > 0 {
                        spans.push(Span::plain("\n"));
                    }
                    let code = matches!(block.kind, BlockKind::Code);
                    spans.extend(row.into_iter().map(|mut span| {
                        if code {
                            span.style |= style::MONO;
                        }
                        span
                    }));
                }
                self.paragraph(&spans, self.config.body_size_pt);
            }
            BlockKind::Footnote { .. } | BlockKind::Note => self.paragraph(&block.spans, self.config.caption_size_pt),
            _ => self.paragraph(&block.spans, self.config.body_size_pt),
        }
    }

    /// Goes on in a section of portrait (or `landscape`) pages, starting
    /// one when the orientation changes, with the headings that end the
    /// section before.
    fn orientation(&mut self, landscape: bool) {
        if self.landscape == landscape {
            return;
        }
        // The headings right before go along, so that none is left alone
        // at the foot of the section before.
        let mut carried = Vec::new();
        while matches!(self.blocks.last(), Some(Block::Para(para)) if para.props.outline.is_some()) {
            carried.extend(self.blocks.pop());
        }
        self.close_section();
        self.landscape = landscape;
        self.blocks.extend(carried.into_iter().rev());
    }

    /// Ends the section being filled, unless it has nothing.
    fn close_section(&mut self) {
        if self.blocks.is_empty() {
            return;
        }
        let page = if self.landscape {
            self.pages.landscape
        } else {
            self.pages.portrait
        };
        self.sections.push(Section {
            page,
            columns: crate::word::fmt::model::Columns {
                count: 1,
                ..Default::default()
            },
            start: SectionStart::NextPage,
            footers: HeaderSet {
                default: Some(0),
                ..Default::default()
            },
            blocks: std::mem::take(&mut self.blocks),
            ..Default::default()
        });
    }

    /// The width of the text of the pages being filled.
    fn text_width(&self) -> f32 {
        Pages::text_width(if self.landscape {
            &self.pages.landscape
        } else {
            &self.pages.portrait
        })
    }

    /// A paragraph of `spans` at `size`, aligned `align`, with no space
    /// around it.
    fn text_para(&self, spans: &[Span], size: f32, align: Align) -> Para {
        Para {
            props: ParaProps {
                align,
                line: LineSpacing::Auto(1.0),
                widow_control: true,
                ..Default::default()
            },
            mark: self.families.run(size),
            inlines: self.families.inlines(spans, size),
            ..Default::default()
        }
    }

    /// A paragraph of running text at `size`.
    fn paragraph(&mut self, spans: &[Span], size: f32) {
        if spans.iter().all(|span| span.text.trim().is_empty()) {
            return;
        }
        let mut para = self.text_para(spans, size, Align::Left);
        para.props.after = self.config.paragraph_space_em * self.config.body_size_pt;
        self.blocks.push(Block::Para(Box::new(para)));
    }

    /// A paragraph indented to the list level `level`.
    fn indented(&mut self, spans: &[Span], level: u8) {
        let size = self.config.body_size_pt;
        let mut para = self.text_para(spans, size, Align::Left);
        para.props.left = f32::from(level) * LIST_INDENT_EM * size;
        para.props.after = self.config.paragraph_space_em * size / 2.0;
        self.blocks.push(Block::Para(Box::new(para)));
    }

    /// A heading of level `level`, kept with what follows.
    fn heading(&mut self, block: &DocBlock, level: u8) {
        let size = if level <= 1 {
            self.config.heading_size_pt
        } else {
            self.config.subheading_size_pt
        };
        let mut para = self.text_para(&block.spans, size, Align::Left);
        bold(&mut para);
        para.props.before = size;
        para.props.after = size / 2.0;
        para.props.keep_next = true;
        para.props.keep_lines = true;
        para.props.outline = Some(level.saturating_sub(1));
        if let Some(anchor) = &block.anchor {
            para.bookmarks.push(anchor.clone());
        }
        self.blocks.push(Block::Para(Box::new(para)));
    }

    /// The caption paragraph of a float, kept with what follows when it is
    /// above.
    fn caption(&self, spans: &[Span], above: bool) -> Option<Para> {
        if spans.iter().all(|span| span.text.trim().is_empty()) {
            return None;
        }
        let space = self.config.paragraph_space_em * self.config.body_size_pt;
        let mut para = self.text_para(spans, self.config.caption_size_pt, Align::Left);
        if above {
            para.props.before = space;
            para.props.after = space / 2.0;
            para.props.keep_next = true;
        } else {
            para.props.before = space / 2.0;
            para.props.after = space;
        }
        para.props.keep_lines = true;
        Some(para)
    }

    /// A float: a table (see [`table`]) or a picture, with its caption.
    fn figure(&mut self, block: &DocBlock, image: Option<&ImageSpec>) {
        let above = block.caption_above;
        if let Some(model) = block.table.as_ref().filter(|table| !table.rows.is_empty()) {
            return self.table(block, model);
        }
        let caption = self.caption(&block.spans, above);
        if caption.is_none() && image.is_none_or(|spec| spec.embedded.is_none()) {
            return;
        }
        // A figure is on portrait pages, as the text.
        self.orientation(false);
        let picture = image.and_then(|spec| self.picture_para(spec, caption.is_some() && !above));
        let mut parts = Vec::new();
        if above {
            parts.extend(caption.clone());
        }
        parts.extend(picture);
        if !above {
            parts.extend(caption);
        }
        self.blocks
            .extend(parts.into_iter().map(|para| Block::Para(Box::new(para))));
    }

    /// The centred paragraph of a stored picture at its size, made smaller
    /// to fit the text; kept with the next paragraph when `keep_next` (a
    /// caption below it). `None` for a picture that is not stored.
    fn picture_para(&self, spec: &ImageSpec, keep_next: bool) -> Option<Para> {
        let index = spec.embedded.filter(|&index| index < self.pictures.len())?;
        let (width, height) = (spec.rect.width().max(1.0), spec.rect.height().max(1.0));
        let page = if self.landscape {
            &self.pages.landscape
        } else {
            &self.pages.portrait
        };
        let max_height = Pages::text_height(page) * self.config.figure_max_height_ratio;
        let scale = (self.text_width() / width).min(max_height / height).min(1.0);
        let space = self.config.paragraph_space_em * self.config.body_size_pt;
        let fmt = self.families.run(self.config.body_size_pt);
        Some(Para {
            props: ParaProps {
                align: Align::Center,
                line: LineSpacing::Auto(1.0),
                before: space,
                after: space,
                keep_next,
                ..Default::default()
            },
            mark: fmt.clone(),
            inlines: vec![Inline::Image {
                index: Some(index),
                width: width * scale,
                height: height * scale,
                anchor: None,
                fmt,
                alt: String::new(),
                crop: [0.0; 4],
            }],
            ..Default::default()
        })
    }

    /// A table with its caption: on the pages being filled when it fits
    /// them, else on landscape pages (see [`table::fit`]).
    fn table(&mut self, block: &DocBlock, model: &crate::model::Table) {
        let (kept, left_out) = self.cell_budget(model);
        let truncated;
        let model = if left_out > 0 {
            let first = |len: usize| kept.min(len);
            truncated = crate::model::Table {
                n_cols: model.n_cols,
                header_rows: model.header_rows,
                col_align: model.col_align.clone(),
                rows: model.rows[..kept].to_vec(),
                rule_above: model.rule_above[..first(model.rule_above.len())].to_vec(),
                rule_bottom: model.rule_bottom.clone(),
                rule_left: model.rule_left[..first(model.rule_left.len())].to_vec(),
                rule_right: model.rule_right[..first(model.rule_right.len())].to_vec(),
                geom: model.geom.clone(),
            };
            &truncated
        } else {
            model
        };
        let portrait = Pages::text_width(&self.pages.portrait);
        let landscape = Pages::text_width(&self.pages.landscape);
        let height = Pages::text_height(&self.pages.landscape);
        let laid = table::lay(self, model, (portrait, landscape), height);
        self.orientation(laid.landscape);
        let above = block.caption_above;
        let caption = self.caption(&block.spans, above);
        if above && let Some(caption) = caption.clone() {
            self.blocks.push(Block::Para(Box::new(caption)));
        }
        for (at, table) in laid.tables.into_iter().enumerate() {
            // A paragraph of no text before each table, kept with it: the
            // space above, which brings the header and the first row along
            // to a new page when they do not fit (the layout keeps a
            // paragraph with the first rows of the table after it).
            let gap = if at == 0 && above && caption.is_some() {
                0.0
            } else {
                self.config.table_space_em * self.config.body_size_pt
            };
            self.blocks.push(Block::Para(Box::new(self.spacer(gap))));
            self.blocks.push(Block::Table(table));
        }
        if !above && let Some(caption) = caption {
            self.blocks.push(Block::Para(Box::new(caption)));
        }
        if left_out > 0 {
            let text = if left_out == 1 {
                "1 more row is not shown in the PDF.".to_string()
            } else {
                format!("{left_out} more rows are not shown in the PDF.")
            };
            self.paragraph(&[Span::plain(text)], self.config.body_size_pt);
        }
    }

    /// How many rows of `model` are laid out, within what is left of
    /// [`WorkbookPdfConfig::max_cells`] (at least its header and one row),
    /// and how many are left out; takes their cells from what is left.
    fn cell_budget(&mut self, model: &crate::model::Table) -> (usize, usize) {
        let least = (model.header_rows + 1).min(model.rows.len());
        let mut kept = 0;
        for row in &model.rows {
            if kept >= least && row.len() > self.cells_left {
                break;
            }
            self.cells_left = self.cells_left.saturating_sub(row.len());
            kept += 1;
        }
        (kept, model.rows.len() - kept)
    }

    /// An empty paragraph `height` high, kept with the next block.
    fn spacer(&self, height: f32) -> Para {
        let mut mark = self.families.run(1.0);
        mark.size = 1.0;
        Para {
            props: ParaProps {
                line: LineSpacing::Exact(height.max(0.01)),
                keep_next: true,
                ..Default::default()
            },
            mark,
            ..Default::default()
        }
    }

    /// The formatted document, with the fonts it was measured with.
    fn finish(mut self, doc: &Document) -> (FDoc, FontBook) {
        if self.blocks.is_empty() && self.sections.is_empty() {
            // An empty document: one empty page.
            self.blocks.push(Block::Para(Box::new(self.spacer(0.0))));
        }
        self.close_section();
        let size = self.config.body_size_pt;
        let footer = Para {
            props: ParaProps {
                align: Align::Center,
                line: LineSpacing::Auto(1.0),
                ..Default::default()
            },
            mark: self.families.run(size),
            inlines: vec![Inline::Field {
                kind: FieldKind::Page,
                fmt: self.families.run(size),
                format: None,
            }],
            ..Default::default()
        };
        let images = doc
            .pictures
            .iter()
            .map(|picture| FImage {
                data: picture.data.clone(),
                format: picture.format,
                bare: None,
                texts: Vec::new(),
            })
            .collect();
        let fdoc = FDoc {
            sections: self.sections,
            parts: vec![vec![Block::Para(Box::new(footer))]],
            images,
            default_tab: DEFAULT_TAB_PT,
            title: doc.meta.title.clone(),
            compat_mode: COMPAT_MODE,
            ..Default::default()
        };
        (fdoc, self.measurer.book)
    }
}

/// Makes every run of `para` bold.
fn bold(para: &mut Para) {
    para.mark.bold = true;
    for inline in &mut para.inlines {
        if let Inline::Text { fmt, .. } | Inline::Break { fmt, .. } = inline {
            fmt.bold = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FloatKind, Meta, Table, TableCell};

    /// A document of a heading and a table of `rows` rows of `cols`
    /// columns, the first row its header.
    fn doc_of(rows: usize, cols: usize, text: impl Fn(usize, usize) -> String) -> Document {
        let mut heading = DocBlock::new(0, BlockKind::Heading { level: 1 });
        heading.spans = vec![Span::plain("Sheet")];
        let cells = (0..rows)
            .map(|row| {
                (0..cols)
                    .map(|col| TableCell {
                        col,
                        colspan: 1,
                        rowspan: 1,
                        spans: vec![Span::plain(text(row, col))],
                    })
                    .collect()
            })
            .collect();
        let mut figure = DocBlock::new(
            1,
            BlockKind::Figure {
                kind: FloatKind::Table,
                image: None,
                label: None,
            },
        );
        figure.caption_above = true;
        figure.spans = vec![Span::plain("Table 1: Numbers")];
        figure.table = Some(Table::ruled(cells, cols, 1, vec!["right"; cols]));
        Document {
            meta: Meta::default(),
            pages: Vec::new(),
            body_size: 11.0,
            leading: 13.2,
            body_font: String::new(),
            math_font: String::new(),
            blocks: vec![heading, figure],
            pictures: Vec::new(),
        }
    }

    /// The text of the pages of a PDF, page by page.
    fn page_texts(pdf: Vec<u8>) -> Vec<String> {
        let (lines, count) = crate::layout::lines::pdf_lines(pdf).expect("a PDF");
        let mut pages = vec![String::new(); count];
        for line in lines {
            pages[line.page].push_str(&line.text);
            pages[line.page].push('\n');
        }
        pages
    }

    #[test]
    fn long_table_repeats_its_header_on_each_page() {
        let doc = doc_of(200, 3, |row, col| {
            if row == 0 {
                format!("Head{col}")
            } else {
                format!("{}", row * 10 + col)
            }
        });
        let pages = page_texts(to_pdf(&doc, None, &WorkbookPdfConfig::default()).expect("pdf"));
        assert!(pages.len() > 2, "{} pages", pages.len());
        for page in &pages {
            assert!(page.contains("Head0"), "a page without the header: {page}");
        }
        assert!(pages.last().is_some_and(|page| page.contains("1992")));
    }

    #[test]
    fn cells_past_the_limit_are_left_out_with_a_note() {
        let doc = doc_of(10, 3, |row, col| format!("v{row}x{col}"));
        let config = WorkbookPdfConfig {
            max_cells: 10,
            ..Default::default()
        };
        let text = page_texts(to_pdf(&doc, None, &config).expect("pdf")).concat();
        assert!(text.contains("v2x2") && !text.contains("v3x0"), "{text}");
        assert!(text.contains("7 more rows are not shown in the PDF."), "{text}");
    }

    #[test]
    fn wide_table_keeps_every_column() {
        let cols = 40;
        let doc = doc_of(5, cols, |row, col| format!("r{row}c{col}value"));
        let pdf = to_pdf(&doc, None, &WorkbookPdfConfig::default()).expect("pdf");
        let text: String = page_texts(pdf).concat();
        for col in 0..cols {
            assert!(text.contains(&format!("r4c{col}value")), "column {col} is missing");
        }
    }
}
