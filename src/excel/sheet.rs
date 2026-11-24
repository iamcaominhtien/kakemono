//! A worksheet part (`xl/worksheets/sheetN.xml`) read into a [`Grid`]:
//! the cells (`sheetData`), with their text formatted as Excel shows it,
//! the merged ranges, the hyperlinks, the hidden rows and columns, and
//! what the sheet declares about its tables (autofilter, frozen rows).
//!
//! The part is read as a stream ([`pull`](super::pull)), not as a tree:
//! a sheet can hold millions of cells. Its declared size (`dimension`) is
//! not trusted: only the cells there are take memory, and only up to the
//! limits of rows and columns of [`ExcelConfig`].

use crate::model::Span;
use crate::model::style::{BOLD, ITALIC};
use crate::word::docx::parse;
use crate::word::xml::REL_NS;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::ExcelConfig;
use super::drawing;
use super::grid::{CellKind, EmbeddedObject, Grid, GridCell, Merge, MergeIndex, Table};
use super::numfmt;
use super::pull::{Event, Reader, Tag};
use super::strings;
use super::styles::Styles;
use super::workbook::{REL_STRICT_NS, SML_NS, SML_STRICT_NS, is_sml};

/// The size of a pixel at 96 per inch, in points.
const PT_PER_PIXEL: f32 = 0.75;
/// Most embedded objects read on a sheet.
const MAX_EMBEDDED_OBJECTS: usize = 1000;
/// Most letters of a column in a cell reference (`XFD`).
const MAX_COLUMN_LETTERS: usize = 3;
/// The width of a digit of Excel's default font (Calibri 11), in pixels:
/// the unit of column widths.
const DIGIT_PX: f32 = 7.0;
/// The padding of a column, in pixels, which `baseColWidth` leaves out.
const COL_PADDING_PX: f32 = 5.0;
/// Points in a pixel at 96 pixels per inch.
const PT_PER_PX: f32 = 0.75;
/// Points in an inch: the unit of `pageMargins`.
const PT_PER_INCH: f32 = 72.0;
/// The widest margin kept, in inches.
const MAX_MARGIN_INCHES: f32 = 20.0;
/// The scales Excel allows, in percent.
const MIN_SCALE_PERCENT: u32 = 10;
/// See [`MIN_SCALE_PERCENT`].
const MAX_SCALE_PERCENT: u32 = 400;
/// The most manual page breaks kept (Excel allows 1 026).
const MAX_BREAKS: usize = 2_048;
/// The largest column width (255 characters) or row height (409 points)
/// Excel allows, with some room.
const MAX_SIZE: f32 = 1_000.0;

/// What a sheet's cells refer to outside the sheet part.
pub(crate) struct SheetContext<'a> {
    /// The shared strings of the workbook.
    pub(crate) strings: &'a [Vec<Span>],
    /// The cell formats of the workbook.
    pub(crate) styles: &'a Styles,
    /// Dates count from 1904.
    pub(crate) date1904: bool,
    /// The external hyperlinks of the sheet's relationships: URL by
    /// relationship id (a `hyperlink`'s `r:id`).
    pub(crate) links: HashMap<String, String>,
    /// The Excel Tables of the sheet: range and header rows.
    pub(crate) tables: Vec<Table>,
    /// The print title rows of the sheet.
    pub(crate) print_title_rows: Option<(u32, u32)>,
    /// The limits of rows and columns.
    pub(crate) config: &'a ExcelConfig,
    /// The sheet is read for the sheet view: the cell formats of every
    /// cell, row and column are kept ([`Grid::looks`]), and the height of
    /// every row the file gives, not only the custom ones.
    pub(crate) view: bool,
}

/// The limits of a sheet, as cell indices: rows and columns at or past
/// them are not read, nor the rows after the one that fills `cells`.
#[derive(Clone, Copy)]
struct Limits {
    /// The first row not read.
    rows: u32,
    /// The first column not read.
    cols: u32,
    /// The most cells kept.
    cells: usize,
}

impl Limits {
    /// Whether the cell `(row, col)` is read.
    fn contain(self, (row, col): (u32, u32)) -> bool {
        row < self.rows && col < self.cols
    }
}

/// The namespace prefixes the root element of a part declares.
struct Prefixes<'a> {
    /// The prefixes of SpreadsheetML (`""` for the default namespace).
    sml: Vec<&'a str>,
    /// The prefixes of relationship ids.
    rel: Vec<&'a str>,
    /// The namespace declarations of the root, as written, to read a
    /// piece of the part on its own.
    declarations: String,
}

impl<'a> Prefixes<'a> {
    /// The prefixes `root` declares.
    fn of(root: &Tag<'a>) -> Self {
        let mut prefixes = Self {
            sml: Vec::new(),
            rel: Vec::new(),
            declarations: String::new(),
        };
        for (name, uri) in root.attributes() {
            let prefix = match name.strip_prefix("xmlns") {
                Some("") => "",
                Some(rest) => match rest.strip_prefix(':') {
                    Some(prefix) => prefix,
                    None => continue,
                },
                None => continue,
            };
            if matches!(uri.as_ref(), SML_NS | SML_STRICT_NS) {
                prefixes.sml.push(prefix);
            } else if matches!(uri.as_ref(), REL_NS | REL_STRICT_NS) {
                prefixes.rel.push(prefix);
            }
            // Written again with the value as it was read.
            let quoted = uri.replace('&', "&amp;").replace('"', "&quot;");
            prefixes.declarations.push_str(&format!(" {name}=\"{quoted}\""));
        }
        prefixes
    }

    /// Whether `tag` is a SpreadsheetML element.
    fn is_sml(&self, tag: &Tag<'_>) -> bool {
        self.sml.contains(&tag.prefix())
    }

    /// The relationship id (`r:id`) of `tag`.
    fn rel_id(&self, tag: &Tag<'_>) -> Option<String> {
        tag.attributes()
            .find(|(name, _)| {
                name.split_once(':')
                    .is_some_and(|(prefix, local)| local == "id" && self.rel.contains(&prefix))
            })
            .map(|(_, value)| value.into_owned())
    }
}

/// Where an element is, for the elements the reader acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// The root element.
    Root,
    /// `sheetData`.
    SheetData,
    /// A row.
    Row,
    /// A cell.
    Cell,
    /// The value of a cell.
    Value,
    /// `cols`.
    Cols,
    /// `mergeCells`.
    MergeCells,
    /// `sheetViews`.
    SheetViews,
    /// The first `sheetView`.
    SheetView,
    /// `hyperlinks`.
    Hyperlinks,
    /// `sheetPr`.
    SheetPr,
    /// `rowBreaks`.
    RowBreaks,
    /// `colBreaks`.
    ColBreaks,
    /// Any other element.
    Other,
}

/// A cell being read.
#[derive(Debug, Default)]
struct RawCell {
    /// Where it is; `None` when it is past the limits.
    at: Option<(u32, u32)>,
    /// Its column, when it is in a row read but past the limit of columns.
    past_col: Option<u32>,
    /// Its type (`t`).
    kind: Option<String>,
    /// Its style index (`s`).
    style: usize,
    /// Its value (`v`).
    value: Option<String>,
    /// Its inline string (`is`).
    inline: Option<Vec<Span>>,
}

/// What the reader of a sheet has read so far.
struct SheetReader<'c, 'a> {
    /// The context of the sheet.
    ctx: &'c SheetContext<'a>,
    /// The limits of rows and columns.
    limits: Limits,
    /// The grid read so far.
    grid: Grid,
    /// The index of the row a row without a reference has.
    next_row: u32,
    /// The current row is past the limit of rows.
    row_past_limit: bool,
    /// The current row has a value.
    row_has_value: bool,
    /// The index of the column a cell without a reference has.
    next_col: u32,
    /// The current row's index.
    row: u32,
    /// The cell being read.
    cell: Option<RawCell>,
    /// The text of the value being read.
    value: String,
    /// The merged ranges, in the order of the file.
    merges: Vec<Merge>,
    /// The hyperlinks: range and URL (for the sheet view, also the
    /// internal ones: `#` and their place).
    links: Vec<(Merge, String)>,
    /// A sheet view has been read (only the first counts).
    seen_view: bool,
    /// The columns past the limit with a value, in the rows read.
    cols_past_limit: HashSet<u32>,
}

/// Reads the worksheet `xml`.
///
/// # Errors
/// When `xml` is not well-formed, or has a document type declaration.
pub(crate) fn read(xml: &str, ctx: &SheetContext<'_>) -> Result<Grid> {
    read_part(xml, ctx).context("reading a worksheet")
}

/// Reads the worksheet `xml` (see [`read`]).
fn read_part(xml: &str, ctx: &SheetContext<'_>) -> Result<Grid> {
    let mut reader = Reader::new(xml);
    let root = loop {
        match reader.next_event()? {
            Some(Event::Start(tag)) => break tag,
            Some(_) => {}
            None => return Ok(Grid::default()),
        }
    };
    let prefixes = Prefixes::of(&root);
    let mut sheet = SheetReader {
        ctx,
        limits: Limits {
            rows: u32::try_from(ctx.config.max_rows).unwrap_or(u32::MAX),
            cols: u32::try_from(ctx.config.max_cols).unwrap_or(u32::MAX),
            cells: ctx.config.max_cells,
        },
        grid: Grid::default(),
        next_row: 0,
        row_past_limit: false,
        row_has_value: false,
        next_col: 0,
        row: 0,
        cell: None,
        value: String::new(),
        merges: Vec::new(),
        links: Vec::new(),
        seen_view: false,
        cols_past_limit: HashSet::new(),
    };
    let mut stack = vec![Place::Root];
    if root.empty {
        stack.clear();
    }
    while let Some(event) = reader.next_event()? {
        match event {
            Event::Start(tag) => {
                let parent = stack.last().copied().unwrap_or(Place::Other);
                let place = if prefixes.is_sml(&tag) {
                    if parent == Place::Cell && tag.local() == "is" {
                        let spans = inline_string(&mut reader, &tag, &prefixes)?;
                        sheet.row_has_value = true;
                        if let Some(cell) = &mut sheet.cell {
                            cell.inline.get_or_insert(spans);
                        }
                        continue;
                    }
                    if parent == Place::Root && tag.local() == "oleObjects" {
                        let piece = subtree(&mut reader, &tag, &prefixes)?;
                        sheet.grid.objects = piece.as_deref().map(embedded_objects).unwrap_or_default();
                        continue;
                    }
                    sheet.start(&tag, parent, &prefixes)
                } else {
                    Place::Other
                };
                if tag.empty {
                    sheet.finish(place);
                } else {
                    stack.push(place);
                }
            }
            Event::End(_) => {
                if let Some(place) = stack.pop() {
                    sheet.finish(place);
                }
            }
            Event::Text(text) => {
                if stack.last() == Some(&Place::Value) {
                    sheet.value.push_str(&text);
                }
            }
        }
    }
    Ok(sheet.done())
}

/// The spans of an inline string (`is`), whose start tag `tag` the reader
/// has just given: the element is read on its own as a tree, as a shared
/// string is. The reader is left after its end tag.
fn inline_string(reader: &mut Reader<'_>, tag: &Tag<'_>, prefixes: &Prefixes<'_>) -> Result<Vec<Span>> {
    let Some(piece) = subtree(reader, tag, prefixes)? else {
        return Ok(Vec::new());
    };
    let doc = parse(&piece)?;
    Ok(doc
        .root_element()
        .children()
        .find(|node| is_sml(*node, "is"))
        .map(strings::string_item)
        .unwrap_or_default())
}

/// The element whose start tag `tag` the reader has just given, as a
/// document of its own: wrapped in an element declaring the prefixes of
/// the sheet's root; `None` for an empty element. The reader is left after
/// its end tag.
fn subtree(reader: &mut Reader<'_>, tag: &Tag<'_>, prefixes: &Prefixes<'_>) -> Result<Option<String>> {
    if tag.empty {
        return Ok(None);
    }
    let mut depth = 1usize;
    let end = loop {
        match reader.next_event()?.context("damaged XML: an unclosed element")? {
            Event::Start(inner) if !inner.empty => depth += 1,
            Event::End(end) => {
                depth -= 1;
                if depth == 0 {
                    break end;
                }
            }
            _ => {}
        }
    };
    Ok(Some(format!(
        "<x{}>{}</x>",
        prefixes.declarations,
        reader.slice(tag.start, end)
    )))
}

/// The embedded objects (`oleObjects`) of the document `piece` that give
/// a picture and an anchor (`objectPr`, Excel 2010 on): the relationship of
/// the picture and the corners of the anchor. A damaged piece gives none,
/// and so do objects past [`MAX_EMBEDDED_OBJECTS`].
fn embedded_objects(piece: &str) -> Vec<EmbeddedObject> {
    let Ok(doc) = parse(piece) else {
        return Vec::new();
    };
    doc.descendants()
        .filter(|node| is_sml(*node, "objectPr"))
        .filter_map(|props| {
            let rel = props
                .attribute((REL_NS, "id"))
                .or_else(|| props.attribute((REL_STRICT_NS, "id")))?;
            let anchor = props.children().find(|node| is_sml(*node, "anchor"))?;
            let (from, to) = (drawing::marker(anchor, "from")?, drawing::marker(anchor, "to")?);
            Some(EmbeddedObject {
                rel: rel.to_string(),
                from,
                to,
            })
        })
        .take(MAX_EMBEDDED_OBJECTS)
        .collect()
}

impl SheetReader<'_, '_> {
    /// Acts on the start tag `tag`, a SpreadsheetML element in `parent`:
    /// where it is.
    fn start(&mut self, tag: &Tag<'_>, parent: Place, prefixes: &Prefixes<'_>) -> Place {
        if let Some(place) = self.start_print(tag, parent) {
            return place;
        }
        match (parent, tag.local()) {
            (Place::Root, "sheetData") => Place::SheetData,
            (Place::Root, "cols") => Place::Cols,
            (Place::Root, "mergeCells") => Place::MergeCells,
            (Place::Root, "sheetViews") => Place::SheetViews,
            (Place::Root, "hyperlinks") => Place::Hyperlinks,
            (Place::Root, "sheetFormatPr") => {
                self.sheet_format(tag);
                Place::Other
            }
            (Place::Root, "autoFilter") => {
                self.grid.declared.auto_filter = tag.attr("ref").and_then(|area| range(&area));
                Place::Other
            }
            (Place::SheetData, "row") => {
                self.start_row(tag);
                Place::Row
            }
            (Place::Row, "c") => {
                self.start_cell(tag);
                Place::Cell
            }
            (Place::Cell, "v") => {
                self.row_has_value = true;
                if self.cell.as_ref().is_some_and(|cell| cell.value.is_none()) {
                    self.value.clear();
                    Place::Value
                } else {
                    Place::Other
                }
            }
            (Place::Cols, "col") => {
                self.hide_cols(tag);
                self.size_cols(tag);
                Place::Other
            }
            (Place::MergeCells, "mergeCell") => {
                if let Some(merge) = tag.attr("ref").and_then(|area| range(&area)) {
                    self.merges.push(merge);
                }
                Place::Other
            }
            (Place::SheetViews, "sheetView") if !self.seen_view => {
                self.seen_view = true;
                self.grid.looks.no_grid_lines = matches!(tag.attr("showGridLines").as_deref(), Some("0" | "false"));
                Place::SheetView
            }
            (Place::SheetView, "pane") => {
                self.grid.declared.frozen_rows = frozen(tag);
                Place::Other
            }
            (Place::Hyperlinks, "hyperlink") => {
                // An internal link (`location`, a place in the workbook)
                // has no relationship.
                let url = prefixes.rel_id(tag).and_then(|id| self.ctx.links.get(&id)).cloned();
                // The sheet view keeps internal links, as `#` and the place.
                let url = url.or_else(|| {
                    self.ctx
                        .view
                        .then(|| tag.attr("location").map(|place| format!("#{place}")))
                        .flatten()
                });
                let area = tag.attr("ref").and_then(|area| range(&area));
                if let (Some(url), Some(area)) = (url, area) {
                    self.links.push((area, url));
                }
                Place::Other
            }
            _ => Place::Other,
        }
    }

    /// Acts on the start tag `tag` in `parent` when it is one of the print
    /// settings: where it is; `None` for another element.
    fn start_print(&mut self, tag: &Tag<'_>, parent: Place) -> Option<Place> {
        Some(match (parent, tag.local()) {
            (Place::Root, "pageSetup") => {
                self.grid.paper_pt = tag
                    .attr("paperSize")
                    .and_then(|code| code.parse::<u32>().ok())
                    .and_then(paper_pt);
                self.page_setup(tag);
                Place::Other
            }
            (Place::Root, "pageMargins") => {
                let print = &mut self.grid.print;
                for (index, name) in ["left", "right", "top", "bottom"].iter().enumerate() {
                    if let Some(inches) = tag
                        .attr(name)
                        .and_then(|value| value.trim().parse::<f32>().ok())
                        .filter(|inches| inches.is_finite() && (0.0..=MAX_MARGIN_INCHES).contains(inches))
                    {
                        print.margins_pt[index] = inches * PT_PER_INCH;
                    }
                }
                Place::Other
            }
            (Place::Root, "printOptions") => {
                let print = &mut self.grid.print;
                print.h_centered = is_on(tag.attr("horizontalCentered").as_deref());
                print.v_centered = is_on(tag.attr("verticalCentered").as_deref());
                print.grid_lines = is_on(tag.attr("gridLines").as_deref());
                Place::Other
            }
            (Place::Root, "sheetPr") => Place::SheetPr,
            (Place::SheetPr, "pageSetUpPr") => {
                self.grid.print.fit_to_page = is_on(tag.attr("fitToPage").as_deref());
                Place::Other
            }
            (Place::Root, "rowBreaks") => Place::RowBreaks,
            (Place::Root, "colBreaks") => Place::ColBreaks,
            (Place::RowBreaks | Place::ColBreaks, "brk") => {
                if let Some(id) = tag.attr("id").and_then(|value| value.parse::<u32>().ok())
                    && self.grid.print.row_breaks.len() + self.grid.print.col_breaks.len() < MAX_BREAKS
                {
                    if parent == Place::RowBreaks {
                        self.grid.print.row_breaks.push(id);
                    } else {
                        self.grid.print.col_breaks.push(id);
                    }
                }
                Place::Other
            }
            _ => return None,
        })
    }

    /// Reads the orientation, scale, fitting and page order of
    /// `pageSetup`.
    fn page_setup(&mut self, tag: &Tag<'_>) {
        let print = &mut self.grid.print;
        print.landscape = tag.attr("orientation").as_deref() == Some("landscape");
        print.over_then_down = tag.attr("pageOrder").as_deref() == Some("overThenDown");
        let number = |name: &str| tag.attr(name).and_then(|value| value.trim().parse::<u32>().ok());
        if let Some(scale) = number("scale") {
            print.scale_percent = scale.clamp(MIN_SCALE_PERCENT, MAX_SCALE_PERCENT) as f32;
        }
        if let Some(pages) = number("fitToWidth") {
            print.fit_width = pages;
        }
        if let Some(pages) = number("fitToHeight") {
            print.fit_height = pages;
        }
    }

    /// Starts the row `tag`.
    fn start_row(&mut self, tag: &Tag<'_>) {
        let index = tag
            .attr("r")
            .and_then(|value| value.parse::<u32>().ok())
            .and_then(|value| value.checked_sub(1))
            .unwrap_or(self.next_row);
        self.next_row = index.saturating_add(1);
        self.row = index;
        self.next_col = 0;
        self.row_has_value = false;
        // A row is read whole: the one that fills the cells is the last.
        self.row_past_limit = index >= self.limits.rows || self.grid.cells.len() >= self.limits.cells;
        if !self.row_past_limit && is_on(tag.attr("hidden").as_deref()) {
            self.grid.hidden_rows.insert(index);
        }
        if !self.row_past_limit
            && (self.ctx.view || is_on(tag.attr("customHeight").as_deref()))
            && let Some(height) = tag.attr("ht").and_then(|value| points(&value)).map(whole_pixels)
        {
            self.grid.sizes.rows.insert(index, height);
        }
        if self.ctx.view && !self.row_past_limit && is_on(tag.attr("customHeight").as_deref()) {
            self.grid.looks.custom_rows.insert(index);
        }
        if self.ctx.view
            && !self.row_past_limit
            && is_on(tag.attr("customFormat").as_deref())
            && let Some(style) = tag.attr("s").and_then(|value| value.parse::<u32>().ok())
        {
            self.grid.looks.rows.insert(index, style);
        }
    }

    /// Reads the default column width and row height of `sheetFormatPr`.
    fn sheet_format(&mut self, tag: &Tag<'_>) {
        let sizes = &mut self.grid.sizes;
        sizes.default_row_pt = tag
            .attr("defaultRowHeight")
            .and_then(|value| points(&value))
            .map(whole_pixels);
        sizes.default_col_pt = tag
            .attr("defaultColWidth")
            .and_then(|value| points(&value))
            .map(col_width_pt)
            .or_else(|| {
                let base = tag.attr("baseColWidth").and_then(|value| points(&value))?;
                Some((base * DIGIT_PX + COL_PADDING_PX) * PT_PER_PX)
            });
    }

    /// Records the width of the columns of `col` (one-based `min` and
    /// `max`), when it gives one.
    fn size_cols(&mut self, tag: &Tag<'_>) {
        let bound = |name| tag.attr(name).and_then(|value| value.parse::<u32>().ok());
        let width = tag.attr("width").and_then(|value| points(&value)).map(col_width_pt);
        if let (Some(min), Some(max), Some(width)) = (bound("min"), bound("max"), width)
            && min >= 1
            && max >= min
        {
            self.grid.sizes.cols.push((min - 1, max - 1, width));
        }
        if self.ctx.view
            && let (Some(min), Some(max), Some(style)) = (bound("min"), bound("max"), bound("style"))
            && min >= 1
            && max >= min
            && style != 0
        {
            self.grid.looks.cols.push((min - 1, max - 1, style));
        }
    }

    /// Starts the cell `tag`.
    fn start_cell(&mut self, tag: &Tag<'_>) {
        let at = tag
            .attr("r")
            .and_then(|reference| cell_ref(&reference))
            .unwrap_or((self.row, self.next_col));
        self.next_col = at.1.saturating_add(1);
        let in_row = !self.row_past_limit;
        if in_row
            && self.limits.contain(at)
            && let Some(vm) = tag.attr("vm").and_then(|value| value.parse::<u32>().ok())
        {
            self.grid.cell_pictures.push((at, vm));
        }
        self.cell = Some(RawCell {
            at: (in_row && self.limits.contain(at)).then_some(at),
            past_col: (in_row && at.1 >= self.limits.cols).then_some(at.1),
            kind: tag.attr("t").map(|kind| kind.into_owned()),
            style: tag.attr("s").and_then(|value| value.parse::<usize>().ok()).unwrap_or(0),
            value: None,
            inline: None,
        });
    }

    /// Records the hidden columns of `col` (one-based `min` and `max`).
    fn hide_cols(&mut self, tag: &Tag<'_>) {
        if !is_on(tag.attr("hidden").as_deref()) {
            return;
        }
        let bound = |name| tag.attr(name).and_then(|value| value.parse::<u32>().ok());
        if let (Some(min), Some(max)) = (bound("min"), bound("max")) {
            let last = max.min(self.limits.cols);
            self.grid.hidden_cols.extend(min.saturating_sub(1)..last);
        }
    }

    /// Ends an element at `place`.
    fn finish(&mut self, place: Place) {
        match place {
            Place::Row => {
                if self.row_past_limit && self.row_has_value {
                    self.grid.truncated_rows = self.grid.truncated_rows.saturating_add(1);
                }
            }
            Place::Value => {
                if let Some(cell) = &mut self.cell {
                    cell.value = Some(strings::unescape(&self.value));
                }
            }
            Place::Cell => {
                let Some(raw) = self.cell.take() else {
                    return;
                };
                if let Some(col) = raw.past_col {
                    // Counted as a column with a value, as a row is: an
                    // empty string or a formula without its value too.
                    if raw.value.is_some() || raw.inline.is_some() {
                        self.cols_past_limit.insert(col);
                    }
                    return;
                }
                if self.ctx.view
                    && let Some(at) = raw.at
                    && raw.style != 0
                {
                    self.grid.looks.cells.insert(at, u32::try_from(raw.style).unwrap_or(0));
                }
                if let Some(at) = raw.at
                    && let Some(cell) = read_cell(raw, self.ctx)
                {
                    self.grid.cells.entry(at).or_insert(cell);
                }
            }
            _ => {}
        }
    }

    /// The grid, with the merges, hyperlinks and declarations.
    fn done(mut self) -> Grid {
        self.grid.merges = merges(&self.merges, self.limits);
        self.grid.truncated_cols = u32::try_from(self.cols_past_limit.len()).unwrap_or(u32::MAX);
        link(&self.links, &mut self.grid.cells);
        self.grid.declared.tables.clone_from(&self.ctx.tables);
        self.grid.declared.print_title_rows = self.ctx.print_title_rows;
        self.grid
    }
}

/// The value of a cell; `None` when it has none (a formula without its
/// cached value, an empty string), unless it centres across the
/// selection.
fn read_cell(raw: RawCell, ctx: &SheetContext<'_>) -> Option<GridCell> {
    let xf = ctx.styles.get(raw.style);
    let value = raw.value;
    let (mut spans, kind) = match raw.kind.as_deref().unwrap_or("n") {
        "s" => {
            let index = value?.trim().parse::<usize>().ok();
            let spans = index.and_then(|index| ctx.strings.get(index)).cloned();
            (spans.unwrap_or_default(), CellKind::Text)
        }
        "inlineStr" => (raw.inline.unwrap_or_default(), CellKind::Text),
        "str" => (plain(value.unwrap_or_default()), CellKind::Text),
        "b" => {
            let text = match value?.trim() {
                "1" | "true" => "TRUE",
                _ => "FALSE",
            };
            (plain(text.to_string()), CellKind::Bool)
        }
        "e" => (plain(value.unwrap_or_default()), CellKind::Error),
        "d" => (plain(iso_date_time(value.unwrap_or_default().trim())), CellKind::Date),
        _ => match value {
            Some(raw) => match raw.trim().parse::<f64>() {
                Ok(number) => {
                    let kind = if numfmt::is_date(&xf.num_fmt) {
                        CellKind::Date
                    } else {
                        CellKind::Number
                    };
                    (plain(numfmt::format(number, &xf.num_fmt, ctx.date1904)), kind)
                }
                Err(_) => (plain(raw), CellKind::Text),
            },
            None => (Vec::new(), CellKind::Text),
        },
    };
    spans.retain(|span| !span.text.is_empty());
    if spans.is_empty() && !xf.center_continuous {
        return None;
    }
    let font = (if xf.bold { BOLD } else { 0 }) | (if xf.italic { ITALIC } else { 0 });
    for span in spans.iter_mut().filter(|span| span.style == 0) {
        span.style = font;
    }
    Some(GridCell {
        spans,
        kind,
        center_continuous: xf.center_continuous,
    })
}

/// An ISO date and time (`t="d"`, `2024-01-31T14:05:09.5Z`) written as the
/// dates of number cells are: `2024-01-31 14:05`, the seconds only when
/// they are not 0, the time left out at midnight; a time alone
/// (`T14:05:00`) as `14:05`. A text that is not one is kept as written.
fn iso_date_time(text: &str) -> String {
    let (date, time) = match text.split_once('T') {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    // Hours, minutes and whole seconds, without a fraction or a zone.
    let clock = time.and_then(|time| {
        fn digits(part: &str) -> Option<&str> {
            (part.len() == 2 && part.bytes().all(|byte| byte.is_ascii_digit())).then_some(part)
        }
        let mut parts = time.splitn(3, ':');
        let hours = digits(parts.next()?)?;
        let minutes = digits(parts.next()?)?;
        let seconds = parts.next().map_or(Some("00"), |rest| rest.get(..2).and_then(digits))?;
        Some(match seconds {
            "00" => format!("{hours}:{minutes}"),
            _ => format!("{hours}:{minutes}:{seconds}"),
        })
    });
    match (date.is_empty(), clock) {
        (true, Some(clock)) => clock,
        (false, Some(clock)) if clock != "00:00" => format!("{date} {clock}"),
        (false, _) => date.to_string(),
        (true, None) => time.unwrap_or_default().to_string(),
    }
}

/// `text` as one unstyled span, none when it is empty.
fn plain(text: String) -> Vec<Span> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![Span::plain(text)]
    }
}

/// A size of the file (a width in characters, a height in points), when
/// it is a number from 0 to [`MAX_SIZE`].
fn points(value: &str) -> Option<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|size| size.is_finite() && (0.0..=MAX_SIZE).contains(size))
}

/// A row height of `points` as Excel draws and prints it: a whole number
/// of pixels at 96 per inch (`ht="12.95"` is 17 pixels, 12.75 points).
fn whole_pixels(points: f32) -> f32 {
    (points / PT_PER_PIXEL).round() * PT_PER_PIXEL
}

/// The width in points of a column `chars` wide (`col width`, in widths
/// of a digit of the default font, its padding included).
fn col_width_pt(chars: f32) -> f32 {
    (chars * DIGIT_PX).round() * PT_PER_PX
}

/// Whether a boolean attribute is set.
fn is_on(value: Option<&str>) -> bool {
    matches!(value, Some("1" | "true"))
}

/// The merged ranges `list`, cut to the limits; of two that overlap, the
/// first. A new range is compared only with the kept ones of the squares of
/// the sheet it covers ([`MergeIndex`]): since kept ranges never overlap, a
/// square holds few of them.
fn merges(list: &[Merge], limits: Limits) -> Vec<Merge> {
    let mut kept = MergeIndex::default();
    for merge in list {
        if !limits.contain((merge.top, merge.left)) {
            continue;
        }
        let merge = Merge {
            bottom: merge.bottom.min(limits.rows - 1),
            right: merge.right.min(limits.cols - 1),
            ..*merge
        };
        if kept.overlapping(&merge).is_empty() {
            kept.add(merge);
        }
    }
    kept.into_ranges()
}

/// The rows frozen at the top by a sheet view's `pane`.
fn frozen(pane: &Tag<'_>) -> u32 {
    if !matches!(pane.attr("state").as_deref(), Some("frozen" | "frozenSplit")) {
        return 0;
    }
    pane.attr("ySplit")
        .and_then(|rows| rows.parse::<f64>().ok())
        .filter(|rows| rows.is_finite() && *rows >= 0.0)
        // A count of rows, written as a number.
        .map_or(0, |rows| rows.min(f64::from(u32::MAX)) as u32)
}

/// Points the cells of each hyperlink of `links` (range, URL) to its URL;
/// a cell in two keeps the last, as when each overwrites the one before. Only the cells there are visited, by
/// column: a range as wide as the sheet costs no more than the cells in
/// it.
fn link(links: &[(Merge, String)], cells: &mut BTreeMap<(u32, u32), GridCell>) {
    if links.is_empty() {
        return;
    }
    // The cells not linked yet, by column.
    let mut unlinked: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    for &(row, col) in cells.keys() {
        unlinked.entry(col).or_default().insert(row);
    }
    // The last first, so that a cell already linked keeps its link.
    for (area, url) in links.iter().rev() {
        let cols: Vec<u32> = unlinked.range(area.left..=area.right).map(|(col, _)| *col).collect();
        for col in cols {
            let Some(rows) = unlinked.get_mut(&col) else {
                continue;
            };
            let hit: Vec<u32> = rows.range(area.top..=area.bottom).copied().collect();
            for row in hit {
                rows.remove(&row);
                if let Some(cell) = cells.get_mut(&(row, col)) {
                    for span in &mut cell.spans {
                        span.href = Some(url.clone());
                    }
                }
            }
            if rows.is_empty() {
                unlinked.remove(&col);
            }
        }
    }
}

/// The size in points (width, height, portrait) of the paper of Excel's
/// `paperSize` code `code` (ECMA-376, 18.3.1.63): Letter, Tabloid, Legal,
/// Executive, A3, A4, A5, B4 and B5 (JIS); `None` for the others.
fn paper_pt(code: u32) -> Option<(f32, f32)> {
    let (width, height) = match code {
        1 | 2 => (612.0, 792.0),
        3 => (792.0, 1224.0),
        5 => (612.0, 1008.0),
        7 => (522.0, 756.0),
        8 => (841.89, 1190.55),
        9 | 10 => (595.28, 841.89),
        11 => (419.53, 595.28),
        12 => (728.5, 1031.81),
        13 => (515.91, 728.5),
        _ => return None,
    };
    Some((width, height))
}

/// A cell reference (`B2`, `$B$2`) as zero-based (row, column).
pub(crate) fn cell_ref(reference: &str) -> Option<(u32, u32)> {
    let reference = reference.trim();
    let digits = reference.find(|ch: char| ch.is_ascii_digit())?;
    let letters: String = reference[..digits].chars().filter(|ch| *ch != '$').collect();
    if letters.is_empty()
        || letters.len() > MAX_COLUMN_LETTERS
        || !letters.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return None;
    }
    let col = letters.bytes().fold(0u32, |col, byte| {
        col * 26 + u32::from(byte.to_ascii_uppercase() - b'A') + 1
    });
    let row = reference[digits..].parse::<u32>().ok()?.checked_sub(1)?;
    Some((row, col - 1))
}

/// A range (`A1:C3`, or one cell `B2`) as a [`Merge`], its corners in
/// order.
pub(crate) fn range(reference: &str) -> Option<Merge> {
    let (first, last) = reference.split_once(':').unwrap_or((reference, reference));
    let (row1, col1) = cell_ref(first)?;
    let (row2, col2) = cell_ref(last)?;
    Some(Merge {
        top: row1.min(row2),
        left: col1.min(col2),
        bottom: row1.max(row2),
        right: col1.max(col2),
    })
}

#[cfg(test)]
mod tests {
    use super::super::grid::CellKind;
    use super::super::styles;
    use super::*;
    use crate::model::style::BOLD;

    /// `body` as a worksheet.
    fn sheet(body: &str) -> String {
        format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{body}</worksheet>"#
        )
    }

    /// Styles: 0 default, 1 a date, 2 bold, 3 centred across the
    /// selection, 4 two decimals.
    fn test_styles() -> Styles {
        styles::read(concat!(
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">"#,
            r#"<fonts><font/><font><b/></font></fonts>"#,
            r#"<cellXfs><xf numFmtId="0"/><xf numFmtId="14"/><xf numFmtId="0" fontId="1"/>"#,
            r#"<xf numFmtId="0"><alignment horizontal="centerContinuous"/></xf><xf numFmtId="2"/></cellXfs></styleSheet>"#,
        ))
        .unwrap()
    }

    /// Reads `body` with the shared strings `a`, `b` and `config`.
    fn read_with(body: &str, links: &[(&str, &str)], config: &ExcelConfig) -> Grid {
        let strings = vec![vec![Span::plain("a")], vec![Span::plain("b")]];
        let styles = test_styles();
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: links
                .iter()
                .map(|(id, url)| (id.to_string(), url.to_string()))
                .collect(),
            tables: Vec::new(),
            print_title_rows: None,
            config,
            view: false,
        };
        read(&sheet(body), &ctx).unwrap()
    }

    /// Reads `body` with the default config.
    fn grid(body: &str) -> Grid {
        read_with(body, &[], &ExcelConfig::default())
    }

    /// The text and kind of every cell, by position.
    fn cells(grid: &Grid) -> Vec<((u32, u32), String, CellKind)> {
        grid.cells
            .iter()
            .map(|(at, cell)| {
                (
                    *at,
                    cell.spans.iter().map(|span| span.text.as_str()).collect(),
                    cell.kind,
                )
            })
            .collect()
    }

    #[test]
    fn page_setup_names_the_paper() {
        let paper = |setup: &str| grid(&format!("<sheetData/>{setup}")).paper_pt;
        assert_eq!(
            paper(r#"<pageSetup paperSize="1" orientation="landscape"/>"#),
            Some((612.0, 792.0))
        );
        assert_eq!(paper(r#"<pageSetup paperSize="9"/>"#), Some((595.28, 841.89)));
        // No size, or one not known here: the layout's own.
        assert_eq!(paper(r#"<pageSetup orientation="portrait"/>"#), None);
        assert_eq!(paper(r#"<pageSetup paperSize="70"/>"#), None);
    }

    /// `(row, col)`, text, kind.
    fn cell(at: (u32, u32), text: &str, kind: CellKind) -> ((u32, u32), String, CellKind) {
        (at, text.to_string(), kind)
    }

    #[test]
    fn cells_get_their_formatted_text() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1" t="s"><v>1</v></c><c r="B1" t="str"><f>A1</f><v>x</v></c>"#,
            r#"<c r="C1" t="inlineStr"><is><r><rPr><b/></rPr><t>B</t></r><r><t>n</t></r><rPh><t>ふ</t></rPh></is></c>"#,
            r#"<c r="D1" t="b"><v>1</v></c><c r="E1" t="e"><v>#N/A</v></c><c r="F1" s="1"><v>45322</v></c>"#,
            r#"<c r="G1" s="4"><v>3.14159</v></c><c r="H1" t="d"><v>2024-01-31T00:00:00</v></c><c r="I1" t="b"><v>0</v></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [
                cell((0, 0), "b", CellKind::Text),
                cell((0, 1), "x", CellKind::Text),
                cell((0, 2), "Bn", CellKind::Text),
                cell((0, 3), "TRUE", CellKind::Bool),
                cell((0, 4), "#N/A", CellKind::Error),
                cell((0, 5), "2024-01-31", CellKind::Date),
                cell((0, 6), "3.14", CellKind::Number),
                cell((0, 7), "2024-01-31", CellKind::Date),
                cell((0, 8), "FALSE", CellKind::Bool),
            ]
        );
        let runs: Vec<_> = grid.cells[&(0, 2)].spans.iter().map(|span| span.style).collect();
        assert_eq!(runs, [BOLD, 0]);
    }

    #[test]
    fn the_cell_style_makes_plain_text_bold() {
        let grid = grid(r#"<sheetData><row><c t="s" s="2"><v>0</v></c><c s="2"><v>7</v></c></row></sheetData>"#);
        assert_eq!(grid.cells.len(), 2);
        assert!(grid.cells.values().all(|cell| cell.spans[0].style == BOLD));
    }

    #[test]
    fn formula_cells_use_their_cached_value() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1"><f>1+1</f><v>2</v></c><c r="B1"><f>1+2</f></c>"#,
            r#"<c r="C1" t="str"><f>"x"</f></c></row></sheetData>"#,
        ));
        assert_eq!(cells(&grid), [cell((0, 0), "2", CellKind::Number)]);
    }

    #[test]
    fn cells_without_a_reference_follow_the_previous_one() {
        let grid = grid(concat!(
            r#"<sheetData><row r="2"><c r="C2"><v>1</v></c><c><v>2</v></c></row>"#,
            r#"<row><c><v>3</v></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [
                cell((1, 2), "1", CellKind::Number),
                cell((1, 3), "2", CellKind::Number),
                cell((2, 0), "3", CellKind::Number),
            ]
        );
    }

    #[test]
    fn dimension_is_not_trusted() {
        let grid = grid(concat!(
            r#"<dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row>"#,
            r#"<row r="1048576"><c r="XFD1048576"><v>2</v></c><c r="B1048576"><v>3</v></c></row></sheetData>"#,
        ));
        // The last row is past the default limits: only A1 is read, and
        // nothing is allocated for the declared size.
        assert_eq!(cells(&grid), [cell((0, 0), "1", CellKind::Number)]);
        let config = ExcelConfig {
            max_rows: 2_000_000,
            max_cols: 20_000,
            ..ExcelConfig::default()
        };
        let grid = read_with(
            concat!(
                r#"<dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1"><v>1</v></c></row>"#,
                r#"<row r="1048576"><c r="XFD1048576"><v>2</v></c><c r="B1048576"><v>3</v></c></row></sheetData>"#,
            ),
            &[],
            &config,
        );
        assert_eq!(grid.cells.len(), 3);
        assert!(grid.cells.contains_key(&(1_048_575, 16_383)));
    }

    #[test]
    fn hidden_rows_and_columns_are_recorded() {
        let grid = grid(concat!(
            r#"<cols><col min="1" max="1" width="9"/><col min="2" max="3" hidden="1"/></cols>"#,
            r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="2" hidden="1"><c r="A2"><v>2</v></c></row>"#,
            r#"<row r="4" hidden="true"/><row r="5" hidden="0"/></sheetData>"#,
        ));
        assert_eq!(grid.hidden_rows, [1, 3].into());
        assert_eq!(grid.hidden_cols, [1, 2].into());
        assert_eq!(grid.cells.len(), 2);
    }

    #[test]
    fn overlapping_merges_keep_the_first() {
        let grid = grid(concat!(
            r#"<sheetData/><mergeCells><mergeCell ref="A1:B2"/><mergeCell ref="B2:C3"/>"#,
            r#"<mergeCell ref="C1:D1"/><mergeCell ref="bad"/><mergeCell ref="A5:A9"/><mergeCell ref="A4:A5"/></mergeCells>"#,
        ));
        let merge = |top, left, bottom, right| Merge {
            top,
            left,
            bottom,
            right,
        };
        assert_eq!(grid.merges, [merge(0, 0, 1, 1), merge(0, 2, 0, 3), merge(4, 0, 8, 0)]);
    }

    /// The longest a test of a large sheet may take, in seconds: an
    /// unoptimised build is many times slower.
    const LARGE_SHEET_SECONDS: f64 = if cfg!(debug_assertions) { 20.0 } else { 1.0 };

    #[test]
    fn many_merges_after_a_tall_one_are_fast() {
        let mut body = String::from(r#"<sheetData/><mergeCells><mergeCell ref="A1:A100000"/>"#);
        for row in 1..=50_000 {
            body.push_str(&format!(r#"<mergeCell ref="B{row}:C{row}"/>"#));
        }
        // Overlaps the tall one.
        body.push_str(r#"<mergeCell ref="A70000:B70000"/></mergeCells>"#);
        let start = std::time::Instant::now();
        let grid = grid(&body);
        let seconds = start.elapsed().as_secs_f64();
        assert_eq!(grid.merges.len(), 50_001);
        assert!(seconds < LARGE_SHEET_SECONDS, "{seconds} s");
    }

    #[test]
    fn many_hyperlinks_over_the_whole_sheet_are_fast() {
        let mut body = String::from("<sheetData>");
        for row in 1..=20_000 {
            body.push_str(&format!(r#"<row r="{row}">"#));
            for col in ["A", "B", "C", "D", "E"] {
                body.push_str(&format!(r#"<c r="{col}{row}"><v>{row}</v></c>"#));
            }
            body.push_str("</row>");
        }
        body.push_str("</sheetData><hyperlinks>");
        for _ in 0..20_000 {
            body.push_str(r#"<hyperlink ref="A1:XFD1048576" r:id="rId1"/>"#);
        }
        body.push_str("</hyperlinks>");
        let start = std::time::Instant::now();
        let grid = read_with(&body, &[("rId1", "https://example.com/")], &ExcelConfig::default());
        let seconds = start.elapsed().as_secs_f64();
        assert_eq!(grid.cells.len(), 100_000);
        assert!(grid.cells.values().all(|cell| cell.spans[0].href.is_some()));
        assert!(seconds < LARGE_SHEET_SECONDS, "{seconds} s");
    }

    #[test]
    fn prefixed_and_strict_sheets_are_read() {
        let strings = Vec::new();
        let styles = Styles::default();
        let config = ExcelConfig::default();
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: [("rId1".to_string(), "https://example.com/".to_string())].into(),
            tables: Vec::new(),
            print_title_rows: None,
            config: &config,
            view: false,
        };
        let xml = concat!(
            r#"<x:worksheet xmlns:x="http://purl.oclc.org/ooxml/spreadsheetml/main" xmlns:rel="http://purl.oclc.org/ooxml/officeDocument/relationships">"#,
            r#"<x:sheetData><x:row><x:c t="inlineStr"><x:is><x:t>A &amp; B</x:t></x:is></x:c><x:c><x:v><![CDATA[4]]><!-- two -->2</x:v></x:c></x:row></x:sheetData>"#,
            r#"<x:hyperlinks><x:hyperlink ref="A1" rel:id="rId1"/></x:hyperlinks></x:worksheet>"#,
        );
        let grid = read(xml, &ctx).unwrap();
        assert_eq!(
            cells(&grid),
            [
                cell((0, 0), "A & B", CellKind::Text),
                cell((0, 1), "42", CellKind::Number)
            ]
        );
        assert_eq!(
            grid.cells[&(0, 0)].spans[0].href.as_deref(),
            Some("https://example.com/")
        );
        // Elements of another namespace are not cells.
        let xml = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:o="urn:other"><sheetData><o:row><c><v>1</v></c></o:row></sheetData></worksheet>"#;
        assert!(read(xml, &ctx).unwrap().cells.is_empty());
    }

    #[test]
    fn declarations_are_read() {
        let strings = Vec::new();
        let styles = Styles::default();
        let config = ExcelConfig::default();
        let table = Merge {
            top: 2,
            left: 0,
            bottom: 8,
            right: 5,
        };
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: HashMap::new(),
            tables: vec![(table, 1)],
            print_title_rows: Some((0, 2)),
            config: &config,
            view: false,
        };
        let xml = sheet(concat!(
            r#"<sheetViews><sheetView workbookViewId="0"><pane ySplit="3" topLeftCell="A4" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>"#,
            r#"<sheetData/><autoFilter ref="A3:F9"/>"#,
        ));
        let grid = read(&xml, &ctx).unwrap();
        assert_eq!(grid.declared.auto_filter, Some(table));
        assert_eq!(grid.declared.frozen_rows, 3);
        assert_eq!(grid.declared.tables, [(table, 1)]);
        assert_eq!(grid.declared.print_title_rows, Some((0, 2)));
        // A split that is not frozen freezes nothing.
        let xml = sheet(r#"<sheetViews><sheetView><pane ySplit="1200"/></sheetView></sheetViews><sheetData/>"#);
        assert_eq!(read(&xml, &ctx).unwrap().declared.frozen_rows, 0);
    }

    #[test]
    fn rows_past_the_limit_are_counted_not_read() {
        let rows: String = (1..=8)
            .map(|row| format!(r#"<row r="{row}"><c><v>{row}</v></c></row>"#))
            .collect();
        let config = ExcelConfig {
            max_rows: 5,
            max_cols: 2,
            ..ExcelConfig::default()
        };
        let body = format!(r#"<sheetData>{rows}<row r="9" hidden="1"/></sheetData>"#);
        let grid = read_with(&body, &[], &config);
        assert_eq!(grid.cells.len(), 5);
        assert_eq!(grid.truncated_rows, 3);
        assert!(grid.hidden_rows.is_empty());
        // Cells past the limit of columns are dropped.
        let grid = read_with(
            r#"<sheetData><row><c><v>1</v></c><c><v>2</v></c><c><v>3</v></c></row></sheetData>"#,
            &[],
            &config,
        );
        assert_eq!(grid.cells.len(), 2);
    }

    #[test]
    fn a_sheet_of_many_cells_stops_at_max_cells() {
        let rows: String = (1..=8)
            .map(|row| format!(r#"<row r="{row}"><c><v>1</v></c><c><v>2</v></c><c><v>3</v></c></row>"#))
            .collect();
        let config = ExcelConfig {
            max_cells: 7,
            ..ExcelConfig::default()
        };
        let grid = read_with(&format!("<sheetData>{rows}</sheetData>"), &[], &config);
        // A row is read whole: the third row starts with 6 cells read, under
        // the limit, and takes the count to 9; the five rows after it are
        // counted, not read.
        assert_eq!(grid.cells.len(), 9);
        assert_eq!(grid.truncated_rows, 5);
        assert_eq!(grid.truncated_cols, 0);
    }

    #[test]
    fn columns_past_the_limit_are_counted_not_read() {
        let config = ExcelConfig {
            max_cols: 2,
            ..ExcelConfig::default()
        };
        // Columns C and E have a value past the limit; D only an empty cell.
        let grid = read_with(
            concat!(
                r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="D1" s="2"/></row>"#,
                r#"<row r="2"><c r="C2"><v>3</v></c><c r="E2" t="inlineStr"><is><t>e</t></is></c></row></sheetData>"#,
            ),
            &[],
            &config,
        );
        assert_eq!(grid.cells.len(), 1);
        assert_eq!(grid.truncated_cols, 2);
    }

    #[test]
    fn an_iso_date_keeps_its_time() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1" t="d"><v>2024-01-31T14:05:00</v></c>"#,
            r#"<c r="B1" t="d"><v>2024-01-31T14:05:09.000</v></c><c r="C1" t="d"><v>2024-01-31T00:00:00Z</v></c>"#,
            r#"<c r="D1" t="d"><v>2024-01-31</v></c><c r="E1" t="d"><v>T14:05:00</v></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [
                cell((0, 0), "2024-01-31 14:05", CellKind::Date),
                cell((0, 1), "2024-01-31 14:05:09", CellKind::Date),
                cell((0, 2), "2024-01-31", CellKind::Date),
                cell((0, 3), "2024-01-31", CellKind::Date),
                cell((0, 4), "14:05", CellKind::Date),
            ]
        );
    }

    #[test]
    fn hyperlinks_point_their_cells_to_the_url() {
        let grid = read_with(
            concat!(
                r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1"><v>3</v></c></row></sheetData>"#,
                r#"<hyperlinks><hyperlink ref="A1" r:id="rId1"/><hyperlink ref="B1" location="Sheet2!A1"/><hyperlink ref="C1:C4" r:id="rId1"/></hyperlinks>"#,
            ),
            &[("rId1", "https://example.com/")],
            &ExcelConfig::default(),
        );
        let hrefs: Vec<_> = grid.cells.values().map(|cell| cell.spans[0].href.as_deref()).collect();
        assert_eq!(
            hrefs,
            [Some("https://example.com/"), None, Some("https://example.com/")]
        );
        assert_eq!(grid.cells[&(0, 1)].spans[0].text, "b");
        // A cell in two hyperlinks keeps the last.
        let grid = read_with(
            concat!(
                r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#,
                r#"<hyperlinks><hyperlink ref="A1" r:id="rId1"/><hyperlink ref="A1:B2" r:id="rId2"/></hyperlinks>"#,
            ),
            &[("rId1", "https://one.example/"), ("rId2", "https://two.example/")],
            &ExcelConfig::default(),
        );
        assert_eq!(
            grid.cells[&(0, 0)].spans[0].href.as_deref(),
            Some("https://two.example/")
        );
    }

    #[test]
    fn empty_cells_are_kept_only_when_centred_across_the_selection() {
        let grid = grid(concat!(
            r#"<sheetData><row r="1"><c r="A1" t="s" s="3"><v>0</v></c><c r="B1" s="3"/><c r="C1" s="2"/>"#,
            r#"<c r="D1" t="s"><v>9</v></c><c r="E1" t="inlineStr"><is><t></t></is></c></row></sheetData>"#,
        ));
        assert_eq!(
            cells(&grid),
            [cell((0, 0), "a", CellKind::Text), cell((0, 1), "", CellKind::Text)]
        );
        assert!(grid.cells.values().all(|cell| cell.center_continuous));
    }

    #[test]
    fn damaged_sheets_are_an_error() {
        let strings = Vec::new();
        let styles = Styles::default();
        let config = ExcelConfig::default();
        let ctx = SheetContext {
            strings: &strings,
            styles: &styles,
            date1904: false,
            links: HashMap::new(),
            tables: Vec::new(),
            print_title_rows: None,
            config: &config,
            view: false,
        };
        assert!(read("<worksheet", &ctx).is_err());
        let dtd = r#"<!DOCTYPE worksheet [<!ENTITY e "x">]><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#;
        assert!(read(dtd, &ctx).is_err());
    }

    #[test]
    fn embedded_objects_give_their_picture_and_anchor() {
        let objects = grid(
            r#"<sheetData/><oleObjects><mc:AlternateContent xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"><mc:Choice Requires="x14"><oleObject progId="Word.Document.8" shapeId="1" r:id="rId1"><objectPr r:id="rId2"><anchor moveWithCells="1"><from><xdr:col>0</xdr:col><xdr:colOff>12700</xdr:colOff><xdr:row>33</xdr:row><xdr:rowOff>0</xdr:rowOff></from><to><xdr:col>2</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>41</xdr:row><xdr:rowOff>25400</xdr:rowOff></to></anchor></objectPr></oleObject></mc:Choice><mc:Fallback><oleObject progId="Word.Document.8" shapeId="1" r:id="rId1"/></mc:Fallback></mc:AlternateContent></oleObjects>"#,
        )
        .objects;
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].rel, "rId2");
        assert_eq!(
            (objects[0].from.row, objects[0].from.col, objects[0].from.col_off_pt),
            (33, 0, 1.0)
        );
        assert_eq!(
            (objects[0].to.row, objects[0].to.col, objects[0].to.row_off_pt),
            (41, 2, 2.0)
        );
    }

    #[test]
    fn row_heights_are_whole_pixels() {
        let sizes = grid(r#"<sheetFormatPr defaultRowHeight="13.2"/><sheetData><row r="1" ht="12.95" customHeight="1"/><row r="2" ht="20" customHeight="1"/></sheetData>"#).sizes;
        assert_eq!(sizes.default_row_pt, Some(13.5));
        assert_eq!(
            (sizes.rows.get(&0).copied(), sizes.rows.get(&1).copied()),
            (Some(12.75), Some(20.25))
        );
    }
}
