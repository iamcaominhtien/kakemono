//! The `.docx` reader: the parts of the package (document, styles,
//! numbering, notes, relationships, pictures) read into a [`WordDoc`].

use super::ir::{Cell, Inline, Item, ListMark, Note, Para, Row, Table, VMerge, WordDoc};
use super::omml;
use super::pictures::normalize;
use super::styles::{Numbering, RunProps, Styles, numbering_ref, outline_level};
use super::symbols::symbol_font_text;
use super::xml::{
    DC_NS, DRAWING_NS, MATH_NS, MC_NS, OFFICE_NS, PACKAGE_REL_NS, REL_NS, VML_NS, WORD_NS, WP_NS, attr, child,
    children, is, val,
};
use super::zip::ZipArchive;
use crate::model::{MathNode, Span};
use anyhow::{Context, Result};
use roxmltree::{Document as Xml, Node, ParsingOptions};
use std::collections::HashMap;

/// The main document part when the package relationships do not name one.
const DEFAULT_DOCUMENT_PART: &str = "word/document.xml";
/// Relationship type of the main document part.
const OFFICE_DOCUMENT_REL: &str = "officeDocument";
/// EMU (English Metric Units) per point: the unit of DrawingML sizes.
const EMU_PER_PT: f32 = 12_700.0;
/// Pixels per point of a VML size given in pixels (96 dpi).
const PT_PER_PX: f32 = 0.75;
/// Points per inch, for VML sizes in inches.
const PT_PER_INCH: f32 = 72.0;
/// Points per centimetre, for VML sizes in centimetres.
const PT_PER_CM: f32 = 72.0 / 2.54;
/// Largest number of XML nodes a part may have (roxmltree's limit is
/// raised for long documents).
const MAX_XML_NODES: u32 = u32::MAX;

/// A relationship of a part: its target and whether it is outside the
/// package (a web address).
#[derive(Debug, Clone)]
pub(crate) struct Rel {
    /// The target: a path in the package (resolved), or a URL.
    pub(crate) target: String,
    /// The target is outside the package.
    pub(crate) external: bool,
}

/// A field (`w:fldChar` ... `w:fldChar`) being read: its instruction, and
/// whether its result has started.
#[derive(Debug, Default)]
struct Field {
    /// The instruction text (`HYPERLINK "..."`, `REF _Ref1 \h`).
    instr: String,
    /// The result (what Word shows) has started.
    in_result: bool,
    /// The link the result points to.
    href: Option<String>,
}

/// What the reader needs while it walks the paragraphs of one part.
struct Reader<'a> {
    /// The package.
    zip: &'a ZipArchive<'a>,
    /// The styles.
    styles: &'a Styles,
    /// The list definitions and counters.
    numbering: &'a mut Numbering,
    /// The pictures read so far.
    pictures: &'a mut Vec<crate::model::Picture>,
    /// Index of each picture read so far, by its path in the package
    /// (`None` when it cannot be shown).
    picture_index: &'a mut HashMap<String, Option<usize>>,
    /// The relationships of the part being read.
    rels: HashMap<String, Rel>,
    /// Fields open at this point of the paragraph.
    fields: Vec<Field>,
    /// Paragraphs of text boxes met in the current paragraph, read after
    /// it.
    boxes: Vec<Item>,
    /// Bookmarks that start between paragraphs, given to the next one.
    pending_bookmarks: Vec<String>,
}

/// Reads a `.docx` file.
///
/// # Errors
/// When `data` is not a ZIP package with a WordprocessingML document, or
/// one of its XML parts is damaged.
pub(crate) fn read(data: &[u8]) -> Result<WordDoc> {
    let zip = ZipArchive::new(data)?;
    let main = main_part(&zip)?;
    let document_xml = zip
        .read_text(&main)?
        .with_context(|| format!("the package has no {main}"))?;
    let document = parse(&document_xml).with_context(|| format!("reading {main}"))?;
    let styles_xml = zip.read_text(&related_part(&zip, &main, "styles"))?;
    let styles_doc = styles_xml.as_deref().map(parse).transpose()?;
    let styles = Styles::parse(styles_doc.as_ref().map(Xml::root_element));
    let numbering_xml = zip.read_text(&related_part(&zip, &main, "numbering"))?;
    let numbering_doc = numbering_xml.as_deref().map(parse).transpose()?;
    let mut numbering = Numbering::parse(numbering_doc.as_ref().map(Xml::root_element));
    let mut pictures = Vec::new();
    let mut picture_index = HashMap::new();
    let mut reader = Reader {
        zip: &zip,
        styles: &styles,
        numbering: &mut numbering,
        pictures: &mut pictures,
        picture_index: &mut picture_index,
        rels: rels_of(&zip, &main)?,
        fields: Vec::new(),
        boxes: Vec::new(),
        pending_bookmarks: Vec::new(),
    };
    let root = document.root_element();
    let body = child(root, WORD_NS, "body").context("the document has no body")?;
    let body = reader.items(body);
    let mut notes = Vec::new();
    for (kind, element, endnote) in [("footnotes", "footnote", false), ("endnotes", "endnote", true)] {
        let part = related_part(&zip, &main, kind);
        let Some(xml) = zip.read_text(&part)? else { continue };
        let notes_doc = parse(&xml).with_context(|| format!("reading {part}"))?;
        reader.rels = rels_of(&zip, &part)?;
        for note in children(notes_doc.root_element(), WORD_NS, element) {
            if matches!(
                attr(note, WORD_NS, "type"),
                Some("separator" | "continuationSeparator" | "continuationNotice")
            ) {
                continue;
            }
            let Some(id) = attr(note, WORD_NS, "id") else { continue };
            notes.push(Note {
                endnote,
                id: id.to_string(),
                body: reader.items(note),
            });
        }
    }
    Ok(WordDoc {
        body,
        notes,
        pictures,
        title: document_title(&zip)?,
        body_size_pt: styles.body_size_pt,
        body_font: styles.body_font.clone(),
    })
}

/// Whether `data` is a `.docx` package (a ZIP archive with a Word
/// document in it).
pub(crate) fn is_docx(data: &[u8]) -> bool {
    data.starts_with(b"PK\x03\x04")
        && ZipArchive::new(data)
            .ok()
            .is_some_and(|zip| main_part(&zip).is_ok_and(|main| zip.contains(&main)))
}

/// Parses an XML part, with DTDs refused and the node limit raised.
pub(crate) fn parse(text: &str) -> Result<Xml<'_>> {
    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MAX_XML_NODES,
    };
    Xml::parse_with_options(text, options).context("damaged XML")
}

/// The path of the main document part, from the package relationships.
pub(crate) fn main_part(zip: &ZipArchive<'_>) -> Result<String> {
    Ok(read_rels(zip, "")?
        .into_iter()
        .find(|(_, kind, rel)| !rel.external && kind == OFFICE_DOCUMENT_REL)
        .map(|(_, _, rel)| rel.target)
        .filter(|target| zip.contains(target))
        .unwrap_or_else(|| DEFAULT_DOCUMENT_PART.to_string()))
}

/// The path of the part related to `part` by a relationship whose type ends
/// with `/kind` (`styles`, `numbering`, `footnotes`), or the usual path
/// next to it.
pub(crate) fn related_part(zip: &ZipArchive<'_>, part: &str, kind: &str) -> String {
    let dir = part.rsplit_once('/').map_or("", |(dir, _)| dir);
    let usual = if dir.is_empty() {
        format!("{kind}.xml")
    } else {
        format!("{dir}/{kind}.xml")
    };
    rel_types_of(zip, part)
        .into_iter()
        .find(|(kind_name, _)| kind_name == kind)
        .map(|(_, target)| target)
        .unwrap_or(usual)
}

/// The relationships of `part` (the package relationships for `""`), by id,
/// with their targets resolved to paths in the package.
pub(crate) fn rels_of(zip: &ZipArchive<'_>, part: &str) -> Result<HashMap<String, Rel>> {
    Ok(read_rels(zip, part)?
        .into_iter()
        .map(|(id, _, rel)| (id, rel))
        .collect())
}

/// The relationships of `part` as (last word of their type, target path).
pub(crate) fn rel_types_of(zip: &ZipArchive<'_>, part: &str) -> Vec<(String, String)> {
    read_rels(zip, part)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, _, rel)| !rel.external)
        .map(|(_, kind, rel)| (kind, rel.target))
        .collect()
}

/// The relationships of `part`: (id, last word of the type, relationship).
pub(crate) fn read_rels(zip: &ZipArchive<'_>, part: &str) -> Result<Vec<(String, String, Rel)>> {
    let (dir, name) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_path = if dir.is_empty() {
        format!("_rels/{name}.rels")
    } else {
        format!("{dir}/_rels/{name}.rels")
    };
    let Some(xml) = zip.read_text(&rels_path)? else {
        return Ok(Vec::new());
    };
    let doc = parse(&xml).with_context(|| format!("reading {rels_path}"))?;
    let mut out = Vec::new();
    for node in children(doc.root_element(), PACKAGE_REL_NS, "Relationship") {
        let (Some(id), Some(target)) = (node.attribute("Id"), node.attribute("Target")) else {
            continue;
        };
        let kind = node.attribute("Type").unwrap_or_default();
        let kind = kind.rsplit('/').next().unwrap_or_default().to_string();
        let external = node.attribute("TargetMode") == Some("External");
        let target = if external {
            target.to_string()
        } else {
            resolve(dir, target)
        };
        out.push((id.to_string(), kind, Rel { target, external }));
    }
    Ok(out)
}

/// The path of `target` relative to the directory `dir`, with `..` and
/// `.` resolved (a target starting with `/` is from the package root).
pub(crate) fn resolve(dir: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        dir.split('/').filter(|part| !part.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// The title in the document properties (`docProps/core.xml`).
pub(crate) fn document_title(zip: &ZipArchive<'_>) -> Result<Option<String>> {
    let Some(xml) = zip.read_text("docProps/core.xml")? else {
        return Ok(None);
    };
    let Ok(doc) = parse(&xml) else { return Ok(None) };
    Ok(doc
        .root_element()
        .children()
        .find(|node| is(*node, DC_NS, "title"))
        .and_then(|node| node.text())
        .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|title| !title.is_empty()))
}

impl Reader<'_> {
    /// The paragraphs and tables of a container (the body, a cell, a note,
    /// a text box), with the content of structured document tags and of
    /// inserted text taken in.
    fn items(&mut self, node: Node<'_, '_>) -> Vec<Item> {
        let mut items = Vec::new();
        for item in node.children().filter(Node::is_element) {
            if item.tag_name().namespace() != Some(WORD_NS) {
                continue;
            }
            match item.tag_name().name() {
                "p" => {
                    let para = self.paragraph(item);
                    items.push(Item::Para(para));
                    items.append(&mut self.boxes);
                }
                "tbl" => items.push(Item::Table(self.table(item))),
                "sdt" => {
                    if let Some(content) = child(item, WORD_NS, "sdtContent") {
                        items.extend(self.items(content));
                    }
                }
                "customXml" | "ins" => items.extend(self.items(item)),
                "bookmarkStart" => {
                    if let Some(name) = attr(item, WORD_NS, "name").filter(|name| *name != "_GoBack") {
                        self.pending_bookmarks.push(name.to_string());
                    }
                }
                _ => {}
            }
        }
        items
    }

    /// A table: its rows and cells, with their spans and merges.
    fn table(&mut self, node: Node<'_, '_>) -> Table {
        let mut table = Table::default();
        for row in children(node, WORD_NS, "tr") {
            let header = child(row, WORD_NS, "trPr")
                .and_then(|props| super::xml::toggle(props, "tblHeader"))
                .unwrap_or(false);
            let mut cells = Vec::new();
            for cell in row
                .children()
                .filter(|item| is(*item, WORD_NS, "tc") || is(*item, WORD_NS, "sdt"))
            {
                let cell = if is(cell, WORD_NS, "sdt") {
                    match child(cell, WORD_NS, "sdtContent").and_then(|content| child(content, WORD_NS, "tc")) {
                        Some(cell) => cell,
                        None => continue,
                    }
                } else {
                    cell
                };
                let props = child(cell, WORD_NS, "tcPr");
                let span = props
                    .and_then(|props| val(props, "gridSpan"))
                    .and_then(|span| span.parse().ok())
                    .unwrap_or(1usize)
                    .max(1);
                let merge = match props.and_then(|props| child(props, WORD_NS, "vMerge")) {
                    Some(merge) if attr(merge, WORD_NS, "val") == Some("restart") => VMerge::Restart,
                    Some(_) => VMerge::Continue,
                    None => VMerge::None,
                };
                cells.push(Cell {
                    span,
                    merge,
                    body: self.items(cell),
                });
            }
            table.rows.push(Row { cells, header });
        }
        table
    }

    /// A paragraph: its style, numbering, bookmarks and content.
    fn paragraph(&mut self, node: Node<'_, '_>) -> Para {
        let props = child(node, WORD_NS, "pPr");
        let style_id = props.and_then(|props| val(props, "pStyle"));
        let (style, style_outline, style_numbering) = self.styles.paragraph(style_id);
        let outline = props.and_then(outline_level).or(style_outline);
        let numbering = props.and_then(numbering_ref).or(style_numbering);
        let list = numbering.and_then(|(num_id, level)| {
            self.numbering
                .next(&num_id, level)
                .map(|marker| ListMark { level, marker })
        });
        let mut para = Para {
            style,
            outline,
            list,
            bookmarks: std::mem::take(&mut self.pending_bookmarks),
            ..Default::default()
        };
        self.fields.clear();
        let base = self.styles.run_props(style_id, None);
        self.content(node, style_id, base, None, &mut para);
        super::display_alone_formula(&mut para);
        para
    }

    /// Reads the content of `node` (a paragraph, or an element that holds
    /// runs: a hyperlink, inserted text, a field) into `para`. `href` is
    /// the link of an enclosing hyperlink.
    fn content(
        &mut self,
        node: Node<'_, '_>,
        style_id: Option<&str>,
        base: RunProps,
        href: Option<&str>,
        para: &mut Para,
    ) {
        for item in node.children().filter(Node::is_element) {
            let ns = item.tag_name().namespace();
            if ns == Some(MATH_NS) {
                match item.tag_name().name() {
                    "oMathPara" => para.inlines.push(Inline::Math {
                        node: omml::formula(item),
                        display: true,
                    }),
                    "oMath" => para.inlines.push(Inline::Math {
                        node: omml::formula(item),
                        display: false,
                    }),
                    _ => {}
                }
                continue;
            }
            if ns == Some(MC_NS) && item.tag_name().name() == "AlternateContent" {
                if let Some(choice) = alternate(item) {
                    self.content(choice, style_id, base, href, para);
                }
                continue;
            }
            if ns != Some(WORD_NS) {
                continue;
            }
            match item.tag_name().name() {
                "r" => self.run(item, style_id, base, href, para),
                "hyperlink" => {
                    let link = self.hyperlink(item);
                    self.content(item, style_id, base, link.as_deref().or(href), para);
                }
                "fldSimple" => {
                    let link = attr(item, WORD_NS, "instr").and_then(field_link);
                    self.content(item, style_id, base, link.as_deref().or(href), para);
                }
                "bookmarkStart" => {
                    if let Some(name) = attr(item, WORD_NS, "name").filter(|name| *name != "_GoBack") {
                        para.bookmarks.push(name.to_string());
                    }
                }
                "ins" | "smartTag" | "customXml" | "dir" | "bdo" => self.content(item, style_id, base, href, para),
                "sdt" => {
                    if let Some(content) = child(item, WORD_NS, "sdtContent") {
                        self.content(content, style_id, base, href, para);
                    }
                }
                _ => {}
            }
        }
    }

    /// The target of a `w:hyperlink`: its web address, or `#bookmark` for
    /// a place in the document.
    fn hyperlink(&self, node: Node<'_, '_>) -> Option<String> {
        if let Some(anchor) = attr(node, WORD_NS, "anchor") {
            return Some(format!("#{anchor}"));
        }
        let id = attr(node, REL_NS, "id")?;
        self.rels.get(id).map(|rel| rel.target.clone())
    }

    /// Reads a run: its text with its style, pictures, note marks and the
    /// field characters that start and end fields.
    fn run(&mut self, node: Node<'_, '_>, style_id: Option<&str>, base: RunProps, href: Option<&str>, para: &mut Para) {
        let props = child(node, WORD_NS, "rPr");
        let run_style = props.and_then(|props| val(props, "rStyle"));
        let mut run_props = if run_style.is_some() {
            self.styles.run_props(style_id, run_style)
        } else {
            base
        };
        if let Some(props) = props {
            run_props = run_props.with(&RunProps::from_node(props));
        }
        let mut text = String::new();
        for item in node.children().filter(Node::is_element) {
            let ns = item.tag_name().namespace();
            if ns == Some(MC_NS) && item.tag_name().name() == "AlternateContent" {
                if let Some(choice) = alternate(item) {
                    self.flush(&mut text, &run_props, href, para);
                    self.objects(choice, para);
                }
                continue;
            }
            if ns != Some(WORD_NS) {
                continue;
            }
            match item.tag_name().name() {
                "fldChar" => {
                    self.flush(&mut text, &run_props, href, para);
                    self.field_char(item);
                }
                "instrText" => {
                    if let Some(field) = self.fields.last_mut() {
                        field.instr.push_str(item.text().unwrap_or_default());
                    }
                }
                _ if self.in_instruction() => {}
                "t" => text.push_str(item.text().unwrap_or_default()),
                "tab" | "ptab" => text.push(' '),
                "br" | "cr" => {
                    if !matches!(attr(item, WORD_NS, "type"), Some("page" | "column")) {
                        text.push('\n');
                    }
                }
                "noBreakHyphen" => text.push('\u{2011}'),
                "sym" => text.extend(super::xml::symbol(item)),
                "footnoteReference" | "endnoteReference" => {
                    self.flush(&mut text, &run_props, href, para);
                    if let Some(id) = attr(item, WORD_NS, "id") {
                        para.inlines.push(Inline::NoteRef {
                            endnote: item.tag_name().name() == "endnoteReference",
                            id: id.to_string(),
                        });
                    }
                }
                "drawing" | "pict" | "object" => {
                    self.flush(&mut text, &run_props, href, para);
                    self.objects(item, para);
                }
                _ => {}
            }
        }
        self.flush(&mut text, &run_props, href, para);
    }

    /// Whether the text read now is the instruction of a field, which Word
    /// does not show.
    fn in_instruction(&self) -> bool {
        self.fields.iter().any(|field| !field.in_result)
    }

    /// Handles a field character: the start of a field, the start of its
    /// result, or its end.
    fn field_char(&mut self, node: Node<'_, '_>) {
        match attr(node, WORD_NS, "fldCharType") {
            Some("begin") => self.fields.push(Field::default()),
            Some("separate") => {
                if let Some(field) = self.fields.last_mut() {
                    field.in_result = true;
                    field.href = field_link(&field.instr);
                }
            }
            Some("end") => {
                self.fields.pop();
            }
            _ => {}
        }
    }

    /// Adds the text read so far as a span with the properties `props`
    /// (hidden text is dropped), and empties it.
    fn flush(&self, text: &mut String, props: &RunProps, href: Option<&str>, para: &mut Para) {
        if text.is_empty() {
            return;
        }
        let taken = std::mem::take(text);
        if props.hidden == Some(true) {
            return;
        }
        let taken = if props.symbol == Some(true) {
            symbol_font_text(&taken)
        } else {
            taken
        };
        let field_href = self.fields.iter().rev().find_map(|field| field.href.clone());
        let mut span = Span::styled(taken, props.style_bits());
        span.color = props.color.unwrap_or([0; 3]);
        span.href = field_href.or_else(|| href.map(str::to_string));
        para.inlines.push(Inline::Text(span));
    }

    /// Reads the pictures and text boxes inside a drawing, a VML picture or
    /// an embedded object.
    fn objects(&mut self, node: Node<'_, '_>, para: &mut Para) {
        // Text boxes: their paragraphs come after this one.
        for content in node.descendants().filter(|item| is(*item, WORD_NS, "txbxContent")) {
            if content
                .ancestors()
                .skip(1)
                .any(|ancestor| is(ancestor, WORD_NS, "txbxContent"))
            {
                continue;
            }
            let fields = std::mem::take(&mut self.fields);
            let items = self.items(content);
            self.fields = fields;
            self.boxes.extend(items);
        }
        // A MathType or Equation Editor formula: its structure rather than
        // its picture.
        if let Some(node) = self.ole_equation(node) {
            para.inlines.push(Inline::Math { node, display: false });
            return;
        }
        if let Some(blip) = node.descendants().find(|item| is(*item, DRAWING_NS, "blip")) {
            let (width_pt, height_pt) = drawing_size(node);
            let alt = node
                .descendants()
                .find(|item| is(*item, WP_NS, "docPr"))
                .and_then(|doc_pr| doc_pr.attribute("descr").or_else(|| doc_pr.attribute("title")))
                .unwrap_or_default()
                .to_string();
            if let Some(id) = attr(blip, REL_NS, "embed") {
                let index = self.picture(id);
                para.inlines.push(Inline::Picture {
                    index,
                    width_pt,
                    height_pt,
                    alt,
                });
            }
            return;
        }
        if let Some(data) = node.descendants().find(|item| is(*item, VML_NS, "imagedata")) {
            let Some(id) = attr(data, REL_NS, "id") else { return };
            let shape = data.parent().filter(|parent| parent.is_element());
            let (width_pt, height_pt) = shape
                .and_then(|shape| shape.attribute("style"))
                .map(vml_size)
                .unwrap_or_default();
            let alt = data
                .attribute(("urn:schemas-microsoft-com:office:office", "title"))
                .unwrap_or_default();
            let index = self.picture(id);
            para.inlines.push(Inline::Picture {
                index,
                width_pt,
                height_pt,
                alt: alt.to_string(),
            });
        }
    }

    /// The formula of an embedded MathType or Equation Editor object in
    /// `node` (`o:OLEObject` with a `ProgID` of `Equation.*`).
    fn ole_equation(&self, node: Node<'_, '_>) -> Option<MathNode> {
        let ole = node.descendants().find(|item| is(*item, OFFICE_NS, "OLEObject"))?;
        if !ole
            .attribute("ProgID")
            .is_some_and(|prog| prog.starts_with("Equation."))
        {
            return None;
        }
        let rel = self.rels.get(attr(ole, REL_NS, "id")?).filter(|rel| !rel.external)?;
        let data = self.zip.read(&rel.target).ok()??;
        super::mtef::equation(&super::equation_native(&data)?)
    }

    /// The index of the picture of relationship `id`, read and converted
    /// once; `None` when it cannot be shown.
    fn picture(&mut self, id: &str) -> Option<usize> {
        let rel = self.rels.get(id).filter(|rel| !rel.external)?;
        if let Some(index) = self.picture_index.get(&rel.target) {
            return *index;
        }
        let index = self
            .zip
            .read(&rel.target)
            .ok()
            .flatten()
            .and_then(normalize)
            .map(|picture| {
                self.pictures.push(picture);
                self.pictures.len() - 1
            });
        self.picture_index.insert(rel.target.clone(), index);
        index
    }
}

/// The content of an `mc:AlternateContent` to read: its first choice
/// (Word's own markup), else its fallback.
pub(crate) fn alternate<'a, 'input>(node: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    child(node, MC_NS, "Choice").or_else(|| child(node, MC_NS, "Fallback"))
}

/// The link of a field instruction: a web address or a bookmark for
/// `HYPERLINK`, a bookmark for `REF` and `PAGEREF` with `\h`.
pub(crate) fn field_link(instr: &str) -> Option<String> {
    let words = field_words(instr);
    let (name, args) = words.split_first()?;
    match name.to_ascii_uppercase().as_str() {
        "HYPERLINK" => {
            let mut target = None;
            let mut bookmark = None;
            let mut iter = args.iter();
            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "\\l" => bookmark = iter.next().cloned(),
                    "\\o" | "\\t" => {
                        iter.next();
                    }
                    switch if switch.starts_with('\\') => {}
                    other if target.is_none() => target = Some(other.to_string()),
                    _ => {}
                }
            }
            match (target, bookmark) {
                (Some(target), Some(bookmark)) => Some(format!("{target}#{bookmark}")),
                (Some(target), None) => Some(target),
                (None, Some(bookmark)) => Some(format!("#{bookmark}")),
                (None, None) => None,
            }
        }
        "REF" | "PAGEREF" | "NOTEREF" => {
            let bookmark = args.first().filter(|arg| !arg.starts_with('\\'))?;
            args.iter().any(|arg| arg == "\\h").then(|| format!("#{bookmark}"))
        }
        _ => None,
    }
}

/// The words of a field instruction: quoted strings are one word without
/// their quotes.
pub(crate) fn field_words(instr: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = instr.chars().peekable();
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
        } else if ch == '"' {
            chars.next();
            let word: String = chars.by_ref().take_while(|&ch| ch != '"').collect();
            words.push(word);
        } else {
            let mut word = String::new();
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                word.push(ch);
                chars.next();
            }
            words.push(word);
        }
    }
    words
}

/// The size of a DrawingML picture in points, from its `wp:extent`.
pub(crate) fn drawing_size(node: Node<'_, '_>) -> (f32, f32) {
    node.descendants()
        .find(|item| is(*item, WP_NS, "extent"))
        .map(|extent| {
            let emu = |name: &str| {
                extent
                    .attribute(name)
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(0.0)
            };
            (emu("cx") / EMU_PER_PT, emu("cy") / EMU_PER_PT)
        })
        .unwrap_or_default()
}

/// The size in points of a VML shape, from its `style` attribute
/// (`width:120pt;height:3.5in`).
pub(crate) fn vml_size(style: &str) -> (f32, f32) {
    let mut size = (0.0, 0.0);
    for declaration in style.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let number: String = value
            .chars()
            .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
            .collect();
        let Ok(number) = number.parse::<f32>() else { continue };
        let points = match value.trim_start_matches(|ch: char| ch.is_ascii_digit() || ch == '.') {
            "in" => number * PT_PER_INCH,
            "cm" => number * PT_PER_CM,
            "mm" => number * PT_PER_CM / 10.0,
            "px" | "" => number * PT_PER_PX,
            _ => number,
        };
        match name.trim() {
            "width" => size.0 = points,
            "height" => size.1 = points,
            _ => {}
        }
    }
    size
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Links of hyperlink and cross-reference fields.
    #[test]
    fn field_links() {
        assert_eq!(
            field_link(" HYPERLINK \"https://arxiv.org/abs/1\" ").as_deref(),
            Some("https://arxiv.org/abs/1")
        );
        assert_eq!(field_link("HYPERLINK \\l \"_Ref12\"").as_deref(), Some("#_Ref12"));
        assert_eq!(field_link(" REF _Ref123 \\h ").as_deref(), Some("#_Ref123"));
        assert_eq!(field_link(" REF _Ref123 ").as_deref(), None);
        assert_eq!(field_link(" PAGE ").as_deref(), None);
    }

    /// Targets are resolved against the directory of their part.
    #[test]
    fn targets_resolve() {
        assert_eq!(resolve("word", "media/image1.png"), "word/media/image1.png");
        assert_eq!(resolve("word", "../customXml/item1.xml"), "customXml/item1.xml");
        assert_eq!(resolve("", "word/document.xml"), "word/document.xml");
        assert_eq!(resolve("word", "/word/media/a.png"), "word/media/a.png");
    }

    /// VML sizes in the units Word writes.
    #[test]
    fn vml_sizes() {
        assert_eq!(vml_size("width:144pt;height:1in"), (144.0, 72.0));
        assert_eq!(vml_size("position:absolute; width:96px ;height:2.54cm"), (72.0, 72.0));
    }
}
