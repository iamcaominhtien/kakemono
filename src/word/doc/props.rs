//! Properties of a `.doc`: the property modifiers (sprms) of characters,
//! paragraphs and table rows, the pages they are stored in (FKP), and the
//! style sheet, fonts, lists and bookmarks.

use super::fib::{i16_at, plc, u16_at, u32_at};
use crate::word::styles::{Level, Numbering, RunProps, StyleDef, Styles};

/// Size of a property page (FKP) in the `WordDocument` stream.
const FKP_LEN: usize = 512;
/// Size of a `BxPap` (paragraph page entry).
const BX_PAP_LEN: usize = 13;
/// Size of a list definition (`LSTF`).
const LSTF_LEN: usize = 28;
/// Size of the fixed part of a list level (`LVLF`).
const LVLF_LEN: usize = 28;
/// Size of a list override (`LFO`).
const LFO_LEN: usize = 16;
/// Levels of a list that is not a simple list.
const LIST_LEVELS: usize = 9;
/// `istdBase` of a style based on no other.
const NO_BASE: u16 = 0x0FFF;
/// Offset of the font name in a font record (`FFN`).
const FFN_NAME_OFFSET: usize = 39;
/// Default font size of Word 97 documents, in half points.
const DEFAULT_HALF_POINTS: u16 = 20;
/// Font sizes are in half points.
const HALF_POINTS_PER_PT: f32 = 2.0;

/// Character sprm: bold.
const SPRM_C_BOLD: u16 = 0x0835;
/// Character sprm: italic.
const SPRM_C_ITALIC: u16 = 0x0836;
/// Character sprm: hidden text.
const SPRM_C_VANISH: u16 = 0x083C;
/// Character sprm: text deleted with change tracking (read as hidden: the
/// document is read with its changes accepted).
const SPRM_C_RMARK_DEL: u16 = 0x0800;
/// Character sprm: the character is special (picture, note mark, ...).
const SPRM_C_SPEC: u16 = 0x0855;
/// Character sprm: superscript or subscript.
const SPRM_C_ISS: u16 = 0x2A48;
/// Character sprm: colour index.
const SPRM_C_ICO: u16 = 0x2A42;
/// Character sprm: RGB colour.
const SPRM_C_CV: u16 = 0x6870;
/// Character sprm: the font of ASCII text.
const SPRM_C_FTC0: u16 = 0x4A4F;
/// Character sprm: the font of text (Word 6 form, still written).
const SPRM_C_FTC: u16 = 0x4A3D;
/// Character sprm: font size in half points.
const SPRM_C_HPS: u16 = 0x4A43;
/// Character sprm: where a picture is in the `Data` stream.
const SPRM_C_PIC_LOCATION: u16 = 0x6A03;
/// The picture of a special character is an embedded OLE object, stored
/// in the `ObjectPool` storage under the picture location.
const SPRM_C_FOLE2: u16 = 0x080A;
/// Character sprm: a symbol character and its font.
const SPRM_C_SYMBOL: u16 = 0x6A09;
/// Character sprm: the character style.
const SPRM_C_ISTD: u16 = 0x4A30;
/// Paragraph sprm: the list level.
const SPRM_P_ILVL: u16 = 0x260A;
/// Paragraph sprm: the list override (1-based).
const SPRM_P_ILFO: u16 = 0x460B;
/// Paragraph sprm: the outline level.
const SPRM_P_OUT_LVL: u16 = 0x2640;
/// Paragraph sprm: the paragraph is in a table.
const SPRM_P_IN_TABLE: u16 = 0x2416;
/// Paragraph sprm: the paragraph ends a table row.
const SPRM_P_TTP: u16 = 0x2417;
/// Paragraph sprm: the depth of the table the paragraph is in.
const SPRM_P_ITAP: u16 = 0x6649;
/// Paragraph sprm: the paragraph ends a row of a nested table.
const SPRM_P_INNER_TTP: u16 = 0x244C;
/// Table sprm: the cells of a row (`TDefTable`).
const SPRM_T_DEF_TABLE: u16 = 0xD608;
/// Table sprm: the row repeats as a header.
const SPRM_T_HEADER: u16 = 0x3404;
/// Table sprm: vertical merging of a cell.
const SPRM_T_VERT_MERGE: u16 = 0xD62B;
/// Paragraph sprm whose variable operand has a one-byte size that may be
/// 255 (tab changes).
const SPRM_P_CHG_TABS: u16 = 0xC615;
/// Toggle operand: the value of the style.
const TOGGLE_LIKE_STYLE: u8 = 0x80;
/// Toggle operand: the opposite of the value of the style.
const TOGGLE_NOT_STYLE: u8 = 0x81;
/// Colours of the colour indexes (`ico`), from 1 (black) to 16.
const ICO_COLORS: [[u8; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 255],
    [0, 255, 255],
    [0, 255, 0],
    [255, 0, 255],
    [255, 0, 0],
    [255, 255, 0],
    [255, 255, 255],
    [0, 0, 128],
    [0, 128, 128],
    [0, 128, 0],
    [128, 0, 128],
    [128, 0, 0],
    [128, 128, 0],
    [128, 128, 128],
    [192, 192, 192],
];
/// `TC80` flag: the cell is merged into the cell before it.
const TC_MERGED: u16 = 0x0002;
/// `TC80` flag: the cell is part of a vertically merged column.
const TC_VERT_MERGE: u16 = 0x0020;
/// `TC80` flag: the cell starts a vertically merged column.
const TC_VERT_RESTART: u16 = 0x0040;
/// Size of a `TC80` cell description.
const TC80_LEN: usize = 20;

/// The sprms of a property list (`grpprl`), each with its operand.
pub(crate) fn sprms(grpprl: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut pos = 0;
    std::iter::from_fn(move || {
        let sprm = u16_at(grpprl, pos)?;
        let rest = grpprl.get(pos + 2..)?;
        let len = operand_len(sprm, rest)?;
        let operand = rest.get(..len)?;
        pos += 2 + len;
        Some((sprm, operand))
    })
}

/// The size of the operand of `sprm`, whose bytes start `rest`.
fn operand_len(sprm: u16, rest: &[u8]) -> Option<usize> {
    Some(match sprm >> 13 {
        0 | 1 => 1,
        2 | 4 | 5 => 2,
        3 => 4,
        7 => 3,
        _ if sprm == SPRM_T_DEF_TABLE || sprm == 0xD606 => usize::from(u16_at(rest, 0)?) + 1,
        _ if sprm == SPRM_P_CHG_TABS && *rest.first()? == 255 => {
            let deleted = usize::from(*rest.get(1)?);
            let added_at = 2 + deleted * 4;
            let added = usize::from(*rest.get(added_at)?);
            added_at + 1 + added * 3
        }
        _ => usize::from(*rest.first()?) + 1,
    })
}

/// A run of bytes of the text with the same properties.
#[derive(Debug, Clone)]
pub(crate) struct Run {
    /// First byte (FC).
    pub(crate) start: u32,
    /// End.
    pub(crate) end: u32,
    /// Paragraph style (paragraph runs only).
    pub(crate) istd: u16,
    /// The property list.
    pub(crate) grpprl: Vec<u8>,
}

/// The runs of a property page table (`PlcBteChpx` or `PlcBtePapx`),
/// sorted by their first byte.
pub(crate) fn runs(word: &[u8], plc_bte: &[u8], paragraphs: bool) -> Vec<Run> {
    let (_, pages) = plc(plc_bte, 4);
    let mut out = Vec::new();
    for page in pages {
        let number = u32_at(page, 0).unwrap_or(0) & 0x003F_FFFF;
        let offset = number as usize * FKP_LEN;
        let Some(fkp) = word.get(offset..offset + FKP_LEN) else {
            continue;
        };
        let count = usize::from(fkp[FKP_LEN - 1]);
        for index in 0..count {
            let (Some(start), Some(end)) = (u32_at(fkp, index * 4), u32_at(fkp, index * 4 + 4)) else {
                break;
            };
            let entries = (count + 1) * 4;
            let (istd, grpprl) = if paragraphs {
                papx(
                    fkp,
                    usize::from(fkp.get(entries + index * BX_PAP_LEN).copied().unwrap_or(0)),
                )
            } else {
                (
                    0,
                    chpx(fkp, usize::from(fkp.get(entries + index).copied().unwrap_or(0))),
                )
            };
            out.push(Run {
                start,
                end,
                istd,
                grpprl,
            });
        }
    }
    out.sort_by_key(|run| run.start);
    out
}

/// The paragraph runs `runs` with the property lists kept in the `Data`
/// stream (`sprmPHugePapx`, for lists too long for an FKP) added to theirs.
pub(crate) fn with_huge_papx(mut runs: Vec<Run>, data: &[u8]) -> Vec<Run> {
    const SPRM_P_HUGE_PAPX: u16 = 0x6646;
    for run in &mut runs {
        let offset = sprms(&run.grpprl)
            .find(|(sprm, _)| *sprm == SPRM_P_HUGE_PAPX)
            .and_then(|(_, operand)| u32_at(operand, 0));
        if let Some(offset) = offset {
            let start = offset as usize;
            let len = usize::from(u16_at(data, start).unwrap_or(0));
            if let Some(huge) = data.get(start + 2..start + 2 + len) {
                run.grpprl.extend_from_slice(huge);
            }
        }
    }
    runs
}

/// The run that holds the byte `fc`.
pub(crate) fn run_at(runs: &[Run], fc: u32) -> Option<&Run> {
    let index = runs.partition_point(|run| run.start <= fc).checked_sub(1)?;
    runs.get(index).filter(|run| fc < run.end)
}

/// The style and property list of a paragraph entry of an FKP at word
/// offset `word_offset` (0: no properties).
fn papx(fkp: &[u8], word_offset: usize) -> (u16, Vec<u8>) {
    if word_offset == 0 {
        return (0, Vec::new());
    }
    let pos = word_offset * 2;
    let Some(&cb) = fkp.get(pos) else {
        return (0, Vec::new());
    };
    let (start, len) = if cb == 0 {
        (pos + 2, usize::from(fkp.get(pos + 1).copied().unwrap_or(0)) * 2)
    } else {
        (pos + 1, usize::from(cb) * 2 - 1)
    };
    let Some(bytes) = fkp.get(start..(start + len).min(FKP_LEN)) else {
        return (0, Vec::new());
    };
    let istd = u16_at(bytes, 0).unwrap_or(0);
    (istd, bytes.get(2..).unwrap_or(&[]).to_vec())
}

/// The property list of a character entry of an FKP at word offset
/// `word_offset` (0: no properties).
fn chpx(fkp: &[u8], word_offset: usize) -> Vec<u8> {
    if word_offset == 0 {
        return Vec::new();
    }
    let pos = word_offset * 2;
    let len = usize::from(fkp.get(pos).copied().unwrap_or(0));
    fkp.get(pos + 1..(pos + 1 + len).min(FKP_LEN)).unwrap_or(&[]).to_vec()
}

/// The properties of a character run that are not text style.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CharExtras {
    /// The character is special (a picture, a note mark, a drawing).
    pub(crate) special: bool,
    /// Where its picture is in the `Data` stream.
    pub(crate) picture: Option<u32>,
    /// A symbol character: its font and character code.
    pub(crate) symbol: Option<(u16, u16)>,
    /// The picture is that of an embedded OLE object.
    pub(crate) ole: bool,
}

/// The style of a character run: its character style, its text
/// properties over those of its styles, and its other properties.
/// `style_props` are the properties its styles give (for the toggles that
/// say "the opposite of the style").
pub(crate) fn char_props(grpprl: &[u8], style_props: RunProps, fonts: &[String]) -> (RunProps, CharExtras) {
    let mut props = RunProps::default();
    let mut extras = CharExtras::default();
    for (sprm, operand) in sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        match sprm {
            SPRM_C_BOLD => props.bold = toggle(byte, style_props.bold),
            SPRM_C_ITALIC => props.italic = toggle(byte, style_props.italic),
            SPRM_C_VANISH => props.hidden = toggle(byte, style_props.hidden),
            SPRM_C_RMARK_DEL if byte == 1 => props.hidden = Some(true),
            SPRM_C_ISS => {
                props.raised = Some(match byte {
                    1 => Some(true),
                    2 => Some(false),
                    _ => None,
                });
            }
            SPRM_C_ICO => props.color = ico_color(byte),
            SPRM_C_CV => {
                props.color = (operand.get(3) == Some(&0)).then(|| [byte, operand[1], operand[2]]);
            }
            SPRM_C_FTC0 | SPRM_C_FTC => {
                if let Some(name) = u16_at(operand, 0).and_then(|index| fonts.get(usize::from(index))) {
                    props.mono = Some(crate::word::styles::is_mono_font(name));
                    props.symbol = Some(name.eq_ignore_ascii_case("Symbol"));
                }
            }
            SPRM_C_SPEC => extras.special = byte != 0,
            SPRM_C_PIC_LOCATION => extras.picture = u32_at(operand, 0),
            SPRM_C_FOLE2 => extras.ole = operand.first().is_some_and(|&flag| flag != 0),
            SPRM_C_SYMBOL => extras.symbol = Some((u16_at(operand, 0).unwrap_or(0), u16_at(operand, 2).unwrap_or(0))),
            _ => {}
        }
    }
    (props, extras)
}

/// The character style named in a character property list.
pub(crate) fn char_style(grpprl: &[u8]) -> Option<u16> {
    sprms(grpprl)
        .find(|(sprm, _)| *sprm == SPRM_C_ISTD)
        .and_then(|(_, operand)| u16_at(operand, 0))
}

/// A toggle property: on, off, as the style, or the opposite of the style.
fn toggle(operand: u8, style: Option<bool>) -> Option<bool> {
    match operand {
        0 => Some(false),
        1 => Some(true),
        TOGGLE_LIKE_STYLE => None,
        TOGGLE_NOT_STYLE => Some(!style.unwrap_or(false)),
        _ => None,
    }
}

/// The colour of a colour index; `None` for automatic (0).
fn ico_color(ico: u8) -> Option<[u8; 3]> {
    ICO_COLORS.get(usize::from(ico).checked_sub(1)?).copied()
}

/// What the reader uses of the properties of a paragraph.
#[derive(Debug, Default, Clone)]
pub(crate) struct ParaProps {
    /// Its style.
    pub(crate) istd: u16,
    /// The list override it is numbered by (1-based; 0 for none).
    pub(crate) ilfo: u16,
    /// Its list level.
    pub(crate) ilvl: u8,
    /// Its outline level.
    pub(crate) outline: Option<u8>,
    /// Depth of the table it is in (0 when it is in none).
    pub(crate) table_depth: u32,
    /// It ends a row of a table (of depth 1).
    pub(crate) row_end: bool,
    /// It ends a row of a nested table.
    pub(crate) inner_row_end: bool,
    /// The row is a header row (on a row end).
    pub(crate) header: bool,
    /// The cells of the row (on a row end).
    pub(crate) cells: Vec<CellDef>,
}

/// A cell of a table row, as the row end describes it.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct CellDef {
    /// Left edge, in twips.
    pub(crate) left: i16,
    /// Right edge, in twips.
    pub(crate) right: i16,
    /// It is merged into the cell before it.
    pub(crate) merged: bool,
    /// It is part of a vertically merged column of cells.
    pub(crate) vert_merge: bool,
    /// It starts a vertically merged column of cells.
    pub(crate) vert_restart: bool,
}

/// The properties of a paragraph of style `istd` with property list
/// `grpprl`.
pub(crate) fn para_props(istd: u16, grpprl: &[u8]) -> ParaProps {
    let mut props = ParaProps {
        istd,
        ..Default::default()
    };
    for (sprm, operand) in sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        match sprm {
            SPRM_P_ILVL => props.ilvl = byte,
            SPRM_P_ILFO => props.ilfo = u16_at(operand, 0).unwrap_or(0),
            SPRM_P_OUT_LVL => props.outline = (byte < 9).then_some(byte),
            SPRM_P_IN_TABLE => props.table_depth = props.table_depth.max(u32::from(byte != 0)),
            SPRM_P_ITAP => props.table_depth = u32_at(operand, 0).unwrap_or(0),
            SPRM_P_TTP => props.row_end = byte != 0,
            SPRM_P_INNER_TTP => props.inner_row_end = byte != 0,
            SPRM_T_HEADER => props.header = byte != 0,
            SPRM_T_DEF_TABLE => props.cells = def_table(operand),
            SPRM_T_VERT_MERGE => {
                let (Some(&cell), Some(&flags)) = (operand.get(1), operand.get(2)) else {
                    continue;
                };
                if let Some(def) = props.cells.get_mut(usize::from(cell)) {
                    def.vert_merge = flags != 0;
                    def.vert_restart = flags == 3;
                }
            }
            _ => {}
        }
    }
    props
}

/// The cells of a `TDefTable` operand: their edges and merges.
fn def_table(operand: &[u8]) -> Vec<CellDef> {
    let body = operand.get(2..).unwrap_or(&[]);
    let count = usize::from(body.first().copied().unwrap_or(0));
    let edges: Vec<i16> = (0..=count).filter_map(|index| i16_at(body, 1 + index * 2)).collect();
    let tc_start = 1 + (count + 1) * 2;
    (0..count)
        .map(|index| {
            let flags = u16_at(body, tc_start + index * TC80_LEN).unwrap_or(0);
            CellDef {
                left: edges.get(index).copied().unwrap_or(0),
                right: edges.get(index + 1).copied().unwrap_or(0),
                merged: flags & TC_MERGED != 0,
                vert_merge: flags & TC_VERT_MERGE != 0,
                vert_restart: flags & TC_VERT_RESTART != 0,
            }
        })
        .collect()
}

/// The names of the fonts of the font table (`SttbfFfn`), by index.
pub(crate) fn fonts(sttb: &[u8]) -> Vec<String> {
    let count = usize::from(u16_at(sttb, 0).unwrap_or(0));
    let mut pos = 4;
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let Some(&len) = sttb.get(pos) else { break };
        let ffn = sttb.get(pos + 1..pos + 1 + usize::from(len)).unwrap_or(&[]);
        names.push(utf16_until_nul(ffn.get(FFN_NAME_OFFSET..).unwrap_or(&[])));
        pos += 1 + usize::from(len);
    }
    names
}

/// The style sheet (`STSH`) as [`Styles`], the style ids being the style
/// indexes (`istd`) as text.
pub(crate) fn stylesheet(stsh: &[u8], fonts: &[String]) -> Styles {
    let header_len = usize::from(u16_at(stsh, 0).unwrap_or(0));
    let header = stsh.get(2..2 + header_len).unwrap_or(&[]);
    let count = usize::from(u16_at(header, 0).unwrap_or(0));
    let base_len = usize::from(u16_at(header, 2).unwrap_or(10));
    let default_font = u16_at(header, 12)
        .and_then(|index| fonts.get(usize::from(index)))
        .cloned()
        .unwrap_or_default();
    let mut defs = Vec::new();
    let mut pos = 2 + header_len;
    for istd in 0..count {
        let Some(len) = u16_at(stsh, pos).map(usize::from) else {
            break;
        };
        let std = stsh.get(pos + 2..pos + 2 + len).unwrap_or(&[]);
        pos += 2 + len;
        if !std.is_empty() {
            defs.push((istd, std_def(std, base_len, fonts)));
        }
    }
    let normal_size = defs
        .iter()
        .find(|(istd, _)| *istd == 0)
        .and_then(|(_, (_, size, _))| *size)
        .unwrap_or(DEFAULT_HALF_POINTS);
    let body_font = defs
        .iter()
        .find(|(istd, _)| *istd == 0)
        .and_then(|(_, (_, _, font))| font.clone())
        .unwrap_or(default_font);
    let mut styles = Styles::new(f32::from(normal_size) / HALF_POINTS_PER_PT, body_font);
    for (istd, (def, _, _)) in defs {
        styles.insert(istd.to_string(), def, istd == 0);
    }
    styles
}

/// One style (`STD`): its definition, its font size in half points and its
/// font, when it sets them.
fn std_def(std: &[u8], base_len: usize, fonts: &[String]) -> (StyleDef, Option<u16>, Option<String>) {
    let kind = u16_at(std, 2).unwrap_or(0) & 0x000F;
    let base = u16_at(std, 2).unwrap_or(0) >> 4;
    let name_len = usize::from(u16_at(std, base_len).unwrap_or(0));
    let name_bytes = std.get(base_len + 2..base_len + 2 + name_len * 2).unwrap_or(&[]);
    let name = utf16(name_bytes);
    let mut pos = base_len + 2 + name_len * 2 + 2;
    let mut upxs = Vec::new();
    // A paragraph style has paragraph then character properties; a
    // character style only character properties.
    let count = match kind {
        1 => 2,
        2 => 1,
        _ => 0,
    };
    for _ in 0..count {
        pos += pos % 2;
        let Some(len) = u16_at(std, pos).map(usize::from) else {
            break;
        };
        upxs.push(std.get(pos + 2..pos + 2 + len).unwrap_or(&[]));
        pos += 2 + len;
    }
    let (para, chars) = match (kind, upxs.as_slice()) {
        (1, [papx, chpx, ..]) => (papx.get(2..).unwrap_or(&[]), *chpx),
        (2, [chpx, ..]) => (&[][..], *chpx),
        _ => (&[][..], &[][..]),
    };
    let props = para_props(0, para);
    let (run, _) = char_props(chars, RunProps::default(), fonts);
    let size = sprms(chars)
        .find(|(sprm, _)| *sprm == SPRM_C_HPS)
        .and_then(|(_, operand)| u16_at(operand, 0));
    let font = sprms(chars)
        .find(|(sprm, _)| *sprm == SPRM_C_FTC0)
        .and_then(|(_, operand)| u16_at(operand, 0))
        .and_then(|index| fonts.get(usize::from(index)).cloned());
    let def = StyleDef {
        name,
        based_on: (base != NO_BASE).then(|| base.to_string()),
        outline: props.outline,
        numbering: (props.ilfo != 0).then(|| (props.ilfo.to_string(), props.ilvl)),
        run,
    };
    (def, size, font)
}

/// The lists of the document (`PlfLst`, followed by their levels, and
/// `PlfLfo`) as [`Numbering`], whose list ids are the 1-based indexes of
/// the overrides paragraphs name (`ilfo`).
pub(crate) fn lists(table: &[u8], plf_lst_offset: usize, plf_lfo: &[u8]) -> Numbering {
    let mut numbering = Numbering::default();
    let Some(plf_lst) = table.get(plf_lst_offset..) else {
        return numbering;
    };
    let count = usize::from(u16_at(plf_lst, 0).unwrap_or(0));
    let mut pos = 2 + count * LSTF_LEN;
    let mut by_id = std::collections::HashMap::new();
    for index in 0..count {
        let lstf = plf_lst
            .get(2 + index * LSTF_LEN..2 + (index + 1) * LSTF_LEN)
            .unwrap_or(&[]);
        let id = u32_at(lstf, 0).unwrap_or(0);
        let simple = lstf.get(26).is_some_and(|flags| flags & 1 != 0);
        let mut levels = Vec::new();
        for _ in 0..if simple { 1 } else { LIST_LEVELS } {
            let Some((level, len)) = level(plf_lst.get(pos..).unwrap_or(&[])) else {
                break;
            };
            levels.push(level);
            pos += len;
        }
        by_id.insert(id, levels);
    }
    let overrides = usize::try_from(u32_at(plf_lfo, 0).unwrap_or(0)).unwrap_or(0);
    for index in 0..overrides {
        let Some(id) = u32_at(plf_lfo, 4 + index * LFO_LEN) else {
            break;
        };
        if let Some(levels) = by_id.get(&id) {
            numbering.insert((index + 1).to_string(), id.to_string(), levels.clone());
        }
    }
    numbering
}

/// One list level (`LVL`) and its size in bytes.
fn level(bytes: &[u8]) -> Option<(Level, usize)> {
    let lvlf = bytes.get(..LVLF_LEN)?;
    let start = u32_at(lvlf, 0)?;
    let format = match lvlf[4] {
        1 => "upperRoman",
        2 => "lowerRoman",
        3 => "upperLetter",
        4 => "lowerLetter",
        22 => "decimalZero",
        23 => "bullet",
        255 => "none",
        _ => "decimal",
    };
    let places: Vec<usize> = lvlf[6..15]
        .iter()
        .take_while(|&&place| place != 0)
        .map(|&place| usize::from(place))
        .collect();
    let grpprls = usize::from(lvlf[24]) + usize::from(lvlf[25]);
    let text_at = LVLF_LEN + grpprls;
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
    let level = Level {
        start,
        format: format.to_string(),
        text,
    };
    Some((level, text_at + 2 + len * 2))
}

/// The bookmarks: (start character position, name).
pub(crate) fn bookmarks(names: &[u8], starts: &[u8]) -> Vec<(u32, String)> {
    let extended = u16_at(names, 0) == Some(0xFFFF);
    let (count, extra, mut pos) = if extended {
        (u16_at(names, 2).unwrap_or(0), u16_at(names, 4).unwrap_or(0), 6)
    } else {
        (u16_at(names, 0).unwrap_or(0), u16_at(names, 2).unwrap_or(0), 4)
    };
    let mut list = Vec::new();
    for _ in 0..count {
        let Some(len) = u16_at(names, pos).map(usize::from) else {
            break;
        };
        list.push(utf16(names.get(pos + 2..pos + 2 + len * 2).unwrap_or(&[])));
        pos += 2 + len * 2 + usize::from(extra);
    }
    let (cps, _) = plc(starts, 4);
    cps.into_iter().zip(list).collect()
}

/// UTF-16 little-endian bytes as text.
pub(crate) fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// UTF-16 little-endian bytes as text, up to the first NUL.
fn utf16_until_nul(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paragraph whose properties are in the `Data` stream gets them.
    #[test]
    fn huge_papx_is_read() {
        let mut data = vec![0u8; 8];
        data.extend_from_slice(&3u16.to_le_bytes());
        data.extend_from_slice(&[0x17, 0x24, 1]);
        let mut grpprl = 0x6646u16.to_le_bytes().to_vec();
        grpprl.extend_from_slice(&8u32.to_le_bytes());
        let runs = with_huge_papx(
            vec![Run {
                start: 0,
                end: 10,
                istd: 0,
                grpprl,
            }],
            &data,
        );
        let found: Vec<u16> = sprms(&runs[0].grpprl).map(|(sprm, _)| sprm).collect();
        assert_eq!(found, [0x6646, 0x2417]);
    }

    /// Sprm operands of every size are skipped correctly.
    #[test]
    fn sprm_lengths() {
        let mut grpprl = Vec::new();
        grpprl.extend_from_slice(&SPRM_C_BOLD.to_le_bytes());
        grpprl.push(1);
        grpprl.extend_from_slice(&SPRM_C_FTC0.to_le_bytes());
        grpprl.extend_from_slice(&3u16.to_le_bytes());
        grpprl.extend_from_slice(&SPRM_C_PIC_LOCATION.to_le_bytes());
        grpprl.extend_from_slice(&1234u32.to_le_bytes());
        grpprl.extend_from_slice(&SPRM_C_ISS.to_le_bytes());
        grpprl.push(1);
        let found: Vec<u16> = sprms(&grpprl).map(|(sprm, _)| sprm).collect();
        assert_eq!(found, [SPRM_C_BOLD, SPRM_C_FTC0, SPRM_C_PIC_LOCATION, SPRM_C_ISS]);
        let fonts = vec![String::new(), String::new(), String::new(), "Courier New".into()];
        let (props, extras) = char_props(&grpprl, RunProps::default(), &fonts);
        assert_eq!(props.bold, Some(true));
        assert_eq!(props.mono, Some(true));
        assert_eq!(props.raised, Some(Some(true)));
        assert_eq!(extras.picture, Some(1234));
    }

    /// A toggle can say "the opposite of the style".
    #[test]
    fn toggles() {
        assert_eq!(toggle(TOGGLE_NOT_STYLE, Some(true)), Some(false));
        assert_eq!(toggle(TOGGLE_LIKE_STYLE, Some(true)), None);
        assert_eq!(toggle(1, None), Some(true));
    }
}
