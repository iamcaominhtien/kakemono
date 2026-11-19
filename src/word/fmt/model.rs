//! A Word document as Word lays it out: sections with their page setup,
//! paragraphs and runs with every formatting property resolved from the
//! styles, tables with their grid and borders, headers, footers, notes,
//! pictures and floating objects. The layout engine (`crate::layout`)
//! works from it; lengths are in points.

use crate::model::MathNode;

/// An RGB colour.
pub type Rgb = [u8; 3];

/// A Word document with its formatting.
#[derive(Debug, Default, Clone)]
pub struct FDoc {
    /// The sections, in order.
    pub sections: Vec<Section>,
    /// The headers and footers, which sections refer to by index.
    pub parts: Vec<Vec<Block>>,
    /// The footnotes, by their id in the file.
    pub footnotes: Vec<(String, Vec<Block>)>,
    /// The line between the text and the footnotes of a page, when the file
    /// sets it.
    pub footnote_separator: Option<Vec<Block>>,
    /// The endnotes, by their id in the file.
    pub endnotes: Vec<(String, Vec<Block>)>,
    /// The pictures, as PNG or JPEG files.
    pub images: Vec<FImage>,
    /// The fonts embedded in the file.
    pub fonts: Vec<EmbeddedFont>,
    /// The kind of each font the document names (`fontTable.xml`), by
    /// lowercase name: used to choose a stand-in for a missing font.
    pub font_kinds: std::collections::HashMap<String, FontKind>,
    /// Distance between the default tab stops.
    pub default_tab: f32,
    /// The document title (document properties).
    pub title: Option<String>,
    /// Headers and footers differ on even and odd pages.
    pub even_and_odd_headers: bool,
    /// Format of the footnote numbers.
    pub footnote_format: NumFormat,
    /// Format of the endnote numbers.
    pub endnote_format: NumFormat,
    /// Words are hyphenated automatically.
    pub auto_hyphenation: bool,
    /// The Word version whose layout rules apply (`w:compatibilityMode`:
    /// 15 for Word 2013 and later, 14 for 2010, 12 for 2007, 11 and below
    /// for older files).
    pub compat_mode: u32,
    /// Lines ended by a line break are not widened in justified paragraphs
    /// (`w:doNotExpandShiftReturn`).
    pub no_expand_shift_return: bool,
    /// The space after a paragraph and the space before the next are added
    /// (`w:doNotUseHTMLParagraphAutoSpacing`); otherwise the larger is used.
    pub add_paragraph_spacing: bool,
}

/// The kind of a font, for choosing a stand-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    /// Without serifs (`swiss`).
    Sans,
    /// With serifs (`roman`).
    Serif,
    /// Monospaced (`modern`, fixed pitch).
    Mono,
    /// Symbols or pictures (`decorative`, a symbol character set).
    Symbol,
}

/// A picture file.
#[derive(Debug, Clone)]
pub struct FImage {
    /// The file.
    pub data: Vec<u8>,
    /// It is a JPEG (else a PNG).
    pub jpeg: bool,
}

/// A font embedded in the document.
#[derive(Debug, Clone)]
pub struct EmbeddedFont {
    /// Its family name, as the runs name it.
    pub family: String,
    /// It is the bold face.
    pub bold: bool,
    /// It is the italic face.
    pub italic: bool,
    /// The font file (TrueType or OpenType).
    pub data: Vec<u8>,
}

/// A section: its page setup and content.
#[derive(Debug, Clone, Default)]
pub struct Section {
    /// Size and margins of its pages.
    pub page: PageSetup,
    /// Its columns.
    pub columns: Columns,
    /// How it starts.
    pub start: SectionStart,
    /// Its headers.
    pub headers: HeaderSet,
    /// Its footers.
    pub footers: HeaderSet,
    /// The first page has its own header and footer.
    pub title_page: bool,
    /// The page number of its first page, when it restarts numbering.
    pub page_number_start: Option<u32>,
    /// Format of its page numbers.
    pub page_number_format: NumFormat,
    /// Height of the lines of its document grid, when lines snap to it.
    pub grid_pitch: Option<f32>,
    /// Its content.
    pub blocks: Vec<Block>,
}

/// Size and margins of a page.
#[derive(Debug, Clone, Copy)]
pub struct PageSetup {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Top margin (to the body text).
    pub top: f32,
    /// Bottom margin.
    pub bottom: f32,
    /// Left margin.
    pub left: f32,
    /// Right margin.
    pub right: f32,
    /// Distance from the top edge to the header.
    pub header: f32,
    /// Distance from the bottom edge to the footer.
    pub footer: f32,
    /// Gutter, added to the left margin.
    pub gutter: f32,
}

impl Default for PageSetup {
    /// US Letter with one-inch margins, Word's default.
    fn default() -> Self {
        PageSetup {
            width: 612.0,
            height: 792.0,
            top: 72.0,
            bottom: 72.0,
            left: 72.0,
            right: 72.0,
            header: 36.0,
            footer: 36.0,
            gutter: 0.0,
        }
    }
}

/// The columns of a section.
#[derive(Debug, Clone, Default)]
pub struct Columns {
    /// Width and following space of each column, when they are not equal;
    /// empty for equal columns.
    pub widths: Vec<(f32, f32)>,
    /// Number of equal columns (at least 1).
    pub count: u32,
    /// Space between equal columns.
    pub space: f32,
    /// A line is drawn between the columns.
    pub separator: bool,
}

/// How a section starts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SectionStart {
    /// On a new page.
    #[default]
    NextPage,
    /// On the same page.
    Continuous,
    /// In the next column.
    NextColumn,
    /// On the next odd page.
    OddPage,
    /// On the next even page.
    EvenPage,
}

/// The headers (or footers) of a section: indexes into [`FDoc::parts`].
#[derive(Debug, Clone, Copy, Default)]
pub struct HeaderSet {
    /// The usual one (odd pages when they differ).
    pub default: Option<usize>,
    /// The first page's.
    pub first: Option<usize>,
    /// The even pages'.
    pub even: Option<usize>,
}

/// A number format (of list items, pages, notes).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NumFormat {
    /// 1, 2, 3.
    #[default]
    Decimal,
    /// 01, 02, 03.
    DecimalZero,
    /// a, b, c.
    LowerLetter,
    /// A, B, C.
    UpperLetter,
    /// i, ii, iii.
    LowerRoman,
    /// I, II, III.
    UpperRoman,
    /// A bullet.
    Bullet,
    /// No number.
    None,
    /// Symbols for notes: *, †, ‡, §.
    Chicago,
    /// One, two, three.
    CardinalText,
    /// First, second, third.
    OrdinalText,
    /// 1st, 2nd, 3rd.
    Ordinal,
}

/// A paragraph or a table.
#[derive(Debug, Clone)]
pub enum Block {
    /// A paragraph (boxed: it is much larger than a table).
    Para(Box<Para>),
    /// A table.
    Table(Table),
}

/// A paragraph.
#[derive(Debug, Clone, Default)]
pub struct Para {
    /// Its id of paragraph style.
    pub style: String,
    /// Its paragraph properties.
    pub props: ParaProps,
    /// Properties of the paragraph mark, which give an empty paragraph its
    /// height.
    pub mark: RunFmt,
    /// Its list number or bullet.
    pub number: Option<NumberMark>,
    /// Its content.
    pub inlines: Vec<Inline>,
    /// The bookmarks that start in it.
    pub bookmarks: Vec<String>,
}

/// The number or bullet of a list item.
#[derive(Debug, Clone)]
pub struct NumberMark {
    /// Its text ("1.", "a)", "•").
    pub text: String,
    /// Its text properties.
    pub fmt: RunFmt,
    /// What follows it.
    pub suffix: NumberSuffix,
    /// Its alignment at the start of the first line.
    pub align: Align,
}

/// What follows a list number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NumberSuffix {
    /// A tab.
    #[default]
    Tab,
    /// A space.
    Space,
    /// Nothing.
    Nothing,
}

/// Horizontal alignment of a paragraph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    /// Left (start).
    #[default]
    Left,
    /// Centred.
    Center,
    /// Right (end).
    Right,
    /// Justified, the last line left.
    Both,
    /// Justified, every line including the last one, letters spaced.
    Distribute,
}

/// Line spacing of a paragraph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineSpacing {
    /// A multiple of the natural line height (1.0 for single).
    Auto(f32),
    /// Exactly this height.
    Exact(f32),
    /// At least this height.
    AtLeast(f32),
}

impl Default for LineSpacing {
    fn default() -> Self {
        LineSpacing::Auto(1.0)
    }
}

/// The properties of a paragraph, resolved.
#[derive(Debug, Clone, Default)]
pub struct ParaProps {
    /// Alignment.
    pub align: Align,
    /// Left indent.
    pub left: f32,
    /// Right indent.
    pub right: f32,
    /// Indent of the first line relative to the others (negative for a
    /// hanging indent).
    pub first_line: f32,
    /// Space before.
    pub before: f32,
    /// Space after.
    pub after: f32,
    /// Space before is chosen by Word ("auto": 14 pt, none at the top of
    /// a cell or page).
    pub before_auto: bool,
    /// Space after is chosen by Word.
    pub after_auto: bool,
    /// Line spacing.
    pub line: LineSpacing,
    /// Spacing before and after is ignored between paragraphs of the same
    /// style.
    pub contextual: bool,
    /// Kept on the same page as the next paragraph.
    pub keep_next: bool,
    /// Kept whole on one page.
    pub keep_lines: bool,
    /// Starts a new page.
    pub page_break_before: bool,
    /// No single line of it at the top or bottom of a page.
    pub widow_control: bool,
    /// Tab stops, by position.
    pub tabs: Vec<TabStop>,
    /// Borders.
    pub borders: ParaBorders,
    /// Background colour.
    pub shading: Option<Rgb>,
    /// Outline level (0 for a heading of level 1).
    pub outline: Option<u8>,
    /// Right-to-left paragraph.
    pub bidi: bool,
    /// Lines snap to the document grid.
    pub snap_to_grid: bool,
    /// No automatic hyphenation.
    pub suppress_hyphens: bool,
    /// The paragraph is in a frame (a positioned box of paragraphs).
    pub frame: Option<Frame>,
}

/// A frame: paragraphs taken out of the text and placed like a text box
/// (`w:framePr`).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    /// Width; 0 when it is that of the text.
    pub width: f32,
    /// Height; 0 when it is that of the text.
    pub height: f32,
    /// The height is exact (else at least `height`).
    pub exact: bool,
    /// Where it is placed and how text flows around it.
    pub anchor: Anchor,
}

impl Default for Frame {
    fn default() -> Self {
        let pos = |from| AnchorPos {
            from,
            align: None,
            offset: 0.0,
        };
        Frame {
            width: 0.0,
            height: 0.0,
            exact: false,
            anchor: Anchor {
                horizontal: pos(AnchorFrom::Column),
                vertical: pos(AnchorFrom::Paragraph),
                wrap: Wrap::Square,
                behind: false,
                dist: [0.0; 4],
            },
        }
    }
}

/// A tab stop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabStop {
    /// Position, from the left edge of the text column.
    pub pos: f32,
    /// Alignment of the text at the stop.
    pub kind: TabKind,
    /// The character that fills the space before the stop.
    pub leader: Option<char>,
}

/// Alignment of the text at a tab stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabKind {
    /// Text starts at the stop.
    Left,
    /// Text is centred on the stop.
    Center,
    /// Text ends at the stop.
    Right,
    /// The decimal point is at the stop.
    Decimal,
    /// A vertical bar is drawn at the stop (no stop for text).
    Bar,
}

/// A border line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    /// Line width.
    pub width: f32,
    /// Distance to the text.
    pub space: f32,
    /// Colour.
    pub color: Rgb,
    /// Style.
    pub style: BorderStyle,
}

/// Style of a border line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    /// One solid line.
    Single,
    /// Two lines.
    Double,
    /// Dots.
    Dotted,
    /// Dashes.
    Dashed,
}

/// The borders of a paragraph.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ParaBorders {
    /// Above.
    pub top: Option<Border>,
    /// Below.
    pub bottom: Option<Border>,
    /// Left.
    pub left: Option<Border>,
    /// Right.
    pub right: Option<Border>,
    /// Between paragraphs with the same borders.
    pub between: Option<Border>,
}

/// Vertical position of text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VertAlign {
    /// On the baseline.
    #[default]
    Baseline,
    /// Superscript.
    Super,
    /// Subscript.
    Sub,
}

/// Underline style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Underline {
    /// One line.
    Single,
    /// Under words only.
    Words,
    /// Two lines.
    Double,
    /// A thick line.
    Thick,
    /// Dots.
    Dotted,
    /// Dashes.
    Dashed,
    /// A wave.
    Wave,
}

/// The properties of a run of text, resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct RunFmt {
    /// Font of Latin text (ASCII range).
    pub font: String,
    /// Font of other Latin text (`w:hAnsi`).
    pub font_hansi: String,
    /// Font of East Asian text.
    pub font_east_asia: String,
    /// Font of complex scripts (Arabic, Hebrew, Thai).
    pub font_cs: String,
    /// Font size.
    pub size: f32,
    /// Font size of complex scripts.
    pub size_cs: f32,
    /// Bold.
    pub bold: bool,
    /// Italic.
    pub italic: bool,
    /// Underline.
    pub underline: Option<Underline>,
    /// Underline colour (the text colour when `None`).
    pub underline_color: Option<Rgb>,
    /// Struck through.
    pub strike: bool,
    /// Struck through twice.
    pub dstrike: bool,
    /// Text colour (`None` for automatic: black).
    pub color: Option<Rgb>,
    /// Highlight colour.
    pub highlight: Option<Rgb>,
    /// Background colour.
    pub shading: Option<Rgb>,
    /// Superscript or subscript.
    pub vert: VertAlign,
    /// Raised (positive) or lowered by this much.
    pub position: f32,
    /// All capitals.
    pub caps: bool,
    /// Small capitals.
    pub small_caps: bool,
    /// Extra space after each character.
    pub spacing: f32,
    /// Horizontal scale, in percent.
    pub scale: f32,
    /// Kerning from this font size on (`None`: no kerning).
    pub kern_from: Option<f32>,
    /// Hidden text.
    pub hidden: bool,
    /// Right-to-left text.
    pub rtl: bool,
    /// Complex-script formatting applies (`w:cs`).
    pub cs: bool,
    /// Link target: a web address, or `#bookmark`.
    pub href: Option<String>,
}

impl Default for RunFmt {
    /// Word's defaults: Times New Roman, 10 pt.
    fn default() -> Self {
        RunFmt {
            font: "Times New Roman".into(),
            font_hansi: "Times New Roman".into(),
            font_east_asia: "Times New Roman".into(),
            font_cs: "Times New Roman".into(),
            size: 10.0,
            size_cs: 10.0,
            bold: false,
            italic: false,
            underline: None,
            underline_color: None,
            strike: false,
            dstrike: false,
            color: None,
            highlight: None,
            shading: None,
            vert: VertAlign::Baseline,
            position: 0.0,
            caps: false,
            small_caps: false,
            spacing: 0.0,
            scale: 100.0,
            kern_from: None,
            hidden: false,
            rtl: false,
            cs: false,
            href: None,
        }
    }
}

/// A break in a paragraph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
    /// A line break.
    Line,
    /// A page break.
    Page,
    /// A column break.
    Column,
}

/// A field whose result depends on the layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// The page number.
    Page,
    /// The number of pages.
    NumPages,
    /// The number of pages of the section.
    SectionPages,
}

/// A piece of the content of a paragraph.
#[derive(Debug, Clone)]
pub enum Inline {
    /// Text.
    Text {
        /// The text.
        text: String,
        /// Its properties.
        fmt: RunFmt,
    },
    /// A tab.
    Tab {
        /// Its properties (for the leader and the line height).
        fmt: RunFmt,
    },
    /// A break.
    Break {
        /// What it breaks.
        kind: BreakKind,
        /// Its properties.
        fmt: RunFmt,
    },
    /// A page number or page count.
    Field {
        /// The field.
        kind: FieldKind,
        /// Its properties.
        fmt: RunFmt,
        /// Number format (`None`: the section's page number format).
        format: Option<NumFormat>,
    },
    /// A picture.
    Image {
        /// Index into [`FDoc::images`]; `None` when it cannot be drawn.
        index: Option<usize>,
        /// Width.
        width: f32,
        /// Height.
        height: f32,
        /// Where it floats; `None` when it is in the line.
        anchor: Option<Anchor>,
        /// Its properties (for the line height).
        fmt: RunFmt,
        /// Its description.
        alt: String,
        /// Share of the picture cut off at each side: left, top, right,
        /// bottom (0 to 1).
        crop: [f32; 4],
    },
    /// A shape: a text box, a rectangle, a line.
    Shape(Box<Shape>),
    /// The mark of a footnote or endnote.
    NoteRef {
        /// It is an endnote.
        endnote: bool,
        /// Its id in the file.
        id: String,
        /// Its properties.
        fmt: RunFmt,
    },
    /// In a note: its own number.
    NoteNumber {
        /// Its properties.
        fmt: RunFmt,
    },
    /// A formula.
    Math {
        /// Its structure.
        node: MathNode,
        /// It is displayed on its own line.
        display: bool,
        /// The properties of its text.
        fmt: RunFmt,
    },
}

/// A shape with its content.
#[derive(Debug, Clone)]
pub struct Shape {
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
    /// Where it floats; `None` when it is in the line.
    pub anchor: Option<Anchor>,
    /// Fill colour.
    pub fill: Option<Rgb>,
    /// Outline.
    pub line: Option<Border>,
    /// Its text, for a text box.
    pub blocks: Vec<Block>,
    /// Inner margins of the text: left, top, right, bottom.
    pub insets: [f32; 4],
    /// Pictures drawn in it (a group), with their position relative to the
    /// shape: (index into [`FDoc::images`], x, y, width, height).
    pub pictures: Vec<(usize, f32, f32, f32, f32)>,
    /// The height grows with the text.
    pub auto_height: bool,
    /// Its properties (for the line height when it is in the line).
    pub fmt: RunFmt,
}

/// Where a floating object is placed and how text flows around it.
#[derive(Debug, Clone, PartialEq)]
pub struct Anchor {
    /// Horizontal position.
    pub horizontal: AnchorPos,
    /// Vertical position.
    pub vertical: AnchorPos,
    /// How text flows around it.
    pub wrap: Wrap,
    /// It is drawn behind the text.
    pub behind: bool,
    /// Distance kept from the text: top, bottom, left, right.
    pub dist: [f32; 4],
}

/// One coordinate of a floating object.
#[derive(Debug, Clone, PartialEq)]
pub struct AnchorPos {
    /// What it is measured from.
    pub from: AnchorFrom,
    /// Alignment in that area, instead of an offset.
    pub align: Option<AnchorAlign>,
    /// Offset from the start of that area.
    pub offset: f32,
}

/// What a floating object is placed relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorFrom {
    /// The page edge.
    Page,
    /// The page margins.
    Margin,
    /// The column (horizontal).
    Column,
    /// The character where it is anchored (horizontal).
    Character,
    /// The paragraph where it is anchored (vertical).
    Paragraph,
    /// The line where it is anchored (vertical).
    Line,
    /// The left or top margin area.
    StartMargin,
    /// The right or bottom margin area.
    EndMargin,
    /// The inside margin.
    InsideMargin,
    /// The outside margin.
    OutsideMargin,
}

/// Alignment of a floating object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorAlign {
    /// Left or top.
    Start,
    /// Centred.
    Center,
    /// Right or bottom.
    End,
    /// Inside.
    Inside,
    /// Outside.
    Outside,
}

/// How text flows around a floating object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// Text ignores it (in front of or behind the text).
    None,
    /// Text flows around its box.
    Square,
    /// Text flows around its outline (here: its box).
    Tight,
    /// Text stays above and below it.
    TopBottom,
}

/// A table.
#[derive(Debug, Clone, Default)]
pub struct Table {
    /// Table properties.
    pub props: TableProps,
    /// Width of each grid column, as Word last laid it out.
    pub grid: Vec<f32>,
    /// Its rows.
    pub rows: Vec<Row>,
}

/// Width of a table or cell.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Width {
    /// Chosen from the content.
    #[default]
    Auto,
    /// In points.
    Pt(f32),
    /// A fraction (0 to 1) of the available width.
    Pct(f32),
}

/// The properties of a table.
#[derive(Debug, Clone, Default)]
pub struct TableProps {
    /// Preferred width.
    pub width: Width,
    /// Indent from the left margin.
    pub indent: f32,
    /// Alignment on the page.
    pub align: Align,
    /// The column widths are fixed (no autofit).
    pub fixed: bool,
    /// Default cell margins: left, top, right, bottom.
    pub cell_margins: [f32; 4],
    /// Borders: top, left, bottom, right, inside horizontal, inside
    /// vertical.
    pub borders: [Option<Border>; 6],
    /// Space between cells.
    pub cell_spacing: f32,
    /// Where it floats, for a floating table: (x, y) placement.
    pub float: Option<Anchor>,
}

/// A row of a table.
#[derive(Debug, Clone, Default)]
pub struct Row {
    /// Its height and whether it is exact (else a minimum).
    pub height: Option<(f32, bool)>,
    /// It is repeated at the top of every page.
    pub header: bool,
    /// It is not split across pages.
    pub cant_split: bool,
    /// Grid columns skipped before its first cell.
    pub grid_before: usize,
    /// Grid columns left after its last cell.
    pub grid_after: usize,
    /// Its cells.
    pub cells: Vec<Cell>,
}

/// Vertical merging of a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VMerge {
    /// Not merged.
    #[default]
    None,
    /// The first of merged cells.
    Restart,
    /// Merged into the cell above.
    Continue,
}

/// Vertical alignment of the content of a cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VAlign {
    /// Top.
    #[default]
    Top,
    /// Centre.
    Center,
    /// Bottom.
    Bottom,
}

/// A cell of a table.
#[derive(Debug, Clone, Default)]
pub struct Cell {
    /// Number of grid columns it spans.
    pub span: usize,
    /// Vertical merging.
    pub vmerge: VMerge,
    /// Preferred width.
    pub width: Width,
    /// Borders: top, left, bottom, right (resolved with the table's).
    pub borders: [Option<Border>; 4],
    /// Background colour.
    pub shading: Option<Rgb>,
    /// Margins: left, top, right, bottom.
    pub margins: [f32; 4],
    /// Vertical alignment.
    pub valign: VAlign,
    /// Its content.
    pub blocks: Vec<Block>,
}

/// A page number written in `format`.
pub fn format_page(number: u32, format: &NumFormat) -> String {
    match format {
        NumFormat::Bullet | NumFormat::None => number.to_string(),
        other => super::styles::format_number(number, other),
    }
}
