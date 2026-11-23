//! The `kakemono` command line: parses the arguments and calls the
//! library. `convert` turns a PDF, a Word document or an EPUB book into HTML, Word, Markdown, LaTeX or EPUB (and JSON), `fetch` downloads an
//! arXiv PDF and `inspect` prints the detected structure for debugging.
#![warn(
    missing_docs,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::min_ident_chars
)]

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use kakemono::analysis::pipeline::Analysis;
use kakemono::model::{Block, BlockKind, Loc};
use kakemono::source::arxiv;
use kakemono::{
    AnalyzeOptions, ChunkOptions, DocxOptions, EpubOptions, Equations, HtmlOptions, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, PageImage, Paper, PngCompression, TokenCounter,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// mimalloc instead of the system allocator: it gives memory freed on one
/// thread back to the others (with glibc, the lines built on the worker
/// threads stay reserved after they are freed), and it is faster.
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// mimalloc option `purge_delay`, in milliseconds (its index in
/// `mi_option_e` of mimalloc.h; the sys crate does not name it).
const MI_OPTION_PURGE_DELAY: libmimalloc_sys::mi_option_t = 15;

/// Tunes mimalloc for a short run that should hold as little memory as it
/// can: freed memory goes back to the system at once rather than after a
/// delay (10 ms by default). This cuts the peak memory of a 1000-page book
/// by about a fifth, for about a tenth more time.
fn tune_allocator() {
    // SAFETY: `mi_option_set` only stores an option value, and
    // `MI_OPTION_PURGE_DELAY` is an option of the bundled mimalloc.
    unsafe {
        libmimalloc_sys::mi_option_set(MI_OPTION_PURGE_DELAY, 0);
    }
}

/// `--scale` must be larger than this (pixels per PDF point): smaller images
/// are unreadable.
const MIN_SCALE: f32 = 0.2;
/// Largest accepted `--scale` (pixels per PDF point): larger images only
/// waste space.
const MAX_SCALE: f32 = 8.0;
/// Smallest accepted `--chunk-tokens`: a section path and a sentence need
/// about this much.
const MIN_CHUNK_TOKENS: usize = 32;
/// `inspect` shortens block texts longer than this many characters.
const INSPECT_MAX_CHARS: usize = 150;
/// Characters kept from the start of a shortened block text.
const INSPECT_HEAD_CHARS: usize = 95;
/// Characters kept from the end of a shortened block text.
const INSPECT_TAIL_CHARS: usize = 45;
/// Bytes per megabyte, for the size report.
const BYTES_PER_MB: f64 = 1e6;

/// Command-line arguments.
#[derive(Parser)]
#[command(
    name = "kakemono",
    version,
    about = "Convert scientific-paper PDFs (arXiv), Word documents and EPUB books into clean, readable HTML, Word documents, Markdown, LaTeX or EPUB books"
)]
struct Cli {
    /// The subcommand to run.
    #[command(subcommand)]
    command: Command,
}

/// The subcommands. The doc comments of the variants and their fields are
/// the help texts.
#[derive(Subcommand)]
enum Command {
    /// Convert a PDF (local file, arXiv id or arXiv URL), a Word document (.docx, .doc) or an EPUB book (.epub) to HTML, Word, Markdown, LaTeX or EPUB.
    Convert(ConvertArgs),
    /// Download the PDF of an arXiv paper.
    Fetch {
        /// arXiv id or URL, e.g. 1706.03762
        id: String,
        /// Output file (default: <id>.pdf)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Print the detected structure of a paper (for debugging).
    Inspect {
        /// Local PDF, Word or EPUB file, arXiv id or arXiv URL
        input: String,
    },
    /// Print the text lines of a PDF with their baselines, as tab-separated
    /// page, baseline, left, right and text (for comparing layouts).
    #[command(hide = true)]
    Lines {
        /// A PDF file
        input: PathBuf,
    },
    /// Read pictures of formulas (black on white, as LaTeX renders them) and
    /// print them as MathML. With several pictures, each line is the file
    /// name, a tab and its MathML (empty when it cannot be read).
    Formula {
        /// Picture files (PNG or JPEG)
        #[arg(required = true)]
        images: Vec<PathBuf>,
        /// Print how sure each reading is (weakest glyph, closest rival)
        /// before its MathML, and give every reading whatever its confidence
        #[arg(long, hide = true)]
        scores: bool,
    },
    /// Draw an SVG as a PNG, the way kakemono draws the SVG pictures it is
    /// given (fonts, pictures named relative to the SVG).
    RenderSvg {
        /// The SVG file
        input: PathBuf,
        /// The PNG file to write
        #[arg(short, long)]
        output: PathBuf,
        /// Pixels per SVG unit
        #[arg(long, default_value_t = 1.0)]
        scale: f32,
    },
}

/// Values of `--mode`; the doc comments are the help texts.
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum ModeArg {
    /// Facsimile of every page with a selectable text layer
    Hard,
    /// Like hard, but all text is real HTML text placed as in the PDF
    #[value(name = "hard-95")]
    Hard95,
    /// Page and column layout kept, text reflowed
    Medium,
    /// Single-column responsive article
    Friendly,
}

impl From<ModeArg> for Mode {
    fn from(arg: ModeArg) -> Self {
        match arg {
            ModeArg::Hard => Mode::Hard,
            ModeArg::Hard95 => Mode::Hard95,
            ModeArg::Medium => Mode::Medium,
            ModeArg::Friendly => Mode::Friendly,
        }
    }
}

/// Values of `--format`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum FormatArg {
    /// HTML page(s) in the chosen mode(s)
    Html,
    /// A Word document (.docx): styles, Word tables and Office Math equations
    Docx,
    /// Markdown (.md): pipe tables, LaTeX formulas ($$), pictures in an image folder
    #[value(name = "md", alias = "markdown")]
    Markdown,
    /// LaTeX (.tex) for pdfLaTeX, XeLaTeX or LuaLaTeX: tables, LaTeX formulas, pictures in an image folder
    #[value(name = "tex", alias = "latex")]
    Latex,
    /// An EPUB 3 book (.epub) for e-readers: reflowing text, MathML formulas, tables, contents
    Epub,
    /// The content as JSON (.json): blocks in reading order, text with markup, LaTeX formulas, pictures in an image folder; can be edited and applied back with --content
    Json,
    /// Chunks for retrieval (.jsonl): pieces of at most --chunk-tokens tokens that follow the sections, each headed by its section path, one JSON object per line
    Chunks,
    /// A PDF (.pdf) of a Word document or an EPUB book, laid out in pages as Word lays it out
    Pdf,
}

/// Values of `--images`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum ImagesArg {
    /// Embed images in the output file (one self-contained file)
    Inline,
    /// Write images to a folder next to the output file (Word documents keep them embedded too; always for LaTeX)
    Files,
}

/// Values of `--fonts`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum FontsArg {
    /// Embed the reading fonts in friendly pages (works offline, about 250 KB) and the math font in pages with MathML (about 280 KB)
    Embed,
    /// Use the fonts installed on the reader's system (smaller files)
    System,
}

/// Values of `--tables`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum TablesArg {
    /// Rebuild tables as real HTML tables where their structure is clear
    Auto,
    /// Never rebuild tables (pictures in medium/friendly, placed text in hard-95)
    Image,
}

/// Values of `--equations`; the doc comments are the help texts.
#[derive(Clone, Copy, ValueEnum, PartialEq)]
enum EquationsArg {
    /// Pictures of the formulas
    Image,
    /// MathML rebuilt from the glyphs where the structure is clear, pictures elsewhere
    Mathml,
}

/// Values of `--page-image`.
#[derive(Clone, Copy, ValueEnum)]
#[allow(
    clippy::missing_docs_in_private_items,
    reason = "a doc comment on a variant becomes its help text and would change --help"
)]
enum PageImageArg {
    // Pages as SVG (vector graphics, sharp at any zoom); a very large page as PNG.
    Svg,
    // Every page as SVG, however large.
    SvgOnly,
    // Pages as PNG bitmaps.
    Png,
}

/// Values of `--png-compression`.
#[derive(Clone, Copy, ValueEnum)]
#[allow(
    clippy::missing_docs_in_private_items,
    reason = "a doc comment on a variant becomes its help text and would change --help"
)]
enum PngCompressionArg {
    // Several times faster, for files about a quarter larger.
    Fast,
    // The default.
    Balanced,
    // About a tenth smaller, for ten times the time.
    High,
}

impl From<PngCompressionArg> for PngCompression {
    fn from(arg: PngCompressionArg) -> Self {
        match arg {
            PngCompressionArg::Fast => PngCompression::Fast,
            PngCompressionArg::Balanced => PngCompression::Balanced,
            PngCompressionArg::High => PngCompression::High,
        }
    }
}

/// Arguments of `convert`; the doc comments of the fields are the help
/// texts.
#[derive(clap::Args)]
struct ConvertArgs {
    /// Local PDF, Word (.docx, .doc) or EPUB (.epub) file, arXiv id (1706.03762) or arXiv URL
    input: String,
    /// Fidelity mode (HTML); for Markdown, friendly adapts the tables to pipe tables and medium keeps their layout
    #[arg(short, long, value_enum, default_value = "friendly")]
    mode: ModeArg,
    /// Produce all four modes
    #[arg(long, conflicts_with = "mode")]
    all: bool,
    /// Output format (default: from the extension of --output, .docx, .md, .tex, .epub, .json or .jsonl; html otherwise); --mode applies to html and md only
    #[arg(short, long, value_enum)]
    format: Option<FormatArg>,
    /// Output HTML, .docx, .md, .tex, .epub, .json or .jsonl file, or a directory (required to be a directory with --all)
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// How images are stored [default: inline for HTML and Word, files for Markdown, LaTeX and JSON; EPUB books always hold theirs]
    #[arg(long, value_enum)]
    images: Option<ImagesArg>,
    /// Folder of the image files, relative to the output file [default: <name>_assets]; with --all, one subfolder per mode
    #[arg(long, value_name = "DIR")]
    image_dir: Option<String>,
    /// Resolution of figure/equation images in pixels per PDF point (HTML and Word)
    #[arg(long, default_value_t = 3.0)]
    scale: f32,
    /// Page background used by hard and hard-95 modes
    #[arg(long, value_enum, default_value = "svg")]
    page_image: PageImageArg,
    /// How hard PNG pictures are compressed: fast draws them several times faster, for larger files
    #[arg(long, value_enum, default_value = "balanced")]
    png_compression: PngCompressionArg,
    /// How tables are handled
    #[arg(long, value_enum, default_value = "auto")]
    tables: TablesArg,
    /// Fonts of friendly mode, and of the formulas of medium and friendly modes
    #[arg(long, value_enum, default_value = "embed")]
    fonts: FontsArg,
    /// How medium and friendly modes (and Word, Markdown, LaTeX and EPUB documents) show displayed formulas
    #[arg(long, value_enum, default_value = "mathml")]
    equations: EquationsArg,
    /// Also write the structured document as JSON, with every field of the analysis (to edit the text and apply it back, use --format json)
    #[arg(long)]
    json: bool,
    /// Use the text of this edited JSON file (written by --format json for the same PDF) in the output: any format, except hard mode
    #[arg(long, value_name = "FILE.json")]
    content: Option<PathBuf>,
    /// Largest size of a chunk in tokens (--format chunks)
    #[arg(long, default_value_t = 512, value_name = "N")]
    chunk_tokens: usize,
    /// Count the tokens of chunks with a real tokenizer instead of the estimate: the path of a Hugging Face tokenizer.json (the one of your embedding model), or cl100k_base or o200k_base (OpenAI)
    #[arg(long, value_name = "FILE.json|NAME")]
    tokenizer: Option<String>,
    /// Number of worker threads: 0 = one per CPU core, 1 = no parallelism
    #[arg(short, long, default_value_t = 0, value_name = "N")]
    jobs: usize,
}

/// Where the files of one `convert` run go.
struct OutputPaths {
    /// The HTML file named by `--output`, when it names a single file.
    file: Option<PathBuf>,
    /// Directory for the generated file names (`<stem>.<mode>.html`, the
    /// JSON); empty for the current directory.
    dir: PathBuf,
}

impl OutputPaths {
    /// Interprets `--output`: a directory when `--all` is given, when it is
    /// an existing directory or when it has no extension; a file otherwise
    /// (then its parent directory takes the JSON). Without `--output`
    /// everything goes to the current directory.
    fn new(output: Option<&PathBuf>, all: bool) -> Self {
        let out_is_dir = all || output.is_some_and(|path| path.is_dir() || path.extension().is_none());
        let dir: PathBuf = match (output, out_is_dir) {
            (Some(path), true) => path.clone(),
            (Some(path), false) => path.parent().map(Path::to_path_buf).unwrap_or_default(),
            (None, _) => PathBuf::new(),
        };
        let file = if out_is_dir { None } else { output.cloned() };
        Self { file, dir }
    }

    /// The HTML file for `mode`: the file named by `--output`, or
    /// `<stem>.<mode>.html` in the output directory.
    fn html_file(&self, stem: &str, mode: Mode) -> PathBuf {
        match &self.file {
            Some(file) => file.clone(),
            None => self.dir.join(format!("{stem}.{}.html", mode.name())),
        }
    }

    /// The Word, Markdown or LaTeX file: the file named by `--output`, or
    /// `<stem>.<ext>` in the output directory.
    fn single_file(&self, stem: &str, ext: &str) -> PathBuf {
        match &self.file {
            Some(file) => file.clone(),
            None => self.dir.join(format!("{stem}.{ext}")),
        }
    }
}

/// Runs the command line and exits with status 1 on error.
fn main() {
    tune_allocator();
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

/// Parses the arguments and runs the subcommand.
fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert(args) => convert(args),
        Command::Fetch { id, output } => {
            let id = arxiv::parse_id(&id).ok_or_else(|| anyhow!("'{id}' is not an arXiv id or URL"))?;
            let dest = output.unwrap_or_else(|| PathBuf::from(format!("{}.pdf", id.replace('/', "_"))));
            arxiv::save_pdf(&id, &dest)?;
            eprintln!("saved {}", dest.display());
            Ok(())
        }
        Command::Inspect { input } => inspect(&input),
        Command::Lines { input } => {
            let data = std::fs::read(&input).with_context(|| format!("reading {}", input.display()))?;
            let (lines, pages) = kakemono::layout::lines::pdf_lines(data)?;
            let mut out = std::io::BufWriter::new(std::io::stdout().lock());
            let written = writeln!(out, "pages\t{pages}").and_then(|()| {
                for line in lines {
                    writeln!(
                        out,
                        "{}\t{:.2}\t{:.2}\t{:.2}\t{}",
                        line.page, line.baseline, line.left, line.right, line.text
                    )?;
                }
                out.flush()
            });
            // A reader that stops early (`| head`) is not an error.
            match written {
                Err(error) if error.kind() != std::io::ErrorKind::BrokenPipe => Err(error.into()),
                _ => Ok(()),
            }
        }
        Command::Formula { images, scores } => formula(&images, scores),
        Command::RenderSvg { input, output, scale } => render_svg(&input, &output, scale),
    }
}

/// Draws the SVG file `input` as the PNG file `output` at `scale` pixels
/// per SVG unit.
///
/// # Errors
/// When `input` cannot be read or drawn, or `output` cannot be written.
fn render_svg(input: &Path, output: &Path, scale: f32) -> Result<()> {
    let svg = std::fs::read_to_string(input).with_context(|| format!("cannot read {}", input.display()))?;
    let png = kakemono::backend::svg_to_png_in(&svg, scale, input.parent())
        .map_err(|why| anyhow::anyhow!("cannot read {} as SVG: {why}", input.display()))?;
    std::fs::write(output, png).with_context(|| format!("writing {}", output.display()))
}

/// Loads the PDF named by `input` (an existing file, else an arXiv id or
/// URL) and returns it with the stem for output file names.
///
/// # Errors
/// When the file cannot be read, the download fails, the data is not a PDF,
/// or `input` is neither a file nor an arXiv id.
fn load(input: &str) -> Result<(Paper, String)> {
    let path = Path::new(input);
    if path.is_file() {
        let data = std::fs::read(path).with_context(|| format!("reading {input}"))?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("paper")
            .to_string();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(input)
            .to_string();
        return Ok((Paper::from_bytes(data)?.with_source(name), stem));
    }
    if let Some(id) = arxiv::parse_id(input) {
        eprintln!("fetching arXiv:{id} ...");
        let data = arxiv::fetch_pdf(&id)?;
        let stem = id.replace('/', "_");
        return Ok((Paper::from_bytes(data)?.with_source(format!("arXiv:{id}")), stem));
    }
    bail!("'{input}' is neither an existing file nor an arXiv id/URL")
}

/// Writes `data` to `path`, creating the parent directories first.
///
/// # Errors
/// When a directory or the file cannot be written.
fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(path, data).with_context(|| format!("writing {}", path.display()))
}

/// Checks the options of `convert` and returns the output format.
///
/// # Errors
/// When options cannot be used together, or a value is out of range.
fn check_args(args: &ConvertArgs) -> Result<FormatArg> {
    if !(args.scale > MIN_SCALE && args.scale <= MAX_SCALE) {
        bail!("--scale must be between 0.2 and 8");
    }
    let format = output_format(args);
    if format != FormatArg::Html && args.all {
        bail!("--all produces the HTML modes; it cannot be used with --format docx, md, tex, epub, json or chunks");
    }
    if args.chunk_tokens < MIN_CHUNK_TOKENS {
        bail!("--chunk-tokens must be at least {MIN_CHUNK_TOKENS}");
    }
    if format != FormatArg::Chunks && args.tokenizer.is_some() {
        bail!("--tokenizer counts the tokens of chunks: it needs --format chunks (or a .jsonl output)");
    }
    if args.content.is_some() && format == FormatArg::Html && !args.all && args.mode == ModeArg::Hard {
        bail!("hard mode shows the pages of the PDF: it cannot show edited text from --content (hard-95 can)");
    }
    if format == FormatArg::Markdown && matches!(args.mode, ModeArg::Hard | ModeArg::Hard95) {
        bail!("Markdown has no pages: --mode can be friendly or medium with --format md");
    }
    if format == FormatArg::Latex && args.images == Some(ImagesArg::Inline) {
        bail!("LaTeX cannot embed pictures: --images inline cannot be used with --format tex");
    }
    if format == FormatArg::Epub && (args.images == Some(ImagesArg::Files) || args.image_dir.is_some()) {
        bail!("an EPUB book holds its pictures: --images files and --image-dir cannot be used with --format epub");
    }
    if let Some(dir) = &args.image_dir
        && (dir.trim().is_empty() || Path::new(dir).is_absolute() || dir.split(['/', '\\']).any(|part| part == ".."))
    {
        bail!("--image-dir must be a relative folder inside the output directory");
    }
    Ok(format)
}

/// Writes a Word document laid out in pages as a PDF, with the text and
/// pictures of `--content` when it is given.
///
/// # Errors
/// When the input is not a Word document or an EPUB book, or the file
/// cannot be written.
fn write_pdf(paper: &Paper, args: &ConvertArgs, stem: &str) -> Result<()> {
    let started = Instant::now();
    let file = OutputPaths::new(args.output.as_ref(), false).single_file(stem, "pdf");
    let data = match &args.content {
        // The layout engine reads a Word file: the edited text and pictures
        // go into a Word document first, which is then laid out.
        Some(content) => {
            let mut analysis = paper.analyze();
            apply_content(paper, &mut analysis, content)?;
            let docx = paper.to_docx(&analysis, &kakemono::DocxOptions::default());
            Paper::from_bytes(docx)?.to_pdf()?
        }
        None => paper.to_pdf()?,
    };
    write_file(&file, &data)?;
    eprintln!(
        "{:<8} -> {} ({:.1} MB, {:.2}s)",
        "pdf",
        file.display(),
        data.len() as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// The `convert` subcommand: analyses the paper once, then writes the HTML
/// of each requested mode or the Word document and, with `--json`, the
/// document as JSON.
///
/// # Errors
/// When `--scale` is out of range, `--all` is given for a Word, Markdown or LaTeX document, `--images inline` for
/// LaTeX, the input cannot be loaded or an output file cannot be written.
fn convert(args: ConvertArgs) -> Result<()> {
    let format = check_args(&args)?;
    let (paper, stem) = load(&args.input)?;
    let paper = paper.with_png_compression(args.png_compression.into());
    if format == FormatArg::Pdf {
        return write_pdf(&paper, &args, &stem);
    }
    let started = Instant::now();
    let jobs = kakemono::common::parallel::resolve_jobs(args.jobs);
    let mut analysis = paper.analyze_with(AnalyzeOptions {
        tables: args.tables == TablesArg::Auto,
        jobs,
        ..Default::default()
    });
    let tables = analysis.doc.blocks.iter().filter(|block| block.table.is_some()).count();
    if paper.format().has_pages() {
        eprintln!(
            "analysed {} pages ({jobs} threads), {} blocks, {tables} tables rebuilt in {:.2}s",
            paper.page_count(),
            analysis.doc.blocks.len(),
            started.elapsed().as_secs_f32()
        );
    } else {
        eprintln!(
            "read {} document, {} blocks, {tables} tables",
            paper.format().name(),
            analysis.doc.blocks.len(),
        );
    }
    if let Some(content) = &args.content {
        apply_content(&paper, &mut analysis, content)?;
    }

    let paths = OutputPaths::new(args.output.as_ref(), args.all);
    match format {
        FormatArg::Docx => write_docx(&paper, &analysis, &args, jobs, &paths.single_file(&stem, "docx"))?,
        FormatArg::Markdown => write_markdown(&paper, &analysis, &args, jobs, &paths.single_file(&stem, "md"))?,
        FormatArg::Latex => write_latex(&paper, &analysis, &args, jobs, &paths.single_file(&stem, "tex"))?,
        FormatArg::Epub => write_epub(&paper, &analysis, &args, jobs, &paths.single_file(&stem, "epub"))?,
        FormatArg::Json => write_json(&paper, &analysis, &args, jobs, &paths.single_file(&stem, "json"))?,
        FormatArg::Chunks => write_chunks(&paper, &analysis, &args, &paths.single_file(&stem, "chunks.jsonl"))?,
        FormatArg::Html => write_html(&paper, &analysis, &args, jobs, &paths, &stem)?,
        FormatArg::Pdf => unreachable!("written above"),
    }

    if args.json {
        let file = paths.dir.join(format!("{stem}.json"));
        let json = serde_json::to_vec_pretty(&analysis.doc)?;
        write_file(&file, &json)?;
        eprintln!("json     -> {}", file.display());
    }
    Ok(())
}

/// The output format: `--format`, or Word when `--output` names a `.docx`
/// file, Markdown when it names a `.md` file, LaTeX when it names a `.tex`
/// file, EPUB when it names an `.epub` file, JSON when it names a `.json`
/// file, HTML otherwise.
fn output_format(args: &ConvertArgs) -> FormatArg {
    args.format.unwrap_or_else(|| {
        let ext = args
            .output
            .as_ref()
            .and_then(|path| path.extension())
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        match ext.as_str() {
            "docx" => FormatArg::Docx,
            "md" | "markdown" => FormatArg::Markdown,
            "tex" | "latex" => FormatArg::Latex,
            "epub" => FormatArg::Epub,
            "json" => FormatArg::Json,
            "jsonl" => FormatArg::Chunks,
            "pdf" => FormatArg::Pdf,
            _ => FormatArg::Html,
        }
    })
}

/// The folder of the image files of `file` (relative to its directory):
/// `--image-dir`, with a subfolder per mode when several modes are written
/// (`mode`), or `<name>_assets`.
fn image_dir(args: &ConvertArgs, file: &Path, mode: Option<Mode>) -> String {
    match (&args.image_dir, mode) {
        (Some(dir), Some(mode)) if args.all => format!("{}/{}", dir.trim_end_matches(['/', '\\']), mode.name()),
        (Some(dir), _) => dir.trim_end_matches(['/', '\\']).to_string(),
        (None, _) => {
            let stem = file.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("paper");
            format!("{stem}_assets")
        }
    }
}

/// Writes the image `files` (paths relative to the directory of `file`) and
/// returns the size report: ` + N image files in DIR`, or nothing.
///
/// # Errors
/// When a file cannot be written.
fn write_images(file: &Path, files: &[(String, Vec<u8>)]) -> Result<String> {
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    for (rel, data) in files {
        write_file(&base.join(rel), data)?;
    }
    let Some((first, _)) = files.first() else {
        return Ok(String::new());
    };
    let dir = Path::new(first).parent().unwrap_or(Path::new(""));
    Ok(format!(
        " + {} image files in {}",
        files.len(),
        base.join(dir).display()
    ))
}

/// Writes the HTML of each requested mode (`--mode`, or all four with
/// `--all`).
///
/// # Errors
/// When a file cannot be written.
fn write_html(
    paper: &Paper,
    analysis: &Analysis,
    args: &ConvertArgs,
    jobs: usize,
    paths: &OutputPaths,
    stem: &str,
) -> Result<()> {
    let modes: Vec<Mode> = if args.all && args.content.is_some() {
        eprintln!("hard mode skipped: it cannot show edited text from --content");
        vec![Mode::Hard95, Mode::Medium, Mode::Friendly]
    } else if args.all {
        vec![Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly]
    } else {
        vec![args.mode.into()]
    };

    for mode in modes {
        let file = paths.html_file(stem, mode);
        write_mode(paper, analysis, args, jobs, mode, &file)?;
    }
    Ok(())
}

/// Writes `analysis` as a Word document to `file` and reports the result
/// on stderr.
///
/// # Errors
/// When the file cannot be written.
fn write_docx(paper: &Paper, analysis: &Analysis, args: &ConvertArgs, jobs: usize, file: &Path) -> Result<()> {
    let opts = DocxOptions {
        scale: args.scale,
        jobs,
        equations: equations(args.equations),
    };
    let started = Instant::now();
    let (data, images) = if args.images == Some(ImagesArg::Files) {
        let out = paper.to_docx_with_images(analysis, &opts, &image_dir(args, file, None));
        (out.docx, out.files)
    } else {
        (paper.to_docx(analysis, &opts), Vec::new())
    };
    write_file(file, &data)?;
    let report = write_images(file, &images)?;
    eprintln!(
        "{:<8} -> {} ({:.1} MB{report}, {:.2}s)",
        "docx",
        file.display(),
        data.len() as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Writes `analysis` as Markdown to `file`, with the image files next to it
/// unless `--images inline`, and reports the result on stderr.
///
/// # Errors
/// When a file cannot be written.
fn write_markdown(paper: &Paper, analysis: &Analysis, args: &ConvertArgs, jobs: usize, file: &Path) -> Result<()> {
    let opts = MarkdownOptions {
        inline_images: args.images == Some(ImagesArg::Inline),
        asset_dir: image_dir(args, file, None),
        scale: args.scale,
        jobs,
        equations: equations(args.equations),
        mode: args.mode.into(),
    };
    let started = Instant::now();
    let out = paper.to_markdown(analysis, &opts);
    write_file(file, out.markdown.as_bytes())?;
    let report = write_images(file, &out.files)?;
    eprintln!(
        "{:<8} -> {} ({:.1} MB{report}, {:.2}s)",
        "markdown",
        file.display(),
        out.markdown.len() as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Writes `analysis` as LaTeX to `file`, with the image files next to it,
/// and reports the result on stderr.
///
/// # Errors
/// When a file cannot be written.
fn write_latex(paper: &Paper, analysis: &Analysis, args: &ConvertArgs, jobs: usize, file: &Path) -> Result<()> {
    let opts = LatexOptions {
        asset_dir: image_dir(args, file, None),
        scale: args.scale,
        jobs,
        equations: equations(args.equations),
    };
    let started = Instant::now();
    let out = paper.to_latex(analysis, &opts);
    write_file(file, out.latex.as_bytes())?;
    let report = write_images(file, &out.files)?;
    eprintln!(
        "{:<8} -> {} ({:.1} MB{report}, {:.2}s)",
        "latex",
        file.display(),
        out.latex.len() as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Writes `analysis` as JSON to `file`, with the image files next to it
/// unless `--images inline`, and reports the result on stderr.
///
/// # Errors
/// When a file cannot be written.
fn write_json(paper: &Paper, analysis: &Analysis, args: &ConvertArgs, jobs: usize, file: &Path) -> Result<()> {
    let opts = JsonOptions {
        inline_images: args.images == Some(ImagesArg::Inline),
        asset_dir: image_dir(args, file, None),
        scale: args.scale,
        jobs,
        equations: equations(args.equations),
    };
    let started = Instant::now();
    let out = paper.to_json(analysis, &opts);
    write_file(file, out.json.as_bytes())?;
    let report = write_images(file, &out.files)?;
    eprintln!(
        "{:<8} -> {} ({:.1} MB{report}, {:.2}s)",
        "json",
        file.display(),
        out.json.len() as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Writes `analysis` as chunks (JSON Lines) to `file` and reports the
/// result on stderr.
///
/// # Errors
/// When the tokenizer of `--tokenizer` cannot be loaded or the file cannot
/// be written.
fn write_chunks(paper: &Paper, analysis: &Analysis, args: &ConvertArgs, file: &Path) -> Result<()> {
    let counter = match &args.tokenizer {
        Some(spec) => TokenCounter::load(spec)?,
        None => TokenCounter::estimate(),
    };
    let opts = ChunkOptions {
        max_tokens: args.chunk_tokens,
        counter,
    };
    let started = Instant::now();
    let chunks = paper.to_chunks(analysis, &opts);
    write_file(file, kakemono::chunks_jsonl(&chunks).as_bytes())?;
    let tokens: usize = chunks.iter().map(|chunk| chunk.tokens).sum();
    eprintln!(
        "{:<8} -> {} ({} chunks, {tokens} tokens{}, {:.2}s)",
        "chunks",
        file.display(),
        chunks.len(),
        if args.tokenizer.is_some() { "" } else { " estimated" },
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Applies the edited JSON file `path` to `analysis` and reports how many
/// blocks changed, with the warnings, on stderr.
///
/// # Errors
/// When the file cannot be read or does not fit this PDF (see
/// [`Paper::apply_content`]).
fn apply_content(paper: &Paper, analysis: &mut Analysis, path: &Path) -> Result<()> {
    let json = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let report = paper
        .apply_content_in(analysis, &json, base)
        .with_context(|| format!("applying {}", path.display()))?;
    for warning in &report.warnings {
        eprintln!("warning: {warning}");
    }
    eprintln!(
        "content  <- {} ({} blocks edited, {} warnings)",
        path.display(),
        report.edited_blocks,
        report.warnings.len()
    );
    Ok(())
}

/// Writes `analysis` as an EPUB book to `file`, dated now, and reports the
/// result on stderr.
///
/// # Errors
/// When the file cannot be written.
fn write_epub(paper: &Paper, analysis: &Analysis, args: &ConvertArgs, jobs: usize, file: &Path) -> Result<()> {
    let opts = EpubOptions {
        scale: args.scale,
        jobs,
        equations: equations(args.equations),
        modified: Some(utc_timestamp(std::time::SystemTime::now())),
        ..Default::default()
    };
    let started = Instant::now();
    let data = paper.to_epub(analysis, &opts);
    write_file(file, &data)?;
    eprintln!(
        "{:<8} -> {} ({:.1} MB, {:.2}s)",
        "epub",
        file.display(),
        data.len() as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// `time` as `CCYY-MM-DDThh:mm:ssZ` (UTC), the form of the date of an EPUB
/// book. A time before 1970 gives 1970-01-01T00:00:00Z.
fn utc_timestamp(time: std::time::SystemTime) -> String {
    const SECONDS_PER_DAY: u64 = 86_400;
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, rest) = (secs / SECONDS_PER_DAY, secs % SECONDS_PER_DAY);
    // Days to a civil date (Howard Hinnant's algorithm), on eras of 400
    // years that start on 1 March.
    let days = days + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    )
}

/// The library value of `--equations`.
fn equations(arg: EquationsArg) -> Equations {
    match arg {
        EquationsArg::Image => Equations::Image,
        EquationsArg::Mathml => Equations::MathMl,
    }
}

/// Renders `analysis` in `mode` and writes the HTML to `file`, with the
/// image files next to it when images are not inlined, then reports the
/// result on stderr.
///
/// # Errors
/// When a file cannot be written.
fn write_mode(
    paper: &Paper,
    analysis: &Analysis,
    args: &ConvertArgs,
    jobs: usize,
    mode: Mode,
    file: &Path,
) -> Result<()> {
    let opts = HtmlOptions {
        mode,
        inline_images: args.images != Some(ImagesArg::Files),
        asset_dir: image_dir(args, file, Some(mode)),
        scale: args.scale,
        page_image: match args.page_image {
            PageImageArg::Svg => PageImage::Svg,
            PageImageArg::SvgOnly => PageImage::SvgOnly,
            PageImageArg::Png => PageImage::Png,
        },
        jobs,
        embed_fonts: args.fonts == FontsArg::Embed,
        equations: equations(args.equations),
    };
    let started = Instant::now();
    let base = file.parent().map(Path::to_path_buf).unwrap_or_default();
    if !base.as_os_str().is_empty() {
        std::fs::create_dir_all(&base).with_context(|| format!("creating {}", base.display()))?;
    }
    // The page is written as its pictures are drawn, to a temporary file
    // that takes the place of `file` once it is complete.
    let mut partial = file.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let created = std::fs::File::create(&partial).with_context(|| format!("writing {}", partial.display()))?;
    let mut out = CountingWriter {
        inner: std::io::BufWriter::new(created),
        bytes: 0,
    };
    let mut images: Vec<String> = Vec::new();
    let written = paper
        .write_html(analysis, &opts, &mut out, &mut |rel, data| {
            let path = base.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, data)?;
            images.push(rel.to_string());
            Ok(())
        })
        .and_then(|()| out.inner.flush());
    if let Err(err) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(err).with_context(|| format!("writing {}", file.display()));
    }
    std::fs::rename(&partial, file).with_context(|| format!("writing {}", file.display()))?;
    let report = match images.first() {
        Some(first) => format!(
            " + {} image files in {}",
            images.len(),
            base.join(Path::new(first).parent().unwrap_or(Path::new(""))).display()
        ),
        None => String::new(),
    };
    eprintln!(
        "{:<8} -> {} ({:.1} MB{report}, {:.2}s)",
        mode.name(),
        file.display(),
        out.bytes as f64 / BYTES_PER_MB,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// A writer that counts the bytes written through it.
struct CountingWriter<W> {
    /// Where the bytes go.
    inner: W,
    /// Bytes written so far.
    bytes: usize,
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes += written;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// The `formula` subcommand: prints the MathML of the formulas pictured in `images`.
///
/// # Errors
/// When a picture cannot be opened, or a single picture cannot be read.
fn formula(images: &[PathBuf], scores: bool) -> Result<()> {
    let cfg = kakemono::raster::RasterConfig::default();
    let mut out = std::io::stdout().lock();
    for path in images {
        let image = image::open(path).with_context(|| format!("cannot open {}", path.display()))?;
        if scores {
            let read = kakemono::raster::formula_with_confidence(&image, &cfg);
            let (weakest, closest, math) = read.map_or((f32::NAN, f32::NAN, String::new()), |(node, confidence)| {
                (
                    confidence.weakest,
                    confidence.closest,
                    kakemono::render::math_html(&node, ""),
                )
            });
            writeln!(out, "{}\t{weakest:.3}\t{closest:.3}\t{math}", path.display())?;
            continue;
        }
        let math = kakemono::raster::formula(&image, &cfg).map(|node| kakemono::render::math_html(&node, ""));
        if images.len() == 1 {
            let math = math.ok_or_else(|| anyhow!("cannot read the formula in {}", path.display()))?;
            writeln!(out, "{math}")?;
        } else {
            writeln!(out, "{}\t{}", path.display(), math.unwrap_or_default())?;
        }
    }
    Ok(())
}

/// The `inspect` subcommand: prints the document's metadata and one line
/// per block (page, column, kind, shortened text) to stdout. Stops quietly
/// when stdout is closed (e.g. piped into `head`).
///
/// # Errors
/// When the input cannot be loaded.
fn inspect(input: &str) -> Result<()> {
    let (paper, _) = load(input)?;
    let analysis = paper.analyze();
    let doc = &analysis.doc;
    let mut out = std::io::stdout().lock();
    let _ = writeln!(
        out,
        "title: {:?}\narxiv: {:?}\npages: {}  body font: {}pt  leading: {:.1}pt\n",
        doc.meta.title,
        doc.meta.arxiv_id,
        doc.pages.len(),
        doc.body_size,
        doc.leading
    );
    for block in &doc.blocks {
        let short = shorten(block.text());
        let kind = block_label(block);
        let zone = zone_label(&block.loc);
        if writeln!(out, "p{:<3} {:<4} {:<22} {}", block.page + 1, zone, kind, short).is_err() {
            break;
        }
    }
    Ok(())
}

/// `text` itself, or for texts longer than [`INSPECT_MAX_CHARS`] its start
/// and end joined by an ellipsis.
fn shorten(text: String) -> String {
    let n = text.chars().count();
    if n > INSPECT_MAX_CHARS {
        let head: String = text.chars().take(INSPECT_HEAD_CHARS).collect();
        let tail: String = text.chars().skip(n - INSPECT_TAIL_CHARS).collect();
        format!("{head} … {tail}")
    } else {
        text
    }
}

/// Short description of a block's kind for `inspect` ("H2", "EQ (3)
/// [200x30]", ...), followed by the table's size for rebuilt tables and by
/// `<math>` for equations whose structure was rebuilt (MathML).
fn block_label(block: &Block) -> String {
    let kind = kind_label(&block.kind);
    match &block.table {
        Some(table) => format!(
            "{kind} <table {}x{} hdr={}>",
            table.rows.len(),
            table.n_cols,
            table.header_rows
        ),
        None if block.math.is_some() => format!("{kind} <math>"),
        None => kind,
    }
}

/// Short description of a block kind with its main attributes.
fn kind_label(kind: &BlockKind) -> String {
    match kind {
        BlockKind::Title => "TITLE".to_string(),
        BlockKind::Authors => "AUTHOR".to_string(),
        BlockKind::Note => "NOTE".to_string(),
        BlockKind::Heading { level } => format!("H{level}"),
        BlockKind::Paragraph { in_abstract: true } => "ABSTRACT".into(),
        BlockKind::Paragraph { .. } => "P".into(),
        BlockKind::Code => "CODE".into(),
        BlockKind::ContentsEntry { level, page } => format!("TOC{level} {page}"),
        BlockKind::ListItem { marker, level } => format!("LI{level} {}", marker.clone().unwrap_or_default()),
        BlockKind::ListContinuation => "LI+".to_string(),
        BlockKind::Equation { number, image } => format!(
            "EQ {} {}",
            number.clone().unwrap_or_default(),
            image
                .map(|spec| format!("[{:.0}x{:.0}]", spec.rect.width(), spec.rect.height()))
                .unwrap_or_else(|| "[math]".into())
        ),
        BlockKind::Figure { kind, image, label } => format!(
            "{:?} {} {}",
            kind,
            label.clone().unwrap_or_default(),
            image
                .map(|spec| format!(
                    "[{:.0},{:.0} {:.0}x{:.0}]",
                    spec.rect.x0,
                    spec.rect.y0,
                    spec.rect.width(),
                    spec.rect.height()
                ))
                .unwrap_or_else(|| "[no image]".into())
        ),
        BlockKind::Footnote { marker } => format!("FN {marker}"),
        BlockKind::Reference { label } => format!("REF {}", label.clone().unwrap_or_default()),
    }
}

/// Column of a block in a multi-column zone as "column/columns", or "-"
/// for single-column zones.
fn zone_label(loc: &Loc) -> String {
    if loc.zone_cols > 1 {
        format!("{}/{}", loc.col + 1, loc.zone_cols)
    } else {
        "-".into()
    }
}

#[cfg(test)]
mod tests {
    use super::utc_timestamp;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(utc_timestamp(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400 + 3_723)),
            "2000-02-29T01:02:03Z"
        );
        assert_eq!(
            utc_timestamp(UNIX_EPOCH + Duration::from_secs(1_794_960_000)),
            "2026-11-18T00:00:00Z"
        );
    }
}
