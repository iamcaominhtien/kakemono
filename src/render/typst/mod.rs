//! Typst output: the document as one `.typ` file that compiles with
//! `typst compile`, built from the same [`Document`] as the HTML modes, in
//! the layout of an article or of a printed book.
//!
//! The file has three parts: the settings (layout, page size, fonts,
//! language, cover), which a reader can change; the template, the Typst
//! functions that lay the document out; and the text, which calls them.
//! Headings are Typst headings, rebuilt tables `table`s, formulas whose
//! structure is known Typst math, the added notes (a translator's) Typst
//! footnotes; figures (and the other formulas) are pictures of their area
//! of the page, written to an image folder next to the file.
//!
//! The book layout is that of a printed book: chapters open on a right-hand
//! page and number their notes from 1, the margins are mirrored, the
//! running heads show the title of the book and of the chapter. Typst
//! breaks each paragraph as a whole and keeps a footnote on the page of its
//! call, which is what such a book needs.
//!
//! The text keeps the order of the PDF: figures and tables are not floats,
//! and their captions keep the label of the paper ("Figure 3:").

mod math;
mod text;

use super::markdown::{linked_anchors, strip_label};
use super::sequence::{AuthorMarkup, Step, author_lines, linked_anchor, push_line, steps, title_spans, write_authors};
use super::{
    CAP_LABEL, Equations, HtmlOptions, ImageStore, Mode, PageImage, caption_first, code, doc_title, equation_math,
    float_class, is_abstract_text, is_scene_break, plain,
};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::{Block, BlockKind, Document, FloatKind, ImageSpec, Span, Table};
use crate::render::Pictures;
use math::{string, typst};
use text::{CALL_END, Text, end_calls, escape, label, protect_start};

/// The functions that lay the document out.
const TEMPLATE: &str = include_str!("template.typ");
/// Indentation step of nested lists, in levels (the template turns a level
/// into a width).
const MAX_LIST_NESTING: u8 = 5;
/// Size of the text of an article.
const ARTICLE_FONT_SIZE: &str = "11pt";
/// Size of the text of a book.
const BOOK_FONT_SIZE: &str = "10.5pt";
/// Page size of an article.
const ARTICLE_PAPER: &str = "\"a4\"";
/// Page size of a book: A5.
const BOOK_PAPER: &str = "(width: 148mm, height: 210mm)";
/// Fonts of the text, in order: Typst takes each character from the first
/// that has it. Libertinus Serif comes with Typst and has the Vietnamese
/// letters.
const FONTS: &str = "(\"Libertinus Serif\", \"New Computer Modern\")";
/// Fewest characters of the opening words of a chapter, set in small
/// capitals in a book.
const OPENING_MIN_CHARS: usize = 10;
/// Most words of the opening words of a chapter.
const OPENING_MAX_WORDS: usize = 3;
/// Share of the letters of a document that only Vietnamese has (ơ, ư, ă, đ
/// and the letters with two accents) from which its language is
/// Vietnamese.
const VIETNAMESE_LETTER_RATIO: f32 = 0.02;
/// Fewest chapters (headings of the outermost level) of a book.
const BOOK_MIN_CHAPTERS: usize = 2;

/// The layout of a Typst document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypstLayout {
    /// One column of text on A4 pages, the title at the top of the first
    /// one, the page number at the foot.
    Article,
    /// A printed book (A5 pages): a title page, chapters that open on a
    /// right-hand page and number their notes from 1, mirrored margins,
    /// running heads.
    Book,
}

/// Everything that controls how a document is written as Typst.
#[derive(Debug, Clone)]
pub struct TypstOptions {
    /// Directory (relative to the `.typ` file) of the picture files. Typst
    /// cannot embed pictures in its source: they are always files.
    /// Characters other than ASCII letters, digits, `.`, `_`, `-` and `/`
    /// are replaced by `_`.
    pub asset_dir: String,
    /// Pixels per PDF point of the pictures of figures and formulas.
    pub scale: f32,
    /// Worker threads for drawing the pictures (0 = one per CPU).
    pub jobs: usize,
    /// How displayed formulas are written: [`Equations::MathMl`] gives Typst
    /// math rebuilt from the glyphs where the structure of the formula is
    /// clear (and inside the text), pictures elsewhere.
    pub equations: Equations,
    /// The layout; `None` chooses it from the document: a book when it has
    /// chapters and no abstract, references or arXiv identifier, an
    /// article otherwise. It is a setting at the top of the file, which can
    /// be changed there.
    pub layout: Option<TypstLayout>,
}

impl Default for TypstOptions {
    /// Pictures written to `images/` at 3 pixels per point, formulas as
    /// Typst math where their structure is clear, the layout chosen from
    /// the document, one worker thread per CPU.
    fn default() -> Self {
        Self {
            asset_dir: "images".into(),
            scale: 3.0,
            jobs: 0,
            equations: Equations::MathMl,
            layout: None,
        }
    }
}

/// The result of the Typst output: the source and the picture files it
/// refers to.
pub struct TypstOutput {
    /// The Typst document (`.typ`).
    pub typst: String,
    /// Files to write next to the `.typ` file: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

/// Writes an analysed document as Typst.
///
/// The text is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and returned
/// in [`TypstOutput::files`].
pub fn render_typst(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &TypstOptions) -> TypstOutput {
    let html_opts = HtmlOptions {
        mode: Mode::Friendly,
        inline_images: false,
        asset_dir: safe_path(&opts.asset_dir),
        scale: opts.scale,
        page_image: PageImage::Svg,
        jobs: opts.jobs,
        embed_fonts: false,
        equations: opts.equations,
    };
    let mut images = ImageStore {
        opts: &html_opts,
        jobs: Vec::new(),
        vector: false,
    };
    let doc = &analysis.doc;
    let anchors = linked_anchors(doc);
    let mut writer = Writer {
        opts,
        text: Text {
            anchors: &anchors,
            math: opts.equations == Equations::MathMl,
        },
        images: &mut images,
        out: String::new(),
        top: top_level(doc),
        abstract_title: None,
        in_abstract: false,
        list_level: 0,
        opening: false,
    };
    writer.write(doc);
    let body = end_calls(writer.out.trim_end());
    let out = images.finish(Pictures::new(pdf, &analysis.doc), body);
    let layout = opts.layout.unwrap_or_else(|| choose_layout(doc));
    TypstOutput {
        typst: document(doc, layout, &out.html),
        files: out.files,
    }
}

/// The whole `.typ` file: the settings, the template and `body`.
fn document(doc: &Document, layout: TypstLayout, body: &str) -> String {
    let source = match super::source_label(doc) {
        "EPUB" => "an EPUB book",
        "Word" => "a Word document",
        _ => "a PDF",
    };
    let (name, size, paper) = match layout {
        TypstLayout::Article => ("article", ARTICLE_FONT_SIZE, ARTICLE_PAPER),
        TypstLayout::Book => ("book", BOOK_FONT_SIZE, BOOK_PAPER),
    };
    let title = book_title(doc);
    format!(
        "// Converted from {source} by kakemono. Compile it with `typst compile`;\n\
// the pictures are files in a folder next to this one.\n\n\
// Settings: change them here.\n\
#let doc-layout = \"{name}\" // \"book\" (a title page, chapters on right-hand pages, running heads) or \"article\"\n\
#let paper = {paper} // \"a4\", \"a5\", (width: 110mm, height: 180mm) for a phone, ...\n\
#let fonts = {FONTS} // each character from the first font that has it\n\
#let font-size = {size}\n\
#let lang = \"{lang}\" // hyphenation and quotation marks\n\
#let doc-title = {title}\n\
#let cover = none // a picture for the cover of a book, such as \"cover.jpg\"\n\n\
{TEMPLATE}\n\
#show: kakemono\n\n\
{body}\n",
        lang = language(doc),
        title = string(&title),
    )
}

/// The title of the document as one line: the text of its title blocks, or
/// the title of the PDF.
fn book_title(doc: &Document) -> String {
    let parts: Vec<String> = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Title))
        .map(|block| plain(&block.spans))
        .filter(|text| !text.is_empty())
        .collect();
    if parts.is_empty() {
        doc_title(doc)
    } else {
        parts.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
    }
}

/// The language of the document, as Typst names it: Vietnamese when enough
/// of its letters are Vietnamese ones, English otherwise.
fn language(doc: &Document) -> &'static str {
    let (mut letters, mut vietnamese) = (0usize, 0usize);
    for block in &doc.blocks {
        for ch in block.spans.iter().flat_map(|span| span.text.chars()) {
            if ch.is_alphabetic() {
                letters += 1;
                if is_vietnamese(ch) {
                    vietnamese += 1;
                }
            }
        }
    }
    if letters > 0 && vietnamese as f32 >= VIETNAMESE_LETTER_RATIO * letters as f32 {
        "vi"
    } else {
        "en"
    }
}

/// Whether `ch` is a letter that only Vietnamese uses among the common
/// languages written in Latin letters.
fn is_vietnamese(ch: char) -> bool {
    matches!(
        ch,
        'ă' | 'Ă' | 'đ' | 'Đ' | 'ơ' | 'Ơ' | 'ư' | 'Ư' | '\u{1ea0}'..='\u{1ef9}'
    )
}

/// The layout a document calls for: a book when it has chapters (at least
/// [`BOOK_MIN_CHAPTERS`] headings of the outermost level) and nothing of a
/// paper (an arXiv identifier, an abstract, references); an article
/// otherwise.
fn choose_layout(doc: &Document) -> TypstLayout {
    let paper = doc.meta.arxiv_id.is_some()
        || doc.blocks.iter().any(|block| match &block.kind {
            BlockKind::Paragraph { in_abstract } => *in_abstract,
            BlockKind::Reference { .. } => true,
            BlockKind::Heading { .. } => is_abstract_text(&block.spans),
            _ => false,
        });
    let top = top_level(doc);
    let chapters = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Heading { level } if Some(level) == top))
        .count();
    if !paper && chapters >= BOOK_MIN_CHAPTERS {
        TypstLayout::Book
    } else {
        TypstLayout::Article
    }
}

/// The outermost level of the headings of the document (an "Abstract"
/// heading apart), which Typst writes as `=`.
fn top_level(doc: &Document) -> Option<u8> {
    doc.blocks
        .iter()
        .filter_map(|block| match block.kind {
            BlockKind::Heading { level } if !is_abstract_text(&block.spans) => Some(level),
            _ => None,
        })
        .min()
}

/// `dir` with the characters that could break a Typst string or a file
/// name replaced by `_`, and without a trailing slash.
fn safe_path(dir: &str) -> String {
    dir.trim_end_matches(['/', '\\'])
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Writes the Typst of a document, block by block.
struct Writer<'a, 'r> {
    /// The output options.
    opts: &'a TypstOptions,
    /// Writes the text of the blocks.
    text: Text<'a>,
    /// The pictures; each file name is a placeholder until the end.
    images: &'a mut ImageStore<'r>,
    /// The Typst so far.
    out: String,
    /// The outermost heading level (see [`top_level`]).
    top: Option<u8>,
    /// The text of the heading of the abstract, written with it.
    abstract_title: Option<String>,
    /// The abstract is open (`#kakemono-abstract(…)[` written).
    in_abstract: bool,
    /// Nesting level of the list being written (0 outside lists).
    list_level: u8,
    /// The next block opens a chapter: when it is a paragraph, its first
    /// words are marked.
    opening: bool,
}

impl Writer<'_, '_> {
    /// Writes every block of `doc`, in the steps of [`steps`]. The entries
    /// of a table of contents become Typst's own table of contents.
    fn write(&mut self, doc: &Document) {
        let blocks: Vec<&Block> = doc.blocks.iter().collect();
        let steps = steps(&blocks, doc.meta.arxiv_id.is_some());
        for (i, step) in steps.iter().enumerate() {
            let first = match step {
                Step::Source => None,
                Step::Code(run) | Step::Title(run) | Step::Authors(run) | Step::Contents(run) => Some(run[0]),
                Step::AbstractHeading(block) | Step::NumberedEquation(block, _) | Step::Block(block) => Some(*block),
            };
            if let Some(block) = first {
                let in_abstract = match &block.kind {
                    BlockKind::Paragraph { in_abstract } => *in_abstract,
                    BlockKind::Equation { .. } => self.in_abstract,
                    _ => false,
                };
                self.set_abstract(in_abstract);
            }
            match step {
                Step::Source => self.arxiv_link(doc),
                Step::Code(run) => {
                    self.anchor(run[0]);
                    self.code(&code::listing_lines(run));
                }
                Step::Title(run) => self.title(doc, run),
                Step::Authors(run) => write_authors(self, run),
                Step::AbstractHeading(block) => {
                    self.anchor(block);
                    self.abstract_title = Some(self.text.spans(&block.spans));
                }
                Step::NumberedEquation(block, number) => {
                    if let BlockKind::Equation { image, .. } = &block.kind {
                        self.equation(block, image.as_ref(), Some(number));
                    }
                }
                Step::Contents(run) => {
                    self.anchor(run[0]);
                    self.paragraph("#kakemono-contents()");
                }
                // The heading of a table of contents stays out of it.
                Step::Block(block)
                    if let BlockKind::Heading { level } = block.kind
                        && matches!(steps.get(i + 1), Some(Step::Contents(_))) =>
                {
                    self.heading(block, level, false);
                }
                Step::Block(block) => self.block(block),
            }
        }
        self.set_abstract(false);
    }

    /// Opens or closes the abstract.
    fn set_abstract(&mut self, open: bool) {
        if open == self.in_abstract {
            return;
        }
        if open {
            let title = self.abstract_title.take().unwrap_or_else(|| "Abstract".to_string());
            self.out.push_str(&format!("#kakemono-abstract([{title}])[\n"));
        } else {
            self.out.push_str("]\n\n");
        }
        self.in_abstract = open;
    }

    /// Writes one block.
    fn block(&mut self, block: &Block) {
        if !matches!(block.kind, BlockKind::ListItem { .. } | BlockKind::ListContinuation) {
            self.list_level = 0;
        }
        let opening = std::mem::take(&mut self.opening);
        match &block.kind {
            BlockKind::Title => self.title_text(&self.text.spans(&block.spans)),
            BlockKind::Authors => write_authors(self, std::slice::from_ref(&block)),
            BlockKind::Note => self.text_paragraph(block, false),
            BlockKind::Heading { level } => self.heading(block, *level, true),
            BlockKind::Paragraph { .. } if is_scene_break(&block.spans) => {
                self.anchor(block);
                self.paragraph("#kakemono-break()");
            }
            BlockKind::Paragraph { .. } => self.text_paragraph(block, opening),
            BlockKind::ListItem { marker, level } => self.list_item(block, marker.as_deref(), *level),
            BlockKind::ListContinuation => self.list_continuation(block),
            BlockKind::Reference { .. } => self.call_paragraph(block, "kakemono-reference", &block.spans),
            BlockKind::Footnote { marker } => self.footnote(block, marker),
            BlockKind::Equation { image, number } => self.equation(block, image.as_ref(), number.as_deref()),
            BlockKind::Figure { kind, image, .. } => self.figure(block, *kind, image.as_ref()),
            BlockKind::ContentsEntry { .. } => self.paragraph("#kakemono-contents()"),
            BlockKind::Code => {
                self.anchor(block);
                let lines: Vec<String> = block
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|span| span.text.as_str()).collect())
                    .collect();
                self.code(&lines);
            }
        }
    }

    /// Writes `text` as a block, followed by a blank line.
    fn paragraph(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push_str("\n\n");
    }

    /// The label that marks the place of a block that some text links to
    /// (`#metadata(none) <k-…>`, which the links find), or nothing.
    fn anchor_label(&self, block: &Block) -> String {
        linked_anchor(block, self.text.anchors)
            .map(|anchor| format!("#metadata(none)<{}>", label(anchor)))
            .unwrap_or_default()
    }

    /// Writes the label of a block that some text links to, on a line of
    /// its own.
    fn anchor(&mut self, block: &Block) {
        let label = self.anchor_label(block);
        push_line(&mut self.out, &label);
    }

    /// Writes a paragraph of running text, with the label of its block. The
    /// first paragraph of a chapter (`opening`) has its first words marked.
    fn text_paragraph(&mut self, block: &Block, opening: bool) {
        let text = if opening {
            self.opening_text(&block.spans)
        } else {
            self.text.spans(&block.spans)
        };
        if text.is_empty() {
            self.anchor(block);
            return;
        }
        let label = self.anchor_label(block);
        if label.is_empty() {
            self.paragraph(&text);
        } else {
            self.paragraph(&format!("{label} {text}"));
        }
    }

    /// The text of the first paragraph of a chapter: its first words (when
    /// the paragraph starts with plain text) in `#kakemono-opening[…]`.
    fn opening_text(&self, spans: &[Span]) -> String {
        let Some(first) = spans
            .first()
            .filter(|span| span.style == 0 && span.href.is_none() && span.note.is_none())
        else {
            return self.text.spans(spans);
        };
        let lead = first.text.trim_start();
        let mut end = 0;
        let mut words = 0;
        for (idx, ch) in lead.char_indices().chain(std::iter::once((lead.len(), ' '))) {
            if ch.is_whitespace() && idx > end {
                words += 1;
                let taken = lead[..idx].chars().count();
                end = idx;
                if taken >= OPENING_MIN_CHARS || words >= OPENING_MAX_WORDS {
                    break;
                }
            }
        }
        let words = lead[..end].trim_end();
        if words.is_empty() {
            return self.text.spans(spans);
        }
        let mut rest = spans.to_vec();
        rest[0].text = lead[end..].to_string();
        let gap = if rest[0].text.starts_with(char::is_whitespace) {
            " "
        } else {
            ""
        };
        format!(
            "#kakemono-opening[{}]{CALL_END}{gap}{}",
            protect_start(&escape(words)),
            self.text.spans(&rest)
        )
        .trim_end()
        .to_string()
    }

    /// Writes a block as a call of `function` with its text as the
    /// content, unless the text is empty.
    fn call_paragraph(&mut self, block: &Block, function: &str, spans: &[Span]) {
        self.anchor(block);
        let text = self.text.spans(spans);
        if !text.is_empty() {
            self.paragraph(&format!("#{function}[{text}]"));
        }
    }

    /// Writes a link to the arXiv page of the paper, after the front matter.
    fn arxiv_link(&mut self, doc: &Document) {
        if let Some(id) = &doc.meta.arxiv_id {
            let url = format!("https://arxiv.org/abs/{id}");
            self.paragraph(&format!(
                "#kakemono-source(link({})[arXiv:{}])",
                string(&url),
                escape(id)
            ));
        }
    }

    /// Writes a run of title blocks as one title.
    fn title(&mut self, doc: &Document, run: &[&Block]) {
        for block in run {
            self.anchor(block);
        }
        let text = self.text.spans(&title_spans(run));
        if text.is_empty() {
            self.title_text(&protect_start(&escape(&doc_title(doc))));
        } else {
            self.title_text(&text);
        }
    }

    /// Writes the title of the document.
    fn title_text(&mut self, text: &str) {
        self.paragraph(&format!("#kakemono-title[{text}]"));
    }

    /// Writes a heading at its level below the outermost one (`=` for the
    /// sections of a paper, the chapters of a book), with its label;
    /// `outlined: false` keeps it out of the table of contents (the
    /// heading of the table of contents itself).
    fn heading(&mut self, block: &Block, level: u8, outlined: bool) {
        let text = self.text.spans(&block.spans);
        if text.is_empty() {
            self.anchor(block);
            return;
        }
        let depth = usize::from(level.saturating_sub(self.top.unwrap_or(level))) + 1;
        let label = linked_anchor(block, self.text.anchors)
            .map(|anchor| format!(" <{}>", label(anchor)))
            .unwrap_or_default();
        if outlined {
            self.paragraph(&format!("{} {text}{label}", "=".repeat(depth)));
        } else {
            self.paragraph(&format!("#heading(level: {depth}, outlined: false)[{text}]{label}"));
        }
        self.opening = depth == 1;
    }

    /// Writes a list item with the marker of the paper (a bullet when it
    /// has none) at its nesting level.
    fn list_item(&mut self, block: &Block, marker: Option<&str>, level: u8) {
        self.anchor(block);
        let level = level.min(MAX_LIST_NESTING);
        self.list_level = level;
        let rest = marker.and_then(|marker| strip_label(&block.spans, marker));
        let marker = match (marker, &rest) {
            (Some(marker), Some(_)) => protect_start(&escape(marker)),
            _ => "•".to_string(),
        };
        let text = self.text.spans(rest.as_deref().unwrap_or(&block.spans));
        self.paragraph(&format!("#kakemono-item({level}, [{marker}], [{text}])"));
    }

    /// Writes a paragraph that continues a list item.
    fn list_continuation(&mut self, block: &Block) {
        self.anchor(block);
        let text = self.text.spans(&block.spans);
        if !text.is_empty() {
            self.paragraph(&format!("#kakemono-continuation({}, [{text}])", self.list_level));
        }
    }

    /// Writes a footnote of the paper where it was, with its mark; a note
    /// without a mark (a note in the margin of a book) as an aside.
    fn footnote(&mut self, block: &Block, marker: &str) {
        self.anchor(block);
        let text = self.text.spans(&block.spans);
        if text.is_empty() {
            return;
        }
        if marker.is_empty() {
            self.paragraph(&format!("#kakemono-aside[{text}]"));
        } else {
            self.paragraph(&format!("#kakemono-note([{}], [{text}])", escape(marker)));
        }
    }

    /// Writes code lines as a listing.
    fn code(&mut self, lines: &[String]) {
        let body = lines.join("\n");
        let body = body.trim_end();
        if body.trim().is_empty() {
            return;
        }
        self.paragraph(&format!("#kakemono-code({})", code_string(body)));
    }

    /// Writes a displayed formula: as Typst math when its structure is
    /// known (and wanted), as a picture otherwise; with its number.
    fn equation(&mut self, block: &Block, image: Option<&ImageSpec>, number: Option<&str>) {
        let body = match (equation_math(block, image, self.opts.equations), image) {
            (Some(math), _) => format!("$ {} $", typst(math)),
            (None, Some(image)) => self.picture(image, "eq"),
            (None, None) => format!("align(center)[{}]", self.text.spans(&block.spans)),
        };
        let number = number
            .map(|number| number.trim().trim_start_matches('(').trim_end_matches(')').trim())
            .filter(|number| !number.is_empty())
            .map_or_else(
                || "none".to_string(),
                |number| format!("[{}]", protect_start(&escape(number))),
            );
        self.anchor(block);
        self.paragraph(&format!("#kakemono-equation({number}, {body})"));
    }

    /// Writes a float: the rebuilt table or the picture, and the caption
    /// on the side it had in the paper, on one page. A float whose text is
    /// all code is a listing.
    fn figure(&mut self, block: &Block, kind: FloatKind, image: Option<&ImageSpec>) {
        self.anchor(block);
        let caption = (!block.spans.is_empty()).then(|| self.caption(&block.spans));
        if kind == FloatKind::Algorithm
            && block.table.is_none()
            && let Some(lines) = code::float_listing_lines(block)
        {
            if let Some(caption) = &caption {
                self.paragraph(caption);
            }
            return self.code(&lines);
        }
        let caption_first = caption_first(block, image);
        let content = if let Some(table) = block.table.as_ref().filter(|table| !table.rows.is_empty()) {
            Some(self.table(table))
        } else {
            image.map(|spec| format!("#{}", self.picture(spec, float_class(kind))))
        };
        let mut parts: Vec<String> = Vec::new();
        if caption_first && let Some(caption) = &caption {
            parts.push(caption.clone());
        }
        parts.extend(content);
        if !caption_first && let Some(caption) = caption {
            parts.push(caption);
        }
        match parts.len() {
            0 => {}
            1 => self.paragraph(&parts[0]),
            _ => self.paragraph(&format!("#kakemono-figure[\n{}\n]", parts.join("\n\n"))),
        }
    }

    /// A caption, with its label ("Figure 3:") apart, which the template
    /// sets in bold.
    fn caption(&self, spans: &[Span]) -> String {
        if let Some(first) = spans.first()
            && let Some(found) = CAP_LABEL.find(&first.text)
        {
            let label = first.text[..found.end()].trim_end();
            let mut rest = spans.to_vec();
            rest[0].text = first.text[label.len()..].to_string();
            format!(
                "#kakemono-caption([{}], [{}])",
                protect_start(&escape(label)),
                self.text.spans(&rest)
            )
        } else {
            format!("#kakemono-caption(none, [{}])", self.text.spans(spans))
        }
    }

    /// The picture of a page area, at its size in the PDF, whose file name
    /// (with `hint` in it) is filled in later: a call without its `#`.
    fn picture(&mut self, spec: &ImageSpec, hint: &str) -> String {
        let width = spec.rect.width().max(1.0);
        let src = self.images.region(spec, hint);
        format!("kakemono-picture(\"{src}\", {width:.1}pt)")
    }

    /// A rebuilt table: each cell at its place (so that the cells covered
    /// by a spanning one are left out), rules above, below and under the
    /// header.
    fn table(&self, table: &Table) -> String {
        let n_cols = table.n_cols.max(1);
        let n_rows = table.rows.len();
        let align: Vec<&str> = (0..n_cols)
            .map(|col| match table.col_align.get(col).copied() {
                Some("center") => "center",
                Some("right") => "right",
                _ => "left",
            })
            .collect();
        let mut taken = vec![vec![false; n_cols]; n_rows];
        let mut args = vec![
            format!("columns: {n_cols}"),
            format!("align: ({},)", align.join(", ")),
            "table.hline(y: 0)".to_string(),
        ];
        if table.header_rows > 0 && table.header_rows < n_rows {
            args.push(format!("table.hline(y: {}, stroke: 0.5pt)", table.header_rows));
        }
        args.push(format!("table.hline(y: {n_rows})"));
        for (row_idx, row) in table.rows.iter().enumerate() {
            for cell in row.iter().filter(|cell| cell.col < n_cols) {
                let colspan = cell.colspan.clamp(1, n_cols - cell.col);
                let rowspan = cell.rowspan.clamp(1, n_rows - row_idx);
                let cols = cell.col..cell.col + colspan;
                let rows = row_idx..row_idx + rowspan;
                if rows.clone().any(|r_idx| cols.clone().any(|c_idx| taken[r_idx][c_idx])) {
                    continue;
                }
                for r_idx in rows {
                    for c_idx in cols.clone() {
                        taken[r_idx][c_idx] = true;
                    }
                }
                let mut spans_args = format!("x: {}, y: {row_idx}", cell.col);
                if colspan > 1 {
                    spans_args.push_str(&format!(", colspan: {colspan}"));
                }
                if rowspan > 1 {
                    spans_args.push_str(&format!(", rowspan: {rowspan}"));
                }
                args.push(format!("table.cell({spans_args})[{}]", self.text.spans(&cell.spans)));
            }
        }
        format!("#kakemono-table(\n  {},\n)", args.join(",\n  "))
    }
}

/// The author groups: each card the name in bold, then the affiliation
/// lines; the template sets the cards side by side.
impl AuthorMarkup for Writer<'_, '_> {
    fn anchor(&mut self, block: &Block) {
        Writer::anchor(self, block);
    }

    fn lines(&self, block: &Block) -> Vec<String> {
        author_lines(
            block,
            |row| self.text.spans(row),
            "#strong[",
            |name| format!("#strong[{name}]{CALL_END}"),
            |line| line,
        )
    }

    fn card(&self, lines: &[String]) -> String {
        format!("[{}]", lines.join(" \\ "))
    }

    fn cards(&mut self, cards: &[String]) {
        self.paragraph(&format!("#kakemono-authors({})", cards.join(", ")));
    }

    fn byline(&mut self, lines: &[String]) {
        self.paragraph(&format!("#kakemono-byline[{}]", lines.join(" \\ ")));
    }
}

/// `code` as a Typst string for a listing: tabs as four spaces.
fn code_string(code: &str) -> String {
    let mut out = String::from("\"");
    for ch in code.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("    "),
            ch if ch.is_control() || ch == CALL_END => {}
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TableCell;
    use std::collections::HashSet;

    #[test]
    fn picture_folder_is_safe() {
        assert_eq!(
            safe_path("my images/1706.03762v7_assets/"),
            "my_images/1706.03762v7_assets"
        );
        assert_eq!(safe_path("a\"b\\c"), "a_b_c");
    }

    #[test]
    fn vietnamese_letters() {
        assert!(is_vietnamese('ư') && is_vietnamese('Ặ') && is_vietnamese('đ'));
        assert!(!is_vietnamese('é') && !is_vietnamese('ł') && !is_vietnamese('a'));
    }

    #[test]
    fn code_is_a_string() {
        assert_eq!(code_string("a\t\"b\"\\\nc"), "\"a    \\\"b\\\"\\\\\\nc\"");
    }

    #[test]
    fn table_with_spanning_cells() {
        let cell = |col: usize, text: &str, colspan: usize, rowspan: usize| TableCell {
            col,
            colspan,
            rowspan,
            spans: vec![Span::plain(text)],
        };
        let table = Table {
            n_cols: 3,
            header_rows: 2,
            col_align: vec!["left", "center", "right"],
            rows: vec![
                vec![cell(0, "Model", 1, 2), cell(1, "Score", 2, 1)],
                vec![cell(0, "covered", 1, 1), cell(1, "A", 1, 1), cell(2, "B", 1, 1)],
                vec![cell(0, "x * y", 1, 1), cell(2, "[3]", 1, 1)],
            ],
            rule_above: Vec::new(),
            rule_bottom: Vec::new(),
            rule_left: Vec::new(),
            rule_right: Vec::new(),
            geom: crate::model::TableGeom::default(),
        };
        let anchors = HashSet::new();
        let mut images = ImageStore {
            opts: &HtmlOptions::default(),
            jobs: Vec::new(),
            vector: false,
        };
        let opts = TypstOptions::default();
        let writer = Writer {
            opts: &opts,
            text: Text {
                anchors: &anchors,
                math: true,
            },
            images: &mut images,
            out: String::new(),
            top: None,
            abstract_title: None,
            in_abstract: false,
            list_level: 0,
            opening: false,
        };
        assert_eq!(
            writer.table(&table),
            "#kakemono-table(\n  columns: 3,\n  align: (left, center, right,),\n  table.hline(y: 0),\n  \
             table.hline(y: 2, stroke: 0.5pt),\n  table.hline(y: 3),\n  table.cell(x: 0, y: 0, rowspan: 2)[Model],\n  \
             table.cell(x: 1, y: 0, colspan: 2)[Score],\n  table.cell(x: 1, y: 1)[A],\n  table.cell(x: 2, y: 1)[B],\n  \
             table.cell(x: 0, y: 2)[x \\* y],\n  table.cell(x: 2, y: 2)[\\[3\\]],\n)"
        );
    }
}
