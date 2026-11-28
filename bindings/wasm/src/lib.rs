//! The WebAssembly binding of kakemono (`@kakemono/wasm`), for browsers,
//! workers and edge runtimes: a [`Document`] is read from bytes and
//! analysed once, then written to any output. Everything is synchronous and
//! runs on the caller's thread (WebAssembly here has no threads): run it in
//! a Web Worker to keep a page responsive.
//!
//! What differs from the native bindings: documents come from bytes only
//! (no files, no network), one thread, no system fonts (text in SVG
//! pictures drawn to PNG, and PDFs laid out from Word documents or
//! workbooks, fall back to the fonts they embed), tokens of chunks are
//! estimated, and a panic of kakemono aborts the module (a `RuntimeError`):
//! load it again after one.

use kakemono_bindings as core;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(typescript_custom_section)]
const TYPES: &str = r#"
/** Options of reading and analysing a document, kept for every output. */
export interface OpenOptions {
  /** The name recorded as the document's source (nothing by default). */
  sourceName?: string
  /** Rebuild tables as real tables when their grid is clear (default true). */
  tables?: boolean
  /** How hard PNG pictures are compressed; the pixels are the same. */
  pngCompression?: 'fast' | 'balanced' | 'high'
  /** An edited copy of the output of `toJson()` (its text, or the parsed object): every output shows its text. */
  content?: string | object
  /** The header rows of every table of an Excel workbook, given instead of found. */
  headerRows?: number
}
type Equations = 'mathml' | 'image'
/** Options of the HTML output. */
export interface HtmlOptions {
  mode?: 'hard' | 'hard-95' | 'medium' | 'friendly'
  equations?: Equations
  /** Pixels per PDF point of the pictures, above 0.2 and at most 8 (default 3). */
  scale?: number
  pageImage?: 'svg' | 'svg-only' | 'png'
  embedFonts?: boolean
  /** Pictures as data URIs in the page (default true); otherwise as `files`. */
  inlineImages?: boolean
  imageDir?: string
}
/** Options of the Markdown output. */
export interface MarkdownOptions {
  mode?: 'medium' | 'friendly'
  equations?: Equations
  scale?: number
  /** The folder of the picture files, relative (default 'images'). */
  imageDir?: string
  inlineImages?: boolean
}
/** Options of the editable JSON output. */
export interface JsonOptions { equations?: Equations; scale?: number; imageDir?: string; inlineImages?: boolean }
/** Options of the LaTeX output. */
export interface LatexOptions { equations?: Equations; scale?: number; imageDir?: string }
/** Options of the Typst output. */
export interface TypstOptions { equations?: Equations; scale?: number; imageDir?: string; layout?: 'auto' | 'article' | 'book' }
/** Options of the Word output: with `imageDir`, its pictures are files too. */
export interface DocxOptions { equations?: Equations; scale?: number; imageDir?: string }
/** Options of the EPUB output. */
export interface EpubOptions {
  equations?: Equations
  scale?: number
  maxMathWidthEm?: number
  cover?: boolean
  /** The book's date, `CCYY-MM-DDThh:mm:ssZ` (default: a fixed date). */
  modified?: string
}
/** Options of the chunks for retrieval (tokens are estimated). */
export interface ChunkOptions { maxTokens?: number }
/** A picture file of an output, at a path relative to the output's folder. */
export interface OutputFile { path: string; data: Uint8Array }
/** A text output and its picture files. */
export interface TextOutput { text: string; files: OutputFile[] }
/** A binary output (Word document, EPUB book, PDF) and its picture files. */
export interface BinaryOutput { data: Uint8Array; files: OutputFile[] }
/** A chunk for retrieval. */
export interface Chunk {
  id: number
  /** The text to embed: the section path, a blank line, then the text. */
  text: string
  tokens: number
  title?: string
  section: string[]
  blocks: number[]
  /** The pages it comes from, from 0 (`[0]` for documents without pages). */
  pages: number[]
}
"#;

/// Logs a panic's message to `console.error` before the module aborts.
#[wasm_bindgen(start)]
fn start() {
    console_error_panic_hook::set_once();
}

/// An error of kakemono as a JavaScript `Error`, its `name` saying its kind
/// (`InvalidArgument`, `DocumentError` or `InternalError`).
fn js_error(err: &core::Error) -> JsValue {
    let error = js_sys::Error::new(err.message());
    error.set_name(match err.kind() {
        core::ErrorKind::InvalidArgument => "InvalidArgument",
        core::ErrorKind::Document => "DocumentError",
        core::ErrorKind::Internal => "InternalError",
        // No call is given a token here (stopping would abort the module).
        core::ErrorKind::Cancelled => "AbortError",
        core::ErrorKind::TimedOut => "TimeoutError",
    });
    error.into()
}

/// Options read from a JavaScript object (`undefined` or `null`: the
/// defaults), through JSON, so that an unknown option is refused as in the
/// other bindings.
fn options<T: Default + serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
    if value.is_undefined() || value.is_null() {
        return Ok(T::default());
    }
    let invalid = |message: String| js_error(&core::Error::invalid(message));
    let json = js_sys::JSON::stringify(&value)
        .map_err(|_| invalid("invalid options: they cannot be written as JSON".into()))?;
    serde_json::from_str(&String::from(json)).map_err(|err| invalid(format!("invalid options: {err}")))
}

/// `opts` with its `content` as JSON text when it is an object (a copy:
/// the caller's object is not changed).
fn with_content_as_text(opts: JsValue) -> Result<JsValue, JsValue> {
    if !opts.is_object() {
        return Ok(opts);
    }
    let content = js_sys::Reflect::get(&opts, &JsValue::from_str("content"))?;
    if !content.is_object() {
        return Ok(opts);
    }
    let copy = js_sys::Object::assign(&js_sys::Object::new(), opts.unchecked_ref());
    let text = js_sys::JSON::stringify(&content)?;
    set(&copy, "content", &text);
    Ok(copy.into())
}

/// Sets `object[key] = value`.
fn set(object: &js_sys::Object, key: &str, value: &JsValue) {
    // Setting a property of a plain new object cannot fail.
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), value);
}

/// The bytes `data` as a new `Uint8Array`.
fn bytes(data: &[u8]) -> JsValue {
    js_sys::Uint8Array::from(data).into()
}

/// An output as `{ text | data, files }`.
fn output(out: core::Output) -> JsValue {
    let object = js_sys::Object::new();
    if let Some(text) = &out.text {
        set(&object, "text", &JsValue::from_str(text));
    }
    if let Some(data) = &out.data {
        set(&object, "data", &bytes(data));
    }
    let files = js_sys::Array::new();
    for file in &out.files {
        let entry = js_sys::Object::new();
        set(&entry, "path", &JsValue::from_str(&file.path));
        set(&entry, "data", &bytes(&file.data));
        files.push(&entry);
    }
    set(&object, "files", &files);
    object.into()
}

/// An array of numbers.
fn numbers(values: &[usize]) -> js_sys::Array {
    values.iter().map(|value| JsValue::from_f64(*value as f64)).collect()
}

/// A PDF, Word document, Excel workbook, EPUB book or web page, read and
/// analysed once: every output written from it shares that analysis.
#[wasm_bindgen]
pub struct Document {
    /// The document.
    doc: core::Document,
}

#[wasm_bindgen]
impl Document {
    /// Reads and analyses the bytes of a document (a PDF, a Word document,
    /// an Excel workbook, an EPUB book or a web page).
    #[wasm_bindgen(js_name = fromBytes)]
    pub fn from_bytes(
        data: &[u8],
        #[wasm_bindgen(unchecked_param_type = "OpenOptions")] opts: JsValue,
    ) -> Result<Document, JsValue> {
        let mut opts: core::OpenOptions = options(with_content_as_text(opts)?)?;
        // One thread: WebAssembly here has no others.
        opts.jobs = Some(1);
        if opts.content_dir.is_some() {
            return Err(js_error(&core::Error::invalid(
                "contentDir needs files: there are none in WebAssembly",
            )));
        }
        core::Document::open(core::Source::Data(data.to_vec()), &opts)
            .map(|doc| Document { doc })
            .map_err(|err| js_error(&err))
    }

    /// The input format: `'pdf'`, `'docx'`, `'doc'`, `'xlsx'`, `'epub'` or
    /// `'html'`.
    #[wasm_bindgen(getter)]
    pub fn format(&self) -> String {
        self.doc.format().to_string()
    }

    /// The number of pages of a PDF (0 for the formats that have none).
    #[wasm_bindgen(getter, js_name = pageCount)]
    pub fn page_count(&self) -> usize {
        self.doc.page_count()
    }

    /// The title of the document, when it has one.
    #[wasm_bindgen(getter)]
    pub fn title(&self) -> Option<String> {
        self.doc.title().map(str::to_string)
    }

    /// The warnings of applying `content`: blocks that kept their own text.
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.doc.warnings().to_vec()
    }

    /// Writes the document as `target` with the options `opts`.
    fn render(&self, target: core::Target, opts: JsValue) -> Result<JsValue, JsValue> {
        let opts: core::RenderOptions = options(opts)?;
        self.doc.render(target, &opts).map(output).map_err(|err| js_error(&err))
    }

    /// An HTML page; its pictures are inline unless `inlineImages` is false.
    #[wasm_bindgen(js_name = toHtml, unchecked_return_type = "TextOutput")]
    pub fn to_html(
        &self,
        #[wasm_bindgen(unchecked_param_type = "HtmlOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Html, opts)
    }

    /// Markdown and its picture files.
    #[wasm_bindgen(js_name = toMarkdown, unchecked_return_type = "TextOutput")]
    pub fn to_markdown(
        &self,
        #[wasm_bindgen(unchecked_param_type = "MarkdownOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Markdown, opts)
    }

    /// The editable JSON and its picture files.
    #[wasm_bindgen(js_name = toJson, unchecked_return_type = "TextOutput")]
    pub fn to_json(
        &self,
        #[wasm_bindgen(unchecked_param_type = "JsonOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Json, opts)
    }

    /// LaTeX source and its picture files.
    #[wasm_bindgen(js_name = toLatex, unchecked_return_type = "TextOutput")]
    pub fn to_latex(
        &self,
        #[wasm_bindgen(unchecked_param_type = "LatexOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Latex, opts)
    }

    /// Typst source and its picture files.
    #[wasm_bindgen(js_name = toTypst, unchecked_return_type = "TextOutput")]
    pub fn to_typst(
        &self,
        #[wasm_bindgen(unchecked_param_type = "TypstOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Typst, opts)
    }

    /// A Word document (`.docx`).
    #[wasm_bindgen(js_name = toDocx, unchecked_return_type = "BinaryOutput")]
    pub fn to_docx(
        &self,
        #[wasm_bindgen(unchecked_param_type = "DocxOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Docx, opts)
    }

    /// An EPUB 3 book.
    #[wasm_bindgen(js_name = toEpub, unchecked_return_type = "BinaryOutput")]
    pub fn to_epub(
        &self,
        #[wasm_bindgen(unchecked_param_type = "EpubOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        self.render(core::Target::Epub, opts)
    }

    /// A Word document, workbook or EPUB book laid out in pages, as a PDF.
    #[wasm_bindgen(js_name = toPdf, unchecked_return_type = "BinaryOutput")]
    pub fn to_pdf(&self) -> Result<JsValue, JsValue> {
        self.render(core::Target::Pdf, JsValue::UNDEFINED)
    }

    /// Chunks for retrieval (RAG), cut at sentence ends and headed by their
    /// section.
    #[wasm_bindgen(js_name = toChunks, unchecked_return_type = "Chunk[]")]
    pub fn to_chunks(
        &self,
        #[wasm_bindgen(unchecked_param_type = "ChunkOptions")] opts: JsValue,
    ) -> Result<JsValue, JsValue> {
        let opts: core::RenderOptions = options(opts)?;
        let chunks = self.doc.chunks(&opts).map_err(|err| js_error(&err))?;
        let array = js_sys::Array::new();
        for chunk in chunks {
            let object = js_sys::Object::new();
            set(&object, "id", &JsValue::from_f64(chunk.id as f64));
            set(&object, "text", &JsValue::from_str(&chunk.text));
            set(&object, "tokens", &JsValue::from_f64(chunk.tokens as f64));
            if let Some(title) = &chunk.title {
                set(&object, "title", &JsValue::from_str(title));
            }
            let section: js_sys::Array = chunk.section.iter().map(|part| JsValue::from_str(part)).collect();
            set(&object, "section", &section);
            set(&object, "blocks", &numbers(&chunk.blocks));
            set(&object, "pages", &numbers(&chunk.pages));
            array.push(&object);
        }
        Ok(array.into())
    }

    /// The structure of the document (its blocks in reading order), as the
    /// command line's `--json` writes it.
    #[wasm_bindgen(unchecked_return_type = "any")]
    pub fn analyze(&self) -> Result<JsValue, JsValue> {
        let out = self
            .doc
            .render(core::Target::Structure, &core::RenderOptions::default())
            .map_err(|err| js_error(&err))?;
        js_sys::JSON::parse(&out.text.unwrap_or_default())
    }
}

/// Adds a TrueType or OpenType font (or a collection) to those documents
/// are laid out and SVG text drawn with: WebAssembly has no installed
/// fonts, and laying out a Word document or a workbook as a PDF needs one.
/// Returns its number of faces.
#[wasm_bindgen(js_name = addFont)]
pub fn add_font(data: &[u8]) -> Result<usize, JsValue> {
    core::add_font(data.to_vec()).map_err(|err| js_error(&err))
}

/// The version of kakemono.
#[wasm_bindgen]
pub fn version() -> String {
    core::version().to_string()
}
