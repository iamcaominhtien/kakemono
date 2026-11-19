//! The property modifiers (sprms) of a `.doc` read into the same unset-
//! until-set properties as those of a `.docx` (`props`): characters,
//! paragraphs, table rows and sections.

use super::model::{
    Align, AnchorAlign, AnchorFrom, Border, BorderStyle, Columns, Frame, LineSpacing, NumFormat, PageSetup, Rgb,
    SectionStart, TabKind, Underline, VAlign, VertAlign, Wrap,
};
use super::props::{RawPara, RawRun, RawTab};
use crate::word::doc::fib::{i16_at, u16_at, u32_at};
use crate::word::doc::props::sprms;

/// Twips per point.
const TWIPS_PER_PT: f32 = 20.0;
/// Font sizes are in half points.
const HALF_POINTS_PER_PT: f32 = 2.0;
/// Border widths are in eighths of a point.
const EIGHTHS_PER_PT: f32 = 8.0;
/// Auto line spacing is in 240ths of a line.
const LINE_UNITS: f32 = 240.0;
/// A `COLORREF` whose last byte is this is "automatic".
const COLOR_AUTO: u8 = 0xFF;
/// Toggle operand: the value of the style.
const TOGGLE_LIKE_STYLE: u8 = 0x80;
/// Toggle operand: the opposite of the value of the style.
const TOGGLE_NOT_STYLE: u8 = 0x81;
/// Colours of the colour indexes (`ico`), from 1 (black) to 16.
const ICO_COLORS: [Rgb; 16] = [
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

/// The colour of a colour index; `None` for automatic (0).
pub(crate) fn ico(index: u8) -> Option<Rgb> {
    ICO_COLORS.get(usize::from(index).checked_sub(1)?).copied()
}

/// A `COLORREF` (red, green, blue, flag); `None` for automatic.
fn colorref(bytes: &[u8]) -> Option<Rgb> {
    let flag = *bytes.get(3)?;
    (flag != COLOR_AUTO).then(|| [bytes[0], bytes[1], bytes[2]])
}

/// A toggle operand over the value `base` the styles give.
fn toggle(operand: u8, base: Option<bool>) -> Option<bool> {
    match operand {
        0 => Some(false),
        1 => Some(true),
        TOGGLE_LIKE_STYLE => base,
        TOGGLE_NOT_STYLE => Some(!base.unwrap_or(false)),
        _ => None,
    }
}

/// A border of Word 97 (`BRC80`, 4 bytes).
fn brc80(bytes: &[u8]) -> Option<Option<Border>> {
    let width = *bytes.first()?;
    let kind = *bytes.get(1)?;
    let color = *bytes.get(2)?;
    let space = bytes.get(3).map_or(0, |byte| byte & 0x1F);
    if kind == 0xFF && width == 0xFF {
        return Some(None);
    }
    Some(border(kind, width, ico(color).unwrap_or([0, 0, 0]), space))
}

/// A border of Word 2000 and later (`BRC`, 8 bytes).
fn brc(bytes: &[u8]) -> Option<Option<Border>> {
    let color = colorref(bytes.get(..4)?).unwrap_or([0, 0, 0]);
    let width = *bytes.get(4)?;
    let kind = *bytes.get(5)?;
    let space = bytes.get(6).map_or(0, |byte| byte & 0x1F);
    Some(border(kind, width, color, space))
}

/// A border from its type, width in eighths of a point, colour and space.
fn border(kind: u8, width: u8, color: Rgb, space: u8) -> Option<Border> {
    let style = match kind {
        0 | 0xFF => return None,
        3 | 10 | 11..=19 => BorderStyle::Double,
        6 => BorderStyle::Dotted,
        7 | 8 | 9 | 22 | 23 => BorderStyle::Dashed,
        _ => BorderStyle::Single,
    };
    let width = (f32::from(width) / EIGHTHS_PER_PT).max(0.25);
    Some(Border {
        width,
        space: f32::from(space),
        color,
        style,
    })
}

/// A shading of Word 97 (`SHD80`, 2 bytes): its background colour.
fn shd80(bytes: &[u8]) -> Option<Option<Rgb>> {
    let value = u16_at(bytes, 0)?;
    let fore = (value & 0x1F) as u8;
    let back = ((value >> 5) & 0x1F) as u8;
    let pattern = value >> 10;
    Some(match pattern {
        0 => ico(back),
        1 => ico(fore).or(Some([0, 0, 0])),
        _ => ico(back),
    })
}

/// A shading of Word 2000 and later (`SHD`, 10 bytes).
fn shd(bytes: &[u8]) -> Option<Option<Rgb>> {
    let fore = colorref(bytes.get(..4)?);
    let back = colorref(bytes.get(4..8)?);
    let pattern = u16_at(bytes, 8)?;
    Some(match pattern {
        0 => back,
        1 => fore.or(Some([0, 0, 0])),
        0xFFFF => None,
        _ => back,
    })
}

/// Applies a character property list to `raw`; `base` is what the styles
/// give (for toggles), `fonts` the font table.
pub(crate) fn apply_chars(grpprl: &[u8], raw: &mut RawRun, base: &RawRun, fonts: &[String]) {
    for (sprm, operand) in sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        let word = || u16_at(operand, 0).unwrap_or(0);
        let font = |index: u16| fonts.get(usize::from(index)).cloned();
        match sprm {
            0x0835 => raw.bold = toggle(byte, base.bold),
            0x0836 => raw.italic = toggle(byte, base.italic),
            0x0837 => raw.strike = toggle(byte, base.strike),
            0x083A => raw.small_caps = toggle(byte, base.small_caps),
            0x083B => raw.caps = toggle(byte, base.caps),
            0x083C => raw.hidden = toggle(byte, base.hidden),
            // Text deleted with change tracking: the document is laid out
            // with its changes accepted.
            0x0800 if byte == 1 => raw.hidden = Some(true),
            0x2A53 => raw.dstrike = toggle(byte, base.dstrike),
            0x2A3E => {
                raw.underline = Some(match byte {
                    0 => None,
                    2 => Some(Underline::Words),
                    3 => Some(Underline::Double),
                    4 | 20 | 23 | 25 | 26 | 27 => Some(Underline::Dotted),
                    6 => Some(Underline::Thick),
                    7 | 39 | 55 | 71 => Some(Underline::Dashed),
                    11 | 43 | 59 => Some(Underline::Wave),
                    _ => Some(Underline::Single),
                });
            }
            0x2A42 => raw.color = Some(ico(byte)),
            0x6870 => raw.color = Some(colorref(operand)),
            0x2A0C => raw.highlight = Some(ico(byte)),
            0x4866 => raw.shading = shd80(operand),
            0xCA71 => raw.shading = shd(operand.get(1..).unwrap_or(&[])),
            0x4A43 => raw.size = Some(f32::from(word()) / HALF_POINTS_PER_PT),
            0x4A61 => raw.size_cs = Some(f32::from(word()) / HALF_POINTS_PER_PT),
            0x2A48 => {
                raw.vert = Some(match byte {
                    1 => VertAlign::Super,
                    2 => VertAlign::Sub,
                    _ => VertAlign::Baseline,
                });
            }
            0x4845 => raw.position = i16_at(operand, 0).map(|half| f32::from(half) / HALF_POINTS_PER_PT),
            0x8840 => raw.spacing = i16_at(operand, 0).map(|twips| f32::from(twips) / TWIPS_PER_PT),
            0x4852 => raw.scale = Some(f32::from(word())),
            0x484B => raw.kern = Some((word() > 0).then(|| f32::from(word()) / HALF_POINTS_PER_PT)),
            0x4A4F | 0x4A3D => raw.font = font(word()),
            0x4A50 => raw.font_ea = font(word()),
            0x4A51 => raw.font_hansi = font(word()),
            0x4A5E => raw.font_cs = font(word()),
            0x085A => raw.rtl = toggle(byte, base.rtl),
            0x0882 => raw.cs = toggle(byte, base.cs),
            _ => {}
        }
    }
}

/// Applies a frame property (`sprmPPc`, `sprmPDxaAbs`, ...) to `raw`.
fn frame_sprm(sprm: u16, operand: &[u8], raw: &mut RawPara) {
    let byte = operand.first().copied().unwrap_or(0);
    let signed = i16_at(operand, 0).unwrap_or(0);
    let twips = f32::from(signed) / TWIPS_PER_PT;
    let frame = raw
        .frame
        .get_or_insert_with(|| Some(Frame::default()))
        .get_or_insert_with(Frame::default);
    match sprm {
        0x261B => {
            frame.anchor.vertical.from = match (byte >> 4) & 3 {
                0 => AnchorFrom::Margin,
                1 => AnchorFrom::Page,
                _ => AnchorFrom::Paragraph,
            };
            frame.anchor.horizontal.from = match (byte >> 6) & 3 {
                1 => AnchorFrom::Margin,
                2 => AnchorFrom::Page,
                _ => AnchorFrom::Column,
            };
        }
        0x8418 => {
            (frame.anchor.horizontal.align, frame.anchor.horizontal.offset) = match signed {
                -4 => (Some(AnchorAlign::Center), 0.0),
                -8 => (Some(AnchorAlign::End), 0.0),
                -12 => (Some(AnchorAlign::Inside), 0.0),
                -16 => (Some(AnchorAlign::Outside), 0.0),
                _ => (None, twips),
            };
        }
        0x8419 => {
            (frame.anchor.vertical.align, frame.anchor.vertical.offset) = match signed {
                -4 => (Some(AnchorAlign::Start), 0.0),
                -8 => (Some(AnchorAlign::Center), 0.0),
                -12 => (Some(AnchorAlign::End), 0.0),
                -16 => (Some(AnchorAlign::Inside), 0.0),
                -20 => (Some(AnchorAlign::Outside), 0.0),
                _ => (None, twips),
            };
        }
        0x841A => frame.width = f32::from(u16_at(operand, 0).unwrap_or(0)) / TWIPS_PER_PT,
        0x442B => {
            let value = u16_at(operand, 0).unwrap_or(0);
            frame.height = f32::from(value & 0x7FFF) / TWIPS_PER_PT;
            frame.exact = value & 0x8000 == 0 && value != 0;
        }
        0x2423 => {
            frame.anchor.wrap = match byte {
                1 => Wrap::TopBottom,
                3 => Wrap::None,
                _ => Wrap::Square,
            };
        }
        0x842F => {
            frame.anchor.dist[2] = twips.abs();
            frame.anchor.dist[3] = twips.abs();
        }
        _ => {
            frame.anchor.dist[0] = twips.abs();
            frame.anchor.dist[1] = twips.abs();
        }
    }
}

/// Applies a paragraph property list to `raw`.
pub(crate) fn apply_para(grpprl: &[u8], raw: &mut RawPara) {
    for (sprm, operand) in sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        let twips = || i16_at(operand, 0).map(|value| f32::from(value) / TWIPS_PER_PT);
        let utwips = || u16_at(operand, 0).map(|value| f32::from(value) / TWIPS_PER_PT);
        match sprm {
            0x2403 | 0x2461 => {
                raw.align = Some(match byte {
                    1 => Align::Center,
                    2 => Align::Right,
                    3 => Align::Both,
                    4 => Align::Distribute,
                    _ => Align::Left,
                });
            }
            0x840F | 0x845E => raw.left = twips(),
            0x840E | 0x845D => raw.right = twips(),
            0x8411 | 0x8460 => raw.first_line = twips(),
            0xA413 => raw.before = utwips(),
            0xA414 => raw.after = utwips(),
            0x245B => raw.before_auto = Some(byte != 0),
            0x245C => raw.after_auto = Some(byte != 0),
            0x6412 => {
                let line = i16_at(operand, 0).unwrap_or(240);
                let multiple = u16_at(operand, 2).unwrap_or(1) != 0;
                raw.line = Some(if multiple {
                    LineSpacing::Auto(f32::from(line) / LINE_UNITS)
                } else if line < 0 {
                    LineSpacing::Exact(f32::from(-line) / TWIPS_PER_PT)
                } else {
                    LineSpacing::AtLeast(f32::from(line) / TWIPS_PER_PT)
                });
            }
            0x2405 => raw.keep_lines = Some(byte != 0),
            0x2406 => raw.keep_next = Some(byte != 0),
            0x2407 => raw.page_break_before = Some(byte != 0),
            0x2431 => raw.widow_control = Some(byte != 0),
            0x246D => raw.contextual = Some(byte != 0),
            0x2441 => raw.bidi = Some(byte != 0),
            0x2640 => raw.outline = Some((byte < 9).then_some(byte)),
            0xC60D | 0xC615 => apply_tabs(operand, raw),
            0x6424 => raw.border_top = brc80(operand),
            0x6425 => raw.border_left = brc80(operand),
            0x6426 => raw.border_bottom = brc80(operand),
            0x6427 => raw.border_right = brc80(operand),
            0x6428 => raw.border_between = brc80(operand),
            0xC64E => raw.border_top = brc(operand.get(1..).unwrap_or(&[])),
            0xC64F => raw.border_left = brc(operand.get(1..).unwrap_or(&[])),
            0xC650 => raw.border_bottom = brc(operand.get(1..).unwrap_or(&[])),
            0xC651 => raw.border_right = brc(operand.get(1..).unwrap_or(&[])),
            0xC652 => raw.border_between = brc(operand.get(1..).unwrap_or(&[])),
            0x442D => raw.shading = shd80(operand),
            0xC64D => raw.shading = shd(operand.get(1..).unwrap_or(&[])),
            0x260A => {
                let (id, _) = raw.numbering.take().unwrap_or_default();
                raw.numbering = Some((id, Some(byte)));
            }
            0x460B => {
                let (_, level) = raw.numbering.take().unwrap_or_default();
                raw.numbering = Some((u16_at(operand, 0).unwrap_or(0).to_string(), level));
            }
            0x261B | 0x8418 | 0x8419 | 0x841A | 0x442B | 0x2423 | 0x842F | 0x842E => frame_sprm(sprm, operand, raw),
            _ => {}
        }
    }
}

/// Applies a tab change (`sprmPChgTabsPapx`, `sprmPChgTabs`).
fn apply_tabs(operand: &[u8], raw: &mut RawPara) {
    // The operand starts with its length.
    let body = operand.get(1..).unwrap_or(&[]);
    let deleted = usize::from(body.first().copied().unwrap_or(0));
    for at in 0..deleted {
        if let Some(pos) = i16_at(body, 1 + at * 2) {
            raw.tabs.push(RawTab::cleared(f32::from(pos) / TWIPS_PER_PT));
        }
    }
    // `sprmPChgTabs` has a tolerance per deleted tab; it is skipped.
    let added_at = 1 + deleted * 2;
    let added = usize::from(body.get(added_at).copied().unwrap_or(0));
    let kinds_at = added_at + 1 + added * 2;
    for at in 0..added {
        let Some(pos) = i16_at(body, added_at + 1 + at * 2) else {
            continue;
        };
        let descriptor = body.get(kinds_at + at).copied().unwrap_or(0);
        let kind = match descriptor & 0x07 {
            1 => TabKind::Center,
            2 => TabKind::Right,
            3 => TabKind::Decimal,
            4 => TabKind::Bar,
            _ => TabKind::Left,
        };
        let leader = match (descriptor >> 3) & 0x07 {
            1 => Some('.'),
            2 => Some('-'),
            3 | 4 => Some('_'),
            5 => Some('·'),
            _ => None,
        };
        raw.tabs.push(RawTab::new(f32::from(pos) / TWIPS_PER_PT, kind, leader));
    }
}

/// The properties of a table row (from the property list of its row end).
#[derive(Debug, Clone, Default)]
pub(crate) struct RowProps {
    /// Edges of its cells, in points (absolute, from the margin).
    pub(crate) edges: Vec<f32>,
    /// Cell flags: merged into the previous, part of a vertical merge,
    /// start of a vertical merge.
    pub(crate) merges: Vec<(bool, bool, bool)>,
    /// Borders of each cell: top, left, bottom, right (`None` when unset,
    /// for the table's border to apply; `Some(None)` for none).
    pub(crate) cell_borders: Vec<[Option<Option<Border>>; 4]>,
    /// Shading of each cell.
    pub(crate) shading: Vec<Option<Rgb>>,
    /// Vertical alignment of each cell.
    pub(crate) valign: Vec<VAlign>,
    /// Half the space between cells: the left and right cell margins.
    pub(crate) gap_half: f32,
    /// Cell margins set for the whole row: left, top, right, bottom.
    pub(crate) margins: [Option<f32>; 4],
    /// Cell margins set for each cell, over the row's.
    pub(crate) cell_margins: Vec<[Option<f32>; 4]>,
    /// Height and whether it is exact.
    pub(crate) height: Option<(f32, bool)>,
    /// The row repeats on every page.
    pub(crate) header: bool,
    /// The row is not split across pages.
    pub(crate) cant_split: bool,
    /// The row's alignment.
    pub(crate) align: Align,
    /// The table's borders: top, left, bottom, right, inside horizontal,
    /// inside vertical.
    pub(crate) table_borders: [Option<Border>; 6],
}

/// The row properties in a property list.
pub(crate) fn row_props(grpprl: &[u8]) -> RowProps {
    let mut row = RowProps {
        gap_half: 5.4,
        ..Default::default()
    };
    for (sprm, operand) in sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        match sprm {
            0xD608 => def_table(operand, &mut row),
            0x9602 => row.gap_half = i16_at(operand, 0).map_or(5.4, |twips| f32::from(twips) / TWIPS_PER_PT),
            0x9407 => {
                let value = i16_at(operand, 0).unwrap_or(0);
                row.height = match value {
                    0 => None,
                    value if value < 0 => Some((f32::from(-value) / TWIPS_PER_PT, true)),
                    value => Some((f32::from(value) / TWIPS_PER_PT, false)),
                };
            }
            0x3404 => row.header = byte != 0,
            0x3403 | 0x3466 => row.cant_split = byte != 0,
            0x5400 | 0x548A => {
                row.align = match u16_at(operand, 0).unwrap_or(0) {
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Left,
                };
            }
            0xD605 => {
                let body = operand.get(1..).unwrap_or(&[]);
                for (at, border) in row.table_borders.iter_mut().enumerate() {
                    *border = body.get(at * 4..at * 4 + 4).and_then(brc80).flatten();
                }
            }
            0xD613 => {
                let body = operand.get(1..).unwrap_or(&[]);
                for (at, border) in row.table_borders.iter_mut().enumerate() {
                    *border = body.get(at * 8..at * 8 + 8).and_then(brc).flatten();
                }
            }
            0xD609 => {
                let body = operand.get(1..).unwrap_or(&[]);
                row.shading = body.chunks(2).map(|chunk| shd80(chunk).flatten()).collect();
            }
            0xD612 => {
                let body = operand.get(1..).unwrap_or(&[]);
                row.shading = body.chunks(10).map(|chunk| shd(chunk).flatten()).collect();
            }
            0xD62C => set_cell_valign(operand, &mut row),
            0xD62B => set_cell_merge(operand, &mut row),
            0xD634 | 0xD632 => set_cell_margins(sprm, operand, &mut row),
            0xD620 | 0xD62F => set_cell_borders(sprm, operand, &mut row),
            _ => {}
        }
    }
    row
}

/// Reads `TDefTable`: cell edges, merges and the borders of each cell.
fn def_table(operand: &[u8], row: &mut RowProps) {
    const TC80_LEN: usize = 20;
    let body = operand.get(2..).unwrap_or(&[]);
    let count = usize::from(body.first().copied().unwrap_or(0));
    row.edges = (0..=count)
        .filter_map(|at| i16_at(body, 1 + at * 2))
        .map(|twips| f32::from(twips) / TWIPS_PER_PT)
        .collect();
    let tc_start = 1 + (count + 1) * 2;
    row.merges.clear();
    row.cell_borders.clear();
    row.valign = vec![VAlign::Top; count];
    for at in 0..count {
        let tc = body.get(tc_start + at * TC80_LEN..tc_start + (at + 1) * TC80_LEN);
        let flags = tc.and_then(|tc| u16_at(tc, 0)).unwrap_or(0);
        row.merges
            .push((flags & 0x0002 != 0, flags & 0x0020 != 0, flags & 0x0040 != 0));
        if let Some(align) = tc.map(|_| (flags >> 7) & 0x3)
            && let Some(slot) = row.valign.get_mut(at)
        {
            *slot = match align {
                1 => VAlign::Center,
                2 => VAlign::Bottom,
                _ => VAlign::Top,
            };
        }
        // A border of zeros is unset (the table's applies); a nil one is
        // none.
        let side = |offset: usize| {
            tc.and_then(|tc| tc.get(offset..offset + 4))
                .filter(|bytes| bytes.iter().any(|&byte| byte != 0))
                .and_then(brc80)
        };
        row.cell_borders.push([side(4), side(8), side(12), side(16)]);
    }
}

/// Reads `sprmTVertAlign`: the vertical alignment of a range of cells.
fn set_cell_valign(operand: &[u8], row: &mut RowProps) {
    let body = operand.get(1..).unwrap_or(&[]);
    let (first, lim, align) = (body.first(), body.get(1), body.get(2));
    if let (Some(&first), Some(&lim), Some(&align)) = (first, lim, align) {
        for cell in usize::from(first)..usize::from(lim) {
            if let Some(slot) = row.valign.get_mut(cell) {
                *slot = match align {
                    1 => VAlign::Center,
                    2 => VAlign::Bottom,
                    _ => VAlign::Top,
                };
            }
        }
    }
}

/// Reads `sprmTVertMerge`: whether a cell is in a vertical merge, and
/// whether it starts it.
fn set_cell_merge(operand: &[u8], row: &mut RowProps) {
    let (Some(&cell), Some(&flags)) = (operand.get(1), operand.get(2)) else {
        return;
    };
    if let Some(slot) = row.merges.get_mut(usize::from(cell)) {
        slot.1 = flags != 0;
        slot.2 = flags == 3;
    }
}

/// Reads `sprmTCellPaddingDefault` (0xD634, the row's default) or
/// `sprmTCellPadding` (a range of cells'): first cell, limit, sides, unit,
/// width.
fn set_cell_margins(sprm: u16, operand: &[u8], row: &mut RowProps) {
    let body = operand.get(1..).unwrap_or(&[]);
    let (first, lim) = (body.first().copied().unwrap_or(0), body.get(1).copied().unwrap_or(0));
    let sides = body.get(2).copied().unwrap_or(0);
    let width = u16_at(body, 4).map_or(0.0, |twips| f32::from(twips) / TWIPS_PER_PT);
    for (bit, slot) in [(1, 1), (2, 0), (4, 3), (8, 2)] {
        if sides & bit == 0 {
            continue;
        }
        if sprm == 0xD634 {
            row.margins[slot] = Some(width);
        } else {
            for cell in usize::from(first)..usize::from(lim) {
                if row.cell_margins.len() <= cell {
                    row.cell_margins.resize(cell + 1, [None; 4]);
                }
                row.cell_margins[cell][slot] = Some(width);
            }
        }
    }
}

/// Reads `sprmTSetBrc80` / `sprmTSetBrc`: borders set on a range of cells.
fn set_cell_borders(sprm: u16, operand: &[u8], row: &mut RowProps) {
    let body = operand.get(1..).unwrap_or(&[]);
    let (Some(&first), Some(&lim), Some(&sides)) = (body.first(), body.get(1), body.get(2)) else {
        return;
    };
    let value = if sprm == 0xD620 {
        body.get(3..7).and_then(brc80)
    } else {
        body.get(3..11).and_then(brc)
    };
    for cell in usize::from(first)..usize::from(lim) {
        let Some(borders) = row.cell_borders.get_mut(cell) else {
            continue;
        };
        for (bit, slot) in [(1, 0), (2, 1), (4, 2), (8, 3)] {
            if sides & bit != 0 {
                borders[slot] = value;
            }
        }
    }
}

/// The properties of a section.
#[derive(Debug, Clone)]
pub(crate) struct SectionProps {
    pub(crate) page: PageSetup,
    pub(crate) columns: Columns,
    pub(crate) start: SectionStart,
    pub(crate) title_page: bool,
    pub(crate) page_number_start: Option<u32>,
    pub(crate) page_number_format: NumFormat,
    pub(crate) grid_pitch: Option<f32>,
}

/// The section properties in a property list (`SEPX`), over Word 97's
/// defaults.
pub(crate) fn section_props(grpprl: &[u8]) -> SectionProps {
    let mut section = word97_section();
    let mut restart = false;
    let mut start_at = None;
    let mut grid_type = 0;
    let mut pitch = None;
    let mut equal = true;
    let mut widths: Vec<(usize, f32)> = Vec::new();
    let mut spaces: Vec<(usize, f32)> = Vec::new();
    for (sprm, operand) in sprms(grpprl) {
        let byte = operand.first().copied().unwrap_or(0);
        let twips = || u16_at(operand, 0).map_or(0.0, |value| f32::from(value) / TWIPS_PER_PT);
        let signed = || i16_at(operand, 0).map_or(0.0, |value| f32::from(value) / TWIPS_PER_PT);
        match sprm {
            0xB01F => section.page.width = twips(),
            0xB020 => section.page.height = twips(),
            0xB021 => section.page.left = twips(),
            0xB022 => section.page.right = twips(),
            0x9023 => section.page.top = signed().abs(),
            0x9024 => section.page.bottom = signed().abs(),
            0xB025 => section.page.gutter = twips(),
            0xB017 => section.page.header = twips(),
            0xB018 => section.page.footer = twips(),
            0x3009 => {
                section.start = match byte {
                    0 => SectionStart::Continuous,
                    1 => SectionStart::NextColumn,
                    3 => SectionStart::EvenPage,
                    4 => SectionStart::OddPage,
                    _ => SectionStart::NextPage,
                };
            }
            0x300A => section.title_page = byte != 0,
            0x500B => section.columns.count = u32::from(u16_at(operand, 0).unwrap_or(0)) + 1,
            0x900C => section.columns.space = twips(),
            0x3019 => section.columns.separator = byte != 0,
            0x3005 => equal = byte != 0,
            0xF203 => {
                if let (Some(&index), Some(width)) = (operand.first(), u16_at(operand, 1)) {
                    widths.push((usize::from(index), f32::from(width) / TWIPS_PER_PT));
                }
            }
            0xF204 => {
                if let (Some(&index), Some(space)) = (operand.first(), u16_at(operand, 1)) {
                    spaces.push((usize::from(index), f32::from(space) / TWIPS_PER_PT));
                }
            }
            0x3011 => restart = byte != 0,
            0x501C => start_at = u16_at(operand, 0).map(u32::from),
            0x300E => {
                section.page_number_format = match byte {
                    1 => NumFormat::UpperRoman,
                    2 => NumFormat::LowerRoman,
                    3 => NumFormat::UpperLetter,
                    4 => NumFormat::LowerLetter,
                    _ => NumFormat::Decimal,
                };
            }
            0x5032 => grid_type = u16_at(operand, 0).unwrap_or(0),
            0x9031 => pitch = Some(twips()),
            _ => {}
        }
    }
    if restart {
        section.page_number_start = Some(start_at.unwrap_or(1));
    }
    if !equal && !widths.is_empty() {
        section.columns.widths = column_widths(section.columns.count as usize, &widths, &spaces);
    }
    // Lines snap to the grid only for the "lines" grid types.
    if matches!(grid_type, 1..=3) {
        section.grid_pitch = pitch;
    }
    section
}

/// Word 97's default section: a Letter page with its margins, one column,
/// starting on a new page.
fn word97_section() -> SectionProps {
    SectionProps {
        page: PageSetup {
            width: 612.0,
            height: 792.0,
            top: 72.0,
            bottom: 72.0,
            left: 90.0,
            right: 90.0,
            header: 36.0,
            footer: 36.0,
            gutter: 0.0,
        },
        columns: Columns {
            count: 1,
            space: 36.0,
            ..Default::default()
        },
        start: SectionStart::NextPage,
        title_page: false,
        page_number_start: None,
        page_number_format: NumFormat::Decimal,
        grid_pitch: None,
    }
}

/// The width and following space of each of `count` columns, from the
/// widths and spaces set by column index (0 where one is not set).
fn column_widths(count: usize, widths: &[(usize, f32)], spaces: &[(usize, f32)]) -> Vec<(f32, f32)> {
    (0..count)
        .map(|at| {
            let width = widths
                .iter()
                .find(|(index, _)| *index == at)
                .map_or(0.0, |(_, width)| *width);
            let space = spaces
                .iter()
                .find(|(index, _)| *index == at)
                .map_or(0.0, |(_, space)| *space);
            (width, space)
        })
        .collect()
}

/// The style (`istd`) a character property list names.
pub(crate) fn char_style(grpprl: &[u8]) -> Option<u16> {
    sprms(grpprl)
        .find(|(sprm, _)| *sprm == 0x4A30)
        .and_then(|(_, operand)| u16_at(operand, 0))
}

/// The picture location and symbol of a character property list, and
/// whether the character is special.
pub(crate) fn char_extras(grpprl: &[u8]) -> (bool, Option<u32>, Option<(u16, u16)>) {
    let mut special = false;
    let mut picture = None;
    let mut symbol = None;
    for (sprm, operand) in sprms(grpprl) {
        match sprm {
            0x0855 => special = operand.first().copied().unwrap_or(0) != 0,
            0x6A03 => picture = u32_at(operand, 0),
            0x6A09 => symbol = Some((u16_at(operand, 0).unwrap_or(0), u16_at(operand, 2).unwrap_or(0))),
            _ => {}
        }
    }
    (special, picture, symbol)
}
