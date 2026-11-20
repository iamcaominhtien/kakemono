//! The native part of the `kakemono` Python package: PDF or Word bytes in, HTML, a
//! Word document, Markdown, LaTeX, an EPUB book, the editable JSON, chunks for retrieval or the document structure (as JSON) out.
//! Every output can show the text of an edited JSON file (`content`). `kakemono/__init__.py` wraps it with
//! path and arXiv handling.

use kakemono::source::arxiv;
use kakemono::{
    AnalyzeOptions, ChunkOptions, DocxOptions, EpubOptions, Equations, HtmlOptions, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, PageImage, Paper, PngCompression, TokenCounter,
};
use pyo3::exceptions::{PyUserWarning, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;

/// Turns any error of the Rust library into a Python `ValueError`.
fn value_error(err: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(format!("{err:#}"))
}

/// Parses a mode name as on the command line.
fn parse_mode(name: &str) -> PyResult<Mode> {
    match name {
        "hard" => Ok(Mode::Hard),
        "hard-95" | "hard95" => Ok(Mode::Hard95),
        "medium" => Ok(Mode::Medium),
        "friendly" => Ok(Mode::Friendly),
        other => Err(value_error(format!(
            "unknown mode '{other}' (expected 'hard', 'hard-95', 'medium' or 'friendly')"
        ))),
    }
}

/// Parses a page-image name as on the command line.
fn parse_page_image(name: &str) -> PyResult<PageImage> {
    match name {
        "svg" => Ok(PageImage::Svg),
        "svg-only" => Ok(PageImage::SvgOnly),
        "png" => Ok(PageImage::Png),
        other => Err(value_error(format!(
            "unknown page_image '{other}' (expected 'svg', 'svg-only' or 'png')"
        ))),
    }
}

/// Parses an equations name as on the command line.
fn parse_equations(name: &str) -> PyResult<Equations> {
    match name {
        "mathml" => Ok(Equations::MathMl),
        "image" => Ok(Equations::Image),
        other => Err(value_error(format!(
            "unknown equations '{other}' (expected 'mathml' or 'image')"
        ))),
    }
}

/// Parses a PNG compression level as on the command line.
fn parse_png_compression(name: &str) -> PyResult<PngCompression> {
    match name {
        "fast" => Ok(PngCompression::Fast),
        "balanced" => Ok(PngCompression::Balanced),
        "high" => Ok(PngCompression::High),
        other => Err(value_error(format!(
            "unknown png_compression '{other}' (expected 'fast', 'balanced' or 'high')"
        ))),
    }
}

/// Converts a PDF or a Word document (`.docx`, `.doc`) to HTML in each of
/// `modes`; returns one HTML string per mode. A Word document has only the
/// friendly mode.
#[pyfunction]
#[pyo3(signature = (pdf, modes, *, source=None, tables=true, equations="mathml", page_image="svg", embed_fonts=true, scale=3.0, jobs=0, png_compression="balanced", content=None))]
#[allow(clippy::too_many_arguments)]
fn convert(
    py: Python<'_>,
    pdf: Vec<u8>,
    modes: Vec<String>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    page_image: &str,
    embed_fonts: bool,
    scale: f32,
    jobs: usize,
    png_compression: &str,
    content: Option<String>,
) -> PyResult<Vec<String>> {
    let modes = modes
        .iter()
        .map(|name| parse_mode(name))
        .collect::<PyResult<Vec<_>>>()?;
    let equations = parse_equations(equations)?;
    let png = parse_png_compression(png_compression)?;
    let page_image = parse_page_image(page_image)?;
    check_scale(scale)?;
    if content.is_some() && modes.contains(&Mode::Hard) {
        return Err(value_error(
            "hard mode shows the pages of the PDF: it cannot show edited text from content (hard-95 can)",
        ));
    }
    let (pages, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, png, content)?;
        if !paper.format().has_pages() && modes.iter().any(|mode| *mode != Mode::Friendly) {
            return Err(value_error(
                "a Word document has no pages: hard, hard-95 and medium show the pages of a PDF; use 'friendly'",
            ));
        }
        let pages = modes
            .into_iter()
            .map(|mode| {
                let opts = HtmlOptions {
                    mode,
                    scale,
                    page_image,
                    jobs,
                    embed_fonts,
                    equations,
                    ..Default::default()
                };
                paper.to_html(&analysis, &opts).html
            })
            .collect();
        Ok::<_, PyErr>((pages, warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok(pages)
}

/// Picture files as Python values: (relative path, bytes).
type PyFiles<'py> = Vec<(String, Bound<'py, PyBytes>)>;

/// Loads and analyses a PDF or Word document, without the GIL being needed, and applies the
/// edited JSON `content` to the analysis. Returns the warnings of applying
/// it (blocks that kept their text), to be given to Python with
/// [`warn_all`] once the GIL is held again.
fn load(
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    jobs: usize,
    png: PngCompression,
    content: Option<String>,
) -> PyResult<(Paper, kakemono::Analysis, Vec<String>)> {
    let mut paper = Paper::from_bytes(pdf).map_err(value_error)?.with_png_compression(png);
    if let Some(source) = source {
        paper = paper.with_source(source);
    }
    let mut analysis = paper.analyze_with(AnalyzeOptions {
        tables,
        jobs,
        ..Default::default()
    });
    let warnings = match content {
        Some(json) => {
            paper
                .apply_content(&mut analysis, &json)
                .map_err(|err| value_error(format!("{err:#}")))?
                .warnings
        }
        None => Vec::new(),
    };
    Ok((paper, analysis, warnings))
}

/// Gives each warning to Python as a `UserWarning`.
fn warn_all(py: Python<'_>, warnings: &[String]) -> PyResult<()> {
    let category = py.get_type::<PyUserWarning>();
    for warning in warnings {
        let message = std::ffi::CString::new(warning.replace('\0', " ")).map_err(value_error)?;
        PyErr::warn(py, &category, &message, 2)?;
    }
    Ok(())
}

/// Checks the `scale` option.
fn check_scale(scale: f32) -> PyResult<()> {
    if scale > 0.2 && scale <= 8.0 {
        Ok(())
    } else {
        Err(value_error("scale must be between 0.2 and 8"))
    }
}

/// Converts a PDF or Word document to a Word document; returns the bytes of the `.docx` file
/// and, when `image_dir` is given, its pictures as files in that folder.
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, png_compression="balanced", image_dir=None, content=None))]
#[allow(clippy::too_many_arguments)]
fn to_docx<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    png_compression: &str,
    image_dir: Option<String>,
    content: Option<String>,
) -> PyResult<(Bound<'py, PyBytes>, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    let png = parse_png_compression(png_compression)?;
    check_scale(scale)?;
    let (data, files, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, png, content)?;
        let opts = DocxOptions { scale, jobs, equations };
        let (data, files) = match image_dir {
            Some(dir) => {
                let out = paper.to_docx_with_images(&analysis, &opts, &dir);
                (out.docx, out.files)
            }
            None => (paper.to_docx(&analysis, &opts), Vec::new()),
        };
        Ok::<_, PyErr>((data, files, warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok((PyBytes::new(py, &data), py_files(py, files)))
}

/// Converts a PDF or Word document to Markdown; returns the text and its picture files
/// (none when `inline_images`: the pictures are then data URIs).
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, png_compression="balanced", image_dir="images".to_string(), inline_images=false, content=None))]
#[allow(clippy::too_many_arguments)]
fn to_markdown<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    png_compression: &str,
    image_dir: String,
    inline_images: bool,
    content: Option<String>,
) -> PyResult<(String, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    let png = parse_png_compression(png_compression)?;
    check_scale(scale)?;
    let (out, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, png, content)?;
        let opts = MarkdownOptions {
            inline_images,
            asset_dir: image_dir,
            scale,
            jobs,
            equations,
        };
        Ok::<_, PyErr>((paper.to_markdown(&analysis, &opts), warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok((out.markdown, py_files(py, out.files)))
}

/// Converts a PDF or Word document to the editable JSON; returns the text and its picture
/// files (none when `inline_images`: the pictures are then data URIs).
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, png_compression="balanced", image_dir="images".to_string(), inline_images=false, content=None))]
#[allow(clippy::too_many_arguments)]
fn to_json<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    png_compression: &str,
    image_dir: String,
    inline_images: bool,
    content: Option<String>,
) -> PyResult<(String, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    let png = parse_png_compression(png_compression)?;
    check_scale(scale)?;
    let (out, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, png, content)?;
        let opts = JsonOptions {
            inline_images,
            asset_dir: image_dir,
            scale,
            jobs,
            equations,
        };
        Ok::<_, PyErr>((paper.to_json(&analysis, &opts), warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok((out.json, py_files(py, out.files)))
}

/// Converts a PDF or Word document to LaTeX; returns the source of the `.tex` file and its
/// picture files (in `image_dir`).
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, png_compression="balanced", image_dir="images".to_string(), content=None))]
#[allow(clippy::too_many_arguments)]
fn to_latex<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    png_compression: &str,
    image_dir: String,
    content: Option<String>,
) -> PyResult<(String, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    let png = parse_png_compression(png_compression)?;
    check_scale(scale)?;
    let (out, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, png, content)?;
        let opts = LatexOptions {
            asset_dir: image_dir,
            scale,
            jobs,
            equations,
        };
        Ok::<_, PyErr>((paper.to_latex(&analysis, &opts), warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok((out.latex, py_files(py, out.files)))
}

/// Converts a PDF or Word document to an EPUB book; returns the bytes of the `.epub` file.
/// `modified` is its date (`CCYY-MM-DDThh:mm:ssZ`), fixed when `None`.
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, png_compression="balanced", max_math_width_em=20.0, cover=true, modified=None, content=None))]
#[allow(clippy::too_many_arguments)]
fn to_epub<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    png_compression: &str,
    max_math_width_em: f32,
    cover: bool,
    modified: Option<String>,
    content: Option<String>,
) -> PyResult<Bound<'py, PyBytes>> {
    let equations = parse_equations(equations)?;
    let png = parse_png_compression(png_compression)?;
    check_scale(scale)?;
    let (data, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, png, content)?;
        let opts = EpubOptions {
            scale,
            jobs,
            equations,
            max_math_width_em,
            cover,
            modified,
        };
        Ok::<_, PyErr>((paper.to_epub(&analysis, &opts), warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok(PyBytes::new(py, &data))
}

/// Picture files as Python values.
fn py_files(py: Python<'_>, files: Vec<(String, Vec<u8>)>) -> PyFiles<'_> {
    files
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new(py, &data)))
        .collect()
}

/// Cuts a PDF or Word document into chunks for retrieval; returns them as JSON Lines (one
/// object per line). `tokenizer` is the path of a Hugging Face
/// `tokenizer.json` or the name of an OpenAI tokenizer; without it, tokens
/// are estimated.
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, max_tokens=512, tokenizer=None, tables=true, jobs=0, content=None))]
#[allow(clippy::too_many_arguments)]
fn to_chunks(
    py: Python<'_>,
    pdf: Vec<u8>,
    source: Option<String>,
    max_tokens: usize,
    tokenizer: Option<String>,
    tables: bool,
    jobs: usize,
    content: Option<String>,
) -> PyResult<String> {
    let counter = match tokenizer {
        Some(spec) => TokenCounter::load(&spec).map_err(|err| value_error(format!("{err:#}")))?,
        None => TokenCounter::estimate(),
    };
    let (jsonl, warnings) = py.detach(move || {
        let (paper, analysis, warnings) = load(pdf, source, tables, jobs, PngCompression::default(), content)?;
        let chunks = paper.to_chunks(&analysis, &ChunkOptions { max_tokens, counter });
        Ok::<_, PyErr>((kakemono::chunks_jsonl(&chunks), warnings))
    })?;
    warn_all(py, &warnings)?;
    Ok(jsonl)
}

/// Analyses a PDF or Word document and returns its structure (blocks in reading order) as a
/// JSON string, the same document as `kakemono convert --json`.
#[pyfunction]
#[pyo3(signature = (pdf, *, tables=true, jobs=0))]
fn analyze(py: Python<'_>, pdf: Vec<u8>, tables: bool, jobs: usize) -> PyResult<String> {
    py.detach(move || {
        let paper = Paper::from_bytes(pdf).map_err(value_error)?;
        let analysis = paper.analyze_with(AnalyzeOptions {
            tables,
            jobs,
            ..Default::default()
        });
        serde_json::to_string(&analysis.doc).map_err(value_error)
    })
}

/// The arXiv identifier in `text` (an id or an arXiv URL), or `None`.
#[pyfunction]
fn parse_arxiv_id(text: &str) -> Option<String> {
    arxiv::parse_id(text)
}

/// Downloads the PDF of an arXiv paper, through the on-disk cache.
#[pyfunction]
fn fetch_arxiv<'py>(py: Python<'py>, id: &str) -> PyResult<Bound<'py, PyBytes>> {
    let id = arxiv::parse_id(id).ok_or_else(|| value_error(format!("'{id}' is not an arXiv id or URL")))?;
    let data = py.detach(|| arxiv::fetch_pdf(&id)).map_err(value_error)?;
    Ok(PyBytes::new(py, &data))
}

/// The native module, imported by `kakemono/__init__.py`.
#[pymodule]
fn _kakemono(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(convert, module)?)?;
    module.add_function(wrap_pyfunction!(to_docx, module)?)?;
    module.add_function(wrap_pyfunction!(to_markdown, module)?)?;
    module.add_function(wrap_pyfunction!(to_latex, module)?)?;
    module.add_function(wrap_pyfunction!(to_json, module)?)?;
    module.add_function(wrap_pyfunction!(to_epub, module)?)?;
    module.add_function(wrap_pyfunction!(to_chunks, module)?)?;
    module.add_function(wrap_pyfunction!(analyze, module)?)?;
    module.add_function(wrap_pyfunction!(parse_arxiv_id, module)?)?;
    module.add_function(wrap_pyfunction!(fetch_arxiv, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
