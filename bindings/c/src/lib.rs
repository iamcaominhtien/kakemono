//! The C ABI of kakemono: `include/kakemono.h`, generated from this file by
//! cbindgen. It is what the .NET binding calls, and it can be called from C
//! or C++ as it is.
//!
//! The contract, for every function:
//! - Strings given to kakemono are NUL-terminated UTF-8. Options are JSON
//!   text (the names of the façade, `bindings/core`: `{"imageDir":"img"}`);
//!   `NULL` takes the defaults.
//! - Strings and bytes given back are a pointer and a length (`len`, which
//!   may be `NULL`). Text may hold NUL characters, so read `len` bytes; a NUL
//!   follows them anyway, for convenience. They belong to the object they
//!   come from, and stay valid until it is freed.
//! - A function that can fail takes `KkError **err`, which may be `NULL`.
//!   On success it sets `*err` to `NULL`; on failure it returns `NULL` and
//!   sets `*err` to an error the caller frees with [`kk_error_free`].
//! - A `KkDocument` can be rendered from several threads at once; it must
//!   not be freed while a render runs.
//! - No panic crosses the ABI: one becomes an error of kind
//!   [`KK_ERROR_INTERNAL`].
#![allow(unsafe_code)]

use kakemono_bindings::{Document, Error, ErrorKind, OpenOptions, Output, RenderOptions, Source};
use std::ffi::{CStr, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

/// The version of this ABI. It changes when a function changes its
/// signature or meaning (not when one is added); a binding checks it when it
/// loads the library.
pub const KK_ABI_VERSION: u32 = 1;

/// Error kind: a wrong argument or option.
pub const KK_ERROR_INVALID_ARGUMENT: u32 = 1;
/// Error kind: the document cannot be read, fetched or written so.
pub const KK_ERROR_DOCUMENT: u32 = 2;
/// Error kind: a bug of kakemono.
pub const KK_ERROR_INTERNAL: u32 = 3;

/// Text given back: its UTF-8 bytes, then a NUL.
struct Text(Vec<u8>);

impl Text {
    /// The text `text`.
    fn new(text: impl Into<String>) -> Self {
        let mut bytes = text.into().into_bytes();
        bytes.push(0);
        Self(bytes)
    }

    /// The pointer and length to give back.
    fn give(&self, len: *mut usize) -> *const c_char {
        set_len(len, self.0.len() - 1);
        self.0.as_ptr().cast()
    }
}

/// Writes `value` to `len` when it is not `NULL`.
fn set_len(len: *mut usize, value: usize) {
    if !len.is_null() {
        // SAFETY: the caller gives a valid `size_t *` or NULL (checked).
        unsafe { *len = value };
    }
}

/// An error, as the ABI gives it.
pub struct KkError {
    /// One of the `KK_ERROR_*` kinds.
    kind: u32,
    /// The message.
    message: Text,
}

/// A document read and analysed once.
pub struct KkDocument {
    /// The document.
    doc: Document,
    /// Its format, as text.
    format: Text,
    /// Its title.
    title: Option<Text>,
    /// The warnings of applying the content.
    warnings: Vec<Text>,
}

/// An output: text or bytes, and picture files.
pub struct KkOutput {
    /// The text of a text target.
    text: Option<Text>,
    /// The bytes of a binary target.
    data: Option<Vec<u8>>,
    /// The picture files: path, bytes.
    files: Vec<(Text, Vec<u8>)>,
}

impl From<Output> for KkOutput {
    fn from(out: Output) -> Self {
        Self {
            text: out.text.map(Text::new),
            data: out.data,
            files: out
                .files
                .into_iter()
                .map(|file| (Text::new(file.path), file.data))
                .collect(),
        }
    }
}

/// The ABI's kind of `kind`.
fn kind_code(kind: ErrorKind) -> u32 {
    match kind {
        ErrorKind::InvalidArgument => KK_ERROR_INVALID_ARGUMENT,
        ErrorKind::Document => KK_ERROR_DOCUMENT,
        ErrorKind::Internal => KK_ERROR_INTERNAL,
    }
}

/// Gives `err` to the caller through `out` (when it is not `NULL`).
fn give_error(out: *mut *mut KkError, err: &Error) {
    if !out.is_null() {
        let boxed = Box::new(KkError {
            kind: kind_code(err.kind()),
            message: Text::new(err.message()),
        });
        // SAFETY: the caller gives a valid `KkError **` or NULL (checked).
        unsafe { *out = Box::into_raw(boxed) };
    }
}

/// Clears `out` (when it is not `NULL`), as on success.
fn clear_error(out: *mut *mut KkError) {
    if !out.is_null() {
        // SAFETY: the caller gives a valid `KkError **` or NULL (checked).
        unsafe { *out = ptr::null_mut() };
    }
}

/// Runs `work` for a function that can fail: its value as a pointer the
/// caller owns, or `NULL` with the error (a panic too) in `err`.
fn fallible<T>(err: *mut *mut KkError, work: impl FnOnce() -> Result<T, Error>) -> *mut T {
    let result = catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(Error::new(ErrorKind::Internal, "internal error in kakemono's C ABI")));
    match result {
        Ok(value) => {
            clear_error(err);
            Box::into_raw(Box::new(value))
        }
        Err(error) => {
            give_error(err, &error);
            ptr::null_mut()
        }
    }
}

/// Runs `work` for a function that cannot fail, with `fallback` if it
/// panics.
fn infallible<T>(fallback: T, work: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or(fallback)
}

/// The UTF-8 text of the C string `text`, which must not be `NULL`.
fn read_str<'a>(text: *const c_char, what: &str) -> Result<&'a str, Error> {
    if text.is_null() {
        return Err(Error::invalid(format!("{what} is NULL")));
    }
    // SAFETY: the caller gives a NUL-terminated string (the contract).
    unsafe { CStr::from_ptr(text) }
        .to_str()
        .map_err(|_| Error::invalid(format!("{what} is not UTF-8")))
}

/// The UTF-8 text of the C string `text`, or `None` for `NULL`.
fn read_opt_str<'a>(text: *const c_char, what: &str) -> Result<Option<&'a str>, Error> {
    if text.is_null() {
        Ok(None)
    } else {
        read_str(text, what).map(Some)
    }
}

/// Opens a document from `source` with the JSON options `options`.
fn open(source: Source, options: *const c_char) -> Result<KkDocument, Error> {
    let opts = OpenOptions::from_json(read_opt_str(options, "options")?)?;
    let doc = Document::open(source, &opts)?;
    Ok(KkDocument {
        format: Text::new(doc.format()),
        title: doc.title().map(Text::new),
        warnings: doc.warnings().iter().map(Text::new).collect(),
        doc,
    })
}

/// The version of this ABI ([`KK_ABI_VERSION`]).
#[unsafe(no_mangle)]
pub extern "C" fn kk_abi_version() -> u32 {
    KK_ABI_VERSION
}

/// The version of kakemono, as text (static).
///
/// # Safety
/// `len` is `NULL` or points to a `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_version(len: *mut usize) -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    set_len(len, VERSION.len() - 1);
    VERSION.as_ptr().cast()
}

/// Reads and analyses the `len` bytes at `data` (a PDF, a Word document, an
/// Excel workbook, an EPUB book or a web page).
///
/// # Safety
/// `data` points to `len` readable bytes (or `len` is 0); `options` is
/// `NULL` or a NUL-terminated string; `err` is `NULL` or points to a
/// `KkError *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_open_data(
    data: *const u8,
    len: usize,
    options: *const c_char,
    err: *mut *mut KkError,
) -> *mut KkDocument {
    fallible(err, || {
        let bytes = if len == 0 {
            Vec::new()
        } else if data.is_null() {
            return Err(Error::invalid("data is NULL"));
        } else {
            // SAFETY: `data` points to `len` readable bytes (the contract).
            unsafe { std::slice::from_raw_parts(data, len) }.to_vec()
        };
        open(Source::Data(bytes), options)
    })
}

/// Reads and analyses the file at `path` (a web page with its pictures next
/// to it).
///
/// # Safety
/// `path` and `options` are NUL-terminated strings (`options` may be
/// `NULL`); `err` is `NULL` or points to a `KkError *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_open_path(
    path: *const c_char,
    options: *const c_char,
    err: *mut *mut KkError,
) -> *mut KkDocument {
    fallible(err, || open(Source::Path(read_str(path, "path")?.to_string()), options))
}

/// Fetches, reads and analyses the document at the `http(s)` address `url`
/// (a web page with its pictures).
///
/// # Safety
/// As [`kk_open_path`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_open_url(
    url: *const c_char,
    options: *const c_char,
    err: *mut *mut KkError,
) -> *mut KkDocument {
    fallible(err, || open(Source::Url(read_str(url, "url")?.to_string()), options))
}

/// Fetches (through the on-disk cache), reads and analyses the PDF of the
/// arXiv paper `id` (an id such as `1706.03762`, or an arXiv URL).
///
/// # Safety
/// As [`kk_open_path`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_open_arxiv(
    id: *const c_char,
    options: *const c_char,
    err: *mut *mut KkError,
) -> *mut KkDocument {
    fallible(err, || open(Source::Arxiv(read_str(id, "id")?.to_string()), options))
}

/// The input format of `doc`: `pdf`, `docx`, `doc`, `xlsx`, `epub` or
/// `html`.
///
/// # Safety
/// `doc` is `NULL` or a live document; `len` is `NULL` or points to a
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_document_format(doc: *const KkDocument, len: *mut usize) -> *const c_char {
    // SAFETY: `doc` is NULL or a live document (the contract).
    match unsafe { doc.as_ref() } {
        Some(doc) => doc.format.give(len),
        None => ptr::null(),
    }
}

/// The title of `doc`, or `NULL` when it has none.
///
/// # Safety
/// As [`kk_document_format`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_document_title(doc: *const KkDocument, len: *mut usize) -> *const c_char {
    // SAFETY: `doc` is NULL or a live document (the contract).
    match unsafe { doc.as_ref() }.and_then(|doc| doc.title.as_ref()) {
        Some(title) => title.give(len),
        None => ptr::null(),
    }
}

/// The number of pages of a PDF (0 for the formats that have none).
///
/// # Safety
/// `doc` is `NULL` or a live document.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_document_page_count(doc: *const KkDocument) -> usize {
    // SAFETY: `doc` is NULL or a live document (the contract).
    infallible(0, || unsafe { doc.as_ref() }.map_or(0, |doc| doc.doc.page_count()))
}

/// The number of warnings of applying the content.
///
/// # Safety
/// `doc` is `NULL` or a live document.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_document_warning_count(doc: *const KkDocument) -> usize {
    // SAFETY: `doc` is NULL or a live document (the contract).
    unsafe { doc.as_ref() }.map_or(0, |doc| doc.warnings.len())
}

/// Warning `index` of applying the content, or `NULL` past the last.
///
/// # Safety
/// As [`kk_document_format`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_document_warning(doc: *const KkDocument, index: usize, len: *mut usize) -> *const c_char {
    // SAFETY: `doc` is NULL or a live document (the contract).
    match unsafe { doc.as_ref() }.and_then(|doc| doc.warnings.get(index)) {
        Some(warning) => warning.give(len),
        None => ptr::null(),
    }
}

/// Writes `doc` as `target` (`html`, `docx`, `markdown`, `json`, `latex`,
/// `typst`, `epub`, `chunks`, `pdf` or `structure`) with the JSON options
/// `options`.
///
/// # Safety
/// `doc` is a live document; `target` and `options` are NUL-terminated
/// strings (`options` may be `NULL`); `err` is `NULL` or points to a
/// `KkError *`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_render(
    doc: *const KkDocument,
    target: *const c_char,
    options: *const c_char,
    err: *mut *mut KkError,
) -> *mut KkOutput {
    fallible(err, || {
        // SAFETY: `doc` is NULL or a live document (the contract).
        let doc = unsafe { doc.as_ref() }.ok_or_else(|| Error::invalid("doc is NULL"))?;
        let target = read_str(target, "target")?.parse()?;
        let opts = RenderOptions::from_json(read_opt_str(options, "options")?)?;
        Ok(KkOutput::from(doc.doc.render(target, &opts)?))
    })
}

/// The text of a text output, or `NULL` for a binary one.
///
/// # Safety
/// `out` is `NULL` or a live output; `len` is `NULL` or points to a
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_output_text(out: *const KkOutput, len: *mut usize) -> *const c_char {
    // SAFETY: `out` is NULL or a live output (the contract).
    match unsafe { out.as_ref() }.and_then(|out| out.text.as_ref()) {
        Some(text) => text.give(len),
        None => ptr::null(),
    }
}

/// The bytes of a binary output, or `NULL` for a text one.
///
/// # Safety
/// As [`kk_output_text`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_output_data(out: *const KkOutput, len: *mut usize) -> *const u8 {
    // SAFETY: `out` is NULL or a live output (the contract).
    match unsafe { out.as_ref() }.and_then(|out| out.data.as_ref()) {
        Some(data) => {
            set_len(len, data.len());
            data.as_ptr()
        }
        None => ptr::null(),
    }
}

/// The number of picture files of an output.
///
/// # Safety
/// `out` is `NULL` or a live output.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_output_file_count(out: *const KkOutput) -> usize {
    // SAFETY: `out` is NULL or a live output (the contract).
    unsafe { out.as_ref() }.map_or(0, |out| out.files.len())
}

/// The path of picture file `index`, relative to the output's folder, or
/// `NULL` past the last.
///
/// # Safety
/// As [`kk_output_text`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_output_file_path(out: *const KkOutput, index: usize, len: *mut usize) -> *const c_char {
    // SAFETY: `out` is NULL or a live output (the contract).
    match unsafe { out.as_ref() }.and_then(|out| out.files.get(index)) {
        Some((path, _)) => path.give(len),
        None => ptr::null(),
    }
}

/// The bytes of picture file `index`, or `NULL` past the last.
///
/// # Safety
/// As [`kk_output_text`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_output_file_data(out: *const KkOutput, index: usize, len: *mut usize) -> *const u8 {
    // SAFETY: `out` is NULL or a live output (the contract).
    match unsafe { out.as_ref() }.and_then(|out| out.files.get(index)) {
        Some((_, data)) => {
            set_len(len, data.len());
            data.as_ptr()
        }
        None => ptr::null(),
    }
}

/// The kind of an error: one of the `KK_ERROR_*` constants (0 for `NULL`).
///
/// # Safety
/// `err` is `NULL` or a live error.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_error_kind(err: *const KkError) -> u32 {
    // SAFETY: `err` is NULL or a live error (the contract).
    unsafe { err.as_ref() }.map_or(0, |err| err.kind)
}

/// The message of an error.
///
/// # Safety
/// `err` is `NULL` or a live error; `len` is `NULL` or points to a
/// `size_t`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_error_message(err: *const KkError, len: *mut usize) -> *const c_char {
    // SAFETY: `err` is NULL or a live error (the contract).
    match unsafe { err.as_ref() } {
        Some(err) => err.message.give(len),
        None => ptr::null(),
    }
}

/// Frees a document (`NULL` does nothing). No render of it may be running.
///
/// # Safety
/// `doc` is `NULL` or a document of `kk_open_*` not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_document_free(doc: *mut KkDocument) {
    if !doc.is_null() {
        // SAFETY: `doc` came from `Box::into_raw` and is freed once (the contract).
        infallible((), || drop(unsafe { Box::from_raw(doc) }));
    }
}

/// Frees an output (`NULL` does nothing).
///
/// # Safety
/// `out` is `NULL` or an output of [`kk_render`] not yet freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_output_free(out: *mut KkOutput) {
    if !out.is_null() {
        // SAFETY: `out` came from `Box::into_raw` and is freed once (the contract).
        infallible((), || drop(unsafe { Box::from_raw(out) }));
    }
}

/// Frees an error (`NULL` does nothing).
///
/// # Safety
/// `err` is `NULL` or an error given by a function of this ABI, not yet
/// freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn kk_error_free(err: *mut KkError) {
    if !err.is_null() {
        // SAFETY: `err` came from `Box::into_raw` and is freed once (the contract).
        infallible((), || drop(unsafe { Box::from_raw(err) }));
    }
}
