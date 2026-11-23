//! The content documents of a book (XHTML) read into the intermediate form
//! of a Word document ([`WordDoc`]): block elements become paragraphs with
//! the style a Word paragraph would have (`<h2>` → "heading 2", `<pre>` →
//! "Source Code", `<figcaption>` → "Caption"), inline elements give the
//! runs their style and links, and lists, tables, pictures, formulas and
//! notes their own items.
//!
//! A note is the target of a link marked as a note reference
//! (`epub:type="noteref"` or `role="doc-noteref"`): it leaves the flow of
//! the text and follows the paragraph that calls it, as a Word footnote.
//! Every element with an `id` is a bookmark, named by its `id` (`path#id`
//! when several documents use it; `path#` for the start of a document), so
//! links between documents keep working.

use super::css::{Props, Sheet};
use super::{entities, mathml, parse, resolve};
use crate::model::{BOLD, ITALIC, MONO, Picture, SUB, SUP, Span};
use crate::word::ir::{Cell, Inline, Item, ListMark, Note, Para, Row, Table, VMerge, WordDoc};
use crate::word::zip::ZipArchive;
use crate::word::{normalize_picture, pixel_size};
use anyhow::{Context, Result};
use roxmltree::{Document as Xml, Node};
use std::collections::{HashMap, HashSet};

/// Namespace of the EPUB structural semantics (`epub:type`).
const OPS_NS: &str = "http://www.idpf.org/2007/ops";
/// Namespace of XLink (`xlink:href` of a picture in an SVG).
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";
/// Namespace of SVG.
const SVG_NS: &str = "http://www.w3.org/2000/svg";
/// Points per CSS pixel.
const PT_PER_PX: f32 = 0.75;
/// Points per inch.
const PT_PER_IN: f32 = 72.0;
/// Points per centimetre.
const PT_PER_CM: f32 = PT_PER_IN / 2.54;
/// Points per pica.
const PT_PER_PC: f32 = 12.0;
/// Font size of the text of a book, in points: a book leaves it to the
/// reader.
const BODY_SIZE_PT: f32 = 12.0;
/// Paragraph styles of the headings, by level.
const HEADING_STYLES: [&str; 6] = [
    "heading 1",
    "heading 2",
    "heading 3",
    "heading 4",
    "heading 5",
    "heading 6",
];
/// Paragraph style of the title.
const TITLE_STYLE: &str = "Title";
/// Paragraph style of a subtitle.
const SUBTITLE_STYLE: &str = "Subtitle";
/// Paragraph style of an author.
const AUTHOR_STYLE: &str = "Author";
/// Paragraph style of a caption.
const CAPTION_STYLE: &str = "Caption";
/// Paragraph style of preformatted text.
const CODE_STYLE: &str = "Source Code";
/// Elements whose content is not part of the text.
const SKIPPED: [&str; 7] = ["head", "script", "style", "nav", "template", "noscript", "title"];
/// Elements that hold blocks: each starts and ends a paragraph.
const BLOCKS: [&str; 24] = [
    "body",
    "div",
    "p",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "aside",
    "blockquote",
    "figure",
    "center",
    "address",
    "dl",
    "dt",
    "dd",
    "hgroup",
    "li",
    "details",
    "summary",
    "fieldset",
    "legend",
    "figcaption",
    "caption",
];
/// Semantics (`epub:type`, `role`) of a page break marker, which holds
/// no text of the book.
const PAGE_BREAKS: [&str; 2] = ["pagebreak", "doc-pagebreak"];
/// Semantics of a link to a note.
const NOTE_REFS: [&str; 2] = ["noteref", "doc-noteref"];
/// Semantics of the link from a note back to its call.
const BACK_LINKS: [&str; 3] = ["backlink", "doc-backlink", "referrer"];

/// What applies to the content of an element.
#[derive(Debug, Clone, Default)]
struct Ctx {
    /// Style flags of the runs.
    style: u8,
    /// Link of the runs.
    href: Option<String>,
    /// White space is kept (`<pre>`).
    pre: bool,
    /// Paragraph style of the paragraphs.
    para_style: &'static str,
    /// Outline level of the paragraphs (headings).
    outline: Option<u8>,
    /// Nesting level of the lists the content is in.
    list_depth: u8,
    /// The content is a note.
    in_note: bool,
}

/// Paragraphs and tables being read, with the paragraph still open.
#[derive(Debug, Default)]
struct Flow {
    /// The items so far.
    items: Vec<Item>,
    /// The open paragraph.
    para: Option<Para>,
    /// Bookmarks for the next paragraph.
    ids: Vec<String>,
    /// List numbering of the next paragraph.
    list: Option<ListMark>,
}

/// One content document of the book.
struct Page<'a> {
    /// Its path in the archive.
    path: &'a str,
    /// Its style sheets.
    sheet: Sheet,
}

/// Reads the content documents of a book.
struct Reader<'a> {
    /// The archive.
    zip: &'a ZipArchive<'a>,
    /// The pictures read so far.
    pictures: Vec<Picture>,
    /// Index of the picture of each path; `None` when it cannot be shown.
    picture_index: HashMap<String, Option<usize>>,
    /// Style sheets read so far, by path.
    sheets: HashMap<String, String>,
    /// The notes read so far.
    notes: Vec<Note>,
    /// The bookmarks (`path#id`) of the notes: targets of note links.
    note_ids: HashSet<String>,
    /// The bookmarks of the note links, which a note links back to.
    noteref_ids: HashSet<String>,
    /// How many elements of the book have each `id`.
    id_counts: HashMap<String, usize>,
    /// The title of the book, reduced for comparing.
    title_key: Option<String>,
    /// A heading was read: what comes after it is not front matter.
    seen_heading: bool,
    /// The note being read started with a link back to its call.
    note_backlink: bool,
}

/// Reads the content documents `spine` of a book (`title` and `creators`
/// from its metadata) into the form of a Word document.
///
/// # Errors
/// When a content document is missing or is not well-formed XML.
pub(crate) fn read_book(
    zip: &ZipArchive<'_>,
    spine: &[String],
    title: Option<&str>,
    creators: &[String],
) -> Result<WordDoc> {
    let mut texts = Vec::new();
    for path in spine {
        let text = zip
            .read_text(path)?
            .with_context(|| format!("the EPUB has no content document {path}"))?;
        // A document with HTML's named references parses once they are
        // numbers (parsed twice only then: most documents have none).
        let text = if entities::has_html_names(&text) && parse(&text).is_err() {
            entities::to_numeric(&text)
        } else {
            text
        };
        texts.push((path.as_str(), text));
    }
    let docs = texts
        .iter()
        .map(|(path, text)| parse(text).with_context(|| format!("reading {path}")))
        .collect::<Result<Vec<Xml<'_>>>>()?;
    let mut reader = Reader {
        zip,
        pictures: Vec::new(),
        picture_index: HashMap::new(),
        sheets: HashMap::new(),
        notes: Vec::new(),
        note_ids: HashSet::new(),
        noteref_ids: HashSet::new(),
        id_counts: HashMap::new(),
        title_key: title.map(text_key),
        seen_heading: false,
        note_backlink: false,
    };
    for doc in &docs {
        for id in doc.descendants().filter_map(|node| node.attribute("id")) {
            *reader.id_counts.entry(id.to_string()).or_default() += 1;
        }
    }
    for ((path, _), doc) in texts.iter().zip(&docs) {
        reader.find_notes(path, doc);
    }
    let mut flow = Flow::default();
    for ((path, _), doc) in texts.iter().zip(&docs) {
        let page = Page {
            path,
            sheet: reader.sheet(path, doc),
        };
        flow.ids.push(format!("{path}#"));
        let body = doc
            .root_element()
            .children()
            .find(|node| node.has_tag_name_local("body"))
            .unwrap_or(doc.root_element());
        reader.element(&page, body, &Ctx::default(), &mut flow);
        flow.flush();
    }
    let mut body = flow.finish();
    front_matter(&mut body, title, creators);
    raise_headings(&mut body);
    Ok(WordDoc {
        body,
        notes: reader.notes,
        pictures: reader.pictures,
        title: title.map(str::to_string),
        body_size_pt: BODY_SIZE_PT,
        body_font: String::new(),
    })
}

/// Local names of elements, whatever their namespace.
trait LocalName {
    /// Whether the node is an element with the local name `name`.
    fn has_tag_name_local(&self, name: &str) -> bool;
}

impl LocalName for Node<'_, '_> {
    fn has_tag_name_local(&self, name: &str) -> bool {
        self.is_element() && self.tag_name().name().eq_ignore_ascii_case(name)
    }
}

impl Flow {
    /// The open paragraph, opened with the style of `ctx` when there is
    /// none.
    fn para(&mut self, ctx: &Ctx) -> &mut Para {
        let ids = &mut self.ids;
        let list = &mut self.list;
        self.para.get_or_insert_with(|| Para {
            style: ctx.para_style.to_string(),
            outline: ctx.outline,
            list: list.take(),
            inlines: Vec::new(),
            bookmarks: std::mem::take(ids),
        })
    }

    /// Closes the open paragraph: it becomes an item when it holds
    /// anything, its bookmarks go to the next one otherwise.
    fn flush(&mut self) {
        let Some(mut para) = self.para.take() else {
            return;
        };
        let has_content = para.inlines.iter().any(|inline| match inline {
            Inline::Text(span) => !span.text.trim().is_empty(),
            _ => true,
        });
        if has_content {
            if let Some(Inline::Text(span)) = para.inlines.last_mut()
                && para.style != CODE_STYLE
            {
                span.text = span.text.trim_end().to_string();
            }
            self.items.push(Item::Para(para));
        } else {
            para.bookmarks.append(&mut self.ids);
            self.ids = para.bookmarks;
            if self.list.is_none() {
                self.list = para.list;
            }
        }
    }

    /// The items, with the bookmarks left at the end in an empty
    /// paragraph.
    fn finish(mut self) -> Vec<Item> {
        self.flush();
        if !self.ids.is_empty() {
            self.items.push(Item::Para(Para {
                bookmarks: self.ids,
                ..Default::default()
            }));
        }
        self.items
    }

    /// Records a bookmark at the element being read: in the open
    /// paragraph for an element inside it, for the next one otherwise.
    fn bookmark(&mut self, name: String, inline: bool) {
        match &mut self.para {
            Some(para) if inline => para.bookmarks.push(name),
            _ => self.ids.push(name),
        }
    }
}

impl<'a> Reader<'a> {
    /// The name of the bookmark of the element `id` of the document at
    /// `path` (its start when `id` is empty): the `id` alone when no other
    /// element of the book has it, which gives the block a readable
    /// anchor, `path#id` otherwise.
    fn key(&self, path: &str, id: &str) -> String {
        if !id.is_empty() && self.id_counts.get(id) == Some(&1) {
            id.to_string()
        } else {
            format!("{path}#{id}")
        }
    }

    /// Records the targets of the note links of the document `doc` at
    /// `path`, and the links themselves.
    fn find_notes(&mut self, path: &str, doc: &Xml<'_>) {
        for link in doc.descendants().filter(|node| node.has_tag_name_local("a")) {
            if !has_semantics(link, &NOTE_REFS) {
                continue;
            }
            if let Some(href) = link.attribute("href").filter(|href| !is_external(href)) {
                let (target, fragment) = resolve(path, href);
                if !fragment.is_empty() {
                    self.note_ids.insert(self.key(&target, &fragment));
                }
            }
            if let Some(id) = link.attribute("id") {
                self.noteref_ids.insert(self.key(path, id));
            }
        }
    }

    /// The style sheets of the document `doc` at `path`: the linked
    /// sheets, then its `<style>` elements.
    fn sheet(&mut self, path: &str, doc: &Xml<'_>) -> Sheet {
        let mut sheet = Sheet::default();
        let Some(head) = doc
            .root_element()
            .children()
            .find(|node| node.has_tag_name_local("head"))
        else {
            return sheet;
        };
        for node in head.children().filter(Node::is_element) {
            if node.has_tag_name_local("style") {
                sheet.add(node.text().unwrap_or_default());
            } else if node.has_tag_name_local("link")
                && node.attribute("rel").is_some_and(|rel| {
                    rel.split_whitespace()
                        .any(|word| word.eq_ignore_ascii_case("stylesheet"))
                })
                && let Some(href) = node.attribute("href")
            {
                let (css_path, _) = resolve(path, href);
                let zip = self.zip;
                let text = self
                    .sheets
                    .entry(css_path)
                    .or_insert_with_key(|key| zip.read_text(key).ok().flatten().unwrap_or_default());
                sheet.add(text);
            }
        }
        sheet
    }

    /// Reads the element `node` (and its content) into `flow`.
    fn element(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow) {
        let name = node.tag_name().name().to_ascii_lowercase();
        let id = node.attribute("id").map(|id| self.key(page.path, id));
        if id.as_ref().is_some_and(|id| self.note_ids.contains(id)) {
            let id = id.unwrap_or_default();
            let body = self.note_body(page, node, ctx);
            self.notes.push(Note {
                endnote: false,
                id,
                body,
            });
            return;
        }
        let classes: Vec<&str> = node.attribute("class").unwrap_or_default().split_whitespace().collect();
        let props = page.sheet.props(&name, &classes, node.attribute("style"));
        if SKIPPED.contains(&name.as_str()) || props.hidden || has_semantics(node, &PAGE_BREAKS) {
            if let Some(id) = id {
                flow.bookmark(id, true);
            }
            return;
        }
        let is_block = BLOCKS.contains(&name.as_str()) || is_heading(&name).is_some();
        if is_block {
            flow.flush();
        }
        if let Some(id) = id {
            flow.bookmark(id, !is_block);
        }
        let ctx = &self.inner_ctx(node, &name, &classes, &props, ctx);
        match name.as_str() {
            "br" => self.text(ctx, flow, "\n"),
            "hr" => flow.flush(),
            "img" => self.image(page, node, ctx, flow),
            "svg" => self.svg(page, node, ctx, flow),
            "math" => {
                if let Some((math, display)) = mathml::read(node) {
                    flow.para(ctx).inlines.push(Inline::Math { node: math, display });
                }
            }
            "ul" | "ol" => self.list(page, node, ctx, flow, (!props.no_markers).then_some(name == "ol")),
            "table" => self.table(page, node, ctx, flow),
            "a" if self.link(page, node, ctx, flow) => {}
            _ => self.children(page, node, ctx, flow),
        }
        if is_block {
            flow.flush();
        } else if props.gap {
            self.text(ctx, flow, " ");
        }
        // A title is not a heading: the authors may come after it.
        if ctx.outline.is_some() && is_heading(&name).is_some() {
            self.seen_heading = true;
        }
    }

    /// What applies to the content of the element `node`, named `name`,
    /// inside `ctx`: its style flags, paragraph style and white space.
    fn inner_ctx(&self, node: Node<'_, '_>, name: &str, classes: &[&str], props: &Props, ctx: &Ctx) -> Ctx {
        let mut inner = ctx.clone();
        inner.style |= match name {
            "b" | "strong" => BOLD,
            "i" | "em" | "cite" | "var" | "dfn" => ITALIC,
            "code" | "tt" | "kbd" | "samp" => MONO,
            "sup" => SUP,
            "sub" => SUB,
            _ => 0,
        };
        inner.style = props.apply(inner.style);
        let has_class = |class: &str| classes.iter().any(|known| known.eq_ignore_ascii_case(class));
        if let Some(level) = is_heading(name) {
            let is_title = has_class("title") && self.title_key.as_deref() == Some(&text_key(&node_text(node)));
            (inner.para_style, inner.outline) = if is_title {
                (TITLE_STYLE, None)
            } else if has_class("subtitle") {
                (SUBTITLE_STYLE, None)
            } else {
                (HEADING_STYLES[usize::from(level) - 1], Some(level - 1))
            };
            inner.style &= !BOLD;
        } else if BLOCKS.contains(&name) {
            inner.para_style = if has_class("author") && !self.seen_heading && !ctx.in_note {
                AUTHOR_STYLE
            } else if has_class("subtitle") && !self.seen_heading {
                SUBTITLE_STYLE
            } else {
                ctx.para_style
            };
        }
        match name {
            "pre" => {
                inner.pre = true;
                inner.para_style = CODE_STYLE;
                inner.style |= MONO;
            }
            "figcaption" | "caption" => {
                inner.para_style = CAPTION_STYLE;
                inner.outline = None;
            }
            _ => {}
        }
        inner
    }

    /// Reads the children of `node`.
    fn children(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow) {
        for child in node.children() {
            if child.is_element() {
                self.element(page, child, ctx, flow);
            } else if let Some(text) = child.text().filter(|_| child.is_text()) {
                self.text(ctx, flow, text);
            }
        }
    }

    /// Adds the text `text` to the open paragraph (opened when the text is
    /// more than white space). White space is collapsed outside `<pre>`.
    fn text(&mut self, ctx: &Ctx, flow: &mut Flow, text: &str) {
        let mut text = if ctx.pre || text == "\n" {
            text.to_string()
        } else {
            collapse_spaces(text)
        };
        let open = flow.para.as_ref().is_some_and(|para| !para.inlines.is_empty());
        if !open && !ctx.pre && text.trim().is_empty() {
            return;
        }
        let para = flow.para(ctx);
        // A line break ends its line: the spaces before it go.
        if text == "\n"
            && !ctx.pre
            && let Some(Inline::Text(span)) = para.inlines.last_mut()
        {
            span.text.truncate(span.text.trim_end().len());
        }
        let after_space = para.inlines.last().is_none_or(|inline| match inline {
            Inline::Text(span) => span.text.ends_with(char::is_whitespace),
            _ => false,
        });
        if after_space && !ctx.pre && text != "\n" {
            text = text.trim_start().to_string();
        }
        if ctx.pre && para.inlines.is_empty() {
            text = text.strip_prefix('\n').map(str::to_string).unwrap_or(text);
        }
        if text.is_empty() {
            return;
        }
        if self.note_backlink {
            text = text.trim_start_matches(['.', ':', ')', ']', ' ']).to_string();
            if text.is_empty() {
                return;
            }
            self.note_backlink = false;
        }
        let mut span = Span::styled(text, ctx.style);
        span.href.clone_from(&ctx.href);
        para.inlines.push(Inline::Text(span));
    }

    /// Reads a link `<a>`: a note reference becomes the mark of its note,
    /// a link back from a note is dropped, any other link gives its
    /// content its target. Returns false when the content is still to be
    /// read (`ctx` does not carry the link then: the caller reads it with
    /// the link given here).
    fn link(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow) -> bool {
        let Some(href) = node.attribute("href") else {
            return false;
        };
        let target = if is_external(href) {
            href.to_string()
        } else {
            let (path, fragment) = resolve(page.path, href);
            self.key(&path, &fragment)
        };
        if ctx.in_note && (has_semantics(node, &BACK_LINKS) || self.noteref_ids.contains(&target)) {
            let empty = flow.para.as_ref().is_none_or(|para| para.inlines.is_empty());
            self.note_backlink |= empty;
            return true;
        }
        if self.note_ids.contains(&target) && has_semantics(node, &NOTE_REFS) {
            flow.para(ctx).inlines.push(Inline::NoteRef {
                endnote: false,
                id: target,
            });
            return true;
        }
        let mut inner = ctx.clone();
        inner.href = Some(if is_external(href) {
            target
        } else {
            format!("#{target}")
        });
        self.children(page, node, &inner, flow);
        true
    }

    /// The paragraphs of a note.
    fn note_body(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx) -> Vec<Item> {
        let ctx = Ctx {
            in_note: true,
            style: ctx.style,
            ..Default::default()
        };
        let mut flow = Flow::default();
        self.note_backlink = false;
        self.children(page, node, &ctx, &mut flow);
        self.note_backlink = false;
        flow.finish()
    }

    /// Reads a list: each item a paragraph with its marker (a number when
    /// `ordered` is `Some(true)`, as for `<ol>`; none for the bullets of
    /// `<ul>`), nested lists one level deeper. The items of a list that
    /// shows no markers (`ordered` is `None`: `list-style: none`, as a
    /// bibliography often is) are plain paragraphs.
    fn list(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow, ordered: Option<bool>) {
        flow.flush();
        let level = ctx.list_depth;
        let inner = Ctx {
            list_depth: level.saturating_add(1),
            para_style: "",
            outline: None,
            ..ctx.clone()
        };
        let kind = node.attribute("type").unwrap_or("1").to_string();
        let mut number: i64 = node
            .attribute("start")
            .and_then(|start| start.trim().parse().ok())
            .unwrap_or(1);
        for child in node.children().filter(Node::is_element) {
            let is_note = child
                .attribute("id")
                .is_some_and(|id| self.note_ids.contains(&self.key(page.path, id)));
            if !child.has_tag_name_local("li") || is_note {
                self.element(page, child, &inner, flow);
                continue;
            }
            if let Some(value) = child.attribute("value").and_then(|value| value.trim().parse().ok()) {
                number = value;
            }
            flow.flush();
            flow.list = ordered.map(|ordered| ListMark {
                level,
                marker: ordered.then(|| format!("{}.", list_number(number, &kind))),
            });
            self.element(page, child, &inner, flow);
            flow.flush();
            flow.list = None;
            number += 1;
        }
    }

    /// Reads a table: its caption (a paragraph before it), then its rows.
    fn table(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow) {
        flow.flush();
        let inner = Ctx {
            para_style: "",
            outline: None,
            list_depth: 0,
            ..ctx.clone()
        };
        for caption in node.children().filter(|child| child.has_tag_name_local("caption")) {
            self.element(page, caption, &inner, flow);
        }
        let mut rows: Vec<(Node<'_, '_>, bool)> = Vec::new();
        for child in node.children().filter(Node::is_element) {
            let header = child.has_tag_name_local("thead");
            if child.has_tag_name_local("tr") {
                rows.push((child, false));
            } else if ["thead", "tbody", "tfoot"]
                .iter()
                .any(|part| child.has_tag_name_local(part))
            {
                rows.extend(
                    child
                        .children()
                        .filter(|row| row.has_tag_name_local("tr"))
                        .map(|row| (row, header)),
                );
            }
        }
        let mut table = Table::default();
        // Cells of the rows above that span down: (column, rows left, span).
        let mut down: Vec<(usize, usize, usize)> = Vec::new();
        for (row, header) in rows {
            let cells: Vec<Node<'_, '_>> = row
                .children()
                .filter(|cell| cell.has_tag_name_local("td") || cell.has_tag_name_local("th"))
                .collect();
            let header = header || (!cells.is_empty() && cells.iter().all(|cell| cell.has_tag_name_local("th")));
            let cells = self.row_cells(page, &cells, &inner, &mut down);
            table.rows.push(Row { cells, header });
        }
        if !table.rows.is_empty() {
            flow.flush();
            let ids = std::mem::take(&mut flow.ids);
            if !ids.is_empty() {
                flow.items.push(Item::Para(Para {
                    bookmarks: ids,
                    ..Default::default()
                }));
            }
            flow.items.push(Item::Table(table));
        }
    }

    /// The cells of one table row: the cells `cells`, with the cells that
    /// span down from the rows above (`down`) as merged cells in their
    /// columns.
    fn row_cells(
        &mut self,
        page: &Page<'_>,
        cells: &[Node<'_, '_>],
        ctx: &Ctx,
        down: &mut Vec<(usize, usize, usize)>,
    ) -> Vec<Cell> {
        let mut out = Vec::new();
        let mut col = 0;
        let merged = |col: &mut usize, out: &mut Vec<Cell>, down: &mut Vec<(usize, usize, usize)>| {
            while let Some(at) = down.iter().position(|&(start, _, _)| start == *col) {
                let (_, left, span) = down[at];
                out.push(Cell {
                    span,
                    merge: VMerge::Continue,
                    body: Vec::new(),
                });
                if left <= 1 {
                    down.remove(at);
                } else {
                    down[at].1 -= 1;
                }
                *col += span;
            }
        };
        for cell in cells {
            merged(&mut col, &mut out, down);
            let span = count_attr(*cell, "colspan");
            let rows = count_attr(*cell, "rowspan");
            let mut flow = Flow::default();
            self.children(page, *cell, ctx, &mut flow);
            if rows > 1 {
                down.push((col, rows - 1, span));
            }
            out.push(Cell {
                span,
                merge: if rows > 1 { VMerge::Restart } else { VMerge::None },
                body: flow.finish(),
            });
            col += span;
        }
        // Cells spanning down past the last cell of the row.
        let mut rest: Vec<usize> = down
            .iter()
            .map(|&(start, _, _)| start)
            .filter(|&start| start >= col)
            .collect();
        rest.sort_unstable();
        for start in rest {
            col = col.max(start);
            merged(&mut col, &mut out, down);
        }
        out
    }

    /// Reads a picture `<img>`.
    fn image(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow) {
        let Some(src) = node.attribute("src") else {
            return;
        };
        let index = self.picture(page.path, src);
        let alt = node.attribute("alt").unwrap_or_default().trim().to_string();
        self.push_picture(node, index, alt, ctx, flow);
    }

    /// Reads an `<svg>`: the picture it shows (`<image>`, as a book's cover
    /// often is), or the drawing itself.
    fn svg(&mut self, page: &Page<'_>, node: Node<'_, '_>, ctx: &Ctx, flow: &mut Flow) {
        let linked = node
            .descendants()
            .find(|child| child.has_tag_name_local("image"))
            .and_then(|image| image.attribute((XLINK_NS, "href")).or_else(|| image.attribute("href")));
        let index = match linked {
            Some(href) => self.picture(page.path, href),
            None => {
                let mut svg = node.document().input_text()[node.range()].to_string();
                if !svg.contains("xmlns=") {
                    svg = svg.replacen("<svg", &format!("<svg xmlns=\"{SVG_NS}\""), 1);
                }
                if svg.contains("xlink:") && !svg.contains("xmlns:xlink") {
                    svg = svg.replacen("<svg", &format!("<svg xmlns:xlink=\"{XLINK_NS}\""), 1);
                }
                normalize_picture(svg.into_bytes()).map(|picture| {
                    self.pictures.push(picture);
                    self.pictures.len() - 1
                })
            }
        };
        let alt = node
            .children()
            .find(|child| child.has_tag_name_local("title"))
            .map(node_text)
            .unwrap_or_default();
        self.push_picture(node, index, alt, ctx, flow);
    }

    /// Adds a picture (`index` into the pictures) to the open paragraph, at
    /// the size the element `node` gives it in its attributes or its
    /// `style`; a size given on one side only keeps the shape of the
    /// picture.
    fn push_picture(&self, node: Node<'_, '_>, index: Option<usize>, alt: String, ctx: &Ctx, flow: &mut Flow) {
        let style = node.attribute("style").unwrap_or_default();
        let size = |name: &str| {
            node.attribute(name)
                .and_then(length_pt)
                .or_else(|| style_length_pt(style, name))
        };
        let (mut width, mut height) = (size("width"), size("height"));
        let pixels = index
            .and_then(|index| self.pictures.get(index))
            .and_then(|picture| pixel_size(&picture.data))
            .filter(|&(px_width, px_height)| px_width > 0.0 && px_height > 0.0);
        if let Some((px_width, px_height)) = pixels {
            match (width, height) {
                (Some(known), None) => height = Some(known * px_height / px_width),
                (None, Some(known)) => width = Some(known * px_width / px_height),
                _ => {}
            }
        }
        flow.para(ctx).inlines.push(Inline::Picture {
            index,
            width_pt: width.unwrap_or(0.0),
            height_pt: height.unwrap_or(0.0),
            alt,
        });
    }

    /// The index of the picture at `href` (from the document at `base`),
    /// read once; `None` when it is missing or cannot be shown.
    fn picture(&mut self, base: &str, href: &str) -> Option<usize> {
        let (path, _) = resolve(base, href);
        if let Some(index) = self.picture_index.get(&path) {
            return *index;
        }
        let index = self
            .zip
            .read(&path)
            .ok()
            .flatten()
            .and_then(normalize_picture)
            .map(|picture| {
                self.pictures.push(picture);
                self.pictures.len() - 1
            });
        self.picture_index.insert(path, index);
        index
    }
}

/// A CSS length (`3in`, `120px`, `120`) in points; `None` for a relative
/// one (`50%`, `2em`) or no length.
fn length_pt(value: &str) -> Option<f32> {
    let value = value.trim();
    let split = value
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(value.len());
    let number: f32 = value[..split].parse().ok()?;
    let scale = match value[split..].trim() {
        "" | "px" => PT_PER_PX,
        "pt" => 1.0,
        "in" => PT_PER_IN,
        "cm" => PT_PER_CM,
        "mm" => PT_PER_CM / 10.0,
        "pc" => PT_PER_PC,
        _ => return None,
    };
    Some(number * scale)
}

/// The length of the property `name` in the declarations `style`, in
/// points.
fn style_length_pt(style: &str, name: &str) -> Option<f32> {
    style.split(';').find_map(|declaration| {
        let (property, value) = declaration.split_once(':')?;
        property
            .trim()
            .eq_ignore_ascii_case(name)
            .then(|| length_pt(value))
            .flatten()
    })
}

/// Gives the book its title and authors at the start, from its metadata,
/// when its text does not: the first heading becomes the title when it is
/// the title, a title paragraph is added otherwise.
fn front_matter(body: &mut Vec<Item>, title: Option<&str>, creators: &[String]) {
    let styled = |body: &[Item], style: &str| {
        body.iter()
            .position(|item| matches!(item, Item::Para(para) if para.style == style))
    };
    let mut at = styled(body, TITLE_STYLE);
    if at.is_none()
        && let Some(title) = title
    {
        let first_heading = body.iter_mut().enumerate().find_map(|(idx, item)| match item {
            Item::Para(para) if para.outline.is_some() => Some((idx, para)),
            _ => None,
        });
        at = match first_heading {
            Some((idx, para)) if text_key(&para.text()) == text_key(title) => {
                para.style = TITLE_STYLE.to_string();
                para.outline = None;
                Some(idx)
            }
            _ => {
                body.insert(0, plain_para(TITLE_STYLE, title));
                Some(0)
            }
        };
    }
    if styled(body, AUTHOR_STYLE).is_none() {
        let start = at.map_or(0, |idx| idx + 1);
        for (offset, creator) in creators.iter().enumerate() {
            body.insert(start + offset, plain_para(AUTHOR_STYLE, creator));
        }
    }
}

/// Raises every heading by the same number of levels, so that the
/// highest is of level 1: many books keep `<h1>` for their title, or use
/// none, and start their chapters at `<h2>` or lower.
fn raise_headings(body: &mut [Item]) {
    let headings = || {
        body.iter().filter_map(|item| match item {
            Item::Para(para) => para.outline,
            Item::Table(_) => None,
        })
    };
    let Some(top) = headings().min().filter(|&top| top > 0) else {
        return;
    };
    for item in body.iter_mut() {
        if let Item::Para(para) = item
            && let Some(level) = &mut para.outline
        {
            *level -= top;
            para.style = HEADING_STYLES[usize::from(*level)].to_string();
        }
    }
}

/// A paragraph of style `style` with the plain text `text`.
fn plain_para(style: &str, text: &str) -> Item {
    Item::Para(Para {
        style: style.to_string(),
        inlines: vec![Inline::Text(Span::plain(text))],
        ..Default::default()
    })
}

/// The level of a heading element (`h1` → 1).
fn is_heading(name: &str) -> Option<u8> {
    let level: u8 = name.strip_prefix('h')?.parse().ok()?;
    (1..=6).contains(&level).then_some(level)
}

/// Whether the element has one of the semantics `names`, in `epub:type`
/// or `role`.
fn has_semantics(node: Node<'_, '_>, names: &[&str]) -> bool {
    [node.attribute((OPS_NS, "type")), node.attribute("role")]
        .into_iter()
        .flatten()
        .flat_map(str::split_whitespace)
        .any(|word| names.contains(&word))
}

/// Whether a link leaves the book.
fn is_external(href: &str) -> bool {
    href.contains("://") || href.starts_with("mailto:") || href.starts_with("tel:")
}

/// The value of a count attribute (`colspan`, `rowspan`): at least 1.
fn count_attr(node: Node<'_, '_>, name: &str) -> usize {
    node.attribute(name)
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

/// The number `number` of an ordered list of type `kind` (`1`, `a`, `A`,
/// `i`, `I`).
fn list_number(number: i64, kind: &str) -> String {
    let Ok(positive) = usize::try_from(number) else {
        return number.to_string();
    };
    match kind {
        "a" | "A" if positive > 0 => {
            let mut letters = String::new();
            let mut rest = positive;
            while rest > 0 {
                rest -= 1;
                letters.insert(0, char::from(b'a' + (rest % 26) as u8));
                rest /= 26;
            }
            if kind == "A" { letters.to_uppercase() } else { letters }
        }
        "i" | "I" if positive > 0 => {
            let roman = crate::word::roman_lower(positive);
            if kind == "I" { roman.to_uppercase() } else { roman }
        }
        _ => number.to_string(),
    }
}

/// `text` with each run of white space made one space.
fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for ch in text.chars() {
        if ch.is_ascii_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
                space = false;
            }
            out.push(ch);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// The text of an element and its descendants.
fn node_text(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(Node::is_text)
        .filter_map(|text| text.text())
        .collect()
}

/// `text` reduced for comparing: lower case, white space collapsed.
fn text_key(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{collapse_spaces, length_pt, list_number, style_length_pt};

    #[test]
    fn list_numbers_and_spaces() {
        assert_eq!(list_number(3, "1"), "3");
        assert_eq!(list_number(28, "a"), "ab");
        assert_eq!(list_number(4, "I"), "IV");
        assert_eq!(collapse_spaces(" a \n\t b "), " a b ");
        assert_eq!(length_pt("3in"), Some(216.0));
        assert_eq!(length_pt("120"), Some(90.0));
        assert_eq!(length_pt("50%"), None);
        assert_eq!(style_length_pt("max-width: 2in; width:1in", "width"), Some(72.0));
    }
}
