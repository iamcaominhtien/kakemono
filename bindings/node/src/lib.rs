//! The Node.js binding of kakemono (`@kakemono/node`): a [`Document`] is read
//! and analysed once, then written to any output. Every method that reads or
//! writes a document returns a promise: the work runs on a thread of
//! kakemono's own (through tokio's blocking pool, so it never fills libuv's
//! pool, which `fs` and `zlib` share), with a large stack.

use kakemono_bindings as core;
use napi::bindgen_prelude::{Buffer, PromiseRaw};
use napi::{Env, Error, Result, Status};
use napi_derive::napi;
use std::sync::Arc;

/// An error of kakemono as a JavaScript error: `code` is `InvalidArg` for a
/// wrong argument or option, `GenericFailure` otherwise.
fn js_error(err: core::Error) -> Error {
    let status = match err.kind() {
        core::ErrorKind::InvalidArgument => Status::InvalidArg,
        core::ErrorKind::Document | core::ErrorKind::Internal => Status::GenericFailure,
    };
    Error::new(status, err.message().to_string())
}

/// Runs `work` on tokio's blocking pool as the promise of a method.
fn spawn<'env, T, F>(env: &'env Env, work: F) -> Result<PromiseRaw<'env, T>>
where
    T: 'static + Send + napi::bindgen_prelude::ToNapiValue,
    F: 'static + Send + FnOnce() -> core::Result<T>,
{
    env.spawn_future(async move {
        tokio::task::spawn_blocking(work)
            .await
            .map_err(|err| Error::new(Status::GenericFailure, format!("internal error in kakemono: {err}")))?
            .map_err(js_error)
    })
}

/// The options of reading and analysing a document, kept for every output.
#[napi(object)]
#[derive(Default)]
pub struct OpenOptions {
    /// The name recorded as the document's source. By default: the file
    /// name, the address, `arXiv:<id>`, nothing for bytes.
    pub source_name: Option<String>,
    /// Rebuild tables as real tables when their grid is clear (default true).
    pub tables: Option<bool>,
    /// Worker threads for one document, 0 for one per core (default 0). On a
    /// server converting several documents at once, 1 or 2 avoids running
    /// more threads than cores.
    pub jobs: Option<u32>,
    /// How hard PNG pictures are compressed; the pixels are the same.
    #[napi(ts_type = "'fast' | 'balanced' | 'high'")]
    pub png_compression: Option<String>,
    /// An edited copy of the output of `toJson()` for the same document
    /// (its text, or the parsed object): every output shows its text.
    #[napi(ts_type = "string | object")]
    pub content: Option<serde_json::Value>,
    /// The folder a relative `replace_image` of `content` is read from.
    pub content_dir: Option<String>,
    /// The header rows of every table of an Excel workbook, given instead
    /// of found.
    pub header_rows: Option<u32>,
}

impl OpenOptions {
    /// The options of the façade.
    fn into_core(self) -> core::OpenOptions {
        core::OpenOptions {
            source_name: self.source_name,
            tables: self.tables,
            jobs: self.jobs.map(|jobs| jobs as usize),
            png_compression: self.png_compression,
            content: self.content.map(|content| match content {
                serde_json::Value::String(text) => text,
                other => other.to_string(),
            }),
            content_dir: self.content_dir,
            header_rows: self.header_rows.map(|rows| rows as usize),
        }
    }
}

/// Options of the HTML output.
#[napi(object)]
#[derive(Default)]
pub struct HtmlOptions {
    /// The mode, from the most faithful to the most readable (default
    /// `'friendly'`).
    #[napi(ts_type = "'hard' | 'hard-95' | 'medium' | 'friendly'")]
    pub mode: Option<String>,
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures, above 0.2 and at most 8
    /// (default 3).
    pub scale: Option<f64>,
    /// How the pages are drawn in the hard modes (default `'svg'`).
    #[napi(ts_type = "'svg' | 'svg-only' | 'png'")]
    pub page_image: Option<String>,
    /// Embed the fonts of the PDF (default true).
    pub embed_fonts: Option<bool>,
    /// Pictures as data URIs in the page (default true); otherwise as
    /// `files`.
    pub inline_images: Option<bool>,
    /// The folder of the picture files, relative (default `'images'`).
    pub image_dir: Option<String>,
}

/// Options of the Markdown output.
#[napi(object)]
#[derive(Default)]
pub struct MarkdownOptions {
    /// `'friendly'` (default), or `'medium'` to keep tables as HTML.
    #[napi(ts_type = "'medium' | 'friendly'")]
    pub mode: Option<String>,
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// The folder of the picture files, relative (default `'images'`).
    pub image_dir: Option<String>,
    /// Pictures as data URIs instead of files (default false).
    pub inline_images: Option<bool>,
}

/// Options of the editable JSON output.
#[napi(object)]
#[derive(Default)]
pub struct JsonOptions {
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// The folder of the picture files, relative (default `'images'`).
    pub image_dir: Option<String>,
    /// Pictures as data URIs instead of files (default false).
    pub inline_images: Option<bool>,
}

/// Options of the Word output.
#[napi(object)]
#[derive(Default)]
pub struct DocxOptions {
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// When given, the pictures are files in this relative folder too
    /// (they are always inside the document).
    pub image_dir: Option<String>,
}

/// Options of the LaTeX output.
#[napi(object)]
#[derive(Default)]
pub struct LatexOptions {
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// The folder of the picture files, relative (default `'images'`).
    pub image_dir: Option<String>,
}

/// Options of the Typst output.
#[napi(object)]
#[derive(Default)]
pub struct TypstOptions {
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// The folder of the picture files, relative (default `'images'`).
    pub image_dir: Option<String>,
    /// A book (title page, chapters), an article, or chosen from the
    /// document (default `'auto'`).
    #[napi(ts_type = "'auto' | 'article' | 'book'")]
    pub layout: Option<String>,
}

/// Options of the EPUB output.
#[napi(object)]
#[derive(Default)]
pub struct EpubOptions {
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// Widest formula kept as MathML, in em (default 20).
    pub max_math_width_em: Option<f64>,
    /// A cover page (default true).
    pub cover: Option<bool>,
    /// The book's date, `CCYY-MM-DDThh:mm:ssZ` (default: a fixed date).
    pub modified: Option<String>,
}

/// Options of the chunks for retrieval.
#[napi(object)]
#[derive(Default)]
pub struct ChunkOptions {
    /// Largest chunk, in tokens (default 512).
    pub max_tokens: Option<u32>,
    /// The path of a Hugging Face `tokenizer.json`, or `'cl100k_base'` /
    /// `'o200k_base'`; tokens are estimated without it.
    pub tokenizer: Option<String>,
}

/// A picture file of an output.
#[napi(object)]
pub struct OutputFile {
    /// Its path, relative to the folder of the output (`images/fig-1.png`).
    pub path: String,
    /// Its bytes.
    pub data: Buffer,
}

/// A text output and its picture files.
#[napi(object)]
pub struct TextOutput {
    /// The text.
    pub text: String,
    /// The picture files to write next to it (empty when the pictures are
    /// inline).
    pub files: Vec<OutputFile>,
}

/// A binary output (Word document, EPUB book, PDF) and its picture files.
#[napi(object)]
pub struct BinaryOutput {
    /// The bytes.
    pub data: Buffer,
    /// The picture files to write next to it (a Word document's, when
    /// `imageDir` is given).
    pub files: Vec<OutputFile>,
}

/// A chunk for retrieval.
#[napi(object)]
pub struct Chunk {
    /// Number of the chunk, in reading order, from 0.
    pub id: u32,
    /// The text to embed: the section path, a blank line, then the text.
    pub text: String,
    /// Its size in tokens.
    pub tokens: u32,
    /// The title of the document.
    pub title: Option<String>,
    /// The headings of the section it is in, from the outermost.
    pub section: Vec<String>,
    /// The blocks of the document it holds (their numbers).
    pub blocks: Vec<u32>,
    /// The pages it comes from, from 1 (empty for documents without pages).
    pub pages: Vec<u32>,
}

/// A `u32` for JavaScript (counts of a document always fit).
fn small(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The files of an output.
fn files(files: Vec<core::File>) -> Vec<OutputFile> {
    files
        .into_iter()
        .map(|file| OutputFile {
            path: file.path,
            data: file.data.into(),
        })
        .collect()
}

/// A text output.
fn text_output(out: core::Output) -> TextOutput {
    TextOutput {
        text: out.text.unwrap_or_default(),
        files: files(out.files),
    }
}

/// A binary output.
fn binary_output(out: core::Output) -> BinaryOutput {
    BinaryOutput {
        data: out.data.unwrap_or_default().into(),
        files: files(out.files),
    }
}

/// A PDF, Word document (`.docx`, `.doc`), Excel workbook (`.xlsx`), EPUB
/// book or web page, read and analysed once: every output written from it
/// shares that analysis. Open one with `Document.fromBytes`, `fromFile`,
/// `fromUrl` or `fromArxiv`.
#[napi]
pub struct Document {
    /// The document, shared with the work in progress.
    doc: Arc<core::Document>,
}

#[napi]
impl Document {
    /// Opens `source`.
    fn open<'env>(
        env: &'env Env,
        source: core::Source,
        options: Option<OpenOptions>,
    ) -> Result<PromiseRaw<'env, Document>> {
        let opts = options.unwrap_or_default().into_core();
        spawn(env, move || {
            core::Document::open(source, &opts).map(|doc| Document { doc: Arc::new(doc) })
        })
    }

    /// Reads and analyses the bytes of a document (copied first).
    #[napi(ts_return_type = "Promise<Document>")]
    pub fn from_bytes<'env>(
        env: &'env Env,
        data: &[u8],
        options: Option<OpenOptions>,
    ) -> Result<PromiseRaw<'env, Document>> {
        Self::open(env, core::Source::Data(data.to_vec()), options)
    }

    /// Reads and analyses the file at `path` (a web page with its pictures
    /// next to it).
    #[napi(ts_return_type = "Promise<Document>")]
    pub fn from_file<'env>(
        env: &'env Env,
        path: String,
        options: Option<OpenOptions>,
    ) -> Result<PromiseRaw<'env, Document>> {
        Self::open(env, core::Source::Path(path), options)
    }

    /// Fetches, reads and analyses the document at the `http(s)` address
    /// `url` (a web page with its pictures).
    #[napi(ts_return_type = "Promise<Document>")]
    pub fn from_url<'env>(
        env: &'env Env,
        url: String,
        options: Option<OpenOptions>,
    ) -> Result<PromiseRaw<'env, Document>> {
        Self::open(env, core::Source::Url(url), options)
    }

    /// Fetches (through the on-disk cache), reads and analyses the PDF of
    /// an arXiv paper: an id such as `'1706.03762'`, or an arXiv URL.
    #[napi(ts_return_type = "Promise<Document>")]
    pub fn from_arxiv<'env>(
        env: &'env Env,
        id: String,
        options: Option<OpenOptions>,
    ) -> Result<PromiseRaw<'env, Document>> {
        Self::open(env, core::Source::Arxiv(id), options)
    }

    /// The input format: `'pdf'`, `'docx'`, `'doc'`, `'xlsx'`, `'epub'` or
    /// `'html'`.
    #[napi(getter, ts_return_type = "'pdf' | 'docx' | 'doc' | 'xlsx' | 'epub' | 'html'")]
    pub fn format(&self) -> String {
        self.doc.format().to_string()
    }

    /// The number of pages of a PDF (0 for the formats that have none).
    #[napi(getter)]
    pub fn page_count(&self) -> u32 {
        small(self.doc.page_count())
    }

    /// The title of the document, when it has one.
    #[napi(getter)]
    pub fn title(&self) -> Option<String> {
        self.doc.title().map(str::to_string)
    }

    /// The warnings of applying `content`: blocks that kept their own text.
    #[napi(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.doc.warnings().to_vec()
    }

    /// Renders the document as `target` on the blocking pool.
    fn render<'env, T, O>(
        &self,
        env: &'env Env,
        target: core::Target,
        opts: core::RenderOptions,
        output: O,
    ) -> Result<PromiseRaw<'env, T>>
    where
        T: 'static + Send + napi::bindgen_prelude::ToNapiValue,
        O: 'static + Send + FnOnce(core::Output) -> T,
    {
        let doc = Arc::clone(&self.doc);
        spawn(env, move || doc.render(target, &opts).map(output))
    }

    /// An HTML page; its pictures are inline unless `inlineImages` is
    /// false.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_html<'env>(&self, env: &'env Env, options: Option<HtmlOptions>) -> Result<PromiseRaw<'env, TextOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            mode: o.mode,
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            page_image: o.page_image,
            embed_fonts: o.embed_fonts,
            inline_images: o.inline_images,
            image_dir: o.image_dir,
            ..Default::default()
        };
        self.render(env, core::Target::Html, opts, text_output)
    }

    /// Markdown and its picture files.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_markdown<'env>(
        &self,
        env: &'env Env,
        options: Option<MarkdownOptions>,
    ) -> Result<PromiseRaw<'env, TextOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            mode: o.mode,
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            inline_images: o.inline_images,
            ..Default::default()
        };
        self.render(env, core::Target::Markdown, opts, text_output)
    }

    /// The editable JSON and its picture files: edit its text, then open
    /// the document again with it as `content`.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_json<'env>(&self, env: &'env Env, options: Option<JsonOptions>) -> Result<PromiseRaw<'env, TextOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            inline_images: o.inline_images,
            ..Default::default()
        };
        self.render(env, core::Target::Json, opts, text_output)
    }

    /// LaTeX source and its picture files.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_latex<'env>(
        &self,
        env: &'env Env,
        options: Option<LatexOptions>,
    ) -> Result<PromiseRaw<'env, TextOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            ..Default::default()
        };
        self.render(env, core::Target::Latex, opts, text_output)
    }

    /// Typst source and its picture files.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_typst<'env>(
        &self,
        env: &'env Env,
        options: Option<TypstOptions>,
    ) -> Result<PromiseRaw<'env, TextOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            layout: o.layout,
            ..Default::default()
        };
        self.render(env, core::Target::Typst, opts, text_output)
    }

    /// A Word document (`.docx`); its pictures are files too when
    /// `imageDir` is given.
    #[napi(ts_return_type = "Promise<BinaryOutput>")]
    pub fn to_docx<'env>(
        &self,
        env: &'env Env,
        options: Option<DocxOptions>,
    ) -> Result<PromiseRaw<'env, BinaryOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            ..Default::default()
        };
        self.render(env, core::Target::Docx, opts, binary_output)
    }

    /// An EPUB 3 book.
    #[napi(ts_return_type = "Promise<BinaryOutput>")]
    pub fn to_epub<'env>(
        &self,
        env: &'env Env,
        options: Option<EpubOptions>,
    ) -> Result<PromiseRaw<'env, BinaryOutput>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            max_math_width_em: o.max_math_width_em.map(|width| width as f32),
            cover: o.cover,
            modified: o.modified,
            ..Default::default()
        };
        self.render(env, core::Target::Epub, opts, binary_output)
    }

    /// A Word document, workbook or EPUB book laid out in pages, as a PDF
    /// (an error for a PDF).
    #[napi(ts_return_type = "Promise<BinaryOutput>")]
    pub fn to_pdf<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, BinaryOutput>> {
        self.render(env, core::Target::Pdf, core::RenderOptions::default(), binary_output)
    }

    /// Chunks for retrieval (RAG), cut at sentence ends and headed by their
    /// section.
    #[napi(ts_return_type = "Promise<Chunk[]>")]
    pub fn to_chunks<'env>(
        &self,
        env: &'env Env,
        options: Option<ChunkOptions>,
    ) -> Result<PromiseRaw<'env, Vec<Chunk>>> {
        let o = options.unwrap_or_default();
        let opts = core::RenderOptions {
            max_tokens: o.max_tokens.map(|tokens| tokens as usize),
            tokenizer: o.tokenizer,
            ..Default::default()
        };
        let doc = Arc::clone(&self.doc);
        spawn(env, move || {
            Ok(doc
                .chunks(&opts)?
                .into_iter()
                .map(|chunk| Chunk {
                    id: small(chunk.id),
                    text: chunk.text,
                    tokens: small(chunk.tokens),
                    title: chunk.title,
                    section: chunk.section,
                    blocks: chunk.blocks.into_iter().map(small).collect(),
                    pages: chunk.pages.into_iter().map(small).collect(),
                })
                .collect())
        })
    }

    /// The structure of the document (its blocks in reading order), as the
    /// command line's `--json` writes it.
    #[napi(ts_return_type = "Promise<any>")]
    pub fn analyze<'env>(&self, env: &'env Env) -> Result<PromiseRaw<'env, serde_json::Value>> {
        let doc = Arc::clone(&self.doc);
        spawn(env, move || {
            let text = doc
                .render(core::Target::Structure, &core::RenderOptions::default())?
                .text
                .unwrap_or_default();
            serde_json::from_str(&text).map_err(|err| core::Error::new(core::ErrorKind::Internal, err.to_string()))
        })
    }
}

/// Adds a TrueType or OpenType font (or a collection) to those documents
/// are laid out and SVG text drawn with, for the documents converted after
/// it (for containers without fonts); returns its number of faces.
#[napi]
pub fn add_font(data: &[u8]) -> Result<u32> {
    core::add_font(data.to_vec()).map(small).map_err(js_error)
}

/// The version of kakemono.
#[napi]
pub fn version() -> &'static str {
    core::version()
}
