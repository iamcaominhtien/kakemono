//! The pictures of a `.doc`: a `PICF` in the `Data` stream, with its size
//! in the document, followed by OfficeArt records that hold the picture
//! file (a PNG, a JPEG, a device-independent bitmap, a metafile).

use super::fib::{i16_at, u16_at, u32_at};

/// Picture storage mode: the name of a linked file follows the header.
const MM_SHAPEFILE: i16 = 0x66;
/// Size of an OfficeArt record header.
const RECORD_HEADER_LEN: usize = 8;
/// Record version of a container (its content is records).
const CONTAINER_VERSION: u16 = 0xF;
/// OfficeArt record type: a picture in the picture store (`FBSE`).
const FBSE: u16 = 0xF007;
/// Size of the fixed part of an `FBSE`.
const FBSE_LEN: usize = 36;
/// Offset of the name length in an `FBSE`.
const FBSE_NAME_LEN_OFFSET: usize = 33;
/// OfficeArt record type: a JPEG.
const BLIP_JPEG: u16 = 0xF01D;
/// OfficeArt record type: a PNG.
const BLIP_PNG: u16 = 0xF01E;
/// OfficeArt record type: a device-independent bitmap.
const BLIP_DIB: u16 = 0xF01F;
/// OfficeArt record type: a CMYK JPEG.
const BLIP_JPEG_CMYK: u16 = 0xF02A;
/// Instances of the bitmap records that have a second identifier.
const TWO_UID_INSTANCES: &[u16] = &[0x46B, 0x6E1, 0x6E3, 0x7A9, 0x6E5];
/// Size of an identifier of a picture record.
const UID_LEN: usize = 16;
/// Twips per point.
const TWIPS_PER_PT: f32 = 20.0;
/// The scale factors of a `PICF` are in thousandths.
const SCALE_UNIT: f32 = 1000.0;
/// Size of the file header put before a device-independent bitmap to make
/// a BMP file.
const BMP_FILE_HEADER_LEN: u32 = 14;

/// The picture at offset `location` of the `Data` stream: its file (a PNG,
/// a JPEG or a BMP; `None` for a metafile or damaged data) and its size in
/// the document, in points.
pub(crate) fn picture(data: &[u8], location: u32) -> (Option<Vec<u8>>, (f32, f32)) {
    let start = location as usize;
    let Some(picf) = data.get(start..) else {
        return (None, (0.0, 0.0));
    };
    let total = u32_at(picf, 0).unwrap_or(0) as usize;
    let header_len = usize::from(u16_at(picf, 4).unwrap_or(0));
    let mode = i16_at(picf, 6).unwrap_or(0);
    let goal = |at: usize| f32::from(i16_at(picf, at).unwrap_or(0));
    let scale = |at: usize| match u16_at(picf, at).unwrap_or(0) {
        0 => 1.0,
        value => f32::from(value) / SCALE_UNIT,
    };
    let size = (goal(28) * scale(32) / TWIPS_PER_PT, goal(30) * scale(34) / TWIPS_PER_PT);
    let mut pos = header_len;
    if mode == MM_SHAPEFILE {
        pos += 1 + usize::from(picf.get(pos).copied().unwrap_or(0));
    }
    let end = total.min(picf.len());
    (find_blip(picf, pos, end), size)
}

/// The picture file of the first bitmap record found between `pos` and
/// `end`, looking into containers and picture store entries.
fn find_blip(bytes: &[u8], mut pos: usize, end: usize) -> Option<Vec<u8>> {
    while pos + RECORD_HEADER_LEN <= end {
        let version_instance = u16_at(bytes, pos)?;
        let kind = u16_at(bytes, pos + 2)?;
        let len = u32_at(bytes, pos + 4)? as usize;
        let body = pos + RECORD_HEADER_LEN;
        if version_instance & 0xF == CONTAINER_VERSION {
            pos = body;
            continue;
        }
        match kind {
            FBSE => {
                let name_len = usize::from(*bytes.get(body + FBSE_NAME_LEN_OFFSET)?);
                pos = body + FBSE_LEN + name_len;
                continue;
            }
            BLIP_JPEG | BLIP_PNG | BLIP_DIB | BLIP_JPEG_CMYK => {
                let instance = version_instance >> 4;
                let uids = if TWO_UID_INSTANCES.contains(&instance) { 2 } else { 1 };
                let start = body + uids * UID_LEN + 1;
                let file = bytes.get(start..(body + len).min(bytes.len()))?;
                return Some(if kind == BLIP_DIB {
                    bmp_file(file)
                } else {
                    file.to_vec()
                });
            }
            _ => pos = body + len,
        }
    }
    None
}

/// A device-independent bitmap made into a BMP file, by putting the file
/// header in front of it.
fn bmp_file(dib: &[u8]) -> Vec<u8> {
    let info_len = u32_at(dib, 0).unwrap_or(40);
    let bits = u32::from(u16_at(dib, 14).unwrap_or(24));
    let used = u32_at(dib, 32).unwrap_or(0);
    let colors = if used > 0 {
        used
    } else if bits <= 8 {
        1 << bits
    } else {
        0
    };
    let offset = BMP_FILE_HEADER_LEN + info_len + colors * 4;
    let size = BMP_FILE_HEADER_LEN + dib.len() as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(dib);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG in a picture store entry, after a shape container, is found
    /// with the size of the picture in the document.
    #[test]
    fn png_in_a_picture_store_entry() {
        let png = b"\x89PNG\r\n\x1a\nDATA".to_vec();
        let mut blip = Vec::new();
        blip.extend_from_slice(&(0x6E0u16 << 4).to_le_bytes());
        blip.extend_from_slice(&BLIP_PNG.to_le_bytes());
        blip.extend_from_slice(&((UID_LEN + 1 + png.len()) as u32).to_le_bytes());
        blip.extend_from_slice(&[0; UID_LEN + 1]);
        blip.extend_from_slice(&png);
        let mut fbse = vec![0u8; FBSE_LEN];
        fbse[FBSE_NAME_LEN_OFFSET] = 0;
        let mut records = Vec::new();
        // An empty shape container, then the store entry.
        records.extend_from_slice(&0x000Fu16.to_le_bytes());
        records.extend_from_slice(&0xF004u16.to_le_bytes());
        records.extend_from_slice(&0u32.to_le_bytes());
        records.extend_from_slice(&0x0002u16.to_le_bytes());
        records.extend_from_slice(&FBSE.to_le_bytes());
        records.extend_from_slice(&((FBSE_LEN + blip.len()) as u32).to_le_bytes());
        records.extend_from_slice(&fbse);
        records.extend_from_slice(&blip);
        let mut picf = vec![0u8; 68];
        picf[4..6].copy_from_slice(&68u16.to_le_bytes());
        picf[6..8].copy_from_slice(&0x64i16.to_le_bytes());
        picf[28..30].copy_from_slice(&1440i16.to_le_bytes());
        picf[30..32].copy_from_slice(&720i16.to_le_bytes());
        picf[32..34].copy_from_slice(&500u16.to_le_bytes());
        picf[34..36].copy_from_slice(&1000u16.to_le_bytes());
        picf.extend_from_slice(&records);
        let total = picf.len() as u32;
        picf[0..4].copy_from_slice(&total.to_le_bytes());
        let mut data = vec![0u8; 10];
        data.extend_from_slice(&picf);
        let (file, size) = picture(&data, 10);
        assert_eq!(file.unwrap(), png);
        assert_eq!(size, (36.0, 36.0));
    }
}
