//! LaTeX output: the document as one `.tex` file that compiles with
//! pdfLaTeX, XeLaTeX or LuaLaTeX, built from the same [`Document`] as the
//! HTML modes. Headings become starred sections, the abstract and lists
//! their environments, rebuilt tables `tabular`s (with `booktabs` rules,
//! `\multicolumn` and `\multirow`), formulas whose structure is known
//! LaTeX math, links `\hyperref` and `\href`; figures (and the other
//! formulas) are pictures of their area of the page, written to an image
//! folder next to the file.
//!
//! The text keeps the order of the PDF: figures and tables are not floats,
//! and their captions keep the label of the paper ("Figure 3:").

pub(crate) mod math;
mod text;
mod vietnamese;

use super::markdown::{linked_anchors, strip_label};
use super::sequence::{AuthorMarkup, Step, author_lines, linked_anchor, push_line, steps, title_spans, write_authors};
use super::{
    CAP_LABEL, Equations, HtmlOptions, ImageStore, Mode, PageImage, caption_first, code, doc_title, equation_math,
    float_class, heading_tag, is_scene_break,
};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::{Block, BlockKind, Document, FloatKind, ImageSpec, MONO, Span, Table, TableCell};
use crate::render::Pictures;
use math::latex;
use std::cell::RefCell;
use std::collections::BTreeSet;
use text::{LAST_LATIN, LATIN_GAPS, Text, escape, label};

/// The packages and commands that every document uses.
const PREAMBLE: &str = include_str!("preamble.tex");
/// Lists LaTeX can nest inside a list (it allows four levels in all).
const MAX_LIST_NESTING: u8 = 3;
/// The encoding of pdfLaTeX's fonts, in [`PREAMBLE`].
const T1_FONTENC: &str = "\\usepackage[T1]{fontenc}";
/// The encodings of a document with Vietnamese letters (see
/// [`vietnamese`]): T5 for those letters, T1 (the last, the main one) for
/// the rest.
const T5_FONTENC: &str = "\\usepackage[T5,T1]{fontenc}";
/// Indentation of a table of contents, per level, in em.
const CONTENTS_INDENT_EM: f32 = 1.5;
/// Hanging indentation of a bibliography entry.
const REFERENCE_INDENT: &str = "1.5em";
/// Share of the characters of a paragraph that are monospaced from which
/// it is set ragged right.
const RAGGED_MONO_RATIO: f32 = 0.5;
/// Space between the author cards that stand side by side.
const AUTHOR_GAP: &str = "2em";
/// Spaces that a tab stands for in a code listing (`verbatim` writes a tab
/// as one space).
const TAB_SPACES: &str = "    ";

/// Everything that controls how a document is written as LaTeX.
#[derive(Debug, Clone)]
pub struct LatexOptions {
    /// Directory (relative to the `.tex` file) of the picture files. LaTeX
    /// cannot embed pictures: they are always files. Characters other than
    /// ASCII letters, digits, `.`, `_`, `-` and `/` are replaced by `_`,
    /// since TeX gives some of them a meaning.
    pub asset_dir: String,
    /// Pixels per PDF point of the pictures of figures and formulas.
    pub scale: f32,
    /// Worker threads for drawing the pictures (0 = one per CPU).
    pub jobs: usize,
    /// How displayed formulas are written: [`Equations::MathMl`] gives LaTeX
    /// math rebuilt from the glyphs where the structure of the formula is
    /// clear (and inside the text), pictures elsewhere.
    pub equations: Equations,
}

impl Default for LatexOptions {
    /// Pictures written to `images/` at 3 pixels per point, formulas as
    /// LaTeX where their structure is clear, one worker thread per CPU.
    fn default() -> Self {
        Self {
            asset_dir: "images".into(),
            scale: 3.0,
            jobs: 0,
            equations: Equations::MathMl,
        }
    }
}

/// The result of the LaTeX output: the source and the picture files it
/// refers to.
pub struct LatexOutput {
    /// The LaTeX document (`.tex`).
    pub latex: String,
    /// Files to write next to the `.tex` file: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

/// Writes an analysed document as LaTeX.
///
/// The text is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and returned
/// in [`LatexOutput::files`].
pub fn render_latex(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &LatexOptions) -> LatexOutput {
    // The footnotes go to the foot of the page of their calls.
    let attached = crate::render::notes::footnotes_attached(analysis);
    let analysis = attached.as_ref().unwrap_or(analysis);
    let html_opts = HtmlOptions {
        mode: Mode::Friendly,
        inline_images: false,
        asset_dir: tex_path(&opts.asset_dir),
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
            table_notes: RefCell::default(),
        },
        images: &mut images,
        out: String::new(),
        group: Group::None,
        item_open: false,
        nested: 0,
        after_text: false,
    };
    writer.write(doc);
    let body = writer.out.trim_end().to_string();
    let out = images.finish(Pictures::new(pdf, &analysis.doc), body);
    LatexOutput {
        latex: document(doc, &out.html),
        files: out.files,
    }
}

/// The whole `.tex` file: the preamble, the characters that pdfLaTeX has no
/// glyph for, the title of the PDF, and `body`.
fn document(doc: &Document, body: &str) -> String {
    let title = escape(&doc_title(doc));
    let (vietnamese, unknown): (Vec<char>, Vec<char>) = unknown_chars(&title, body)
        .into_iter()
        .partition(|&ch| vietnamese::t5_letter(ch).is_some());
    let mut out = if vietnamese.is_empty() {
        String::from(PREAMBLE)
    } else {
        // T1 stays the main encoding (the last one loaded).
        PREAMBLE.replacen(T1_FONTENC, T5_FONTENC, 1)
    };
    if doc.pages.is_empty() {
        let source = match crate::render::source_label(doc) {
            "EPUB" => "an EPUB book",
            "HTML" => "a web page",
            _ => "a Word document",
        };
        out = out.replacen("Converted from a PDF", &format!("Converted from {source}"), 1);
    }
    if !vietnamese.is_empty() {
        out.push_str(&format!(
            "% Vietnamese letters: pdfLaTeX sets them in the T5 encoding (vntex).\n\\ifPDFTeX\n  \\newcommand{{{cmd}}}[1]{{{{\\fontencoding{{T5}}\\selectfont#1}}}}\n",
            cmd = vietnamese::T5_COMMAND
        ));
        for ch in vietnamese {
            let letter = vietnamese::t5_letter(ch).expect("partitioned on t5_letter");
            out.push_str(&format!(
                "  \\DeclareUnicodeCharacter{{{:04X}}}{{{}{{{letter}}}}}\n",
                u32::from(ch),
                vietnamese::T5_COMMAND
            ));
        }
        out.push_str("\\fi\n");
    }
    if !unknown.is_empty() {
        out.push_str("% Characters that the fonts of pdfLaTeX do not have.\n\\ifPDFTeX\n");
        for ch in unknown {
            out.push_str(&format!(
                "  \\DeclareUnicodeCharacter{{{:04X}}}{{\\kakemonomissing}}\n",
                u32::from(ch)
            ));
        }
        out.push_str("\\fi\n");
    }
    out.push_str(&format!(
        "\\hypersetup{{pdftitle={{{title}}}}}\n\n\\begin{{document}}\n\n"
    ));
    out.push_str(body);
    out.push_str("\n\n\\end{document}\n");
    out
}

/// The characters of `title` and `body` that pdfLaTeX's text fonts lack:
/// those left as they are by [`escape`] and by the math, outside the Latin
/// blocks.
fn unknown_chars(title: &str, body: &str) -> BTreeSet<char> {
    title
        .chars()
        .chain(body.chars())
        .filter(|&ch| !ch.is_ascii() && (ch > LAST_LATIN || LATIN_GAPS.contains(&ch)))
        .collect()
}

/// `dir` with the characters that TeX file names cannot safely hold
/// replaced by `_`, and without a trailing slash.
fn tex_path(dir: &str) -> String {
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

/// The environment that is open around the blocks being written.
#[derive(PartialEq, Clone, Copy)]
enum Group {
    /// None.
    None,
    /// A list (`itemize`).
    List,
    /// The abstract (`abstract`).
    Abstract,
}

impl Group {
    /// The group of `block`, given the one that is open: equations do not
    /// interrupt an abstract.
    fn of(block: &Block, current: Self) -> Self {
        match &block.kind {
            BlockKind::ListItem { .. } | BlockKind::ListContinuation => Self::List,
            BlockKind::Paragraph { in_abstract: true } => Self::Abstract,
            BlockKind::Equation { .. } if current == Self::Abstract => current,
            _ => Self::None,
        }
    }

    /// The name of the environment of the group.
    fn environment(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::List => Some("itemize"),
            Self::Abstract => Some("abstract"),
        }
    }
}

/// Writes the LaTeX of a document, block by block.
struct Writer<'a, 'r> {
    /// The output options.
    opts: &'a LatexOptions,
    /// Writes the text of the blocks.
    text: Text<'a>,
    /// The pictures; each file name is a placeholder until the end.
    images: &'a mut ImageStore<'r>,
    /// The LaTeX so far.
    out: String,
    /// The environment that is open.
    group: Group,
    /// An `\item` of the open list has been written.
    item_open: bool,
    /// Lists open inside the open list.
    nested: u8,
    /// The block written last is a paragraph of text: a displayed formula
    /// after it continues it (no blank line, which would start a new
    /// paragraph above the formula).
    after_text: bool,
}

impl Writer<'_, '_> {
    /// Writes every block of `doc`, in the steps of [`steps`]. The heading
    /// of the abstract is left to the `abstract` environment.
    fn write(&mut self, doc: &Document) {
        let blocks: Vec<&Block> = doc.blocks.iter().collect();
        for step in steps(&blocks, doc.meta.arxiv_id.is_some()) {
            match step {
                Step::Source => self.arxiv_link(doc),
                Step::Code(run) => {
                    self.set_group(Group::None);
                    self.anchor(run[0]);
                    self.verbatim(&code::listing_lines(run));
                }
                Step::Title(run) => {
                    self.set_group(Group::of(run[0], self.group));
                    self.title(doc, run);
                }
                Step::Authors(run) => {
                    self.set_group(Group::of(run[0], self.group));
                    write_authors(self, run);
                }
                Step::AbstractHeading(block) => {
                    self.set_group(Group::of(block, self.group));
                    self.anchor(block);
                }
                Step::NumberedEquation(block, number) => {
                    self.set_group(Group::of(block, self.group));
                    if let BlockKind::Equation { image, .. } = &block.kind {
                        self.equation(block, image.as_ref(), Some(&number));
                    }
                }
                Step::Contents(run) => {
                    for block in run {
                        self.set_group(Group::of(block, self.group));
                        self.block(block);
                    }
                }
                Step::Block(block) => {
                    self.set_group(Group::of(block, self.group));
                    self.block(block);
                }
            }
        }
        self.set_group(Group::None);
    }

    /// Closes the open environment and opens that of `group`, when they
    /// differ.
    fn set_group(&mut self, group: Group) {
        if group == self.group {
            return;
        }
        self.nest(0);
        if let Some(env) = self.group.environment() {
            self.paragraph(&format!("\\end{{{env}}}"));
        }
        if let Some(env) = group.environment() {
            self.paragraph(&format!("\\begin{{{env}}}"));
        }
        self.group = group;
        self.item_open = false;
    }

    /// Writes one block.
    fn block(&mut self, block: &Block) {
        if !matches!(
            block.kind,
            BlockKind::ListItem { .. } | BlockKind::ListContinuation | BlockKind::Equation { .. }
        ) {
            self.anchor(block);
        }
        match &block.kind {
            BlockKind::Title => self.title_text(&self.text.spans(&block.spans)),
            BlockKind::Authors => write_authors(self, std::slice::from_ref(&block)),
            BlockKind::Note => self.text_paragraph(&block.spans),
            BlockKind::Heading { level } => self.heading(block, *level),
            BlockKind::Paragraph { .. } if is_scene_break(&block.spans) => {
                self.paragraph("\\begin{center}\n* * *\n\\end{center}");
            }
            BlockKind::Paragraph { .. } => self.text_paragraph(&block.spans),
            BlockKind::ListItem { marker, level } => self.list_item(block, marker.as_deref(), *level),
            BlockKind::ListContinuation => self.list_continuation(block),
            BlockKind::Reference { .. } => {
                let text = self.text.spans(&block.spans);
                if !text.is_empty() {
                    self.paragraph(&format!(
                        "{{\\small\\noindent\\hangindent={REFERENCE_INDENT}\\relax {text}\\par}}"
                    ));
                }
            }
            BlockKind::Footnote { marker } => self.footnote(block, marker),
            BlockKind::Equation { image, number } => self.equation(block, image.as_ref(), number.as_deref()),
            BlockKind::Figure { kind, image, .. } => self.figure(block, *kind, image.as_ref()),
            BlockKind::ContentsEntry { level, page } => self.contents_entry(block, *level, page),
            BlockKind::Code => {
                let lines: Vec<String> = block
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|span| span.text.as_str()).collect())
                    .collect();
                self.verbatim(&lines);
            }
        }
    }

    /// Writes `text` as a block, followed by a blank line.
    fn paragraph(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push_str("\n\n");
        self.after_text = false;
    }

    /// Writes spans as a paragraph of running text. A paragraph that is
    /// mostly monospaced is set ragged right: the spaces of a monospaced
    /// font do not stretch to justify its lines.
    fn text_paragraph(&mut self, spans: &[Span]) {
        let text = self.text.spans(spans);
        if text.is_empty() {
            return;
        }
        if mono_ratio(spans) >= RAGGED_MONO_RATIO {
            self.paragraph(&format!("{{\\raggedright {text}\\par}}"));
        } else {
            self.paragraph(&text);
        }
        self.after_text = true;
    }

    /// The `\label` that marks the place of a block that some text links
    /// to, or nothing.
    fn anchor_label(&self, block: &Block) -> String {
        linked_anchor(block, self.text.anchors)
            .map(|anchor| format!("\\phantomsection\\label{{{}}}", label(anchor)))
            .unwrap_or_default()
    }

    /// Writes the `\label` of a block that some text links to, on a line
    /// of its own.
    fn anchor(&mut self, block: &Block) {
        let label = self.anchor_label(block);
        push_line(&mut self.out, &label);
    }

    /// Writes a link to the arXiv page of the paper, after the front matter.
    fn arxiv_link(&mut self, doc: &Document) {
        if let Some(id) = &doc.meta.arxiv_id {
            let link = format!("\\href{{https://arxiv.org/abs/{}}}{{arXiv:{}}}", escape(id), escape(id));
            self.paragraph(&format!("\\begin{{center}}\n{link}\n\\end{{center}}"));
        }
    }

    /// Writes a run of title blocks as one title.
    fn title(&mut self, doc: &Document, run: &[&Block]) {
        for block in run {
            self.anchor(block);
        }
        let text = self.text.spans(&title_spans(run));
        if text.is_empty() {
            self.title_text(&escape(&doc_title(doc)));
        } else {
            self.title_text(&text);
        }
    }

    /// Writes the title of the document, centred.
    fn title_text(&mut self, text: &str) {
        self.paragraph(&format!(
            "\\begin{{center}}\n{{\\LARGE\\bfseries {text}\\par}}\n\\end{{center}}"
        ));
    }

    /// Writes a heading as a starred section (the number, if any, is part
    /// of its text): `\section*` for the sections of the paper.
    fn heading(&mut self, block: &Block, level: u8) {
        let text = self.text.spans(&block.spans);
        if text.is_empty() {
            return;
        }
        let command = match heading_tag(level) {
            "h2" => "section",
            "h3" => "subsection",
            "h4" => "subsubsection",
            _ => "paragraph",
        };
        self.paragraph(&format!("\\{command}*{{{text}}}"));
    }

    /// Writes a list item: `\item[3.]` with the marker of the paper, its
    /// text after it.
    fn list_item(&mut self, block: &Block, marker: Option<&str>, level: u8) {
        self.nest(level.min(MAX_LIST_NESTING));
        let rest = marker.and_then(|marker| strip_label(&block.spans, marker));
        let item = match (marker, &rest) {
            (Some(marker), Some(_)) => format!("\\item[{{{}}}]", escape(marker)),
            _ => "\\item".to_string(),
        };
        let text = self.text.spans(rest.as_deref().unwrap_or(&block.spans));
        let text = if rest.is_some() { text } else { protect_start(&text) };
        let label = self.anchor_label(block);
        self.paragraph(format!("{item} {label}{text}").trim_end());
        self.item_open = true;
    }

    /// Opens or closes lists inside the open list until `level` are open.
    fn nest(&mut self, level: u8) {
        while self.nested > level {
            self.paragraph("\\end{itemize}");
            self.nested -= 1;
            self.item_open = true;
        }
        while self.nested < level {
            if !self.item_open {
                self.out.push_str("\\item[]\n");
            }
            self.paragraph("\\begin{itemize}");
            self.nested += 1;
            self.item_open = false;
        }
    }

    /// Writes a paragraph that continues a list item.
    fn list_continuation(&mut self, block: &Block) {
        if !self.item_open {
            self.out.push_str("\\item[]\n");
            self.item_open = true;
        }
        let text = self.text.spans(&block.spans);
        let label = self.anchor_label(block);
        if !text.is_empty() || !label.is_empty() {
            self.paragraph(&format!("{label}{text}"));
        }
    }

    /// Writes a footnote, small, with its mark raised; a note without a
    /// mark (a note in the margin of a book) as a quotation.
    fn footnote(&mut self, block: &Block, marker: &str) {
        let text = self.text.spans(&block.spans);
        if text.is_empty() {
            return;
        }
        if marker.is_empty() {
            self.paragraph(&format!("\\begin{{quote}}\n\\small {text}\n\\end{{quote}}"));
        } else {
            self.paragraph(&format!(
                "{{\\footnotesize\\noindent\\textsuperscript{{{}}}\\,{text}\\par}}",
                escape(marker)
            ));
        }
    }

    /// Writes an entry of a table of contents: indented by its level, with
    /// a leader to its page number.
    fn contents_entry(&mut self, block: &Block, level: u8, page: &str) {
        let text = self.text.spans(&block.spans);
        if text.is_empty() {
            return;
        }
        let indent = f32::from(level.saturating_sub(1)) * CONTENTS_INDENT_EM;
        let page = if page.is_empty() {
            String::new()
        } else {
            format!("\\dotfill {}", escape(page))
        };
        self.paragraph(&format!("\\noindent\\hspace*{{{indent}em}}{text}{page}\\par"));
    }

    /// Writes code lines as a `verbatim` listing.
    fn verbatim(&mut self, lines: &[String]) {
        let body: Vec<String> = lines
            .iter()
            .map(|line| {
                line.replace('\t', TAB_SPACES)
                    .replace("\\end{verbatim}", "\\end {verbatim}")
            })
            .collect();
        let body = body.join("\n");
        let body = body.trim_end();
        if body.trim().is_empty() {
            return;
        }
        self.paragraph(&format!(
            "\\begin{{small}}\n\\begin{{verbatim}}\n{body}\n\\end{{verbatim}}\n\\end{{small}}"
        ));
    }

    /// Writes a displayed formula: as LaTeX math when its structure is
    /// known (and wanted), as a picture otherwise; with its number. It
    /// continues the paragraph before it.
    fn equation(&mut self, block: &Block, image: Option<&ImageSpec>, number: Option<&str>) {
        let body = match (equation_math(block, image, self.opts.equations), image) {
            (Some(math), _) => latex(math).replace(['\n', '\r'], " "),
            (None, Some(image)) => format!("\\vcenter{{\\hbox{{{}}}}}", self.picture(image, "eq")),
            (None, None) => format!("\\text{{{}}}", self.text.spans(&block.spans)),
        };
        let tag = number
            .map(|number| number.trim().trim_start_matches('(').trim_end_matches(')').trim())
            .filter(|tag| !tag.is_empty());
        let display = match tag {
            Some(tag) => format!(
                "\\begin{{equation*}}\n{body}\n\\tag{{{}}}\n\\end{{equation*}}",
                escape(tag)
            ),
            None => format!("\\[\n{body}\n\\]"),
        };
        if self.after_text && self.out.ends_with("\n\n") {
            self.out.pop();
        }
        let label = self.anchor_label(block);
        if label.is_empty() {
            self.paragraph(&display);
        } else {
            self.paragraph(&format!("{label}\n{display}"));
        }
    }

    /// Writes a float: the rebuilt table or the picture, and the caption
    /// on the side it had in the paper. A picture and its caption stay on
    /// one page. A float whose text is all code is a listing.
    fn figure(&mut self, block: &Block, kind: FloatKind, image: Option<&ImageSpec>) {
        let caption = (!block.spans.is_empty()).then(|| self.caption(&block.spans));
        if kind == FloatKind::Algorithm
            && block.table.is_none()
            && let Some(lines) = code::float_listing_lines(block)
        {
            if let Some(caption) = &caption {
                self.paragraph(caption);
            }
            return self.verbatim(&lines);
        }
        let caption_first = caption_first(block, image);
        let content = if let Some(table) = block.table.as_ref().filter(|table| !table.rows.is_empty()) {
            // Footnotes are lost inside a table: its cells get the marks,
            // and the notes follow the table.
            *self.text.table_notes.borrow_mut() = Some(Vec::new());
            let tabular = self.tabular(table);
            let notes = self.text.table_notes.borrow_mut().take().unwrap_or_default();
            Some(format!(
                "\\begin{{center}}\n\\begin{{kakemonotable}}\n{tabular}\n\\end{{kakemonotable}}\n\\end{{center}}{}",
                footnote_texts(&notes)
            ))
        } else {
            image.map(|spec| {
                format!(
                    "\\begin{{center}}\n{}\n\\end{{center}}",
                    self.picture(spec, float_class(kind))
                )
            })
        };
        let mut parts: Vec<String> = Vec::new();
        if caption_first && let Some(caption) = &caption {
            parts.push(caption.clone());
        }
        parts.extend(content);
        if !caption_first && let Some(caption) = caption {
            parts.push(caption);
        }
        if parts.is_empty() {
            return;
        }
        if block.table.is_none() && image.is_some() {
            self.paragraph(&format!(
                "\\noindent\\begin{{minipage}}{{\\linewidth}}\n{}\n\\end{{minipage}}\n\\medskip",
                parts.join("\n\n")
            ));
        } else {
            for part in parts {
                self.paragraph(&part);
            }
        }
    }

    /// A caption, small, with its label ("Figure 3:") in bold.
    fn caption(&self, spans: &[Span]) -> String {
        let text = if let Some(first) = spans.first()
            && let Some(found) = CAP_LABEL.find(&first.text)
        {
            let label = first.text[..found.end()].trim_end();
            let mut rest = spans.to_vec();
            rest[0].text = first.text[label.len()..].to_string();
            format!("\\textbf{{{}}} {}", escape(label), self.text.spans(&rest))
                .trim_end()
                .to_string()
        } else {
            self.text.spans(spans)
        };
        format!("{{\\small {text}\\par}}")
    }

    /// The picture of a page area, at its size in the PDF, whose file name
    /// (with `hint` in it) is filled in later.
    fn picture(&mut self, spec: &ImageSpec, hint: &str) -> String {
        let width = spec.rect.width().max(1.0);
        let src = self.images.region(spec, hint);
        format!("\\kakemonopicture{{{width:.1}bp}}{{{src}}}")
    }

    /// A rebuilt table as a `tabular`, with `booktabs` rules above, below
    /// and under the header, and its spanning cells.
    fn tabular(&self, table: &Table) -> String {
        let n_cols = table.n_cols.max(1);
        let align: Vec<char> = (0..n_cols).map(|col| align_letter(table, col)).collect();
        let spec: String = align.iter().collect();
        let n_rows = table.rows.len();
        // For each position covered by a cell spanning from an earlier
        // row: the number of columns that cell spans, at its first column.
        let mut covered: Vec<Vec<Option<usize>>> = vec![vec![None; n_cols]; n_rows];
        let mut lines = vec![format!("\\begin{{tabular}}{{{spec}}}"), "\\toprule".to_string()];
        for (row_idx, row) in table.rows.iter().enumerate() {
            let mut cells: Vec<String> = Vec::new();
            let mut col = 0;
            let mut row_cells = row.iter().filter(|cell| cell.col < n_cols).peekable();
            while col < n_cols {
                while row_cells.next_if(|cell| cell.col < col).is_some() {}
                if let Some(width) = covered[row_idx][col] {
                    cells.push(multicolumn(width, align[col], String::new()));
                    col += width;
                } else if let Some(cell) = row_cells.next_if(|cell| cell.col == col) {
                    let colspan = cell.colspan.clamp(1, n_cols - col);
                    let rowspan = cell.rowspan.clamp(1, n_rows - row_idx);
                    for below in covered.iter_mut().take(row_idx + rowspan).skip(row_idx + 1) {
                        below[col] = Some(colspan);
                    }
                    cells.push(self.cell(cell, colspan, rowspan, align[col]));
                    col += colspan;
                } else {
                    cells.push(String::new());
                    col += 1;
                }
            }
            lines.push(format!("{} \\\\", cells.join(" & ")));
            if row_idx + 1 == table.header_rows && row_idx + 1 < n_rows {
                lines.push("\\midrule".to_string());
            }
        }
        lines.push("\\bottomrule".to_string());
        lines.push("\\end{tabular}".to_string());
        lines.join("\n")
    }

    /// One cell of a `tabular`, in `\multirow` and `\multicolumn` when it
    /// spans several rows or columns.
    fn cell(&self, cell: &TableCell, colspan: usize, rowspan: usize, align: char) -> String {
        let text = self.text.spans(&cell.spans);
        let text = if rowspan > 1 {
            format!("\\multirow{{{rowspan}}}{{*}}{{{text}}}")
        } else {
            text
        };
        multicolumn(colspan, align, protect_start(&text))
    }
}

/// The author groups, centred: each card a `tabular` (the name in bold,
/// then the affiliation lines), the cards side by side.
impl AuthorMarkup for Writer<'_, '_> {
    fn anchor(&mut self, block: &Block) {
        Writer::anchor(self, block);
    }

    fn lines(&self, block: &Block) -> Vec<String> {
        author_lines(
            block,
            |row| self.text.spans(row),
            "\\textbf{",
            |name| format!("\\textbf{{{name}}}"),
            |line| protect_start(&line),
        )
    }

    fn card(&self, lines: &[String]) -> String {
        format!(
            "\\begin{{tabular}}[t]{{@{{}}c@{{}}}}\n{}\n\\end{{tabular}}",
            lines.join("\\\\\n")
        )
    }

    fn cards(&mut self, cards: &[String]) {
        let row = cards.join(&format!("\n\\hspace{{{AUTHOR_GAP}}}\n"));
        self.paragraph(&format!("\\begin{{center}}\n{row}\n\\end{{center}}"));
    }

    fn byline(&mut self, lines: &[String]) {
        self.paragraph(&format!("\\begin{{center}}\n{}\n\\end{{center}}", lines.join("\\\\\n")));
    }
}

/// The share of the characters of `spans` (spaces apart) that are
/// monospaced.
fn mono_ratio(spans: &[Span]) -> f32 {
    let count = |mono_only: bool| {
        spans
            .iter()
            .filter(|span| !mono_only || span.style & MONO != 0)
            .map(|span| span.text.chars().filter(|ch| !ch.is_whitespace()).count())
            .sum::<usize>()
    };
    let total = count(false);
    if total == 0 {
        return 0.0;
    }
    count(true) as f32 / total as f32
}

/// The alignment letter (`l`, `c`, `r`) of column `col` of `table`.
fn align_letter(table: &Table, col: usize) -> char {
    match table.col_align.get(col).copied() {
        Some("center") => 'c',
        Some("right") => 'r',
        _ => 'l',
    }
}

/// `text` in a `\multicolumn` of `width` columns, or alone when `width` is 1.
fn multicolumn(width: usize, align: char, text: String) -> String {
    if width > 1 {
        format!("\\multicolumn{{{width}}}{{{align}}}{{{text}}}")
    } else {
        text
    }
}

/// `text` with an empty group before a `[` or `*` that starts it, which a
/// `\\` or an `\item` before it would read as its option.
fn protect_start(text: &str) -> String {
    if text.starts_with(['[', '*']) {
        format!("{{}}{text}")
    } else {
        text.to_string()
    }
}

/// The texts of the notes whose marks are in a table (`\footnotemark`),
/// numbered as their marks: the counter goes back by their number, then
/// steps once per note. Empty without notes.
fn footnote_texts(notes: &[String]) -> String {
    if notes.is_empty() {
        return String::new();
    }
    let texts: String = notes
        .iter()
        .map(|note| format!("\\stepcounter{{footnote}}\\footnotetext{{{note}}}"))
        .collect();
    format!("\n\\addtocounter{{footnote}}{{-{}}}{texts}", notes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_folder_is_safe_for_tex() {
        assert_eq!(
            tex_path("my images/1706.03762v7_assets/"),
            "my_images/1706.03762v7_assets"
        );
        assert_eq!(tex_path("a%b#c"), "a_b_c");
    }

    #[test]
    fn unknown_characters_are_outside_latin() {
        let chars: Vec<char> = unknown_chars("Erdős", "é ž 中 ſ").into_iter().collect();
        assert_eq!(chars, vec!['ſ', '中']);
    }

    #[test]
    fn options_of_items_are_protected() {
        assert_eq!(protect_start("[1] Smith"), "{}[1] Smith");
        assert_eq!(protect_start("Smith [1]"), "Smith [1]");
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
                vec![cell(1, "A", 1, 1), cell(2, "B", 1, 1)],
                vec![cell(0, "x & y", 1, 1), cell(2, "[3]", 1, 1)],
            ],
            rule_above: Vec::new(),
            rule_bottom: Vec::new(),
            rule_left: Vec::new(),
            rule_right: Vec::new(),
            geom: crate::model::TableGeom::default(),
        };
        let anchors = std::collections::HashSet::new();
        let mut images = ImageStore {
            opts: &HtmlOptions::default(),
            jobs: Vec::new(),
            vector: false,
        };
        let opts = LatexOptions::default();
        let writer = Writer {
            opts: &opts,
            text: Text {
                anchors: &anchors,
                math: true,
                table_notes: RefCell::default(),
            },
            images: &mut images,
            out: String::new(),
            group: Group::None,
            item_open: false,
            nested: 0,
            after_text: false,
        };
        assert_eq!(
            writer.tabular(&table),
            "\\begin{tabular}{lcr}\n\\toprule\n\\multirow{2}{*}{Model} & \\multicolumn{2}{c}{Score} \\\\\n \
             & A & B \\\\\n\\midrule\nx \\& y &  & {}[3] \\\\\n\\bottomrule\n\\end{tabular}"
        );
    }
}
