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
use super::{
    CAP_LABEL, Equations, HtmlOptions, ImageStore, Mode, PageImage, caption_first, code, code_run, doc_title,
    equation_math, float_class, heading_tag, is_abstract_text, is_scene_break, plain, stray_equation_number,
};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::{Block, BlockKind, Document, FloatKind, ImageSpec, MONO, Span, Table, TableCell};
use crate::paper::InputFormat;
use crate::render::Pictures;
use math::latex;
use std::collections::BTreeSet;
use text::{LAST_LATIN, LATIN_GAPS, Text, escape, label};

/// The packages and commands that every document uses.
const PREAMBLE: &str = include_str!("preamble.tex");
/// The end of the opening comment of [`PREAMBLE`], where [`CJK_NOTE`] goes.
const PREAMBLE_HEAD_END: &str = "folder next to this one.\n";
/// Said at the top of a document with CJK text.
const CJK_NOTE: &str = "% Its Chinese, Japanese or Korean text needs LuaLaTeX or XeLaTeX: pdfLaTeX\n\
                        % has no font for it and writes a ? for each of its characters.\n";
/// The end of the choice of fonts in [`PREAMBLE`], where [`CJK_FONTS`] go.
const FONTS_END: &str = "  \\usepackage{fontspec}\n\\fi\n";
/// The fonts of CJK text, for a document that has some. Noto Serif and
/// Noto Sans CJK, when installed, have the letters of the four languages;
/// otherwise Harano Aji, which TeX Live ships (LuaTeX-ja's own default,
/// and given to xeCJK by file name so that it is found in any TeX Live),
/// has Japanese only. Bold, sans-serif and monospaced text is in Gothic,
/// as is usual in Japanese; the CJK fonts follow the Latin ones into
/// sans-serif and monospaced text (`match`), and xeCJK keeps the spaces
/// between CJK characters (Korean words are separated by spaces).
const CJK_FONTS: &str = r"\ifLuaTeX
  \usepackage[match]{luatexja-fontspec}
  \IfFontExistsTF{Noto Serif CJK JP}{\setmainjfont{Noto Serif CJK JP}}{}
  \IfFontExistsTF{Noto Sans CJK JP}{\setsansjfont{Noto Sans CJK JP}}{}
\fi
\ifXeTeX
  \usepackage{xeCJK}
  \xeCJKsetup{CJKspace=true}
  \IfFontExistsTF{Noto Serif CJK JP}
    {\setCJKmainfont{Noto Serif CJK JP}}
    {\setCJKmainfont[BoldFont=HaranoAjiGothic-Bold.otf]{HaranoAjiMincho-Regular.otf}}
  \IfFontExistsTF{Noto Sans CJK JP}
    {\setCJKsansfont{Noto Sans CJK JP}\setCJKmonofont{Noto Sans CJK JP}}
    {\setCJKsansfont[BoldFont=HaranoAjiGothic-Bold.otf]{HaranoAjiGothic-Medium.otf}%
     \setCJKmonofont{HaranoAjiGothic-Medium.otf}}
\fi
";
/// Body rows of a table from which it is a `longtable`, broken across
/// pages with its header repeated, rather than one box shrunk to the width
/// of the text: a box never breaks, so a table taller than the page is cut
/// at its bottom, and a huge one exhausts TeX's memory. 30 rows of 11 pt
/// text are about 410 pt high, under two thirds of the text height (650
/// pt): a shorter table, with its caption, always fits on a page.
const LONG_TABLE_MIN_BODY_ROWS: usize = 30;
/// The font and column spacing of a long table, which is not shrunk to the
/// width of the text: (most columns, size command, font size in pt,
/// `\tabcolsep` in pt) in order, the first that the table's column count
/// fits deciding. The sizes are those of the 11 pt `article` class, whose
/// `\tabcolsep` is 6 pt.
const LONG_TABLE_SIZES: &[(usize, &str, f32, f32)] = &[
    (4, "", 10.95, 6.0),
    (6, "\\small", 10.0, 4.0),
    (9, "\\footnotesize", 9.0, 3.0),
    (usize::MAX, "\\scriptsize", 8.0, 2.0),
];
/// The height of the text in pt: letter paper with the 2.5 cm margins of
/// [`PREAMBLE`]. Only used to guess whether the header of a long table is
/// too tall to repeat.
const TEXT_HEIGHT_PT: f32 = 650.0;
/// The distance between the lines of a table, for its font size (that of
/// LaTeX's sizes).
const LINE_HEIGHT_RATIO: f32 = 1.2;
/// The share of the height of the text above which the header of a long
/// table is only on its first page, not repeated on each: a header of
/// narrow wrapped columns can take most of a page.
const LONG_TABLE_MAX_HEAD_RATIO: f32 = 0.25;
/// The share of an even split of the free width of a long table that a
/// wrapped column gets at least, when the line has no room for
/// [`MIN_WRAPPED_COLUMN_EM`] in every one: the rest is shared by their text.
const MIN_EVEN_SHARE_RATIO: f32 = 0.5;
/// The width of the text in pt: letter paper with the 2.5 cm margins of
/// [`PREAMBLE`]. Only used to guess whether a long table fits the line.
const TEXT_WIDTH_PT: f32 = 472.0;
/// The width guessed for a CJK character (fullwidth) in em, and for any
/// other character.
const CJK_CHAR_EM: f32 = 1.0;
/// See [`CJK_CHAR_EM`].
const OTHER_CHAR_EM: f32 = 0.5;
/// The narrowest a wrapped column of a long table is made, in em of its font.
const MIN_WRAPPED_COLUMN_EM: f32 = 4.0;
/// The packages of a long table, loaded when the document has one.
const LONG_TABLE_PACKAGES: &str = "\\usepackage{longtable}\n\\usepackage{array}\n";
/// Where [`LONG_TABLE_PACKAGES`] go in [`PREAMBLE`].
const TABLE_PACKAGES_END: &str = "\\usepackage{multirow}\n";
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
/// Longest line (in characters) of an author group set as a card beside
/// the others; a group with a longer line is a paragraph of its own.
const AUTHOR_CARD_MAX_CHARS: usize = 60;
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
pub fn render_latex(
    analysis: &Analysis,
    pdf: Option<&PdfFile>,
    input: InputFormat,
    opts: &LatexOptions,
) -> LatexOutput {
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
        group: Group::None,
        item_open: false,
        nested: 0,
        after_text: false,
    };
    writer.write(doc);
    let body = writer.out.trim_end().to_string();
    let out = images.finish(Pictures::new(pdf, &analysis.doc), body);
    LatexOutput {
        latex: document(doc, &out.html, input),
        files: out.files,
    }
}

/// The whole `.tex` file: the preamble (naming the format `input` the
/// document was read from, with the CJK fonts of XeLaTeX and LuaLaTeX when
/// the text has CJK characters), the characters that pdfLaTeX has no glyph
/// for, the title of the PDF, and `body`.
fn document(doc: &Document, body: &str, input: InputFormat) -> String {
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
    let source = match input {
        InputFormat::Pdf => "a PDF",
        InputFormat::Docx | InputFormat::Doc => "a Word document",
        InputFormat::Xlsx => "an Excel workbook",
    };
    out = out.replacen("Converted from a PDF", &format!("Converted from {source}"), 1);
    if body.contains("\\begin{longtable}") {
        out = out.replacen(
            TABLE_PACKAGES_END,
            &format!("{TABLE_PACKAGES_END}{LONG_TABLE_PACKAGES}"),
            1,
        );
    }
    if title.chars().chain(body.chars()).any(is_cjk) {
        out = out.replacen(PREAMBLE_HEAD_END, &format!("{PREAMBLE_HEAD_END}{CJK_NOTE}"), 1);
        out = out.replacen(FONTS_END, &format!("{FONTS_END}{CJK_FONTS}"), 1);
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

/// Whether `ch` is Chinese, Japanese or Korean: a Han ideograph, kana,
/// Hangul, Bopomofo, CJK punctuation or a fullwidth form. The fonts of
/// [`PREAMBLE`] have none of them; [`CJK_FONTS`] gives them fonts.
fn is_cjk(ch: char) -> bool {
    matches!(
        u32::from(ch),
        0x1100..=0x11FF // Hangul Jamo
            | 0x2E80..=0x2FDF // CJK and Kangxi radicals
            | 0x3000..=0x303F // CJK symbols and punctuation
            | 0x3040..=0x30FF // Hiragana, Katakana
            | 0x3100..=0x312F // Bopomofo
            | 0x3130..=0x318F // Hangul compatibility Jamo
            | 0x31A0..=0x31FF // Bopomofo extended, CJK strokes, Katakana extension
            | 0x3200..=0x33FF // Enclosed CJK, CJK compatibility
            | 0x3400..=0x4DBF // Han, extension A
            | 0x4E00..=0x9FFF // Han
            | 0xA960..=0xA97F // Hangul Jamo extended A
            | 0xAC00..=0xD7FF // Hangul syllables, Jamo extended B
            | 0xF900..=0xFAFF // CJK compatibility ideographs
            | 0xFE30..=0xFE4F // CJK compatibility forms
            | 0xFF00..=0xFFEF // Halfwidth and fullwidth forms
            | 0x20000..=0x3FFFF // Han, extensions B and after
    )
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
    /// Writes every block of `doc`. Runs of code paragraphs become one
    /// listing, the title blocks one title, and the heading of the abstract
    /// is left to the `abstract` environment.
    fn write(&mut self, doc: &Document) {
        let blocks: Vec<&Block> = doc.blocks.iter().collect();
        let mut idx = 0;
        let mut after_front = false;
        while idx < blocks.len() {
            let block = blocks[idx];
            if !after_front && !matches!(block.kind, BlockKind::Title | BlockKind::Authors | BlockKind::Note) {
                self.arxiv_link(doc);
                after_front = true;
            }
            if let Some(end) = code_run(&blocks, idx) {
                self.set_group(Group::None);
                self.anchor(block);
                self.verbatim(&code::listing_lines(&blocks[idx..end]));
                idx = end;
                continue;
            }
            self.set_group(Group::of(block, self.group));
            match &block.kind {
                BlockKind::Title => {
                    idx = self.title(doc, &blocks, idx);
                    continue;
                }
                BlockKind::Authors => {
                    idx = self.authors(&blocks, idx);
                    continue;
                }
                BlockKind::Heading { .. }
                    if is_abstract_text(&block.spans)
                        && blocks
                            .get(idx + 1)
                            .is_some_and(|next| matches!(next.kind, BlockKind::Paragraph { in_abstract: true })) =>
                {
                    self.anchor(block);
                    idx += 1;
                    continue;
                }
                // An equation whose number was read as a paragraph of its
                // own takes it back.
                BlockKind::Equation { image, number: None } => {
                    if let Some(number) = blocks.get(idx + 1).and_then(|next| stray_equation_number(next)) {
                        self.equation(block, image.as_ref(), Some(&number));
                        idx += 2;
                        continue;
                    }
                }
                _ => {}
            }
            self.block(block);
            idx += 1;
        }
        if !after_front {
            self.arxiv_link(doc);
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
            BlockKind::Authors => {
                self.authors(std::slice::from_ref(&block), 0);
            }
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
        block
            .anchor
            .as_deref()
            .filter(|anchor| self.text.anchors.contains(*anchor))
            .map(|anchor| format!("\\phantomsection\\label{{{}}}", label(anchor)))
            .unwrap_or_default()
    }

    /// Writes the `\label` of a block that some text links to, on a line
    /// of its own.
    fn anchor(&mut self, block: &Block) {
        let label = self.anchor_label(block);
        if !label.is_empty() {
            self.out.push_str(&label);
            self.out.push('\n');
        }
    }

    /// Writes a link to the arXiv page of the paper, after the front matter.
    fn arxiv_link(&mut self, doc: &Document) {
        if let Some(id) = &doc.meta.arxiv_id {
            let link = format!("\\href{{https://arxiv.org/abs/{}}}{{arXiv:{}}}", escape(id), escape(id));
            self.paragraph(&format!("\\begin{{center}}\n{link}\n\\end{{center}}"));
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
        let text = self.text.spans(&spans);
        if text.is_empty() {
            self.title_text(&escape(&doc_title(doc)));
        } else {
            self.title_text(&text);
        }
        end
    }

    /// Writes the title of the document, centred.
    fn title_text(&mut self, text: &str) {
        self.paragraph(&format!(
            "\\begin{{center}}\n{{\\LARGE\\bfseries {text}\\par}}\n\\end{{center}}"
        ));
    }

    /// Writes the author groups that start at `blocks[start]`, centred, and
    /// returns the index of the block after them. Each group (the name in
    /// bold, then the affiliation lines) is a card, and the cards stand
    /// side by side as far as the width allows; a group with a line too
    /// long for a card (a list of many authors) is a centred paragraph.
    fn authors(&mut self, blocks: &[&Block], start: usize) -> usize {
        let end = start
            + blocks[start..]
                .iter()
                .take_while(|block| matches!(block.kind, BlockKind::Authors))
                .count();
        let mut cards: Vec<String> = Vec::new();
        for block in &blocks[start..end] {
            self.anchor(block);
            let lines = self.author_lines(block);
            if lines.is_empty() {
                continue;
            }
            if author_rows(block)
                .iter()
                .all(|row| plain(row).chars().count() <= AUTHOR_CARD_MAX_CHARS)
            {
                cards.push(format!(
                    "\\begin{{tabular}}[t]{{@{{}}c@{{}}}}\n{}\n\\end{{tabular}}",
                    lines.join("\\\\\n")
                ));
            } else {
                self.author_cards(&mut cards);
                self.paragraph(&format!("\\begin{{center}}\n{}\n\\end{{center}}", lines.join("\\\\\n")));
            }
        }
        self.author_cards(&mut cards);
        end
    }

    /// Writes the author cards collected so far, side by side, and empties
    /// `cards`.
    fn author_cards(&mut self, cards: &mut Vec<String>) {
        if !cards.is_empty() {
            let row = cards.join(&format!("\n\\hspace{{{AUTHOR_GAP}}}\n"));
            self.paragraph(&format!("\\begin{{center}}\n{row}\n\\end{{center}}"));
            cards.clear();
        }
    }

    /// The lines of an author group: the name in bold, then the
    /// affiliation lines.
    fn author_lines(&self, block: &Block) -> Vec<String> {
        author_rows(block)
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let text = self.text.spans(row);
                if i == 0 && !text.is_empty() && !text.starts_with("\\textbf{") {
                    format!("\\textbf{{{text}}}")
                } else {
                    protect_start(&text)
                }
            })
            .filter(|text| !text.is_empty())
            .collect()
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
            Some(
                if table.rows.len().saturating_sub(table.header_rows) > LONG_TABLE_MIN_BODY_ROWS {
                    self.longtable(table)
                } else {
                    format!(
                        "\\begin{{center}}\n\\begin{{kakemonotable}}\n{}\n\\end{{kakemonotable}}\n\\end{{center}}",
                        self.tabular(table)
                    )
                },
            )
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
        let cols = Columns::plain(table);
        let spec: String = (0..cols.align.len()).map(|col| cols.spec(col)).collect();
        let mut lines = vec![format!("\\begin{{tabular}}{{{spec}}}"), "\\toprule".to_string()];
        lines.extend(self.rows(table, &cols, false));
        lines.push("\\bottomrule".to_string());
        lines.push("\\end{tabular}".to_string());
        lines.join("\n")
    }

    /// A rebuilt table as a `longtable`, which breaks across pages with
    /// its header rows repeated at the top of each, in a smaller font when
    /// it has many columns ([`LONG_TABLE_SIZES`]); its long columns wrap
    /// when it would be wider than the text.
    fn longtable(&self, table: &Table) -> String {
        let n_cols = table.n_cols.max(1);
        let &(_, size, size_pt, tabcolsep_pt) = LONG_TABLE_SIZES
            .iter()
            .find(|(most, ..)| n_cols <= *most)
            .expect("the last size takes any count");
        let cols = Columns::fitted(table, size_pt, tabcolsep_pt);
        let spec: String = (0..n_cols).map(|col| cols.spec(col)).collect();
        let mut lines = Vec::new();
        if !size.is_empty() {
            lines.push(format!(
                "\\begingroup{size}\\setlength{{\\tabcolsep}}{{{tabcolsep_pt}pt}}"
            ));
        }
        lines.push(format!("\\begin{{longtable}}{{{spec}}}"));
        lines.push("\\toprule".to_string());
        let rows = self.rows(table, &cols, true);
        let header = table.header_rows.min(table.rows.len());
        if header > 0 && header < table.rows.len() {
            // The header rows and the rule under them: `rows` puts it
            // after the last header row. A header too tall to repeat is
            // only on the first page.
            lines.extend(rows[..=header].iter().cloned());
            if cols.header_lines(table) as f32 * size_pt * LINE_HEIGHT_RATIO
                > LONG_TABLE_MAX_HEAD_RATIO * TEXT_HEIGHT_PT
            {
                lines.push("\\endfirsthead".to_string());
                lines.push("\\toprule".to_string());
            }
            lines.push("\\endhead".to_string());
            lines.extend(rows[header + 1..].iter().cloned());
        } else {
            lines.extend(rows);
        }
        lines.push("\\bottomrule".to_string());
        lines.push("\\end{longtable}".to_string());
        if !size.is_empty() {
            lines.push("\\endgroup".to_string());
        }
        lines.join("\n")
    }

    /// The rows of `table` set in the columns `cols`, one line each, with a
    /// `\midrule` line after the header rows. With `split_header`, a cell
    /// of the header spans no row of the body (the header is repeated on
    /// each page of a long table, apart from its body).
    fn rows(&self, table: &Table, cols: &Columns, split_header: bool) -> Vec<String> {
        let n_cols = table.n_cols.max(1);
        let n_rows = table.rows.len();
        // For each position covered by a cell spanning from an earlier
        // row: the number of columns that cell spans, at its first column.
        let mut covered: Vec<Vec<Option<usize>>> = vec![vec![None; n_cols]; n_rows];
        let mut lines = Vec::new();
        for (row_idx, row) in table.rows.iter().enumerate() {
            let last_row = if split_header && row_idx < table.header_rows {
                table.header_rows.min(n_rows)
            } else {
                n_rows
            };
            let mut cells: Vec<String> = Vec::new();
            let mut col = 0;
            let mut row_cells = row.iter().filter(|cell| cell.col < n_cols).peekable();
            while col < n_cols {
                while row_cells.next_if(|cell| cell.col < col).is_some() {}
                if let Some(width) = covered[row_idx][col] {
                    cells.push(multicolumn(width, &cols.span_spec(col, width), String::new()));
                    col += width;
                } else if let Some(cell) = row_cells.next_if(|cell| cell.col == col) {
                    let colspan = cell.colspan.clamp(1, n_cols - col);
                    let rowspan = cell.rowspan.clamp(1, last_row - row_idx);
                    for below in covered.iter_mut().take(row_idx + rowspan).skip(row_idx + 1) {
                        below[col] = Some(colspan);
                    }
                    cells.push(self.cell(cell, col, colspan, rowspan, cols));
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
        lines
    }

    /// One cell of a table, at column `col` of `cols`, in `\multirow` and
    /// `\multicolumn` when it spans several rows or columns.
    fn cell(&self, cell: &TableCell, col: usize, colspan: usize, rowspan: usize, cols: &Columns) -> String {
        let text = self.text.spans(&cell.spans);
        let text = if rowspan > 1 {
            // In a wrapped column, `=` sets the text at the column's width.
            let width = if cols.wraps(col, colspan) { "=" } else { "*" };
            format!("\\multirow{{{rowspan}}}{{{width}}}{{{text}}}")
        } else {
            text
        };
        multicolumn(colspan, &cols.span_spec(col, colspan), protect_start(&text))
    }
}

/// The columns of a table as LaTeX sets them: each column's alignment, and
/// whether its text wraps in a `p` column, with its share of the width of
/// the text (its two `\tabcolsep` included).
struct Columns {
    /// The alignment letter (`l`, `c`, `r`) of each column.
    align: Vec<char>,
    /// The text of the column wraps.
    wrap: Vec<bool>,
    /// The share of the width of the text each column takes (guessed for
    /// one that does not wrap).
    share: Vec<f32>,
    /// The width of an em of the table's font, and the space on each side
    /// of a column, as shares of the width of the text.
    em_share: f32,
    /// See [`Columns::em_share`].
    pad_share: f32,
}

impl Columns {
    /// Columns as wide as their text: none wraps.
    fn plain(table: &Table) -> Self {
        let n_cols = table.n_cols.max(1);
        Self {
            align: (0..n_cols).map(|col| align_letter(table, col)).collect(),
            wrap: vec![false; n_cols],
            share: vec![0.0; n_cols],
            em_share: 0.0,
            pad_share: 0.0,
        }
    }

    /// The lines the header rows of `table` take, guessed: in each row,
    /// those of its tallest cell, a cell of wrapped columns taking as many
    /// as its text is wider than they are.
    fn header_lines(&self, table: &Table) -> usize {
        let n_cols = self.align.len();
        table
            .rows
            .iter()
            .take(table.header_rows)
            .map(|row| {
                row.iter()
                    .filter(|cell| cell.col < n_cols)
                    .map(|cell| {
                        let width = cell.colspan.clamp(1, n_cols - cell.col);
                        if !self.wraps(cell.col, width) {
                            return 1;
                        }
                        let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
                        let share: f32 = self.share[cell.col..cell.col + width].iter().sum();
                        let room_em = ((share - 2.0 * self.pad_share) / self.em_share).max(1.0);
                        (text_em(&text) / room_em).ceil().max(1.0) as usize
                    })
                    .max()
                    .unwrap_or(1)
            })
            .sum()
    }

    /// The columns of a long table set at `size_pt`: as wide as their text
    /// when the table then fits the width of the text ([`TEXT_WIDTH_PT`]).
    /// Otherwise the columns wider than an even share of the line wrap, and
    /// share what the others leave in proportion to the mean width of
    /// their body cells, so that a typical row takes as many lines in each
    /// of them; each is at least [`MIN_WRAPPED_COLUMN_EM`] wide when there
    /// is room for that ([`MIN_EVEN_SHARE_RATIO`]). The columns are
    /// `tabcolsep_pt` apart on each side.
    fn fitted(table: &Table, size_pt: f32, tabcolsep_pt: f32) -> Self {
        let mut cols = Self::plain(table);
        let n_cols = cols.align.len();
        // The width of a column's longest body cell on one line, or of the
        // longest word of a header cell (which may wrap); and the total
        // width and count of its body cells with text.
        let mut natural = vec![0.0f32; n_cols];
        let mut body = vec![(0.0f32, 0usize); n_cols];
        // The longest word of each column's body cells, and whether one of
        // them has more than one word (a column of numbers or codes has
        // none: it could not wrap).
        let mut word = vec![0.0f32; n_cols];
        let mut breaks = vec![false; n_cols];
        for (row, cells) in table.rows.iter().enumerate() {
            for cell in cells.iter().filter(|cell| cell.colspan <= 1 && cell.col < n_cols) {
                let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
                let em = if row < table.header_rows {
                    word_em(&text)
                } else {
                    text_em(&text)
                };
                natural[cell.col] = natural[cell.col].max(em);
                if row >= table.header_rows && em > 0.0 {
                    body[cell.col].0 += em;
                    body[cell.col].1 += 1;
                    word[cell.col] = word[cell.col].max(word_em(&text));
                    breaks[cell.col] |= word_em(&text) < em;
                }
            }
        }
        cols.em_share = size_pt / TEXT_WIDTH_PT;
        cols.pad_share = tabcolsep_pt / TEXT_WIDTH_PT;
        let share_of = |em: f32| (em * size_pt + 2.0 * tabcolsep_pt) / TEXT_WIDTH_PT;
        cols.share = natural.iter().map(|&em| share_of(em)).collect();
        if cols.share.iter().sum::<f32>() <= 1.0 {
            return cols;
        }
        let even = 1.0 / n_cols as f32;
        cols.wrap = (0..n_cols).map(|col| cols.share[col] > even && breaks[col]).collect();
        let wrapped: Vec<usize> = (0..n_cols).filter(|&col| cols.wrap[col]).collect();
        let kept: f32 = (0..n_cols)
            .filter(|&col| !cols.wrap[col])
            .map(|col| cols.share[col])
            .sum();
        let left = 1.0 - kept;
        let weight = |col: usize| {
            let (sum, count) = body[col];
            (if count == 0 { 0.0 } else { sum / count as f32 }).max(MIN_WRAPPED_COLUMN_EM)
        };
        if left <= 0.0 {
            // The others leave nothing: the table is wider than the text.
            for &col in &wrapped {
                cols.share[col] = share_of(MIN_WRAPPED_COLUMN_EM);
            }
            return cols;
        }
        // Each wrapped column gets at least its floor: the least width (or
        // its longest word, when longer), or part of an even share of what
        // is left when that is less. The others share the rest in
        // proportion to their weights.
        let even_left = left / wrapped.len() as f32;
        let floor = |col: usize| share_of(MIN_WRAPPED_COLUMN_EM.max(word[col])).min(MIN_EVEN_SHARE_RATIO * even_left);
        let mut floored = vec![false; n_cols];
        loop {
            let free: Vec<usize> = wrapped.iter().copied().filter(|&col| !floored[col]).collect();
            let rest = left
                - wrapped
                    .iter()
                    .filter(|&&col| floored[col])
                    .map(|&col| floor(col))
                    .sum::<f32>();
            let total: f32 = free.iter().map(|&col| weight(col)).sum();
            let mut changed = false;
            for &col in &free {
                cols.share[col] = rest * weight(col) / total;
                if cols.share[col] < floor(col) {
                    floored[col] = true;
                    changed = true;
                }
            }
            for &col in wrapped.iter().filter(|&&col| floored[col]) {
                cols.share[col] = floor(col);
            }
            if !changed || free.is_empty() {
                break;
            }
        }
        cols
    }

    /// Whether any of the `width` columns from `col` wraps.
    fn wraps(&self, col: usize, width: usize) -> bool {
        self.wrap[col..col + width].iter().any(|&wrap| wrap)
    }

    /// The column specification of column `col`.
    fn spec(&self, col: usize) -> String {
        self.span_spec(col, 1)
    }

    /// The column specification of the `width` columns from `col` taken
    /// together (a `\multicolumn`): the alignment of the first when none
    /// wraps, else a `p` column as wide as all of them.
    fn span_spec(&self, col: usize, width: usize) -> String {
        let align = self.align[col];
        if !self.wraps(col, width) {
            return align.to_string();
        }
        let share: f32 = self.share[col..col + width].iter().sum();
        let ragged = match align {
            'c' => "\\centering",
            'r' => "\\raggedleft",
            _ => "\\raggedright",
        };
        format!(">{{{ragged}\\arraybackslash}}p{{\\dimexpr{share:.4}\\linewidth-2\\tabcolsep\\relax}}")
    }
}

/// The width of the longest part of `text` that cannot be broken across
/// lines, guessed in em: a word, a CJK character (a line may break after
/// any).
fn word_em(text: &str) -> f32 {
    text.split(|ch: char| ch.is_whitespace() || is_cjk(ch))
        .map(text_em)
        .fold(if text.chars().any(is_cjk) { CJK_CHAR_EM } else { 0.0 }, f32::max)
}

/// The width of `text` set on one line, guessed in em.
fn text_em(text: &str) -> f32 {
    text.chars()
        .map(|ch| if is_cjk(ch) { CJK_CHAR_EM } else { OTHER_CHAR_EM })
        .sum()
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

/// The rows of an author group: its lines, or its text as one line.
fn author_rows(block: &Block) -> &[Vec<Span>] {
    if block.rows.is_empty() {
        std::slice::from_ref(&block.spans)
    } else {
        &block.rows[..]
    }
}

/// The alignment letter (`l`, `c`, `r`) of column `col` of `table`.
fn align_letter(table: &Table, col: usize) -> char {
    match table.col_align.get(col).copied() {
        Some("center") => 'c',
        Some("right") => 'r',
        _ => 'l',
    }
}

/// `text` in a `\multicolumn` of `width` columns set as `spec`, or alone
/// when `width` is 1.
fn multicolumn(width: usize, spec: &str, text: String) -> String {
    if width > 1 {
        format!("\\multicolumn{{{width}}}{{{spec}}}{{{text}}}")
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

    /// A document without pages whose only paragraph is `text`.
    fn doc_of(text: &str) -> Document {
        let mut block = Block::new(0, BlockKind::Paragraph { in_abstract: false });
        block.spans = vec![Span::plain(text)];
        Document {
            meta: crate::model::Meta::default(),
            pages: Vec::new(),
            body_size: 10.0,
            leading: 12.0,
            body_font: String::new(),
            math_font: String::new(),
            blocks: vec![block],
            pictures: Vec::new(),
        }
    }

    #[test]
    fn cjk_text_gets_cjk_fonts_for_lualatex_and_xelatex() {
        for text in ["一貫性のある操作", "ひらがな", "カタカナ", "한국어", "全角，"] {
            let tex = document(&doc_of(text), text, InputFormat::Xlsx);
            assert!(tex.contains("\\usepackage[match]{luatexja-fontspec}"), "{text}: {tex}");
            assert!(tex.contains("\\usepackage{xeCJK}"), "{text}");
            assert!(tex.contains("\\setCJKmainfont"), "{text}");
            assert!(tex.contains("needs LuaLaTeX or XeLaTeX"), "{text}");
        }
    }

    #[test]
    fn text_without_cjk_keeps_the_plain_preamble() {
        let text = "Erdős café ž";
        let tex = document(&doc_of(text), text, InputFormat::Pdf);
        assert!(tex.starts_with(PREAMBLE), "{tex}");
        assert!(!tex.contains("luatexja") && !tex.contains("xeCJK"));
    }

    /// A table of `n_cols` columns: a header row, then `body` rows, each
    /// cell's text given by `text(row, col)`.
    fn table_of(n_cols: usize, body: usize, text: impl Fn(usize, usize) -> String) -> Table {
        let row = |row: usize| {
            (0..n_cols)
                .map(|col| TableCell {
                    col,
                    colspan: 1,
                    rowspan: 1,
                    spans: vec![Span::plain(text(row, col))],
                })
                .collect()
        };
        Table {
            n_cols,
            header_rows: 1,
            col_align: vec!["left"; n_cols],
            rows: (0..=body).map(row).collect(),
            rule_above: Vec::new(),
            rule_bottom: Vec::new(),
            rule_left: Vec::new(),
            rule_right: Vec::new(),
            geom: crate::model::TableGeom::default(),
        }
    }

    /// The LaTeX of a table block holding `table`.
    fn table_latex(table: Table) -> String {
        let mut block = Block::new(
            0,
            BlockKind::Figure {
                kind: FloatKind::Table,
                image: None,
                label: None,
            },
        );
        block.table = Some(table);
        let anchors = std::collections::HashSet::new();
        let html = HtmlOptions::default();
        let mut images = ImageStore {
            opts: &html,
            jobs: Vec::new(),
        };
        let opts = LatexOptions::default();
        let mut writer = Writer {
            opts: &opts,
            text: Text {
                anchors: &anchors,
                math: true,
            },
            images: &mut images,
            out: String::new(),
            group: Group::None,
            item_open: false,
            nested: 0,
            after_text: false,
        };
        writer.figure(&block, FloatKind::Table, None);
        writer.out
    }

    #[test]
    fn a_short_table_is_one_box() {
        let latex = table_latex(table_of(3, 30, |row, col| format!("r{row}c{col}")));
        assert!(latex.contains("\\begin{kakemonotable}"), "{latex}");
        assert!(!latex.contains("longtable"));
    }

    #[test]
    fn a_long_table_breaks_across_pages_with_its_header_repeated() {
        let latex = table_latex(table_of(3, 31, |row, col| format!("r{row}c{col}")));
        assert!(!latex.contains("kakemonotable"), "{latex}");
        assert!(latex.contains("\\begin{longtable}{lll}"), "{latex}");
        let head = latex
            .find("r0c0 & r0c1 & r0c2 \\\\\n\\midrule\n\\endhead")
            .expect("no repeated header");
        assert!(latex.find("r1c0").unwrap() > head);
        assert!(latex.contains("r31c0 & r31c1 & r31c2 \\\\\n\\bottomrule\n\\end{longtable}"));
    }

    #[test]
    fn a_long_table_with_many_columns_is_set_smaller() {
        let latex = table_latex(table_of(8, 40, |row, col| format!("{row}.{col}")));
        assert!(
            latex.contains("\\begingroup\\footnotesize\\setlength{\\tabcolsep}{3pt}\n\\begin{longtable}"),
            "{latex}"
        );
    }

    #[test]
    fn a_long_table_too_wide_for_the_page_wraps_its_long_columns() {
        let latex = table_latex(table_of(4, 40, |row, col| {
            if col == 2 {
                format!("{row} {}", "a long description ".repeat(10))
            } else {
                format!("{row}")
            }
        }));
        let spec_line = latex
            .lines()
            .find(|line| line.starts_with("\\begin{longtable}"))
            .unwrap();
        assert!(
            spec_line.starts_with("\\begin{longtable}{ll>{\\raggedright\\arraybackslash}p{"),
            "{spec_line}"
        );
        assert!(spec_line.ends_with("}l}"), "{spec_line}");
    }

    #[test]
    fn a_long_table_with_a_header_taller_than_a_quarter_page_shows_it_once() {
        let latex = table_latex(table_of(16, 40, |row, col| {
            if row == 0 {
                "a long header of many words ".repeat(6)
            } else {
                format!("{row} {col} text of the cell {}", "word ".repeat(col))
            }
        }));
        assert!(
            latex.contains("\\midrule\n\\endfirsthead\n\\toprule\n\\endhead"),
            "{latex}"
        );
        let short = table_latex(table_of(16, 40, |row, col| {
            format!("{row} {col} text {}", "word ".repeat(col))
        }));
        assert!(!short.contains("\\endfirsthead") && short.contains("\\endhead"));
    }

    #[test]
    fn long_tables_load_their_packages() {
        let text = "\\begin{longtable}{ll}\na & b \\\\\n\\end{longtable}";
        let tex = document(&doc_of("x"), text, InputFormat::Pdf);
        assert!(tex.contains("\\usepackage{longtable}") && tex.contains("\\usepackage{array}"));
        assert!(!document(&doc_of("x"), "x", InputFormat::Pdf).contains("longtable"));
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
        };
        let opts = LatexOptions::default();
        let writer = Writer {
            opts: &opts,
            text: Text {
                anchors: &anchors,
                math: true,
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
