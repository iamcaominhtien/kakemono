//! JSON output: the content of the document as one `.json` file that other
//! programs can read (search, language models, data extraction) and that
//! can be edited and given back to kakemono, which then writes any other
//! output from it ([`apply`]).
//!
//! The file holds the blocks in reading order. The text of a block is one
//! string with HTML-like tags for its styles, links and formulas (see
//! [`markup`]), so that it can be edited as a whole. Formulas also come as
//! LaTeX and MathML when their structure is known, figures and the other
//! formulas as pictures (files next to the JSON, or data URIs).
//!
//! The file names its source (format, name, SHA-256 of the bytes): edited
//! text is applied to the analysis of that same source, and each block
//! keeps the `id` and `type` it has there. Geometry (page and box) is in a
//! `layout` object of its own, which sources without pages leave out.

pub(crate) mod apply;
pub(crate) mod citation;
pub(crate) mod front;
pub(crate) mod markup;
pub(crate) mod plain_text;

use super::latex::math::latex;
use super::{Equations, HtmlOptions, ImageStore, Mode, PageImage, float_class, math_html, plain};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::common::geom::Rect;
use crate::model::{Block, BlockKind, Document, FloatKind, ImageSpec, Span, Table};
use crate::render::Pictures;
use markup::to_markup;
use serde::Serialize;

/// Version of the layout of the JSON file. It changes when a field changes
/// meaning or goes away; new fields may be added without a new version.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Name of the format, written in each file so that a JSON file from
/// elsewhere is recognised as such.
pub(crate) const FORMAT_NAME: &str = "kakemono-content";

/// Decimal places of the coordinates in `layout` (hundredths of a point).
const COORD_STEPS_PER_PT: f64 = 100.0;

/// Everything that controls how a document is written as JSON.
#[derive(Debug, Clone)]
pub struct JsonOptions {
    /// Embed the pictures as data URIs (one self-contained file) instead of
    /// writing them to [`JsonOptions::asset_dir`].
    pub inline_images: bool,
    /// Directory (relative to the JSON file) of the picture files.
    pub asset_dir: String,
    /// Pixels per PDF point of the pictures of figures and formulas.
    pub scale: f32,
    /// Worker threads for drawing the pictures (0 = one per CPU).
    pub jobs: usize,
    /// Pictures of displayed formulas: [`Equations::MathMl`] gives a picture
    /// only to the formulas whose structure is not known (the others have
    /// their LaTeX and MathML), [`Equations::Image`] to every formula.
    pub equations: Equations,
}

impl Default for JsonOptions {
    /// Pictures written to `images/` at 3 pixels per point, pictures only
    /// for formulas without LaTeX, one worker thread per CPU.
    fn default() -> Self {
        Self {
            inline_images: false,
            asset_dir: "images".into(),
            scale: 3.0,
            jobs: 0,
            equations: Equations::MathMl,
        }
    }
}

/// The result of the JSON output: the text and, when pictures are not
/// embedded, the picture files it refers to.
pub struct JsonOutput {
    /// The JSON document.
    pub json: String,
    /// Files to write next to the JSON file: (relative path, contents).
    pub files: Vec<(String, Vec<u8>)>,
}

/// Where a document came from, as the JSON output names it.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SourceInfo {
    /// Format of the source file (`pdf`).
    pub(crate) format: &'static str,
    /// Name of the source (file name, arXiv id), when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    /// SHA-256 of the bytes of the source, in hexadecimal.
    pub(crate) sha256: String,
}

/// The whole JSON file.
#[derive(Serialize)]
struct FileOut<'a> {
    /// Always [`FORMAT_NAME`].
    format: &'static str,
    /// [`FORMAT_VERSION`].
    version: u32,
    /// Version of kakemono that wrote the file.
    generator: String,
    /// Where the document came from.
    source: &'a SourceInfo,
    /// Title and identifiers.
    meta: MetaOut<'a>,
    /// Size of each page, in points (sources with pages only).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pages: Vec<[f64; 2]>,
    /// The blocks, in reading order.
    blocks: Vec<BlockOut>,
}

/// Bibliographic information.
#[derive(Serialize)]
struct MetaOut<'a> {
    /// Title of the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<&'a str>,
    /// arXiv identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    arxiv_id: Option<&'a str>,
    /// Authors, date, keywords and the rest of the front matter (read-only;
    /// see [`front`]).
    #[serde(flatten)]
    front: front::Front,
}

/// One block. Fields that do not apply to its type are left out.
#[derive(Serialize, Default)]
struct BlockOut {
    /// Number of the block in reading order, the same for every output of
    /// the same source.
    id: usize,
    /// What the block is (`paragraph`, `heading`, `figure`, ...).
    #[serde(rename = "type")]
    kind: &'static str,
    /// Level of a heading (1 for sections) or of a contents entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<u8>,
    /// The paragraph belongs to the abstract.
    #[serde(rename = "abstract", skip_serializing_if = "std::ops::Not::not")]
    in_abstract: bool,
    /// Marker of a list item ("1.", "(a)") or of a footnote.
    #[serde(skip_serializing_if = "Option::is_none")]
    marker: Option<String>,
    /// What kind of float a figure is: figure, table, algorithm, graphic.
    #[serde(skip_serializing_if = "Option::is_none")]
    float: Option<&'static str>,
    /// Label of a caption ("Figure 2") or of a reference ("12").
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    /// Number of an equation as printed ("(3)").
    #[serde(skip_serializing_if = "Option::is_none")]
    number: Option<String>,
    /// Page number of a contents entry as printed ("47", "xi").
    #[serde(skip_serializing_if = "Option::is_none")]
    printed_page: Option<String>,
    /// Anchor that links point to ("sec-3.1", "fig-2", "ref-12").
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor: Option<String>,
    /// For a heading: the id of the heading it is under.
    #[serde(skip_serializing_if = "Option::is_none")]
    parent: Option<usize>,
    /// For any other block: the id of the heading of its section.
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<usize>,
    /// The text, with markup (editable).
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    /// The text without markup, formulas as `$LaTeX$` (read-only; see
    /// [`plain_text`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    plain: Option<String>,
    /// What a reference cites, read from its text (read-only; see
    /// [`citation`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    citation: Option<citation::Citation>,
    /// The lines of an author group, with markup (editable).
    #[serde(skip_serializing_if = "Option::is_none")]
    rows: Option<Vec<String>>,
    /// The lines of a code listing, as plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    lines: Option<Vec<String>>,
    /// Text found inside a figure (axis labels, legends), as plain text.
    #[serde(skip_serializing_if = "Option::is_none")]
    figure_text: Option<String>,
    /// The text of a formula as the PDF gives it (alt text).
    #[serde(skip_serializing_if = "Option::is_none")]
    alt: Option<String>,
    /// LaTeX of a formula whose structure is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    latex: Option<String>,
    /// MathML of a formula whose structure is known.
    #[serde(skip_serializing_if = "Option::is_none")]
    mathml: Option<String>,
    /// The picture of a figure or formula: file path or data URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    /// A rebuilt table.
    #[serde(skip_serializing_if = "Option::is_none")]
    table: Option<TableOut>,
    /// Place on the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    layout: Option<LayoutOut>,
}

/// A rebuilt table.
#[derive(Serialize)]
struct TableOut {
    /// Number of grid columns.
    columns: usize,
    /// Number of leading rows that form the header.
    header_rows: usize,
    /// "left", "center" or "right", per column.
    align: Vec<&'static str>,
    /// The cells of each row, left to right.
    rows: Vec<Vec<CellOut>>,
}

/// One cell of a table.
#[derive(Serialize)]
struct CellOut {
    /// First grid column of the cell.
    col: usize,
    /// Number of columns it spans, when more than one.
    #[serde(skip_serializing_if = "is_one")]
    colspan: usize,
    /// Number of rows it spans, when more than one.
    #[serde(skip_serializing_if = "is_one")]
    rowspan: usize,
    /// The text, with markup (editable).
    text: String,
    /// The text without markup (read-only).
    plain: String,
}

/// Where a block is on its page.
#[derive(Serialize)]
struct LayoutOut {
    /// Page index, from 0.
    page: usize,
    /// Bounding box `[x0, y0, x1, y1]` in points, y pointing down.
    bbox: [f64; 4],
}

/// Writes an analysed document as JSON.
///
/// The text is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and either
/// embedded as data URIs or returned in [`JsonOutput::files`].
pub(crate) fn render_json(
    analysis: &Analysis,
    pdf: Option<&PdfFile>,
    opts: &JsonOptions,
    source: &SourceInfo,
) -> JsonOutput {
    let html_opts = HtmlOptions {
        mode: Mode::Friendly,
        inline_images: opts.inline_images,
        asset_dir: opts.asset_dir.clone(),
        scale: opts.scale,
        page_image: PageImage::Svg,
        jobs: opts.jobs,
        embed_fonts: false,
        equations: opts.equations,
    };
    let mut images = ImageStore {
        opts: &html_opts,
        jobs: Vec::new(),
        vector: true,
    };
    let doc = &analysis.doc;
    let file = FileOut {
        format: FORMAT_NAME,
        version: FORMAT_VERSION,
        generator: format!("kakemono {}", env!("CARGO_PKG_VERSION")),
        source,
        meta: MetaOut {
            title: doc.meta.title.as_deref(),
            arxiv_id: doc.meta.arxiv_id.as_deref(),
            front: front::front_matter(doc),
        },
        pages: doc
            .pages
            .iter()
            .map(|page| [round(page.width), round(page.height)])
            .collect(),
        blocks: blocks_out(doc, opts, &mut images),
    };
    let json = serde_json::to_string_pretty(&file).expect("the JSON output serialises");
    // serde_json writes the placeholder marks of the pictures as escapes;
    // the image store looks for the marks themselves.
    let json = json.replace(MARK_ESCAPED, &super::assets::MARK.to_string()) + "\n";
    let out = images.finish(Pictures::new(pdf, &analysis.doc), json);
    JsonOutput {
        json: out.html,
        files: out.files,
    }
}

/// How serde_json writes [`super::assets::MARK`] in a string.
const MARK_ESCAPED: &str = "\\u0001";

/// The blocks of `doc` as JSON, with the section each one is in.
fn blocks_out(doc: &Document, opts: &JsonOptions, images: &mut ImageStore<'_>) -> Vec<BlockOut> {
    // The headings open above the current block: (level, id).
    let mut headings: Vec<(u8, usize)> = Vec::new();
    let mut out = Vec::with_capacity(doc.blocks.len());
    for block in &doc.blocks {
        let mut json = block_out(block, opts, images);
        if let BlockKind::Heading { level } = block.kind {
            while headings.last().is_some_and(|&(open, _)| open >= level) {
                headings.pop();
            }
            json.parent = headings.last().map(|&(_, id)| id);
            headings.push((level, block.id));
        } else {
            json.section = headings.last().map(|&(_, id)| id);
        }
        out.push(json);
    }
    out
}

/// One block as JSON, without its section.
fn block_out(block: &Block, opts: &JsonOptions, images: &mut ImageStore<'_>) -> BlockOut {
    let mut json = BlockOut {
        id: block.id,
        kind: kind_name(&block.kind),
        anchor: block.anchor.clone(),
        layout: Some(LayoutOut {
            page: block.page,
            bbox: bbox(&block.bbox),
        }),
        plain: plain_text::block_plain(block),
        ..Default::default()
    };
    match &block.kind {
        BlockKind::Title
        | BlockKind::Note
        | BlockKind::ListContinuation
        | BlockKind::Paragraph { in_abstract: false } => json.text = Some(text_of(&block.spans)),
        BlockKind::Paragraph { in_abstract: true } => {
            json.in_abstract = true;
            json.text = Some(text_of(&block.spans));
        }
        BlockKind::Heading { level } => {
            json.level = Some(*level);
            json.text = Some(text_of(&block.spans));
        }
        BlockKind::ListItem { marker, level } => {
            json.marker = marker.clone();
            json.level = (*level > 0).then_some(*level);
            json.text = Some(text_of(&block.spans));
        }
        BlockKind::Footnote { marker } => {
            json.marker = Some(marker.clone());
            json.text = Some(text_of(&block.spans));
        }
        BlockKind::Reference { label } => {
            json.label = label.clone();
            json.text = Some(text_of(&block.spans));
            json.citation = citation::parse(&block.spans);
        }
        BlockKind::ContentsEntry { level, page } => {
            json.level = Some(*level);
            json.printed_page = Some(page.clone());
            json.text = Some(text_of(&block.spans));
        }
        BlockKind::Authors if block.rows.is_empty() => json.text = Some(text_of(&block.spans)),
        BlockKind::Authors => json.rows = Some(block.rows.iter().map(|row| text_of(row)).collect()),
        BlockKind::Code => json.lines = Some(block.rows.iter().map(|row| plain_line(row)).collect()),
        BlockKind::Equation { image, number } => equation_out(&mut json, block, image.as_ref(), number, opts, images),
        BlockKind::Figure { kind, image, label } => {
            json.float = Some(float_name(*kind));
            json.label = label.clone();
            json.text = Some(text_of(&block.spans));
            json.figure_text = block.rows.first().map(|row| plain(row)).filter(|text| !text.is_empty());
            json.image = image.as_ref().map(|spec| images.region(spec, float_class(*kind)));
            json.table = block.table.as_ref().map(table_out);
        }
    }
    json
}

/// The fields of a displayed formula: its LaTeX and MathML when its
/// structure is known, its picture otherwise (or always, with
/// [`Equations::Image`]).
fn equation_out(
    json: &mut BlockOut,
    block: &Block,
    image: Option<&ImageSpec>,
    number: &Option<String>,
    opts: &JsonOptions,
    images: &mut ImageStore<'_>,
) {
    let alt = plain(&block.spans);
    json.number = number.clone();
    if let Some(node) = &block.math {
        json.latex = Some(latex(node));
        json.mathml = Some(math_html(node, &alt));
    }
    if let Some(image) = image
        && (block.math.is_none() || opts.equations == Equations::Image)
    {
        json.image = Some(images.region(image, "eq"));
    }
    json.alt = Some(alt);
}

/// A rebuilt table as JSON.
fn table_out(table: &Table) -> TableOut {
    TableOut {
        columns: table.n_cols,
        header_rows: table.header_rows,
        align: table.col_align.clone(),
        rows: table
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|cell| CellOut {
                        col: cell.col,
                        colspan: cell.colspan,
                        rowspan: cell.rowspan,
                        text: text_of(&cell.spans),
                        plain: plain_text::plain_of(&cell.spans),
                    })
                    .collect()
            })
            .collect(),
    }
}

/// The text of spans with markup, with the placeholder mark of the image
/// store removed (it is never part of real text).
pub(crate) fn text_of(spans: &[Span]) -> String {
    to_markup(spans).text.replace(super::assets::MARK, "")
}

/// A line of code as plain text, without the placeholder mark.
fn plain_line(row: &[Span]) -> String {
    row.iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
        .replace(super::assets::MARK, "")
}

/// The name of a block kind in the JSON (`type`).
pub(crate) fn kind_name(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Title => "title",
        BlockKind::Authors => "authors",
        BlockKind::Note => "note",
        BlockKind::Heading { .. } => "heading",
        BlockKind::Paragraph { .. } => "paragraph",
        BlockKind::ListItem { .. } => "list_item",
        BlockKind::ListContinuation => "list_continuation",
        BlockKind::Equation { .. } => "equation",
        BlockKind::Figure { .. } => "figure",
        BlockKind::Footnote { .. } => "footnote",
        BlockKind::Reference { .. } => "reference",
        BlockKind::Code => "code",
        BlockKind::ContentsEntry { .. } => "contents_entry",
    }
}

/// The name of a float kind.
fn float_name(kind: FloatKind) -> &'static str {
    match kind {
        FloatKind::Figure => "figure",
        FloatKind::Table => "table",
        FloatKind::Algorithm => "algorithm",
        FloatKind::Graphic => "graphic",
    }
}

/// A box as `[x0, y0, x1, y1]`, rounded.
fn bbox(rect: &Rect) -> [f64; 4] {
    [round(rect.x0), round(rect.y0), round(rect.x1), round(rect.y1)]
}

/// A coordinate rounded to [`COORD_STEPS_PER_PT`].
fn round(value: f32) -> f64 {
    (f64::from(value) * COORD_STEPS_PER_PT).round() / COORD_STEPS_PER_PT
}

/// Serde predicate: a span of one row or column, left out of the JSON.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_one(value: &usize) -> bool {
    *value == 1
}
