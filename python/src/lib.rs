//! The native part of the `kakemono` Python package: PDF bytes in, HTML, a
//! Word document, Markdown, LaTeX, an EPUB book or the document structure (as JSON) out. `kakemono/__init__.py` wraps it with
//! path and arXiv handling.

use kakemono::source::arxiv;
use kakemono::{
    AnalyzeOptions, DocxOptions, EpubOptions, Equations, HtmlOptions, LatexOptions, MarkdownOptions, Mode, PageImage,
    Paper,
};
use pyo3::exceptions::PyValueError;
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

/// Converts a PDF to HTML in each of `modes`; returns one HTML string per mode.
#[pyfunction]
#[pyo3(signature = (pdf, modes, *, source=None, tables=true, equations="mathml", page_image="svg", embed_fonts=true, scale=3.0, jobs=0))]
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
) -> PyResult<Vec<String>> {
    let modes = modes
        .iter()
        .map(|name| parse_mode(name))
        .collect::<PyResult<Vec<_>>>()?;
    let equations = parse_equations(equations)?;
    let page_image = parse_page_image(page_image)?;
    if !(scale > 0.2 && scale <= 8.0) {
        return Err(value_error("scale must be between 0.2 and 8"));
    }
    py.detach(move || {
        let mut paper = Paper::from_bytes(pdf).map_err(value_error)?;
        if let Some(source) = source {
            paper = paper.with_source(source);
        }
        let analysis = paper.analyze_with(AnalyzeOptions {
            tables,
            jobs,
            ..Default::default()
        });
        Ok(modes
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
            .collect())
    })
}

/// Picture files as Python values: (relative path, bytes).
type PyFiles<'py> = Vec<(String, Bound<'py, PyBytes>)>;

/// Loads and analyses a PDF, without the GIL being needed.
fn load(pdf: Vec<u8>, source: Option<String>, tables: bool, jobs: usize) -> PyResult<(Paper, kakemono::Analysis)> {
    let mut paper = Paper::from_bytes(pdf).map_err(value_error)?;
    if let Some(source) = source {
        paper = paper.with_source(source);
    }
    let analysis = paper.analyze_with(AnalyzeOptions {
        tables,
        jobs,
        ..Default::default()
    });
    Ok((paper, analysis))
}

/// Checks the `scale` option.
fn check_scale(scale: f32) -> PyResult<()> {
    if scale > 0.2 && scale <= 8.0 {
        Ok(())
    } else {
        Err(value_error("scale must be between 0.2 and 8"))
    }
}

/// Converts a PDF to a Word document; returns the bytes of the `.docx` file
/// and, when `image_dir` is given, its pictures as files in that folder.
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, image_dir=None))]
#[allow(clippy::too_many_arguments)]
fn to_docx<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    image_dir: Option<String>,
) -> PyResult<(Bound<'py, PyBytes>, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    check_scale(scale)?;
    let (data, files) = py.detach(move || {
        let (paper, analysis) = load(pdf, source, tables, jobs)?;
        let opts = DocxOptions { scale, jobs, equations };
        Ok::<_, PyErr>(match image_dir {
            Some(dir) => {
                let out = paper.to_docx_with_images(&analysis, &opts, &dir);
                (out.docx, out.files)
            }
            None => (paper.to_docx(&analysis, &opts), Vec::new()),
        })
    })?;
    Ok((PyBytes::new(py, &data), py_files(py, files)))
}

/// Converts a PDF to Markdown; returns the text and its picture files
/// (none when `inline_images`: the pictures are then data URIs).
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, image_dir="images".to_string(), inline_images=false))]
#[allow(clippy::too_many_arguments)]
fn to_markdown<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    image_dir: String,
    inline_images: bool,
) -> PyResult<(String, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    check_scale(scale)?;
    let out = py.detach(move || {
        let (paper, analysis) = load(pdf, source, tables, jobs)?;
        let opts = MarkdownOptions {
            inline_images,
            asset_dir: image_dir,
            scale,
            jobs,
            equations,
        };
        Ok::<_, PyErr>(paper.to_markdown(&analysis, &opts))
    })?;
    Ok((out.markdown, py_files(py, out.files)))
}

/// Converts a PDF to LaTeX; returns the source of the `.tex` file and its
/// picture files (in `image_dir`).
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, image_dir="images".to_string()))]
#[allow(clippy::too_many_arguments)]
fn to_latex<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    image_dir: String,
) -> PyResult<(String, PyFiles<'py>)> {
    let equations = parse_equations(equations)?;
    check_scale(scale)?;
    let out = py.detach(move || {
        let (paper, analysis) = load(pdf, source, tables, jobs)?;
        let opts = LatexOptions {
            asset_dir: image_dir,
            scale,
            jobs,
            equations,
        };
        Ok::<_, PyErr>(paper.to_latex(&analysis, &opts))
    })?;
    Ok((out.latex, py_files(py, out.files)))
}

/// Converts a PDF to an EPUB book; returns the bytes of the `.epub` file.
/// `modified` is its date (`CCYY-MM-DDThh:mm:ssZ`), fixed when `None`.
#[pyfunction]
#[pyo3(signature = (pdf, *, source=None, tables=true, equations="mathml", scale=3.0, jobs=0, cover=true, modified=None))]
#[allow(clippy::too_many_arguments)]
fn to_epub<'py>(
    py: Python<'py>,
    pdf: Vec<u8>,
    source: Option<String>,
    tables: bool,
    equations: &str,
    scale: f32,
    jobs: usize,
    cover: bool,
    modified: Option<String>,
) -> PyResult<Bound<'py, PyBytes>> {
    let equations = parse_equations(equations)?;
    check_scale(scale)?;
    let data = py.detach(move || {
        let (paper, analysis) = load(pdf, source, tables, jobs)?;
        let opts = EpubOptions {
            scale,
            jobs,
            equations,
            cover,
            modified,
        };
        Ok::<_, PyErr>(paper.to_epub(&analysis, &opts))
    })?;
    Ok(PyBytes::new(py, &data))
}

/// Picture files as Python values.
fn py_files(py: Python<'_>, files: Vec<(String, Vec<u8>)>) -> PyFiles<'_> {
    files
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new(py, &data)))
        .collect()
}

/// Analyses a PDF and returns its structure (blocks in reading order) as a
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
    module.add_function(wrap_pyfunction!(to_epub, module)?)?;
    module.add_function(wrap_pyfunction!(analyze, module)?)?;
    module.add_function(wrap_pyfunction!(parse_arxiv_id, module)?)?;
    module.add_function(wrap_pyfunction!(fetch_arxiv, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
