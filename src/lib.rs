//! kakemono: convert scientific-paper PDFs (such as those on arXiv), Word
//! documents (`.docx`, `.doc`), Excel workbooks (`.xlsx`), EPUB books
//! (`.epub`) and web pages (an `.html` file or an `http(s)` address:
//! [`Paper::from_path`], [`Paper::from_url`]) into clean HTML, Word
//! documents, Markdown, LaTeX, Typst or EPUB books.
//!
//! The pipeline is: PDF access -> layout analysis and structuring ->
//! structured document ([`model`]) -> HTML in one of four fidelity modes
//! ([`Mode`]), a Word document ([`Paper::to_docx`]), Markdown
//! ([`Paper::to_markdown`]), LaTeX ([`Paper::to_latex`]), Typst
//! ([`Paper::to_typst`]) or an EPUB book ([`Paper::to_epub`]). See
//! `ARCHITECTURE.md` in the repository.
//!
//! The entry points are [`Paper`] (load, analyse and render step by step,
//! e.g. to produce several modes from one analysis) and [`convert`] (all in
//! one call with default options). Analysis thresholds live in
//! [`LayoutConfig`], reachable through [`AnalyzeOptions`].
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

// The stable, documented API: these modules and the items re-exported below.
pub mod config;
pub mod model;
pub mod source;

// The stages of the pipeline. They are public for the `kakemono` executable,
// the examples and the tests, but they are internals: hidden from the
// documentation and free to change in any release.
#[doc(hidden)]
pub mod analysis;
#[doc(hidden)]
pub mod backend;
// Charts of workbooks and Word documents: read, drawn and made ready here,
// placed in the documents by the Excel and Word readers.
pub(crate) mod chart;
#[doc(hidden)]
pub mod common;
#[doc(hidden)]
pub mod epub;
#[doc(hidden)]
pub mod excel;
#[doc(hidden)]
pub mod html;
#[doc(hidden)]
pub mod layout;
#[doc(hidden)]
pub mod paper;
#[doc(hidden)]
pub mod powerpoint;
#[doc(hidden)]
pub mod raster;
#[doc(hidden)]
pub mod render;
#[doc(hidden)]
pub mod word;

pub use analysis::pipeline::Analysis;
pub use backend::{PageRenderer, PngCompression};
pub use config::{AnalyzeOptions, LayoutConfig};
pub use paper::{InputFormat, Paper, convert};
pub use render::{
    Chunk, ChunkOptions, ContentReport, DocxOptions, DocxOutput, EpubOptions, Equations, HtmlOptions, HtmlOutput,
    JsonOptions, JsonOutput, LatexOptions, LatexOutput, MarkdownOptions, MarkdownOutput, Mode, NAMED_TOKENIZERS,
    PageImage, TokenCounter, TypstLayout, TypstOptions, TypstOutput, chunks_jsonl,
};
