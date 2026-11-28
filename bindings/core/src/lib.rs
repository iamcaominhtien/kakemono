//! The façade of kakemono that its bindings for other languages share (the
//! C ABI, Node, WebAssembly, the JVM): a [`Document`] is read and analysed
//! once ([`Document::open`]), then written to any [`Target`]
//! ([`Document::render`]). The options are plain data ([`OpenOptions`],
//! [`RenderOptions`]) that each binding fills from its own language, or reads
//! from JSON text. The checks, defaults and messages are those of the Python
//! binding (`python/src/lib.rs`).
//!
//! The work runs on a thread of its own with a large stack
//! ([`STACK_SIZE`]), not on the caller's (the threads of a JVM, of .NET's
//! pool or of libuv have about 1 MB, and reading a PDF recurses deeply).
//! A panic of the library is caught there and returned as an [`Error`], so
//! that no binding unwinds into its host; on WebAssembly, which has neither
//! threads nor unwinding, the work runs on the caller's stack and a panic
//! aborts the module.
//!
//! Every conversion can be stopped from another thread, or after a time
//! limit, by a [`CancelToken`] (the `_with` methods): it then ends with an
//! error of kind [`ErrorKind::Cancelled`] or [`ErrorKind::TimedOut`], soon
//! after the request (normally within a page), and the [`Document`] stays
//! usable. Not on WebAssembly, where stopping would abort the module.

use kakemono::config::ExcelConfig;
use kakemono::{
    AnalyzeOptions, ChunkOptions, DocxOptions, EpubOptions, Equations, HtmlOptions, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, PageImage, Paper, PngCompression, TokenCounter, TypstLayout, TypstOptions,
};
pub use kakemono::{CancelToken, Cancelled, Chunk};
use serde::Deserialize;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

/// Smallest accepted `scale` (pixels per PDF point), exclusive: smaller
/// pictures are unreadable. The command line has the same limits.
const MIN_SCALE: f32 = 0.2;
/// Largest accepted `scale` (pixels per PDF point).
const MAX_SCALE: f32 = 8.0;

/// Stack size of the thread the work runs on: as large as the library's
/// own worker threads (`common::parallel`), since with one job the work runs
/// there.
pub const STACK_SIZE: usize = 16 << 20;

/// What kind of error an [`Error`] is, for the bindings to map it to their
/// language's exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorKind {
    /// A wrong argument or option (an unknown target, a scale out of range,
    /// invalid JSON options).
    InvalidArgument = 1,
    /// The document cannot be read, fetched or written so (a damaged file,
    /// a missing file, a failed download, content that does not fit).
    Document = 2,
    /// A bug of kakemono (a panic), or a thread that could not start.
    Internal = 3,
    /// The conversion was stopped by [`CancelToken::cancel`].
    Cancelled = 4,
    /// The conversion was stopped by the time limit of its [`CancelToken`].
    TimedOut = 5,
}

/// What went wrong: its kind, and a message for people with its causes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The kind of error.
    kind: ErrorKind,
    /// The message.
    message: String,
}

impl Error {
    /// An error of `kind` with `message`.
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// A wrong argument or option.
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidArgument, message)
    }

    /// The kind of error.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

impl From<anyhow::Error> for Error {
    fn from(err: anyhow::Error) -> Self {
        Self::new(ErrorKind::Document, format!("{err:#}"))
    }
}

impl From<Cancelled> for Error {
    fn from(stopped: Cancelled) -> Self {
        let kind = match stopped {
            Cancelled::TimedOut => ErrorKind::TimedOut,
            _ => ErrorKind::Cancelled,
        };
        Self::new(kind, stopped.to_string())
    }
}

/// A result of this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// The message of a panic's payload.
pub fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    panic
        .downcast_ref::<&str>()
        .map(|text| (*text).to_string())
        .or_else(|| panic.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown cause".into())
}

/// Runs `work` on a thread with a [`STACK_SIZE`] stack and waits for it,
/// turning a panic into an [`Error`]. Bindings call it for their own work
/// that may reach the library (the façade's methods already use it).
///
/// # Errors
/// The error of `work`, or [`ErrorKind::Internal`] when it panics or the
/// thread cannot start.
#[cfg(not(target_arch = "wasm32"))]
pub fn guarded<T: Send>(work: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("kakemono".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || catch_unwind(AssertUnwindSafe(work)))
            .map_err(|err| Error::new(ErrorKind::Internal, format!("cannot start a thread: {err}")))?;
        match worker.join() {
            Ok(Ok(result)) => result,
            Ok(Err(panic)) | Err(panic) => Err(Error::new(
                ErrorKind::Internal,
                format!("internal error in kakemono: {}", panic_message(&*panic)),
            )),
        }
    })
}

/// [`guarded`] on WebAssembly: on the caller's stack (there are no
/// threads); a panic aborts the module.
///
/// # Errors
/// The error of `work`.
#[cfg(target_arch = "wasm32")]
pub fn guarded<T: Send>(work: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    work()
}

/// [`guarded`], stopped by `stop` when it is given: a conversion stopped
/// so returns an error of kind [`ErrorKind::Cancelled`] or
/// [`ErrorKind::TimedOut`].
///
/// # Errors
/// As [`guarded`], and when `stop` stops the work before its end (or is
/// already stopped).
pub fn guarded_with<T: Send>(stop: Option<&CancelToken>, work: impl FnOnce() -> Result<T> + Send) -> Result<T> {
    guarded(|| match stop {
        // A stopped download can end in an error of its own before the
        // token unwinds: the error is then the token's.
        Some(token) => token.run(work)?.map_err(|err| match token.state() {
            Some(stopped) => stopped.into(),
            None => err,
        }),
        None => work(),
    })
}

/// The token a call runs with, from a binding's two ways of stopping it:
/// `cancel`, a token of the caller's, and `timeout`, a time limit of this
/// call (a child of `cancel` when both are given, so that cancelling it
/// still stops the call). `None` when neither is given.
pub fn stop_token(cancel: Option<&CancelToken>, timeout: Option<Duration>) -> Option<CancelToken> {
    match (cancel, timeout) {
        (Some(token), Some(limit)) => Some(token.child(Some(limit))),
        (Some(token), None) => Some(token.clone()),
        (None, Some(limit)) => Some(CancelToken::with_timeout(limit)),
        (None, None) => None,
    }
}

/// A time limit given in milliseconds by a binding: `None` for none.
///
/// # Errors
/// When `millis` is not a positive, finite number.
pub fn timeout_millis(millis: Option<f64>) -> Result<Option<Duration>> {
    millis
        .map(|ms| {
            Duration::try_from_secs_f64(ms / 1000.0)
                .ok()
                .filter(|limit| !limit.is_zero())
                .ok_or_else(|| Error::invalid("timeout must be a positive number of milliseconds"))
        })
        .transpose()
}

/// The version of the kakemono library.
pub fn version() -> &'static str {
    kakemono::VERSION
}

/// Adds a TrueType or OpenType font (or a collection) to those documents
/// are laid out and SVG text drawn with, for the documents converted after
/// it: for where no font is installed (WebAssembly, small containers).
/// Returns the number of faces.
///
/// # Errors
/// When `data` holds no font face.
pub fn add_font(data: Vec<u8>) -> Result<usize> {
    kakemono::add_font(data).map_err(|err| Error::invalid(format!("{err:#}")))
}

/// The arXiv identifier in `text` (an id such as `1706.03762` or an arXiv
/// URL), if there is one.
pub fn parse_arxiv_id(text: &str) -> Option<String> {
    kakemono::source::arxiv::parse_id(text)
}

/// Whether `text` is an `http(s)` address.
pub fn is_url(text: &str) -> bool {
    kakemono::source::web::is_url(text)
}

/// Where a document is read from.
#[derive(Debug, Clone)]
pub enum Source {
    /// The bytes of a PDF, a Word document, an Excel workbook, an EPUB book
    /// or a web page.
    Data(Vec<u8>),
    /// The file at a path (a web page with its pictures next to it).
    Path(String),
    /// The document at an `http(s)` address (a web page with its pictures,
    /// fetched). Needs the `net` feature.
    Url(String),
    /// The PDF of an arXiv paper (an id or an arXiv URL), through the
    /// on-disk cache. Needs the `net` feature.
    Arxiv(String),
}

impl Source {
    /// What a string names, as the command line and the Python package read
    /// it: an existing file, else an arXiv id or URL, else an `http(s)`
    /// address.
    ///
    /// # Errors
    /// When `text` names none of them.
    pub fn guess(text: &str) -> Result<Self> {
        if Path::new(text).exists() {
            return Ok(Self::Path(text.to_string()));
        }
        if let Some(id) = parse_arxiv_id(text) {
            return Ok(Self::Arxiv(id));
        }
        if is_url(text) {
            return Ok(Self::Url(text.trim().to_string()));
        }
        Err(Error::invalid(format!(
            "'{text}' is neither a file, an arXiv id or URL, nor an http(s) address"
        )))
    }
}

/// The options of reading and analysing a document, kept for every output.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenOptions {
    /// The name recorded as the document's source (a file name, an
    /// address). By default: the file name of a path, the address of a URL,
    /// `arXiv:<id>`, nothing for bytes.
    pub source_name: Option<String>,
    /// Rebuild tables as real tables when their grid is clear (default true).
    pub tables: Option<bool>,
    /// Worker threads, 0 for one per core (default 0).
    pub jobs: Option<usize>,
    /// How hard PNG pictures are compressed: `"fast"`, `"balanced"`
    /// (default) or `"high"`; the pixels are the same.
    pub png_compression: Option<String>,
    /// The text of an edited copy of the JSON output for the same document:
    /// every output shows its text.
    pub content: Option<String>,
    /// The folder a relative `replace_image` of `content` is read from.
    pub content_dir: Option<String>,
    /// The header rows of every table of an Excel workbook, given instead
    /// of found.
    pub header_rows: Option<usize>,
}

impl OpenOptions {
    /// Options read from JSON text (`null`, empty or absent: the defaults).
    ///
    /// # Errors
    /// When the JSON is invalid or names an unknown option.
    pub fn from_json(json: Option<&str>) -> Result<Self> {
        from_json(json)
    }
}

/// Options read from JSON text; the defaults for nothing, `""` or `null`.
fn from_json<T: Default + for<'de> Deserialize<'de>>(json: Option<&str>) -> Result<T> {
    match json.map(str::trim) {
        None | Some("" | "null") => Ok(T::default()),
        Some(json) => serde_json::from_str(json).map_err(|err| Error::invalid(format!("invalid options: {err}"))),
    }
}

/// What a document is written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// An HTML page (one of the four modes).
    Html,
    /// A Word document (bytes).
    Docx,
    /// Markdown, with its pictures.
    Markdown,
    /// The editable JSON, with its pictures.
    Json,
    /// LaTeX source, with its pictures.
    Latex,
    /// Typst source, with its pictures.
    Typst,
    /// An EPUB book (bytes).
    Epub,
    /// Chunks for retrieval, as JSON Lines.
    Chunks,
    /// A Word document, workbook or EPUB book laid out in pages (bytes).
    Pdf,
    /// The document structure (blocks in reading order), as JSON.
    Structure,
}

impl Target {
    /// Every target, with its name.
    pub const ALL: [(&'static str, Self); 10] = [
        ("html", Self::Html),
        ("docx", Self::Docx),
        ("markdown", Self::Markdown),
        ("json", Self::Json),
        ("latex", Self::Latex),
        ("typst", Self::Typst),
        ("epub", Self::Epub),
        ("chunks", Self::Chunks),
        ("pdf", Self::Pdf),
        ("structure", Self::Structure),
    ];

    /// The name of the target, as [`Target::from_str`] reads it.
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, target)| *target == self)
            .map_or("", |(name, _)| name)
    }

    /// The options that apply to this target (by their JSON names).
    fn options(self) -> &'static [&'static str] {
        match self {
            Self::Html => &[
                "mode",
                "equations",
                "scale",
                "pageImage",
                "embedFonts",
                "inlineImages",
                "imageDir",
            ],
            Self::Docx => &["equations", "scale", "imageDir"],
            Self::Markdown => &["equations", "scale", "imageDir", "inlineImages", "mode"],
            Self::Json => &["equations", "scale", "imageDir", "inlineImages"],
            Self::Latex => &["equations", "scale", "imageDir"],
            Self::Typst => &["equations", "scale", "imageDir", "layout"],
            Self::Epub => &["equations", "scale", "maxMathWidthEm", "cover", "modified"],
            Self::Chunks => &["maxTokens", "tokenizer"],
            Self::Pdf | Self::Structure => &[],
        }
    }

    /// Whether the output is bytes (else text).
    pub fn is_binary(self) -> bool {
        matches!(self, Self::Docx | Self::Epub | Self::Pdf)
    }
}

impl FromStr for Target {
    type Err = Error;

    fn from_str(name: &str) -> Result<Self> {
        Self::ALL
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, target)| *target)
            .ok_or_else(|| {
                let names: Vec<&str> = Self::ALL.iter().map(|(name, _)| *name).collect();
                Error::invalid(format!(
                    "unknown target '{name}' (expected one of {})",
                    names.join(", ")
                ))
            })
    }
}

/// The options of one output. Each applies to some targets only
/// ([`Document::render`] refuses the others); an option not given takes the
/// default of the Python package.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderOptions {
    /// HTML: `"hard"`, `"hard-95"`, `"medium"` or `"friendly"` (default);
    /// Markdown: `"friendly"` (default) or `"medium"` (keeps HTML tables).
    pub mode: Option<String>,
    /// How displayed formulas are written: `"mathml"` (default) or `"image"`.
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures, above 0.2 and at most 8
    /// (default 3).
    pub scale: Option<f32>,
    /// HTML: the page pictures as `"svg"` (default), `"svg-only"` or `"png"`.
    pub page_image: Option<String>,
    /// HTML: embed the fonts of the PDF (default true).
    pub embed_fonts: Option<bool>,
    /// HTML, Markdown, JSON: pictures as data URIs instead of files (default
    /// true for HTML, false otherwise).
    pub inline_images: Option<bool>,
    /// The folder of the picture files, relative (default `"images"`; for
    /// Word documents, no files unless given).
    pub image_dir: Option<String>,
    /// Typst: `"auto"` (default), `"article"` or `"book"`.
    pub layout: Option<String>,
    /// EPUB: widest formula kept as MathML, in em (default 20).
    pub max_math_width_em: Option<f32>,
    /// EPUB: a cover page (default true).
    pub cover: Option<bool>,
    /// EPUB: its date, `CCYY-MM-DDThh:mm:ssZ` (default: fixed).
    pub modified: Option<String>,
    /// Chunks: largest chunk, in tokens (default 512).
    pub max_tokens: Option<usize>,
    /// Chunks: the path of a Hugging Face `tokenizer.json` or the name of an
    /// OpenAI tokenizer (`cl100k_base`, `o200k_base`); tokens are estimated
    /// without it.
    pub tokenizer: Option<String>,
}

impl RenderOptions {
    /// Options read from JSON text (`null`, empty or absent: the defaults).
    ///
    /// # Errors
    /// When the JSON is invalid or names an unknown option.
    pub fn from_json(json: Option<&str>) -> Result<Self> {
        from_json(json)
    }

    /// The options given, by their JSON names.
    fn given(&self) -> Vec<&'static str> {
        let all = [
            ("mode", self.mode.is_some()),
            ("equations", self.equations.is_some()),
            ("scale", self.scale.is_some()),
            ("pageImage", self.page_image.is_some()),
            ("embedFonts", self.embed_fonts.is_some()),
            ("inlineImages", self.inline_images.is_some()),
            ("imageDir", self.image_dir.is_some()),
            ("layout", self.layout.is_some()),
            ("maxMathWidthEm", self.max_math_width_em.is_some()),
            ("cover", self.cover.is_some()),
            ("modified", self.modified.is_some()),
            ("maxTokens", self.max_tokens.is_some()),
            ("tokenizer", self.tokenizer.is_some()),
        ];
        all.into_iter()
            .filter(|(_, given)| *given)
            .map(|(name, _)| name)
            .collect()
    }

    /// The formulas option.
    fn equations(&self) -> Result<Equations> {
        match self.equations.as_deref().unwrap_or("mathml") {
            "mathml" => Ok(Equations::MathMl),
            "image" => Ok(Equations::Image),
            other => Err(Error::invalid(format!(
                "unknown equations '{other}' (expected 'mathml' or 'image')"
            ))),
        }
    }

    /// The scale, checked.
    fn scale(&self) -> Result<f32> {
        let scale = self.scale.unwrap_or(3.0);
        if scale > MIN_SCALE && scale <= MAX_SCALE {
            Ok(scale)
        } else {
            Err(Error::invalid("scale must be between 0.2 and 8"))
        }
    }

    /// The picture folder: `image_dir` or `default`, which must stay inside
    /// the output folder.
    fn image_dir(&self, default: &str) -> Result<String> {
        let dir = self.image_dir.as_deref().unwrap_or(default);
        check_image_dir(dir)?;
        Ok(dir.trim_end_matches(['/', '\\']).to_string())
    }
}

/// Checks that `dir` is a relative folder that stays inside the output
/// folder.
fn check_image_dir(dir: &str) -> Result<()> {
    let normal = dir.replace('\\', "/");
    // A `:` makes a Windows path drive-relative (`C:dir`) or names an NTFS
    // stream: either can leave the output folder.
    if dir.is_empty()
        || normal.starts_with('/')
        || Path::new(dir).is_absolute()
        || normal.split('/').any(|part| part == ".." || part.contains(':'))
    {
        return Err(Error::invalid(format!(
            "imageDir must be a relative folder inside the output folder, not '{dir}'"
        )));
    }
    Ok(())
}

/// Parses a mode name as on the command line.
fn parse_mode(name: &str) -> Result<Mode> {
    match name {
        "hard" => Ok(Mode::Hard),
        "hard-95" | "hard95" => Ok(Mode::Hard95),
        "medium" => Ok(Mode::Medium),
        "friendly" => Ok(Mode::Friendly),
        other => Err(Error::invalid(format!(
            "unknown mode '{other}' (expected 'hard', 'hard-95', 'medium' or 'friendly')"
        ))),
    }
}

/// The name of a mode, as [`parse_mode`] reads it.
fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Hard => "hard",
        Mode::Hard95 => "hard-95",
        Mode::Medium => "medium",
        Mode::Friendly => "friendly",
    }
}

/// Parses a page-image name as on the command line.
fn parse_page_image(name: &str) -> Result<PageImage> {
    match name {
        "svg" => Ok(PageImage::Svg),
        "svg-only" => Ok(PageImage::SvgOnly),
        "png" => Ok(PageImage::Png),
        other => Err(Error::invalid(format!(
            "unknown pageImage '{other}' (expected 'svg', 'svg-only' or 'png')"
        ))),
    }
}

/// Parses a PNG compression level as on the command line.
fn parse_png_compression(name: &str) -> Result<PngCompression> {
    match name {
        "fast" => Ok(PngCompression::Fast),
        "balanced" => Ok(PngCompression::Balanced),
        "high" => Ok(PngCompression::High),
        other => Err(Error::invalid(format!(
            "unknown pngCompression '{other}' (expected 'fast', 'balanced' or 'high')"
        ))),
    }
}

/// Checks the date of an EPUB book.
fn check_modified(date: &str) -> Result<()> {
    let bytes = date.as_bytes();
    let digit = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let ok = bytes.len() == 20
        && digit(0..4)
        && bytes[4] == b'-'
        && digit(5..7)
        && bytes[7] == b'-'
        && digit(8..10)
        && bytes[10] == b'T'
        && digit(11..13)
        && bytes[13] == b':'
        && digit(14..16)
        && bytes[16] == b':'
        && digit(17..19)
        && bytes[19] == b'Z';
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "modified must look like 2026-01-31T12:00:00Z, not '{date}'"
        )))
    }
}

/// A file written with an output: a picture, at a path relative to the
/// output's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Its path, relative to the folder of the output (`images/fig-1.png`).
    pub path: String,
    /// Its bytes.
    pub data: Vec<u8>,
}

/// What [`Document::render`] writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// The text of a text target (HTML, Markdown, JSON, LaTeX, Typst,
    /// chunks, structure).
    pub text: Option<String>,
    /// The bytes of a binary target (Word document, EPUB book, PDF).
    pub data: Option<Vec<u8>>,
    /// The picture files to write next to it.
    pub files: Vec<File>,
}

impl Output {
    /// A text output.
    fn text(text: String, files: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            text: Some(text),
            data: None,
            files: into_files(files),
        }
    }

    /// A binary output.
    fn data(data: Vec<u8>, files: Vec<(String, Vec<u8>)>) -> Self {
        Self {
            text: None,
            data: Some(data),
            files: into_files(files),
        }
    }
}

/// The library's (path, bytes) pairs as [`File`]s.
fn into_files(files: Vec<(String, Vec<u8>)>) -> Vec<File> {
    files.into_iter().map(|(path, data)| File { path, data }).collect()
}

/// A document read and analysed once, with the edited text of
/// [`OpenOptions::content`] applied: every output shares that analysis. It
/// can be used from several threads at once.
pub struct Document {
    /// The document.
    paper: Paper,
    /// Its analysis, with the content applied.
    analysis: kakemono::Analysis,
    /// Worker threads.
    jobs: usize,
    /// Whether content was applied.
    edited: bool,
    /// The warnings of applying the content.
    warnings: Vec<String>,
}

impl fmt::Debug for Document {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Document")
            .field("format", &self.format())
            .field("edited", &self.edited)
            .finish_non_exhaustive()
    }
}

impl Document {
    /// Reads and analyses the document at `source`.
    ///
    /// # Errors
    /// When the document cannot be read, fetched or analysed, an option is
    /// wrong, or the content does not fit the document.
    pub fn open(source: Source, opts: &OpenOptions) -> Result<Self> {
        Self::open_with(source, opts, None)
    }

    /// [`Document::open`], stopped by `stop` (see the crate's comment).
    ///
    /// # Errors
    /// As [`Document::open`], and when `stop` stops it.
    pub fn open_with(source: Source, opts: &OpenOptions, stop: Option<&CancelToken>) -> Result<Self> {
        guarded_with(stop, || Self::open_unguarded(source, opts))
    }

    /// [`Document::open`], without catching panics.
    fn open_unguarded(source: Source, opts: &OpenOptions) -> Result<Self> {
        let png = parse_png_compression(opts.png_compression.as_deref().unwrap_or("balanced"))?;
        let excel = ExcelConfig {
            header_rows: opts.header_rows,
            ..Default::default()
        };
        let (paper, default_name) = match source {
            Source::Data(data) => (Paper::from_bytes_with(data, &excel)?, None),
            Source::Path(path) => {
                let name = Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned());
                (Paper::from_path_with(&path, &excel)?, name)
            }
            Source::Url(url) => {
                if opts.header_rows.is_some() {
                    return Err(Error::invalid(
                        "headerRows applies to a workbook read from bytes or a file, not from an address",
                    ));
                }
                (Paper::from_url(&url)?, None)
            }
            Source::Arxiv(text) => {
                let id = parse_arxiv_id(&text)
                    .ok_or_else(|| Error::invalid(format!("'{text}' is not an arXiv id or URL")))?;
                let data = kakemono::source::arxiv::fetch_pdf(&id)?;
                (Paper::from_bytes_with(data, &excel)?, Some(format!("arXiv:{id}")))
            }
        };
        let mut paper = paper.with_png_compression(png);
        if let Some(name) = opts.source_name.clone().or(default_name) {
            paper = paper.with_source(name);
        }
        let jobs = opts.jobs.unwrap_or(0);
        let mut analysis = paper.analyze_with(AnalyzeOptions {
            tables: opts.tables.unwrap_or(true),
            jobs,
            ..Default::default()
        });
        let warnings = match (&opts.content, &opts.content_dir) {
            (Some(json), Some(dir)) => paper.apply_content_in(&mut analysis, json, Path::new(dir))?.warnings,
            (Some(json), None) => paper.apply_content(&mut analysis, json)?.warnings,
            (None, Some(_)) => return Err(Error::invalid("contentDir is given without content")),
            (None, None) => Vec::new(),
        };
        Ok(Self {
            paper,
            analysis,
            jobs,
            edited: opts.content.is_some(),
            warnings,
        })
    }

    /// The input format: `"pdf"`, `"docx"`, `"doc"`, `"xlsx"`, `"epub"` or
    /// `"html"`.
    pub fn format(&self) -> &'static str {
        self.paper.format().name()
    }

    /// The number of pages of a PDF (0 for the formats that have none).
    pub fn page_count(&self) -> usize {
        if self.paper.format().has_pages() {
            self.paper.page_count()
        } else {
            0
        }
    }

    /// The title of the document, when it has one (a paper's title, a Word
    /// document's or a page's).
    pub fn title(&self) -> Option<&str> {
        self.analysis.doc.meta.title.as_deref()
    }

    /// The warnings of applying the content: blocks that kept their text.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Writes the document as `target`.
    ///
    /// # Errors
    /// When an option is wrong or does not apply to `target`, or the
    /// document cannot be written so (the hard mode of a Word document, a
    /// PDF of a PDF, an unknown tokenizer).
    pub fn render(&self, target: Target, opts: &RenderOptions) -> Result<Output> {
        self.render_with(target, opts, None)
    }

    /// [`Document::render`], stopped by `stop` (see the crate's comment).
    ///
    /// # Errors
    /// As [`Document::render`], and when `stop` stops it.
    pub fn render_with(&self, target: Target, opts: &RenderOptions, stop: Option<&CancelToken>) -> Result<Output> {
        guarded_with(stop, || self.render_unguarded(target, opts))
    }

    /// [`Document::render`] with the options as JSON text.
    ///
    /// # Errors
    /// As [`Document::render`], and when the target or the JSON is invalid.
    pub fn render_json(&self, target: &str, opts: Option<&str>) -> Result<Output> {
        self.render_json_with(target, opts, None)
    }

    /// [`Document::render_json`], stopped by `stop`.
    ///
    /// # Errors
    /// As [`Document::render_json`], and when `stop` stops it.
    pub fn render_json_with(&self, target: &str, opts: Option<&str>, stop: Option<&CancelToken>) -> Result<Output> {
        self.render_with(target.parse()?, &RenderOptions::from_json(opts)?, stop)
    }

    /// [`Document::render`], without catching panics.
    fn render_unguarded(&self, target: Target, opts: &RenderOptions) -> Result<Output> {
        let allowed = target.options();
        if let Some(name) = opts.given().into_iter().find(|name| !allowed.contains(name)) {
            return Err(Error::invalid(format!("{name} does not apply to {}", target.name())));
        }
        let jobs = self.jobs;
        let (paper, analysis) = (&self.paper, &self.analysis);
        Ok(match target {
            Target::Html => {
                let mode = parse_mode(opts.mode.as_deref().unwrap_or("friendly"))?;
                // The modes that draw a layout do not show edited text: the
                // pages of a PDF in hard mode, a Word document's or a
                // workbook's own layout in every mode but friendly (an EPUB
                // book and a web page have none: they are always friendly).
                let layout_only = if paper.format().has_pages() {
                    mode == Mode::Hard
                } else {
                    mode != Mode::Friendly && matches!(self.format(), "docx" | "doc" | "xlsx")
                };
                if self.edited && layout_only {
                    return Err(Error::invalid(format!(
                        "the {} mode of a {} document shows its own layout: it cannot show edited text from content{}",
                        mode_name(mode),
                        self.format(),
                        if paper.format().has_pages() {
                            " (hard-95 can)"
                        } else {
                            " (friendly can)"
                        }
                    )));
                }
                let html = HtmlOptions {
                    mode,
                    scale: opts.scale()?,
                    page_image: parse_page_image(opts.page_image.as_deref().unwrap_or("svg"))?,
                    jobs,
                    embed_fonts: opts.embed_fonts.unwrap_or(true),
                    equations: opts.equations()?,
                    inline_images: opts.inline_images.unwrap_or(true),
                    asset_dir: opts.image_dir("images")?,
                };
                let out = paper.to_html(analysis, &html);
                Output::text(out.html, out.files)
            }
            Target::Docx => {
                let docx = DocxOptions {
                    scale: opts.scale()?,
                    jobs,
                    equations: opts.equations()?,
                };
                match &opts.image_dir {
                    Some(_) => {
                        let out = paper.to_docx_with_images(analysis, &docx, &opts.image_dir("images")?);
                        Output::data(out.docx, out.files)
                    }
                    None => Output::data(paper.to_docx(analysis, &docx), Vec::new()),
                }
            }
            Target::Markdown => {
                let mode = match parse_mode(opts.mode.as_deref().unwrap_or("friendly"))? {
                    mode @ (Mode::Medium | Mode::Friendly) => mode,
                    _ => {
                        return Err(Error::invalid(
                            "Markdown has no pages: mode can be 'friendly' or 'medium'",
                        ));
                    }
                };
                let markdown = MarkdownOptions {
                    inline_images: opts.inline_images.unwrap_or(false),
                    asset_dir: opts.image_dir("images")?,
                    scale: opts.scale()?,
                    jobs,
                    equations: opts.equations()?,
                    mode,
                };
                let out = paper.to_markdown(analysis, &markdown);
                Output::text(out.markdown, out.files)
            }
            Target::Json => {
                let json = JsonOptions {
                    inline_images: opts.inline_images.unwrap_or(false),
                    asset_dir: opts.image_dir("images")?,
                    scale: opts.scale()?,
                    jobs,
                    equations: opts.equations()?,
                };
                let out = paper.to_json(analysis, &json);
                Output::text(out.json, out.files)
            }
            Target::Latex => {
                let latex = LatexOptions {
                    asset_dir: opts.image_dir("images")?,
                    scale: opts.scale()?,
                    jobs,
                    equations: opts.equations()?,
                };
                let out = paper.to_latex(analysis, &latex);
                Output::text(out.latex, out.files)
            }
            Target::Typst => {
                let layout = match opts.layout.as_deref().unwrap_or("auto") {
                    "auto" => None,
                    "article" => Some(TypstLayout::Article),
                    "book" => Some(TypstLayout::Book),
                    other => {
                        return Err(Error::invalid(format!(
                            "layout must be 'auto', 'article' or 'book', not '{other}'"
                        )));
                    }
                };
                let typst = TypstOptions {
                    asset_dir: opts.image_dir("images")?,
                    scale: opts.scale()?,
                    jobs,
                    equations: opts.equations()?,
                    layout,
                };
                let out = paper.to_typst(analysis, &typst);
                Output::text(out.typst, out.files)
            }
            Target::Epub => {
                if let Some(date) = &opts.modified {
                    check_modified(date)?;
                }
                let max_math_width_em = opts.max_math_width_em.unwrap_or(20.0);
                if !(max_math_width_em.is_finite() && max_math_width_em > 0.0) {
                    return Err(Error::invalid("maxMathWidthEm must be a positive number"));
                }
                let epub = EpubOptions {
                    scale: opts.scale()?,
                    jobs,
                    equations: opts.equations()?,
                    max_math_width_em,
                    cover: opts.cover.unwrap_or(true),
                    modified: opts.modified.clone(),
                };
                Output::data(paper.to_epub(analysis, &epub), Vec::new())
            }
            Target::Chunks => Output::text(kakemono::chunks_jsonl(&self.chunks_unguarded(opts)?), Vec::new()),
            Target::Pdf => Output::data(self.to_pdf()?, Vec::new()),
            Target::Structure => Output::text(
                serde_json::to_string(&analysis.doc).map_err(|err| Error::new(ErrorKind::Internal, err.to_string()))?,
                Vec::new(),
            ),
        })
    }

    /// Chunks for retrieval, as objects (the [`Target::Chunks`] output is
    /// the same as JSON Lines). Only `maxTokens` and `tokenizer` apply.
    ///
    /// # Errors
    /// When another option is given, `maxTokens` is 0 or the tokenizer
    /// cannot be loaded.
    pub fn chunks(&self, opts: &RenderOptions) -> Result<Vec<Chunk>> {
        self.chunks_with(opts, None)
    }

    /// [`Document::chunks`], stopped by `stop`.
    ///
    /// # Errors
    /// As [`Document::chunks`], and when `stop` stops it.
    pub fn chunks_with(&self, opts: &RenderOptions, stop: Option<&CancelToken>) -> Result<Vec<Chunk>> {
        guarded_with(stop, || {
            let allowed = Target::Chunks.options();
            if let Some(name) = opts.given().into_iter().find(|name| !allowed.contains(name)) {
                return Err(Error::invalid(format!("{name} does not apply to chunks")));
            }
            self.chunks_unguarded(opts)
        })
    }

    /// [`Document::chunks`], without the thread and the check of options.
    fn chunks_unguarded(&self, opts: &RenderOptions) -> Result<Vec<Chunk>> {
        let max_tokens = opts.max_tokens.unwrap_or(512);
        if max_tokens == 0 {
            return Err(Error::invalid("maxTokens must be at least 1"));
        }
        let counter = match &opts.tokenizer {
            Some(spec) => TokenCounter::load(spec)?,
            None => TokenCounter::estimate(),
        };
        Ok(self
            .paper
            .to_chunks(&self.analysis, &ChunkOptions { max_tokens, counter }))
    }

    /// A Word document, workbook or EPUB book laid out in pages, as a PDF;
    /// with edited text, the edited document is laid out.
    fn to_pdf(&self) -> Result<Vec<u8>> {
        if self.paper.format().has_pages() {
            return Err(Error::invalid(
                "the input is already a PDF: only a Word document, a workbook or an EPUB book is laid out as a PDF",
            ));
        }
        if self.edited {
            // The layout engine reads a Word file: the edited text and
            // pictures go into a Word document first, as on the command line.
            let docx = self.paper.to_docx(&self.analysis, &DocxOptions::default());
            Ok(Paper::from_bytes(docx)?.to_pdf()?)
        } else {
            Ok(self.paper.to_pdf()?)
        }
    }
}

#[cfg(test)]
mod tests;
