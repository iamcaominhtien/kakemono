//! Reads a `.docx` with all the formatting the layout needs: sections,
//! resolved paragraph and run properties, list numbers, tables with their
//! style, headers, footers, notes, pictures, shapes and embedded fonts.

use super::model::{
    Anchor, AnchorAlign, AnchorFrom, AnchorPos, Block, BreakKind, Cell, Columns, EmbeddedFont, FDoc, FImage, FieldKind,
    Inline, NumFormat, NumberMark, PageSetup, Para, Rgb, Row, RunFmt, Section, SectionStart, Shape, Table, TableProps,
    VMerge, Width, Wrap,
};
use super::props::{RawCell, RawPara, RawRun, TWIPS_PER_PT, ThemeFonts, is_on, parse_color, theme_fonts};
use super::styles::{Conditional, Numbering, Styles, TableStyle, parse_num_format};
use crate::word::docx::{Rel, alternate, document_title, field_words, main_part, parse, related_part, rels_of};
use crate::word::omml;
use crate::word::pictures::normalize;
use crate::word::xml::{
    DRAWING_NS, MATH_NS, MC_NS, REL_NS, VML_NS, WORD_NS, WP_NS, attr, child, children, is, toggle, val,
};
use crate::word::zip::ZipArchive;
use anyhow::{Context, Result};
use roxmltree::{Document as Xml, Node};
use std::collections::HashMap;

/// EMU (English Metric Units) per point.
const EMU_PER_PT: f32 = 12_700.0;
/// Namespace of Word shapes (`wps:`).
const WPS_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
/// Namespace of Word groups (`wpg:`).
const WPG_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";
/// Namespace of DrawingML pictures (`pic:`).
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
/// Namespace of the Office VML extensions (`o:`).
const OFFICE_NS: &str = "urn:schemas-microsoft-com:office:office";
/// Default left and right insets of a text box, in points (0.1 inch).
const TEXTBOX_INSET_X_PT: f32 = 7.2;
/// Default top and bottom insets of a text box, in points (0.05 inch).
const TEXTBOX_INSET_Y_PT: f32 = 3.6;
/// Default distance between the default tab stops, in points.
const DEFAULT_TAB_PT: f32 = 36.0;
/// Cell margins of a table without any: 0.08 inch left and right.
const DEFAULT_CELL_MARGIN_X_PT: f32 = 5.4;
/// Length of the key of an obfuscated font, in bytes.
const FONT_KEY_LEN: usize = 16;
/// Bytes of an obfuscated font that are scrambled.
const FONT_OBFUSCATED_LEN: usize = 32;

/// A field being read.
#[derive(Debug, Default)]
struct FieldState {
    instr: String,
    in_result: bool,
    /// Its result is replaced by an [`Inline::Field`].
    skip_result: bool,
    href: Option<String>,
}

/// The table context of the paragraphs being read: what the table style
/// gives them.
#[derive(Debug, Clone, Default)]
struct CellContext {
    para: RawPara,
    run: RawRun,
}

/// The reader.
struct Reader<'a> {
    zip: &'a ZipArchive<'a>,
    styles: &'a Styles,
    theme: &'a ThemeFonts,
    numbering: Numbering,
    images: Vec<FImage>,
    image_index: HashMap<String, Option<usize>>,
    rels: HashMap<String, Rel>,
    fields: Vec<FieldState>,
    parts: Vec<Vec<Block>>,
    part_index: HashMap<String, usize>,
    cell: Option<CellContext>,
    pending_bookmarks: Vec<String>,
}

/// Reads a `.docx` file with its formatting.
///
/// # Errors
/// When `data` is not a Word package, or a needed part is damaged.
pub fn read(data: &[u8]) -> Result<FDoc> {
    let zip = ZipArchive::new(data)?;
    let main = main_part(&zip)?;
    let document_xml = zip
        .read_text(&main)?
        .with_context(|| format!("the package has no {main}"))?;
    let document = parse(&document_xml).with_context(|| format!("reading {main}"))?;
    let theme_xml = zip.read_text(&related_part(&zip, &main, "theme"))?;
    let theme_xml = match theme_xml {
        Some(xml) => Some(xml),
        None => zip.read_text("word/theme/theme1.xml")?,
    };
    let theme_doc = theme_xml.as_deref().map(parse).transpose().ok().flatten();
    let theme = theme_doc
        .as_ref()
        .map(|doc| theme_fonts(doc.root_element()))
        .unwrap_or_default();
    let styles_xml = zip.read_text(&related_part(&zip, &main, "styles"))?;
    let styles_doc = styles_xml.as_deref().map(parse).transpose()?;
    let styles = Styles::parse(styles_doc.as_ref().map(Xml::root_element), &theme);
    let numbering_xml = zip.read_text(&related_part(&zip, &main, "numbering"))?;
    let numbering_doc = numbering_xml.as_deref().map(parse).transpose()?;
    let numbering = Numbering::parse(numbering_doc.as_ref().map(Xml::root_element), &theme);
    let mut doc = FDoc {
        default_tab: DEFAULT_TAB_PT,
        endnote_format: NumFormat::LowerRoman,
        compat_mode: 12,
        ..Default::default()
    };
    read_settings(&zip, &main, &mut doc)?;
    let mut reader = Reader {
        zip: &zip,
        styles: &styles,
        theme: &theme,
        numbering,
        images: Vec::new(),
        image_index: HashMap::new(),
        rels: rels_of(&zip, &main)?,
        fields: Vec::new(),
        parts: Vec::new(),
        part_index: HashMap::new(),
        cell: None,
        pending_bookmarks: Vec::new(),
    };
    let body = child(document.root_element(), WORD_NS, "body").context("the document has no body")?;
    doc.sections = reader.body(body);
    for (kind, element) in [("footnotes", "footnote"), ("endnotes", "endnote")] {
        let part = related_part(&zip, &main, kind);
        let Some(xml) = zip.read_text(&part)? else { continue };
        let Ok(notes_doc) = parse(&xml) else { continue };
        reader.rels = rels_of(&zip, &part)?;
        let mut notes = Vec::new();
        for note in children(notes_doc.root_element(), WORD_NS, element) {
            match attr(note, WORD_NS, "type") {
                Some("separator") if kind == "footnotes" => {
                    doc.footnote_separator = Some(reader.blocks(note));
                    continue;
                }
                Some("separator" | "continuationSeparator" | "continuationNotice") => continue,
                _ => {}
            }
            let Some(id) = attr(note, WORD_NS, "id") else { continue };
            notes.push((id.to_string(), reader.blocks(note)));
        }
        if kind == "footnotes" {
            doc.footnotes = notes;
        } else {
            doc.endnotes = notes;
        }
    }
    doc.fonts = embedded_fonts(&zip, &main);
    doc.font_kinds = font_kinds(&zip, &main);
    doc.title = document_title(&zip)?;
    doc.images = reader.images;
    doc.parts = reader.parts;
    Ok(doc)
}

/// Reads `settings.xml`: default tab stops, header options, note formats,
/// hyphenation and the compatibility mode.
fn read_settings(zip: &ZipArchive<'_>, main: &str, doc: &mut FDoc) -> Result<()> {
    let Some(xml) = zip.read_text(&related_part(zip, main, "settings"))? else {
        return Ok(());
    };
    let Ok(settings) = parse(&xml) else { return Ok(()) };
    let root = settings.root_element();
    if let Some(tab) = val(root, "defaultTabStop").and_then(|tab| tab.parse::<f32>().ok())
        && tab > 0.0
    {
        doc.default_tab = tab / TWIPS_PER_PT;
    }
    doc.even_and_odd_headers = toggle(root, "evenAndOddHeaders").unwrap_or(false);
    doc.auto_hyphenation = toggle(root, "autoHyphenation").unwrap_or(false);
    if let Some(format) = child(root, WORD_NS, "footnotePr").and_then(|props| val(props, "numFmt")) {
        doc.footnote_format = parse_num_format(format);
    }
    if let Some(format) = child(root, WORD_NS, "endnotePr").and_then(|props| val(props, "numFmt")) {
        doc.endnote_format = parse_num_format(format);
    }
    if let Some(compat) = child(root, WORD_NS, "compat") {
        doc.no_expand_shift_return = toggle(compat, "doNotExpandShiftReturn").unwrap_or(false);
        doc.add_paragraph_spacing = toggle(compat, "doNotUseHTMLParagraphAutoSpacing").unwrap_or(false);
        doc.split_break_and_mark = toggle(compat, "splitPgBreakAndParaMark").unwrap_or(false);
        for setting in children(compat, WORD_NS, "compatSetting") {
            if attr(setting, WORD_NS, "name") == Some("compatibilityMode")
                && let Some(mode) = attr(setting, WORD_NS, "val").and_then(|mode| mode.parse().ok())
            {
                doc.compat_mode = mode;
            }
        }
    }
    Ok(())
}

/// The fonts embedded in the package (`fontTable.xml`), de-obfuscated.
fn embedded_fonts(zip: &ZipArchive<'_>, main: &str) -> Vec<EmbeddedFont> {
    let part = related_part(zip, main, "fontTable");
    let Ok(Some(xml)) = zip.read_text(&part) else {
        return Vec::new();
    };
    let Ok(table) = parse(&xml) else { return Vec::new() };
    let Ok(rels) = rels_of(zip, &part) else {
        return Vec::new();
    };
    let mut fonts = Vec::new();
    for font in children(table.root_element(), WORD_NS, "font") {
        let Some(family) = attr(font, WORD_NS, "name") else {
            continue;
        };
        for (element, bold, italic) in [
            ("embedRegular", false, false),
            ("embedBold", true, false),
            ("embedItalic", false, true),
            ("embedBoldItalic", true, true),
        ] {
            let Some(embed) = child(font, WORD_NS, element) else {
                continue;
            };
            let Some(rel) = attr(embed, REL_NS, "id").and_then(|id| rels.get(id)) else {
                continue;
            };
            let Ok(Some(mut data)) = zip.read(&rel.target) else {
                continue;
            };
            if let Some(key) = attr(embed, WORD_NS, "fontKey") {
                deobfuscate(&mut data, key);
            }
            fonts.push(EmbeddedFont {
                family: family.to_string(),
                bold,
                italic,
                data,
            });
        }
    }
    fonts
}

/// The kind of each font of the font table, by lowercase name.
fn font_kinds(zip: &ZipArchive<'_>, main: &str) -> HashMap<String, super::model::FontKind> {
    use super::model::FontKind;
    let part = related_part(zip, main, "fontTable");
    let Ok(Some(xml)) = zip.read_text(&part) else {
        return HashMap::new();
    };
    let Ok(table) = parse(&xml) else { return HashMap::new() };
    let mut kinds = HashMap::new();
    for font in children(table.root_element(), WORD_NS, "font") {
        let Some(name) = attr(font, WORD_NS, "name") else {
            continue;
        };
        let kind = if val(font, "charset") == Some("02") {
            FontKind::Symbol
        } else if val(font, "pitch") == Some("fixed") {
            FontKind::Mono
        } else {
            match val(font, "family") {
                Some("roman") => FontKind::Serif,
                Some("modern") => FontKind::Mono,
                Some("decorative") => FontKind::Symbol,
                _ => FontKind::Sans,
            }
        };
        kinds.insert(name.to_lowercase(), kind);
    }
    kinds
}

/// Undoes the obfuscation of an embedded font: its first 32 bytes are
/// XORed with the key, a GUID read backwards.
fn deobfuscate(data: &mut [u8], key: &str) {
    let hex: String = key.chars().filter(char::is_ascii_hexdigit).collect();
    if hex.len() != FONT_KEY_LEN * 2 {
        return;
    }
    let mut bytes = [0u8; FONT_KEY_LEN];
    for (at, byte) in bytes.iter_mut().enumerate() {
        let start = hex.len() - 2 * (at + 1);
        *byte = u8::from_str_radix(&hex[start..start + 2], 16).unwrap_or(0);
    }
    for (at, byte) in data.iter_mut().take(FONT_OBFUSCATED_LEN).enumerate() {
        *byte ^= bytes[at % FONT_KEY_LEN];
    }
}

/// Twips in the attribute `name` of `node`, in points.
fn twips(node: Node<'_, '_>, name: &str) -> Option<f32> {
    attr(node, WORD_NS, name)
        .and_then(|value| value.parse::<f32>().ok())
        .map(|value| value / TWIPS_PER_PT)
}

/// EMU in the attribute `name` of `node` (no namespace), in points.
fn emu(node: Node<'_, '_>, name: &str) -> Option<f32> {
    node.attribute(name)
        .and_then(|value| value.parse::<f32>().ok())
        .map(|value| value / EMU_PER_PT)
}

impl Reader<'_> {
    /// The sections of the body.
    fn body(&mut self, body: Node<'_, '_>) -> Vec<Section> {
        let mut sections = Vec::new();
        let mut blocks = Vec::new();
        let mut previous = Section::default();
        self.walk(
            body,
            &mut |reader, item, blocks: &mut Vec<Block>| {
                if is(item, WORD_NS, "p")
                    && let Some(props) = child(item, WORD_NS, "pPr")
                    && let Some(sect) = child(props, WORD_NS, "sectPr")
                {
                    blocks.push(Block::Para(Box::new(reader.paragraph(item))));
                    let section = reader.section(sect, &previous, std::mem::take(blocks));
                    previous = section.clone();
                    previous.blocks.clear();
                    sections.push(section);
                    return true;
                }
                false
            },
            &mut blocks,
        );
        let last = child(body, WORD_NS, "sectPr");
        let section = match last {
            Some(sect) => self.section(sect, &previous, blocks),
            None => Section {
                blocks,
                columns: Columns {
                    count: 1,
                    ..Default::default()
                },
                ..previous
            },
        };
        sections.push(section);
        sections
    }

    /// Walks the block-level content of `node` into `blocks`; `hook` may
    /// take an element itself (returning true).
    fn walk(
        &mut self,
        node: Node<'_, '_>,
        hook: &mut dyn FnMut(&mut Self, Node<'_, '_>, &mut Vec<Block>) -> bool,
        blocks: &mut Vec<Block>,
    ) {
        for item in node.children().filter(Node::is_element) {
            if item.tag_name().namespace() != Some(WORD_NS) {
                continue;
            }
            if hook(self, item, blocks) {
                continue;
            }
            match item.tag_name().name() {
                "p" => {
                    let para = self.paragraph(item);
                    blocks.push(Block::Para(Box::new(para)));
                }
                "tbl" => blocks.push(Block::Table(self.table(item))),
                "sdt" => {
                    if let Some(content) = child(item, WORD_NS, "sdtContent") {
                        self.walk(content, hook, blocks);
                    }
                }
                "customXml" | "ins" | "moveTo" => self.walk(item, hook, blocks),
                "bookmarkStart" => {
                    if let Some(name) = attr(item, WORD_NS, "name").filter(|name| *name != "_GoBack") {
                        self.pending_bookmarks.push(name.to_string());
                    }
                }
                _ => {}
            }
        }
    }

    /// The paragraphs and tables of a container (a cell, a note, a header).
    fn blocks(&mut self, node: Node<'_, '_>) -> Vec<Block> {
        let mut blocks = Vec::new();
        self.walk(node, &mut |_, _, _| false, &mut blocks);
        blocks
    }

    /// A section from its `w:sectPr`, inheriting the headers and footers of
    /// the previous section where it sets none.
    fn section(&mut self, sect: Node<'_, '_>, previous: &Section, blocks: Vec<Block>) -> Section {
        let page = page_setup(sect);
        let columns = section_columns(sect);
        let start = match val(sect, "type") {
            Some("continuous") => SectionStart::Continuous,
            Some("nextColumn") => SectionStart::NextColumn,
            Some("oddPage") => SectionStart::OddPage,
            Some("evenPage") => SectionStart::EvenPage,
            _ => SectionStart::NextPage,
        };
        let mut headers = previous.headers;
        let mut footers = previous.footers;
        for (element, set) in [("headerReference", &mut headers), ("footerReference", &mut footers)] {
            for reference in children(sect, WORD_NS, element) {
                let Some(id) = attr(reference, REL_NS, "id") else {
                    continue;
                };
                let index = self.part(id);
                match attr(reference, WORD_NS, "type").unwrap_or("default") {
                    "first" => set.first = index,
                    "even" => set.even = index,
                    _ => set.default = index,
                }
            }
        }
        let numbering = child(sect, WORD_NS, "pgNumType");
        let grid = child(sect, WORD_NS, "docGrid");
        let grid_pitch = grid
            .filter(|grid| {
                matches!(
                    attr(*grid, WORD_NS, "type"),
                    Some("lines" | "linesAndChars" | "snapToChars")
                )
            })
            .and_then(|grid| twips(grid, "linePitch"));
        Section {
            page,
            columns,
            start,
            headers,
            footers,
            title_page: toggle(sect, "titlePg").unwrap_or(false),
            page_number_start: numbering
                .and_then(|numbering| attr(numbering, WORD_NS, "start"))
                .and_then(|start| start.parse().ok()),
            page_number_format: numbering
                .and_then(|numbering| attr(numbering, WORD_NS, "fmt"))
                .map_or(NumFormat::Decimal, parse_num_format),
            grid_pitch,
            blocks,
        }
    }

    /// The index of the header or footer part of relationship `id`, read
    /// once.
    fn part(&mut self, id: &str) -> Option<usize> {
        let rel = self.rels.get(id).filter(|rel| !rel.external)?.clone();
        if let Some(&index) = self.part_index.get(&rel.target) {
            return Some(index);
        }
        let xml = self.zip.read_text(&rel.target).ok()??;
        let doc = parse(&xml).ok()?;
        let saved_rels = std::mem::replace(&mut self.rels, rels_of(self.zip, &rel.target).unwrap_or_default());
        let saved_fields = std::mem::take(&mut self.fields);
        let blocks = self.blocks(doc.root_element());
        self.rels = saved_rels;
        self.fields = saved_fields;
        self.parts.push(blocks);
        let index = self.parts.len() - 1;
        self.part_index.insert(rel.target, index);
        Some(index)
    }

    /// The paragraph and text properties a paragraph of style `style_id`
    /// starts from, with the table style of its cell.
    fn base(&self, style_id: Option<&str>) -> (RawPara, RawRun) {
        let (style_para, style_run) = self.styles.paragraph(style_id);
        let mut para = self.styles.default_para.clone();
        let mut run = self.styles.default_run.clone();
        let is_default = self.styles.para_style_id(style_id) == self.styles.para_style_id(None);
        match &self.cell {
            Some(cell) if is_default => {
                // In a table, the table style wins over the default
                // paragraph style, but not over the text properties that
                // style changes from the document's defaults.
                para.overlay(&style_para);
                para.overlay(&cell.para);
                let defaults = run.clone();
                run.overlay(&style_run);
                run.overlay_unchanged(&cell.run, &style_run, &defaults);
            }
            Some(cell) => {
                para.overlay(&cell.para);
                run.overlay(&cell.run);
                para.overlay(&style_para);
                run.overlay(&style_run);
            }
            None => {
                para.overlay(&style_para);
                run.overlay(&style_run);
            }
        }
        (para, run)
    }

    /// A paragraph with its resolved properties and content.
    fn paragraph(&mut self, node: Node<'_, '_>) -> Para {
        let props = child(node, WORD_NS, "pPr");
        let style_id = props.and_then(|props| val(props, "pStyle"));
        let (mut para_raw, base_run) = self.base(style_id);
        let direct = props.map(RawPara::parse).unwrap_or_default();
        let mut mark_raw = base_run.clone();
        if let Some(mark) = props.and_then(|props| child(props, WORD_NS, "rPr")) {
            mark_raw.overlay(&RawRun::parse(mark, self.theme));
        }
        // The list: from the paragraph, else its style.
        let mut numbering = para_raw.numbering.clone();
        if let Some((id, level)) = &direct.numbering {
            let (old_id, old_level) = numbering.take().unwrap_or_default();
            let id = if id.is_empty() { old_id } else { id.clone() };
            numbering = Some((id, level.or(old_level)));
        }
        let from_style = direct.numbering.is_none();
        let mut number = None;
        if let Some((num_id, level)) = numbering.filter(|(id, _)| !id.is_empty() && id != "0") {
            let level = level.unwrap_or(0);
            if let Some(def) = self.numbering.level(&num_id, level, self.styles) {
                // A list set by the paragraph's style is beneath the style:
                // the style's own indents win over the level's.
                if from_style {
                    let mut under = def.para.clone();
                    // The style's tab stops replace the level's.
                    under.tabs.clear();
                    under.overlay(&para_raw);
                    para_raw = under;
                } else {
                    para_raw.overlay(&def.para);
                }
                let hidden = mark_raw.hidden == Some(true);
                if !hidden && let Some((def, text)) = self.numbering.next(&num_id, level, self.styles) {
                    let mut fmt_raw = mark_raw.clone();
                    // Underline and highlight of the mark do not reach the
                    // number.
                    fmt_raw.underline = None;
                    fmt_raw.overlay(&def.run);
                    let mut fmt = fmt_raw.resolve();
                    let text = if def.format == NumFormat::Bullet {
                        symbol_text(&text, &mut fmt)
                    } else {
                        text
                    };
                    if def.format != NumFormat::None || !text.is_empty() {
                        number = Some(NumberMark {
                            text,
                            fmt,
                            suffix: def.suffix,
                            align: def.align,
                        });
                    }
                }
            }
        }
        para_raw.overlay(&direct);
        let mut para = Para {
            style: self.styles.para_style_id(style_id),
            props: para_raw.resolve(),
            mark: mark_raw.resolve(),
            number,
            inlines: Vec::new(),
            bookmarks: std::mem::take(&mut self.pending_bookmarks),
        };
        self.fields.clear();
        self.content(node, &base_run, None, &mut para);
        para
    }

    /// Reads the content of `node` (a paragraph, a hyperlink, a field...)
    /// into `para`.
    fn content(&mut self, node: Node<'_, '_>, base: &RawRun, href: Option<&str>, para: &mut Para) {
        for item in node.children().filter(Node::is_element) {
            let ns = item.tag_name().namespace();
            if ns == Some(MATH_NS) {
                let display = item.tag_name().name() == "oMathPara";
                if display || item.tag_name().name() == "oMath" {
                    let fmt = base.resolve();
                    para.inlines.push(Inline::Math {
                        node: omml::formula(item),
                        display,
                        fmt,
                    });
                }
                continue;
            }
            if ns == Some(MC_NS) && item.tag_name().name() == "AlternateContent" {
                if let Some(choice) = alternate(item) {
                    self.content(choice, base, href, para);
                }
                continue;
            }
            if ns != Some(WORD_NS) {
                continue;
            }
            match item.tag_name().name() {
                "r" => self.run(item, base, href, para),
                "hyperlink" => {
                    let link = self.hyperlink(item);
                    self.content(item, base, link.as_deref().or(href), para);
                }
                "fldSimple" => {
                    let instr = attr(item, WORD_NS, "instr").unwrap_or_default();
                    if let Some((kind, format)) = layout_field(instr) {
                        let fmt = self.first_run_fmt(item, base);
                        para.inlines.push(Inline::Field { kind, fmt, format });
                    } else {
                        let link = crate::word::docx::field_link(instr);
                        self.content(item, base, link.as_deref().or(href), para);
                    }
                }
                "bookmarkStart" => {
                    if let Some(name) = attr(item, WORD_NS, "name").filter(|name| *name != "_GoBack") {
                        para.bookmarks.push(name.to_string());
                    }
                }
                "ins" | "moveTo" | "smartTag" | "customXml" | "dir" | "bdo" => self.content(item, base, href, para),
                "sdt" => {
                    if let Some(content) = child(item, WORD_NS, "sdtContent") {
                        self.content(content, base, href, para);
                    }
                }
                _ => {}
            }
        }
    }

    /// The properties of the first run in `node`, for a field shown in
    /// place of it.
    fn first_run_fmt(&self, node: Node<'_, '_>, base: &RawRun) -> RunFmt {
        let run = node.descendants().find(|item| is(*item, WORD_NS, "r"));
        match run {
            Some(run) => self.run_fmt(run, base, None),
            None => base.resolve(),
        }
    }

    /// The resolved properties of a run.
    fn run_fmt(&self, node: Node<'_, '_>, base: &RawRun, href: Option<&str>) -> RunFmt {
        let props = child(node, WORD_NS, "rPr");
        let mut raw = base.clone();
        if let Some(style) = props.and_then(|props| val(props, "rStyle")) {
            raw.overlay(&self.styles.character(style));
        }
        if let Some(props) = props {
            raw.overlay(&RawRun::parse(props, self.theme));
        }
        let mut fmt = raw.resolve();
        let field_href = self.fields.iter().rev().find_map(|field| field.href.clone());
        fmt.href = field_href.or_else(|| href.map(str::to_string));
        fmt
    }

    /// The target of a `w:hyperlink`.
    fn hyperlink(&self, node: Node<'_, '_>) -> Option<String> {
        if let Some(anchor) = attr(node, WORD_NS, "anchor") {
            return Some(format!("#{anchor}"));
        }
        let id = attr(node, REL_NS, "id")?;
        self.rels.get(id).map(|rel| rel.target.clone())
    }

    /// Whether text read now is hidden by a field: its instruction, or a
    /// result replaced by a layout field.
    fn hidden_by_field(&self) -> bool {
        self.fields.iter().any(|field| !field.in_result || field.skip_result)
    }

    /// Reads a run.
    fn run(&mut self, node: Node<'_, '_>, base: &RawRun, href: Option<&str>, para: &mut Para) {
        let mut fmt = self.run_fmt(node, base, href);
        let mut text = String::new();
        for item in node.children().filter(Node::is_element) {
            let ns = item.tag_name().namespace();
            if ns == Some(MC_NS) && item.tag_name().name() == "AlternateContent" {
                if let Some(choice) = alternate(item)
                    && !self.hidden_by_field()
                {
                    flush(&mut text, &fmt, para);
                    self.objects(choice, &fmt, para);
                }
                continue;
            }
            if ns != Some(WORD_NS) {
                continue;
            }
            match item.tag_name().name() {
                "fldChar" => {
                    flush(&mut text, &fmt, para);
                    self.field_char(item, &fmt, para);
                    fmt = self.run_fmt(node, base, href);
                }
                "instrText" => {
                    if let Some(field) = self.fields.last_mut()
                        && !field.in_result
                    {
                        field.instr.push_str(item.text().unwrap_or_default());
                    }
                }
                _ if self.hidden_by_field() => {}
                "t" => text.push_str(item.text().unwrap_or_default()),
                "tab" | "ptab" => {
                    flush(&mut text, &fmt, para);
                    para.inlines.push(Inline::Tab { fmt: fmt.clone() });
                }
                "br" | "cr" => {
                    flush(&mut text, &fmt, para);
                    let kind = break_kind(item);
                    para.inlines.push(Inline::Break { kind, fmt: fmt.clone() });
                }
                "noBreakHyphen" => text.push('\u{2011}'),
                "softHyphen" => text.push('\u{00AD}'),
                "sym" => {
                    flush(&mut text, &fmt, para);
                    para.inlines.extend(symbol(item, &fmt));
                }
                "pgNum" => {
                    flush(&mut text, &fmt, para);
                    para.inlines.push(Inline::Field {
                        kind: FieldKind::Page,
                        fmt: fmt.clone(),
                        format: None,
                    });
                }
                "footnoteReference" | "endnoteReference" => {
                    flush(&mut text, &fmt, para);
                    if let Some(id) = attr(item, WORD_NS, "id") {
                        para.inlines.push(Inline::NoteRef {
                            endnote: item.tag_name().name() == "endnoteReference",
                            id: id.to_string(),
                            fmt: fmt.clone(),
                        });
                    }
                }
                "footnoteRef" | "endnoteRef" => {
                    flush(&mut text, &fmt, para);
                    para.inlines.push(Inline::NoteNumber { fmt: fmt.clone() });
                }
                "drawing" | "pict" | "object" => {
                    flush(&mut text, &fmt, para);
                    self.objects(item, &fmt, para);
                }
                _ => {}
            }
        }
        flush(&mut text, &fmt, para);
    }

    /// Handles a field character.
    fn field_char(&mut self, node: Node<'_, '_>, fmt: &RunFmt, para: &mut Para) {
        match attr(node, WORD_NS, "fldCharType") {
            Some("begin") => self.fields.push(FieldState::default()),
            Some("separate") => {
                let hidden_outside = self
                    .fields
                    .iter()
                    .rev()
                    .skip(1)
                    .any(|field| !field.in_result || field.skip_result);
                if let Some(field) = self.fields.last_mut() {
                    field.in_result = true;
                    if let Some((kind, format)) = layout_field(&field.instr) {
                        field.skip_result = true;
                        if !hidden_outside {
                            para.inlines.push(Inline::Field {
                                kind,
                                fmt: fmt.clone(),
                                format,
                            });
                        }
                    } else {
                        field.href = crate::word::docx::field_link(&field.instr);
                    }
                }
            }
            Some("end") => {
                if let Some(field) = self.fields.pop()
                    && !field.in_result
                    && let Some((kind, format)) = layout_field(&field.instr)
                    && !self.hidden_by_field()
                {
                    // A field without a result: it is shown all the same.
                    para.inlines.push(Inline::Field {
                        kind,
                        fmt: fmt.clone(),
                        format,
                    });
                }
            }
            _ => {}
        }
    }

    /// Reads the pictures and shapes inside a drawing, a VML picture or an
    /// embedded object.
    fn objects(&mut self, node: Node<'_, '_>, fmt: &RunFmt, para: &mut Para) {
        for drawing in node
            .descendants()
            .filter(|item| is(*item, WP_NS, "inline") || is(*item, WP_NS, "anchor"))
        {
            // Drawings inside the text box of another one are read with it.
            if drawing
                .ancestors()
                .skip(1)
                .any(|ancestor| is(ancestor, WORD_NS, "txbxContent"))
            {
                continue;
            }
            if let Some(inline) = self.drawing(drawing, fmt) {
                para.inlines.push(inline);
            }
        }
        if node
            .descendants()
            .any(|item| is(item, WP_NS, "inline") || is(item, WP_NS, "anchor"))
        {
            return;
        }
        // VML.
        for shape in node.children().filter(|item| {
            item.tag_name().namespace() == Some(VML_NS)
                && matches!(
                    item.tag_name().name(),
                    "shape" | "rect" | "roundrect" | "oval" | "group" | "line"
                )
        }) {
            if let Some(inline) = self.vml_shape(shape, fmt) {
                para.inlines.push(inline);
            }
        }
    }

    /// A DrawingML drawing (`wp:inline` or `wp:anchor`).
    fn drawing(&mut self, node: Node<'_, '_>, fmt: &RunFmt) -> Option<Inline> {
        let extent = child(node, WP_NS, "extent");
        let width = extent.and_then(|extent| emu(extent, "cx")).unwrap_or(0.0);
        let height = extent.and_then(|extent| emu(extent, "cy")).unwrap_or(0.0);
        let anchor = is(node, WP_NS, "anchor").then(|| drawing_anchor(node));
        let alt = child(node, WP_NS, "docPr")
            .and_then(|doc_pr| doc_pr.attribute("descr").or_else(|| doc_pr.attribute("title")))
            .unwrap_or_default()
            .to_string();
        let data = node.descendants().find(|item| is(*item, DRAWING_NS, "graphicData"))?;
        if let Some(pic) = child(data, PIC_NS, "pic") {
            let (index, crop) = self.blip(pic);
            return Some(Inline::Image {
                index,
                width,
                height,
                anchor,
                fmt: fmt.clone(),
                alt,
                crop,
            });
        }
        if let Some(shape) = child(data, WPS_NS, "wsp") {
            return Some(Inline::Shape(Box::new(
                self.wps_shape(shape, width, height, anchor, fmt),
            )));
        }
        if let Some(group) = child(data, WPG_NS, "wgp") {
            let mut shape = Shape {
                width,
                height,
                anchor,
                fill: None,
                line: None,
                blocks: Vec::new(),
                insets: [0.0; 4],
                pictures: Vec::new(),
                auto_height: false,
                fmt: fmt.clone(),
            };
            self.group(group, width, height, &mut shape);
            return Some(Inline::Shape(Box::new(shape)));
        }
        // A chart or diagram: its place is kept.
        Some(Inline::Image {
            index: None,
            width,
            height,
            anchor,
            fmt: fmt.clone(),
            alt,
            crop: [0.0; 4],
        })
    }

    /// The picture of a `pic:pic` and its cropping.
    fn blip(&mut self, pic: Node<'_, '_>) -> (Option<usize>, [f32; 4]) {
        let Some(fill) = pic.descendants().find(|item| is(*item, PIC_NS, "blipFill")) else {
            return (None, [0.0; 4]);
        };
        let index = child(fill, DRAWING_NS, "blip")
            .and_then(|blip| attr(blip, REL_NS, "embed"))
            .and_then(|id| self.image(id));
        let crop = child(fill, DRAWING_NS, "srcRect").map_or([0.0; 4], |rect| {
            let share = |name: &str| {
                rect.attribute(name)
                    .and_then(|value| value.parse::<f32>().ok())
                    .map_or(0.0, |value| value / 100_000.0)
            };
            [share("l"), share("t"), share("r"), share("b")]
        });
        (index, crop)
    }

    /// A Word shape (`wps:wsp`): its fill, outline and text box.
    fn wps_shape(
        &mut self,
        node: Node<'_, '_>,
        width: f32,
        height: f32,
        anchor: Option<Anchor>,
        fmt: &RunFmt,
    ) -> Shape {
        let props = child(node, WPS_NS, "spPr");
        let fill = props.and_then(solid_fill);
        let line = props
            .and_then(|props| child(props, DRAWING_NS, "ln"))
            .and_then(line_border);
        let body = child(node, WPS_NS, "bodyPr");
        let inset = |name: &str, default: f32| body.and_then(|body| emu(body, name)).unwrap_or(default);
        let insets = [
            inset("lIns", TEXTBOX_INSET_X_PT),
            inset("tIns", TEXTBOX_INSET_Y_PT),
            inset("rIns", TEXTBOX_INSET_X_PT),
            inset("bIns", TEXTBOX_INSET_Y_PT),
        ];
        let auto_height = body.is_some_and(|body| child(body, DRAWING_NS, "spAutoFit").is_some());
        let blocks = match node.descendants().find(|item| is(*item, WORD_NS, "txbxContent")) {
            Some(content) => self.text_box(content),
            None => Vec::new(),
        };
        Shape {
            width,
            height,
            anchor,
            fill,
            line,
            blocks,
            insets,
            pictures: Vec::new(),
            auto_height,
            fmt: fmt.clone(),
        }
    }

    /// The content of a text box, read apart from the paragraph it is in.
    fn text_box(&mut self, content: Node<'_, '_>) -> Vec<Block> {
        let fields = std::mem::take(&mut self.fields);
        let bookmarks = std::mem::take(&mut self.pending_bookmarks);
        let cell = self.cell.take();
        let blocks = self.blocks(content);
        self.fields = fields;
        self.pending_bookmarks = bookmarks;
        self.cell = cell;
        blocks
    }

    /// The pictures of a group (`wpg:wgp`), placed in the shape of size
    /// `width` x `height`.
    fn group(&mut self, group: Node<'_, '_>, width: f32, height: f32, shape: &mut Shape) {
        let xfrm = child(group, WPG_NS, "grpSpPr").and_then(|props| child(props, DRAWING_NS, "xfrm"));
        let pair = |name: &str, x: &str, y: &str| {
            xfrm.and_then(|xfrm| child(xfrm, DRAWING_NS, name))
                .map(|node| (emu(node, x).unwrap_or(0.0), emu(node, y).unwrap_or(0.0)))
        };
        let child_off = pair("chOff", "x", "y").unwrap_or((0.0, 0.0));
        let child_ext = pair("chExt", "cx", "cy").unwrap_or((width, height));
        let sx = if child_ext.0 > 0.0 { width / child_ext.0 } else { 1.0 };
        let sy = if child_ext.1 > 0.0 { height / child_ext.1 } else { 1.0 };
        for pic in group.descendants().filter(|item| is(*item, PIC_NS, "pic")) {
            let xfrm = child(pic, PIC_NS, "spPr").and_then(|props| child(props, DRAWING_NS, "xfrm"));
            let Some(xfrm) = xfrm else { continue };
            let off = child(xfrm, DRAWING_NS, "off");
            let ext = child(xfrm, DRAWING_NS, "ext");
            let (Some(off), Some(ext)) = (off, ext) else { continue };
            let (index, _) = self.blip(pic);
            let Some(index) = index else { continue };
            shape.pictures.push((
                index,
                (emu(off, "x").unwrap_or(0.0) - child_off.0) * sx,
                (emu(off, "y").unwrap_or(0.0) - child_off.1) * sy,
                emu(ext, "cx").unwrap_or(0.0) * sx,
                emu(ext, "cy").unwrap_or(0.0) * sy,
            ));
        }
        if let Some(content) = group.descendants().find(|item| is(*item, WORD_NS, "txbxContent")) {
            shape.blocks = self.text_box(content);
        }
    }

    /// A VML shape: a picture or a text box, in the line or floating.
    fn vml_shape(&mut self, shape: Node<'_, '_>, fmt: &RunFmt) -> Option<Inline> {
        let style = shape.attribute("style").unwrap_or_default();
        let css = vml_style(style);
        let length = |name: &str| css.get(name).and_then(|value| vml_length(value));
        let width = length("width").unwrap_or(0.0);
        let height = length("height").unwrap_or(0.0);
        let anchor = (css.get("position").map(String::as_str) == Some("absolute")).then(|| vml_anchor(shape, &css));
        if let Some(data) = shape.descendants().find(|item| is(*item, VML_NS, "imagedata")) {
            let index = attr(data, REL_NS, "id").and_then(|id| self.image(id));
            let alt = data.attribute((OFFICE_NS, "title")).unwrap_or_default().to_string();
            return Some(Inline::Image {
                index,
                width,
                height,
                anchor,
                fmt: fmt.clone(),
                alt,
                crop: [0.0; 4],
            });
        }
        let content = shape.descendants().find(|item| is(*item, WORD_NS, "txbxContent"));
        let fill = shape
            .attribute("fillcolor")
            .and_then(vml_color)
            .filter(|_| shape.attribute("filled") != Some("f"));
        let line = (shape.attribute("stroked") != Some("f") && shape.tag_name().name() != "shape"
            || shape.attribute("strokecolor").is_some())
        .then(|| super::model::Border {
            width: shape.attribute("strokeweight").and_then(vml_length).unwrap_or(0.75),
            space: 0.0,
            color: shape.attribute("strokecolor").and_then(vml_color).unwrap_or([0, 0, 0]),
            style: super::model::BorderStyle::Single,
        });
        if content.is_none() && fill.is_none() && line.is_none() {
            return None;
        }
        let blocks = match content {
            Some(content) => self.text_box(content),
            None => Vec::new(),
        };
        Some(Inline::Shape(Box::new(Shape {
            width,
            height,
            anchor,
            fill,
            line,
            blocks,
            insets: [
                TEXTBOX_INSET_X_PT,
                TEXTBOX_INSET_Y_PT,
                TEXTBOX_INSET_X_PT,
                TEXTBOX_INSET_Y_PT,
            ],
            pictures: Vec::new(),
            auto_height: css.contains_key("mso-fit-shape-to-text"),
            fmt: fmt.clone(),
        })))
    }

    /// The index of the picture of relationship `id`, read once.
    fn image(&mut self, id: &str) -> Option<usize> {
        let rel = self.rels.get(id).filter(|rel| !rel.external)?.clone();
        if let Some(index) = self.image_index.get(&rel.target) {
            return *index;
        }
        let index = self
            .zip
            .read(&rel.target)
            .ok()
            .flatten()
            .and_then(normalize)
            .map(|picture| {
                self.images.push(FImage {
                    data: picture.data,
                    jpeg: picture.jpeg,
                });
                self.images.len() - 1
            });
        self.image_index.insert(rel.target, index);
        index
    }

    /// A table, with its style resolved into its cells.
    fn table(&mut self, node: Node<'_, '_>) -> Table {
        let props = child(node, WORD_NS, "tblPr");
        let style_id = props.and_then(|props| val(props, "tblStyle"));
        let style = self.styles.table(style_id);
        let mut raw = style.table.clone();
        if let Some(props) = props {
            raw.overlay(&super::props::RawTable::parse(props));
        }
        let look = props.and_then(|props| child(props, WORD_NS, "tblLook"));
        let look = TableLook::parse(look);
        let grid: Vec<f32> = child(node, WORD_NS, "tblGrid")
            .map(|grid| {
                children(grid, WORD_NS, "gridCol")
                    .map(|col| twips(col, "w").unwrap_or(0.0))
                    .collect()
            })
            .unwrap_or_default();
        let cell_margins = [
            raw.cell_margins[0].unwrap_or(DEFAULT_CELL_MARGIN_X_PT),
            raw.cell_margins[1].unwrap_or(0.0),
            raw.cell_margins[2].unwrap_or(DEFAULT_CELL_MARGIN_X_PT),
            raw.cell_margins[3].unwrap_or(0.0),
        ];
        let borders = raw.borders.map(Option::flatten);
        let float = props
            .and_then(|props| child(props, WORD_NS, "tblpPr"))
            .map(table_anchor);
        let mut table = Table {
            props: TableProps {
                width: raw.width.unwrap_or_default(),
                indent: raw.indent.unwrap_or(0.0),
                align: raw.align.unwrap_or_default(),
                fixed: raw.fixed.unwrap_or(false),
                cell_margins,
                borders,
                cell_spacing: raw.cell_spacing.unwrap_or(0.0),
                float,
            },
            grid,
            rows: Vec::new(),
        };
        let rows: Vec<Node<'_, '_>> = table_rows(node);
        let row_count = rows.len();
        let saved_cell = self.cell.take();
        for (row_at, row) in rows.into_iter().enumerate() {
            table
                .rows
                .push(self.row(row, row_at, row_count, &style, &look, &raw.borders));
        }
        self.cell = saved_cell;
        table
    }

    /// A row of a table.
    fn row(
        &mut self,
        row: Node<'_, '_>,
        row_at: usize,
        row_count: usize,
        style: &TableStyle,
        look: &TableLook,
        table_borders: &[Option<Option<super::model::Border>>; 6],
    ) -> Row {
        let props = child(row, WORD_NS, "trPr");
        let height = props
            .and_then(|props| child(props, WORD_NS, "trHeight"))
            .and_then(|height| {
                let value = twips(height, "val")?;
                match attr(height, WORD_NS, "hRule") {
                    Some("exact") => Some((value, true)),
                    Some("auto") => None,
                    _ => Some((value, false)),
                }
            });
        let count = |name: &str| {
            props
                .and_then(|props| val(props, name))
                .and_then(|value| value.parse().ok())
                .unwrap_or(0usize)
        };
        let mut out = Row {
            height,
            header: props.and_then(|props| toggle(props, "tblHeader")).unwrap_or(false),
            cant_split: props.and_then(|props| toggle(props, "cantSplit")).unwrap_or(false),
            grid_before: count("gridBefore"),
            grid_after: count("gridAfter"),
            cells: Vec::new(),
        };
        let cells: Vec<Node<'_, '_>> = row_cells(row);
        let cell_count = cells.len();
        for (col_at, cell) in cells.into_iter().enumerate() {
            let conditions = cell_conditions(look, row_at, row_count, col_at, cell_count);
            let (context, mut cell_raw, cond_borders) = conditional_formats(style, conditions);
            let props = child(cell, WORD_NS, "tcPr");
            let direct = props.map(RawCell::parse).unwrap_or_default();
            cell_raw.overlay(&direct);
            let span = props
                .and_then(|props| val(props, "gridSpan"))
                .and_then(|span| span.parse().ok())
                .unwrap_or(1usize)
                .max(1);
            let vmerge = match props.and_then(|props| child(props, WORD_NS, "vMerge")) {
                Some(merge) if attr(merge, WORD_NS, "val") == Some("restart") => VMerge::Restart,
                Some(_) => VMerge::Continue,
                None => VMerge::None,
            };
            // A border of the cell: its own, else the table's (outer edges)
            // or inside lines.
            let first = col_at == 0;
            let last = col_at + 1 == cell_count;
            let top_edge = row_at == 0;
            let bottom_edge = row_at + 1 == row_count;
            let pick = |own: Option<Option<super::model::Border>>, outer: usize, inner: usize, is_edge: bool| {
                own.unwrap_or_else(|| table_borders[if is_edge { outer } else { inner }].flatten())
            };
            let own = |at: usize| direct.borders[at].or(cond_borders[at]).or(style.cell.borders[at]);
            let borders = [
                pick(own(0), 0, 4, top_edge),
                pick(own(1), 1, 5, first),
                pick(own(2), 2, 4, bottom_edge),
                pick(own(3), 3, 5, last),
            ];
            self.cell = Some(context);
            let blocks = self.blocks(cell);
            out.cells.push(Cell {
                span,
                vmerge,
                width: cell_raw.width.unwrap_or(Width::Auto),
                borders,
                shading: cell_raw.shading.flatten(),
                margins: [
                    cell_raw.margins[0].unwrap_or(f32::NAN),
                    cell_raw.margins[1].unwrap_or(f32::NAN),
                    cell_raw.margins[2].unwrap_or(f32::NAN),
                    cell_raw.margins[3].unwrap_or(f32::NAN),
                ],
                valign: cell_raw.valign.unwrap_or_default(),
                blocks,
            });
        }
        out
    }
}

/// Which conditional parts of its style a table uses (`w:tblLook`).
#[derive(Debug, Clone, Copy)]
struct TableLook {
    first_row: bool,
    last_row: bool,
    first_col: bool,
    last_col: bool,
    band_rows: bool,
    band_cols: bool,
}

impl TableLook {
    /// Reads a `w:tblLook` (attributes, or the older bit mask).
    fn parse(node: Option<Node<'_, '_>>) -> Self {
        let mut look = TableLook {
            first_row: true,
            last_row: false,
            first_col: true,
            last_col: false,
            band_rows: true,
            band_cols: false,
        };
        let Some(node) = node else { return look };
        if let Some(mask) = attr(node, WORD_NS, "val").and_then(|mask| u32::from_str_radix(mask, 16).ok()) {
            look.first_row = mask & 0x0020 != 0;
            look.last_row = mask & 0x0040 != 0;
            look.first_col = mask & 0x0080 != 0;
            look.last_col = mask & 0x0100 != 0;
            look.band_rows = mask & 0x0200 == 0;
            look.band_cols = mask & 0x0400 == 0;
        }
        let flag = |name: &str| attr(node, WORD_NS, name).map(is_on);
        look.first_row = flag("firstRow").unwrap_or(look.first_row);
        look.last_row = flag("lastRow").unwrap_or(look.last_row);
        look.first_col = flag("firstColumn").unwrap_or(look.first_col);
        look.last_col = flag("lastColumn").unwrap_or(look.last_col);
        look.band_rows = flag("noHBand").map_or(look.band_rows, |no| !no);
        look.band_cols = flag("noVBand").map_or(look.band_cols, |no| !no);
        look
    }
}

/// The page size and margins of a section's `w:sectPr`.
fn page_setup(sect: Node<'_, '_>) -> PageSetup {
    let mut page = PageSetup::default();
    if let Some(size) = child(sect, WORD_NS, "pgSz") {
        page.width = twips(size, "w").unwrap_or(page.width);
        page.height = twips(size, "h").unwrap_or(page.height);
    }
    if let Some(margins) = child(sect, WORD_NS, "pgMar") {
        page.top = twips(margins, "top").unwrap_or(page.top);
        page.bottom = twips(margins, "bottom").unwrap_or(page.bottom);
        page.left = twips(margins, "left").unwrap_or(page.left);
        page.right = twips(margins, "right").unwrap_or(page.right);
        page.header = twips(margins, "header").unwrap_or(page.header);
        page.footer = twips(margins, "footer").unwrap_or(page.footer);
        page.gutter = twips(margins, "gutter").unwrap_or(0.0);
    }
    page
}

/// The columns of a section's `w:sectPr`: their count and spacing, or
/// their own widths when they are not equal.
fn section_columns(sect: Node<'_, '_>) -> Columns {
    let mut columns = Columns {
        count: 1,
        space: 36.0,
        ..Default::default()
    };
    if let Some(cols) = child(sect, WORD_NS, "cols") {
        columns.count = attr(cols, WORD_NS, "num")
            .and_then(|num| num.parse().ok())
            .unwrap_or(1u32)
            .max(1);
        columns.space = twips(cols, "space").unwrap_or(columns.space);
        columns.separator = attr(cols, WORD_NS, "sep").is_some_and(is_on);
        let equal = attr(cols, WORD_NS, "equalWidth").is_none_or(is_on);
        if !equal {
            columns.widths = children(cols, WORD_NS, "col")
                .map(|col| (twips(col, "w").unwrap_or(0.0), twips(col, "space").unwrap_or(0.0)))
                .collect();
            if !columns.widths.is_empty() {
                columns.count = columns.widths.len() as u32;
            }
        }
    }
    columns
}

/// The kind of a break (`w:br`, `w:cr`): page, column or line.
fn break_kind(item: Node<'_, '_>) -> BreakKind {
    match attr(item, WORD_NS, "type") {
        Some("page") => BreakKind::Page,
        Some("column") => BreakKind::Column,
        _ => BreakKind::Line,
    }
}

/// The text of a symbol (`w:sym`) in a run of format `fmt`, in its own
/// font; `None` when its character code cannot be read.
fn symbol(item: Node<'_, '_>, fmt: &RunFmt) -> Option<Inline> {
    let font = attr(item, WORD_NS, "font").unwrap_or_default();
    let code = attr(item, WORD_NS, "char").and_then(|code| u32::from_str_radix(code, 16).ok())?;
    let mut sym_fmt = fmt.clone();
    sym_fmt.font = font.to_string();
    sym_fmt.font_hansi = font.to_string();
    let raw: String = char::from_u32(code).into_iter().collect();
    let text = symbol_text(&raw, &mut sym_fmt);
    Some(Inline::Text { text, fmt: sym_fmt })
}

/// The conditional formats of a table style that apply to the cell at
/// (`row_at`, `col_at`) of a table of `row_count` rows with `cell_count`
/// cells in the row, from the table's look, in the order they apply.
fn cell_conditions(
    look: &TableLook,
    row_at: usize,
    row_count: usize,
    col_at: usize,
    cell_count: usize,
) -> Vec<&'static str> {
    let mut conditions = vec!["wholeTable"];
    if look.band_rows {
        let band = row_at.saturating_sub(usize::from(look.first_row));
        conditions.push(if band % 2 == 0 { "band1Horz" } else { "band2Horz" });
    }
    if look.band_cols {
        let band = col_at.saturating_sub(usize::from(look.first_col));
        conditions.push(if band % 2 == 0 { "band1Vert" } else { "band2Vert" });
    }
    let first_row = look.first_row && row_at == 0;
    let last_row = look.last_row && row_at + 1 == row_count;
    let first_col = look.first_col && col_at == 0;
    let last_col = look.last_col && col_at + 1 == cell_count;
    if first_col {
        conditions.push("firstCol");
    }
    if last_col {
        conditions.push("lastCol");
    }
    if first_row {
        conditions.push("firstRow");
    }
    if last_row {
        conditions.push("lastRow");
    }
    if first_row && first_col {
        conditions.push("nwCell");
    }
    if first_row && last_col {
        conditions.push("neCell");
    }
    if last_row && first_col {
        conditions.push("swCell");
    }
    if last_row && last_col {
        conditions.push("seCell");
    }
    conditions
}

/// The table style's text, paragraph and cell properties and borders for a
/// cell, with the formats of `conditions` laid over the whole table's in
/// order.
fn conditional_formats(
    style: &TableStyle,
    conditions: Vec<&str>,
) -> (CellContext, RawCell, [Option<Option<super::model::Border>>; 6]) {
    let mut context = CellContext {
        para: style.para.clone(),
        run: style.run.clone(),
    };
    let mut cell_raw = style.cell.clone();
    let mut cond_borders: [Option<Option<super::model::Border>>; 6] = Default::default();
    for name in conditions {
        if let Some(Conditional { run, para, cell }) = style.conditions.get(name) {
            context.run.overlay(run);
            context.para.overlay(para);
            cell_raw.overlay(cell);
            for (slot, border) in cond_borders.iter_mut().zip(cell.borders) {
                if border.is_some() {
                    *slot = border;
                }
            }
        }
    }
    (context, cell_raw, cond_borders)
}

/// The rows of a table, looking through structured document tags.
fn table_rows<'a, 'input>(node: Node<'a, 'input>) -> Vec<Node<'a, 'input>> {
    let mut rows = Vec::new();
    for item in node.children().filter(Node::is_element) {
        if is(item, WORD_NS, "tr") {
            rows.push(item);
        } else if is(item, WORD_NS, "sdt")
            && let Some(content) = child(item, WORD_NS, "sdtContent")
        {
            rows.extend(table_rows(content));
        } else if is(item, WORD_NS, "customXml") {
            rows.extend(table_rows(item));
        }
    }
    rows
}

/// The cells of a row, looking through structured document tags.
fn row_cells<'a, 'input>(row: Node<'a, 'input>) -> Vec<Node<'a, 'input>> {
    let mut cells = Vec::new();
    for item in row.children().filter(Node::is_element) {
        if is(item, WORD_NS, "tc") {
            cells.push(item);
        } else if is(item, WORD_NS, "sdt")
            && let Some(content) = child(item, WORD_NS, "sdtContent")
        {
            cells.extend(row_cells(content));
        } else if is(item, WORD_NS, "customXml") {
            cells.extend(row_cells(item));
        }
    }
    cells
}

/// Adds the text read so far as an [`Inline::Text`].
fn flush(text: &mut String, fmt: &RunFmt, para: &mut Para) {
    if text.is_empty() {
        return;
    }
    let taken = std::mem::take(text);
    let mut fmt = fmt.clone();
    let taken = symbol_text(&taken, &mut fmt);
    para.inlines.push(Inline::Text { text: taken, fmt });
}

/// Text in a symbol font (Symbol, Wingdings), whose codes are not
/// Unicode, mapped to the Unicode characters they draw.
pub(super) fn symbol_text(text: &str, fmt: &mut RunFmt) -> String {
    let font = fmt.font.to_ascii_lowercase();
    let is_symbol = font == "symbol";
    let is_wingdings = font.starts_with("wingdings");
    if !is_symbol && !is_wingdings {
        return text.to_string();
    }
    text.chars()
        .map(|ch| {
            let code = u32::from(ch);
            let low = if (0xF000..=0xF0FF).contains(&code) {
                code - 0xF000
            } else {
                code
            };
            let Ok(low) = u8::try_from(low) else { return ch };
            if is_symbol {
                crate::word::symbols::symbol_font_char(low).unwrap_or(ch)
            } else {
                wingdings_char(low).unwrap_or(ch)
            }
        })
        .collect()
}

/// The Unicode character of the Wingdings characters used as bullets.
fn wingdings_char(code: u8) -> Option<char> {
    Some(match code {
        0x6C => '●',
        0x6E => '■',
        0x6F => '□',
        0x71 => '❑',
        0x75 => '◆',
        0x76 => '❖',
        0x77 => '⬥',
        0xA7 => '▪',
        0xA8 => '◻',
        0xD8 => '➢',
        0xE0 => '→',
        0xE8 => '➔',
        0xF0 => '⇨',
        0xFB => '✗',
        0xFC => '✓',
        0xFE => '☑',
        0x9F => '•',
        _ => return None,
    })
}

/// The layout field of an instruction (page number, page count) and its
/// number format switch.
pub(super) fn layout_field(instr: &str) -> Option<(FieldKind, Option<NumFormat>)> {
    let words = field_words(instr);
    let (name, args) = words.split_first()?;
    let kind = match name.to_ascii_uppercase().as_str() {
        "PAGE" => FieldKind::Page,
        "NUMPAGES" => FieldKind::NumPages,
        "SECTIONPAGES" => FieldKind::SectionPages,
        "PAGEREF" => FieldKind::PageRef(args.iter().find(|arg| !arg.starts_with('\\'))?.clone()),
        _ => return None,
    };
    let mut format = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "\\*"
            && let Some(name) = iter.next()
        {
            format = match name.as_str() {
                "roman" => Some(NumFormat::LowerRoman),
                "Roman" | "ROMAN" => Some(NumFormat::UpperRoman),
                "alphabetic" => Some(NumFormat::LowerLetter),
                "Alphabetic" | "ALPHABETIC" => Some(NumFormat::UpperLetter),
                "Arabic" | "arabic" => Some(NumFormat::Decimal),
                _ => format,
            };
        }
    }
    Some((kind, format))
}

/// The placement of a DrawingML floating object (`wp:anchor`).
fn drawing_anchor(node: Node<'_, '_>) -> Anchor {
    let dist = |name: &str| emu(node, name).unwrap_or(0.0);
    let position = |name: &str, default: AnchorFrom| {
        let Some(pos) = child(node, WP_NS, name) else {
            return AnchorPos {
                from: default,
                align: None,
                offset: 0.0,
            };
        };
        let from = match pos.attribute("relativeFrom").unwrap_or_default() {
            "page" => AnchorFrom::Page,
            "margin" => AnchorFrom::Margin,
            "column" => AnchorFrom::Column,
            "character" => AnchorFrom::Character,
            "paragraph" => AnchorFrom::Paragraph,
            "line" => AnchorFrom::Line,
            "leftMargin" | "topMargin" => AnchorFrom::StartMargin,
            "rightMargin" | "bottomMargin" => AnchorFrom::EndMargin,
            "insideMargin" => AnchorFrom::InsideMargin,
            "outsideMargin" => AnchorFrom::OutsideMargin,
            _ => default,
        };
        let align = child(pos, WP_NS, "align")
            .and_then(|align| align.text())
            .map(|text| match text {
                "center" => AnchorAlign::Center,
                "right" | "bottom" => AnchorAlign::End,
                "inside" => AnchorAlign::Inside,
                "outside" => AnchorAlign::Outside,
                _ => AnchorAlign::Start,
            });
        let offset = child(pos, WP_NS, "posOffset")
            .and_then(|offset| offset.text())
            .and_then(|text| text.trim().parse::<f32>().ok())
            .map_or(0.0, |value| value / EMU_PER_PT);
        AnchorPos { from, align, offset }
    };
    let mut horizontal = position("positionH", AnchorFrom::Column);
    let mut vertical = position("positionV", AnchorFrom::Paragraph);
    if node.attribute("simplePos").is_some_and(is_on)
        && let Some(simple) = child(node, WP_NS, "simplePos")
    {
        horizontal = AnchorPos {
            from: AnchorFrom::Page,
            align: None,
            offset: emu(simple, "x").unwrap_or(0.0),
        };
        vertical = AnchorPos {
            from: AnchorFrom::Page,
            align: None,
            offset: emu(simple, "y").unwrap_or(0.0),
        };
    }
    let wrap = if child(node, WP_NS, "wrapSquare").is_some() {
        Wrap::Square
    } else if child(node, WP_NS, "wrapTight").is_some() || child(node, WP_NS, "wrapThrough").is_some() {
        Wrap::Tight
    } else if child(node, WP_NS, "wrapTopAndBottom").is_some() {
        Wrap::TopBottom
    } else {
        Wrap::None
    };
    Anchor {
        horizontal,
        vertical,
        wrap,
        behind: node.attribute("behindDoc").is_some_and(is_on),
        dist: [dist("distT"), dist("distB"), dist("distL"), dist("distR")],
    }
}

/// The placement of a floating table (`w:tblpPr`).
fn table_anchor(node: Node<'_, '_>) -> Anchor {
    let from = |name: &str, default: AnchorFrom| match attr(node, WORD_NS, name) {
        Some("page") => AnchorFrom::Page,
        Some("margin") => AnchorFrom::Margin,
        Some("text") => AnchorFrom::Paragraph,
        _ => default,
    };
    let align = |name: &str| {
        attr(node, WORD_NS, name).map(|value| match value {
            "center" => AnchorAlign::Center,
            "right" | "bottom" => AnchorAlign::End,
            "inside" => AnchorAlign::Inside,
            "outside" => AnchorAlign::Outside,
            _ => AnchorAlign::Start,
        })
    };
    let h_from = match from("horzAnchor", AnchorFrom::Column) {
        AnchorFrom::Paragraph => AnchorFrom::Column,
        other => other,
    };
    Anchor {
        horizontal: AnchorPos {
            from: h_from,
            align: align("tblpXSpec"),
            offset: twips(node, "tblpX").unwrap_or(0.0),
        },
        vertical: AnchorPos {
            from: from("vertAnchor", AnchorFrom::Margin),
            align: align("tblpYSpec"),
            offset: twips(node, "tblpY").unwrap_or(0.0),
        },
        wrap: Wrap::Square,
        behind: false,
        dist: [
            twips(node, "topFromText").unwrap_or(0.0),
            twips(node, "bottomFromText").unwrap_or(0.0),
            twips(node, "leftFromText").unwrap_or(0.0),
            twips(node, "rightFromText").unwrap_or(0.0),
        ],
    }
}

/// The solid fill of a DrawingML shape's properties.
fn solid_fill(props: Node<'_, '_>) -> Option<Rgb> {
    if child(props, DRAWING_NS, "noFill").is_some() {
        return None;
    }
    let fill = child(props, DRAWING_NS, "solidFill")?;
    drawing_color(fill)
}

/// A DrawingML colour (`a:srgbClr`, or an approximated scheme colour).
fn drawing_color(node: Node<'_, '_>) -> Option<Rgb> {
    if let Some(rgb) = child(node, DRAWING_NS, "srgbClr") {
        return rgb.attribute("val").and_then(parse_color);
    }
    if let Some(sys) = child(node, DRAWING_NS, "sysClr") {
        return sys.attribute("lastClr").and_then(parse_color);
    }
    if let Some(scheme) = child(node, DRAWING_NS, "schemeClr") {
        return Some(match scheme.attribute("val").unwrap_or_default() {
            "bg1" | "lt1" => [255, 255, 255],
            "tx1" | "dk1" => [0, 0, 0],
            "accent1" => [68, 114, 196],
            "accent2" => [237, 125, 49],
            "accent3" => [165, 165, 165],
            "accent4" => [255, 192, 0],
            "accent5" => [91, 155, 213],
            "accent6" => [112, 173, 71],
            "tx2" | "dk2" => [68, 84, 106],
            "bg2" | "lt2" => [231, 230, 230],
            _ => [128, 128, 128],
        });
    }
    None
}

/// The outline of a DrawingML shape (`a:ln`) as a border.
fn line_border(ln: Node<'_, '_>) -> Option<super::model::Border> {
    if child(ln, DRAWING_NS, "noFill").is_some() {
        return None;
    }
    let color = child(ln, DRAWING_NS, "solidFill").and_then(drawing_color)?;
    Some(super::model::Border {
        width: emu(ln, "w").unwrap_or(0.75),
        space: 0.0,
        color,
        style: super::model::BorderStyle::Single,
    })
}

/// The declarations of a VML `style` attribute.
fn vml_style(style: &str) -> HashMap<String, String> {
    style
        .split(';')
        .filter_map(|declaration| {
            let (name, value) = declaration.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

/// A VML length (`12pt`, `1.5in`, `3cm`, `96px`, `100` for pixels) in
/// points.
fn vml_length(value: &str) -> Option<f32> {
    let value = value.trim();
    let split = value
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
        .unwrap_or(value.len());
    let number: f32 = value[..split].parse().ok()?;
    Some(match &value[split..] {
        "pt" => number,
        "in" => number * 72.0,
        "cm" => number * 72.0 / 2.54,
        "mm" => number * 72.0 / 25.4,
        "emu" => number / EMU_PER_PT,
        "pc" => number * 12.0,
        _ => number * 0.75,
    })
}

/// A VML colour (`#RRGGBB`, `#RGB`, or a few names).
fn vml_color(value: &str) -> Option<Rgb> {
    let value = value.split_whitespace().next()?;
    if let Some(hex) = value.strip_prefix('#') {
        if hex.len() == 3 {
            let expanded: String = hex.chars().flat_map(|ch| [ch, ch]).collect();
            return parse_color(&expanded);
        }
        return parse_color(hex.get(..6)?);
    }
    Some(match value {
        "black" => [0, 0, 0],
        "white" => [255, 255, 255],
        "red" => [255, 0, 0],
        "green" => [0, 128, 0],
        "blue" => [0, 0, 255],
        "yellow" => [255, 255, 0],
        "gray" | "grey" => [128, 128, 128],
        "silver" => [192, 192, 192],
        _ => return None,
    })
}

/// The placement of a floating VML shape.
fn vml_anchor(shape: Node<'_, '_>, css: &HashMap<String, String>) -> Anchor {
    let length = |name: &str| css.get(name).and_then(|value| vml_length(value)).unwrap_or(0.0);
    let from = |name: &str, default: AnchorFrom| match css.get(name).map(String::as_str) {
        Some("page") => AnchorFrom::Page,
        Some("margin") => AnchorFrom::Margin,
        Some("text") => AnchorFrom::Paragraph,
        Some("char") => AnchorFrom::Character,
        Some("line") => AnchorFrom::Line,
        Some("left-margin-area" | "top-margin-area") => AnchorFrom::StartMargin,
        Some("right-margin-area" | "bottom-margin-area") => AnchorFrom::EndMargin,
        _ => default,
    };
    let align = |name: &str| {
        css.get(name).and_then(|value| match value.as_str() {
            "center" => Some(AnchorAlign::Center),
            "right" | "bottom" => Some(AnchorAlign::End),
            "left" | "top" => Some(AnchorAlign::Start),
            "inside" => Some(AnchorAlign::Inside),
            "outside" => Some(AnchorAlign::Outside),
            _ => None,
        })
    };
    let h_from = match from("mso-position-horizontal-relative", AnchorFrom::Column) {
        AnchorFrom::Paragraph => AnchorFrom::Column,
        other => other,
    };
    let wrap = shape
        .children()
        .find(|item| item.tag_name().name() == "wrap")
        .and_then(|wrap| wrap.attribute("type"));
    let z_index = css
        .get("z-index")
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    Anchor {
        horizontal: AnchorPos {
            from: h_from,
            align: align("mso-position-horizontal"),
            offset: length("margin-left") + length("left"),
        },
        vertical: AnchorPos {
            from: from("mso-position-vertical-relative", AnchorFrom::Paragraph),
            align: align("mso-position-vertical"),
            offset: length("margin-top") + length("top"),
        },
        wrap: match wrap {
            Some("square") => Wrap::Square,
            Some("tight" | "through") => Wrap::Tight,
            Some("topAndBottom") => Wrap::TopBottom,
            _ => Wrap::None,
        },
        behind: z_index < 0,
        dist: [0.0; 4],
    }
}

/// The text of a list level, formatted, for tests.
#[cfg(test)]
pub(crate) fn number_text(number: u32, format: &NumFormat) -> String {
    super::styles::format_number(number, format)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The key of an obfuscated font is the GUID read backwards.
    #[test]
    fn fonts_are_deobfuscated() {
        let key = "{00112233-4455-6677-8899-AABBCCDDEEFF}";
        let mut data = vec![0u8; 40];
        deobfuscate(&mut data, key);
        assert_eq!(data[0], 0xFF);
        assert_eq!(data[15], 0x00);
        assert_eq!(data[16], 0xFF);
        assert_eq!(data[32], 0);
    }

    /// VML lengths in their units.
    #[test]
    fn vml_lengths() {
        assert_eq!(vml_length("72pt"), Some(72.0));
        assert_eq!(vml_length("1in"), Some(72.0));
        assert_eq!(vml_length("96px"), Some(72.0));
        assert_eq!(number_text(4, &NumFormat::LowerRoman), "iv");
    }
}
