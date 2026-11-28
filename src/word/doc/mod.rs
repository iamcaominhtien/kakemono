//! The `.doc` reader (the binary format of Word 97–2003): the streams of
//! the compound file read into a [`WordDoc`], like a `.docx`.
//!
//! The text is a sequence of characters (through the piece table, `fib`)
//! in which paragraph marks, cell marks, field marks and special characters
//! (pictures, note marks) are characters too; the properties of characters
//! and paragraphs are found by the byte each character is stored at
//! (`props`).

pub(crate) mod fib;
pub(crate) mod picture;
pub(crate) mod props;

use super::docx::field_link;
use super::ir::{Cell, Inline, Item, ListMark, Note, Para, Row, Table, VMerge, WordDoc};
use super::pictures::{MAX_PICTURE_BYTES, store};
use super::styles::{Numbering, RunProps, Styles};
use super::symbols::{symbol_font_char, symbol_font_text};
use crate::model::{MathNode, Span};
use anyhow::{Context, Result};
use fib::{Fib, Pair, Text, plc};
use props::{CellDef, ParaProps, Run, char_props, char_style, para_props, run_at};
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// Signature of a compound file (OLE2), the container of a `.doc`.
pub(crate) const COMPOUND_FILE_MAGIC: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
/// Paragraph mark.
const PARAGRAPH_END: u16 = 0x0D;
/// Cell mark (also the end of a table row).
const CELL_END: u16 = 0x07;
/// Line break.
const LINE_BREAK: u16 = 0x0B;
/// Page or section break.
const PAGE_BREAK: u16 = 0x0C;
/// Column break.
const COLUMN_BREAK: u16 = 0x0E;
/// Tab.
const TAB: u16 = 0x09;
/// Start of a field.
const FIELD_BEGIN: u16 = 0x13;
/// Separator between the instruction of a field and its result.
const FIELD_SEPARATE: u16 = 0x14;
/// End of a field.
const FIELD_END: u16 = 0x15;
/// A picture (special character).
const PICTURE: u16 = 0x01;
/// The mark of a footnote or endnote (special character).
const NOTE_MARK: u16 = 0x02;
/// Where a floating shape is anchored (special character).
const SHAPE_ANCHOR: u16 = 0x08;
/// Size of an `FSPA` (a floating shape of `PlcSpaMom`).
const FSPA_LEN: usize = 26;
/// Size of an `FTXBXS` (a text box of `PlcftxbxTxt`).
const FTXBXS_LEN: usize = 22;
/// Offset of the shape id in an `FTXBXS`.
const FTXBXS_LID: usize = 14;
/// A symbol character (special character `(`, with its font and code).
const SYMBOL: u16 = 0x28;
/// Non-breaking hyphen.
const NON_BREAKING_HYPHEN: u16 = 0x1E;
/// Size of a footnote reference element (`FRD`).
const FRD_LEN: usize = 2;
/// Cell edges closer than this (in twips) are one grid line.
const GRID_TOLERANCE_TWIPS: i16 = 30;

/// Whether `data` is a compound file, as a `.doc` is.
pub(crate) fn is_doc(data: &[u8]) -> bool {
    data.starts_with(COMPOUND_FILE_MAGIC)
}

/// Reads a `.doc` file.
///
/// # Errors
/// When the file is no Word 97–2003 document, is protected by a password,
/// or is damaged.
pub(crate) fn read(data: &[u8]) -> Result<WordDoc> {
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).context("not a Word document (damaged compound file)")?;
    let word = stream(&mut file, "/WordDocument")?.context("the file has no Word document in it")?;
    let fib = Fib::parse(&word)?;
    let table_name = if fib.table_1 { "/1Table" } else { "/0Table" };
    let table = stream(&mut file, table_name)?.context("damaged document: no table stream")?;
    let data_stream = stream(&mut file, "/Data")?.unwrap_or_default();
    let equations = equation_streams(&mut file);
    let text = Text::read(&word, fib.slice(&table, Pair::Clx))?;
    let fonts = props::fonts(fib.slice(&table, Pair::SttbfFfn));
    let styles = props::stylesheet(fib.slice(&table, Pair::Stshf), &fonts);
    let numbering = props::lists(&table, fib.offset(Pair::PlfLst), fib.slice(&table, Pair::PlfLfo));
    let mut reader = Reader {
        text: &text,
        data: &data_stream,
        fonts: &fonts,
        styles: &styles,
        numbering,
        chars: props::runs(&word, fib.slice(&table, Pair::PlcfBteChpx), false),
        paras: props::with_huge_papx(
            props::runs(&word, fib.slice(&table, Pair::PlcfBtePapx), true),
            &data_stream,
        ),
        bookmarks: HashMap::new(),
        note_refs: HashMap::new(),
        pictures: Vec::new(),
        fields: Vec::new(),
        anchors: HashMap::new(),
        text_boxes: HashMap::new(),
        boxes: Vec::new(),
        equations,
    };
    reader.read_text_boxes(&fib, &table);
    for (cp, name) in props::bookmarks(fib.slice(&table, Pair::SttbfBkmk), fib.slice(&table, Pair::PlcfBkf)) {
        reader.bookmarks.entry(cp).or_default().push(name);
    }
    let footnotes = note_ranges(&fib, &table, false);
    let endnotes = note_ranges(&fib, &table, true);
    for (endnote, ranges) in [(false, &footnotes), (true, &endnotes)] {
        for (index, (cp, _)) in ranges.iter().enumerate() {
            reader.note_refs.insert(*cp, (endnote, index.to_string()));
        }
    }
    let body = reader.items(0, fib.ccp_text);
    let mut notes = Vec::new();
    for (endnote, ranges) in [(false, footnotes), (true, endnotes)] {
        for (index, (_, (start, end))) in ranges.into_iter().enumerate() {
            notes.push(Note {
                endnote,
                id: index.to_string(),
                body: reader.items(start, end),
            });
        }
    }
    Ok(WordDoc {
        body,
        notes,
        pictures: reader.pictures,
        title: None,
        body_size_pt: styles.body_size_pt,
        body_font: styles.body_font.clone(),
    })
}

/// The `Equation Native` streams of the objects embedded in the document
/// (`ObjectPool/_<number>`), by number.
fn equation_streams(file: &mut cfb::CompoundFile<Cursor<&[u8]>>) -> HashMap<u32, Vec<u8>> {
    let names: Vec<(u32, String)> = file
        .read_storage("/ObjectPool")
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    let number = entry.name().strip_prefix('_')?.parse().ok()?;
                    Some((number, format!("/ObjectPool/{}/Equation Native", entry.name())))
                })
                .collect()
        })
        .unwrap_or_default();
    names
        .into_iter()
        .filter_map(|(number, path)| Some((number, stream(file, &path).ok()??)))
        .collect()
}

/// The contents of the stream `name`, or `None` when the file has none.
pub(crate) fn stream(file: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>> {
    if !file.is_stream(name) {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.open_stream(name)
        .and_then(|mut stream| stream.read_to_end(&mut bytes))
        .with_context(|| format!("reading the {} stream", name.trim_start_matches('/')))?;
    Ok(Some(bytes))
}

/// The notes of a kind: for each, the position of its mark in the main
/// text and the range of its text.
pub(crate) fn note_ranges(fib: &Fib, table: &[u8], endnote: bool) -> Vec<(u32, (u32, u32))> {
    let (refs, texts, base) = if endnote {
        (
            fib.slice(table, Pair::PlcfendRef),
            fib.slice(table, Pair::PlcfendTxt),
            fib.ccp_text + fib.ccp_ftn + fib.ccp_hdd + fib.ccp_atn,
        )
    } else {
        (
            fib.slice(table, Pair::PlcffndRef),
            fib.slice(table, Pair::PlcffndTxt),
            fib.ccp_text,
        )
    };
    let (marks, _) = plc(refs, FRD_LEN);
    let (starts, _) = plc(texts, 0);
    marks
        .iter()
        .take(marks.len().saturating_sub(1))
        .enumerate()
        .filter_map(|(index, &mark)| {
            let start = *starts.get(index)?;
            let end = *starts.get(index + 1)?;
            Some((mark, (base + start, base + end)))
        })
        .collect()
}

/// A field being read: its instruction, whether its result has started,
/// and where the result links to.
#[derive(Debug, Default)]
struct Field {
    /// The instruction text.
    instr: String,
    /// The result has started.
    in_result: bool,
    /// The link of the result.
    href: Option<String>,
    /// The formula of an embedded equation object of the field
    /// (`EMBED Equation.3`), which stands for its result (a picture).
    formula: Option<MathNode>,
    /// The result is left out (the formula stands for it).
    skip_result: bool,
}

/// A table being read: its rows so far, and the cells of the current row.
#[derive(Debug, Default)]
struct TableBuild {
    /// The rows read so far, each with the cell edges of its row end.
    rows: Vec<(Vec<Cell>, Vec<CellDef>, bool)>,
    /// The cells of the current row.
    cells: Vec<Cell>,
    /// The paragraphs of the current cell.
    items: Vec<Item>,
}

/// Walks the text of a `.doc`.
struct Reader<'a> {
    /// The text.
    text: &'a Text,
    /// The `Data` stream (pictures).
    data: &'a [u8],
    /// The font names.
    fonts: &'a [String],
    /// The styles.
    styles: &'a Styles,
    /// The lists and their counters.
    numbering: Numbering,
    /// The character property runs.
    chars: Vec<Run>,
    /// The paragraph property runs.
    paras: Vec<Run>,
    /// The bookmarks starting at each character position.
    bookmarks: HashMap<u32, Vec<String>>,
    /// The note whose mark is at each character position of the main text.
    note_refs: HashMap<u32, (bool, String)>,
    /// The pictures read so far.
    pictures: Vec<crate::model::Picture>,
    /// The fields open at this point.
    fields: Vec<Field>,
    /// The shape anchored at each character position of the main text.
    anchors: HashMap<u32, u32>,
    /// The text of each text box, by shape id.
    text_boxes: HashMap<u32, Vec<Item>>,
    /// Text box contents anchored in the paragraph being read: they come
    /// after it.
    boxes: Vec<Item>,
    /// The `Equation Native` streams of the embedded formulas, by the
    /// number of their object storage.
    equations: HashMap<u32, Vec<u8>>,
}

impl Reader<'_> {
    /// Reads the floating shapes of the main text and the text of its text
    /// boxes.
    fn read_text_boxes(&mut self, fib: &Fib, table: &[u8]) {
        let (cps, entries) = plc(fib.slice(table, Pair::PlcSpaMom), FSPA_LEN);
        for (cp, entry) in cps.iter().zip(entries) {
            if let Some(spid) = fib::u32_at(entry, 0) {
                self.anchors.insert(*cp, spid);
            }
        }
        let story = fib.ccp_text + fib.ccp_ftn + fib.ccp_hdd + fib.ccp_atn + fib.ccp_edn;
        let (cps, entries) = plc(fib.slice(table, Pair::PlcftxbxTxt), FTXBXS_LEN);
        for (at, entry) in entries.iter().enumerate() {
            let (Some(lid), Some(&start), Some(&end)) = (fib::u32_at(entry, FTXBXS_LID), cps.get(at), cps.get(at + 1))
            else {
                continue;
            };
            if start < end && self.anchors.values().any(|&spid| spid == lid) {
                let items = self.items(story + start, story + end);
                self.text_boxes.insert(lid, items);
            }
        }
    }

    /// The paragraphs and tables of the characters `start..end`.
    fn items(&mut self, start: u32, end: u32) -> Vec<Item> {
        let end = end.min(self.text.units.len() as u32);
        let mut items = Vec::new();
        let mut table: Option<TableBuild> = None;
        let mut cp = start;
        self.fields.clear();
        while cp < end {
            crate::common::cancel::check();
            let para_end = (cp..end)
                .find(|&at| matches!(self.text.units[at as usize], PARAGRAPH_END | CELL_END))
                .unwrap_or(end.saturating_sub(1).max(cp));
            let terminator = self.text.units.get(para_end as usize).copied().unwrap_or(PARAGRAPH_END);
            let props = self.para_props(para_end);
            let para = self.paragraph(cp, para_end, &props);
            cp = para_end + 1;
            if props.table_depth == 0 {
                if let Some(done) = table.take() {
                    items.push(Item::Table(finish_table(done)));
                }
                items.push(Item::Para(para));
                items.append(&mut self.boxes);
                continue;
            }
            let build = table.get_or_insert_with(TableBuild::default);
            if props.row_end && props.table_depth == 1 {
                let cells = std::mem::take(&mut build.cells);
                build.items.clear();
                build.rows.push((cells, props.cells.clone(), props.header));
                continue;
            }
            if props.inner_row_end {
                continue;
            }
            build.items.push(Item::Para(para));
            build.items.append(&mut self.boxes);
            if terminator == CELL_END && props.table_depth == 1 {
                let body = std::mem::take(&mut build.items);
                build.cells.push(Cell {
                    span: 1,
                    merge: VMerge::None,
                    body,
                });
            }
        }
        if let Some(done) = table {
            items.push(Item::Table(finish_table(done)));
        }
        items
    }

    /// The properties of the paragraph whose mark is at `cp`.
    fn para_props(&self, cp: u32) -> ParaProps {
        let fc = self.text.fcs.get(cp as usize).copied().unwrap_or(0);
        run_at(&self.paras, fc).map_or_else(ParaProps::default, |run| para_props(run.istd, &run.grpprl))
    }

    /// The paragraph of the characters `start..mark` (its mark at `mark`).
    fn paragraph(&mut self, start: u32, mark: u32, props: &ParaProps) -> Para {
        let style_id = props.istd.to_string();
        let (style, style_outline, style_numbering) = self.styles.paragraph(Some(&style_id));
        let numbering = (props.ilfo != 0)
            .then(|| (props.ilfo.to_string(), props.ilvl))
            .or(style_numbering);
        let list =
            numbering.and_then(|(id, level)| self.numbering.next(&id, level).map(|marker| ListMark { level, marker }));
        let mut para = Para {
            style,
            outline: props.outline.or(style_outline),
            list,
            ..Default::default()
        };
        let mut units: Vec<u16> = Vec::new();
        let mut current: Option<RunProps> = None;
        for cp in start..mark {
            if let Some(names) = self.bookmarks.get(&cp) {
                para.bookmarks.extend(names.iter().cloned());
            }
            let unit = self.text.units[cp as usize];
            let fc = self.text.fcs[cp as usize];
            let (run_props, extras) = self.char_props(&style_id, fc);
            // The object of an embedding field is named by its separator (or
            // by a picture character of its instruction).
            if matches!(unit, PICTURE | FIELD_SEPARATE) && extras.special && extras.ole {
                self.field_object(extras.picture);
            }
            if self.field_char(unit, &mut units, current.as_ref(), &mut para) {
                continue;
            }
            if run_props.hidden == Some(true) {
                continue;
            }
            if current != Some(run_props) {
                flush(&mut units, current.as_ref(), self.field_href(), &mut para);
                current = Some(run_props);
            }
            self.character(cp, unit, extras, &mut units, current.as_ref(), &mut para);
        }
        flush(&mut units, current.as_ref(), self.field_href(), &mut para);
        super::display_alone_formula(&mut para);
        para
    }

    /// Handles a character of a field: its start, the separator before its
    /// result, its end, or a character of its instruction (which Word does
    /// not show). Returns whether the character was one of these.
    fn field_char(&mut self, unit: u16, units: &mut Vec<u16>, current: Option<&RunProps>, para: &mut Para) -> bool {
        match unit {
            FIELD_BEGIN => {
                flush(units, current, self.field_href(), para);
                self.fields.push(Field::default());
                true
            }
            FIELD_SEPARATE => {
                flush(units, current, self.field_href(), para);
                if let Some(field) = self.fields.last_mut() {
                    field.in_result = true;
                    field.href = field_link(&field.instr);
                    if let Some(node) = field.formula.take() {
                        para.inlines.push(Inline::Math { node, display: false });
                        field.skip_result = true;
                    }
                }
                true
            }
            FIELD_END => {
                flush(units, current, self.field_href(), para);
                self.fields.pop();
                true
            }
            _ if self.fields.iter().any(|field| field.skip_result) => true,
            _ if self.fields.iter().any(|field| !field.in_result) => {
                if let Some(field) = self.fields.last_mut() {
                    // Objects of the field (its data) stand in the
                    // instruction as control characters.
                    let text = if unit < 0x20 {
                        String::from(" ")
                    } else {
                        String::from_utf16_lossy(&[unit])
                    };
                    field.instr.push_str(&text);
                }
                true
            }
            _ => false,
        }
    }

    /// Notes the formula of the embedded object stored under `location`
    /// when it appears in the instruction of the open field.
    fn field_object(&mut self, location: Option<u32>) {
        let formula = location
            .and_then(|location| self.equations.get(&location))
            .and_then(|native| super::mtef::equation(native));
        if let Some(field) = self.fields.last_mut().filter(|field| !field.in_result) {
            field.formula = formula.or(field.formula.take());
        }
    }

    /// Adds the character `unit` at `cp` to the paragraph: text to
    /// `units`, a picture or a note mark as an inline of its own.
    fn character(
        &mut self,
        cp: u32,
        unit: u16,
        extras: props::CharExtras,
        units: &mut Vec<u16>,
        current: Option<&RunProps>,
        para: &mut Para,
    ) {
        match unit {
            SHAPE_ANCHOR if extras.special => {
                if let Some(items) = self.anchors.get(&cp).and_then(|spid| self.text_boxes.remove(spid)) {
                    self.boxes.extend(items);
                }
            }
            PICTURE if extras.special => {
                flush(units, current, self.field_href(), para);
                let formula = extras
                    .picture
                    .filter(|_| extras.ole)
                    .and_then(|location| self.equations.get(&location))
                    .and_then(|native| super::mtef::equation(native));
                if let Some(node) = formula {
                    para.inlines.push(Inline::Math { node, display: false });
                } else if let Some(location) = extras.picture {
                    para.inlines.push(self.picture(location));
                }
            }
            NOTE_MARK if extras.special => {
                flush(units, current, self.field_href(), para);
                if let Some((endnote, id)) = self.note_refs.get(&cp).cloned() {
                    para.inlines.push(Inline::NoteRef { endnote, id });
                }
            }
            // Word draws the symbol of `sprmCSymbol` even on a character
            // not marked special.
            SYMBOL if extras.special || extras.symbol.is_some() => {
                if let Some((font, code)) = extras.symbol {
                    units.extend(
                        symbol(self.fonts.get(usize::from(font)), code)
                            .encode_utf16(&mut [0; 2])
                            .iter()
                            .copied(),
                    );
                }
            }
            LINE_BREAK => units.push(u16::from(b'\n')),
            TAB => units.push(u16::from(b' ')),
            NON_BREAKING_HYPHEN => units.push(0x2011),
            PAGE_BREAK | COLUMN_BREAK => {}
            unit if unit < 0x20 => {}
            unit => units.push(unit),
        }
    }

    /// The link of the innermost field whose result is being read.
    fn field_href(&self) -> Option<String> {
        self.fields.iter().rev().find_map(|field| field.href.clone())
    }

    /// The text properties of the character stored at `fc` in a paragraph
    /// of style `style_id`, and its other properties.
    fn char_props(&self, style_id: &str, fc: u32) -> (RunProps, props::CharExtras) {
        let grpprl = run_at(&self.chars, fc).map_or(&[][..], |run| run.grpprl.as_slice());
        let char_style = char_style(grpprl).map(|istd| istd.to_string());
        let style_props = self.styles.run_props(Some(style_id), char_style.as_deref());
        let (own, extras) = char_props(grpprl, style_props, self.fonts);
        (style_props.with(&own), extras)
    }

    /// The picture at `location` of the `Data` stream, read and converted.
    fn picture(&mut self, location: u32) -> Inline {
        let (file, (width_pt, height_pt)) = picture::picture(self.data, location);
        let index = file.and_then(|data| store(&mut self.pictures, data, MAX_PICTURE_BYTES));
        Inline::Picture {
            index,
            width_pt,
            height_pt,
            alt: String::new(),
        }
    }
}

/// Adds the characters `units` as a span with the properties `props`, and
/// empties them.
fn flush(units: &mut Vec<u16>, props: Option<&RunProps>, href: Option<String>, para: &mut Para) {
    if units.is_empty() {
        return;
    }
    let text = String::from_utf16_lossy(units);
    units.clear();
    let props = props.copied().unwrap_or_default();
    let text = if props.symbol == Some(true) {
        symbol_font_text(&text)
    } else {
        text
    };
    let mut span = Span::styled(text, props.style_bits());
    span.color = props.color.unwrap_or([0; 3]);
    span.href = href;
    para.inlines.push(Inline::Text(span));
}

/// The character `code` of the symbol font `font`, in Unicode.
fn symbol(font: Option<&String>, code: u16) -> char {
    let code = if (0xF000..=0xF0FF).contains(&code) {
        code - 0xF000
    } else {
        code
    };
    if font.is_some_and(|font| font.eq_ignore_ascii_case("Symbol"))
        && let Some(ch) = u8::try_from(code).ok().and_then(symbol_font_char)
    {
        return ch;
    }
    char::from_u32(u32::from(code)).unwrap_or(char::REPLACEMENT_CHARACTER)
}

/// The table of the rows read: the cells of each row placed on one grid of
/// columns from the cell edges, with their spans and merges.
fn finish_table(build: TableBuild) -> Table {
    let mut rows = build.rows;
    if !build.cells.is_empty() {
        rows.push((build.cells, Vec::new(), false));
    }
    let mut lines: Vec<i16> = rows
        .iter()
        .flat_map(|(_, defs, _)| defs.iter().flat_map(|def| [def.left, def.right]))
        .collect();
    lines.sort_unstable();
    lines.dedup_by(|a, b| (*a - *b).abs() <= GRID_TOLERANCE_TWIPS);
    let column = |edge: i16| {
        lines
            .iter()
            .position(|line| (line - edge).abs() <= GRID_TOLERANCE_TWIPS)
    };
    let mut table = Table::default();
    for (cells, defs, header) in rows {
        let mut row = Row {
            cells: Vec::new(),
            header,
        };
        for (index, mut cell) in cells.into_iter().enumerate() {
            let def = defs.get(index).copied().unwrap_or_default();
            if def.merged
                && let Some(previous) = row.cells.last_mut()
            {
                previous.span += 1;
                continue;
            }
            cell.span = match (column(def.left), column(def.right)) {
                (Some(left), Some(right)) if right > left => right - left,
                _ => 1,
            };
            cell.merge = match (def.vert_merge, def.vert_restart) {
                (true, true) => VMerge::Restart,
                (true, false) => VMerge::Continue,
                _ => VMerge::None,
            };
            row.cells.push(cell);
        }
        table.rows.push(row);
    }
    table
}
