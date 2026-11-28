//! The Node.js binding of kakemono (`@kakemono/node`): a [`Document`] is read
//! and analysed once, then written to any output. Every method that reads or
//! writes a document returns a promise: the work runs on a thread of
//! kakemono's own (through tokio's blocking pool, so it never fills libuv's
//! pool, which `fs` and `zlib` share), with a large stack.

use core::CancelToken;
use kakemono_bindings as core;
use napi::bindgen_prelude::{
    Buffer, FnArgs, FromNapiValue, Function, JsObjectValue, JsValue, Object, PromiseRaw, TypeName, Unknown,
    ValidateNapiValue,
};
use napi::{Env, Error, Result, Status, ValueType, sys};
use napi_derive::napi;
use std::sync::Arc;

/// An error of kakemono as a JavaScript error, its `code` saying its kind:
/// `InvalidArg` for a wrong argument or option, `GenericFailure` for a
/// document that cannot be read, fetched or written so, `Unknown` for a bug
/// of kakemono (`Cancelled` for a stopped call, which [`rejection`] gives as
/// an `AbortError` or a `TimeoutError` instead).
fn js_error(err: core::Error) -> Error {
    let status = match err.kind() {
        core::ErrorKind::InvalidArgument => Status::InvalidArg,
        core::ErrorKind::Document => Status::GenericFailure,
        core::ErrorKind::Internal => Status::Unknown,
        core::ErrorKind::Cancelled | core::ErrorKind::TimedOut => Status::Cancelled,
    };
    Error::new(status, err.message().to_string())
}

/// The conversions that run at once: `KAKEMONO_MAX_CONCURRENCY`, or one per
/// core. The others wait for their turn, so that a burst of calls (a server's
/// requests) holds that many documents in memory, not all of them; the work
/// is bound by the processor, so running more at once is not faster.
fn conversions() -> &'static tokio::sync::Semaphore {
    static CONVERSIONS: std::sync::OnceLock<tokio::sync::Semaphore> = std::sync::OnceLock::new();
    CONVERSIONS.get_or_init(|| {
        let cores = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        let max = std::env::var("KAKEMONO_MAX_CONCURRENCY")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|&max| max > 0)
            .unwrap_or(cores);
        tokio::sync::Semaphore::new(max)
    })
}

/// An `AbortSignal` given with a call: a listener of its `abort` event
/// cancels the call's token, and is removed once the call ends (when this
/// is dropped, on the JavaScript thread).
pub struct Signal {
    /// The environment the signal lives in.
    env: sys::napi_env,
    /// The signal.
    signal: sys::napi_ref,
    /// The listener added to it (`None` when the signal was already aborted).
    listener: Option<sys::napi_ref>,
    /// Cancelled when the signal is aborted.
    token: CancelToken,
    /// The JavaScript thread of `env`: the only one the references may be
    /// used on.
    thread: std::thread::ThreadId,
}

/// `addEventListener` or `removeEventListener` of an event target, called
/// with an event name and a listener of type `L`.
type Listen<'env, L> = Function<'env, FnArgs<(&'env str, L)>, ()>;

impl TypeName for Signal {
    fn type_name() -> &'static str {
        "AbortSignal"
    }

    fn value_type() -> ValueType {
        ValueType::Object
    }
}

impl ValidateNapiValue for Signal {}

impl FromNapiValue for Signal {
    unsafe fn from_napi_value(env: sys::napi_env, value: sys::napi_value) -> Result<Self> {
        // SAFETY: napi gives a value of `env`, on its thread.
        let signal = unsafe { Object::from_napi_value(env, value) }?;
        let token = CancelToken::new();
        let aborted: Option<bool> = signal.get_named_property("aborted")?;
        let add: Option<Listen<'_, Function<'_, (), ()>>> = signal.get_named_property("addEventListener")?;
        let (Some(aborted), Some(add)) = (aborted, add) else {
            return Err(Error::new(Status::InvalidArg, "signal must be an AbortSignal"));
        };
        let mut signal_ref = std::ptr::null_mut();
        // SAFETY: `value` is a live object of `env`.
        check(unsafe { sys::napi_create_reference(env, value, 1, &mut signal_ref) })?;
        let mut this = Self {
            env,
            signal: signal_ref,
            listener: None,
            token,
            thread: std::thread::current().id(),
        };
        if aborted {
            this.token.cancel();
            return Ok(this);
        }
        let stopper = this.token.clone();
        // SAFETY: as above.
        let js_env = Env::from_raw(env);
        let listener = js_env.create_function_from_closure("abort", move |_| {
            stopper.cancel();
            Ok(())
        })?;
        add.apply(signal, FnArgs::from(("abort", listener)))?;
        let mut listener_ref = std::ptr::null_mut();
        // SAFETY: `listener` is a live function of `env`.
        check(unsafe { sys::napi_create_reference(env, listener.raw(), 1, &mut listener_ref) })?;
        this.listener = Some(listener_ref);
        Ok(this)
    }
}

impl Signal {
    /// The signal's `reason`, for the `cause` of the error of an aborted call.
    fn reason(&self) -> Option<Unknown<'_>> {
        let signal = self.object()?;
        signal.get_named_property::<Unknown<'_>>("reason").ok()
    }

    /// The signal, while it lives.
    fn object(&self) -> Option<Object<'_>> {
        let mut value = std::ptr::null_mut();
        // SAFETY: `signal` is a reference of `env`, used on its thread.
        check(unsafe { sys::napi_get_reference_value(self.env, self.signal, &mut value) }).ok()?;
        // SAFETY: as above.
        unsafe { Object::from_napi_value(self.env, value) }.ok()
    }
}

impl Drop for Signal {
    fn drop(&mut self) {
        // napi drops the callback that owns this on another thread when the
        // environment went away first (a terminated worker): the references
        // went with it, and must not be touched.
        if std::thread::current().id() != self.thread {
            return;
        }
        // Errors are left: the signal may already be collected.
        if let Some(listener) = self.listener.take() {
            let mut function = std::ptr::null_mut();
            // SAFETY: the references are of `env`, used on its thread
            // (checked above).
            unsafe {
                if check(sys::napi_get_reference_value(self.env, listener, &mut function)).is_ok()
                    && let Some(signal) = self.object()
                    && let Ok(Some(remove)) =
                        signal.get_named_property::<Option<Listen<'_, Unknown<'_>>>>("removeEventListener")
                    && let Ok(function) = Unknown::from_napi_value(self.env, function)
                {
                    let _ = remove.apply(signal, FnArgs::from(("abort", function)));
                }
                sys::napi_delete_reference(self.env, listener);
            }
        }
        // SAFETY: as above.
        unsafe { sys::napi_delete_reference(self.env, self.signal) };
    }
}

/// A napi status as a result.
fn check(status: sys::napi_status) -> Result<()> {
    if status == sys::Status::napi_ok {
        Ok(())
    } else {
        Err(Error::new(
            Status::GenericFailure,
            format!("napi call failed ({status})"),
        ))
    }
}

/// How a call may be stopped: its `signal` and `timeout` options.
struct Stop {
    /// The signal, kept until the call ends.
    signal: Option<Signal>,
    /// The token of the call, or why the options are wrong.
    token: core::Result<Option<CancelToken>>,
}

impl Stop {
    /// From a call's options.
    fn new(signal: Option<Signal>, timeout: Option<f64>) -> Self {
        let token = core::timeout_millis(timeout)
            .map(|limit| core::stop_token(signal.as_ref().map(|signal| &signal.token), limit));
        Self { signal, token }
    }
}

/// The error a call is rejected with: [`js_error`], or, for a call that
/// was stopped, an `AbortError` (`code` `'ABORT_ERR'`, its `cause` the
/// signal's reason) or a `TimeoutError` (`code` `'ETIMEDOUT'`).
fn rejection(env: &Env, err: core::Error, signal: Option<&Signal>) -> Error {
    let (name, code) = match err.kind() {
        core::ErrorKind::Cancelled => ("AbortError", "ABORT_ERR"),
        core::ErrorKind::TimedOut => ("TimeoutError", "ETIMEDOUT"),
        _ => return js_error(err),
    };
    let build = || -> Result<Error> {
        let mut error = env.create_error(Error::new(Status::Cancelled, err.message().to_string()))?;
        error.set_named_property("name", name)?;
        error.set_named_property("code", code)?;
        if name == "AbortError"
            && let Some(reason) = signal.and_then(Signal::reason)
        {
            error.set_named_property("cause", reason)?;
        }
        Ok(Error::from(error.to_unknown()))
    };
    build().unwrap_or_else(|_| js_error(err))
}

/// How often a call waiting for its turn looks at its token.
const QUEUE_POLL: std::time::Duration = std::time::Duration::from_millis(20);

/// Runs `work` on tokio's blocking pool as the promise of a method, once
/// its turn comes ([`conversions`]), with the token of `stop`; a call
/// stopped while it waits for its turn leaves the queue at once.
fn spawn<'env, T, F>(env: &'env Env, stop: Stop, work: F) -> Result<PromiseRaw<'env, T>>
where
    T: 'static + Send + napi::bindgen_prelude::ToNapiValue,
    F: 'static + Send + FnOnce(Option<&CancelToken>) -> core::Result<T>,
{
    let Stop { signal, token } = stop;
    env.spawn_future_with_callback(
        async move {
            // The outcome is always handed to the callback, which removes
            // the signal's listener.
            Ok(run(token, work).await)
        },
        move |env, outcome: core::Result<T>| {
            let result = outcome.map_err(|err| rejection(env, err, signal.as_ref()));
            drop(signal);
            result
        },
    )
}

/// [`spawn`]'s work: its turn, then `work` on the blocking pool.
async fn run<T, F>(token: core::Result<Option<CancelToken>>, work: F) -> core::Result<T>
where
    T: 'static + Send,
    F: 'static + Send + FnOnce(Option<&CancelToken>) -> core::Result<T>,
{
    let internal = |err: &dyn std::fmt::Display| {
        core::Error::new(core::ErrorKind::Internal, format!("internal error in kakemono: {err}"))
    };
    let token = token?;
    let mut acquire = std::pin::pin!(conversions().acquire());
    // Its turn among the conversions (the semaphore is never closed).
    let _turn = loop {
        match &token {
            None => break (&mut acquire).await.map_err(|err| internal(&err))?,
            Some(token) => match tokio::time::timeout(QUEUE_POLL, &mut acquire).await {
                Ok(turn) => break turn.map_err(|err| internal(&err))?,
                Err(_) => {
                    if let Some(stopped) = token.state() {
                        return Err(stopped.into());
                    }
                }
            },
        }
    };
    tokio::task::spawn_blocking(move || work(token.as_ref()))
        .await
        .map_err(|err| internal(&err))?
}

/// The options of reading and analysing a document, kept for every output.
#[napi(object, object_to_js = false)]
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
    pub jobs: Option<f64>,
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
    pub header_rows: Option<f64>,
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// A count option: a whole number, 0 or more (JavaScript numbers are
/// doubles, and napi would wrap -1 to 4294967295).
fn count(name: &str, value: Option<f64>) -> core::Result<Option<usize>> {
    match value {
        None => Ok(None),
        Some(number) if number.is_finite() && number >= 0.0 && number.fract() == 0.0 && number <= 1e15 => {
            Ok(Some(number as usize))
        }
        Some(number) => Err(core::Error::invalid(format!(
            "{name} must be a whole number, 0 or more, not {number}"
        ))),
    }
}

impl OpenOptions {
    /// The options of the façade.
    fn into_core(self) -> core::Result<core::OpenOptions> {
        Ok(core::OpenOptions {
            source_name: self.source_name,
            tables: self.tables,
            jobs: count("jobs", self.jobs)?,
            png_compression: self.png_compression,
            content: self.content.map(|content| match content {
                serde_json::Value::String(text) => text,
                other => other.to_string(),
            }),
            content_dir: self.content_dir,
            header_rows: count("headerRows", self.header_rows)?,
        })
    }
}

/// Options of the HTML output.
#[napi(object, object_to_js = false)]
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
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the Markdown output.
#[napi(object, object_to_js = false)]
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
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the editable JSON output.
#[napi(object, object_to_js = false)]
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
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the Word output.
#[napi(object, object_to_js = false)]
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
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the LaTeX output.
#[napi(object, object_to_js = false)]
#[derive(Default)]
pub struct LatexOptions {
    /// How displayed formulas are written (default `'mathml'`).
    #[napi(ts_type = "'mathml' | 'image'")]
    pub equations: Option<String>,
    /// Pixels per PDF point of the pictures (default 3).
    pub scale: Option<f64>,
    /// The folder of the picture files, relative (default `'images'`).
    pub image_dir: Option<String>,
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the Typst output.
#[napi(object, object_to_js = false)]
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
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the EPUB output.
#[napi(object, object_to_js = false)]
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
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// Options of the chunks for retrieval.
#[napi(object, object_to_js = false)]
#[derive(Default)]
pub struct ChunkOptions {
    /// Largest chunk, in tokens (default 512).
    pub max_tokens: Option<f64>,
    /// The path of a Hugging Face `tokenizer.json`, or `'cl100k_base'` /
    /// `'o200k_base'`; tokens are estimated without it.
    pub tokenizer: Option<String>,
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

/// The options of a call that has no others: how to stop it.
#[napi(object, object_to_js = false)]
#[derive(Default)]
pub struct StopOptions {
    /// Stops the call when it is aborted: the promise is then rejected
    /// with an `AbortError` (its `cause` is the signal's reason).
    #[napi(ts_type = "AbortSignal")]
    pub signal: Option<Signal>,
    /// A time limit for the call, in milliseconds: past it, the promise is
    /// rejected with a `TimeoutError`.
    pub timeout: Option<f64>,
}

impl StopOptions {
    /// The [`Stop`] of `options`.
    fn stop(options: Option<Self>) -> Stop {
        let options = options.unwrap_or_default();
        Stop::new(options.signal, options.timeout)
    }
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
    /// The pages it comes from, from 0 (`[0]` for documents without pages).
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
        let mut options = options.unwrap_or_default();
        let stop = Stop::new(options.signal.take(), options.timeout);
        let opts = options.into_core();
        spawn(env, stop, move |token| {
            let opts = opts?;
            core::Document::open_with(source, &opts, token).map(|doc| Document { doc: Arc::new(doc) })
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
        stop: Stop,
        target: core::Target,
        opts: core::RenderOptions,
        output: O,
    ) -> Result<PromiseRaw<'env, T>>
    where
        T: 'static + Send + napi::bindgen_prelude::ToNapiValue,
        O: 'static + Send + FnOnce(core::Output) -> T,
    {
        let doc = Arc::clone(&self.doc);
        spawn(env, stop, move |token| {
            doc.render_with(target, &opts, token).map(output)
        })
    }

    /// An HTML page; its pictures are inline unless `inlineImages` is
    /// false.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_html<'env>(&self, env: &'env Env, options: Option<HtmlOptions>) -> Result<PromiseRaw<'env, TextOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
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
        self.render(env, stop, core::Target::Html, opts, text_output)
    }

    /// Markdown and its picture files.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_markdown<'env>(
        &self,
        env: &'env Env,
        options: Option<MarkdownOptions>,
    ) -> Result<PromiseRaw<'env, TextOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            mode: o.mode,
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            inline_images: o.inline_images,
            ..Default::default()
        };
        self.render(env, stop, core::Target::Markdown, opts, text_output)
    }

    /// The editable JSON and its picture files: edit its text, then open
    /// the document again with it as `content`.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_json<'env>(&self, env: &'env Env, options: Option<JsonOptions>) -> Result<PromiseRaw<'env, TextOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            inline_images: o.inline_images,
            ..Default::default()
        };
        self.render(env, stop, core::Target::Json, opts, text_output)
    }

    /// LaTeX source and its picture files.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_latex<'env>(
        &self,
        env: &'env Env,
        options: Option<LatexOptions>,
    ) -> Result<PromiseRaw<'env, TextOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            ..Default::default()
        };
        self.render(env, stop, core::Target::Latex, opts, text_output)
    }

    /// Typst source and its picture files.
    #[napi(ts_return_type = "Promise<TextOutput>")]
    pub fn to_typst<'env>(
        &self,
        env: &'env Env,
        options: Option<TypstOptions>,
    ) -> Result<PromiseRaw<'env, TextOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            layout: o.layout,
            ..Default::default()
        };
        self.render(env, stop, core::Target::Typst, opts, text_output)
    }

    /// A Word document (`.docx`); its pictures are files too when
    /// `imageDir` is given.
    #[napi(ts_return_type = "Promise<BinaryOutput>")]
    pub fn to_docx<'env>(
        &self,
        env: &'env Env,
        options: Option<DocxOptions>,
    ) -> Result<PromiseRaw<'env, BinaryOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            image_dir: o.image_dir,
            ..Default::default()
        };
        self.render(env, stop, core::Target::Docx, opts, binary_output)
    }

    /// An EPUB 3 book.
    #[napi(ts_return_type = "Promise<BinaryOutput>")]
    pub fn to_epub<'env>(
        &self,
        env: &'env Env,
        options: Option<EpubOptions>,
    ) -> Result<PromiseRaw<'env, BinaryOutput>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            equations: o.equations,
            scale: o.scale.map(|scale| scale as f32),
            max_math_width_em: o.max_math_width_em.map(|width| width as f32),
            cover: o.cover,
            modified: o.modified,
            ..Default::default()
        };
        self.render(env, stop, core::Target::Epub, opts, binary_output)
    }

    /// A Word document, workbook or EPUB book laid out in pages, as a PDF
    /// (an error for a PDF).
    #[napi(ts_return_type = "Promise<BinaryOutput>")]
    pub fn to_pdf<'env>(&self, env: &'env Env, options: Option<StopOptions>) -> Result<PromiseRaw<'env, BinaryOutput>> {
        let stop = StopOptions::stop(options);
        self.render(
            env,
            stop,
            core::Target::Pdf,
            core::RenderOptions::default(),
            binary_output,
        )
    }

    /// Chunks for retrieval (RAG), cut at sentence ends and headed by their
    /// section.
    #[napi(ts_return_type = "Promise<Chunk[]>")]
    pub fn to_chunks<'env>(
        &self,
        env: &'env Env,
        options: Option<ChunkOptions>,
    ) -> Result<PromiseRaw<'env, Vec<Chunk>>> {
        let mut o = options.unwrap_or_default();
        let stop = Stop::new(o.signal.take(), o.timeout);
        let opts = core::RenderOptions {
            tokenizer: o.tokenizer,
            ..Default::default()
        };
        let max_tokens = count("maxTokens", o.max_tokens);
        let doc = Arc::clone(&self.doc);
        spawn(env, stop, move |token| {
            let opts = core::RenderOptions {
                max_tokens: max_tokens?,
                ..opts
            };
            Ok(doc
                .chunks_with(&opts, token)?
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
    pub fn analyze<'env>(
        &self,
        env: &'env Env,
        options: Option<StopOptions>,
    ) -> Result<PromiseRaw<'env, serde_json::Value>> {
        let doc = Arc::clone(&self.doc);
        spawn(env, StopOptions::stop(options), move |token| {
            let text = doc
                .render_with(core::Target::Structure, &core::RenderOptions::default(), token)?
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
