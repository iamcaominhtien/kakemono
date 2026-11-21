//! High-level entry points: PDF, Word or Excel bytes in, structured document and
//! HTML (or Word, Markdown, LaTeX or EPUB) out.

use crate::analysis::pipeline::{Analysis, analyze};
use crate::backend::{PageRenderer, PdfFile, PngCompression};
use crate::config::{AnalyzeOptions, ExcelConfig};
use crate::excel;
use crate::model::{Document, Meta};
use crate::render::{
    Chunk, ChunkOptions, ContentReport, DocxOptions, DocxOutput, EpubOptions, HtmlOptions, HtmlOutput, JsonOptions,
    JsonOutput, LatexOptions, LatexOutput, MarkdownOptions, MarkdownOutput, Mode, SourceInfo, apply_content, render,
    render_chunks, render_docx, render_docx_with_images, render_epub, render_json, render_latex, render_markdown,
    render_to,
};
use crate::word::{self, WordFormat};
use anyhow::Result;
use sha2::{Digest, Sha256};

/// The format of the file a [`Paper`] was loaded from. More formats may
/// be read in a later version: a `match` on it needs a `_` arm.
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
}

impl InputFormat {
    /// The usual file extension (`pdf`, `docx`, `doc`, `xlsx`), also the name of the
    /// format in the JSON output.
    pub fn name(self) -> &'static str {
        match self {
            InputFormat::Pdf => "pdf",
            InputFormat::Docx => "docx",
            InputFormat::Doc => "doc",
            InputFormat::Xlsx => "xlsx",
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
    /// An Excel workbook, read into its structure when loaded (no file is
    /// kept: there is no layout to make).
    Sheet(Box<Document>),
}

/// A loaded PDF, Word document or Excel workbook, ready to be analysed and
/// rendered.
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
    /// Loads a PDF, a Word document (`.docx`, `.doc`) or an Excel workbook
    /// (`.xlsx`) from its bytes; the format is recognised from the bytes.
    ///
    /// # Errors
    /// When the data is not a readable PDF, Word document or workbook, or
    /// the PDF has no pages.
    pub fn from_bytes(data: Vec<u8>) -> Result<Self> {
        Self::from_bytes_with(data, &ExcelConfig::default())
    }

    /// Loads a file like [`Paper::from_bytes`], reading a workbook with
    /// the thresholds `excel` (they do not apply to the other formats).
    ///
    /// # Errors
    /// As [`Paper::from_bytes`].
    pub fn from_bytes_with(data: Vec<u8>, excel: &ExcelConfig) -> Result<Self> {
        let sha256 = Sha256::digest(&data).iter().map(|byte| format!("{byte:02x}")).collect();
        // Both are ZIP packages: each looks for its own main part.
        if excel::detect(&data) {
            let doc = excel::read(&data, excel, None)?;
            return Ok(Self::loaded(Source::Sheet(Box::new(doc)), sha256));
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

    /// The format of the file.
    pub fn format(&self) -> InputFormat {
        match &self.file {
            Source::Pdf(_) => InputFormat::Pdf,
            Source::Word(_, format, _) => *format,
            Source::Sheet(_) => InputFormat::Xlsx,
        }
    }

    /// The PDF, when the paper is one.
    fn pdf(&self) -> Option<&PdfFile> {
        match &self.file {
            Source::Pdf(pdf) => Some(pdf),
            Source::Word(..) | Source::Sheet(_) => None,
        }
    }

    /// The format a Word document is read with.
    fn word_format(&self) -> Option<WordFormat> {
        match &self.file {
            Source::Pdf(_) | Source::Sheet(_) => None,
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
    /// page (`medium`). `None` for a PDF, the `friendly` mode, or when the
    /// layout failed.
    fn word_layout_html(&self, opts: &HtmlOptions) -> Option<HtmlOutput> {
        let Source::Word(_, _, data) = &self.file else {
            return None;
        };
        match opts.mode {
            Mode::Friendly => None,
            Mode::Medium => {
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

    /// The document laid out in pages as Word lays it out, as a PDF: only
    /// for a Word document (a PDF already has its pages, a workbook has
    /// none).
    ///
    /// # Errors
    /// When the paper is a PDF or a workbook, or the layout cannot be
    /// written.
    pub fn to_pdf(&self) -> Result<Vec<u8>> {
        match &self.file {
            Source::Pdf(_) => anyhow::bail!("the input is already a PDF"),
            Source::Sheet(_) => anyhow::bail!("a workbook has no page layout"),
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

    /// Number of pages; 0 for a Word document or a workbook, which have
    /// none.
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
    /// A Word document or a workbook needs no analysis: its structure, read
    /// when it was loaded, is returned (`opts` does not apply to it).
    pub fn analyze_with(&self, opts: AnalyzeOptions) -> Analysis {
        let pdf = match &self.file {
            Source::Pdf(pdf) => pdf,
            Source::Word(doc, _, _) | Source::Sheet(doc) => {
                let mut doc = doc.as_ref().clone();
                doc.meta.source = self.source.clone();
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
    /// layout fails, they give the `friendly` page.
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
        if self.word_format().is_some() && opts.mode != Mode::Friendly {
            if opts.mode == Mode::Medium {
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
        apply_content(analysis, json, &self.source_info())
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

/// One-shot conversion: loads the PDF, analyses it and renders it.
///
/// # Errors
/// When the data is not a readable PDF, or the PDF has no pages.
pub fn convert(data: Vec<u8>, opts: &HtmlOptions) -> Result<HtmlOutput> {
    let paper = Paper::from_bytes(data)?;
    let analysis = paper.analyze();
    Ok(paper.to_html(&analysis, opts))
}
