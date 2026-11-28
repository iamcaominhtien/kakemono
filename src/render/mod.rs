//! HTML output in four fidelity modes (see [`Mode`]), Word output
//! ([`render_docx`]), Markdown output ([`render_markdown`]), LaTeX output
//! ([`render_latex`]), Typst output ([`render_typst`]), EPUB output
//! ([`render_epub`]) and JSON output
//! (`render_json`), which can also be edited and applied back
//! (`apply_content`), and chunks for retrieval ([`render_chunks`]).

mod assets;
mod chunks;
mod code;
mod docx;
mod epub;
pub(crate) mod escape;
mod flow;
mod formula;
mod friendly;
mod hard;
mod hard95;
mod inline_math;
mod json;
mod latex;
mod markdown;
pub(crate) mod mathml;
mod medium;
mod notes;
mod options;
mod runs;
mod sequence;
mod table;
mod typst;
pub(crate) mod zip;

#[cfg(test)]
pub(crate) use docx::inline_math as docx_omml_for_tests;

pub(crate) use assets::{ImageStore, Pictures};
pub use chunks::{
    Chunk, ChunkOptions, NAMED_TOKENIZERS, TokenCounter, chunks_jsonl, estimate as estimate_tokens, render_chunks,
};
pub use docx::{DocxOptions, DocxOutput, render_docx, render_docx_with_images};
pub use epub::{EpubOptions, render_epub};
pub(crate) use escape::*;
pub(crate) use flow::*;
pub use json::apply::ContentReport;
pub(crate) use json::apply::apply_content;
pub(crate) use json::figure_lines::figure_svg;
pub use json::{JsonOptions, JsonOutput};
pub(crate) use json::{SourceInfo, render_json};
pub use latex::{LatexOptions, LatexOutput, render_latex};
pub use markdown::{MarkdownOptions, MarkdownOutput, render_markdown};
pub(crate) use mathml::math_css;
pub use mathml::math_html;
pub use options::{Equations, HtmlOptions, HtmlOutput, Mode, PageImage};
pub(crate) use table::*;
pub use typst::{TypstLayout, TypstOptions, TypstOutput, render_typst};

use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::model::*;

/// One output mode: turns an analysed document into a complete HTML page.
pub(crate) trait Renderer {
    /// Writes the HTML. Pictures (page backgrounds, figures, equations) are
    /// requested from `images`, which returns their `src` and draws them
    /// afterwards.
    fn render(&self, analysis: &Analysis, images: &mut ImageStore<'_>) -> String;
}

/// One `@font-face` rule with the font `data` (woff2) embedded as a base64
/// data URI.
pub(crate) fn font_face(family: &str, style: &str, weight: &str, data: &[u8]) -> String {
    use base64::Engine;
    format!(
        "@font-face{{font-family:\"{family}\";font-style:{style};font-weight:{weight};font-display:swap;\
src:url(data:font/woff2;base64,{}) format(\"woff2\")}}\n",
        base64::engine::general_purpose::STANDARD.encode(data)
    )
}

/// The renderer of a mode: one implementation of [`Renderer`] per [`Mode`].
fn renderer_for(mode: Mode) -> &'static dyn Renderer {
    match mode {
        Mode::Friendly => &friendly::Friendly,
        Mode::Medium => &medium::Medium,
        Mode::Hard => &hard::Hard,
        Mode::Hard95 => &hard95::Hard95,
    }
}

/// Renders an analysed document as HTML in the mode chosen in `opts`.
/// Added notes are numbered in reading order across the document.
///
/// The HTML is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and either
/// inlined as data URIs or returned in [`HtmlOutput::files`].
pub fn render(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &HtmlOptions) -> HtmlOutput {
    let mut images = ImageStore {
        opts,
        jobs: Vec::new(),
        svg_as_pdf: false,
    };
    let numbered = notes::numbered_analysis(analysis);
    let html = renderer_for(opts.mode).render(numbered.as_ref().unwrap_or(analysis), &mut images);
    images.finish(Pictures::new(pdf, &analysis.doc), html)
}

/// Renders an analysed document as HTML, like [`render`], but writes it to
/// `out` while the pictures are drawn, a few at a time, so that neither the
/// whole page nor all its pictures are held in memory. When pictures are
/// not inlined, each picture file goes to `file` as (relative path,
/// contents) instead.
///
/// # Errors
/// The first error of `out` or `file`.
pub fn render_to(
    analysis: &Analysis,
    pdf: Option<&PdfFile>,
    opts: &HtmlOptions,
    out: &mut dyn std::io::Write,
    file: &mut dyn FnMut(String, Vec<u8>) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let mut images = ImageStore {
        opts,
        jobs: Vec::new(),
        svg_as_pdf: false,
    };
    let numbered = notes::numbered_analysis(analysis);
    let html = renderer_for(opts.mode).render(numbered.as_ref().unwrap_or(analysis), &mut images);
    images.finish_into(Pictures::new(pdf, &analysis.doc), &html, out, file)
}
