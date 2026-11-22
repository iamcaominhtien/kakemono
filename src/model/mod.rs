//! The structured document produced by the layout analysis and consumed by
//! the HTML renderers.
//!
//! [`Document`] is the fixed boundary between analysis and output: the
//! analysis builds it, every renderer reads it, and it is what the JSON
//! output serialises. Fields marked `#[serde(skip)]` carry page geometry
//! that only the position-preserving renderers need.

pub mod line;
pub mod math;
pub mod style;

use crate::common::geom::Rect;
use serde::Serialize;

pub use line::{FRACTION_TERM, Line, LineGlyph, RAISED_FRACTION_TERM, RotatedText};
pub use math::MathNode;
pub use style::*;

/// A run of text with uniform styling, the unit of all text content.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Span {
    /// The text of the run.
    pub text: String,
    /// Bit set of `BOLD`, `ITALIC`, `MONO`, `MATH`, `SUP`, `SUB`, `SANS`.
    pub style: u8,
    /// Link target: an in-document anchor (`#ref-12`) or an external URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    /// Text colour as RGB; black is the default.
    #[serde(skip_serializing_if = "is_black")]
    pub color: [u8; 3],
    /// A formula in running text whose structure is known (the Office Math
    /// of a Word document): the span stands for it, and `text` is its plain
    /// text. `None` for the spans of a PDF, whose formulas are rebuilt from
    /// their styles.
    #[serde(skip)]
    pub math: Option<Box<MathNode>>,
    /// A translator's note attached at this point of the text (the `<tn>`
    /// tag of the editable JSON): the span is the note's call, and the
    /// note's content is these spans (styled text, without formulas or
    /// other notes). The call's `text` is its mark: empty in the document,
    /// filled with the note's number only by the outputs that number the
    /// notes themselves. `None` for every other span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<Vec<Span>>,
}

impl Span {
    /// An unstyled, unlinked, black span.
    pub fn plain(text: impl Into<String>) -> Self {
        Self::styled(text, 0)
    }

    /// An unlinked, black span with the style flags `style`.
    pub fn styled(text: impl Into<String>, style: u8) -> Self {
        Self {
            text: text.into(),
            style,
            href: None,
            color: [0; 3],
            math: None,
            note: None,
        }
    }
}

/// One cell of a reconstructed table.
#[derive(Debug, Clone, Serialize)]
pub struct TableCell {
    /// Index of the first grid column the cell occupies.
    pub col: usize,
    /// Number of grid columns the cell spans (at least 1).
    pub colspan: usize,
    /// Number of grid rows the cell spans (at least 1).
    pub rowspan: usize,
    /// Styled text of the cell.
    pub spans: Vec<Span>,
}

/// A table whose rows and columns could be reconstructed.
#[derive(Debug, Clone, Serialize)]
pub struct Table {
    /// Number of grid columns.
    pub n_cols: usize,
    /// Number of leading rows that form the header.
    pub header_rows: usize,
    /// "left" / "center" / "right" per column.
    pub col_align: Vec<&'static str>,
    /// The cells of each row, left to right. Positions covered by a cell
    /// spanning from an earlier row or column have no entry.
    pub rows: Vec<Vec<TableCell>>,
    /// Rule above each cell position `[row][col]`: 0 none, 1 thin, 2 heavy.
    pub rule_above: Vec<Vec<u8>>,
    /// Rule below the last row, per column.
    pub rule_bottom: Vec<u8>,
    /// Vertical rule on the left edge of each cell position `[row][col]`.
    pub rule_left: Vec<Vec<bool>>,
    /// Vertical rule on the right edge of the table, per row.
    pub rule_right: Vec<bool>,
    /// Geometry on the page (for the position-preserving renderer).
    #[serde(skip)]
    pub geom: TableGeom,
}

/// Where the grid of a [`Table`] lies on the page, so that the
/// position-preserving renderer can place the HTML table over the original.
#[derive(Debug, Clone, Default)]
pub struct TableGeom {
    /// Bounding box of the whole table.
    pub bbox: Rect,
    /// `n_cols + 1` x positions of the column boundaries.
    pub col_bounds: Vec<f32>,
    /// Horizontal extent (left, right) of the text of each column.
    pub col_text: Vec<(f32, f32)>,
    /// `rows + 1` y positions of the row boundaries.
    pub row_bounds: Vec<f32>,
    /// Baseline of the text of each row.
    pub row_baselines: Vec<f32>,
    /// Dominant font size of the table text.
    pub size: f32,
}

/// The text of one source line, as it appears in the PDF.
#[derive(Debug, Clone, Default)]
pub struct HLine {
    /// Styled text of the line.
    pub spans: Vec<Span>,
    /// The line ended with a hyphen that was removed when joining it to the
    /// next line.
    pub soft_hyphen: bool,
}

/// A run of glyphs of a displayed formula with its exact position.
#[derive(Debug, Clone)]
pub struct EqRun {
    /// The text of the run.
    pub text: String,
    /// Left edge of the run.
    pub x: f32,
    /// Baseline.
    pub y: f32,
    /// Width along the baseline.
    pub width: f32,
    /// Font size in points.
    pub size: f32,
    /// Style flags (see [`style`]).
    pub style: u8,
    /// Fill colour as RGB.
    pub color: [u8; 3],
    /// For glyphs that do not sit on the baseline in the usual way (big
    /// operators, tall delimiters, radicals): the box their ink occupies.
    pub ink: Option<Rect>,
}

/// A piece of rotated text (axis labels, margin stamps).
#[derive(Debug, Clone)]
pub struct RotatedRun {
    /// The text of the run.
    pub text: String,
    /// Origin of the first glyph, on the baseline.
    pub x: f32,
    /// Vertical coordinate of that origin (see [`RotatedRun::x`]).
    pub y: f32,
    /// Direction of the baseline in degrees (y axis pointing down).
    pub angle: f32,
    /// Font size in points.
    pub size: f32,
    /// Length along the baseline.
    pub length: f32,
    /// Style flags (see [`style`]).
    pub style: u8,
    /// Fill colour as RGB.
    pub color: [u8; 3],
    /// Origins of all glyphs.
    pub origins: Vec<(f32, f32)>,
}

/// A line of text that is not part of a text block, with its styling.
#[derive(Debug, Clone)]
pub struct PLine {
    /// The line with its glyph geometry.
    pub line: Line,
    /// Styled text of the line.
    pub spans: Vec<Span>,
    /// "header" for running heads and page numbers, "eqno" for equation
    /// numbers.
    pub kind: &'static str,
}

/// A picture: a rectangular area of a page of a PDF, drawn from the page,
/// or a picture file stored in the document (a Word document's pictures).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ImageSpec {
    /// Zero-based page index (0 for a stored picture).
    pub page: usize,
    /// The area to render, in page coordinates; for a stored picture, its
    /// size in the document, in points, from the origin.
    pub rect: Rect,
    /// Render on a transparent background (equations).
    pub transparent: bool,
    /// Index of the stored picture in [`Document::pictures`]; `None` for an
    /// area of a page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedded: Option<usize>,
}

impl ImageSpec {
    /// An area of a page of the PDF.
    pub fn region(page: usize, rect: Rect, transparent: bool) -> Self {
        Self {
            page,
            rect,
            transparent,
            embedded: None,
        }
    }
}

/// A picture file stored in the document (a picture of a Word document),
/// ready to be written as it is.
#[derive(Debug, Clone)]
pub struct Picture {
    /// The PNG or JPEG data.
    pub data: Vec<u8>,
    /// The data is a JPEG (a PNG otherwise).
    pub jpeg: bool,
}

/// What a [`Block`] is, with the data specific to that kind. Serialised as
/// a `type` tag in snake case next to the block's other fields.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    /// The paper's title (may be split over several blocks).
    Title,
    /// One author / affiliation group; `Block::rows` holds its lines.
    Authors,
    /// Miscellaneous front-matter text (notices above the title, ...).
    Note,
    /// A section heading.
    Heading {
        /// Nesting depth: 1 for sections, 2 for subsections, ...
        level: u8,
    },
    /// A paragraph of running text.
    Paragraph {
        /// The paragraph belongs to the abstract.
        in_abstract: bool,
    },
    /// An item of a bulleted or numbered list.
    ListItem {
        /// `None` for bullet lists.
        marker: Option<String>,
        /// Nesting level: 0 for an item of the outermost list, 1 for one
        /// of a list inside it, ...
        level: u8,
    },
    /// A further paragraph of the list item before it.
    ListContinuation,
    /// A displayed formula, shown as an image of its area or from its
    /// structure ([`Block::math`]).
    Equation {
        /// The area of the page that shows the formula; `None` when there
        /// is no picture of it (the formulas of a Word document, which have
        /// their structure instead).
        image: Option<ImageSpec>,
        /// The equation number as printed ("(3)"), if it has one.
        number: Option<String>,
    },
    /// A float (figure, table, algorithm) or an uncaptioned graphic; the
    /// block's spans hold the caption.
    Figure {
        /// What kind of float it is.
        kind: FloatKind,
        /// The area of the page that shows the float, if it has one.
        image: Option<ImageSpec>,
        /// Label of the caption ("Figure 2"), if there is a caption.
        label: Option<String>,
    },
    /// A footnote.
    Footnote {
        /// The marker that links the footnote to its call in the text.
        marker: String,
    },
    /// An entry of the bibliography.
    Reference {
        /// Citation label ("12", "Smith et al. 2020"), if the entry has one.
        label: Option<String>,
    },
    /// A code listing; `Block::rows` holds its lines, indented with spaces.
    Code,
    /// An entry of a table of contents; the spans hold its title, linked to
    /// the heading it names when one is found.
    ContentsEntry {
        /// Nesting level: 1 for the outermost entries (chapters).
        level: u8,
        /// The page number as printed ("47", "xi").
        page: String,
    },
}

/// A unit of content of the document (paragraph, heading, figure, ...), in
/// reading order.
#[derive(Debug, Clone, Serialize)]
pub struct Block {
    /// Sequential number of the block, in reading order.
    pub id: usize,
    /// What the block is.
    #[serde(flatten)]
    pub kind: BlockKind,
    /// Zero-based page index (of the first line, for blocks that continue
    /// onto another page or column).
    pub page: usize,
    /// Bounding box on its page.
    pub bbox: Rect,
    /// Text content. For figures this is the caption, for equations the
    /// extracted text of the formula (alt text).
    pub spans: Vec<Span>,
    /// Line-wise content for blocks whose line breaks matter (authors).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<Vec<Span>>,
    /// Stable anchor for cross references ("fig-2", "sec-3.1", "ref-12").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// Where the block sits in the page layout (zone and column).
    #[serde(skip)]
    pub loc: Loc,
    /// Source lines with their geometry (facsimile text layer).
    #[serde(skip)]
    pub lines: Vec<Line>,
    /// For each line, the text it contributes to `spans` (already joined
    /// with its successor: trailing space or removed hyphen).
    #[serde(skip)]
    pub line_texts: Vec<String>,
    /// The same, with styling.
    #[serde(skip)]
    pub hlines: Vec<HLine>,
    /// Reconstructed structure of a table float.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<Table>,
    /// The caption of a float comes before its picture or table, in a
    /// document read without page geometry (Word). For a PDF the side of
    /// the caption follows from the positions of its lines.
    #[serde(skip)]
    pub caption_above: bool,
    /// Lines inside a float (labels of a figure, cells of a table) with
    /// their styled text.
    #[serde(skip)]
    pub inner_lines: Vec<Line>,
    /// Styled text of each of `inner_lines`, in the same order.
    #[serde(skip)]
    pub inner_spans: Vec<Vec<Span>>,
    /// Positioned glyph runs of an equation.
    #[serde(skip)]
    pub eq_runs: Vec<EqRun>,
    /// Structure of an equation rebuilt from its glyphs and rules, when
    /// every part of it could be explained (see
    /// [`analysis::math`](crate::analysis::math)).
    #[serde(skip)]
    pub math: Option<MathNode>,
}

impl Block {
    /// An empty block of the given kind on the first page, with no box; the
    /// caller fills in the content.
    pub fn new(id: usize, kind: BlockKind) -> Self {
        Self {
            id,
            kind,
            page: 0,
            bbox: Rect::default(),
            spans: Vec::new(),
            rows: Vec::new(),
            anchor: None,
            loc: Loc::default(),
            lines: Vec::new(),
            line_texts: Vec::new(),
            hlines: Vec::new(),
            table: None,
            caption_above: false,
            inner_lines: Vec::new(),
            inner_spans: Vec::new(),
            eq_runs: Vec::new(),
            math: None,
        }
    }

    /// The plain text of the block (its spans concatenated, without
    /// styling).
    pub fn text(&self) -> String {
        self.spans.iter().map(|span| span.text.as_str()).collect()
    }
}

/// Size of a page.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageInfo {
    /// Width in points.
    pub width: f32,
    /// Height in points.
    pub height: f32,
}

/// Bibliographic information about the paper.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Meta {
    /// Title, from the title blocks with whitespace normalised.
    pub title: Option<String>,
    /// arXiv identifier, from the source or the arXiv margin stamp.
    pub arxiv_id: Option<String>,
    /// Text of the arXiv margin stamp (`arXiv:1706.03762v7 [cs.CL] 2 Aug
    /// 2023`), which the analysis removes from the pages.
    pub arxiv_stamp: Option<String>,
    /// Where the PDF came from (file name, arXiv id, ...).
    pub source: Option<String>,
}

/// The structured content of a whole paper.
#[derive(Debug, Clone, Serialize)]
pub struct Document {
    /// Bibliographic information.
    pub meta: Meta,
    /// Size of every page, by page index; empty for a document without
    /// pages (a Word document).
    pub pages: Vec<PageInfo>,
    /// Font size of the running text in points.
    pub body_size: f32,
    /// Distance between the baselines of consecutive lines of running
    /// text, in points.
    pub leading: f32,
    /// PostScript name of the font of the running text.
    pub body_font: String,
    /// PostScript name of the math font with the most glyphs (empty when
    /// the document has no math font).
    pub math_font: String,
    /// All content, in reading order.
    pub blocks: Vec<Block>,
    /// The pictures stored in the document (those of a Word document),
    /// which [`ImageSpec::embedded`] refers to; empty for a PDF.
    #[serde(skip)]
    pub pictures: Vec<Picture>,
}

/// Position of a block in the layout of its page, used to keep the reading
/// order and the columns of the medium renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Loc {
    /// Zero-based page index.
    pub page: usize,
    /// Index of the top-level zone on the page (zones are horizontal
    /// bands, each single-column or multi-column).
    pub zone: usize,
    /// Number of columns of that zone.
    pub zone_cols: usize,
    /// Column of the zone the block belongs to.
    pub col: usize,
    /// Index of the leaf on its page (unique per column fragment).
    pub leaf: usize,
}

/// The kind of a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FloatKind {
    /// A captioned figure.
    Figure,
    /// A captioned table.
    Table,
    /// A captioned algorithm or listing.
    Algorithm,
    /// A drawing or image without a caption.
    Graphic,
}

/// Horizontal alignment of a table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    /// Flush left.
    Left,
    /// Centred.
    Center,
    /// Flush right.
    Right,
}

/// Serde predicate: the colour is black, the default, so it is left out of
/// the JSON.
fn is_black(color: &[u8; 3]) -> bool {
    *color == [0; 3]
}
