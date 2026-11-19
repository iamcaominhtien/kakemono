//! Windows metafiles (EMF and WMF), the vector pictures of Word documents
//! (charts, drawings, the previews of equations), turned into SVG: their
//! drawing records are replayed on a small model of a Windows device
//! context (pens, brushes, fonts, coordinate mapping) and written as SVG
//! paths, text and bitmaps.

mod dc;
mod emf;
mod svg;
mod wmf;

/// Signature of an EMF file, at byte 40 of its header.
const EMF_SIGNATURE: u32 = 0x464D_4520;
/// Key of a placeable WMF file.
const WMF_PLACEABLE_KEY: u32 = 0x9AC6_CDD7;

/// Whether `data` is a Windows metafile (EMF or WMF).
pub(crate) fn is_metafile(data: &[u8]) -> bool {
    is_emf(data) || is_wmf(data)
}

/// The metafile `data` as an SVG document, or `None` when it is not one or
/// draws nothing.
pub(crate) fn to_svg(data: &[u8]) -> Option<String> {
    if is_emf(data) {
        emf::to_svg(data)
    } else if is_wmf(data) {
        wmf::to_svg(data)
    } else {
        None
    }
}

/// Whether `data` is an EMF file.
fn is_emf(data: &[u8]) -> bool {
    u32_at(data, 0) == Some(1) && u32_at(data, 40) == Some(EMF_SIGNATURE)
}

/// Whether `data` is a WMF file, placeable or not.
fn is_wmf(data: &[u8]) -> bool {
    if u32_at(data, 0) == Some(WMF_PLACEABLE_KEY) {
        return true;
    }
    // A plain WMF header: type 1 or 2, header size 9 words, version 0x100
    // or 0x300.
    matches!(u16_at(data, 0), Some(1 | 2))
        && u16_at(data, 2) == Some(9)
        && matches!(u16_at(data, 4), Some(0x100 | 0x300))
}

/// The little-endian `u16` at `pos`.
fn u16_at(bytes: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(pos..pos + 2)?.try_into().ok()?))
}

/// The little-endian `i16` at `pos`.
fn i16_at(bytes: &[u8], pos: usize) -> Option<i16> {
    Some(i16::from_le_bytes(bytes.get(pos..pos + 2)?.try_into().ok()?))
}

/// The little-endian `u32` at `pos`.
fn u32_at(bytes: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?))
}

/// The little-endian `i32` at `pos`.
fn i32_at(bytes: &[u8], pos: usize) -> Option<i32> {
    Some(i32::from_le_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?))
}

/// The little-endian `f32` at `pos`.
fn f32_at(bytes: &[u8], pos: usize) -> Option<f32> {
    Some(f32::from_le_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes the SVG of the metafile `KAKEMONO_METAFILE` next to it (run by
    /// hand: `cargo test metafile_to_svg -- --ignored`).
    #[test]
    #[ignore = "a tool for looking at metafiles"]
    fn metafile_to_svg() {
        let path = std::env::var("KAKEMONO_METAFILE").expect("KAKEMONO_METAFILE names a file");
        let data = std::fs::read(&path).expect("the metafile is readable");
        let svg = to_svg(&data).expect("an SVG");
        std::fs::write(format!("{path}.svg"), svg).expect("the SVG is written");
    }

    /// A WMF that draws a red rectangle and a word becomes an SVG with both.
    #[test]
    fn wmf_rectangle_and_text() {
        let mut wmf = Vec::new();
        // Placeable header: key, handle, bbox 0,0,1000,500, 1000 units per
        // inch, reserved, checksum.
        wmf.extend_from_slice(&WMF_PLACEABLE_KEY.to_le_bytes());
        for value in [0u16, 0, 0, 1000, 500, 1000, 0, 0, 0] {
            wmf.extend_from_slice(&value.to_le_bytes());
        }
        // Standard header.
        for value in [1u16, 9, 0x300, 0, 0, 3, 0, 0, 0] {
            wmf.extend_from_slice(&value.to_le_bytes());
        }
        let mut record = |function: u16, params: &[i16]| {
            let size = 3 + params.len() as u32;
            wmf.extend_from_slice(&size.to_le_bytes());
            wmf.extend_from_slice(&function.to_le_bytes());
            for param in params {
                wmf.extend_from_slice(&param.to_le_bytes());
            }
        };
        // A red solid brush, selected, and a rectangle.
        record(0x02FC, &[0, 0x00FF, 0, 0]);
        record(0x012D, &[0]);
        record(0x041B, &[400, 900, 100, 100]);
        // TEXTOUT "Hi" at (200, 300).
        record(0x0521, &[2, i16::from_le_bytes(*b"Hi"), 300, 200]);
        record(0, &[]);
        assert!(is_metafile(&wmf));
        let svg = to_svg(&wmf).expect("an SVG");
        assert!(svg.contains("#ff0000"), "{svg}");
        assert!(svg.contains(">Hi<"), "{svg}");
    }
}
