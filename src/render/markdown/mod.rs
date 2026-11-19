//! Markdown output: the document as one `.md` file, built from the same
//! [`Document`] as the HTML modes. Headings, lists, captions, references and
//! code listings get their Markdown syntax; rebuilt tables become pipe
//! tables (HTML tables when cells span rows or columns); formulas whose
//! structure is known become LaTeX between `$$`; figures (and the other
//! formulas) are pictures of their area of the page, written to an image
//! folder next to the file (or embedded as data URIs).

pub(crate) mod inline;

use super::latex::math::latex;
use super::{
    CAP_LABEL, Equations, HtmlOptions, ImageStore, Mode, PageImage, caption_first, code, code_run, doc_title,
    equation_math, figure_alt, float_class, heading_tag, is_abstract_text, is_scene_break, plain,
    stray_equation_number, table_walk,
};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::{Block, BlockKind, Document, FloatKind, ImageSpec, Span, Table, TableCell};
use crate::render::Pictures;
use inline::{Inline, escape, escape_line_start, spans_html};
use std::collections::HashSet;

/// Code fences are at least this many backticks long.
const MIN_FENCE: usize = 3;
/// Spaces of indentation per level of a table of contents.
const CONTENTS_INDENT: usize = 2;

/// Everything that controls how a document is written as Markdown.
#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Embed the pictures as data URIs (one self-contained file) instead of
    /// writing them to [`MarkdownOptions::asset_dir`].
    pub inline_images: bool,
    /// Directory (relative to the Markdown file) of the picture files.
    pub asset_dir: String,
    /// Pixels per PDF point of the pictures of figures and formulas.
    pub scale: f32,
    /// Worker threads for drawing the pictures (0 = one per CPU).
    pub jobs: usize,
    /// How displayed formulas are written: [`Equations::MathMl`] gives LaTeX
    /// (`$$ … $$`) rebuilt from the glyphs where the structure of the
    /// formula is clear, pictures elsewhere.
    pub equations: Equations,
}

impl Default for MarkdownOptions {
    /// Pictures written to `images/` at 3 pixels per point, formulas as
    /// LaTeX where their structure is clear, one worker thread per CPU.
    fn default() -> Self {
        Self {
            inline_images: false,
            asset_dir: "images".into(),
            scale: 3.0,
            jobs: 0,
            equations: Equations::MathMl,
        }
    }
}

/// The result of the Markdown output: the text and, when pictures are not
/// embedded, the picture files it refers to.
pub struct MarkdownOutput {
    /// The Markdown document.
    pub markdown: String,
    /// Files to write next to the Markdown file: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

/// Writes an analysed document as Markdown.
///
/// The text is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and either
/// embedded as data URIs or returned in [`MarkdownOutput::files`].
pub fn render_markdown(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &MarkdownOptions) -> MarkdownOutput {
    let html_opts = HtmlOptions {
        mode: Mode::Friendly,
        inline_images: opts.inline_images,
        asset_dir: opts.asset_dir.clone(),
        scale: opts.scale,
        page_image: PageImage::Svg,
        jobs: opts.jobs,
        embed_fonts: false,
        equations: opts.equations,
    };
    let mut images = ImageStore {
        opts: &html_opts,
        jobs: Vec::new(),
    };
    let doc = &analysis.doc;
    let anchors = linked_anchors(doc);
    let mut writer = Writer {
        opts,
        inline: Inline {
            anchors: &anchors,
            math: opts.equations == Equations::MathMl,
        },
        images: &mut images,
        out: String::new(),
        list_indent: 0,
        bracket_src: !opts.inline_images && opts.asset_dir.contains(' '),
    };
    writer.write(doc);
    let text = writer.out.trim_end().to_string() + "\n";
    let out = images.finish(Pictures::new(pdf, &analysis.doc), text);
    MarkdownOutput {
        markdown: out.html,
        files: out.files,
    }
}

/// The anchors of the blocks that some text links to: only those get an
/// `<a id>`, so that the Markdown is not cluttered with unused ones.
pub(crate) fn linked_anchors(doc: &Document) -> HashSet<String> {
    let targets: HashSet<&str> = doc
        .blocks
        .iter()
        .flat_map(|block| block.spans.iter().chain(block.rows.iter().flatten()))
        .chain(
            doc.blocks
                .iter()
                .filter_map(|block| block.table.as_ref())
                .flat_map(|table| table.rows.iter().flatten().flat_map(|cell| cell.spans.iter())),
        )
        .filter_map(|span| span.href.as_deref()?.strip_prefix('#'))
        .collect();
    doc.blocks
        .iter()
        .filter_map(|block| block.anchor.as_deref())
        .filter(|anchor| targets.contains(anchor))
        .map(str::to_string)
        .collect()
}

/// Writes the Markdown of a document, block by block.
struct Writer<'a, 'r> {
    /// The output options.
    opts: &'a MarkdownOptions,
    /// Writes the text of the blocks.
    inline: Inline<'a>,
    /// The pictures; each `src` is a placeholder until the end.
    images: &'a mut ImageStore<'r>,
    /// The Markdown so far.
    out: String,
    /// Indentation of the content of the list item written last (the
    /// width of its marker): a continuation paragraph is indented by it.
    list_indent: usize,
    /// Picture paths hold a space: they go in angle brackets.
    bracket_src: bool,
}

impl Writer<'_, '_> {
    /// Writes every block of `doc`. Runs of code paragraphs become one
    /// listing, the title blocks one title, and an abstract without a
    /// heading gets one.
    fn write(&mut self, doc: &Document) {
        let blocks: Vec<&Block> = doc.blocks.iter().collect();
        let mut idx = 0;
        let mut in_abstract = false;
        let mut after_front = false;
        while idx < blocks.len() {
            let block = blocks[idx];
            if !after_front && !matches!(block.kind, BlockKind::Title | BlockKind::Authors | BlockKind::Note) {
                self.arxiv_link(doc);
                after_front = true;
            }
            if let Some(end) = code_run(&blocks, idx) {
                self.anchor(block);
                self.fence(&code::listing_lines(&blocks[idx..end]));
                (idx, in_abstract) = (end, false);
                continue;
            }
            match &block.kind {
                BlockKind::Title => {
                    idx = self.title(doc, &blocks, idx);
                    continue;
                }
                BlockKind::Paragraph { in_abstract: true } => {
                    let had_heading = idx > 0
                        && matches!(blocks[idx - 1].kind, BlockKind::Heading { .. })
                        && is_abstract_text(&blocks[idx - 1].spans);
                    if !in_abstract && !had_heading {
                        self.paragraph(&format!("{} Abstract", heading_marks(1)));
                    }
                    in_abstract = true;
                }
                BlockKind::Equation { .. } => {}
                _ => in_abstract = false,
            }
            // An equation whose number was read as a paragraph of its own
            // takes it back.
            if let BlockKind::Equation { image, number: None } = &block.kind
                && let Some(number) = blocks.get(idx + 1).and_then(|next| stray_equation_number(next))
            {
                self.equation(block, image.as_ref(), Some(&number));
                idx += 2;
                continue;
            }
            self.block(block);
            idx += 1;
        }
        if !after_front {
            self.arxiv_link(doc);
        }
    }

    /// Writes one block.
    fn block(&mut self, block: &Block) {
        self.anchor(block);
        match &block.kind {
            BlockKind::Title => self.paragraph(&format!("# {}", self.inline.spans(&block.spans))),
            BlockKind::Authors => self.authors(block),
            BlockKind::Note => self.text_paragraph(&block.spans),
            BlockKind::Heading { level } => {
                let text = self.inline.spans(&block.spans);
                self.paragraph(&format!("{} {text}", heading_marks(*level)));
            }
            BlockKind::Paragraph { .. } if is_scene_break(&block.spans) => self.paragraph("* * *"),
            BlockKind::Paragraph { .. } => self.text_paragraph(&block.spans),
            BlockKind::ListItem { marker } => self.list_item(block, marker.as_deref()),
            BlockKind::ListContinuation => {
                let text = escape_line_start(&self.inline.spans(&block.spans));
                self.paragraph(&format!("{}{text}", " ".repeat(self.list_indent.max(2))));
            }
            BlockKind::Reference { .. } => self.text_paragraph(&block.spans),
            // A note without a mark (a note in the margin of a book) is set
            // apart as a quotation.
            BlockKind::Footnote { marker } if marker.is_empty() => {
                let text = self.inline.spans(&block.spans);
                if !text.is_empty() {
                    self.paragraph(&format!("> {text}"));
                }
            }
            BlockKind::Footnote { marker } => {
                let mark = format!("<sup>{}</sup> ", escape(marker));
                self.paragraph(&format!("{mark}{}", self.inline.spans(&block.spans)));
            }
            BlockKind::Equation { image, number } => self.equation(block, image.as_ref(), number.as_deref()),
            BlockKind::Figure { kind, image, label } => self.figure(block, *kind, image.as_ref(), label),
            BlockKind::ContentsEntry { level, page } => {
                let indent = " ".repeat(usize::from(level.saturating_sub(1)) * CONTENTS_INDENT);
                let text = self.inline.spans(&block.spans);
                let page = if page.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", escape(page))
                };
                self.line(&format!("{indent}- {text}{page}"));
            }
            BlockKind::Code => {
                let lines: Vec<String> = block
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|span| span.text.as_str()).collect())
                    .collect();
                self.fence(&lines);
            }
        }
    }

    /// Writes `text` as a block, followed by a blank line.
    fn paragraph(&mut self, text: &str) {
        self.end_tight_list();
        self.out.push_str(text);
        self.out.push_str("\n\n");
    }

    /// Writes `text` as one line of a tight list (no blank line after it).
    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// Ends a tight list (lines written by [`Writer::line`]) with a blank
    /// line.
    fn end_tight_list(&mut self) {
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            self.out.push('\n');
        }
    }

    /// Writes spans as a paragraph of running text.
    fn text_paragraph(&mut self, spans: &[Span]) {
        let text = self.inline.spans(spans);
        if !text.is_empty() {
            self.paragraph(&escape_line_start(&text));
        }
    }

    /// Writes the `<a id>` of a block that some text links to.
    fn anchor(&mut self, block: &Block) {
        if let Some(anchor) = block
            .anchor
            .as_deref()
            .filter(|anchor| self.inline.anchors.contains(*anchor))
        {
            self.end_tight_list();
            self.out.push_str(&format!("<a id=\"{}\"></a>\n\n", super::esc(anchor)));
        }
    }

    /// Writes a link to the arXiv page of the paper, after the front matter.
    fn arxiv_link(&mut self, doc: &Document) {
        if let Some(id) = &doc.meta.arxiv_id {
            self.paragraph(&format!("[arXiv:{}](https://arxiv.org/abs/{id})", escape(id)));
        }
    }

    /// Writes the title blocks that start at `blocks[start]` as one title
    /// and returns the index of the block after them.
    fn title(&mut self, doc: &Document, blocks: &[&Block], start: usize) -> usize {
        let end = start
            + blocks[start..]
                .iter()
                .take_while(|block| matches!(block.kind, BlockKind::Title))
                .count();
        let mut spans: Vec<Span> = Vec::new();
        for block in &blocks[start..end] {
            self.anchor(block);
            if !spans.is_empty() {
                spans.push(Span::plain(" "));
            }
            spans.extend(block.spans.iter().cloned());
        }
        let text = self.inline.spans(&spans);
        if text.is_empty() {
            self.paragraph(&format!("# {}", escape(&doc_title(doc))));
        } else {
            self.paragraph(&format!("# {text}"));
        }
        end
    }

    /// Writes an author group: the name in bold, then the affiliation
    /// lines, each on a line of its own.
    fn authors(&mut self, block: &Block) {
        let rows = if block.rows.is_empty() {
            std::slice::from_ref(&block.spans)
        } else {
            &block.rows[..]
        };
        let lines: Vec<String> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let text = self.inline.spans(row);
                if i == 0 && !text.is_empty() && !text.contains('*') {
                    format!("**{text}**")
                } else {
                    text
                }
            })
            .filter(|text| !text.is_empty())
            .collect();
        if !lines.is_empty() {
            self.paragraph(&escape_line_start(&lines.join("  \n")));
        }
    }

    /// Writes a list item: a numbered one (`3.`) when its marker is a
    /// number, a bullet item otherwise, with any other marker kept as its
    /// first word.
    fn list_item(&mut self, block: &Block, marker: Option<&str>) {
        let number = marker
            .map(|marker| marker.trim_end_matches(['.', ')']))
            .filter(|digits| !digits.is_empty() && digits.len() <= 9 && digits.bytes().all(|b| b.is_ascii_digit()));
        let rest = marker.and_then(|marker| strip_label(&block.spans, marker));
        let prefix = match (number, marker, &rest) {
            (Some(number), _, _) => format!("{number}. "),
            (None, Some(marker), Some(_)) => format!("- {} ", escape(marker)),
            _ => "- ".to_string(),
        };
        let text = self.inline.spans(rest.as_deref().unwrap_or(&block.spans));
        self.list_indent = if number.is_some() { prefix.len() } else { 2 };
        self.paragraph(&format!("{prefix}{text}"));
    }

    /// Writes code lines as a fenced code block.
    fn fence(&mut self, lines: &[String]) {
        let longest = lines
            .iter()
            .flat_map(|line| line.split(|ch| ch != '`'))
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat((longest + 1).max(MIN_FENCE));
        let body = lines.join("\n");
        self.paragraph(&format!("{fence}\n{}\n{fence}", body.trim_end()));
    }

    /// Writes a displayed formula: as LaTeX when its structure is known
    /// (and wanted), as a picture otherwise; with its number.
    fn equation(&mut self, block: &Block, image: Option<&ImageSpec>, number: Option<&str>) {
        let tag = number.map(|number| number.trim().trim_start_matches('(').trim_end_matches(')').trim());
        if let Some(math) = equation_math(block, image, self.opts.equations) {
            let tag = tag.map(|tag| format!(" \\tag{{{tag}}}")).unwrap_or_default();
            self.paragraph(&format!("$$\n{}{tag}\n$$", latex(math)));
            return;
        }
        let shown = match image {
            Some(image) => self.picture(image, &plain(&block.spans), "eq"),
            None => escape(&plain(&block.spans)),
        };
        let number = number.map(|number| format!(" {}", escape(number))).unwrap_or_default();
        self.paragraph(&format!("{shown}{number}"));
    }

    /// Writes a float: the rebuilt table or the picture, and the caption
    /// on the side it had in the paper. A float whose text is all code is
    /// a listing.
    fn figure(&mut self, block: &Block, kind: FloatKind, image: Option<&ImageSpec>, label: &Option<String>) {
        let caption = (!block.spans.is_empty()).then(|| self.caption(&block.spans));
        if kind == FloatKind::Algorithm
            && block.table.is_none()
            && let Some(lines) = code::float_listing_lines(block)
        {
            if let Some(caption) = &caption {
                self.paragraph(caption);
            }
            return self.fence(&lines);
        }
        let caption_first = caption_first(block, image);
        if caption_first && let Some(caption) = &caption {
            self.paragraph(caption);
        }
        if let Some(table) = &block.table {
            let text = self.table(table);
            self.paragraph(&text);
        } else if let Some(spec) = image {
            let picture = self.picture(spec, &figure_alt(block, label), float_class(kind));
            self.paragraph(&picture);
        }
        if !caption_first && let Some(caption) = &caption {
            self.paragraph(caption);
        }
    }

    /// A caption, with its label ("Figure 3:") in bold.
    fn caption(&self, spans: &[Span]) -> String {
        if let Some(first) = spans.first()
            && let Some(found) = CAP_LABEL.find(&first.text)
        {
            let label = first.text[..found.end()].trim_end();
            let mut rest = spans.to_vec();
            rest[0].text = first.text[label.len()..].to_string();
            let text = self.inline.spans(&rest);
            return format!("**{}** {text}", escape(label)).trim_end().to_string();
        }
        escape_line_start(&self.inline.spans(spans))
    }

    /// The Markdown picture of a page area, whose `src` is drawn later.
    fn picture(&mut self, spec: &ImageSpec, alt: &str, hint: &str) -> String {
        let src = self.images.region(spec, hint);
        let alt = escape(&alt.replace(['\n', '\r'], " "));
        if self.bracket_src {
            format!("![{alt}](<{src}>)")
        } else {
            format!("![{alt}]({src})")
        }
    }

    /// A rebuilt table: a pipe table when it has one header row and no
    /// cell spans several positions, an HTML table otherwise.
    fn table(&self, table: &Table) -> String {
        let simple = table.header_rows == 1
            && table.n_cols > 0
            && table
                .rows
                .iter()
                .flatten()
                .all(|cell| cell.colspan == 1 && cell.rowspan == 1);
        if simple {
            self.pipe_table(table)
        } else {
            self.html_table(table)
        }
    }

    /// A table as a GitHub-flavoured pipe table.
    fn pipe_table(&self, table: &Table) -> String {
        let mut lines = Vec::new();
        for (row_idx, row) in table.rows.iter().enumerate() {
            let mut cells = vec![String::new(); table.n_cols];
            for cell in row.iter().filter(|cell| cell.col < table.n_cols) {
                cells[cell.col] = self.inline.cell(&cell.spans);
            }
            lines.push(format!("| {} |", cells.join(" | ")));
            if row_idx == 0 {
                let rule: Vec<&str> = (0..table.n_cols)
                    .map(|col| match table.col_align.get(col).copied() {
                        Some("center") => ":---:",
                        Some("right") => "---:",
                        _ => "---",
                    })
                    .collect();
                lines.push(format!("| {} |", rule.join(" | ")));
            }
        }
        lines.join("\n")
    }

    /// A table as plain HTML (no classes), with its spanning cells.
    fn html_table(&self, table: &Table) -> String {
        let mut out = String::from("<table>");
        let mut open_row: Option<usize> = None;
        table_walk(table, |row_idx, _, cell| {
            if open_row != Some(row_idx) {
                if open_row.is_some() {
                    out.push_str("</tr>");
                }
                out.push_str("\n<tr>");
                open_row = Some(row_idx);
            }
            let tag = if row_idx < table.header_rows { "th" } else { "td" };
            out.push_str(&match cell {
                None => format!("<{tag}></{tag}>"),
                Some(cell) => self.html_cell(cell, tag),
            });
        });
        if open_row.is_some() {
            out.push_str("</tr>");
        }
        out.push_str("\n</table>");
        out
    }

    /// One cell of an HTML table.
    fn html_cell(&self, cell: &TableCell, tag: &str) -> String {
        format!(
            "<{tag}{}>{}</{tag}>",
            super::span_attrs(cell),
            spans_html(&cell.spans, self.inline.anchors)
        )
    }
}

/// The `#` marks of a heading of `level` (`##` for sections: `#` is the
/// title), as for the HTML headings.
fn heading_marks(level: u8) -> &'static str {
    match heading_tag(level) {
        "h2" => "##",
        "h3" => "###",
        "h4" => "####",
        _ => "#####",
    }
}

/// The spans after the label `label` at the start of their text (and the
/// spaces after it), or `None` when the text does not start with it.
pub(crate) fn strip_label(spans: &[Span], label: &str) -> Option<Vec<Span>> {
    let first = spans.first()?;
    let rest = first.text.strip_prefix(label).filter(|_| !label.is_empty())?;
    let mut spans = spans.to_vec();
    spans[0].text = rest.trim_start().to_string();
    Some(spans)
}
