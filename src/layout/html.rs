//! A Word document as one continuous HTML page with Word's formatting (the
//! `medium` mode for Word input): page-wide sheets with the margins of each
//! section, paragraphs with their fonts, sizes, colours, indents, spacing,
//! borders and shading, lists with their numbers, tables with their grid,
//! borders, merged cells and shading, pictures and text boxes, and the
//! notes at the end. Nothing is cut into pages: the text flows as in Word's
//! web view, at the width of the page.

use super::fonts::FontBook;
use super::para::{self, Ctx};
use super::shape::Shaper;
use crate::render::escape::esc;
use crate::word::fmt::model::{
    Align, Anchor, AnchorAlign, Block, Border, BorderStyle, FDoc, FImage, FieldKind, Inline, LineSpacing, NumberSuffix,
    Para, RunFmt, Shape, Table, Underline, VAlign, VMerge, VertAlign, Wrap, format_page,
};
use base64::Engine as _;
use std::collections::HashMap;
use std::fmt::Write as _;

/// The style of the page.
const CSS: &str = "body{margin:0;background:#e9e9e9;color:#000}\
.sheet{background:#fff;margin:24px auto;box-shadow:0 1px 4px rgba(0,0,0,.25);box-sizing:border-box;overflow-wrap:break-word}\
.sheet p{margin:0;white-space:pre-wrap}\
.sheet table{border-collapse:collapse;table-layout:fixed}\
.sheet td{vertical-align:top;overflow:hidden}\
.sheet img{vertical-align:baseline}\
.header{border-bottom:1px dashed #ccc;margin-bottom:12pt}\
.footer{border-top:1px dashed #ccc;margin-top:12pt}\
.notes{border-top:1px solid #000;width:33%;margin-top:12pt}\
.tabline{position:relative;white-space:pre}\
.tabline>span{position:absolute;top:0}\
@media (max-width:700px){.sheet{width:auto!important;padding:16px!important;margin:0}}";
/// Width of a phone screen below which the sheet loses its margins.
const NOTE_PREFIX: &str = "note-";

/// Renders `doc` as an HTML page.
pub fn to_html(doc: &FDoc) -> String {
    let mut book = FontBook::new(&doc.fonts, &doc.font_kinds);
    let mut shaper = Shaper::default();
    let notes = note_numbers(doc);
    let mut coverage = HashMap::new();
    let mut writer = Writer {
        doc,
        book: &mut book,
        shaper: &mut shaper,
        notes: &notes,
        coverage: &mut coverage,
        out: String::new(),
        last_style: None,
    };
    writer.document();
    resolve_images(&writer.out, &doc.images)
}

/// Starts and ends the mark of a picture in the HTML being written.
const IMAGE_MARK: char = '\u{1}';
/// Separates the parts of a picture mark: index, alt text, style.
const IMAGE_SEP: char = '\u{2}';

/// The HTML `html` with its picture marks replaced: a picture used once
/// becomes an `<img>` holding its data; one used more than once is defined
/// once, in an SVG at the end, and each use refers to it.
fn resolve_images(html: &str, images: &[FImage]) -> String {
    let parts: Vec<&str> = html.split(IMAGE_MARK).collect();
    let mark = |part: &str| -> Option<(usize, String, String)> {
        let mut fields = part.split(IMAGE_SEP);
        let index = fields.next()?.parse().ok()?;
        Some((index, fields.next()?.to_string(), fields.next()?.to_string()))
    };
    let mut uses: HashMap<usize, usize> = HashMap::new();
    for part in parts.iter().skip(1).step_by(2) {
        if let Some((index, _, _)) = mark(part) {
            *uses.entry(index).or_default() += 1;
        }
    }
    let data_uri = |index: usize| {
        let image = &images[index];
        let kind = if image.jpeg { "jpeg" } else { "png" };
        format!(
            "data:image/{kind};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&image.data)
        )
    };
    let mut out = String::with_capacity(html.len());
    for (at, part) in parts.iter().enumerate() {
        if at % 2 == 0 {
            out.push_str(part);
            continue;
        }
        let Some((index, alt, style)) = mark(part) else {
            continue;
        };
        if uses.get(&index).copied().unwrap_or(0) > 1 {
            let _ = write!(
                out,
                "<svg role=\"img\" aria-label=\"{alt}\" viewBox=\"0 0 1 1\" preserveAspectRatio=\"none\" style=\"{style}\"><use href=\"#kimg{index}\"/></svg>"
            );
        } else {
            let _ = write!(out, "<img src=\"{}\" alt=\"{alt}\" style=\"{style}\">", data_uri(index));
        }
    }
    let mut shared: Vec<usize> = uses
        .into_iter()
        .filter(|&(_, count)| count > 1)
        .map(|(index, _)| index)
        .collect();
    if !shared.is_empty() {
        shared.sort_unstable();
        let mut defs =
            String::from("<svg width=\"0\" height=\"0\" style=\"position:absolute\" aria-hidden=\"true\"><defs>");
        for index in shared {
            let _ = write!(
                defs,
                "<image id=\"kimg{index}\" width=\"1\" height=\"1\" preserveAspectRatio=\"none\" href=\"{}\"/>",
                data_uri(index)
            );
        }
        defs.push_str("</defs></svg>");
        match out.rfind("</body>") {
            Some(at) => out.insert_str(at, &defs),
            None => out.push_str(&defs),
        }
    }
    out
}

/// The number shown for each note, by (endnote, id).
fn note_numbers(doc: &FDoc) -> HashMap<(bool, String), String> {
    let mut numbers = HashMap::new();
    for (endnote, notes) in [(false, &doc.footnotes), (true, &doc.endnotes)] {
        for (at, (id, _)) in notes.iter().enumerate() {
            let format = if endnote {
                &doc.endnote_format
            } else {
                &doc.footnote_format
            };
            numbers.insert((endnote, id.clone()), format_page(at as u32 + 1, format));
        }
    }
    numbers
}

/// Writes the HTML.
struct Writer<'a> {
    /// The document.
    doc: &'a FDoc,
    /// Its fonts (for line heights and the positions of tab stops).
    book: &'a mut FontBook,
    /// The shaper.
    shaper: &'a mut Shaper,
    /// Note numbers.
    notes: &'a HashMap<(bool, String), String>,
    /// Faces that cover characters.
    coverage: &'a mut HashMap<(super::fonts::FaceId, char), bool>,
    /// The HTML written.
    out: String,
    /// Style of the previous paragraph (for contextual spacing).
    last_style: Option<String>,
}

impl Writer<'_> {
    /// A context for the paragraph layout.
    fn ctx(&mut self) -> Ctx<'_> {
        Ctx {
            book: self.book,
            shaper: self.shaper,
            doc: self.doc,
            notes: self.notes,
            page_refs: None,
            own_note: None,
            page: None,
            page_count: None,
            section_pages: None,
            coverage: self.coverage,
            origin: 0.0,
        }
    }

    /// The whole page.
    fn document(&mut self) {
        let title = self.doc.title.clone().unwrap_or_default();
        let _ = write!(
            self.out,
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>{CSS}{}</style></head><body>",
            esc(&title),
            crate::render::mathml::math_css(false)
        );
        let mut open: Option<(f32, [f32; 2])> = None;
        let count = self.doc.sections.len();
        for (at, section) in self.doc.sections.iter().enumerate() {
            let page = section.page;
            let key = (page.width, [page.left, page.right]);
            if open != Some(key) {
                if open.is_some() {
                    self.out.push_str("</div>");
                }
                let _ = write!(
                    self.out,
                    "<div class=\"sheet\" style=\"width:{}pt;padding:{}pt {}pt {}pt {}pt\">",
                    num(page.width),
                    num(page.top.abs()),
                    num(page.right),
                    num(page.bottom.abs()),
                    num(page.left + page.gutter)
                );
                if at == 0 {
                    self.part(
                        section
                            .headers
                            .first
                            .filter(|_| section.title_page)
                            .or(section.headers.default),
                        "header",
                    );
                }
                open = Some(key);
            }
            let width = page.width - page.left - page.right - page.gutter;
            self.blocks(&section.blocks, width);
            if at + 1 == count {
                self.notes_html(width);
                self.part(
                    section
                        .footers
                        .first
                        .filter(|_| section.title_page)
                        .or(section.footers.default),
                    "footer",
                );
            }
        }
        if open.is_some() {
            self.out.push_str("</div>");
        }
        self.out.push_str("</body></html>");
    }

    /// A header or footer part.
    fn part(&mut self, part: Option<usize>, class: &str) {
        let Some(blocks) = part.and_then(|part| self.doc.parts.get(part)) else {
            return;
        };
        let _ = write!(self.out, "<div class=\"{class}\">");
        self.blocks(blocks, f32::INFINITY);
        self.out.push_str("</div>");
    }

    /// The footnotes and endnotes, after the text.
    fn notes_html(&mut self, width: f32) {
        let notes: Vec<(bool, String, Vec<Block>)> = self
            .doc
            .footnotes
            .iter()
            .map(|(id, blocks)| (false, id.clone(), blocks.clone()))
            .chain(
                self.doc
                    .endnotes
                    .iter()
                    .map(|(id, blocks)| (true, id.clone(), blocks.clone())),
            )
            .collect();
        if notes.is_empty() {
            return;
        }
        self.out.push_str("<div class=\"notes\"></div>");
        for (endnote, id, blocks) in notes {
            let _ = write!(
                self.out,
                "<div id=\"{NOTE_PREFIX}{}{}\">",
                if endnote { "e" } else { "f" },
                esc(&id)
            );
            self.blocks(&blocks, width);
            self.out.push_str("</div>");
        }
    }

    /// Paragraphs and tables.
    fn blocks(&mut self, blocks: &[Block], width: f32) {
        for block in blocks {
            match block {
                Block::Para(para) => self.paragraph(para, width),
                Block::Table(table) => self.table(table),
            }
        }
    }

    /// A paragraph.
    fn paragraph(&mut self, para: &Para, width: f32) {
        let props = &para.props;
        let same_style = self.last_style.as_deref() == Some(para.style.as_str());
        self.last_style = Some(para.style.clone());
        let contextual = props.contextual && same_style;
        let mut style = String::new();
        let before = if contextual { 0.0 } else { props.before };
        let after = if props.contextual { 0.0 } else { props.after };
        let _ = write!(
            style,
            "margin:{}pt {}pt {}pt {}pt;text-indent:{}pt;text-align:{};line-height:{}",
            num(before),
            num(props.right),
            num(after),
            num(props.left),
            num(props.first_line),
            align_css(props.align),
            self.line_height(para)
        );
        border_css(&mut style, props.borders.top, "top");
        border_css(&mut style, props.borders.bottom, "bottom");
        border_css(&mut style, props.borders.left, "left");
        border_css(&mut style, props.borders.right, "right");
        if let Some(color) = props.shading {
            let _ = write!(style, ";background:{}", hex(color));
        }
        let has_tabs = para.inlines.iter().any(|inline| matches!(inline, Inline::Tab { .. }))
            || para
                .number
                .as_ref()
                .is_some_and(|number| number.suffix == NumberSuffix::Tab);
        if has_tabs && width.is_finite() && self.tab_line(para, width, &style) {
            return;
        }
        let _ = write!(self.out, "<p style=\"{style}\"{}>", anchor_ids(&para.bookmarks));
        if let Some(number) = &para.number {
            let gap = match number.suffix {
                NumberSuffix::Tab => " ",
                NumberSuffix::Space => " ",
                NumberSuffix::Nothing => "",
            };
            let _ = write!(
                self.out,
                "<span style=\"{}\">{}{gap}</span>",
                run_css(&number.fmt),
                esc(&number.text)
            );
        }
        self.inlines(&para.inlines);
        if para.inlines.is_empty() && para.number.is_none() {
            let _ = write!(self.out, "<span style=\"{}\">&#8203;</span>", run_css(&para.mark));
        }
        self.out.push_str("</p>");
    }

    /// The CSS line height of a paragraph.
    fn line_height(&mut self, para: &Para) -> String {
        let size = para.mark.size.max(1.0);
        let (ascent, descent, leading) = self.ctx().line_metrics(&para.mark);
        let natural = (ascent + descent + leading) / size;
        match para.props.line {
            LineSpacing::Auto(multiple) => num(natural * multiple),
            LineSpacing::Exact(value) => format!("{}pt", num(value)),
            LineSpacing::AtLeast(value) => format!("max({}pt,{})", num(value), num(natural)),
        }
    }

    /// A paragraph of one line with tabs, its pieces placed where Word puts
    /// them (from the layout); `false` when it takes more than one line.
    fn tab_line(&mut self, para: &Para, width: f32, style: &str) -> bool {
        let layout = para::layout(&mut self.ctx(), para, width);
        let [line] = layout.lines.as_slice() else { return false };
        let _ = write!(
            self.out,
            "<p class=\"tabline\" style=\"{style};height:{}pt\"{}>",
            num(line.height),
            anchor_ids(&para.bookmarks)
        );
        let fmt = para
            .inlines
            .iter()
            .find_map(|inline| match inline {
                Inline::Text { fmt, .. } => Some(fmt.clone()),
                _ => None,
            })
            .unwrap_or_else(|| para.mark.clone());
        for item in &line.items {
            if let super::display::Item::Text(run) = item {
                let top = line.baseline - run.size * 0.8;
                let _ = write!(
                    self.out,
                    "<span style=\"left:{}pt;top:{}pt;{};font-size:{}pt;color:{}\">{}</span>",
                    num(run.x - para.props.left - para.props.first_line.min(0.0)),
                    num(top),
                    family_css(&fmt),
                    num(run.size),
                    hex(run.color),
                    esc(&run.text)
                );
            }
        }
        self.out.push_str("</p>");
        true
    }

    /// The content of a paragraph.
    fn inlines(&mut self, inlines: &[Inline]) {
        for inline in inlines {
            match inline {
                Inline::Text { text, fmt } if !fmt.hidden => self.text(text, fmt),
                Inline::Tab { fmt } => {
                    let _ = write!(self.out, "<span style=\"{}\">\t</span>", run_css(fmt));
                }
                Inline::Break { .. } => self.out.push_str("<br>"),
                Inline::Field { kind, fmt, format } => {
                    let value = match kind {
                        FieldKind::Page => format_page(1, format.as_ref().unwrap_or(&Default::default())),
                        _ => "1".into(),
                    };
                    self.text(&value, fmt);
                }
                Inline::Image {
                    index,
                    width,
                    height,
                    anchor,
                    alt,
                    ..
                } => self.image(*index, *width, *height, anchor.as_ref(), alt),
                Inline::Shape(shape) => self.shape(shape),
                Inline::NoteRef { endnote, id, fmt } => {
                    let number = self.notes.get(&(*endnote, id.clone())).cloned().unwrap_or_default();
                    let _ = write!(
                        self.out,
                        "<a href=\"#{NOTE_PREFIX}{}{}\" style=\"{}\">{}</a>",
                        if *endnote { "e" } else { "f" },
                        esc(id),
                        run_css(fmt),
                        esc(&number)
                    );
                }
                Inline::NoteNumber { fmt } => self.text("•", fmt),
                Inline::Math { node, display, .. } => {
                    let html = if *display {
                        crate::render::mathml::math_html(node, "")
                    } else {
                        crate::render::mathml::inline_math_html(node, "")
                    };
                    self.out.push_str(&html);
                }
                Inline::Text { .. } => {}
            }
        }
    }

    /// A run of text.
    fn text(&mut self, text: &str, fmt: &RunFmt) {
        let css = run_css(fmt);
        match fmt
            .href
            .as_deref()
            .filter(|href| crate::common::links::is_safe_link(href))
        {
            Some(href) => {
                let _ = write!(self.out, "<a href=\"{}\" style=\"{css}\">{}</a>", esc(href), esc(text));
            }
            None => {
                let _ = write!(self.out, "<span style=\"{css}\">{}</span>", esc(text));
            }
        }
    }

    /// A picture, in the line or floating.
    fn image(&mut self, index: Option<usize>, width: f32, height: f32, anchor: Option<&Anchor>, alt: &str) {
        let Some(index) = index.filter(|&index| index < self.doc.images.len()) else {
            return;
        };
        // A mark that `resolve_images` turns into the picture.
        let _ = write!(
            self.out,
            "{IMAGE_MARK}{index}{IMAGE_SEP}{}{IMAGE_SEP}width:{}pt;height:{}pt{}{IMAGE_MARK}",
            esc(alt),
            num(width),
            num(height),
            float_css(anchor)
        );
    }

    /// A text box or drawn shape.
    fn shape(&mut self, shape: &Shape) {
        let mut style = format!(
            "display:inline-block;vertical-align:top;box-sizing:border-box;max-width:100%;width:{}pt",
            num(shape.width.max(1.0))
        );
        if shape.width <= 0.0 {
            style = "display:inline-block;vertical-align:top".into();
        }
        if !shape.auto_height && shape.height > 0.0 {
            let _ = write!(style, ";height:{}pt", num(shape.height));
        } else {
            let _ = write!(style, ";min-height:{}pt", num(shape.height));
        }
        let [left, top, right, bottom] = shape.insets;
        let _ = write!(
            style,
            ";padding:{}pt {}pt {}pt {}pt",
            num(top),
            num(right),
            num(bottom),
            num(left)
        );
        if let Some(color) = shape.fill {
            let _ = write!(style, ";background:{}", hex(color));
        }
        if let Some(line) = shape.line {
            let _ = write!(style, ";border:{}", border_value(line));
        }
        style.push_str(&float_css(shape.anchor.as_ref()));
        let _ = write!(
            self.out,
            "<span style=\"{style};text-indent:0;text-align:left;white-space:normal\">"
        );
        for &(index, x, y, width, height) in &shape.pictures {
            let _ = (x, y);
            self.image(Some(index), width, height, None, "");
        }
        let saved = self.last_style.take();
        let inner = (shape.width - left - right).max(1.0);
        // Block content inside an inline box: paragraphs become blocks.
        let mut sub = Writer {
            doc: self.doc,
            book: self.book,
            shaper: self.shaper,
            notes: self.notes,
            coverage: self.coverage,
            out: String::new(),
            last_style: None,
        };
        sub.blocks(&shape.blocks, if shape.width > 0.0 { inner } else { f32::INFINITY });
        let inner_html = sub.out.replace("<p ", "<span class=\"p\" ").replace("</p>", "</span>");
        let _ = write!(
            self.out,
            "{}</span>",
            inner_html.replace("class=\"p\" style=\"", "style=\"display:block;")
        );
        self.last_style = saved;
    }

    /// A table.
    fn table(&mut self, table: &Table) {
        let total: f32 = table.grid.iter().sum();
        let mut style = format!("width:{}pt", num(total));
        let first_margin = table.props.cell_margins[0];
        match table.props.align {
            Align::Center => style.push_str(";margin-left:auto;margin-right:auto"),
            Align::Right => style.push_str(";margin-left:auto"),
            _ => {
                let indent = if self.doc.compat_mode < 15 {
                    table.props.indent - first_margin
                } else {
                    table.props.indent
                };
                let _ = write!(style, ";margin-left:{}pt", num(indent));
            }
        }
        let _ = write!(self.out, "<table style=\"{style}\"><colgroup>");
        for width in &table.grid {
            let _ = write!(self.out, "<col style=\"width:{}pt\">", num(*width));
        }
        self.out.push_str("</colgroup>");
        let spans = row_spans(table);
        for (row_at, row) in table.rows.iter().enumerate() {
            let height = row.height.map_or(String::new(), |(value, _)| {
                format!(" style=\"height:{}pt\"", num(value))
            });
            let _ = write!(self.out, "<tr{height}>");
            if row.grid_before > 0 {
                let _ = write!(self.out, "<td colspan=\"{}\"></td>", row.grid_before);
            }
            let mut column = row.grid_before;
            for (cell_at, cell) in row.cells.iter().enumerate() {
                let span = cell.span.max(1);
                let width: f32 = table.grid.iter().skip(column).take(span).sum();
                column += span;
                if cell.vmerge == VMerge::Continue {
                    continue;
                }
                let rows = spans.get(&(row_at, cell_at)).copied().unwrap_or(1);
                self.cell(table, cell, span, rows, width);
            }
            self.out.push_str("</tr>");
        }
        self.out.push_str("</table>");
        self.last_style = None;
    }

    /// A cell of a table.
    fn cell(&mut self, table: &Table, cell: &crate::word::fmt::model::Cell, span: usize, rows: usize, width: f32) {
        let margin = |side: usize| {
            let value = cell.margins[side];
            if value.is_nan() {
                table.props.cell_margins[side]
            } else {
                value
            }
        };
        let mut style = format!(
            "padding:{}pt {}pt {}pt {}pt;vertical-align:{}",
            num(margin(1)),
            num(margin(2)),
            num(margin(3)),
            num(margin(0)),
            match cell.valign {
                VAlign::Top => "top",
                VAlign::Center => "middle",
                VAlign::Bottom => "bottom",
            }
        );
        for (side, name) in [(0, "top"), (1, "left"), (2, "bottom"), (3, "right")] {
            border_css(&mut style, cell.borders[side], name);
        }
        if let Some(color) = cell.shading {
            let _ = write!(style, ";background:{}", hex(color));
        }
        let colspan = if span > 1 {
            format!(" colspan=\"{span}\"")
        } else {
            String::new()
        };
        let rowspan = if rows > 1 {
            format!(" rowspan=\"{rows}\"")
        } else {
            String::new()
        };
        let _ = write!(self.out, "<td{colspan}{rowspan} style=\"{style}\">");
        self.last_style = None;
        let inner = (width - margin(0) - margin(2)).max(1.0);
        self.blocks(&cell.blocks, inner);
        self.out.push_str("</td>");
    }
}

/// The number of rows each cell that starts a vertical merge spans, by
/// (row, cell).
fn row_spans(table: &Table) -> HashMap<(usize, usize), usize> {
    let mut spans = HashMap::new();
    // The cell that starts the merge in each grid column.
    let mut open: HashMap<usize, (usize, usize)> = HashMap::new();
    for (row_at, row) in table.rows.iter().enumerate() {
        let mut column = row.grid_before;
        for (cell_at, cell) in row.cells.iter().enumerate() {
            match cell.vmerge {
                VMerge::Restart => {
                    open.insert(column, (row_at, cell_at));
                    spans.insert((row_at, cell_at), 1);
                }
                VMerge::Continue => {
                    if let Some(start) = open.get(&column)
                        && let Some(count) = spans.get_mut(start)
                    {
                        *count += 1;
                    }
                }
                VMerge::None => {
                    open.remove(&column);
                }
            }
            column += cell.span.max(1);
        }
    }
    spans
}

/// The `id` attributes of a paragraph's bookmarks (the first one).
fn anchor_ids(bookmarks: &[String]) -> String {
    bookmarks
        .first()
        .map_or(String::new(), |name| format!(" id=\"{}\"", esc(name)))
}

/// CSS `text-align`.
fn align_css(align: Align) -> &'static str {
    match align {
        Align::Center => "center",
        Align::Right => "right",
        Align::Both | Align::Distribute => "justify",
        Align::Left => "left",
    }
}

/// Floating of a picture or shape: to the side it is placed on.
fn float_css(anchor: Option<&Anchor>) -> String {
    let Some(anchor) = anchor else { return String::new() };
    if anchor.wrap == Wrap::None || anchor.wrap == Wrap::TopBottom {
        return ";display:block;margin:4pt auto".into();
    }
    let right = matches!(anchor.horizontal.align, Some(AnchorAlign::End | AnchorAlign::Outside))
        || anchor.horizontal.offset > 200.0;
    let [top, bottom, left, right_dist] = anchor.dist;
    if right {
        format!(
            ";float:right;margin:{}pt 0 {}pt {}pt",
            num(top),
            num(bottom),
            num(left.max(4.0))
        )
    } else {
        format!(
            ";float:left;margin:{}pt {}pt {}pt 0",
            num(top),
            num(right_dist.max(4.0)),
            num(bottom)
        )
    }
}

/// CSS of a border on `side`.
fn border_css(style: &mut String, border: Option<Border>, side: &str) {
    if let Some(border) = border {
        let _ = write!(
            style,
            ";border-{side}:{};padding-{side}:{}pt",
            border_value(border),
            num(border.space)
        );
    }
}

/// The value of a CSS `border`.
fn border_value(border: Border) -> String {
    let style = match border.style {
        BorderStyle::Single => "solid",
        BorderStyle::Double => "double",
        BorderStyle::Dotted => "dotted",
        BorderStyle::Dashed => "dashed",
    };
    let width = if border.style == BorderStyle::Double {
        (border.width * 3.0).max(2.25)
    } else {
        border.width.max(0.25)
    };
    format!("{}pt {style} {}", num(width), hex(border.color))
}

/// The CSS of a run of text.
fn run_css(fmt: &RunFmt) -> String {
    let mut css = family_css(fmt);
    let size = if fmt.vert == VertAlign::Baseline {
        fmt.size
    } else {
        fmt.size * 2.0 / 3.0
    };
    let _ = write!(css, ";font-size:{}pt", num(size));
    if fmt.bold {
        css.push_str(";font-weight:bold");
    }
    if fmt.italic {
        css.push_str(";font-style:italic");
    }
    let mut lines = Vec::new();
    if let Some(underline) = fmt.underline {
        lines.push("underline");
        let style = match underline {
            Underline::Double => Some("double"),
            Underline::Dotted => Some("dotted"),
            Underline::Dashed => Some("dashed"),
            Underline::Wave => Some("wavy"),
            _ => None,
        };
        if let Some(style) = style {
            let _ = write!(css, ";text-decoration-style:{style}");
        }
    }
    if fmt.strike || fmt.dstrike {
        lines.push("line-through");
    }
    if !lines.is_empty() {
        let _ = write!(css, ";text-decoration-line:{}", lines.join(" "));
    }
    if let Some(color) = fmt.color {
        let _ = write!(css, ";color:{}", hex(color));
    }
    if let Some(color) = fmt.highlight.or(fmt.shading) {
        let _ = write!(css, ";background:{}", hex(color));
    }
    match fmt.vert {
        VertAlign::Super => css.push_str(";vertical-align:super"),
        VertAlign::Sub => css.push_str(";vertical-align:sub"),
        VertAlign::Baseline if fmt.position != 0.0 => {
            let _ = write!(css, ";position:relative;top:{}pt", num(-fmt.position));
        }
        VertAlign::Baseline => {}
    }
    if fmt.caps {
        css.push_str(";text-transform:uppercase");
    }
    if fmt.small_caps {
        css.push_str(";font-variant:small-caps");
    }
    if fmt.spacing != 0.0 {
        let _ = write!(css, ";letter-spacing:{}pt", num(fmt.spacing));
    }
    css
}

/// The CSS `font-family` of a run: its font, the open fonts with the same
/// metrics, then a generic family.
fn family_css(fmt: &RunFmt) -> String {
    let font = if fmt.font.is_empty() {
        "Times New Roman"
    } else {
        fmt.font.as_str()
    };
    let lower = font.to_ascii_lowercase();
    let (open, generic) = match lower.as_str() {
        "times new roman" | "times" => ("'Liberation Serif','Tinos'", "serif"),
        "arial" | "helvetica" => ("'Liberation Sans','Arimo'", "sans-serif"),
        "courier new" | "courier" => ("'Liberation Mono','Cousine'", "monospace"),
        "calibri" | "calibri light" => ("'Carlito'", "sans-serif"),
        "cambria" => ("'Caladea'", "serif"),
        "georgia" => ("'Gelasio'", "serif"),
        "segoe ui" => ("'Selawik'", "sans-serif"),
        "verdana" | "tahoma" | "trebuchet ms" => ("'DejaVu Sans'", "sans-serif"),
        _ if lower.contains("mono") || lower.contains("courier") || lower == "consolas" => {
            ("'DejaVu Sans Mono'", "monospace")
        }
        _ if lower.contains("sans") || lower.contains("gothic") || lower.contains("arial") => {
            ("'DejaVu Sans'", "sans-serif")
        }
        _ => ("'DejaVu Serif'", "serif"),
    };
    format!("font-family:'{}',{open},{generic}", font.replace(['\'', '"'], ""))
}

/// `#rrggbb`.
fn hex([red, green, blue]: [u8; 3]) -> String {
    format!("#{red:02x}{green:02x}{blue:02x}")
}

/// A number written compactly for CSS.
fn num(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == rounded.trunc() {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}
