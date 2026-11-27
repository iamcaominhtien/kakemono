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
use crate::model::{BOLD, Block, BlockKind, Document, FloatKind, ImageSpec, MONO, Span, Table, TableCell};
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
/// The symbols that the escape of text writes as math, which CJK text uses
/// as characters (△ is the minus sign of Japanese statistics) and the CJK
/// fonts have: in a document with CJK text they are `\kakemonotext`, the
/// character under XeLaTeX and LuaLaTeX, math under pdfLaTeX.
const CJK_TEXT_SYMBOLS: &[(char, &str)] = &[
    ('△', "\\triangle"),
    ('□', "\\square"),
    ('〈', "\\langle"),
    ('〉', "\\rangle"),
];
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
/// xeCJK takes the symbols of Japanese text (①, ―, ≦, ●, ☆, box drawing,
/// arrows) for Latin ones and looks for them in Latin Modern, which lacks
/// them: those of the dash, Roman numerals, arrows, mathematical operators,
/// enclosed alphanumerics, box drawing, geometric shapes and miscellaneous
/// symbols blocks that both Noto Serif CJK and Harano Aji Mincho have and
/// Latin Modern has not (checked with `fc-query`) are declared CJK. LuaTeX-ja
/// sets them in its Japanese font by itself.
const CJK_FONTS: &str = r#"\ifLuaTeX
  \usepackage[match]{luatexja-fontspec}
  \IfFontExistsTF{Noto Serif CJK JP}{\setmainjfont{Noto Serif CJK JP}}{}
  \IfFontExistsTF{Noto Sans CJK JP}{\setsansjfont{Noto Sans CJK JP}}{}
\fi
\ifXeTeX
  \usepackage{xeCJK}
  \xeCJKsetup{CJKspace=true}
  \xeCJKDeclareCharClass{CJK}{"2015, "2160->"216B, "2170->"217B, "2194, "2196->"2199,
    "21C4->"21C6, "21CB->"21CC, "21D0, "21D2, "21D4, "21E6->"21E9, "21F5, "2200,
    "2202->"2203, "2205->"220B, "220F, "2211, "2213, "221D, "221F->"2220, "2225->"222E,
    "2234->"2235, "223D, "2243, "2245, "2248, "2260->"2262, "2264->"2267, "226A->"226B,
    "2272->"2273, "2276->"2277, "2282->"2287, "228A->"228B, "2295->"2298, "22A0, "22A5,
    "22BF, "22DA->"22DB, "22EF, "2460->"2492, "249C->"254B, "2550, "255E, "2561, "256A,
    "256D->"2573, "25A0->"25A2, "25AA->"25AB, "25B1->"25B3, "25B6->"25B7, "25BC->"25BD,
    "25C0->"25C1, "25C6->"25C7, "25C9->"25CC, "25CE->"25D3, "25E2->"25E5, "25EF,
    "2600->"2603, "2605->"2606, "260E, "2616->"2617, "261C->"261F, "2640, "2642,
    "2660->"2669, "266B->"266F, "2672->"267D, "26A0, "26BD->"26BE}
  \IfFontExistsTF{Noto Serif CJK JP}
    {\setCJKmainfont{Noto Serif CJK JP}}
    {\setCJKmainfont[BoldFont=HaranoAjiGothic-Bold.otf]{HaranoAjiMincho-Regular.otf}}
  \IfFontExistsTF{Noto Sans CJK JP}
    {\setCJKsansfont{Noto Sans CJK JP}\setCJKmonofont{Noto Sans CJK JP}}
    {\setCJKsansfont[BoldFont=HaranoAjiGothic-Bold.otf]{HaranoAjiGothic-Medium.otf}%
     \setCJKmonofont{HaranoAjiGothic-Medium.otf}}
\fi
% A symbol of CJK text: the character #1, or the math symbol #2 under pdfLaTeX.
\newcommand{\kakemonotext}[2]{\ifPDFTeX\ensuremath{#2}\else#1\fi}
"#;
/// Most body rows of a table set as one box, shrunk to the width of the
/// text; a table with more is a `longtable`, broken across pages with its
/// header repeated: a box never breaks, so a table taller than the page is
/// cut at its bottom, and a huge one exhausts TeX's memory. 30 rows of
/// 11 pt text are about 410 pt high, under two thirds of the text height
/// (650 pt): a table of at most 30 rows, with its caption, always fits on
/// a page.
const MAX_BOXED_TABLE_BODY_ROWS: usize = 30;
/// The font and column spacing of a long table, which is not shrunk to the
/// width of the text: (most columns, size command, font size in pt,
/// `\tabcolsep` in pt). The first that the table's column count fits is
/// tried first; when the table is then wider than the text, the next ones
/// in turn. `\tiny` is only such a last resort. The sizes are those of the
/// 11 pt `article` class, whose `\tabcolsep` is 6 pt.
const LONG_TABLE_SIZES: &[(usize, &str, f32, f32)] = &[
    (4, "", 10.95, 6.0),
    (6, "\\small", 10.0, 4.0),
    (9, "\\footnotesize", 9.0, 3.0),
    (usize::MAX, "\\scriptsize", 8.0, 2.0),
    (usize::MAX, "\\tiny", 6.0, 1.0),
];
/// A table with no geometry (read from a workbook or a Word document) too
/// wide for the text even in `\scriptsize`: shrunk to the width of the text
/// in a box, it would be unreadable, so it is a `longtable` (turned, or cut
/// into groups of columns, when it must be). A paper's table, which fitted
/// its page, is boxed as before.
fn too_wide_to_box(table: &Table) -> bool {
    let readable = &LONG_TABLE_SIZES[..LONG_TABLE_SIZES.len() - 1];
    table.geom.col_bounds.is_empty()
        && !readable
        .iter()
        .any(|size| Columns::fitted(table, size.2, size.3, TEXT_WIDTH_PT).fits())
}

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
/// The widest a wrapped column of a long table is kept for its typical
/// cell, in em: it is at least as wide as the mean of its body cells (or
/// its longest word) up to this, so that a short label (a name, a CJK
/// term of up to 6 characters) is on one line. A table that then does not
/// fit is set smaller, turned or cut into groups of columns.
const TYPICAL_CELL_MAX_EM: f32 = 6.0;
/// The width of the text in pt: letter paper with the 2.5 cm margins of
/// [`PREAMBLE`]. Only used to guess whether a long table fits the line.
const TEXT_WIDTH_PT: f32 = 472.0;
/// The width guessed for a CJK character (fullwidth) in em.
const CJK_CHAR_EM: f32 = 1.0;
/// The width guessed for a capital letter, `m` or `w` in em (those of
/// Latin Modern are 0.68 to 0.83 em).
const WIDE_CHAR_EM: f32 = 0.75;
/// The width guessed for `f i j l r t`, punctuation and a space in em
/// (0.28 to 0.39 em in Latin Modern).
const NARROW_CHAR_EM: f32 = 0.33;
/// The width guessed for any other character in em (a figure is 0.5 em).
const OTHER_CHAR_EM: f32 = 0.5;
/// How much wider bold text is (Latin Modern bold against regular, 1.15).
const BOLD_WIDTH_RATIO: f32 = 1.15;
/// How much wider, in em, the text of a small size is than at 10 pt: Latin
/// Modern draws small sizes from wider designs. (smallest font size in pt
/// it applies to, ratio), largest first; measured on the alphabet and the
/// figures with pdfLaTeX (`\tiny`: 0.61 em a letter against 0.49 em).
const SIZE_WIDTH_RATIOS: &[(f32, f32)] = &[(10.0, 1.0), (9.0, 1.03), (8.0, 1.06), (0.0, 1.22)];
/// The narrowest a column of a long table is ever made, in em of its font:
/// the room for one CJK character.
const MIN_COLUMN_EM: f32 = 1.0;
/// The share of the width of the text that the guessed widths of the
/// columns of a long table may fill: the guesses run short by up to a
/// tenth (bold cells, wide letters), and a table wider than the text runs
/// off the page.
const LONG_TABLE_FILL_RATIO: f32 = 0.9;
/// How much more than [`LONG_TABLE_FILL_RATIO`] of the width of the text
/// the columns of a long table may be guessed to take and still be taken
/// to fit (rounding).
const FIT_TOLERANCE_RATIO: f32 = 0.001;
/// The share of the width of the text that the guessed widths of a group
/// of columns may fill (see `column_groups`): the guesses run short in the
/// smallest fonts, whose figures are relatively wider.
const COLUMN_GROUP_FILL_RATIO: f32 = 0.85;
/// The narrowest a wrapped column of a long table is made, in em of its font.
const MIN_WRAPPED_COLUMN_EM: f32 = 4.0;
/// The packages of a long table, loaded when the document has one.
const LONG_TABLE_PACKAGES: &str = "\\usepackage{longtable}\n\\usepackage{array}\n";
/// The package of a table on landscape pages, loaded when the document has
/// one.
const LANDSCAPE_PACKAGE: &str = "\\usepackage{pdflscape}\n";
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
        svg_as_pdf: true,
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
    let cjk = title.chars().chain(body.chars()).any(is_cjk);
    // The symbols of CJK text written as math are characters of the CJK
    // fonts under XeLaTeX and LuaLaTeX: as text, they stay in the text of
    // the PDF (math symbols are lost to its text extraction).
    let (title, body) = if cjk {
        let as_text = |text: &str| {
            CJK_TEXT_SYMBOLS
                .iter()
                .fold(text.to_string(), |text, (symbol, command)| {
                    text.replace(
                        &format!("\\ensuremath{{{command}}}"),
                        &format!("\\kakemonotext{{{symbol}}}{{{command}}}"),
                    )
                })
        };
        (as_text(&title), as_text(body))
    } else {
        (title, body.to_string())
    };
    let body = body.as_str();
    let (vietnamese, unknown): (Vec<char>, Vec<char>) = unknown_chars(&title, body)
        .into_iter()
        .partition(|&ch| vietnamese::t5_letter(ch).is_some());
    let mut out = if vietnamese.is_empty() {
        String::from(PREAMBLE)
    } else {
        // T1 stays the main encoding (the last one loaded).
        PREAMBLE.replacen(T1_FONTENC, T5_FONTENC, 1)
    };
    out = out.replacen("Converted from a PDF", &format!("Converted from {}", input.phrase()), 1);
    if body.contains("\\begin{longtable}") {
        let landscape = if body.contains("\\begin{landscape}") {
            LANDSCAPE_PACKAGE
        } else {
            ""
        };
        out = out.replacen(
            TABLE_PACKAGES_END,
            &format!("{TABLE_PACKAGES_END}{LONG_TABLE_PACKAGES}{landscape}"),
            1,
        );
    }
    if cjk {
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
            | 0x3130..=0x31FF // Hangul compatibility Jamo, kanbun, Bopomofo extended, CJK strokes, Katakana extension
            | 0x3200..=0x33FF // Enclosed CJK, CJK compatibility
            | 0x3400..=0x4DBF // Han, extension A
            | 0x4E00..=0x9FFF // Han
            | 0xA960..=0xA97F // Hangul Jamo extended A
            | 0xAC00..=0xD7FF // Hangul syllables, Jamo extended B
            | 0xF900..=0xFAFF // CJK compatibility ideographs
            | 0xFE10..=0xFE1F // Vertical forms
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
                if table.rows.len().saturating_sub(table.header_rows) > MAX_BOXED_TABLE_BODY_ROWS || too_wide_to_box(table)
                {
                    return self.paragraph(&self.longtable(table, caption.as_deref(), caption_first));
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
    /// when it would be wider than the text. No column is ever cut off:
    /// a table still too wide in the smallest font is set on landscape
    /// pages, and one too wide even there is cut into groups of columns,
    /// each a table of its own with the first column repeated. Its
    /// `caption` (before it when `caption_first`) is kept on its page.
    fn longtable(&self, table: &Table, caption: Option<&str>, caption_first: bool) -> String {
        let n_cols = table.n_cols.max(1);
        let start = LONG_TABLE_SIZES
            .iter()
            .position(|(most, ..)| n_cols <= *most)
            .expect("the last sizes take any count");
        let portrait = LONG_TABLE_SIZES[start..].iter().find_map(|&size| {
            let cols = Columns::fitted(table, size.2, size.3, TEXT_WIDTH_PT);
            cols.fits().then_some((size, cols))
        });
        let (above, below) = match caption {
            Some(caption) if caption_first => (Some(caption), None),
            Some(caption) => (None, Some(caption)),
            None => (None, None),
        };
        let (tables, landscape) = match portrait {
            Some((size, cols)) => (
                self.longtable_part(table, &cols, size, TEXT_HEIGHT_PT, (above, below)),
                false,
            ),
            None => {
                // Turned, in the smallest font: the width of the text is
                // the height of the page.
                let size = *LONG_TABLE_SIZES.last().expect("sizes");
                let cols = Columns::fitted(table, size.2, size.3, TEXT_HEIGHT_PT);
                let tables = if cols.fits() {
                    self.longtable_part(table, &cols, size, TEXT_WIDTH_PT, (above, below))
                } else {
                    let groups = column_groups(&cols);
                    groups
                        .iter()
                        .enumerate()
                        .map(|(i, group)| {
                            let part = table_columns(table, group);
                            let cols = Columns::fitted(&part, size.2, size.3, TEXT_HEIGHT_PT);
                            // The caption above the first group, or below
                            // the last.
                            let captions = (above.filter(|_| i == 0), below.filter(|_| i + 1 == groups.len()));
                            self.longtable_part(&part, &cols, size, TEXT_WIDTH_PT, captions)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                };
                (tables, true)
            }
        };
        if landscape {
            format!("\\begin{{landscape}}\n{tables}\n\\end{{landscape}}")
        } else {
            tables
        }
    }

    /// One `longtable` of `table` in the columns `cols`, at `size` (a row of
    /// [`LONG_TABLE_SIZES`]), on pages whose text is `page_height_pt` high,
    /// with the captions `(above, below)`. A caption is a row of the table,
    /// over all its columns: above, in the head of its first page; below,
    /// in the foot of its last. Outside the table, it could be left alone
    /// at the bottom of a page (longtable breaks the page before the head
    /// when the head and the first row do not fit).
    fn longtable_part(
        &self,
        table: &Table,
        cols: &Columns,
        size: (usize, &str, f32, f32),
        page_height_pt: f32,
        (above, below): (Option<&str>, Option<&str>),
    ) -> String {
        let (_, size, size_pt, tabcolsep_pt) = size;
        let n_cols = table.n_cols.max(1);
        let spec: String = (0..n_cols).map(|col| cols.spec(col)).collect();
        let mut lines = Vec::new();
        if !size.is_empty() {
            lines.push(format!(
                "\\begingroup{size}\\setlength{{\\tabcolsep}}{{{tabcolsep_pt}pt}}"
            ));
        }
        let captioned = above.is_some() || below.is_some();
        if captioned {
            // A captioned table starts at the left margin, as its caption.
            lines.push("\\begingroup\\setlength{\\LTleft}{0pt}\\setlength{\\LTright}{\\fill}".to_string());
        }
        lines.push(format!("\\begin{{longtable}}{{{spec}}}"));
        // A caption is a paragraph as wide as the text, in a box of no
        // width: it does not widen the table.
        let caption_row = |caption: &str| {
            format!(
                "\\multicolumn{{{n_cols}}}{{@{{}}l@{{}}}}{{\\makebox[0pt][l]{{\\parbox[t]{{\\linewidth}}{{{caption}}}}}}} \\\\*"
            )
        };
        if let Some(caption) = above {
            lines.push(caption_row(caption));
        }
        lines.push("\\toprule".to_string());
        let rows = self.rows(table, cols, true);
        let header = table.header_rows.min(table.rows.len());
        let (head, body) = if header > 0 && header < table.rows.len() {
            // The header rows and the rule under them: `rows` puts it
            // after the last header row.
            (&rows[..=header], &rows[header + 1..])
        } else {
            (&rows[..0], &rows[..])
        };
        // LaTeX sets a head apart from the body on its first run: a row of
        // empty cells, not printed (`\kill`), gives each wrapped column its
        // width there, so that a `\multicolumn` of the head does not widen
        // the last column it spans.
        let mut head = head.to_vec();
        let head_spans = table.rows[..header].iter().flatten().any(|cell| cell.colspan > 1);
        if head_spans && !head.is_empty() && cols.wrap.iter().any(|&wrap| wrap) {
            head.insert(0, format!("{}\\kill", " & ".repeat(n_cols - 1)));
        }
        lines.extend(head.iter().cloned());
        // A header too tall to repeat is only on the first page.
        let repeat = !head.is_empty()
            && cols.header_lines(table) as f32 * size_pt * LINE_HEIGHT_RATIO
                <= LONG_TABLE_MAX_HEAD_RATIO * page_height_pt;
        if above.is_some() || (!head.is_empty() && !repeat) {
            lines.push("\\endfirsthead".to_string());
            lines.push("\\toprule".to_string());
            if repeat {
                lines.extend(head.iter().cloned());
            }
        }
        if above.is_some() || !head.is_empty() {
            lines.push("\\endhead".to_string());
        }
        lines.push("\\bottomrule".to_string());
        if let Some(caption) = below {
            lines.push(caption_row(caption));
        }
        lines.push("\\endlastfoot".to_string());
        lines.extend(body.iter().cloned());
        lines.push("\\end{longtable}".to_string());
        if captioned {
            lines.push("\\endgroup".to_string());
        }
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
        // row: the number of columns that cell spans and its column
        // specification, at its first column.
        let mut covered: Vec<Vec<Option<(usize, String)>>> = vec![vec![None; n_cols]; n_rows];
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
                if let Some((width, spec)) = covered[row_idx][col].take() {
                    cells.push(multicolumn(width, &spec, String::new()));
                    col += width;
                } else if let Some(cell) = row_cells.next_if(|cell| cell.col == col) {
                    let colspan = cell.colspan.clamp(1, n_cols - col);
                    let rowspan = cell.rowspan.clamp(1, last_row - row_idx);
                    let spec = cols.cell_spec(col, colspan, &cell.spans);
                    for below in covered.iter_mut().take(row_idx + rowspan).skip(row_idx + 1) {
                        below[col] = Some((colspan, spec.clone()));
                    }
                    cells.push(self.cell(cell, colspan, rowspan, &spec));
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

    /// One cell of a table, in `\multirow` and `\multicolumn` (of the column
    /// specification `spec`, from [`Columns::cell_spec`]) when it spans
    /// several rows or columns.
    fn cell(&self, cell: &TableCell, colspan: usize, rowspan: usize, spec: &str) -> String {
        let text = self.text.spans(&cell.spans);
        let text = if rowspan > 1 {
            // In a `p` column, `=` sets the text at the column's width.
            let width = if spec.contains("p{") { "=" } else { "*" };
            format!("\\multirow{{{rowspan}}}{{{width}}}{{{text}}}")
        } else {
            text
        };
        multicolumn(colspan, spec, protect_start(&text))
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
    /// The least share of the width each column can take: its longest word
    /// when it wraps, its text otherwise; at least 1 em either way.
    least: Vec<f32>,
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
            least: vec![0.0; n_cols],
        }
    }

    /// Whether the columns fit the width of the text they were fitted to.
    fn fits(&self) -> bool {
        self.share.iter().sum::<f32>() <= LONG_TABLE_FILL_RATIO + FIT_TOLERANCE_RATIO
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
    /// of them; each is at least as wide as its typical cell
    /// ([`TYPICAL_CELL_MAX_EM`]) and never under [`MIN_COLUMN_EM`], even
    /// when the table then does not fit. Once any column wraps, every
    /// column is set in its width ([`Columns::paragraph`]), so that the
    /// table is as wide as the shares say. The columns are `tabcolsep_pt`
    /// apart on each side, the text `width_pt` wide.
    fn fitted(table: &Table, size_pt: f32, tabcolsep_pt: f32, width_pt: f32) -> Self {
        let mut cols = Self::plain(table);
        let n_cols = cols.align.len();
        let TextWidths {
            natural,
            body,
            word,
            breaks,
            spans,
        } = TextWidths::of(table, n_cols);
        let em_pt = em_width_pt(size_pt);
        cols.em_share = em_pt / width_pt;
        cols.pad_share = tabcolsep_pt / width_pt;
        let share_of = |em: f32| (em * em_pt + 2.0 * tabcolsep_pt) / width_pt;
        cols.share = natural.iter().map(|&em| share_of(em)).collect();
        let mean = |col: usize| {
            let (sum, count) = body[col];
            if count == 0 { 0.0 } else { sum / count as f32 }
        };
        // The width of a wrapped column's typical cell, in em.
        let typical = |col: usize| mean(col).max(word[col]).clamp(MIN_COLUMN_EM, TYPICAL_CELL_MAX_EM);
        cols.least = (0..n_cols)
            .map(|col| {
                if breaks[col] {
                    share_of(typical(col))
                } else {
                    // An empty column, or one of narrow characters, still
                    // keeps an em inside its `\tabcolsep`: a `p` column of
                    // no width (or less) runs its text into the next.
                    share_of(natural[col].max(MIN_COLUMN_EM))
                }
            })
            .collect();
        if cols.share.iter().sum::<f32>() <= LONG_TABLE_FILL_RATIO {
            return cols;
        }
        // The columns under a cell over several of them are together as
        // wide as its typical text.
        let floors = span_floors(&spans, &cols.least, |em| share_of(em.min(TYPICAL_CELL_MAX_EM)));
        for (col, floor) in floors.into_iter().enumerate() {
            cols.least[col] = cols.least[col].max(floor);
        }
        let even = 1.0 / n_cols as f32;
        cols.wrap = (0..n_cols).map(|col| cols.share[col] > even && breaks[col]).collect();
        for col in (0..n_cols).filter(|&col| !cols.wrap[col]) {
            cols.share[col] = cols.share[col].max(cols.least[col]);
        }
        let wrapped: Vec<usize> = (0..n_cols).filter(|&col| cols.wrap[col]).collect();
        let kept: f32 = (0..n_cols)
            .filter(|&col| !cols.wrap[col])
            .map(|col| cols.share[col])
            .sum();
        let left = LONG_TABLE_FILL_RATIO - kept;
        let weight = |col: usize| mean(col).max(MIN_WRAPPED_COLUMN_EM);
        if left <= 0.0 {
            // The others leave nothing: the table is wider than the text.
            for &col in &wrapped {
                cols.share[col] = cols.least[col];
            }
            return cols;
        }
        // Each wrapped column gets at least its floor, the width of its
        // typical cell (when the floors do not fit, the table does not
        // either); the others share the rest in proportion to their weights.
        let floor = |col: usize| cols.least[col];
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

    /// Whether the `width` columns from `col` are set as one `p` column:
    /// when one of them wraps, or when they are a `\multicolumn` of a long
    /// table, whose text could be wider than they are (its widths are not
    /// counted in theirs).
    fn paragraph(&self, col: usize, width: usize) -> bool {
        let fitted = self.share.iter().any(|&share| share > 0.0);
        let wrapping = self.wrap.iter().any(|&wrap| wrap);
        self.wraps(col, width) || (fitted && (width > 1 || wrapping))
    }

    /// The column specification of a cell of the text `spans` over the
    /// `width` columns from `col`: [`Columns::span_spec`], but the
    /// alignment of the first column for a cell over several columns of a
    /// table none of whose columns wraps, when its text fits on one line in
    /// them. A `p` column there is as wide as its columns are guessed to be,
    /// wider than they are when the guess runs long: LaTeX sets a long
    /// table's head apart from its body on its first run, and a `p` cell of
    /// the head over columns as narrow as their header then widens the last
    /// of them, and the table, by the difference.
    fn cell_spec(&self, col: usize, width: usize, spans: &[Span]) -> String {
        let wrapping = self.wrap.iter().any(|&wrap| wrap);
        if width > 1 && !wrapping && self.paragraph(col, width) {
            let text: String = spans.iter().map(|span| span.text.as_str()).collect();
            let share: f32 = self.share[col..col + width].iter().sum();
            if text_em(&text) * BOLD_WIDTH_RATIO * self.em_share + 2.0 * self.pad_share <= share {
                return self.align[col].to_string();
            }
        }
        self.span_spec(col, width)
    }

    /// The column specification of the `width` columns from `col` taken
    /// together (a `\multicolumn`): the alignment of the first, or a `p`
    /// column as wide as all of them ([`Columns::paragraph`]).
    fn span_spec(&self, col: usize, width: usize) -> String {
        let align = self.align[col];
        if !self.paragraph(col, width) {
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

/// The widths of the text of each column of a table, guessed in em.
struct TextWidths {
    /// The width of a column's longest cell on one line (a header cell is
    /// bold).
    natural: Vec<f32>,
    /// The total width and the count of a column's body cells with text.
    body: Vec<(f32, usize)>,
    /// The longest word of a column's cells.
    word: Vec<f32>,
    /// One of a column's cells has more than one word (a column of numbers
    /// or codes under one-word headers has none: it could not wrap).
    breaks: Vec<bool>,
    /// The cells over several columns: (first column, columns, width of
    /// the text on one line).
    spans: Vec<(usize, usize, f32)>,
}

impl TextWidths {
    /// The widths of the `n_cols` columns of `table`; a cell over several
    /// columns counts in none.
    fn of(table: &Table, n_cols: usize) -> Self {
        let mut widths = Self {
            natural: vec![0.0; n_cols],
            body: vec![(0.0, 0); n_cols],
            word: vec![0.0; n_cols],
            breaks: vec![false; n_cols],
            spans: Vec::new(),
        };
        for (row, cells) in table.rows.iter().enumerate() {
            for cell in cells.iter().filter(|cell| cell.colspan > 1 && cell.col < n_cols) {
                let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
                let bold = row < table.header_rows || cell.spans.iter().any(|span| span.style & BOLD != 0);
                let ratio = if bold { BOLD_WIDTH_RATIO } else { 1.0 };
                let width = cell.colspan.min(n_cols - cell.col);
                widths.spans.push((cell.col, width, text_em(&text) * ratio));
            }
            for cell in cells.iter().filter(|cell| cell.colspan <= 1 && cell.col < n_cols) {
                let text: String = cell.spans.iter().map(|span| span.text.as_str()).collect();
                let col = cell.col;
                // A header cell is bold, and so may be a body cell.
                let bold = row < table.header_rows || cell.spans.iter().any(|span| span.style & BOLD != 0);
                let ratio = if bold { BOLD_WIDTH_RATIO } else { 1.0 };
                let (em, word) = (text_em(&text) * ratio, word_em(&text) * ratio);
                widths.natural[col] = widths.natural[col].max(em);
                widths.word[col] = widths.word[col].max(word);
                widths.breaks[col] |= word < em;
                if row >= table.header_rows && em > 0.0 {
                    widths.body[col].0 += em;
                    widths.body[col].1 += 1;
                }
            }
        }
        widths
    }
}

/// The width in pt of the em that the widths of text are guessed in, at
/// the font size `size_pt` ([`SIZE_WIDTH_RATIOS`]).
fn em_width_pt(size_pt: f32) -> f32 {
    size_pt
        * SIZE_WIDTH_RATIOS
            .iter()
            .find(|(least, _)| size_pt >= *least)
            .map_or(1.0, |&(_, ratio)| ratio)
}

/// The least share of each column that the cells over several columns
/// `spans` (first column, columns, width in em) ask for: when the columns
/// under a cell are together narrower (by `least`) than `share_of` its
/// width, each gets an even part of what is missing.
fn span_floors(spans: &[(usize, usize, f32)], least: &[f32], share_of: impl Fn(f32) -> f32) -> Vec<f32> {
    let mut floors = vec![0.0f32; least.len()];
    for &(col, width, em) in spans {
        let have: f32 = least[col..col + width].iter().sum();
        let missing = share_of(em) - have;
        if missing > 0.0 {
            for column in col..col + width {
                floors[column] = floors[column].max(least[column] + missing / width as f32);
            }
        }
    }
    floors
}

/// The columns of a table too wide for the text, in groups that each fit
/// with its first column (repeated in each), by the least width of each
/// column ([`Columns::least`]) and [`COLUMN_GROUP_FILL_RATIO`]; a column
/// too wide even alone with the first is a group of its own.
fn column_groups(cols: &Columns) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut width = 0.0;
    for col in 1..cols.least.len() {
        match groups.last_mut() {
            Some(group) if width + cols.least[col] <= COLUMN_GROUP_FILL_RATIO => group.push(col),
            _ => {
                groups.push(vec![0, col]);
                width = cols.least[0];
            }
        }
        width += cols.least[col];
    }
    if groups.is_empty() {
        groups.push(vec![0]);
    }
    groups
}

/// The table of the columns `keep` (in order) of `table`: a cell is at its
/// first kept column and spans the kept columns it covered; a cell that
/// covers none is left out.
fn table_columns(table: &Table, keep: &[usize]) -> Table {
    let rows = table
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .filter_map(|cell| {
                    let covered: Vec<usize> = (0..keep.len())
                        .filter(|&new| (cell.col..cell.col + cell.colspan.max(1)).contains(&keep[new]))
                        .collect();
                    let first = *covered.first()?;
                    Some(TableCell {
                        col: first,
                        colspan: covered.len(),
                        rowspan: cell.rowspan,
                        spans: cell.spans.clone(),
                    })
                })
                .collect()
        })
        .collect();
    Table {
        n_cols: keep.len(),
        header_rows: table.header_rows,
        col_align: keep
            .iter()
            .map(|&col| table.col_align.get(col).copied().unwrap_or("left"))
            .collect(),
        rows,
        rule_above: Vec::new(),
        rule_bottom: Vec::new(),
        rule_left: Vec::new(),
        rule_right: Vec::new(),
        geom: table.geom.clone(),
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
        .map(|ch| match ch {
            _ if is_cjk(ch) => CJK_CHAR_EM,
            'A'..='Z' | 'm' | 'w' => WIDE_CHAR_EM,
            'f' | 'i' | 'j' | 'l' | 'r' | 't' | ' ' | '.' | ',' | ':' | ';' | '\'' | '(' | ')' => NARROW_CHAR_EM,
            _ => OTHER_CHAR_EM,
        })
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
    fn xelatex_sets_the_symbols_of_cjk_text_in_the_cjk_font() {
        let tex = document(&doc_of("①②の手順 ≦ ― ●"), "①②の手順 ≦ ― ●", InputFormat::Xlsx);
        let declared = tex
            .lines()
            .skip_while(|line| !line.contains("\\xeCJKDeclareCharClass{CJK}"))
            .take_while(|line| !line.contains('}') || line.contains("DeclareCharClass"))
            .collect::<String>();
        for code in ["\"2015", "\"2264->\"2267", "\"2460->", "\"25CE->\"25D3"] {
            assert!(declared.contains(code), "{code} not declared: {declared}");
        }
        // Kanbun and vertical forms are CJK text.
        assert!(is_cjk('\u{3190}') && is_cjk('\u{319F}') && is_cjk('\u{FE10}') && is_cjk('\u{FE19}'));
    }

    #[test]
    fn symbols_of_cjk_text_are_characters_under_lualatex_and_xelatex() {
        let body = escape("△11 523 □ 〈注〉");
        let tex = document(&doc_of("人口 △11 523"), &body, InputFormat::Xlsx);
        for (symbol, command) in [
            ("△", "\\triangle"),
            ("□", "\\square"),
            ("〈", "\\langle"),
            ("〉", "\\rangle"),
        ] {
            assert!(
                tex.contains(&format!("\\kakemonotext{{{symbol}}}{{{command}}}")),
                "{symbol}: {tex}"
            );
        }
        // Without CJK text, the math symbol stays.
        let latin = document(&doc_of("x"), &escape("△ x"), InputFormat::Pdf);
        assert!(latin.contains("\\ensuremath{\\triangle}") && !latin.contains("kakemonotext"));
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
        figure_latex(&block)
    }

    /// The LaTeX of the figure or table `block`.
    fn figure_latex(block: &Block) -> String {
        let anchors = std::collections::HashSet::new();
        let html = HtmlOptions::default();
        let mut images = ImageStore {
            opts: &html,
            jobs: Vec::new(),
            svg_as_pdf: true,
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
        writer.figure(block, FloatKind::Table, None);
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
        assert!(latex.contains("\\endhead\n\\bottomrule\n\\endlastfoot\n"), "{latex}");
        assert!(
            latex.contains("r31c0 & r31c1 & r31c2 \\\\\n\\end{longtable}"),
            "{latex}"
        );
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
        // Every column in its width, the long one the widest.
        let shares: Vec<f32> = spec_line
            .split("p{\\dimexpr")
            .skip(1)
            .map(|part| part.split("\\linewidth").next().unwrap().parse().unwrap())
            .collect();
        assert_eq!(shares.len(), 4, "{spec_line}");
        assert!(shares[2] > shares[0] + shares[1] + shares[3], "{spec_line}");
    }

    #[test]
    fn a_long_table_with_a_header_taller_than_a_quarter_page_shows_it_once() {
        let latex = table_latex(table_of(16, 40, |row, col| {
            if row == 0 {
                "a long header of many words ".repeat(30)
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

    /// The value of the cell at `row`, `col` of a wide table of numbers.
    fn number(row: usize, col: usize) -> String {
        format!("{}.{col:02}", (row * 1000 + col * 37) % 99_999)
    }

    #[test]
    fn a_long_table_of_unbreakable_columns_too_wide_is_turned() {
        // 20 columns of numbers: wider than the text even in \tiny.
        let latex = table_latex(table_of(20, 31, number));
        assert!(
            latex.contains("\\begin{landscape}") && latex.contains("\\end{landscape}"),
            "{latex}"
        );
        assert!(latex.contains("\\tiny"), "{latex}");
        assert!(latex.contains(&number(31, 19)));
    }

    #[test]
    fn a_long_table_too_wide_even_turned_is_cut_into_groups_of_columns() {
        let latex = table_latex(table_of(60, 31, number));
        let tables = latex.matches("\\begin{longtable}").count();
        assert!(tables >= 2, "{tables} tables");
        // Every column is in one of them, the first one repeated in each.
        for col in 0..60 {
            assert!(latex.contains(&number(31, col)), "column {col} lost");
        }
        assert_eq!(latex.matches(&format!("{} &", number(31, 0))).count(), tables);
    }

    #[test]
    fn wrapped_columns_are_at_least_one_em_wide() {
        let table = table_of(16, 40, |row, col| format!("{row} {col} {}", "語".repeat(10 + col * 20)));
        for &(_, _, size_pt, tabcolsep_pt) in LONG_TABLE_SIZES {
            let cols = Columns::fitted(&table, size_pt, tabcolsep_pt, TEXT_WIDTH_PT);
            for col in (0..16).filter(|&col| cols.wrap[col]) {
                let content_pt = cols.share[col] * TEXT_WIDTH_PT - 2.0 * tabcolsep_pt;
                assert!(
                    content_pt >= size_pt * 0.999,
                    "column {col}: {content_pt} pt at {size_pt} pt"
                );
            }
        }
    }

    #[test]
    fn every_column_of_a_long_table_keeps_one_em_inside_its_padding() {
        // An empty column and a column of single characters next to wrapped
        // ones: a share of only the two `\tabcolsep` leaves no room, or a
        // negative one, for the text.
        let table = table_of(16, 40, |row, col| match col {
            0 => String::new(),
            1 => "x".to_string(),
            _ => format!("{row} {col} {}", "語".repeat(10 + col * 20)),
        });
        for &(_, _, size_pt, tabcolsep_pt) in LONG_TABLE_SIZES {
            for width_pt in [TEXT_WIDTH_PT, TEXT_HEIGHT_PT] {
                let cols = Columns::fitted(&table, size_pt, tabcolsep_pt, width_pt);
                if !cols.wrap.iter().any(|&wrap| wrap) {
                    continue;
                }
                for col in 0..16 {
                    let content_pt = cols.share[col] * width_pt - 2.0 * tabcolsep_pt;
                    assert!(
                        content_pt >= size_pt * 0.999,
                        "column {col}: {content_pt} pt at {size_pt} pt in {width_pt} pt"
                    );
                }
            }
        }
    }

    /// The text of each page of the PDF that pdfLaTeX makes of `tex` in a
    /// scratch folder named after `name`; `None` (said on the standard
    /// error) when pdfLaTeX or pdftotext is not installed.
    fn compiled_pages(name: &str, tex: &str) -> Option<Vec<String>> {
        let dir = std::env::temp_dir().join(format!("kakemono-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.tex"), tex).unwrap();
        let Ok(out) = std::process::Command::new("pdflatex")
            .args(["-interaction=nonstopmode", "-halt-on-error", "t.tex"])
            .current_dir(&dir)
            .output()
        else {
            eprintln!("SKIPPED {name}: pdflatex is not installed, the LaTeX was not compiled");
            return None;
        };
        assert!(
            out.status.success() && dir.join("t.pdf").is_file(),
            "pdflatex failed:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let Ok(text) = std::process::Command::new("pdftotext")
            .args(["-layout", "t.pdf", "-"])
            .current_dir(&dir)
            .output()
        else {
            eprintln!("SKIPPED {name}: pdftotext is not installed, the PDF text was not checked");
            return None;
        };
        let _ = std::fs::remove_dir_all(&dir);
        Some(
            String::from_utf8_lossy(&text.stdout)
                .split('\u{c}')
                .map(str::to_string)
                .collect(),
        )
    }

    /// The widths of the alignments pdfLaTeX reports as too wide (`Overfull
    /// \hbox (..pt too wide) in alignment`) on its first run over `tex`, in
    /// a scratch folder named after `name`; `None` (said on the standard
    /// error) when pdfLaTeX is not installed.
    fn first_run_overfull_alignments(name: &str, tex: &str) -> Option<Vec<f32>> {
        let dir = std::env::temp_dir().join(format!("kakemono-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t.tex"), tex).unwrap();
        let Ok(out) = std::process::Command::new("pdflatex")
            .args(["-interaction=nonstopmode", "-halt-on-error", "t.tex"])
            .current_dir(&dir)
            .output()
        else {
            eprintln!("SKIPPED {name}: pdflatex is not installed, the LaTeX was not compiled");
            return None;
        };
        assert!(
            out.status.success(),
            "pdflatex failed:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let log = std::fs::read(dir.join("t.log")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let overfull = String::from_utf8_lossy(&log)
            .lines()
            .filter(|line| line.starts_with("Overfull \\hbox (") && line.contains("in alignment"))
            .filter_map(|line| line["Overfull \\hbox (".len()..].split("pt").next()?.parse().ok())
            .collect();
        Some(overfull)
    }

    /// A long table's head is set apart from its body on LaTeX's first run:
    /// a `\multicolumn` in it over columns of no width there widens the last
    /// of them, and the table runs off the page until LaTeX is run again.
    /// One of wrapped columns (a form, labels over several of them) and one
    /// of numbers under a header over twelve of them.
    #[test]
    fn a_long_table_with_spans_in_its_head_fits_on_the_first_run() {
        let span = |col: usize, colspan: usize, rowspan: usize, text: &str| TableCell {
            col,
            colspan,
            rowspan,
            spans: vec![Span::plain(text)],
        };
        let mut form = table_of(20, 40, |row, col| match col {
            1 => format!("{row} {}", "a note on what to do ".repeat(4)),
            5..=12 if row % 3 == 0 => "y".to_string(),
            14 => "Labour office".to_string(),
            16 => format!("{row} the witness, with address and phone number"),
            19 => "TEL".to_string(),
            _ => String::new(),
        });
        form.rows[0] = vec![
            span(2, 4, 1, "Work name"),
            span(6, 1, 1, ":"),
            span(7, 13, 1, "Outer walls"),
        ];
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let mut prices = table_of(14, 40, |row, col| match (row, col) {
            (_, 0) => format!("{} ...", 1990 + row),
            (..=20, _) => "-".to_string(),
            _ => format!("1{}.{}0", 80 + (row * 7 + col) % 40, col % 10),
        });
        prices.header_rows = 2;
        prices.rows[0] = vec![
            span(0, 1, 2, "reporting year"),
            span(1, 12, 1, "reporting month"),
            span(13, 1, 2, "yearly average"),
        ];
        prices.rows[1] = months
            .iter()
            .enumerate()
            .map(|(i, month)| span(i + 1, 1, 1, month))
            .collect();
        prices.rows[2] = vec![span(1, 13, 1, "Wood products for energy production 1 2015 = 100")];
        let body = format!("{}\n\n{}", table_latex(form), table_latex(prices));
        let tex = document(&doc_of("x"), &body, InputFormat::Pdf);
        let Some(overfull) = first_run_overfull_alignments("long-table-heads", &tex) else {
            return;
        };
        assert!(overfull.iter().all(|&pt| pt <= 5.0), "too wide by {overfull:?} pt");
    }

    /// The caption of a long table is on the page of its first row, wherever
    /// the table starts on the page.
    #[test]
    fn the_caption_of_a_long_table_stays_with_its_first_row() {
        let mut block = Block::new(
            0,
            BlockKind::Figure {
                kind: FloatKind::Table,
                image: None,
                label: None,
            },
        );
        block.spans = vec![Span::plain("Table 1: CAPTIONMARK")];
        block.caption_above = true;
        block.table = Some(table_of(2, 31, |row, col| {
            if row == 1 && col == 0 {
                "FIRSTROW".to_string()
            } else {
                format!("r{row}c{col}")
            }
        }));
        let table = figure_latex(&block);
        // The table starts `room` pt above the bottom of the first page.
        for room in (10..=90).step_by(8) {
            let body = format!("\\noindent\\rule{{1pt}}{{\\dimexpr\\textheight-{room}pt\\relax}}\n\n{table}");
            let tex = document(&doc_of("x"), &body, InputFormat::Pdf);
            let Some(pages) = compiled_pages(&format!("caption-{room}"), &tex) else {
                return;
            };
            let page_of = |mark: &str| pages.iter().position(|page| page.contains(mark));
            assert_eq!(
                page_of("CAPTIONMARK"),
                page_of("FIRSTROW"),
                "{room} pt left: the caption is not on the page of the first row"
            );
        }
    }

    /// Long tables compile with pdfLaTeX (when it is installed) and show
    /// their last row and last column: one of wrapped columns, with a
    /// `\multicolumn` over two of them and a `\multirow{..}{=}` in one, and
    /// one of 20 columns of numbers, which needs landscape pages.
    #[test]
    fn long_tables_compile_and_keep_every_column() {
        let mut wrapped = table_of(3, 40, |row, col| match col {
            0 => format!("{row}"),
            1 => format!("row {row} {}", "has a long description ".repeat(6)),
            _ => format!("end{row} {}", "and some more words ".repeat(5)),
        });
        wrapped.header_rows = 2;
        wrapped.rows[0] = vec![
            TableCell {
                col: 0,
                colspan: 1,
                rowspan: 2,
                spans: vec![Span::plain("Key")],
            },
            TableCell {
                col: 1,
                colspan: 2,
                rowspan: 1,
                spans: vec![Span::plain("Both texts")],
            },
        ];
        wrapped.rows[1].remove(0);
        // A cell over three rows of the body, in a wrapped column.
        wrapped.rows[5][1].rowspan = 3;
        wrapped.rows[6].remove(1);
        wrapped.rows[7].remove(1);
        let first = table_latex(wrapped);
        assert!(
            first.contains("\\multicolumn{2}{>{\\raggedright\\arraybackslash}p{"),
            "{first}"
        );
        assert!(first.contains("\\multirow{3}{=}"), "{first}");
        let second = table_latex(table_of(20, 31, number));
        let tex = document(&doc_of("x"), &format!("{first}\n\n{second}"), InputFormat::Pdf);
        let Some(pages) = compiled_pages("long-tables", &tex) else {
            return;
        };
        let text = pages.concat();
        for value in ["end40", "Both texts", &number(31, 19), &number(31, 0)] {
            assert!(text.contains(value), "{value} is not in the PDF");
        }
    }

    /// A form laid out over 20 columns, as Japanese workbooks are: long
    /// notes in column 1, short labels in columns 14 to 16, and columns
    /// of one character or none between them.
    fn form() -> Table {
        table_of(20, 40, |row, col| match col {
            1 => format!("{row} {}", "注意事項の説明文".repeat(4)),
            2 => "会社名".to_string(),
            5..=12 => {
                if row % 3 == 0 {
                    "年".to_string()
                } else {
                    String::new()
                }
            }
            14 => "〇〇労働基準監督署".to_string(),
            15 => "〇〇水道局".to_string(),
            16 => format!("{row} 現認者（参考人）の確認（住所、氏名、電話番号）"),
            19 => "TEL".to_string(),
            _ => String::new(),
        })
    }

    #[test]
    fn a_long_table_with_wrapped_columns_sets_every_column_in_its_width() {
        let latex = table_latex(form());
        let spec = latex
            .lines()
            .find(|line| line.starts_with("\\begin{longtable}"))
            .unwrap();
        let columns = spec.matches("p{\\dimexpr").count();
        assert_eq!(columns, 20, "{spec}");
    }

    #[test]
    fn short_labels_of_a_wrapped_column_stay_on_one_line() {
        let mut table = form();
        // A label over columns 17 and 18, empty elsewhere.
        table.rows[3].retain(|cell| cell.col != 17 && cell.col != 18);
        table.rows[3].push(TableCell {
            col: 17,
            colspan: 2,
            rowspan: 1,
            spans: vec![Span::plain("〇〇監理役")],
        });
        for &(_, _, size_pt, tabcolsep_pt) in LONG_TABLE_SIZES {
            for width_pt in [TEXT_WIDTH_PT, TEXT_HEIGHT_PT] {
                let cols = Columns::fitted(&table, size_pt, tabcolsep_pt, width_pt);
                if cols.fits() {
                    let room_em = (cols.share[15] * width_pt - 2.0 * tabcolsep_pt) / size_pt;
                    assert!(room_em >= 5.0 - 1e-3, "〇〇水道局 in {room_em} em at {size_pt} pt");
                    let room_em = ((cols.share[17] + cols.share[18]) * width_pt - 2.0 * tabcolsep_pt) / size_pt;
                    assert!(room_em >= 5.0 - 1e-3, "〇〇監理役 in {room_em} em at {size_pt} pt");
                }
            }
        }
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
            svg_as_pdf: true,
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
