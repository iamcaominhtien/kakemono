//! kakemono: convert scientific-paper PDFs (such as those on arXiv) into
//! clean HTML, or into Word documents.
//!
//! The pipeline is: PDF access -> layout analysis and structuring ->
//! structured document ([`model`]) -> HTML in one of four fidelity modes
//! ([`Mode`]), or a Word document ([`Paper::to_docx`]). See
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
#[doc(hidden)]
pub mod common;
#[doc(hidden)]
pub mod paper;
#[doc(hidden)]
pub mod raster;
#[doc(hidden)]
pub mod render;

pub use analysis::pipeline::Analysis;
pub use backend::PageRenderer;
pub use config::{AnalyzeOptions, LayoutConfig};
pub use paper::{Paper, convert};
pub use render::{DocxOptions, Equations, HtmlOptions, HtmlOutput, Mode, PageImage};
