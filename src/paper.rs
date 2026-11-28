//! High-level entry points: PDF, Word, Excel, EPUB or HTML bytes (or a
//! file, or an address of the Web) in, structured document and HTML (or
//! Word, Markdown, LaTeX, Typst or EPUB) out.

use crate::analysis::pipeline::{Analysis, analyze};
use crate::backend::{PageRenderer, PdfFile, PngCompression};
use crate::config::{AnalyzeOptions, ExcelConfig, WorkbookPdfConfig, WorkbookPdfStyle};
use crate::excel;
use crate::model::{Document, Meta};
use crate::render::{
    Chunk, ChunkOptions, ContentReport, DocxOptions, DocxOutput, EpubOptions, HtmlOptions, HtmlOutput, JsonOptions,
    JsonOutput, LatexOptions, LatexOutput, MarkdownOptions, MarkdownOutput, Mode, SourceInfo, TypstOptions,
    TypstOutput, apply_content, render, render_chunks, render_docx, render_docx_with_images, render_epub, render_json,
    render_latex, render_markdown, render_to, render_typst,
};
use crate::word::{self, WordFormat};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::Path;

/// The format of the file a [`Paper`] was loaded from. More formats may
/// be added: a `match` on it needs a `_` arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputFormat {
    /// A PDF.
    Pdf,
    /// A Word document in Office Open XML (`.docx`).
    Docx,
    /// A Word 97–2003 document (`.doc`).
    Doc,
    /// An Excel workbook in Office Open XML (`.xlsx`).
    Xlsx,
    /// An EPUB book (`.epub`).
    Epub,
    /// A web page (`.html`, or an address of the Web).
    Html,
}

impl InputFormat {
    /// The usual file extension (`pdf`, `docx`, `doc`, `xlsx`, `epub`, `html`), also the name of the
    /// format in the JSON output.
    pub fn name(self) -> &'static str {
        match self {
            InputFormat::Pdf => "pdf",
            InputFormat::Docx => "docx",
            InputFormat::Doc => "doc",
            InputFormat::Xlsx => "xlsx",
            InputFormat::Epub => "epub",
            InputFormat::Html => "html",
        }
    }

    /// Whether a document of this format has pages to show: only a PDF.
    /// The `hard`, `hard-95` and `medium` modes need them.
    pub fn has_pages(self) -> bool {
        self == InputFormat::Pdf
    }
}

/// What a [`Paper`] holds.
enum Source {
    /// A PDF, analysed on demand.
    Pdf(PdfFile),
    /// A Word document, read into its structure when loaded, with its file
    /// (read again with its formatting for the layout).
    Word(Box<Document>, InputFormat, Vec<u8>),
    /// An Excel workbook, read into its structure when loaded (its pages
    /// are laid out from that structure), with the paper its print
    /// settings name (width and height in points), its file (read again
    /// with the look of every cell for the sheet view) and the thresholds
    /// it was read with.
    Sheet(Box<Document>, Option<(f32, f32)>, Vec<u8>, ExcelConfig),
    /// An EPUB book or a web page (the format), read into its structure
    /// when loaded.
    Book(Box<Document>, InputFormat),
}

/// A loaded PDF, Word document, Excel workbook, EPUB book or web page,
/// ready to be analysed and rendered.
///
/// Typical use: [`Paper::from_bytes`], [`Paper::analyze`], then
/// [`Paper::to_html`] once per output mode (the analysis does not depend on
/// the mode, so it is computed once).
pub struct Paper {
    /// The document itself.
    file: Source,
    /// Where the document came from (file name, arXiv id, ...), recorded in
    /// the output.
    source: Option<String>,
    /// SHA-256 of the bytes of the file, in hexadecimal: the JSON output
    /// names its source with it, and an edited JSON file is only applied to
    /// the same file.
    sha256: String,
    /// A Word document laid out in pages, read back as a PDF and analysed,
    /// for the modes that show pages (made when first needed; `None` when
    /// the layout failed).
    laid: std::sync::OnceLock<Option<Laid>>,
}

/// A Word document laid out in pages as Word lays it out.
struct Laid {
    /// The PDF of the pages.
    pdf: PdfFile,
    /// Its analysis.
    analysis: Analysis,
}

impl Paper {
    /// Loads a PDF, a Word document (`.docx`, `.doc`), an Excel workbook
    /// (`.xlsx`), an EPUB book (`.epub`) or a web page (HTML) from its
    /// bytes; the format is recognised from the bytes. The pictures of a
    /// web page loaded so are only those inside it (`data:` URLs):
    /// [`Paper::from_path`] reads them next to its file, [`Paper::from_url`]
    /// fetches them.
    ///
    /// # Errors
    /// When the data is not a readable PDF, Word document, workbook, EPUB
    /// book or web page, or the PDF has no pages.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Self::from_bytes_with(data, &ExcelConfig::default())
    }

    /// Loads a file like [`Paper::from_bytes`], reading a workbook with
    /// the thresholds `excel` (they do not apply to the other formats).
    ///
    /// # Errors
    /// As [`Paper::from_bytes`].
    pub fn from_bytes_with(data: Vec<u8>, excel: &ExcelConfig) -> Result<Self> {
        if crate::html::is_html(&data) {
            return Self::from_html(&data, crate::html::Origin::Bytes);
        }
        let sha256 = sha256_hex(&data);
        if crate::epub::is_epub(&data) {
            let doc = crate::epub::read(&data, None)?;
            return Ok(Self::loaded(Source::Book(Box::new(doc), InputFormat::Epub), sha256));
        }
        // A workbook, a Word document and an EPUB book are ZIP packages:
        // each looks for its own main part.
        if excel::detect(&data) {
            let (doc, paper) = excel::read_with_paper(&data, excel, None)?;
            return Ok(Self::loaded(Source::Sheet(Box::new(doc), paper, data, *excel), sha256));
        }
        let file = match word::detect(&data) {
            Some(format) => {
                let doc = word::read(&data, format, None)?;
                let format = match format {
                    WordFormat::Docx => InputFormat::Docx,
                    WordFormat::Doc => InputFormat::Doc,
                };
                Source::Word(Box::new(doc), format, data)
            }
            None => Source::Pdf(PdfFile::open(data)?),
        };
        Ok(Self::loaded(file, sha256))
    }

    /// A paper of `file`, whose bytes have the SHA-256 `sha256`.
    fn loaded(file: Source, sha256: String) -> Self {
        Self {
            file,
            source: None,
            sha256,
            laid: std::sync::OnceLock::new(),
        }
    }

    /// Loads the file at `path`, as [`Paper::from_bytes`] does; the
    /// pictures and style sheets of a web page are read next to it.
    ///
    /// # Errors
    /// When the file cannot be read, or [`Paper::from_bytes`] fails.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_path_with(path, &ExcelConfig::default())
    }

    /// Loads the file at `path` like [`Paper::from_path`], reading a
    /// workbook with the thresholds `excel`.
    ///
    /// # Errors
    /// As [`Paper::from_path`].
    pub fn from_path_with(path: impl AsRef<Path>, excel: &ExcelConfig) -> Result<Self> {
        let path = path.as_ref();
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        if crate::html::is_html(&data) {
            return Self::from_html(&data, crate::html::Origin::File(path));
        }
        Self::from_bytes_with(data, excel)
    }

    /// Fetches the document at the `http(s)` address `url` and loads it, as
    /// [`Paper::from_bytes`] does: a web page with its pictures (fetched
    /// too), or a PDF, a Word document or an EPUB book. Its source
    /// ([`Meta::source`]) is the address.
    ///
    /// # Errors
    /// When the document cannot be fetched (see
    /// [`source::web::fetch`](crate::source::web::fetch)) or read.
    pub fn from_url(url: &str) -> Result<Self> {
        let fetched = crate::source::web::fetch(url)?;
        let paper = if fetched.is_html() || crate::html::is_html(&fetched.data) {
            let origin = crate::html::Origin::Url {
                url: &fetched.url,
                charset: fetched.charset(),
            };
            Self::from_html(&fetched.data, origin)?
        } else {
            Self::from_bytes(fetched.data)?
        };
        Ok(paper.with_source(url.trim()))
    }

    /// Loads the web page `data` saved from the `http(s)` address `url`: its
    /// links and pictures are read as at that address, and its pictures are
    /// fetched. Its source ([`Meta::source`]) is the address.
    ///
    /// # Errors
    /// When `url` is not a URL.
    pub fn from_page_at(data: &[u8], url: &str) -> Result<Self> {
        let origin = crate::html::Origin::Url {
            url: url.trim(),
            charset: None,
        };
        Ok(Self::from_html(data, origin)?.with_source(url.trim()))
    }

    /// Loads the web page `data` from `origin`.
    fn from_html(data: &[u8], origin: crate::html::Origin<'_>) -> Result<Self> {
        Ok(Self {
            file: Source::Book(Box::new(crate::html::read(data, origin, None)?), InputFormat::Html),
            source: None,
            sha256: sha256_hex(data),
            laid: std::sync::OnceLock::new(),
        })
    }

    /// The format of the file.
    pub fn format(&self) -> InputFormat {
        match &self.file {
            Source::Pdf(_) => InputFormat::Pdf,
            Source::Word(_, format, _) => *format,
            Source::Sheet(..) => InputFormat::Xlsx,
            Source::Book(_, format) => *format,
        }
    }

    /// The PDF, when the paper is one.
    fn pdf(&self) -> Option<&PdfFile> {
        match &self.file {
            Source::Pdf(pdf) => Some(pdf),
            Source::Word(..) | Source::Sheet(..) | Source::Book(..) => None,
        }
    }

    /// The format a Word document is read with.
    fn word_format(&self) -> Option<WordFormat> {
        match &self.file {
            Source::Pdf(_) | Source::Sheet(..) | Source::Book(..) => None,
            Source::Word(_, InputFormat::Doc, _) => Some(WordFormat::Doc),
            Source::Word(..) => Some(WordFormat::Docx),
        }
    }

    /// The Word document laid out in pages and analysed, made once.
    fn laid(&self) -> Option<&Laid> {
        self.word_format()?;
        self.laid
            .get_or_init(|| {
                let pdf = PdfFile::open(self.to_pdf().ok()?).ok()?;
                let (raw, fonts) = pdf.extract(1);
                let meta = Meta {
                    source: self.source.clone(),
                    ..Default::default()
                };
                let analysis = analyze(raw, &fonts, meta, AnalyzeOptions::default());
                Some(Laid { pdf, analysis })
            })
            .as_ref()
    }

    /// The HTML of a Word document in a mode that shows its layout: its
    /// pages (`hard`, `hard-95`), or Word's formatting in one continuous
    /// page (`medium`); of a workbook in a mode other than `friendly`, its
    /// sheets as Excel shows them. `None` for a PDF, the `friendly` mode,
    /// or when the layout failed.
    fn word_layout_html(&self, opts: &HtmlOptions) -> Option<HtmlOutput> {
        if let Source::Sheet(_, _, data, config) = &self.file {
            if opts.mode == Mode::Friendly {
                return None;
            }
            return Some(HtmlOutput {
                html: excel::sheet_view(data, config, self.source.as_deref()).ok()?,
                files: Vec::new(),
            });
        }
        let Source::Word(_, _, data) = &self.file else {
            return None;
        };
        match opts.mode {
            Mode::Friendly => None,
            Mode::Medium => {
                // Without a font, the friendly page.
                if !crate::layout::fonts::any_font() {
                    return None;
                }
                let doc = word::fmt::read(data, self.word_format()?).ok()?;
                Some(HtmlOutput {
                    html: crate::layout::html::to_html(&doc),
                    files: Vec::new(),
                })
            }
            Mode::Hard | Mode::Hard95 => {
                let laid = self.laid()?;
                Some(render(&laid.analysis, Some(&laid.pdf), opts))
            }
        }
    }

    /// The document laid out in pages, as a PDF: a Word document as Word
    /// lays it out; a workbook printed as Excel prints it (each sheet's
    /// print area cut into pages by its print settings, with the look of
    /// every cell), as [`WorkbookPdfConfig::default`] does; with
    /// [`WorkbookPdfStyle::Document`], laid out as a document instead; an
    /// EPUB book or a web page written as a Word document first, which is
    /// laid out. A PDF already has its pages.
    ///
    /// # Errors
    /// When the paper is a PDF, no font is installed nor given with
    /// [`crate::add_font`], or the layout cannot be written.
    pub fn to_pdf(&self) -> Result<Vec<u8>> {
        self.to_pdf_with(&WorkbookPdfConfig::default())
    }

    /// The document laid out in pages, as [`Paper::to_pdf`] does; a
    /// workbook with the page, fonts and table limits of `config` (they do
    /// not apply to a Word document).
    ///
    /// # Errors
    /// As [`Paper::to_pdf`].
    pub fn to_pdf_with(&self, config: &WorkbookPdfConfig) -> Result<Vec<u8>> {
        if !matches!(self.file, Source::Pdf(_)) && !crate::layout::fonts::any_font() {
            anyhow::bail!(
                "no font is installed: laying out pages needs at least one TrueType or OpenType font \
                 (install one, name its folder in KAKEMONO_FONT_DIRS, or give it with add_font)"
            );
        }
        match &self.file {
            Source::Pdf(_) => anyhow::bail!("the input is already a PDF"),
            Source::Sheet(doc, paper, data, excel_config) => match config.style {
                WorkbookPdfStyle::Document => crate::layout::workbook::to_pdf(doc, *paper, config),
                // As the sheet view reads it, without its limits.
                _ => {
                    // A header's `&F` prints the workbook's file name.
                    let file = self.source.as_deref().map(|source| {
                        std::path::Path::new(source)
                            .file_name()
                            .map_or(source, |name| name.to_str().unwrap_or(source))
                    });
                    let book = excel::read_book(data, excel_config)?;
                    crate::layout::workbook::print::to_pdf(&book, config, file)
                }
            },
            Source::Book(..) => {
                let docx = self.to_docx(&self.analyze(), &DocxOptions::default());
                Paper::from_bytes(docx)?.to_pdf()
            }
            Source::Word(_, format, data) => {
                let format = match format {
                    InputFormat::Doc => WordFormat::Doc,
                    _ => WordFormat::Docx,
                };
                let doc = word::fmt::read(data, format)?;
                crate::layout::to_pdf(&doc)
            }
        }
    }

    /// Records where the PDF came from (file name, arXiv id, ...); it ends up
    /// in [`Meta::source`].
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Sets how hard the PNG pictures of every output are compressed
    /// ([`PngCompression::Balanced`] by default). The pixels stay the same;
    /// [`PngCompression::Fast`] draws the pictures faster for larger files.
    /// Pictures with many colours are stored as JPEG when that is much
    /// smaller than their PNG, so a few more of them may be with `Fast`.
    #[must_use]
    pub fn with_png_compression(mut self, compression: PngCompression) -> Self {
        if let Source::Pdf(pdf) = &mut self.file {
            pdf.set_png_compression(compression);
        }
        self
    }

    /// Number of pages; 0 for a Word document, a workbook or a book, which
    /// have none.
    pub fn page_count(&self) -> usize {
        self.pdf().map_or(0, PdfFile::page_count)
    }

    /// Runs content extraction and layout analysis with default options.
    pub fn analyze(&self) -> Analysis {
        self.analyze_with(AnalyzeOptions::default())
    }

    /// Runs content extraction and layout analysis.
    ///
    /// Pages are read, and their lines built, on `opts.jobs` threads; the
    /// result does not depend on the number of threads.
    /// A Word document, a workbook or a book needs no analysis: its structure, read
    /// when it was loaded, is returned (`opts` does not apply to it).
    pub fn analyze_with(&self, opts: AnalyzeOptions) -> Analysis {
        let pdf = match &self.file {
            Source::Pdf(pdf) => pdf,
            Source::Word(doc, _, _) | Source::Sheet(doc, ..) | Source::Book(doc, _) => {
                let mut doc = doc.as_ref().clone();
                doc.meta.source = self.source.clone();
                // A file with no title of its own is called by its name.
                if doc.meta.title.is_none() {
                    doc.meta.title = self.source.as_deref().and_then(file_title);
                }
                return Analysis {
                    doc,
                    loose: Vec::new(),
                    rotated: Vec::new(),
                };
            }
        };
        let (raw, fonts) = pdf.extract(opts.jobs);
        let meta = Meta {
            source: self.source.clone(),
            ..Default::default()
        };
        let mut analysis = analyze(raw, &fonts, meta, opts);
        // A cover that is a picture holds no title text: the document
        // information of the PDF names the document then.
        if analysis.doc.meta.title.is_none() {
            analysis.doc.meta.title = pdf.info_title();
        }
        analysis
    }

    /// A renderer for pictures of pages and page regions, or `None` for a
    /// Word document or a workbook, which have no pages. Not thread-safe: every thread
    /// that draws needs its own.
    pub fn renderer(&self) -> Option<PageRenderer<'_>> {
        self.pdf().map(PdfFile::renderer)
    }

    /// Renders an analysis to HTML in the requested mode. `analysis` must
    /// come from this paper, since images are drawn from its PDF. A Word
    /// document is first laid out as Word lays it out: `hard` and `hard-95`
    /// show its pages, `medium` its formatting in one continuous page
    /// (neither shows text edited with [`Paper::apply_content`]); when the
    /// layout fails, they give the `friendly` page. A workbook shows its
    /// sheets as Excel does, a tab per sheet, in every mode but `friendly`
    /// (nor do they show edited text).
    pub fn to_html(&self, analysis: &Analysis, opts: &HtmlOptions) -> HtmlOutput {
        if let Some(output) = self.word_layout_html(opts) {
            return output;
        }
        render(analysis, self.pdf(), &self.html_options(opts))
    }

    /// `opts`, with the friendly mode for a document without pages.
    fn html_options(&self, opts: &HtmlOptions) -> HtmlOptions {
        let mut opts = opts.clone();
        if !self.format().has_pages() {
            opts.mode = Mode::Friendly;
        }
        opts
    }

    /// Writes an analysis as HTML to `out`, like [`Paper::to_html`], but
    /// without holding the whole page in memory: the pictures are drawn a
    /// few at a time and written as soon as they are ready, which matters
    /// for long documents (all the page pictures of `hard` mode). When
    /// pictures are not inlined, each picture file is given to `file` as
    /// (path relative to the HTML file, contents) instead of being
    /// returned. The bytes written are those of [`HtmlOutput::html`].
    ///
    /// # Errors
    /// The first error of `out` or `file`.
    pub fn write_html(
        &self,
        analysis: &Analysis,
        opts: &HtmlOptions,
        out: &mut dyn std::io::Write,
        file: &mut dyn FnMut(&str, &[u8]) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let sheet = matches!(self.file, Source::Sheet(..));
        if (self.word_format().is_some() || sheet) && opts.mode != Mode::Friendly {
            if opts.mode == Mode::Medium || sheet {
                if let Some(output) = self.word_layout_html(opts) {
                    return out.write_all(output.html.as_bytes());
                }
            } else if let Some(laid) = self.laid() {
                return render_to(&laid.analysis, Some(&laid.pdf), opts, out, &mut |name, data| {
                    file(&name, &data)
                });
            }
        }
        render_to(
            analysis,
            self.pdf(),
            &self.html_options(opts),
            out,
            &mut |name, data| file(&name, &data),
        )
    }

    /// Writes an analysis as a Word document (`.docx`) and returns the
    /// bytes of the file. `analysis` must come from this paper, since the
    /// pictures are drawn from its PDF.
    pub fn to_docx(&self, analysis: &Analysis, opts: &DocxOptions) -> Vec<u8> {
        render_docx(analysis, self.pdf(), opts)
    }

    /// Writes an analysis as a Word document, like [`Paper::to_docx`], and
    /// also returns each picture of the document as a file of its own in
    /// `image_dir` (a relative path, such as `images`): the pictures stay
    /// embedded in the document too.
    pub fn to_docx_with_images(&self, analysis: &Analysis, opts: &DocxOptions, image_dir: &str) -> DocxOutput {
        render_docx_with_images(analysis, self.pdf(), opts, image_dir)
    }

    /// Writes an analysis as Markdown. `analysis` must come from this paper,
    /// since the pictures are drawn from its PDF.
    pub fn to_markdown(&self, analysis: &Analysis, opts: &MarkdownOptions) -> MarkdownOutput {
        render_markdown(analysis, self.pdf(), opts)
    }

    /// Writes an analysis as a LaTeX document (`.tex`) with its picture
    /// files. `analysis` must come from this paper, since the pictures are
    /// drawn from its PDF.
    pub fn to_latex(&self, analysis: &Analysis, opts: &LatexOptions) -> LatexOutput {
        render_latex(analysis, self.pdf(), opts)
    }

    /// Writes an analysis as a Typst document (`.typ`) with its picture
    /// files, in the layout of an article or of a printed book. `analysis`
    /// must come from this paper, since the pictures are drawn from its PDF.
    pub fn to_typst(&self, analysis: &Analysis, opts: &TypstOptions) -> TypstOutput {
        render_typst(analysis, self.pdf(), opts)
    }

    /// Writes an analysis as an EPUB book (`.epub`) and returns the bytes of
    /// the file. `analysis` must come from this paper, since the pictures
    /// are drawn from its PDF.
    pub fn to_epub(&self, analysis: &Analysis, opts: &EpubOptions) -> Vec<u8> {
        render_epub(analysis, self.pdf(), opts)
    }

    /// Writes an analysis as JSON: the content of the document, block by
    /// block, with its text in a form that can be edited and applied back
    /// with [`Paper::apply_content`]. `analysis` must come from this paper,
    /// since the pictures are drawn from its PDF.
    pub fn to_json(&self, analysis: &Analysis, opts: &JsonOptions) -> JsonOutput {
        render_json(analysis, self.pdf(), opts, &self.source_info())
    }

    /// Cuts an analysis into chunks for retrieval (RAG): pieces of at most
    /// `opts.max_tokens` tokens that follow the sections of the document,
    /// each headed by its section path, with the blocks and pages it comes
    /// from. Write them as JSON Lines with [`chunks_jsonl`](crate::chunks_jsonl).
    pub fn to_chunks(&self, analysis: &Analysis, opts: &ChunkOptions) -> Vec<Chunk> {
        render_chunks(&analysis.doc, opts)
    }

    /// Applies an edited JSON file (written by [`Paper::to_json`] for this
    /// same PDF) to `analysis`: the edited text replaces the text of its
    /// blocks, and every output written from `analysis` afterwards shows
    /// it. Text that was not edited is left as it is. The hard mode, whose
    /// text layer follows the lines of the PDF, has no text for the edited
    /// blocks.
    ///
    /// # Errors
    /// When `json` is not a kakemono JSON file, was made from another PDF,
    /// or names blocks this analysis does not have (it was made with other
    /// analysis options or another version of kakemono). Problems with the
    /// text of single blocks are not errors: those blocks keep their text,
    /// and [`ContentReport::warnings`] says why.
    pub fn apply_content(&self, analysis: &mut Analysis, json: &str) -> Result<ContentReport> {
        self.apply_content_from(analysis, json, None)
    }

    /// [`Paper::apply_content`] for a JSON file in the folder `base`: a
    /// figure's `replace_image` that is a relative path is read from there.
    ///
    /// # Errors
    /// As [`Paper::apply_content`].
    pub fn apply_content_in(&self, analysis: &mut Analysis, json: &str, base: &Path) -> Result<ContentReport> {
        self.apply_content_from(analysis, json, Some(base))
    }

    /// Applies `json` (relative `replace_image` paths read from `base`),
    /// then draws again the figures whose text was edited
    /// (`figure_lines`): their new picture replaces the old one.
    fn apply_content_from(&self, analysis: &mut Analysis, json: &str, base: Option<&Path>) -> Result<ContentReport> {
        let (mut report, edits) = apply_content(analysis, json, &self.source_info(), base)?;
        let Some(renderer) = self.renderer() else {
            return Ok(report);
        };
        for edit in edits {
            let block = &analysis.doc.blocks[edit.block_idx];
            let Some(svg) = crate::render::figure_svg(&renderer, block, &edit, &mut report.warnings) else {
                report.warnings.push(format!(
                    "block {}: the figure could not be drawn again with its edited text",
                    block.id
                ));
                continue;
            };
            // Made ready (its caps checked, its PNG size found), but its own
            // SVG kept: kakemono wrote it, and its edited text stays text,
            // which the SVG written again by usvg would draw as paths.
            let ready =
                crate::word::pictures::normalize(svg.clone().into_bytes()).map(|picture| match picture.format {
                    crate::model::PictureFormat::Svg => crate::model::Picture {
                        data: svg.into_bytes(),
                        ..picture
                    },
                    _ => picture,
                });
            let Some(picture) = ready else {
                report.warnings.push(format!(
                    "block {}: the figure drawn again with its edited text is too large to keep",
                    block.id
                ));
                continue;
            };
            analysis.doc.pictures.push(picture);
            let index = analysis.doc.pictures.len() - 1;
            if let crate::model::BlockKind::Figure { image: Some(spec), .. } =
                &mut analysis.doc.blocks[edit.block_idx].kind
            {
                spec.embedded = Some(index);
            }
        }
        Ok(report)
    }

    /// The source as the JSON output names it.
    fn source_info(&self) -> SourceInfo {
        SourceInfo {
            format: self.format().name(),
            name: self.source.clone(),
            sha256: self.sha256.clone(),
        }
    }
}

/// The SHA-256 of `data`, in hexadecimal.
fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// One-shot conversion: loads the PDF, analyses it and renders it.
///
/// # Errors
/// When the data is not a readable PDF, or the PDF has no pages.
pub fn convert(data: Vec<u8>, opts: &HtmlOptions) -> Result<HtmlOutput> {
    let paper = Paper::from_bytes(data)?;
    let analysis = paper.analyze();
    Ok(paper.to_html(&analysis, opts))
}

/// The name of the file `source` (a path or a name) without its folder and
/// extension (`report.xlsx` is `report`); `None` when nothing is left.
fn file_title(source: &str) -> Option<String> {
    let path = std::path::Path::new(source);
    let stem = path.file_stem()?.to_str()?.trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The friendly page and the Word document of a workbook say what it
    /// was converted from.
    #[test]
    fn outputs_name_the_format_of_a_workbook() {
        let paper = Paper::from_bytes(std::fs::read("tests/data/excel/sample.xlsx").unwrap()).unwrap();
        let analysis = paper.analyze();
        let html = paper.to_html(&analysis, &HtmlOptions::default()).html;
        assert!(html.contains("Converted from Excel by kakemono"), "friendly colophon");
        let docx = paper.to_docx(&analysis, &DocxOptions::default());
        let zip = crate::word::zip::ZipArchive::new(&docx).unwrap();
        let core = zip.read_text("docProps/core.xml").unwrap().unwrap();
        assert!(core.contains("Converted from Excel by kakemono"), "{core}");
    }
}
