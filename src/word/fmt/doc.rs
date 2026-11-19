//! Reads a `.doc` (Word 97–2003) with all the formatting the layout needs:
//! sections, headers and footers, styles and direct formatting resolved
//! from their property modifiers (sprms), lists, tables, notes, fields and
//! pictures, into the same form as a `.docx` ([`FDoc`]).

use super::drawing;
use super::model::{
    Block, BreakKind, Cell, FDoc, FImage, FieldKind, HeaderSet, Inline, NumFormat, NumberMark, NumberSuffix, Para, Row,
    RunFmt, Section, Shape, Table, TableProps, VMerge, Width,
};
use super::props::{RawPara, RawRun};
use super::sprm::{RowProps, SectionProps, apply_chars, apply_para, char_extras, char_style, row_props, section_props};
use super::styles::{Level, Numbering, Style, StyleKind, Styles, parse_num_format};
use crate::word::doc::fib::{Fib, Pair, Text, plc, u16_at, u32_at};
use crate::word::doc::props::{Run, bookmarks, fonts as font_table, run_at, runs, utf16};
use crate::word::doc::{note_ranges, picture, stream};
use crate::word::pictures::normalize;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Cursor;

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
/// Separator between a field's instruction and its result.
const FIELD_SEPARATE: u16 = 0x14;
/// End of a field.
const FIELD_END: u16 = 0x15;
/// A picture (special character).
const PICTURE: u16 = 0x01;
/// The mark of a note (special character).
const NOTE_MARK: u16 = 0x02;
/// Character marking where a floating shape is anchored.
const SHAPE_ANCHOR: u16 = 0x08;
/// A symbol (special character, with its font and code).
const SYMBOL: u16 = 0x28;
/// Non-breaking hyphen.
const NON_BREAKING_HYPHEN: u16 = 0x1E;
/// Optional hyphen.
const SOFT_HYPHEN: u16 = 0x1F;
/// Size of a section descriptor (`SED`).
const SED_LEN: usize = 12;
/// Size of a list definition (`LSTF`).
const LSTF_LEN: usize = 28;
/// Size of the fixed part of a list level (`LVLF`).
const LVLF_LEN: usize = 28;
/// Size of a list override (`LFO`).
const LFO_LEN: usize = 16;
/// Levels of a list that is not a simple list.
const LIST_LEVELS: usize = 9;
/// Size of a list level override (`LFOLVL`).
const LFOLVL_LEN: usize = 8;

/// `istdBase` of a style based on no other.
const NO_BASE: u16 = 0x0FFF;
/// Stories of the header document before those of the sections
/// (separators and continuation notices).
const SEPARATOR_STORIES: usize = 6;
/// Stories per section: even header, odd header, even footer, odd footer,
/// first header, first footer.
const STORIES_PER_SECTION: usize = 6;
/// Twips per point.
const TWIPS_PER_PT: f32 = 20.0;
/// Word 97's default font size, in points.
const DEFAULT_SIZE_PT: f32 = 10.0;
/// Cell edges closer than this are one grid line, in points.
const GRID_TOLERANCE_PT: f32 = 1.5;

/// Each style of the style sheet, by index (`istd`): its kind, the style it
/// is based on, and its raw paragraph and character property lists.
type StyleKinds = HashMap<u16, (StyleKind, u16, Vec<u8>, Vec<u8>)>;

/// A field being read.
#[derive(Debug, Default)]
struct FieldState {
    instr: String,
    in_result: bool,
    skip_result: bool,
    href: Option<String>,
}

/// The reader.
struct Reader<'a> {
    text: &'a Text,
    data: &'a [u8],
    fonts: &'a [String],
    styles: Styles,
    /// Resolved properties of each paragraph style and of each character
    /// style, by `istd`.
    para_styles: HashMap<u16, (RawPara, RawRun)>,
    /// Each paragraph style's properties split into what Normal gives and
    /// what the style and its other base styles set (a list set by the
    /// style goes between them).
    style_layers: HashMap<u16, (RawPara, RawPara)>,
    char_styles: HashMap<u16, RawRun>,
    numbering: Numbering,
    chars: Vec<Run>,
    paras: Vec<Run>,
    bookmarks: HashMap<u32, Vec<String>>,
    note_refs: HashMap<u32, (bool, String)>,
    images: Vec<FImage>,
    fields: Vec<FieldState>,
    defaults: RawRun,
    /// The floating shapes of the main text, by the position of their
    /// anchor.
    floats: HashMap<u32, drawing::Float>,
    /// The text of the text boxes, by shape id.
    text_boxes: HashMap<u32, Vec<Block>>,
    /// Floating shapes anchored next to each other, waiting to be put in
    /// their drawing order.
    pending_shapes: Vec<(usize, Shape)>,
}

/// The document-wide settings of a `.doc`, from its DOP.
fn settings(dop: &[u8]) -> FDoc {
    FDoc {
        default_tab: u16_at(dop, 10)
            .map(|twips| f32::from(twips) / TWIPS_PER_PT)
            .filter(|tab| (1.0..=288.0).contains(tab))
            .unwrap_or(36.0),
        even_and_odd_headers: dop.first().is_some_and(|flags| flags & 1 != 0),
        endnote_format: NumFormat::LowerRoman,
        compat_mode: 11,
        split_break_and_mark: true,
        ..Default::default()
    }
}

/// Reads a `.doc` file with its formatting.
///
/// # Errors
/// When the file is not a Word 97–2003 document or is damaged.
pub fn read(data: &[u8]) -> Result<FDoc> {
    let mut file = cfb::CompoundFile::open(Cursor::new(data)).context("not a Word document (damaged compound file)")?;
    let word = stream(&mut file, "/WordDocument")?.context("the file has no Word document in it")?;
    let fib = Fib::parse(&word)?;
    let table_name = if fib.table_1 { "/1Table" } else { "/0Table" };
    let table = stream(&mut file, table_name)?.context("damaged document: no table stream")?;
    let data_stream = stream(&mut file, "/Data")?.unwrap_or_default();
    let text = Text::read(&word, fib.slice(&table, Pair::Clx))?;
    let fonts = font_table(fib.slice(&table, Pair::SttbfFfn));
    let (styles, defaults, kinds) = stylesheet(fib.slice(&table, Pair::Stshf), &fonts);
    let numbering = lists(
        &table,
        fib.offset(Pair::PlfLst),
        fib.slice(&table, Pair::PlfLfo),
        &fonts,
    );
    let mut reader = Reader {
        text: &text,
        data: &data_stream,
        fonts: &fonts,
        styles,
        para_styles: HashMap::new(),
        style_layers: HashMap::new(),
        char_styles: HashMap::new(),
        numbering,
        chars: runs(&word, fib.slice(&table, Pair::PlcfBteChpx), false),
        paras: crate::word::doc::props::with_huge_papx(
            runs(&word, fib.slice(&table, Pair::PlcfBtePapx), true),
            &data_stream,
        ),
        bookmarks: HashMap::new(),
        note_refs: HashMap::new(),
        images: Vec::new(),
        fields: Vec::new(),
        defaults,
        floats: drawing::floats(
            fib.slice(&table, Pair::PlcSpaMom),
            fib.slice(&table, Pair::DggInfo),
            &word,
        ),
        text_boxes: HashMap::new(),
        pending_shapes: Vec::new(),
    };
    reader.resolve_styles(&kinds);
    reader.read_text_boxes(&fib, &table);
    for (cp, name) in bookmarks(fib.slice(&table, Pair::SttbfBkmk), fib.slice(&table, Pair::PlcfBkf)) {
        reader.bookmarks.entry(cp).or_default().push(name);
    }
    let footnotes = note_ranges(&fib, &table, false);
    let endnotes = note_ranges(&fib, &table, true);
    for (endnote, ranges) in [(false, &footnotes), (true, &endnotes)] {
        for (index, (cp, _)) in ranges.iter().enumerate() {
            reader.note_refs.insert(*cp, (endnote, index.to_string()));
        }
    }
    let mut doc = settings(fib.slice(&table, Pair::Dop));
    let ranges = section_ranges(&fib, &table, &word);
    doc.sections = reader.sections(&mut doc, &fib, &table, ranges);
    for (endnote, ranges) in [(false, footnotes), (true, endnotes)] {
        for (index, (_, (start, end))) in ranges.into_iter().enumerate() {
            let blocks = reader.blocks(start, end);
            if endnote {
                doc.endnotes.push((index.to_string(), blocks));
            } else {
                doc.footnotes.push((index.to_string(), blocks));
            }
        }
    }
    doc.images = reader.images;
    doc.font_kinds = HashMap::new();
    Ok(doc)
}

/// The sections of the main text: the range of characters of each, with
/// its properties (one section for the whole text when there is none).
fn section_ranges(fib: &Fib, table: &[u8], word: &[u8]) -> Vec<(u32, u32, SectionProps)> {
    let (bounds, seds) = plc(fib.slice(table, Pair::PlcfSed), SED_LEN);
    let mut ranges: Vec<(u32, u32, SectionProps)> = Vec::new();
    for (at, sed) in seds.iter().enumerate() {
        let (Some(&start), Some(&end)) = (bounds.get(at), bounds.get(at + 1)) else {
            break;
        };
        let fc = u32_at(sed, 2).unwrap_or(u32::MAX);
        let grpprl = if fc == u32::MAX {
            &[][..]
        } else {
            let fc = fc as usize;
            let len = usize::from(u16_at(word, fc).unwrap_or(0));
            word.get(fc + 2..fc + 2 + len).unwrap_or(&[])
        };
        ranges.push((start, end.min(fib.ccp_text), section_props(grpprl)));
    }
    if ranges.is_empty() {
        ranges.push((0, fib.ccp_text, section_props(&[])));
    }
    ranges
}

impl Reader<'_> {
    /// Reads the sections in `ranges` with their headers and footers (six
    /// stories per section; an empty one inherits the previous section's),
    /// adding the stories and the footnote separator to `doc`.
    fn sections(
        &mut self,
        doc: &mut FDoc,
        fib: &Fib,
        table: &[u8],
        ranges: Vec<(u32, u32, SectionProps)>,
    ) -> Vec<Section> {
        let mut sections = Vec::new();
        let (hdd, _) = plc(fib.slice(table, Pair::PlcfHdd), 0);
        let hdd_base = fib.ccp_text + fib.ccp_ftn;
        // The first story is the footnote separator.
        if let (Some(&from), Some(&to)) = (hdd.first(), hdd.get(1))
            && to > from
        {
            doc.footnote_separator = Some(self.story(hdd_base + from, hdd_base + to));
        }
        let mut previous = [None; STORIES_PER_SECTION];
        for (at, (start, end, props)) in ranges.into_iter().enumerate() {
            let mut stories = previous;
            for (slot, story) in stories.iter_mut().enumerate() {
                let index = SEPARATOR_STORIES + at * STORIES_PER_SECTION + slot;
                let (Some(&from), Some(&to)) = (hdd.get(index), hdd.get(index + 1)) else {
                    continue;
                };
                if to > from {
                    let blocks = self.story(hdd_base + from, hdd_base + to);
                    doc.parts.push(blocks);
                    *story = Some(doc.parts.len() - 1);
                }
            }
            previous = stories;
            let [
                even_header,
                odd_header,
                even_footer,
                odd_footer,
                first_header,
                first_footer,
            ] = stories;
            let blocks = self.blocks(start, end);
            sections.push(Section {
                page: props.page,
                columns: props.columns,
                start: props.start,
                headers: HeaderSet {
                    default: odd_header,
                    first: first_header,
                    even: even_header,
                },
                footers: HeaderSet {
                    default: odd_footer,
                    first: first_footer,
                    even: even_footer,
                },
                title_page: props.title_page,
                page_number_start: props.page_number_start,
                page_number_format: props.page_number_format,
                grid_pitch: props.grid_pitch,
                blocks,
            });
        }
        sections
    }
}

/// The style sheet: the styles with their raw property lists, the default
/// text properties, and each style's kind, base and lists.
fn stylesheet(stsh: &[u8], fonts: &[String]) -> (Styles, RawRun, StyleKinds) {
    let header_len = usize::from(u16_at(stsh, 0).unwrap_or(0));
    let header = stsh.get(2..2 + header_len).unwrap_or(&[]);
    let count = usize::from(u16_at(header, 0).unwrap_or(0));
    let base_len = usize::from(u16_at(header, 2).unwrap_or(10));
    let font = |offset: usize| u16_at(header, offset).and_then(|index| fonts.get(usize::from(index)).cloned());
    let defaults = RawRun {
        font: font(12),
        font_ea: font(14),
        font_hansi: font(16),
        size: Some(DEFAULT_SIZE_PT),
        ..Default::default()
    };
    let mut kinds = HashMap::new();
    let mut pos = 2 + header_len;
    for istd in 0..count {
        let Some(len) = u16_at(stsh, pos).map(usize::from) else {
            break;
        };
        let std = stsh.get(pos + 2..pos + 2 + len).unwrap_or(&[]);
        pos += 2 + len;
        if std.is_empty() {
            continue;
        }
        let kind_bits = u16_at(std, 2).unwrap_or(0) & 0x000F;
        let base = u16_at(std, 2).unwrap_or(0) >> 4;
        let kind = match kind_bits {
            2 => StyleKind::Character,
            3 => StyleKind::Table,
            4 => StyleKind::Numbering,
            _ => StyleKind::Paragraph,
        };
        let name_len = usize::from(u16_at(std, base_len).unwrap_or(0));
        let mut upx_pos = base_len + 2 + name_len * 2 + 2;
        let upx_count = match kind {
            StyleKind::Paragraph => 2,
            StyleKind::Character => 1,
            StyleKind::Table => 3,
            StyleKind::Numbering => 1,
        };
        let mut upxs: Vec<&[u8]> = Vec::new();
        for _ in 0..upx_count {
            upx_pos += upx_pos % 2;
            let Some(len) = u16_at(std, upx_pos).map(usize::from) else {
                break;
            };
            upxs.push(std.get(upx_pos + 2..upx_pos + 2 + len).unwrap_or(&[]));
            upx_pos += 2 + len;
        }
        let (papx, chpx) = match (kind, upxs.as_slice()) {
            (StyleKind::Paragraph, [papx, chpx, ..]) => (papx.get(2..).unwrap_or(&[]).to_vec(), chpx.to_vec()),
            (StyleKind::Character, [chpx, ..]) => (Vec::new(), chpx.to_vec()),
            (StyleKind::Table, [_, papx, chpx, ..]) => (papx.get(2..).unwrap_or(&[]).to_vec(), chpx.to_vec()),
            _ => (Vec::new(), Vec::new()),
        };
        if std::env::var_os("KAKEMONO_DEBUG_DOC").is_some() {
            eprintln!(
                "STYLE {istd} {kind:?} base {base}: {} | {}",
                debug_sprms(&papx),
                debug_sprms(&chpx)
            );
        }
        kinds.insert(istd as u16, (kind, base, papx, chpx));
    }
    let mut styles = Styles::default();
    styles.default_run = defaults.clone();
    (styles, defaults, kinds)
}

/// The lists: each list definition with its levels, and each override
/// (`ilfo`, 1-based) that paragraphs name.
fn lists(table: &[u8], plf_lst_offset: usize, plf_lfo: &[u8], fonts: &[String]) -> Numbering {
    let mut numbering = Numbering::default();
    let Some(plf_lst) = table.get(plf_lst_offset..).filter(|_| plf_lst_offset > 0) else {
        return numbering;
    };
    let count = usize::from(u16_at(plf_lst, 0).unwrap_or(0));
    let mut pos = 2 + count * LSTF_LEN;
    let mut by_id = HashMap::new();
    for index in 0..count {
        let lstf = plf_lst
            .get(2 + index * LSTF_LEN..2 + (index + 1) * LSTF_LEN)
            .unwrap_or(&[]);
        let id = u32_at(lstf, 0).unwrap_or(0);
        let simple = lstf.get(26).is_some_and(|flags| flags & 1 != 0);
        let mut levels = Vec::new();
        for _ in 0..if simple { 1 } else { LIST_LEVELS } {
            let Some((level, len)) = level(plf_lst.get(pos..).unwrap_or(&[]), fonts) else {
                break;
            };
            levels.push(level);
            pos += len;
        }
        by_id.insert(id, levels);
    }
    // The overrides, then their level overrides (`LFOData`).
    let lfos = usize::try_from(u32_at(plf_lfo, 0).unwrap_or(0)).unwrap_or(0);
    let mut data_pos = 4 + lfos * LFO_LEN;
    for index in 0..lfos {
        let lfo = plf_lfo
            .get(4 + index * LFO_LEN..4 + (index + 1) * LFO_LEN)
            .unwrap_or(&[]);
        let Some(id) = u32_at(lfo, 0) else { break };
        let level_overrides = usize::from(lfo.get(12).copied().unwrap_or(0));
        let mut overrides = HashMap::new();
        // `LFOData`: a CP, then the level overrides.
        data_pos += 4;
        for _ in 0..level_overrides {
            let Some(entry) = plf_lfo.get(data_pos..data_pos + LFOLVL_LEN) else {
                break;
            };
            let start = u32_at(entry, 0).unwrap_or(0);
            let flags = u32_at(entry, 4).unwrap_or(0);
            data_pos += LFOLVL_LEN;
            let level_index = (flags & 0x0F) as u8;
            let restart = flags & 0x10 != 0;
            let mut whole = None;
            if flags & 0x20 != 0
                && let Some((level, len)) = level(plf_lfo.get(data_pos..).unwrap_or(&[]), fonts)
            {
                whole = Some(level);
                data_pos += len;
            }
            overrides.insert(level_index, (restart.then_some(start), whole));
        }
        if let Some(levels) = by_id.get(&id) {
            numbering.insert((index + 1).to_string(), id.to_string(), levels.clone(), overrides);
        }
    }
    numbering
}

/// One list level (`LVL`) and its size in bytes.
fn level(bytes: &[u8], fonts: &[String]) -> Option<(Level, usize)> {
    let lvlf = bytes.get(..LVLF_LEN)?;
    let start = u32_at(lvlf, 0)?;
    let format = match lvlf[4] {
        1 => "upperRoman",
        2 => "lowerRoman",
        3 => "upperLetter",
        4 => "lowerLetter",
        5 => "ordinal",
        6 => "cardinalText",
        7 => "ordinalText",
        22 => "decimalZero",
        23 => "bullet",
        255 => "none",
        _ => "decimal",
    };
    let flags = lvlf[5];
    let places: Vec<usize> = lvlf[6..15]
        .iter()
        .take_while(|&&place| place != 0)
        .map(|&place| usize::from(place))
        .collect();
    let suffix = match lvlf[15] {
        1 => NumberSuffix::Space,
        2 => NumberSuffix::Nothing,
        _ => NumberSuffix::Tab,
    };
    let chpx_len = usize::from(lvlf[24]);
    let papx_len = usize::from(lvlf[25]);
    let restart = lvlf[26];
    let papx = bytes.get(LVLF_LEN..LVLF_LEN + papx_len).unwrap_or(&[]);
    let chpx = bytes
        .get(LVLF_LEN + papx_len..LVLF_LEN + papx_len + chpx_len)
        .unwrap_or(&[]);
    let text_at = LVLF_LEN + papx_len + chpx_len;
    let len = usize::from(u16_at(bytes, text_at)?);
    let units: Vec<u16> = (0..len)
        .filter_map(|index| u16_at(bytes, text_at + 2 + index * 2))
        .collect();
    let mut text = String::new();
    for (index, unit) in units.iter().enumerate() {
        if places.contains(&(index + 1)) {
            text.push_str(&format!("%{}", unit + 1));
        } else {
            text.push_str(&String::from_utf16_lossy(&[*unit]));
        }
    }
    let mut para = RawPara::default();
    apply_para(papx, &mut para);
    let mut run = RawRun::default();
    apply_chars(chpx, &mut run, &RawRun::default(), fonts);
    let level = Level {
        start,
        format: parse_num_format(format),
        text,
        align: match flags & 0x03 {
            1 => super::model::Align::Center,
            2 => super::model::Align::Right,
            _ => super::model::Align::Left,
        },
        suffix,
        para,
        run,
        restart: (restart != 0).then_some(u32::from(restart)),
        legal: flags & 0x04 != 0,
    };
    Some((level, text_at + 2 + len * 2))
}

impl Reader<'_> {
    /// Resolves every style with what it inherits.
    fn resolve_styles(&mut self, kinds: &StyleKinds) {
        fn resolve(istd: u16, kinds: &StyleKinds, fonts: &[String], depth: usize) -> (RawPara, RawRun) {
            let Some((_, base, papx, chpx)) = kinds.get(&istd) else {
                return (RawPara::default(), RawRun::default());
            };
            let (mut para, mut run) = if *base != NO_BASE && depth < 16 {
                resolve(*base, kinds, fonts, depth + 1)
            } else {
                (RawPara::default(), RawRun::default())
            };
            apply_para(papx, &mut para);
            let inherited = run.clone();
            apply_chars(chpx, &mut run, &inherited, fonts);
            (para, run)
        }
        for (&istd, (kind, _, _, _)) in kinds {
            let (para, run) = resolve(istd, kinds, self.fonts, 0);
            if *kind != StyleKind::Character {
                // Normal (style 0), and what the styles above it set.
                let mut chain = vec![istd];
                while let Some(&(_, base, _, _)) = chain.last().and_then(|at| kinds.get(at)) {
                    if base == NO_BASE || chain.len() > 16 {
                        break;
                    }
                    chain.push(base);
                }
                let normal = if chain.contains(&0) && istd != 0 {
                    resolve(0, kinds, self.fonts, 1).0
                } else {
                    RawPara::default()
                };
                let mut own = RawPara::default();
                for at in chain.iter().rev().filter(|&&at| at != 0 || istd == 0) {
                    if let Some((_, _, papx, _)) = kinds.get(at) {
                        apply_para(papx, &mut own);
                    }
                }
                self.style_layers.insert(istd, (normal, own));
            }
            match kind {
                StyleKind::Character => {
                    self.char_styles.insert(istd, run);
                }
                _ => {
                    self.styles.insert(
                        istd.to_string(),
                        Style {
                            kind: *kind,
                            ..Default::default()
                        },
                        istd == 0,
                    );
                    self.para_styles.insert(istd, (para, run));
                }
            }
        }
    }

    /// A header or footer story: its blocks, without the paragraph mark Word
    /// adds at the end of every story.
    fn story(&mut self, start: u32, end: u32) -> Vec<Block> {
        let mut blocks = self.blocks(start, end);
        if blocks.len() > 1
            && let Some(Block::Para(last)) = blocks.last()
            && last.inlines.is_empty()
        {
            blocks.pop();
        }
        blocks
    }

    /// The properties of the paragraph whose mark is at `cp`: its style and
    /// property list.
    fn papx(&self, cp: u32) -> (u16, Vec<u8>) {
        let fc = self.text.fcs.get(cp as usize).copied().unwrap_or(0);
        run_at(&self.paras, fc).map_or((0, Vec::new()), |run| (run.istd, run.grpprl.clone()))
    }

    /// The paragraphs and tables of the characters `start..end`.
    fn blocks(&mut self, start: u32, end: u32) -> Vec<Block> {
        let end = end.min(self.text.units.len() as u32);
        let mut blocks = Vec::new();
        // The tables being read, one per depth of nesting.
        let mut tables: Vec<TableBuild> = Vec::new();
        let mut cp = start;
        self.fields.clear();
        while cp < end {
            let mark = (cp..end)
                .find(|&at| matches!(self.text.units[at as usize], PARAGRAPH_END | CELL_END))
                .unwrap_or(end.saturating_sub(1).max(cp));
            let terminator = self.text.units.get(mark as usize).copied().unwrap_or(PARAGRAPH_END);
            let (istd, grpprl) = self.papx(mark);
            if std::env::var_os("KAKEMONO_DEBUG_DOC").is_some() {
                let first: String = (cp..mark.min(cp + 30))
                    .map(|at| char::from_u32(u32::from(self.text.units[at as usize])).unwrap_or('?'))
                    .collect();
                eprintln!("PARA {istd} {first:?} {}", debug_sprms(&grpprl));
            }
            let flags = table_flags(&grpprl);
            let depth = flags.depth as usize;
            close_tables(&mut tables, depth, &mut blocks);
            tables.resize_with(depth, TableBuild::default);
            if let Some(table) = tables.last_mut()
                && flags.row_end
            {
                // The row end: its properties describe the row.
                if std::env::var_os("KAKEMONO_DEBUG_DOC").is_some() {
                    eprintln!("ROW {}", debug_sprms(&grpprl));
                }
                table.rows.push((std::mem::take(&mut table.cells), row_props(&grpprl)));
                table.cell.clear();
                cp = mark + 1;
                continue;
            }
            let para = self.paragraph(cp, mark, istd, &grpprl, depth >= 1);
            cp = mark + 1;
            match tables.last_mut() {
                None => blocks.push(Block::Para(Box::new(para))),
                Some(table) => {
                    table.cell.push(Block::Para(Box::new(para)));
                    // A cell of the outer table ends with a cell mark; one of
                    // a nested table, with a paragraph mark flagged as such.
                    if (depth == 1 && terminator == CELL_END) || (depth > 1 && flags.inner_cell_end) {
                        table.cells.push(std::mem::take(&mut table.cell));
                    }
                }
            }
        }
        close_tables(&mut tables, 0, &mut blocks);
        blocks
    }

    /// The list number or bullet of a paragraph, from its own list (in
    /// `direct`) or else its style's (`style_para`): the list level's
    /// properties are laid into `para_raw` (beneath the style's when the
    /// list comes from the style). `None` when the paragraph is in no list,
    /// its mark is hidden or its number is empty.
    fn list_number(
        &mut self,
        istd: u16,
        style_para: &RawPara,
        direct: &RawPara,
        para_raw: &mut RawPara,
        mark_raw: &RawRun,
    ) -> Option<NumberMark> {
        let mut numbering = style_para.numbering.clone();
        if let Some((id, level)) = &direct.numbering {
            let (old_id, old_level) = numbering.take().unwrap_or_default();
            let id = if id.is_empty() { old_id } else { id.clone() };
            numbering = Some((id, level.or(old_level)));
        }
        let from_style = direct.numbering.is_none();
        let mut number = None;
        if let Some((num_id, level)) = numbering.filter(|(id, _)| !id.is_empty() && id != "0") {
            let level = level.unwrap_or(0);
            if let Some(def) = self.numbering.level(&num_id, level, &self.styles) {
                // A list set by the paragraph's style is beneath the style
                // but above Normal: the indents of the style (and of its
                // base styles) win over the level's, Normal's do not.
                if from_style {
                    let mut level = def.para.clone();
                    // The style's tab stops replace the level's.
                    level.tabs.clear();
                    let (inherited, own) = self.style_layers.get(&istd).cloned().unwrap_or_default();
                    let mut under = inherited;
                    under.overlay(&level);
                    under.overlay(&own);
                    *para_raw = under;
                } else {
                    para_raw.overlay(&def.para);
                }
                if mark_raw.hidden != Some(true)
                    && let Some((def, text)) = self.numbering.next(&num_id, level, &self.styles)
                {
                    let mut fmt_raw = mark_raw.clone();
                    fmt_raw.underline = None;
                    fmt_raw.overlay(&def.run);
                    let mut fmt = fmt_raw.resolve();
                    let text = super::docx::symbol_text(&text, &mut fmt);
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
        number
    }

    /// A paragraph: its resolved properties and content.
    fn paragraph(&mut self, start: u32, mark: u32, istd: u16, grpprl: &[u8], in_table: bool) -> Para {
        let _ = in_table;
        let (style_para, style_run) = self.para_styles.get(&istd).cloned().unwrap_or_default();
        let mut base_run = self.defaults.clone();
        base_run.overlay(&style_run);
        let mut direct = RawPara::default();
        apply_para(grpprl, &mut direct);
        let mut para_raw = style_para.clone();
        // The paragraph mark's own character properties.
        let mark_fc = self.text.fcs.get(mark as usize).copied().unwrap_or(0);
        let mark_chpx = run_at(&self.chars, mark_fc)
            .map(|run| run.grpprl.clone())
            .unwrap_or_default();
        if std::env::var_os("KAKEMONO_DEBUG_DOC").is_some() {
            eprintln!("MARK {istd} {}", debug_sprms(&mark_chpx));
        }
        let mut mark_raw = base_run.clone();
        let mark_base = mark_raw.clone();
        apply_chars(&mark_chpx, &mut mark_raw, &mark_base, self.fonts);
        let number = self.list_number(istd, &style_para, &direct, &mut para_raw, &mark_raw);
        para_raw.overlay(&direct);
        let mut para = Para {
            style: istd.to_string(),
            props: para_raw.resolve(),
            mark: mark_raw.resolve(),
            number,
            inlines: Vec::new(),
            bookmarks: Vec::new(),
        };
        let mut text: Vec<u16> = Vec::new();
        let mut current: Option<RunFmt> = None;
        for cp in start..mark {
            if let Some(names) = self.bookmarks.get(&cp) {
                para.bookmarks.extend(names.iter().cloned());
            }
            let unit = self.text.units[cp as usize];
            let fc = self.text.fcs[cp as usize];
            let chpx = run_at(&self.chars, fc)
                .map(|run| run.grpprl.clone())
                .unwrap_or_default();
            let fmt = self.run_fmt(&base_run, &chpx);
            if unit == PAGE_BREAK && std::env::var_os("KAKEMONO_DEBUG_DOC").is_some() {
                eprintln!(
                    "BREAK hidden={} fields={:?} chpx={}",
                    fmt.hidden,
                    self.fields,
                    debug_sprms(&chpx)
                );
            }
            if self.field_char(unit, &mut text, current.as_ref(), &fmt, &mut para) {
                continue;
            }
            if fmt.hidden {
                continue;
            }
            if current.as_ref() != Some(&fmt) {
                flush(&mut text, current.as_ref(), &mut para);
                current = Some(fmt.clone());
            }
            self.character(cp, unit, &chpx, &mut text, &fmt, &mut para);
        }
        flush(&mut text, current.as_ref(), &mut para);
        self.put_shapes(&mut para);
        para
    }

    /// The resolved properties of a character with property list `chpx` in
    /// a paragraph whose style gives `base`.
    fn run_fmt(&self, base: &RawRun, chpx: &[u8]) -> RunFmt {
        let mut raw = base.clone();
        if let Some(istd) = char_style(chpx)
            && let Some(style) = self.char_styles.get(&istd)
        {
            raw.overlay(style);
        }
        let inherited = raw.clone();
        apply_chars(chpx, &mut raw, &inherited, self.fonts);
        let mut fmt = raw.resolve();
        fmt.href = self.fields.iter().rev().find_map(|field| field.href.clone());
        fmt
    }

    /// Handles a field character; returns whether `unit` was one.
    fn field_char(
        &mut self,
        unit: u16,
        text: &mut Vec<u16>,
        current: Option<&RunFmt>,
        fmt: &RunFmt,
        para: &mut Para,
    ) -> bool {
        match unit {
            FIELD_BEGIN => {
                flush(text, current, para);
                self.fields.push(FieldState::default());
                true
            }
            FIELD_SEPARATE => {
                flush(text, current, para);
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
                true
            }
            FIELD_END => {
                flush(text, current, para);
                if let Some(field) = self.fields.pop()
                    && !field.in_result
                    && let Some((kind, format)) = layout_field(&field.instr)
                {
                    para.inlines.push(Inline::Field {
                        kind,
                        fmt: fmt.clone(),
                        format,
                    });
                }
                true
            }
            _ if self.fields.iter().any(|field| !field.in_result) => {
                if let Some(field) = self.fields.last_mut()
                    && !field.in_result
                {
                    let ch = if unit < 0x20 {
                        String::from(" ")
                    } else {
                        String::from_utf16_lossy(&[unit])
                    };
                    field.instr.push_str(&ch);
                }
                true
            }
            _ if self.fields.iter().any(|field| field.skip_result) => true,
            _ => false,
        }
    }

    /// Adds the character `unit` at `cp` to the paragraph.
    fn character(&mut self, cp: u32, unit: u16, chpx: &[u8], text: &mut Vec<u16>, fmt: &RunFmt, para: &mut Para) {
        let (special, picture_at, symbol) = char_extras(chpx);
        if unit == SHAPE_ANCHOR && special {
            // Shapes anchored together wait to be put in drawing order.
            if let Some(shape) = self.float_shape(cp) {
                flush(text, Some(fmt), para);
                self.pending_shapes.push(shape);
            }
            return;
        }
        self.put_shapes(para);
        let push = |inline: Inline, text: &mut Vec<u16>, para: &mut Para| {
            flush(text, Some(fmt), para);
            para.inlines.push(inline);
        };
        match unit {
            PICTURE if special => {
                if let Some(location) = picture_at {
                    let inline = self.picture(location, fmt);
                    push(inline, text, para);
                }
            }
            NOTE_MARK if special => {
                if let Some((endnote, id)) = self.note_refs.get(&cp).cloned() {
                    push(
                        Inline::NoteRef {
                            endnote,
                            id,
                            fmt: fmt.clone(),
                        },
                        text,
                        para,
                    );
                } else {
                    push(Inline::NoteNumber { fmt: fmt.clone() }, text, para);
                }
            }
            SYMBOL if special => {
                if let Some(inline) = self.symbol(symbol, fmt) {
                    push(inline, text, para);
                }
            }
            LINE_BREAK => push(
                Inline::Break {
                    kind: BreakKind::Line,
                    fmt: fmt.clone(),
                },
                text,
                para,
            ),
            PAGE_BREAK => push(
                Inline::Break {
                    kind: BreakKind::Page,
                    fmt: fmt.clone(),
                },
                text,
                para,
            ),
            COLUMN_BREAK => push(
                Inline::Break {
                    kind: BreakKind::Column,
                    fmt: fmt.clone(),
                },
                text,
                para,
            ),
            TAB => push(Inline::Tab { fmt: fmt.clone() }, text, para),
            NON_BREAKING_HYPHEN => text.push(0x2011),
            SOFT_HYPHEN => text.push(0x00AD),
            unit if unit < 0x20 => {}
            unit => text.push(unit),
        }
    }

    /// Reads the text of the text boxes of the main text.
    fn read_text_boxes(&mut self, fib: &Fib, table: &[u8]) {
        let story = fib.ccp_text + fib.ccp_ftn + fib.ccp_hdd + fib.ccp_atn + fib.ccp_edn;
        for (spid, start, end) in drawing::text_boxes(fib.slice(table, Pair::PlcftxbxTxt)) {
            if start >= end || !self.floats.values().any(|float| float.spid == spid) {
                continue;
            }
            let mut blocks = self.blocks(story + start, story + end);
            // The paragraph mark ending the story is not a paragraph of
            // the text box.
            if blocks.len() > 1
                && let Some(Block::Para(last)) = blocks.last()
                && last.inlines.is_empty()
            {
                blocks.pop();
            }
            self.text_boxes.insert(spid, blocks);
        }
    }

    /// The character of a symbol font (`sprmCSymbol`): its font and code.
    fn symbol(&self, symbol: Option<(u16, u16)>, fmt: &RunFmt) -> Option<Inline> {
        let (font, code) = symbol?;
        let mut sym_fmt = fmt.clone();
        if let Some(name) = self.fonts.get(usize::from(font)) {
            sym_fmt.font = name.clone();
            sym_fmt.font_hansi = name.clone();
        }
        let raw: String = char::from_u32(u32::from(code)).into_iter().collect();
        let shown = super::docx::symbol_text(&raw, &mut sym_fmt);
        Some(Inline::Text {
            text: shown,
            fmt: sym_fmt,
        })
    }

    /// Puts the waiting floating shapes in the paragraph, in their drawing
    /// order.
    fn put_shapes(&mut self, para: &mut Para) {
        if self.pending_shapes.is_empty() {
            return;
        }
        let mut shapes = std::mem::take(&mut self.pending_shapes);
        shapes.sort_by_key(|(order, _)| *order);
        para.inlines
            .extend(shapes.into_iter().map(|(_, shape)| Inline::Shape(Box::new(shape))));
    }

    /// The floating shape anchored at `cp`, with its text or picture, and
    /// its place in the drawing order.
    fn float_shape(&mut self, cp: u32) -> Option<(usize, Shape)> {
        let float = self.floats.get(&cp)?.clone();
        let blocks = self.text_boxes.get(&float.spid).cloned().unwrap_or_default();
        let (width, height) = float.size;
        let mut shape = Shape {
            width,
            height,
            anchor: Some(float.anchor),
            fill: float.fill,
            line: float.line,
            blocks,
            insets: float.insets,
            pictures: Vec::new(),
            auto_height: false,
            fmt: RunFmt::default(),
        };
        if let Some(picture) = float.picture.and_then(normalize) {
            self.images.push(FImage {
                data: picture.data,
                jpeg: picture.jpeg,
            });
            shape.pictures.push((self.images.len() - 1, 0.0, 0.0, width, height));
            // A picture has no fill, nor an outline unless one is set.
            shape.fill = None;
            if !float.line_set {
                shape.line = None;
            }
        }
        Some((float.order, shape))
    }

    /// The picture at `location` of the `Data` stream.
    fn picture(&mut self, location: u32, fmt: &RunFmt) -> Inline {
        let (file, (width, height)) = picture::picture(self.data, location);
        let index = file.and_then(normalize).map(|picture| {
            self.images.push(FImage {
                data: picture.data,
                jpeg: picture.jpeg,
            });
            self.images.len() - 1
        });
        Inline::Image {
            index,
            width,
            height,
            anchor: None,
            fmt: fmt.clone(),
            alt: String::new(),
            crop: [0.0; 4],
        }
    }
}

/// A property list as text, for debugging.
fn debug_sprms(grpprl: &[u8]) -> String {
    let list: Vec<String> = crate::word::doc::props::sprms(grpprl)
        .map(|(sprm, operand)| {
            let bytes: String = operand.iter().take(12).map(|byte| format!("{byte:02x}")).collect();
            format!("{sprm:04X}:{bytes}")
        })
        .collect();
    list.join(" ")
}

/// What the table properties of a paragraph say of its place in tables.
#[derive(Default)]
struct TableFlags {
    /// Depth of the table the paragraph is in (0 outside tables).
    depth: u32,
    /// Whether it ends a row of its table.
    row_end: bool,
    /// Whether it ends a cell of a nested table.
    inner_cell_end: bool,
}

/// The place in tables of a paragraph with the properties `grpprl`.
fn table_flags(grpprl: &[u8]) -> TableFlags {
    let mut flags = TableFlags::default();
    let (mut ttp, mut inner_ttp) = (false, false);
    for (sprm, operand) in crate::word::doc::props::sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        match sprm {
            0x2416 => flags.depth = flags.depth.max(u32::from(byte != 0)),
            0x6649 => flags.depth = u32_at(operand, 0).unwrap_or(0),
            0x2417 => ttp = byte != 0,
            0x244B => flags.inner_cell_end = byte != 0,
            0x244C => inner_ttp = byte != 0,
            _ => {}
        }
    }
    flags.row_end = if flags.depth > 1 { inner_ttp } else { ttp } && flags.depth >= 1;
    flags
}

/// A table being read: its finished rows, the cells of the current row and
/// the blocks of the current cell.
#[derive(Default)]
struct TableBuild {
    /// Finished rows, with their properties.
    rows: Vec<(Vec<Vec<Block>>, RowProps)>,
    /// Finished cells of the current row.
    cells: Vec<Vec<Block>>,
    /// Blocks of the current cell.
    cell: Vec<Block>,
}

/// Closes the tables nested deeper than `depth`, each put in the cell of
/// the table around it, or in `blocks` for the outermost.
fn close_tables(tables: &mut Vec<TableBuild>, depth: usize, blocks: &mut Vec<Block>) {
    while tables.len() > depth {
        let Some(done) = tables.pop() else { break };
        if done.rows.is_empty() {
            // Text never closed into a row: keep it.
            let target = tables.last_mut().map_or(&mut *blocks, |table| &mut table.cell);
            target.extend(done.cells.into_iter().flatten().chain(done.cell));
            continue;
        }
        let table = Block::Table(build_table(done.rows));
        match tables.last_mut() {
            Some(outer) => outer.cell.push(table),
            None => blocks.push(table),
        }
    }
}

/// A table from its rows: the grid from the cell edges of every row, the
/// cells with their spans, merges, borders, shading and margins.
fn build_table(rows: Vec<(Vec<Vec<Block>>, RowProps)>) -> Table {
    let mut lines: Vec<f32> = rows.iter().flat_map(|(_, props)| props.edges.iter().copied()).collect();
    lines.sort_by(f32::total_cmp);
    lines.dedup_by(|a, b| (*a - *b).abs() <= GRID_TOLERANCE_PT);
    let column = |edge: f32| lines.iter().position(|line| (line - edge).abs() <= GRID_TOLERANCE_PT);
    let grid: Vec<f32> = lines.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let first = rows.first().map(|(_, props)| props.clone()).unwrap_or_default();
    let left_edge = lines.first().copied().unwrap_or(0.0);
    let mut table = Table {
        props: table_props(&first, left_edge),
        grid,
        rows: Vec::new(),
    };
    for (cells, props) in rows {
        let mut row = Row {
            height: props.height,
            header: props.header,
            cant_split: props.cant_split,
            grid_before: props.edges.first().and_then(|&edge| column(edge)).unwrap_or(0),
            grid_after: 0,
            cells: Vec::new(),
        };
        for (at, blocks) in cells.into_iter().enumerate() {
            let (merged, vert, restart) = props.merges.get(at).copied().unwrap_or_default();
            if merged && let Some(previous) = row.cells.last_mut() {
                previous.span += 1;
                previous.blocks.extend(blocks);
                continue;
            }
            let left = props.edges.get(at).copied().unwrap_or(0.0);
            let right = props.edges.get(at + 1).copied().unwrap_or(left);
            let span = match (column(left), column(right)) {
                (Some(first), Some(end)) if end > first => end - first,
                _ => 1,
            };
            let own = props.cell_borders.get(at).copied().unwrap_or_default();
            let tb = props.table_borders;
            let last = at + 1 == props.edges.len().saturating_sub(1);
            let borders = [
                own[0].unwrap_or(tb[0]),
                own[1].unwrap_or(if at == 0 { tb[1] } else { tb[5] }),
                own[2].unwrap_or(tb[4].or(tb[2])),
                own[3].unwrap_or(if last { tb[3] } else { tb[5] }),
            ];
            row.cells.push(Cell {
                span,
                vmerge: match (vert, restart) {
                    (true, true) => VMerge::Restart,
                    (true, false) => VMerge::Continue,
                    _ => VMerge::None,
                },
                width: Width::Pt(right - left),
                borders,
                shading: props.shading.get(at).copied().flatten(),
                margins: [
                    cell_margin(&props, at, 0),
                    cell_margin(&props, at, 1),
                    cell_margin(&props, at, 2),
                    cell_margin(&props, at, 3),
                ],
                valign: props.valign.get(at).copied().unwrap_or_default(),
                blocks,
            });
        }
        table.rows.push(row);
    }
    table
}

/// The properties of a table whose first row has `first` and whose grid
/// starts at `left_edge`.
fn table_props(first: &RowProps, left_edge: f32) -> TableProps {
    let first_left = cell_margin(first, 0, 0);
    TableProps {
        width: Width::Auto,
        // The text of the first cell starts at the edge plus the cell's
        // margin (the layout takes the margin off for older documents).
        indent: left_edge + first_left,
        align: first.align,
        fixed: true,
        cell_margins: [
            first.margins[0].unwrap_or(first.gap_half),
            first.margins[1].unwrap_or(0.0),
            first.margins[2].unwrap_or(first.gap_half),
            first.margins[3].unwrap_or(0.0),
        ],
        borders: first.table_borders,
        cell_spacing: 0.0,
        float: None,
    }
}

/// The margin of cell `at` of a row on `side` (left, top, right, bottom):
/// the cell's own, else the row's, else half the gap between cells (left and
/// right) or unset (top and bottom).
fn cell_margin(row: &RowProps, at: usize, side: usize) -> f32 {
    row.cell_margins
        .get(at)
        .and_then(|margins| margins[side])
        .or(row.margins[side])
        .unwrap_or(if side.is_multiple_of(2) { row.gap_half } else { f32::NAN })
}

/// Adds the characters `text` as an [`Inline::Text`], and empties them.
fn flush(text: &mut Vec<u16>, fmt: Option<&RunFmt>, para: &mut Para) {
    if text.is_empty() {
        return;
    }
    let shown = String::from_utf16_lossy(text);
    text.clear();
    let mut fmt = fmt.cloned().unwrap_or_default();
    let shown = super::docx::symbol_text(&shown, &mut fmt);
    para.inlines.push(Inline::Text { text: shown, fmt });
}

/// The layout field of an instruction (page number, page count).
fn layout_field(instr: &str) -> Option<(FieldKind, Option<NumFormat>)> {
    super::docx::layout_field(instr)
}

/// The UTF-16 text of `bytes` (kept for symmetry with the `.docx` reader).
#[allow(dead_code)]
fn text_of(bytes: &[u8]) -> String {
    utf16(bytes)
}
