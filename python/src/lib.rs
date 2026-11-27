//! The native part of the `kakemono` Python package. A [`Document`] is a PDF,
//! a Word document, an Excel workbook, an EPUB book or a web page read and analysed once (with the text of an edited JSON file
//! applied, `content`); it is then written as HTML, a Word document, Markdown,
//! LaTeX, an EPUB book, the editable JSON, chunks for retrieval, a PDF (Word
//! documents, workbooks) or the document structure (as JSON). [`render_svg`] and
//! [`read_formula`] are the `render-svg` and `formula` commands.
//! `kakemono/__init__.py` wraps it with path and arXiv handling.

use kakemono::config::ExcelConfig;
use kakemono::source::arxiv;
use kakemono::{
    AnalyzeOptions, ChunkOptions, DocxOptions, EpubOptions, Equations, HtmlOptions, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, PageImage, Paper, PngCompression, TokenCounter, TypstLayout, TypstOptions,
};
use pyo3::exceptions::{PyUserWarning, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use std::path::Path;

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

/// Checks the `scale` option of the conversions (pixels per PDF point).
fn check_scale(scale: f32) -> PyResult<()> {
    if scale > 0.2 && scale <= 8.0 {
        Ok(())
    } else {
        Err(value_error("scale must be between 0.2 and 8"))
    }
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

/// Picture files as Python values: (relative path, bytes).
type PyFiles<'py> = Vec<(String, Bound<'py, PyBytes>)>;

/// Picture files as Python values.
fn py_files(py: Python<'_>, files: Vec<(String, Vec<u8>)>) -> PyFiles<'_> {
    files
        .into_iter()
        .map(|(name, data)| (name, PyBytes::new(py, &data)))
        .collect()
}

/// A PDF or Word document, read and analysed once, with the edited text of
/// `content` applied: every output written from it shares that analysis.
#[pyclass(frozen, module = "kakemono._kakemono")]
struct Document {
    /// The document.
    paper: Paper,
    /// Its analysis, with `content` applied.
    analysis: kakemono::Analysis,
    /// Worker threads, for the analysis and for drawing pictures.
    jobs: usize,
    /// Whether `content` was given: a PDF of a Word document is then laid
    /// out from the edited document.
    edited: bool,
}

#[pymethods]
impl Document {
    /// Reads and analyses `data` (the bytes of a PDF, a Word document, an
    /// Excel workbook, an EPUB book or a web page), the file at `path` (a web page with its
    /// pictures next to it) or the document at the address `url` (a web
    /// page with its pictures, fetched), and
    /// applies the edited JSON `content` to the analysis; a relative
    /// `replace_image` of `content` is read from `content_dir`. The warnings
    /// of applying it (blocks that kept their text) are given as
    /// `UserWarning`s. `header_rows` gives the header rows of every table
    /// of a workbook, instead of finding them.
    #[new]
    #[pyo3(signature = (data=None, *, path=None, url=None, source=None, tables=true, jobs=0, png_compression="balanced", content=None, content_dir=None, header_rows=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        data: Option<Vec<u8>>,
        path: Option<String>,
        url: Option<String>,
        source: Option<String>,
        tables: bool,
        jobs: usize,
        png_compression: &str,
        content: Option<String>,
        content_dir: Option<String>,
        header_rows: Option<usize>,
    ) -> PyResult<Self> {
        let png = parse_png_compression(png_compression)?;
        let edited = content.is_some();
        let (paper, analysis, warnings) = py.detach(move || {
            let excel = ExcelConfig {
                header_rows,
                ..Default::default()
            };
            let paper = match (data, path, url) {
                (Some(data), None, None) => Paper::from_bytes_with(data, &excel),
                (None, Some(path), None) => Paper::from_path_with(path, &excel),
                (None, None, Some(url)) => Paper::from_url(&url),
                _ => return Err(value_error("give one of data, path and url")),
            };
            let mut paper = paper.map_err(value_error)?.with_png_compression(png);
            if let Some(source) = source {
                paper = paper.with_source(source);
            }
            let mut analysis = paper.analyze_with(AnalyzeOptions {
                tables,
                jobs,
                ..Default::default()
            });
            let warnings = match (content, content_dir) {
                (Some(json), Some(dir)) => {
                    paper
                        .apply_content_in(&mut analysis, &json, Path::new(&dir))
                        .map_err(value_error)?
                        .warnings
                }
                (Some(json), None) => paper.apply_content(&mut analysis, &json).map_err(value_error)?.warnings,
                (None, _) => Vec::new(),
            };
            Ok::<_, PyErr>((paper, analysis, warnings))
        })?;
        warn_all(py, &warnings)?;
        Ok(Self {
            paper,
            analysis,
            jobs,
            edited,
        })
    }

    /// The input format: `"pdf"`, `"docx"`, `"doc"`, `"xlsx"`, `"epub"` or `"html"`.
    #[getter]
    fn format(&self) -> &'static str {
        self.paper.format().name()
    }

    /// The number of pages of a PDF (0 for a Word document, which has none).
    #[getter]
    fn page_count(&self) -> usize {
        if self.paper.format().has_pages() {
            self.paper.page_count()
        } else {
            0
        }
    }

    /// The HTML of each of `modes`. A Word document has only the friendly
    /// mode, and the hard mode cannot show edited text.
    #[pyo3(signature = (modes, *, equations="mathml", page_image="svg", embed_fonts=true, scale=3.0))]
    fn convert(
        &self,
        py: Python<'_>,
        modes: Vec<String>,
        equations: &str,
        page_image: &str,
        embed_fonts: bool,
        scale: f32,
    ) -> PyResult<Vec<String>> {
        let modes = modes
            .iter()
            .map(|name| parse_mode(name))
            .collect::<PyResult<Vec<_>>>()?;
        let equations = parse_equations(equations)?;
        let page_image = parse_page_image(page_image)?;
        check_scale(scale)?;
        if self.edited && modes.contains(&Mode::Hard) {
            return Err(value_error(
                "hard mode shows the pages of the PDF: it cannot show edited text from content (hard-95 can)",
            ));
        }
        if !self.paper.format().has_pages() && modes.iter().any(|mode| *mode != Mode::Friendly) {
            return Err(value_error(
                "a Word document or an EPUB book has no pages: hard, hard-95 and medium show the pages of a PDF; use 'friendly'",
            ));
        }
        Ok(py.detach(|| {
            modes
                .into_iter()
                .map(|mode| {
                    let opts = HtmlOptions {
                        mode,
                        scale,
                        page_image,
                        jobs: self.jobs,
                        embed_fonts,
                        equations,
                        ..Default::default()
                    };
                    self.paper.to_html(&self.analysis, &opts).html
                })
                .collect()
        }))
    }

    /// The bytes of a Word document and, when `image_dir` is given, its
    /// pictures as files in that folder.
    #[pyo3(signature = (*, equations="mathml", scale=3.0, image_dir=None))]
    fn to_docx<'py>(
        &self,
        py: Python<'py>,
        equations: &str,
        scale: f32,
        image_dir: Option<String>,
    ) -> PyResult<(Bound<'py, PyBytes>, PyFiles<'py>)> {
        let equations = parse_equations(equations)?;
        check_scale(scale)?;
        let (data, files) = py.detach(|| {
            let opts = DocxOptions {
                scale,
                jobs: self.jobs,
                equations,
            };
            match image_dir {
                Some(dir) => {
                    let out = self.paper.to_docx_with_images(&self.analysis, &opts, &dir);
                    (out.docx, out.files)
                }
                None => (self.paper.to_docx(&self.analysis, &opts), Vec::new()),
            }
        });
        Ok((PyBytes::new(py, &data), py_files(py, files)))
    }

    /// Markdown and its picture files (none when `inline_images`: the
    /// pictures are then data URIs).
    #[pyo3(signature = (*, equations="mathml", scale=3.0, image_dir="images".to_string(), inline_images=false, mode="friendly"))]
    fn to_markdown<'py>(
        &self,
        py: Python<'py>,
        equations: &str,
        scale: f32,
        image_dir: String,
        inline_images: bool,
        mode: &str,
    ) -> PyResult<(String, PyFiles<'py>)> {
        let mode = match parse_mode(mode)? {
            mode @ (Mode::Medium | Mode::Friendly) => mode,
            _ => {
                return Err(PyValueError::new_err(
                    "Markdown has no pages: mode can be 'friendly' or 'medium'",
                ));
            }
        };
        let equations = parse_equations(equations)?;
        check_scale(scale)?;
        let out = py.detach(|| {
            let opts = MarkdownOptions {
                inline_images,
                asset_dir: image_dir,
                scale,
                jobs: self.jobs,
                equations,
                mode,
            };
            self.paper.to_markdown(&self.analysis, &opts)
        });
        Ok((out.markdown, py_files(py, out.files)))
    }

    /// The editable JSON and its picture files (none when `inline_images`:
    /// the pictures are then data URIs).
    #[pyo3(signature = (*, equations="mathml", scale=3.0, image_dir="images".to_string(), inline_images=false))]
    fn to_json<'py>(
        &self,
        py: Python<'py>,
        equations: &str,
        scale: f32,
        image_dir: String,
        inline_images: bool,
    ) -> PyResult<(String, PyFiles<'py>)> {
        let equations = parse_equations(equations)?;
        check_scale(scale)?;
        let out = py.detach(|| {
            let opts = JsonOptions {
                inline_images,
                asset_dir: image_dir,
                scale,
                jobs: self.jobs,
                equations,
            };
            self.paper.to_json(&self.analysis, &opts)
        });
        Ok((out.json, py_files(py, out.files)))
    }

    /// The source of a LaTeX document and its picture files (in `image_dir`).
    #[pyo3(signature = (*, equations="mathml", scale=3.0, image_dir="images".to_string()))]
    fn to_latex<'py>(
        &self,
        py: Python<'py>,
        equations: &str,
        scale: f32,
        image_dir: String,
    ) -> PyResult<(String, PyFiles<'py>)> {
        let equations = parse_equations(equations)?;
        check_scale(scale)?;
        let out = py.detach(|| {
            let opts = LatexOptions {
                asset_dir: image_dir,
                scale,
                jobs: self.jobs,
                equations,
            };
            self.paper.to_latex(&self.analysis, &opts)
        });
        Ok((out.latex, py_files(py, out.files)))
    }

    /// The source of a Typst document and its picture files (in
    /// `image_dir`). `layout` is `"auto"`, `"article"` or `"book"`.
    #[pyo3(signature = (*, equations="mathml", scale=3.0, image_dir="images".to_string(), layout="auto"))]
    fn to_typst<'py>(
        &self,
        py: Python<'py>,
        equations: &str,
        scale: f32,
        image_dir: String,
        layout: &str,
    ) -> PyResult<(String, PyFiles<'py>)> {
        let equations = parse_equations(equations)?;
        check_scale(scale)?;
        let layout = match layout {
            "auto" => None,
            "article" => Some(TypstLayout::Article),
            "book" => Some(TypstLayout::Book),
            other => {
                return Err(PyValueError::new_err(format!(
                    "layout must be \"auto\", \"article\" or \"book\", not {other:?}"
                )));
            }
        };
        let out = py.detach(|| {
            let opts = TypstOptions {
                asset_dir: image_dir,
                scale,
                jobs: self.jobs,
                equations,
                layout,
            };
            self.paper.to_typst(&self.analysis, &opts)
        });
        Ok((out.typst, py_files(py, out.files)))
    }

    /// The bytes of an EPUB book. `modified` is its date
    /// (`CCYY-MM-DDThh:mm:ssZ`), fixed when `None`.
    #[pyo3(signature = (*, equations="mathml", scale=3.0, max_math_width_em=20.0, cover=true, modified=None))]
    fn to_epub<'py>(
        &self,
        py: Python<'py>,
        equations: &str,
        scale: f32,
        max_math_width_em: f32,
        cover: bool,
        modified: Option<String>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let equations = parse_equations(equations)?;
        check_scale(scale)?;
        let data = py.detach(|| {
            let opts = EpubOptions {
                scale,
                jobs: self.jobs,
                equations,
                max_math_width_em,
                cover,
                modified,
            };
            self.paper.to_epub(&self.analysis, &opts)
        });
        Ok(PyBytes::new(py, &data))
    }

    /// Chunks for retrieval as JSON Lines (one object per line). `tokenizer`
    /// is the path of a Hugging Face `tokenizer.json` or the name of an
    /// OpenAI tokenizer; without it, tokens are estimated.
    #[pyo3(signature = (*, max_tokens=512, tokenizer=None))]
    fn to_chunks(&self, py: Python<'_>, max_tokens: usize, tokenizer: Option<String>) -> PyResult<String> {
        let counter = match tokenizer {
            Some(spec) => TokenCounter::load(&spec).map_err(value_error)?,
            None => TokenCounter::estimate(),
        };
        Ok(py.detach(|| {
            let chunks = self
                .paper
                .to_chunks(&self.analysis, &ChunkOptions { max_tokens, counter });
            kakemono::chunks_jsonl(&chunks)
        }))
    }

    /// A Word document laid out in pages as Word lays it out, as the bytes
    /// of a PDF. With edited text, the edited document is laid out.
    fn to_pdf<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        if self.paper.format().has_pages() {
            return Err(value_error(
                "the input is already a PDF: only a Word document, a workbook or an EPUB book is laid out as a PDF",
            ));
        }
        let data = py.detach(|| {
            if self.edited {
                // The layout engine reads a Word file: the edited text and
                // pictures go into a Word document first, as on the command
                // line.
                let docx = self.paper.to_docx(&self.analysis, &DocxOptions::default());
                Paper::from_bytes(docx).and_then(|paper| paper.to_pdf())
            } else {
                self.paper.to_pdf()
            }
        });
        Ok(PyBytes::new(py, &data.map_err(value_error)?))
    }

    /// The structure of the document (blocks in reading order) as JSON, the
    /// same document as `kakemono convert --json`.
    fn analyze(&self, py: Python<'_>) -> PyResult<String> {
        py.detach(|| serde_json::to_string(&self.analysis.doc))
            .map_err(value_error)
    }
}

/// Draws an SVG as a PNG, as `kakemono render-svg` does (the same fonts,
/// `KAKEMONO_FONT_DIRS` included); pictures named by relative paths are
/// read from `resources_dir`. Returns the bytes of the PNG.
#[pyfunction]
#[pyo3(signature = (svg, *, scale=1.0, resources_dir=None))]
fn render_svg<'py>(
    py: Python<'py>,
    svg: String,
    scale: f32,
    resources_dir: Option<String>,
) -> PyResult<Bound<'py, PyBytes>> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(value_error("scale must be a positive number"));
    }
    let png = py
        .detach(|| kakemono::backend::svg_to_png_in(&svg, scale, resources_dir.as_deref().map(Path::new)))
        .map_err(|why| value_error(format!("cannot draw the SVG: {why}")))?;
    Ok(PyBytes::new(py, &png))
}

/// Reads the formula pictured in `image` (the bytes of a PNG or JPEG, black
/// on white, as LaTeX renders it), as `kakemono formula` does; returns its
/// MathML, or `None` when it cannot be read with confidence.
#[pyfunction]
fn read_formula(py: Python<'_>, image: Vec<u8>) -> PyResult<Option<String>> {
    py.detach(|| {
        let image =
            image::load_from_memory(&image).map_err(|err| value_error(format!("cannot read the picture: {err}")))?;
        let cfg = kakemono::raster::RasterConfig::default();
        Ok(kakemono::raster::formula(&image, &cfg).map(|node| kakemono::render::math_html(&node, "")))
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
    module.add_class::<Document>()?;
    module.add_function(wrap_pyfunction!(render_svg, module)?)?;
    module.add_function(wrap_pyfunction!(read_formula, module)?)?;
    module.add_function(wrap_pyfunction!(parse_arxiv_id, module)?)?;
    module.add_function(wrap_pyfunction!(fetch_arxiv, module)?)?;
    module.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
