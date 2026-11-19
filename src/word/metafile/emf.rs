//! EMF files: 32-bit records, with a header that gives the picture's frame
//! in hundredths of a millimetre and the device's resolution. Records
//! between `BEGINPATH` and `ENDPATH` build a path instead of drawing.

use super::dc::{Dc, Font, Matrix, Object, Pen, ROP_NOP};
use super::wmf::{colorref, dib_to_png};
use super::{f32_at, i16_at, i32_at, u32_at};

/// Millimetres per inch.
const MM_PER_INCH: f32 = 25.4;
/// Points per inch.
const PT_PER_INCH: f32 = 72.0;
/// Stock objects have this bit set in their index.
const STOCK_OBJECT: u32 = 0x8000_0000;
/// The most records read (a damaged file stops there).
const MAX_RECORDS: usize = 2_000_000;
/// Offset of the data of a record (after its type and size).
const DATA: usize = 8;

/// The device's pixels per millimetre, by axis.
struct Device {
    /// Horizontal and vertical pixels per millimetre.
    px_per_mm: (f32, f32),
}

/// The EMF `data` as SVG.
pub(super) fn to_svg(data: &[u8]) -> Option<String> {
    let frame = [
        i32_at(data, 24)?,
        i32_at(data, 28)?,
        i32_at(data, 32)?,
        i32_at(data, 36)?,
    ]
    .map(|value| value as f32);
    let device_px = (i32_at(data, 72)? as f32, i32_at(data, 76)? as f32);
    let device_mm = (i32_at(data, 80)? as f32, i32_at(data, 84)? as f32);
    if device_mm.0 <= 0.0 || device_mm.1 <= 0.0 {
        return None;
    }
    let device = Device {
        px_per_mm: (device_px.0 / device_mm.0, device_px.1 / device_mm.1),
    };
    let mut dc = Dc::new();
    dc.state.fixed_scale = Some((1.0, 1.0));
    let mut pos = u32_at(data, 4)? as usize;
    for _ in 0..MAX_RECORDS {
        let kind = u32_at(data, pos)?;
        let size = u32_at(data, pos + 4)? as usize;
        if kind == 14 || size < DATA {
            break;
        }
        let record = data.get(pos..(pos + size).min(data.len()))?;
        replay(&mut dc, &device, kind, record);
        pos += size;
    }
    if dc.svg.is_empty() {
        return None;
    }
    // The frame, in hundredths of a millimetre, in device pixels.
    let [left, top, right, bottom] = frame;
    let (sx, sy) = (device.px_per_mm.0 / 100.0, device.px_per_mm.1 / 100.0);
    let view = [left * sx, top * sy, (right - left) * sx, (bottom - top) * sy];
    let size = (
        (right - left) / 100.0 / MM_PER_INCH * PT_PER_INCH,
        (bottom - top) / 100.0 / MM_PER_INCH * PT_PER_INCH,
    );
    if view[2] <= 0.0 || view[3] <= 0.0 {
        return None;
    }
    Some(dc.svg.finish(view, size))
}

/// The `i32` at byte `at` of the record's data, as `f32`.
fn int(record: &[u8], at: usize) -> f32 {
    i32_at(record, DATA + at).unwrap_or(0) as f32
}

/// Replays one record.
fn replay(dc: &mut Dc, device: &Device, kind: u32, record: &[u8]) {
    match kind {
        9..=12 | 17 | 35 | 36 => mapping(dc, device, kind, record),
        19 => dc.state.even_odd = int(record, 0) != 2.0,
        22 => dc.state.text_align.flags = u32_at(record, DATA).unwrap_or(0),
        24 => dc.state.text_color = colorref(record, DATA),
        33 => dc.save(),
        34 => dc.restore(i32_at(record, DATA).unwrap_or(-1)),
        37..=40 | 82 | 93 | 94 | 95 => objects(dc, kind, record),
        59..=64 | 67 | 68 => path(dc, kind),
        83 | 84 => text(dc, kind, record),
        76 | 77 | 81 => bitmap(dc, kind, record),
        _ => shape(dc, kind, record),
    }
}

/// Records that set the mapping from logical to device coordinates.
fn mapping(dc: &mut Dc, device: &Device, kind: u32, record: &[u8]) {
    let pair = (int(record, 0), int(record, 4));
    match kind {
        9 => dc.state.window_ext = Some(pair),
        10 => dc.state.window_org = pair,
        11 => dc.state.viewport_ext = Some(pair),
        12 => dc.state.viewport_org = pair,
        17 => {
            let (mx, my) = device.px_per_mm;
            dc.state.fixed_scale = match u32_at(record, DATA).unwrap_or(1) {
                2 => Some((0.1 * mx, -0.1 * my)),
                3 => Some((0.01 * mx, -0.01 * my)),
                4 => Some((0.254 * mx, -0.254 * my)),
                5 => Some((0.0254 * mx, -0.0254 * my)),
                6 => Some((MM_PER_INCH / 1440.0 * mx, -MM_PER_INCH / 1440.0 * my)),
                7 | 8 => None,
                _ => Some((1.0, 1.0)),
            };
        }
        35 => dc.state.world = xform(record, DATA),
        _ => {
            let matrix = xform(record, DATA);
            dc.state.world = match u32_at(record, DATA + 24).unwrap_or(1) {
                2 => matrix.then(dc.state.world),
                3 => dc.state.world.then(matrix),
                4 => matrix,
                _ => Matrix::IDENTITY,
            };
        }
    }
}

/// An `XFORM` at byte `at`.
fn xform(record: &[u8], at: usize) -> Matrix {
    let value = |index: usize| f32_at(record, at + index * 4).unwrap_or(0.0);
    Matrix {
        coefficients: [value(0), value(1), value(2), value(3), value(4), value(5)],
    }
}

/// Records that create, select and delete objects.
fn objects(dc: &mut Dc, kind: u32, record: &[u8]) {
    let index = u32_at(record, DATA).unwrap_or(0);
    match kind {
        37 if index & STOCK_OBJECT != 0 => {
            if let Some(object) = stock(index & !STOCK_OBJECT) {
                dc.select_object(object);
            }
        }
        37 => dc.select(index as usize),
        40 => dc.delete(index as usize),
        38 => {
            let style = u32_at(record, DATA + 4).unwrap_or(0) & 0x0F;
            let pen = (style != 5).then(|| Pen {
                color: colorref(record, DATA + 16),
                width: int(record, 8),
            });
            dc.create(Object::Pen(pen), Some(index as usize));
        }
        95 => {
            let style = u32_at(record, DATA + 20).unwrap_or(0) & 0x0F;
            let pen = (style != 5).then(|| Pen {
                color: colorref(record, DATA + 32),
                width: int(record, 24),
            });
            dc.create(Object::Pen(pen), Some(index as usize));
        }
        39 => {
            let style = u32_at(record, DATA + 4).unwrap_or(0);
            let brush = (style != 1).then(|| colorref(record, DATA + 8));
            dc.create(Object::Brush(brush), Some(index as usize));
        }
        82 => dc.create(Object::Font(font(record)), Some(index as usize)),
        _ => dc.create(Object::Brush(Some([192, 192, 192])), Some(index as usize)),
    }
}

/// A stock object.
fn stock(index: u32) -> Option<Object> {
    let gray = |level: u8| Object::Brush(Some([level; 3]));
    Some(match index {
        0 => gray(255),
        1 => gray(192),
        2 => gray(128),
        3 => gray(64),
        4 => gray(0),
        5 => Object::Brush(None),
        6 => Object::Pen(Some(Pen {
            color: [255; 3],
            width: 0.0,
        })),
        7 => Object::Pen(Some(Pen {
            color: [0; 3],
            width: 0.0,
        })),
        8 => Object::Pen(None),
        10..=17 => Object::Font(Font::default()),
        _ => return None,
    })
}

/// A `LOGFONTW` (in `EXTCREATEFONTINDIRECTW`).
fn font(record: &[u8]) -> Font {
    let at = DATA + 4;
    let face: Vec<u16> = (0..32)
        .map_while(|index| super::u16_at(record, at + 28 + index * 2))
        .take_while(|&unit| unit != 0)
        .collect();
    let face = String::from_utf16_lossy(&face);
    Font {
        height: i32_at(record, at).unwrap_or(-12) as f32,
        weight: i32_at(record, at + 16).unwrap_or(400),
        italic: record.get(at + 20).is_some_and(|&byte| byte != 0),
        underline: record.get(at + 21).is_some_and(|&byte| byte != 0),
        escapement: i32_at(record, at + 8).unwrap_or(0) as f32,
        face: if face.is_empty() { "Arial".into() } else { face },
    }
}

/// Path brackets and the records that draw the path built.
fn path(dc: &mut Dc, kind: u32) {
    match kind {
        59 => {
            dc.in_path = true;
            dc.path = Some(String::new());
        }
        60 => dc.in_path = false,
        61 => {
            if let Some(path) = dc.path.as_mut() {
                path.push('Z');
            }
        }
        62 => dc.draw_path(true, false),
        63 => dc.draw_path(true, true),
        64 => dc.draw_path(false, true),
        _ => {
            dc.in_path = false;
            dc.path = None;
        }
    }
}

/// `EXTTEXTOUTA` and `EXTTEXTOUTW`.
fn text(dc: &mut Dc, kind: u32, record: &[u8]) {
    let at = DATA + 28;
    let (x, y) = (
        i32_at(record, at).unwrap_or(0) as f32,
        i32_at(record, at + 4).unwrap_or(0) as f32,
    );
    let count = u32_at(record, at + 8).unwrap_or(0) as usize;
    let string_at = u32_at(record, at + 12).unwrap_or(0) as usize;
    let dx_at = u32_at(record, at + 36).unwrap_or(0) as usize;
    let text: String = if kind == 84 {
        let units: Vec<u16> = (0..count)
            .map_while(|index| super::u16_at(record, string_at + index * 2))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        record
            .get(string_at..string_at + count)
            .unwrap_or(&[])
            .iter()
            .map(|&byte| super::wmf::cp1252(byte))
            .collect()
    };
    let dx: Vec<f32> = if dx_at == 0 {
        Vec::new()
    } else {
        (0..count)
            .map_while(|index| i32_at(record, dx_at + index * 4).map(|value| value as f32))
            .collect()
    };
    let dx = if dx.len() == count { dx } else { Vec::new() };
    dc.text(x, y, &text, &dx);
}

/// Bitmaps: `BITBLT`, `STRETCHBLT`, `STRETCHDIBITS`.
fn bitmap(dc: &mut Dc, kind: u32, record: &[u8]) {
    let field = |index: usize| int(record, 16 + index * 4);
    let (x, y, width, height, bmi_at, bits_at, bits_len) = if kind == 81 {
        let (bmi, bits, len) = (field(6), field(8), field(9));
        (field(0), field(1), field(12), field(13), bmi, bits, len)
    } else {
        // BITBLT and STRETCHBLT: dest box, raster op, source origin, source
        // transform (24 bytes), background colour, usage, then the bitmap.
        let base = 16 + 4 * 4 + 4 + 8 + 24 + 8;
        let at = |index: usize| int(record, base + index * 4);
        (field(0), field(1), field(2), field(3), at(0), at(2), at(3))
    };
    let (bmi_at, bits_at, bits_len) = (bmi_at as usize, bits_at as usize, bits_len as usize);
    let rop = u32_at(record, DATA + if kind == 81 { 60 } else { 32 }).unwrap_or(0);
    if rop == ROP_NOP {
        return;
    }
    if bmi_at == 0 || bits_len == 0 {
        dc.fill_blit(rop, [x, y, width, height]);
        return;
    }
    let (Some(bmi), Some(bits)) = (record.get(bmi_at..bits_at), record.get(bits_at..bits_at + bits_len)) else {
        return;
    };
    let mut dib = bmi.to_vec();
    dib.extend_from_slice(bits);
    let Some(png) = dib_to_png(&dib) else { return };
    let (left, top) = dc.point(x, y);
    let (right, bottom) = dc.point(x + width, y + height);
    dc.svg.image(&png, [left, top, right - left, bottom - top]);
}

/// The points of a record of 32-bit (`wide`) or 16-bit points from byte
/// `at` of its data.
fn points(record: &[u8], at: usize, count: usize, wide: bool) -> Vec<(f32, f32)> {
    (0..count)
        .map_while(|index| {
            if wide {
                let pos = DATA + at + index * 8;
                Some((i32_at(record, pos)? as f32, i32_at(record, pos + 4)? as f32))
            } else {
                let pos = DATA + at + index * 4;
                Some((f32::from(i16_at(record, pos)?), f32::from(i16_at(record, pos + 2)?)))
            }
        })
        .collect()
}

/// Shapes: lines, polygons, Bézier curves, rectangles, ellipses, arcs.
fn shape(dc: &mut Dc, kind: u32, record: &[u8]) {
    let count = u32_at(record, DATA + 16).unwrap_or(0) as usize;
    let wide = kind < 85;
    match kind {
        27 => {
            dc.state.position = (int(record, 0), int(record, 4));
            if dc.in_path {
                let data = dc.poly_data(&[dc.state.position], false);
                dc.draw(&data, false);
            }
        }
        54 => {
            let to = (int(record, 0), int(record, 4));
            let data = if dc.in_path {
                dc.poly_data(&[to], false).replacen('M', "L", 1)
            } else {
                dc.poly_data(&[dc.state.position, to], false)
            };
            dc.draw(&data, false);
            dc.state.position = to;
        }
        3 | 4 | 86 | 87 => {
            let points = points(record, 20, count, wide);
            let closed = kind == 3 || kind == 86;
            let data = dc.poly_data(&points, closed);
            dc.draw(&data, closed);
        }
        6 | 89 => {
            let points = points(record, 20, count, wide);
            let mut data = dc.poly_data(&points, false);
            if dc.in_path {
                data = data.replacen('M', "L", 1);
            } else {
                data = dc.poly_data(&[dc.state.position], false) + &data.replacen('M', "L", 1);
            }
            dc.state.position = points.last().copied().unwrap_or(dc.state.position);
            dc.draw(&data, false);
        }
        2 | 85 | 5 | 88 => beziers(dc, kind, record, count, wide),
        7 | 8 | 90 | 91 => poly_poly(dc, kind, record, wide),
        42..=47 | 55 => boxes(dc, kind, record),
        _ => {}
    }
}

/// `POLYBEZIER` and `POLYBEZIERTO`.
fn beziers(dc: &mut Dc, kind: u32, record: &[u8], count: usize, wide: bool) {
    let points = points(record, 20, count, wide);
    let to = kind == 5 || kind == 88;
    let (start, curves) = if to {
        ((!dc.in_path).then_some(dc.state.position), &points[..])
    } else {
        (points.first().copied(), points.get(1..).unwrap_or(&[]))
    };
    let data = dc.bezier_data(start, curves);
    dc.state.position = points.last().copied().unwrap_or(dc.state.position);
    dc.draw(&data, false);
}

/// `POLYPOLYLINE` and `POLYPOLYGON`.
fn poly_poly(dc: &mut Dc, kind: u32, record: &[u8], wide: bool) {
    let polys = u32_at(record, DATA + 16).unwrap_or(0) as usize;
    let counts: Vec<usize> = (0..polys)
        .map(|index| u32_at(record, DATA + 24 + index * 4).unwrap_or(0) as usize)
        .collect();
    let total: usize = counts.iter().sum();
    let all = points(record, 24 + polys * 4, total, wide);
    let closed = kind == 8 || kind == 91;
    let mut data = String::new();
    let mut at = 0;
    for count in counts {
        let end = (at + count).min(all.len());
        data.push_str(&dc.poly_data(&all[at..end], closed));
        at = end;
    }
    dc.draw(&data, closed);
}

/// Shapes in a box: ellipses, rectangles, rounded rectangles, arcs, chords
/// and pies.
fn boxes(dc: &mut Dc, kind: u32, record: &[u8]) {
    let bounds = [int(record, 0), int(record, 4), int(record, 8), int(record, 12)];
    let [left, top, right, bottom] = bounds;
    let data = match kind {
        42 => dc.ellipse_data(bounds),
        43 | 44 => dc.poly_data(&[(left, top), (right, top), (right, bottom), (left, bottom)], true),
        _ => {
            let start = (int(record, 16), int(record, 20));
            let end = (int(record, 24), int(record, 28));
            dc.arc_data(bounds, start, end, kind == 47, kind == 46)
        }
    };
    let filled = matches!(kind, 42 | 43 | 44 | 46 | 47);
    dc.draw(&data, filled);
}
