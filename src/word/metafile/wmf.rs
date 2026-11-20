//! WMF files: 16-bit records of logical coordinates, often with a
//! "placeable" header giving the picture's box and resolution.

use super::dc::{Dc, Font, Object, Pen, ROP_NOP, Rgb};
use super::svg::Svg;
use super::{WMF_PLACEABLE_KEY, i16_at, u16_at, u32_at};

/// Size of the placeable header.
const PLACEABLE_LEN: usize = 22;
/// Size of the standard header.
const HEADER_LEN: usize = 18;
/// Units per inch assumed without a placeable header.
const DEFAULT_UNITS_PER_INCH: f32 = 1440.0;
/// Points per inch.
const PT_PER_INCH: f32 = 72.0;
/// The most records read (a damaged file stops there).
const MAX_RECORDS: usize = 1_000_000;

/// The picture's box (logical units) and its size in points.
struct Frame {
    /// Left, top, width, height.
    view: [f32; 4],
    /// Width and height in points.
    size: (f32, f32),
}

/// The drawing of the WMF `data`, with the area of the picture it
/// shows (x, y, width, height, picture units) and its size in points.
pub(super) fn draw(data: &[u8]) -> Option<(Svg, [f32; 4], (f32, f32))> {
    let placeable = u32_at(data, 0) == Some(WMF_PLACEABLE_KEY);
    let header_at = if placeable { PLACEABLE_LEN } else { 0 };
    let header_words = usize::from(u16_at(data, header_at + 2)?);
    let mut pos = header_at + (header_words * 2).max(HEADER_LEN);
    let mut frame = placeable.then(|| placeable_frame(data)).flatten();
    let mut dc = Dc::new();
    dc.state.text_align.flags = 0;
    let mut window: Option<[f32; 4]> = None;
    for _ in 0..MAX_RECORDS {
        let words = u32_at(data, pos)? as usize;
        let function = u16_at(data, pos + 4)?;
        if function == 0 || words < 3 {
            break;
        }
        let end = (pos + words * 2).min(data.len());
        let params = data.get(pos + 6..end)?;
        record(&mut dc, function, params, &mut window);
        pos = end;
        if pos >= data.len() {
            break;
        }
    }
    if dc.svg.is_empty() {
        return None;
    }
    if let Some(view) = window {
        let size = frame.as_ref().map_or_else(
            || {
                (
                    view[2].abs() / DEFAULT_UNITS_PER_INCH * PT_PER_INCH,
                    view[3].abs() / DEFAULT_UNITS_PER_INCH * PT_PER_INCH,
                )
            },
            |frame| frame.size,
        );
        frame = Some(Frame {
            view: [0.0, 0.0, view[2].abs(), view[3].abs()],
            size,
        });
    }
    let frame = frame?;
    Some((dc.svg, frame.view, frame.size))
}

/// The box and size given by the placeable header.
fn placeable_frame(data: &[u8]) -> Option<Frame> {
    let left = f32::from(i16_at(data, 6)?);
    let top = f32::from(i16_at(data, 8)?);
    let right = f32::from(i16_at(data, 10)?);
    let bottom = f32::from(i16_at(data, 12)?);
    let inch = f32::from(u16_at(data, 14)?).max(1.0);
    Some(Frame {
        view: [
            left.min(right),
            top.min(bottom),
            (right - left).abs(),
            (bottom - top).abs(),
        ],
        size: (
            (right - left).abs() / inch * PT_PER_INCH,
            (bottom - top).abs() / inch * PT_PER_INCH,
        ),
    })
}

/// The `i16` parameter at index `at`, as `f32`.
fn param(params: &[u8], at: usize) -> f32 {
    f32::from(i16_at(params, at * 2).unwrap_or(0))
}

/// Replays one record.
fn record(dc: &mut Dc, function: u16, params: &[u8], window: &mut Option<[f32; 4]>) {
    match function {
        0x020B | 0x020C => set_window(dc, function, params, window),
        0x0103 => {}
        0x0106 => dc.state.even_odd = u16_at(params, 0) != Some(2),
        0x0209 => dc.state.text_color = colorref(params, 0),
        0x012E => dc.state.text_align.flags = u32::from(u16_at(params, 0).unwrap_or(0)),
        0x001E => dc.save(),
        0x0127 => dc.restore(i32::from(i16_at(params, 0).unwrap_or(-1))),
        0x02FA | 0x02FC | 0x02FB | 0x00F7 | 0x06FF | 0x01F9 | 0x0142 => create(dc, function, params),
        0x012D => dc.select(usize::from(u16_at(params, 0).unwrap_or(0))),
        0x01F0 => dc.delete(usize::from(u16_at(params, 0).unwrap_or(0))),
        0x0521 | 0x0A32 => text(dc, function, params),
        0x0F43 | 0x0B41 | 0x0940 => bitmap(dc, function, params),
        _ => shape(dc, function, params),
    }
}

/// `SETWINDOWORG` and `SETWINDOWEXT`: the window, mapped to a box of its
/// size at the origin (flipped when its extent is negative).
fn set_window(dc: &mut Dc, function: u16, params: &[u8], window: &mut Option<[f32; 4]>) {
    let (y, x) = (param(params, 0), param(params, 1));
    let mut view = window.unwrap_or([0.0, 0.0, 0.0, 0.0]);
    if function == 0x020B {
        dc.state.window_org = (x, y);
        view[0] = x;
        view[1] = y;
    } else {
        dc.state.window_ext = Some((x, y));
        view[2] = x;
        view[3] = y;
    }
    *window = Some(view);
    if let Some((ext_x, ext_y)) = dc.state.window_ext {
        dc.state.viewport_ext = Some((ext_x.abs(), ext_y.abs()));
        dc.state.viewport_org = (
            if ext_x < 0.0 { ext_x.abs() } else { 0.0 },
            if ext_y < 0.0 { ext_y.abs() } else { 0.0 },
        );
    }
}

/// The `COLORREF` at byte `pos`.
pub(super) fn colorref(bytes: &[u8], pos: usize) -> Rgb {
    match bytes.get(pos..pos + 3) {
        Some(&[red, green, blue]) => [red, green, blue],
        _ => [0, 0, 0],
    }
}

/// Records that create objects (every one takes an index, even those that
/// draw nothing here).
fn create(dc: &mut Dc, function: u16, params: &[u8]) {
    let object = match function {
        0x02FA => {
            let style = u16_at(params, 0).unwrap_or(0) & 0x0F;
            Object::Pen((style != 5).then(|| Pen {
                color: colorref(params, 6),
                width: param(params, 1),
            }))
        }
        0x02FC => {
            let style = u16_at(params, 0).unwrap_or(0);
            Object::Brush((style != 1).then(|| colorref(params, 2)))
        }
        0x02FB => Object::Font(font(params)),
        0x01F9 | 0x0142 => Object::Brush(Some([192, 192, 192])),
        _ => Object::Other,
    };
    dc.create(object, None);
}

/// A `LOGFONT` with an ANSI face name.
fn font(params: &[u8]) -> Font {
    let face: String = params
        .get(18..50)
        .unwrap_or(&[])
        .iter()
        .take_while(|&&byte| byte != 0)
        .map(|&byte| cp1252(byte))
        .collect();
    Font {
        height: param(params, 0),
        weight: i32::from(i16_at(params, 8).unwrap_or(400)),
        italic: params.get(10).is_some_and(|&byte| byte != 0),
        underline: params.get(11).is_some_and(|&byte| byte != 0),
        escapement: param(params, 2),
        face: if face.is_empty() { "Arial".into() } else { face },
    }
}

/// `TEXTOUT` and `EXTTEXTOUT`.
fn text(dc: &mut Dc, function: u16, params: &[u8]) {
    if function == 0x0521 {
        let len = usize::from(u16_at(params, 0).unwrap_or(0));
        let Some(bytes) = params.get(2..2 + len) else { return };
        let after = 2 + len.div_ceil(2) * 2;
        let (y, x) = (
            param(&params[after.min(params.len())..], 0),
            param(&params[after.min(params.len())..], 1),
        );
        let text: String = bytes.iter().map(|&byte| cp1252(byte)).collect();
        dc.text(x, y, &text, &[]);
        return;
    }
    let (y, x) = (param(params, 0), param(params, 1));
    let count = usize::from(u16_at(params, 4).unwrap_or(0));
    let options = u16_at(params, 6).unwrap_or(0);
    let mut at = 8;
    if options & 0x06 != 0 {
        at += 8;
    }
    let Some(bytes) = params.get(at..at + count) else {
        return;
    };
    let text: String = bytes.iter().map(|&byte| cp1252(byte)).collect();
    let dx_at = at + count.div_ceil(2) * 2;
    let dx: Vec<f32> = (0..count)
        .map_while(|index| i16_at(params, dx_at + index * 2).map(f32::from))
        .collect();
    let dx = if dx.len() == count { dx } else { Vec::new() };
    dc.text(x, y, &text, &dx);
}

/// Bitmaps: `STRETCHDIB`, `DIBSTRETCHBLT`, `DIBBITBLT`.
fn bitmap(dc: &mut Dc, function: u16, params: &[u8]) {
    // Parameters after the raster operation, in reverse order.
    let (fields, dib_at) = match function {
        0x0F43 => (&params[6.min(params.len())..], 22),
        0x0B41 => (&params[4.min(params.len())..], 20),
        _ => (&params[4.min(params.len())..], 16),
    };
    let (height, width, y, x) = match function {
        0x0940 => (param(fields, 2), param(fields, 3), param(fields, 4), param(fields, 5)),
        _ => (param(fields, 4), param(fields, 5), param(fields, 6), param(fields, 7)),
    };
    let Some(dib) = params.get(dib_at..) else { return };
    let rop = u32_at(params, 0).unwrap_or(0);
    if rop == ROP_NOP {
        return;
    }
    if dib.len() < 12 {
        // No bitmap: the box painted as the raster operation says.
        dc.fill_blit(rop, [x, y, width, height]);
        return;
    }
    let Some(png) = dib_to_png(dib) else { return };
    let (left, top) = dc.point(x, y);
    let (right, bottom) = dc.point(x + width, y + height);
    dc.svg.image(&png, [left, top, right - left, bottom - top]);
}

/// Shapes: lines, rectangles, ellipses, polygons, arcs.
fn shape(dc: &mut Dc, function: u16, params: &[u8]) {
    let box_at = |first: usize| {
        [
            param(params, first + 3),
            param(params, first + 2),
            param(params, first + 1),
            param(params, first),
        ]
    };
    match function {
        0x0214 => dc.state.position = (param(params, 1), param(params, 0)),
        0x0213 => {
            let to = (param(params, 1), param(params, 0));
            let data = dc.poly_data(&[dc.state.position, to], false);
            dc.draw(&data, false);
            dc.state.position = to;
        }
        0x041B | 0x061C => {
            let first = if function == 0x061C { 2 } else { 0 };
            let [left, top, right, bottom] = box_at(first);
            let data = dc.poly_data(&[(left, top), (right, top), (right, bottom), (left, bottom)], true);
            dc.draw(&data, true);
        }
        0x0418 => {
            let data = dc.ellipse_data(box_at(0));
            dc.draw(&data, true);
        }
        0x0324 | 0x0325 => {
            let points = points(params, 2, usize::from(u16_at(params, 0).unwrap_or(0)));
            let data = dc.poly_data(&points, function == 0x0324);
            dc.draw(&data, function == 0x0324);
        }
        0x0538 => poly_polygon(dc, params),
        0x0817 | 0x081A | 0x0830 => {
            let end = (param(params, 1), param(params, 0));
            let start = (param(params, 3), param(params, 2));
            let data = dc.arc_data(box_at(4), start, end, function == 0x081A, function == 0x0830);
            dc.draw(&data, function != 0x0817);
        }
        _ => {}
    }
}

/// `POLYPOLYGON`.
fn poly_polygon(dc: &mut Dc, params: &[u8]) {
    let polygons = usize::from(u16_at(params, 0).unwrap_or(0));
    let counts: Vec<usize> = (0..polygons)
        .map(|index| usize::from(u16_at(params, 2 + index * 2).unwrap_or(0)))
        .collect();
    let mut at = 2 + polygons * 2;
    let mut data = String::new();
    for count in counts {
        let polygon = points(params, at, count);
        data.push_str(&dc.poly_data(&polygon, true));
        at += count * 4;
    }
    dc.draw(&data, true);
}

/// `count` points of 16-bit (x, y) from byte `at`.
fn points(params: &[u8], at: usize, count: usize) -> Vec<(f32, f32)> {
    (0..count)
        .map_while(|index| {
            let x = i16_at(params, at + index * 4)?;
            let y = i16_at(params, at + index * 4 + 2)?;
            Some((f32::from(x), f32::from(y)))
        })
        .collect()
}

/// A device-independent bitmap (`BITMAPINFO` and its bits) as a PNG.
pub(super) fn dib_to_png(dib: &[u8]) -> Option<Vec<u8>> {
    let header = u32_at(dib, 0)? as usize;
    let (bits, colors_used) = if header == 12 {
        (u16_at(dib, 10)?, 0)
    } else {
        (u16_at(dib, 14)?, u32_at(dib, 32).unwrap_or(0))
    };
    let entry = if header == 12 { 3 } else { 4 };
    let palette = match (bits, colors_used) {
        (1 | 4 | 8, 0) => 1usize << bits,
        (_, used) => used as usize,
    };
    // BI_BITFIELDS masks follow a 40-byte header.
    let masks = if header == 40 && u32_at(dib, 16) == Some(3) {
        12
    } else {
        0
    };
    let offset = 14 + header + masks + palette * entry;
    let size = 14 + dib.len();
    let mut bmp = Vec::with_capacity(size);
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&u32::try_from(size).ok()?.to_le_bytes());
    bmp.extend_from_slice(&[0, 0, 0, 0]);
    bmp.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
    bmp.extend_from_slice(dib);
    let image = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp).ok()?;
    let mut png = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// A byte of Windows-1252 text.
pub(super) fn cp1252(byte: u8) -> char {
    /// The characters of the bytes 0x80–0x9F.
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘',
        '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&byte) {
        HIGH[usize::from(byte - 0x80)]
    } else {
        char::from(byte)
    }
}

/// A character of a symbol font (Symbol, MT Extra, Wingdings) as the
/// Unicode character it draws; other fonts' characters are kept.
pub(super) fn symbol_char(face: &str, ch: char) -> char {
    let code = u32::from(ch);
    let low = if (0xF000..=0xF0FF).contains(&code) {
        code - 0xF000
    } else {
        code
    };
    let Ok(low) = u8::try_from(low) else { return ch };
    match face.to_ascii_lowercase().as_str() {
        "symbol" => crate::word::symbols::symbol_font_char(low).unwrap_or(ch),
        "mt extra" => mt_extra_char(low).unwrap_or(ch),
        _ => ch,
    }
}

/// The characters of MT Extra (MathType's extra symbols) equations use most.
fn mt_extra_char(code: u8) -> Option<char> {
    Some(match code {
        0x23 => '⋯',
        0x24 => '⋮',
        0x25 => '⋰',
        0x26 => '⋱',
        0x3C => '◁',
        0x3E => '▷',
        0x41 => '↕',
        0x42 => '↔',
        0x43 => '⇕',
        0x44 => '⇔',
        0x60 => '‵',
        0x61 => '↦',
        0x62 => '↿',
        0x63 => '⇂',
        0x64 => '⇀',
        0x65 => '↽',
        0x66 => '⇁',
        0x68 => 'ℏ',
        0x6A => 'ȷ',
        0x6B => '⇄',
        0x6C => 'ℓ',
        0x70 => '℘',
        0x71 => '⇌',
        0x72 => '⇋',
        0x73 => '⇆',
        0x7B => '⏞',
        0x7D => '⏟',
        _ => return None,
    })
}
