//! Floating shapes of a `.doc`: text boxes, rectangles and pictures that
//! float over the text. The main text marks where each is anchored with a
//! character 8; `PlcSpaMom` gives its box (`FSPA`), the OfficeArt drawing
//! (`DggInfo`) its fill, outline, text insets and picture, and the text box
//! story (`PlcftxbxTxt`) its text.

use super::model::{Anchor, AnchorAlign, AnchorFrom, AnchorPos, Border, BorderStyle, Rgb, Wrap};
use crate::word::doc::fib::{plc, u16_at, u32_at};
use std::collections::HashMap;

/// Size of an `FSPA`.
const FSPA_LEN: usize = 26;
/// Size of an `FTXBXS`.
const FTXBXS_LEN: usize = 22;
/// Offset of `lid` (the shape id) in an `FTXBXS`.
const FTXBXS_LID: usize = 14;
/// Size of an OfficeArt record header.
const RECORD_HEADER_LEN: usize = 8;
/// Version of an OfficeArt container record.
const CONTAINER_VERSION: u16 = 0xF;
/// `OfficeArtFSP`: a shape's id and flags.
const FSP: u16 = 0xF00A;
/// `OfficeArtFOPT`: a shape's properties.
const FOPT: u16 = 0xF00B;
/// `OfficeArtTertiaryFOPT`: more of a shape's properties.
const TERTIARY_FOPT: u16 = 0xF122;
/// `OfficeArtSpContainer`: one shape.
const SP_CONTAINER: u16 = 0xF004;
/// `OfficeArtBStoreContainer`: the pictures.
const BSTORE_CONTAINER: u16 = 0xF001;
/// `OfficeArtFBSE`: one picture of the store.
const FBSE: u16 = 0xF007;
/// Offset of `size` in an `OfficeArtFBSE`.
const FBSE_SIZE: usize = 20;
/// Offset of `foDelay` (where the picture is in the `WordDocument`
/// stream) in an `OfficeArtFBSE`.
const FBSE_DELAY: usize = 28;
/// `fGroup` flag of an `OfficeArtFSP`: the shape is a group.
const FSP_GROUP: u32 = 0x1;
/// `fChild` flag of an `OfficeArtFSP`: the shape is in a group.
const FSP_CHILD: u32 = 0x2;
/// `fDeleted` flag of an `OfficeArtFSP`.
const FSP_DELETED: u32 = 0x8;
/// Property: left text inset (EMU).
const PID_TEXT_LEFT: u16 = 0x0081;
/// Property: top text inset (EMU).
const PID_TEXT_TOP: u16 = 0x0082;
/// Property: right text inset (EMU).
const PID_TEXT_RIGHT: u16 = 0x0083;
/// Property: bottom text inset (EMU).
const PID_TEXT_BOTTOM: u16 = 0x0084;
/// Property: the picture, an index into the picture store (from 1).
const PID_PIB: u16 = 0x0104;
/// Property: fill colour.
const PID_FILL_COLOR: u16 = 0x0181;
/// Property: fill opacity (16.16 fixed point, 1 opaque).
const PID_FILL_OPACITY: u16 = 0x0182;
/// Fill opacity of an opaque fill.
const OPAQUE: f32 = 65_536.0;
/// Property: fill booleans (`fFilled`).
const PID_FILL_BOOLEANS: u16 = 0x01BF;
/// Property: line colour.
const PID_LINE_COLOR: u16 = 0x01C0;
/// Property: line width (EMU).
const PID_LINE_WIDTH: u16 = 0x01CB;
/// Property: line booleans (`fLine`).
const PID_LINE_BOOLEANS: u16 = 0x01FF;
/// Property: horizontal alignment (`posh`).
const PID_POSH: u16 = 0x038F;
/// Property: what the horizontal position is relative to (`posrelh`).
const PID_POSRELH: u16 = 0x0390;
/// Property: vertical alignment (`posv`).
const PID_POSV: u16 = 0x0391;
/// Property: what the vertical position is relative to (`posrelv`).
const PID_POSRELV: u16 = 0x0392;
/// Bit of the fill booleans: `fFilled`.
const FILLED: u32 = 0x10;
/// Bit of the fill booleans: `fUsefFilled` (`fFilled` is set).
const USE_FILLED: u32 = 0x10_0000;
/// Bit of the line booleans: `fLine`.
const LINE: u32 = 0x08;
/// Bit of the line booleans: `fUsefLine` (`fLine` is set).
const USE_LINE: u32 = 0x08_0000;
/// Bit of an OfficeArt colour that is not a plain RGB value (scheme,
/// system or palette colour).
const COLOR_NOT_RGB: u32 = 0xFF00_0000;
/// EMU per point.
const EMU_PER_PT: f32 = 12_700.0;
/// Twips per point.
const TWIPS_PER_PT: f32 = 20.0;
/// Default left and right text inset of a text box, in points (0.1 inch).
const DEFAULT_INSET_X_PT: f32 = 7.2;
/// Default top and bottom text inset of a text box, in points (0.05 inch).
const DEFAULT_INSET_Y_PT: f32 = 3.6;
/// Default outline width, in points.
const DEFAULT_LINE_PT: f32 = 0.75;
/// Distance Word keeps between a floating shape and the text beside it,
/// in points (0.125 inch).
const DEFAULT_WRAP_DIST_PT: f32 = 9.0;

/// A floating shape of the main text.
#[derive(Debug, Clone)]
pub(super) struct Float {
    /// Its id, which ties it to its text box text.
    pub(super) spid: u32,
    /// Width and height.
    pub(super) size: (f32, f32),
    /// Where it floats.
    pub(super) anchor: Anchor,
    /// Fill colour (`None`: not filled).
    pub(super) fill: Option<Rgb>,
    /// Outline.
    pub(super) line: Option<Border>,
    /// Text insets: left, top, right, bottom.
    pub(super) insets: [f32; 4],
    /// Its picture file, when it shows one.
    pub(super) picture: Option<Vec<u8>>,
    /// Its outline is set in its properties (pictures have none by
    /// default).
    pub(super) line_set: bool,
    /// Its place in the drawing order (later ones are drawn on top).
    pub(super) order: usize,
}

/// What the OfficeArt drawing says of one shape.
#[derive(Debug, Default, Clone)]
struct ShapeProps {
    /// The shape is a group or in one (not drawn here).
    grouped: bool,
    /// Its place in the drawing order.
    order: usize,
    /// Its simple properties, by id.
    props: HashMap<u16, u32>,
}

/// The floating shapes anchored in the main text, by the position of their
/// anchor character.
pub(super) fn floats(plc_spa: &[u8], dgg: &[u8], word: &[u8]) -> HashMap<u32, Float> {
    let shapes = shape_props(dgg);
    let pictures = picture_store(dgg);
    let (cps, entries) = plc(plc_spa, FSPA_LEN);
    let mut out = HashMap::new();
    for (cp, fspa) in cps.iter().zip(entries) {
        let Some(spid) = u32_at(fspa, 0) else { continue };
        let props = shapes.get(&spid).cloned().unwrap_or_default();
        if std::env::var_os("KAKEMONO_DEBUG_DOC").is_some() {
            let mut list: Vec<_> = props
                .props
                .iter()
                .map(|(pid, value)| format!("{pid:04X}={value:08X}"))
                .collect();
            list.sort();
            eprintln!(
                "FLOAT cp {cp} spid {spid} z {} flags {:04X} grouped {} {}",
                props.order,
                u16_at(fspa, 20).unwrap_or(0),
                props.grouped,
                list.join(" ")
            );
        }
        if props.grouped {
            continue;
        }
        let coord = |at: usize| u32_at(fspa, at).map_or(0.0, |value| value as i32 as f32 / TWIPS_PER_PT);
        let (left, top, right, bottom) = (coord(4), coord(8), coord(12), coord(16));
        let flags = u16_at(fspa, 20).unwrap_or(0);
        let picture = props
            .props
            .get(&PID_PIB)
            .and_then(|&index| pictures.get((index as usize).checked_sub(1)?))
            .and_then(|&(at, len)| crate::word::doc::picture::find_blip(word, at, at.saturating_add(len)));
        out.insert(
            *cp,
            Float {
                spid,
                size: ((right - left).max(0.0), (bottom - top).max(0.0)),
                anchor: anchor(flags, left, top, &props.props),
                fill: fill(&props.props),
                line: line(&props.props),
                insets: insets(&props.props),
                picture,
                line_set: props.props.contains_key(&PID_LINE_BOOLEANS),
                order: props.order,
            },
        );
    }
    out
}

/// The text box texts of the main text: (shape id, first and end
/// character), the characters counted from the start of the text box
/// story.
pub(super) fn text_boxes(plc_txbx: &[u8]) -> Vec<(u32, u32, u32)> {
    let (cps, entries) = plc(plc_txbx, FTXBXS_LEN);
    entries
        .iter()
        .enumerate()
        .filter_map(|(at, entry)| {
            let lid = u32_at(entry, FTXBXS_LID)?;
            Some((lid, *cps.get(at)?, *cps.get(at + 1)?))
        })
        .collect()
}

/// Where a shape floats, from the flags of its `FSPA` and its position
/// properties.
fn anchor(flags: u16, left: f32, top: f32, props: &HashMap<u16, u32>) -> Anchor {
    let bx = (flags >> 1) & 0x3;
    let by = (flags >> 3) & 0x3;
    let wr = (flags >> 5) & 0xF;
    let below = flags & 0x4000 != 0;
    let from_h = match props.get(&PID_POSRELH) {
        Some(1) => AnchorFrom::Page,
        Some(2) => AnchorFrom::Column,
        Some(3) => AnchorFrom::Character,
        Some(_) => AnchorFrom::Margin,
        None => match bx {
            1 => AnchorFrom::Page,
            2 => AnchorFrom::Column,
            _ => AnchorFrom::Margin,
        },
    };
    let from_v = match props.get(&PID_POSRELV) {
        Some(1) => AnchorFrom::Page,
        Some(2) => AnchorFrom::Paragraph,
        Some(3) => AnchorFrom::Line,
        Some(_) => AnchorFrom::Margin,
        None => match by {
            1 => AnchorFrom::Page,
            2 => AnchorFrom::Paragraph,
            _ => AnchorFrom::Margin,
        },
    };
    let align = |value: Option<&u32>| match value {
        Some(1) => Some(AnchorAlign::Start),
        Some(2) => Some(AnchorAlign::Center),
        Some(3) => Some(AnchorAlign::End),
        Some(4) => Some(AnchorAlign::Inside),
        Some(5) => Some(AnchorAlign::Outside),
        _ => None,
    };
    let wrap = match wr {
        1 => Wrap::TopBottom,
        3 => Wrap::None,
        4 | 5 => Wrap::Tight,
        _ => Wrap::Square,
    };
    let side = if wrap == Wrap::Square || wrap == Wrap::Tight {
        DEFAULT_WRAP_DIST_PT
    } else {
        0.0
    };
    Anchor {
        horizontal: AnchorPos {
            from: from_h,
            align: align(props.get(&PID_POSH)),
            offset: left,
        },
        vertical: AnchorPos {
            from: from_v,
            align: align(props.get(&PID_POSV)),
            offset: top,
        },
        wrap,
        behind: below,
        dist: [0.0, 0.0, side, side],
    }
}

/// The fill colour of a shape: white unless its properties say otherwise.
fn fill(props: &HashMap<u16, u32>) -> Option<Rgb> {
    let booleans = props.get(&PID_FILL_BOOLEANS).copied().unwrap_or(0);
    if booleans & USE_FILLED != 0 && booleans & FILLED == 0 {
        return None;
    }
    let opacity = props
        .get(&PID_FILL_OPACITY)
        .map_or(1.0, |&value| (value as f32 / OPAQUE).clamp(0.0, 1.0));
    if opacity <= 0.0 {
        return None;
    }
    let color = props
        .get(&PID_FILL_COLOR)
        .map_or([255, 255, 255], |&color| rgb(color, [255, 255, 255]));
    // A see-through fill is drawn as its colour over the white page.
    Some(color.map(|channel| (f32::from(channel) * opacity + 255.0 * (1.0 - opacity)).round() as u8))
}

/// The outline of a shape: a thin black line unless its properties say
/// otherwise.
fn line(props: &HashMap<u16, u32>) -> Option<Border> {
    let booleans = props.get(&PID_LINE_BOOLEANS).copied().unwrap_or(0);
    if booleans & USE_LINE != 0 && booleans & LINE == 0 {
        return None;
    }
    let width = props
        .get(&PID_LINE_WIDTH)
        .map_or(DEFAULT_LINE_PT, |&emu| emu as f32 / EMU_PER_PT);
    Some(Border {
        style: BorderStyle::Single,
        width,
        color: props
            .get(&PID_LINE_COLOR)
            .map_or([0, 0, 0], |&color| rgb(color, [0, 0, 0])),
        space: 0.0,
    })
}

/// The text insets of a shape: left, top, right, bottom.
fn insets(props: &HashMap<u16, u32>) -> [f32; 4] {
    let get = |pid: u16, default: f32| props.get(&pid).map_or(default, |&emu| emu as f32 / EMU_PER_PT);
    [
        get(PID_TEXT_LEFT, DEFAULT_INSET_X_PT),
        get(PID_TEXT_TOP, DEFAULT_INSET_Y_PT),
        get(PID_TEXT_RIGHT, DEFAULT_INSET_X_PT),
        get(PID_TEXT_BOTTOM, DEFAULT_INSET_Y_PT),
    ]
}

/// An OfficeArt colour as RGB, or `default` when it is a scheme, system or
/// palette colour.
fn rgb(color: u32, default: Rgb) -> Rgb {
    if color & COLOR_NOT_RGB != 0 {
        return default;
    }
    let [red, green, blue, _] = color.to_le_bytes();
    [red, green, blue]
}

/// The OfficeArt records of `bytes` from `pos` to `end`, in order:
/// (record type, instance, start of the body, length of the body); the
/// records inside a container follow it.
fn records(bytes: &[u8], pos: usize, end: usize) -> Vec<(u16, u16, usize, usize)> {
    let mut out = Vec::new();
    walk(bytes, pos, end.min(bytes.len()), &mut out);
    out
}

/// Adds the records from `at` to `stop` to `out`, entering containers.
fn walk(bytes: &[u8], mut at: usize, stop: usize, out: &mut Vec<(u16, u16, usize, usize)>) {
    while at + RECORD_HEADER_LEN <= stop {
        let (Some(version_instance), Some(kind), Some(len)) =
            (u16_at(bytes, at), u16_at(bytes, at + 2), u32_at(bytes, at + 4))
        else {
            break;
        };
        let body = at + RECORD_HEADER_LEN;
        let len = (len as usize).min(stop - body);
        out.push((kind, version_instance >> 4, body, len));
        if version_instance & 0xF == CONTAINER_VERSION {
            walk(bytes, body, body + len, out);
        }
        at = body + len;
    }
}

/// The records of the drawing data `dgg` (`OfficeArtContent`): the
/// drawing group container, then each drawing, after its one-byte label.
fn content_records(dgg: &[u8]) -> Vec<(u16, u16, usize, usize)> {
    let mut out = Vec::new();
    let Some(len) = u32_at(dgg, 4) else { return out };
    let first_end = (RECORD_HEADER_LEN + len as usize).min(dgg.len());
    walk(dgg, 0, first_end, &mut out);
    let mut at = first_end;
    while at + 1 + RECORD_HEADER_LEN <= dgg.len() {
        let start = at + 1;
        let Some(len) = u32_at(dgg, start + 4) else { break };
        let stop = (start + RECORD_HEADER_LEN + len as usize).min(dgg.len());
        walk(dgg, start, stop, &mut out);
        at = stop;
    }
    out
}

/// The properties of each shape of the drawings, by shape id.
fn shape_props(dgg: &[u8]) -> HashMap<u32, ShapeProps> {
    let mut out = HashMap::new();
    let all = content_records(dgg);
    for &(kind, _, body, len) in &all {
        if kind != SP_CONTAINER {
            continue;
        }
        let mut shape = ShapeProps {
            order: out.len(),
            ..ShapeProps::default()
        };
        let mut spid = None;
        for (inner, instance, inner_body, inner_len) in records(dgg, body, body + len) {
            match inner {
                FSP => {
                    spid = u32_at(dgg, inner_body);
                    let flags = u32_at(dgg, inner_body + 4).unwrap_or(0);
                    shape.grouped = flags & (FSP_GROUP | FSP_CHILD | FSP_DELETED) != 0;
                }
                FOPT | TERTIARY_FOPT => {
                    for at in 0..usize::from(instance) {
                        let entry = inner_body + at * 6;
                        if entry + 6 > inner_body + inner_len {
                            break;
                        }
                        let (Some(opid), Some(value)) = (u16_at(dgg, entry), u32_at(dgg, entry + 2)) else {
                            break;
                        };
                        shape.props.insert(opid & 0x3FFF, value);
                    }
                }
                _ => {}
            }
        }
        if let Some(spid) = spid {
            out.entry(spid).or_insert(shape);
        }
    }
    out
}

/// The pictures of the picture store: where each one's record is in the
/// `WordDocument` stream, and its length.
fn picture_store(dgg: &[u8]) -> Vec<(usize, usize)> {
    let all = content_records(dgg);
    let Some(&(_, _, body, len)) = all.iter().find(|record| record.0 == BSTORE_CONTAINER) else {
        return Vec::new();
    };
    records(dgg, body, body + len)
        .into_iter()
        .filter(|record| record.0 == FBSE)
        .map(|(_, _, fbse, _)| {
            let size = u32_at(dgg, fbse + FBSE_SIZE).unwrap_or(0) as usize;
            let delay = u32_at(dgg, fbse + FBSE_DELAY).unwrap_or(u32::MAX) as usize;
            (delay, size)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fill is white by default, absent when turned off or fully
    /// transparent, and lightened by its transparency.
    #[test]
    fn fill_follows_its_properties() {
        let mut props = HashMap::new();
        assert_eq!(fill(&props), Some([255, 255, 255]));
        props.insert(PID_FILL_COLOR, 0x0066_3300);
        assert_eq!(fill(&props), Some([0x00, 0x33, 0x66]));
        props.insert(PID_FILL_OPACITY, 0);
        assert_eq!(fill(&props), None);
        props.insert(PID_FILL_OPACITY, 0x8000);
        assert_eq!(fill(&props), Some([128, 153, 179]));
        props.insert(PID_FILL_BOOLEANS, USE_FILLED);
        assert_eq!(fill(&props), None);
    }

    /// The flags of an `FSPA` give what the shape is placed against and how
    /// text wraps around it.
    #[test]
    fn anchor_from_fspa_flags() {
        // bx = column (2), by = paragraph (2), wr = none (3).
        let flags = (2 << 1) | (2 << 3) | (3 << 5);
        let anchor = anchor(flags, 10.0, 20.0, &HashMap::new());
        assert_eq!(anchor.horizontal.from, AnchorFrom::Column);
        assert_eq!(anchor.vertical.from, AnchorFrom::Paragraph);
        assert_eq!(anchor.wrap, Wrap::None);
        assert_eq!((anchor.horizontal.offset, anchor.vertical.offset), (10.0, 20.0));
        assert!(!anchor.behind);
    }

    /// Text box entries name their shape and their range of the story.
    #[test]
    fn text_box_ranges() {
        let mut plc_bytes = Vec::new();
        for cp in [0u32, 12, 30] {
            plc_bytes.extend_from_slice(&cp.to_le_bytes());
        }
        for lid in [1025u32, 1026] {
            let mut entry = vec![0u8; FTXBXS_LEN];
            entry[FTXBXS_LID..FTXBXS_LID + 4].copy_from_slice(&lid.to_le_bytes());
            plc_bytes.extend_from_slice(&entry);
        }
        assert_eq!(text_boxes(&plc_bytes), vec![(1025, 0, 12), (1026, 12, 30)]);
    }
}
