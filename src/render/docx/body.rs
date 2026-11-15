//! The body of the Word document (`word/document.xml`): the blocks in
//! reading order as paragraphs, tables, equations and pictures.

use super::omml::display_math;
use super::parts::{TEXT_WIDTH_TWIPS, TWIPS_PER_PT, section_props};
use super::table::table_xml;
use super::xml::{bookmark_name, run_props, text_runs};
use super::{DocxOptions, PendingImage};
use crate::model::*;
use crate::render::assets::MARK;
use crate::render::{
    CAP_LABEL, Equations, code, code_run, figure_alt, is_abstract_text, is_scene_break, plain, stray_equation_number,
};
use std::collections::{HashMap, HashSet};

/// Font size of the running text of the Word document, in points (the
/// `w:sz` of the default style is twice this). Pictures of formulas are
/// scaled by this over the body size of the paper, so that they match the
/// text around them.
const BODY_SIZE_PT: f32 = 11.0;
/// Tallest picture, in points: a little less than the text height of a
/// page, so that a picture and a line of caption fit on one page.
const MAX_IMAGE_HEIGHT_PT: f32 = 600.0;
/// English Metric Units per point, the unit of picture sizes in DrawingML.
const EMU_PER_PT: f32 = 12700.0;
/// Width of each side column of the table that holds a numbered equation
/// (the number sits in the right one), in twips.
const EQ_SIDE_TWIPS: u32 = TEXT_WIDTH_TWIPS / 10;
/// Width of the middle column of that table, which holds the equation.
const EQ_MIDDLE_TWIPS: u32 = TEXT_WIDTH_TWIPS - 2 * EQ_SIDE_TWIPS;
/// Link schemes written as external hyperlinks; other targets stay text.
const LINK_SCHEMES: [&str; 4] = ["http://", "https://", "mailto:", "ftp://"];
/// The marker of an item of a bullet list.
const BULLET: &str = "•";
/// A tab, as a run: separates a label from the text in a hanging indent.
const TAB_RUN: &str = "<w:r><w:tab/></w:r>";
/// An empty paragraph only a few points high, between two tables: Word
/// would join tables that touch into one.
const TABLE_GAP: &str = "<w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"120\" w:lineRule=\"exact\"/></w:pPr></w:p>";

/// Where a link of a span points in the Word document.
enum LinkTarget {
    /// A bookmark of this document.
    Bookmark(String),
    /// An external URL: the index of its relationship.
    External(usize),
}

/// Writes the body of the document, block by block.
pub(super) struct Body<'a> {
    /// The output options.
    opts: &'a DocxOptions,
    /// Scale of the pictures of formulas (see [`BODY_SIZE_PT`]).
    eq_scale: f32,
    /// The XML so far. Pictures are placeholders (see [`MARK`]) holding an
    /// index into `images`.
    pub(super) xml: String,
    /// The pictures the body needs, drawn once the body is written.
    pub(super) images: Vec<PendingImage>,
    /// The external link targets, by relationship index.
    pub(super) links: Vec<String>,
    /// The index of each external link target in `links`.
    link_ids: HashMap<String, usize>,
    /// The anchors of the blocks, which become bookmarks.
    anchors: HashSet<String>,
    /// Bookmarks written so far (their ids).
    bookmarks: usize,
    /// The last thing written is a table.
    after_table: bool,
}

impl<'a> Body<'a> {
    /// An empty body for `doc`.
    pub(super) fn new(doc: &Document, opts: &'a DocxOptions) -> Self {
        Self {
            opts,
            eq_scale: BODY_SIZE_PT / doc.body_size.max(1.0),
            xml: String::new(),
            images: Vec::new(),
            links: Vec::new(),
            link_ids: HashMap::new(),
            anchors: doc.blocks.iter().filter_map(|block| block.anchor.clone()).collect(),
            bookmarks: 0,
            after_table: false,
        }
    }

    /// Writes every block of `doc`, then the page layout. Runs of code
    /// paragraphs become one listing, the title blocks one title, and an
    /// abstract without a heading gets one.
    pub(super) fn write(&mut self, doc: &Document) {
        let blocks: Vec<&Block> = doc.blocks.iter().collect();
        let mut idx = 0;
        let mut in_abstract = false;
        while idx < blocks.len() {
            let block = blocks[idx];
            if let Some(end) = code_run(&blocks, idx) {
                self.listing(&code::listing_lines(&blocks[idx..end]), block.anchor.as_deref());
                (idx, in_abstract) = (end, false);
                continue;
            }
            match &block.kind {
                BlockKind::Title => {
                    (idx, in_abstract) = (self.title(&blocks, idx), false);
                    continue;
                }
                BlockKind::Paragraph { in_abstract: true } => {
                    let had_heading = idx > 0
                        && matches!(blocks[idx - 1].kind, BlockKind::Heading { .. })
                        && is_abstract_text(&blocks[idx - 1].spans);
                    if !in_abstract && !had_heading {
                        self.paragraph("AbstractHeading", "", None, &text_runs("Abstract", ""));
                    }
                    in_abstract = true;
                }
                // Equations do not interrupt an abstract.
                BlockKind::Equation { .. } => {}
                _ => in_abstract = false,
            }
            // An equation whose number was read as a paragraph of its own
            // takes it back.
            if let BlockKind::Equation { image, number: None } = &block.kind
                && let Some(number) = blocks.get(idx + 1).and_then(|next| stray_equation_number(next))
            {
                self.equation(block, image, Some(&number));
                idx += 2;
                continue;
            }
            self.block(block);
            idx += 1;
        }
        self.xml.push_str(&section_props());
    }

    /// Writes one block.
    fn block(&mut self, block: &Block) {
        let anchor = block.anchor.as_deref();
        match &block.kind {
            BlockKind::Title => self.simple("Title", block),
            BlockKind::Authors => self.authors(block),
            BlockKind::Note => self.simple("Note", block),
            BlockKind::Heading { level } => self.simple(&format!("Heading{}", (*level).clamp(1, 4)), block),
            BlockKind::Paragraph { .. } if is_scene_break(&block.spans) => self.simple("SceneBreak", block),
            BlockKind::Paragraph { in_abstract: true } => self.simple("Abstract", block),
            BlockKind::Paragraph { .. } => self.simple("Normal", block),
            BlockKind::ListItem { marker } => self.list_item(block, marker.as_deref()),
            BlockKind::ListContinuation => self.simple("ListContinue", block),
            BlockKind::Reference { label } => self.reference(block, label.as_deref()),
            BlockKind::Footnote { marker } => {
                let mark = if marker.is_empty() {
                    String::new()
                } else {
                    text_runs(marker, &run_props(style::SUP, [0; 3], None)) + &text_runs(" ", "")
                };
                let content = mark + &self.runs(&block.spans);
                self.paragraph("FootnoteText", "", anchor, &content);
            }
            BlockKind::Equation { image, number } => self.equation(block, image, number.as_deref()),
            BlockKind::Figure { kind, image, label } => self.figure(block, *kind, image.as_ref(), label),
            BlockKind::Code => {
                let lines: Vec<String> = block
                    .rows
                    .iter()
                    .map(|row| row.iter().map(|span| span.text.as_str()).collect())
                    .collect();
                self.listing(&lines, anchor);
            }
        }
    }

    /// Writes the block as one paragraph of style `style`.
    fn simple(&mut self, style: &str, block: &Block) {
        let content = self.runs(&block.spans);
        self.paragraph(style, "", block.anchor.as_deref(), &content);
    }

    /// Writes a paragraph of style `style` with the paragraph properties
    /// `props` (which must follow the style in schema order), a bookmark
    /// for `anchor` and the runs `content`.
    fn paragraph(&mut self, style: &str, props: &str, anchor: Option<&str>, content: &str) {
        let style = if style == "Normal" {
            String::new()
        } else {
            format!("<w:pStyle w:val=\"{style}\"/>")
        };
        let bookmark = self.bookmark(anchor);
        self.xml.push_str("<w:p>");
        if !style.is_empty() || !props.is_empty() {
            self.xml.push_str(&format!("<w:pPr>{style}{props}</w:pPr>"));
        }
        self.xml.push_str(&bookmark);
        self.xml.push_str(content);
        self.xml.push_str("</w:p>");
        self.after_table = false;
    }

    /// A bookmark for `anchor`, or nothing.
    fn bookmark(&mut self, anchor: Option<&str>) -> String {
        let Some(anchor) = anchor else {
            return String::new();
        };
        let id = self.bookmarks;
        self.bookmarks += 1;
        format!(
            "<w:bookmarkStart w:id=\"{id}\" w:name=\"{}\"/><w:bookmarkEnd w:id=\"{id}\"/>",
            bookmark_name(anchor)
        )
    }

    /// Starts a table: a small gap first when the last thing written is a
    /// table too.
    fn before_table(&mut self) {
        if self.after_table {
            self.xml.push_str(TABLE_GAP);
        }
    }

    /// Styled spans as runs, with their links: a link to an anchor of the
    /// document points to its bookmark, a web link to its URL; other
    /// targets are left out.
    fn runs(&mut self, spans: &[Span]) -> String {
        let mut out = String::new();
        for span in spans {
            match span.href.as_deref().and_then(|href| self.link_target(href)) {
                Some(LinkTarget::Bookmark(name)) => out.push_str(&format!(
                    "<w:hyperlink w:anchor=\"{name}\" w:history=\"1\">{}</w:hyperlink>",
                    text_runs(&span.text, &run_props(span.style, span.color, Some("CrossReference")))
                )),
                Some(LinkTarget::External(id)) => out.push_str(&format!(
                    "<w:hyperlink r:id=\"rIdLink{id}\" w:history=\"1\">{}</w:hyperlink>",
                    text_runs(&span.text, &run_props(span.style, span.color, Some("Hyperlink")))
                )),
                None => out.push_str(&text_runs(&span.text, &run_props(span.style, span.color, None))),
            }
        }
        out
    }

    /// Where the link `href` points, registering an external URL.
    fn link_target(&mut self, href: &str) -> Option<LinkTarget> {
        if let Some(anchor) = href.strip_prefix('#') {
            return self
                .anchors
                .contains(anchor)
                .then(|| LinkTarget::Bookmark(bookmark_name(anchor)));
        }
        if !LINK_SCHEMES.iter().any(|scheme| href.starts_with(scheme)) {
            return None;
        }
        let next = self.links.len();
        let id = *self.link_ids.entry(href.to_string()).or_insert(next);
        if id == next {
            self.links.push(href.to_string());
        }
        Some(LinkTarget::External(id))
    }

    /// Writes the title blocks that start at `blocks[start]` as one title
    /// and returns the index of the block after them.
    fn title(&mut self, blocks: &[&Block], start: usize) -> usize {
        let end = start
            + blocks[start..]
                .iter()
                .take_while(|block| matches!(block.kind, BlockKind::Title))
                .count();
        let mut spans: Vec<Span> = Vec::new();
        for block in &blocks[start..end] {
            if !spans.is_empty() {
                spans.push(Span::plain(" "));
            }
            spans.extend(block.spans.iter().cloned());
        }
        let content = self.runs(&spans);
        let anchor = blocks[start..end].iter().find_map(|block| block.anchor.as_deref());
        self.paragraph("Title", "", anchor, &content);
        end
    }

    /// Writes an author group: the name, then the affiliation lines.
    fn authors(&mut self, block: &Block) {
        let rows = if block.rows.is_empty() {
            std::slice::from_ref(&block.spans)
        } else {
            &block.rows[..]
        };
        let mut anchor = block.anchor.as_deref();
        for (i, row) in rows.iter().enumerate() {
            let content = self.runs(row);
            self.paragraph(
                if i == 0 { "AuthorName" } else { "Author" },
                "",
                anchor.take(),
                &content,
            );
        }
    }

    /// Writes a list item with its marker (a bullet when it has none)
    /// hanging in the margin.
    fn list_item(&mut self, block: &Block, marker: Option<&str>) {
        let content = match marker {
            Some(marker) => match strip_label(&block.spans, marker) {
                Some(rest) => text_runs(marker, "") + TAB_RUN + &self.runs(&rest),
                None => self.runs(&block.spans),
            },
            None => text_runs(BULLET, "") + TAB_RUN + &self.runs(&block.spans),
        };
        self.paragraph("ListItem", "", block.anchor.as_deref(), &content);
    }

    /// Writes a bibliography entry with its label ("[12]", "12.") hanging
    /// in the margin.
    fn reference(&mut self, block: &Block, label: Option<&str>) {
        let printed = label.and_then(|label| {
            [format!("[{label}]"), format!("{label}."), label.to_string()]
                .into_iter()
                .find_map(|printed| strip_label(&block.spans, &printed).map(|rest| (printed, rest)))
        });
        let content = match printed {
            Some((printed, rest)) => text_runs(&printed, "") + TAB_RUN + &self.runs(&rest),
            None => self.runs(&block.spans),
        };
        self.paragraph("Bibliography", "", block.anchor.as_deref(), &content);
    }

    /// Writes code lines as monospaced paragraphs, with their indentation.
    fn listing(&mut self, lines: &[String], anchor: Option<&str>) {
        let mut anchor = anchor;
        for (i, line) in lines.iter().enumerate() {
            let spacing = match (i == 0, i + 1 == lines.len()) {
                (true, true) => "<w:spacing w:before=\"120\" w:after=\"160\"/>",
                (true, false) => "<w:spacing w:before=\"120\"/>",
                (false, true) => "<w:spacing w:after=\"160\"/>",
                (false, false) => "",
            };
            self.paragraph("Code", spacing, anchor.take(), &text_runs(line, ""));
        }
    }

    /// Writes a displayed formula: as an Office Math equation when its
    /// structure is known (and wanted), as a picture otherwise; with its
    /// number at the right margin.
    fn equation(&mut self, block: &Block, image: &ImageSpec, number: Option<&str>) {
        let anchor = block.anchor.as_deref();
        let content = match &block.math {
            Some(math) if self.opts.equations == Equations::MathMl => display_math(math),
            _ => {
                let room = if number.is_some() {
                    EQ_MIDDLE_TWIPS
                } else {
                    TEXT_WIDTH_TWIPS
                };
                let scale = self.eq_scale;
                self.image_run(image, &plain(&block.spans), room as f32 / TWIPS_PER_PT, scale)
            }
        };
        let Some(number) = number else {
            return self.paragraph("Equation", "", anchor, &content);
        };
        // A borderless table of three columns: the equation centred on the
        // line, its number flush right.
        self.before_table();
        let bookmark = self.bookmark(anchor);
        let cell = |width: u32, inner: &str| {
            format!(
                "<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/><w:vAlign w:val=\"center\"/></w:tcPr>{inner}</w:tc>"
            )
        };
        self.xml.push_str(&format!(
            "<w:tbl><w:tblPr><w:tblW w:w=\"{TEXT_WIDTH_TWIPS}\" w:type=\"dxa\"/><w:tblLayout w:type=\"fixed\"/></w:tblPr>\
<w:tblGrid><w:gridCol w:w=\"{EQ_SIDE_TWIPS}\"/><w:gridCol w:w=\"{EQ_MIDDLE_TWIPS}\"/><w:gridCol w:w=\"{EQ_SIDE_TWIPS}\"/></w:tblGrid>\
<w:tr>{}{}{}</w:tr></w:tbl>",
            cell(EQ_SIDE_TWIPS, "<w:p><w:pPr><w:pStyle w:val=\"EquationNumber\"/></w:pPr></w:p>"),
            cell(
                EQ_MIDDLE_TWIPS,
                &format!("<w:p><w:pPr><w:pStyle w:val=\"Equation\"/></w:pPr>{bookmark}{content}</w:p>")
            ),
            cell(
                EQ_SIDE_TWIPS,
                &format!(
                    "<w:p><w:pPr><w:pStyle w:val=\"EquationNumber\"/></w:pPr>{}</w:p>",
                    text_runs(number, "")
                )
            ),
        ));
        self.after_table = true;
    }

    /// Writes a float: the rebuilt table or the picture, and the caption
    /// on the side it had in the paper. A float whose text is all code is
    /// a listing.
    fn figure(&mut self, block: &Block, kind: FloatKind, image: Option<&ImageSpec>, label: &Option<String>) {
        let mut anchor = block.anchor.as_deref();
        let caption = (!block.spans.is_empty()).then(|| self.caption_runs(&block.spans));
        if kind == FloatKind::Algorithm
            && block.table.is_none()
            && let Some(lines) = code::float_listing_lines(block)
        {
            if let Some(caption) = &caption {
                self.paragraph("Caption", "<w:keepNext/>", anchor.take(), caption);
            }
            return self.listing(&lines, anchor);
        }
        let caption_first = match (image, block.lines.first()) {
            (Some(spec), Some(line)) => line.bbox.cy() < spec.rect.cy(),
            _ => false,
        };
        if caption_first && let Some(caption) = &caption {
            self.paragraph("Caption", "<w:keepNext/>", anchor.take(), caption);
        }
        if let Some(table) = &block.table {
            self.before_table();
            let bookmark = self.bookmark(anchor.take());
            let xml = table_xml(table, &mut |spans| self.runs(spans));
            self.xml.push_str(&bookmark);
            self.xml.push_str(&xml);
            self.after_table = true;
        } else if let Some(spec) = image {
            let run = self.image_run(
                spec,
                &figure_alt(block, label),
                TEXT_WIDTH_TWIPS as f32 / TWIPS_PER_PT,
                1.0,
            );
            let keep = if caption.is_some() && !caption_first {
                "<w:keepNext/>"
            } else {
                ""
            };
            self.paragraph("Figure", keep, anchor.take(), &run);
        }
        if !caption_first && let Some(caption) = &caption {
            self.paragraph("Caption", "", anchor.take(), caption);
        }
    }

    /// The runs of a caption, with its label ("Figure 3:") in bold.
    fn caption_runs(&mut self, spans: &[Span]) -> String {
        if let Some(first) = spans.first()
            && let Some(found) = CAP_LABEL.find(&first.text)
        {
            let label = first.text[..found.end()].trim_end();
            let mut rest = spans.to_vec();
            rest[0].text = first.text[label.len()..].to_string();
            let bold = run_props(first.style | style::BOLD, first.color, None);
            return text_runs(label, &bold) + &self.runs(&rest);
        }
        self.runs(spans)
    }

    /// A placeholder for the picture of `spec`, at `scale` times its size
    /// in the PDF, but at most `max_width_pt` wide and
    /// [`MAX_IMAGE_HEIGHT_PT`] high. The picture is drawn later.
    fn image_run(&mut self, spec: &ImageSpec, alt: &str, max_width_pt: f32, scale: f32) -> String {
        let width = spec.rect.width().max(1.0) * scale;
        let height = spec.rect.height().max(1.0) * scale;
        let fit = (max_width_pt / width).min(MAX_IMAGE_HEIGHT_PT / height).min(1.0);
        self.images.push(PendingImage {
            spec: *spec,
            width_emu: (width * fit * EMU_PER_PT).round() as u64,
            height_emu: (height * fit * EMU_PER_PT).round() as u64,
            alt: alt.to_string(),
        });
        format!("{MARK}{}{MARK}", self.images.len() - 1)
    }
}

/// The spans after the label `label` at the start of their text (and the
/// spaces after it), or `None` when the text does not start with it.
fn strip_label(spans: &[Span], label: &str) -> Option<Vec<Span>> {
    let first = spans.first()?;
    let rest = first.text.strip_prefix(label).filter(|_| !label.is_empty())?;
    let mut spans = spans.to_vec();
    spans[0].text = rest.trim_start().to_string();
    Some(spans)
}
