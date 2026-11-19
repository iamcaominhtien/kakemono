//! The File Information Block of a `.doc` (where everything else is) and
//! its text: the piece table that maps character positions (CP) to bytes
//! of the `WordDocument` stream (FC).

use anyhow::{Result, bail};

/// `wIdent` of a Word document.
const WORD_IDENT: u16 = 0xA5EC;
/// Lowest `nFib` read here: Word 97.
const MIN_NFIB: u16 = 0x00C1;
/// Flag of `FibBase` for an encrypted document.
const FLAG_ENCRYPTED: u16 = 0x0100;
/// Flag of `FibBase` for the table stream `1Table` (else `0Table`).
const FLAG_WHICH_TABLE: u16 = 0x0200;
/// Flag of `FibBase` for an obfuscated (XOR) document.
const FLAG_OBFUSCATED: u16 = 0x8000;
/// Size of `FibBase`.
const FIB_BASE_LEN: usize = 32;
/// `clxt` of a `Prc` in the CLX.
const CLX_PRC: u8 = 1;
/// `clxt` of the `Pcdt` in the CLX.
const CLX_PCDT: u8 = 2;
/// Size of a piece descriptor (`Pcd`).
const PCD_LEN: usize = 8;
/// Bit of an `FcCompressed` for 8-bit text.
const FC_COMPRESSED: u32 = 0x4000_0000;
/// Characters of 8-bit text in the range 0x80–0x9F, which Word reads as
/// Windows-1252 (the rest of the range is Latin-1).
const CP1252_HIGH: [u16; 32] = [
    0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D,
    0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
    0x0153, 0x009D, 0x017E, 0x0178,
];

/// Index of an `fc`/`lcb` pair in `FibRgFcLcb97`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Pair {
    /// The style sheet (`STSH`).
    Stshf = 1,
    /// Footnote reference positions.
    PlcffndRef = 2,
    /// Footnote text positions.
    PlcffndTxt = 3,
    /// Section descriptors.
    PlcfSed = 6,
    /// Header and footer story positions.
    PlcfHdd = 11,
    /// Character property pages.
    PlcfBteChpx = 12,
    /// Paragraph property pages.
    PlcfBtePapx = 13,
    /// The font table.
    SttbfFfn = 15,
    /// Bookmark names.
    SttbfBkmk = 21,
    /// Bookmark start positions.
    PlcfBkf = 22,
    /// Document properties (`DOP`).
    Dop = 31,
    /// The piece table.
    Clx = 33,
    /// Floating shapes anchored in the main text (`PlcfSpa`).
    PlcSpaMom = 40,
    /// Endnote reference positions.
    PlcfendRef = 46,
    /// Endnote text positions.
    PlcfendTxt = 47,
    /// The drawings (`OfficeArtContent`).
    DggInfo = 50,
    /// Text box stories of the main text.
    PlcftxbxTxt = 56,
    /// The list definitions.
    PlfLst = 73,
    /// The list overrides, which paragraphs name.
    PlfLfo = 74,
}

/// What the reader uses of the File Information Block.
#[derive(Debug)]
pub(crate) struct Fib {
    /// The table stream is `1Table` (else `0Table`).
    pub(crate) table_1: bool,
    /// Characters of the main text.
    pub(crate) ccp_text: u32,
    /// Characters of the footnotes.
    pub(crate) ccp_ftn: u32,
    /// Characters of the headers and footers.
    pub(crate) ccp_hdd: u32,
    /// Characters of the comments.
    pub(crate) ccp_atn: u32,
    /// Characters of the endnotes.
    pub(crate) ccp_edn: u32,
    /// The `fc`/`lcb` pairs: where each structure is in the table stream.
    pairs: Vec<(u32, u32)>,
}

impl Fib {
    /// Reads the FIB at the start of the `WordDocument` stream.
    ///
    /// # Errors
    /// When the stream is no Word 97–2003 document, or is encrypted.
    pub(crate) fn parse(word: &[u8]) -> Result<Self> {
        if u16_at(word, 0) != Some(WORD_IDENT) {
            bail!("not a Word document (unknown signature)");
        }
        let nfib = u16_at(word, 2).unwrap_or(0);
        if nfib < MIN_NFIB {
            bail!("a Word 6 or Word 95 document: only Word 97 and later .doc files can be read; save it as .docx");
        }
        let flags = u16_at(word, 0x0A).unwrap_or(0);
        if flags & (FLAG_ENCRYPTED | FLAG_OBFUSCATED) != 0 {
            bail!("the document is protected by a password");
        }
        let mut pos = FIB_BASE_LEN;
        let csw = usize::from(u16_at(word, pos).unwrap_or(0));
        pos += 2 + csw * 2;
        let cslw = usize::from(u16_at(word, pos).unwrap_or(0));
        let lw = pos + 2;
        let long = |index: usize| {
            (index < cslw)
                .then(|| u32_at(word, lw + index * 4))
                .flatten()
                .unwrap_or(0)
        };
        pos = lw + cslw * 4;
        let pairs_count = usize::from(u16_at(word, pos).unwrap_or(0));
        let pairs = (0..pairs_count)
            .map(|index| {
                let at = pos + 2 + index * 8;
                (u32_at(word, at).unwrap_or(0), u32_at(word, at + 4).unwrap_or(0))
            })
            .collect();
        Ok(Self {
            table_1: flags & FLAG_WHICH_TABLE != 0,
            ccp_text: long(3),
            ccp_ftn: long(4),
            ccp_hdd: long(5),
            ccp_atn: long(7),
            ccp_edn: long(8),
            pairs,
        })
    }

    /// The bytes of the structure `pair` in the table stream (empty when
    /// the document has none).
    pub(crate) fn slice<'a>(&self, table: &'a [u8], pair: Pair) -> &'a [u8] {
        let (fc, lcb) = self.pairs.get(pair as usize).copied().unwrap_or((0, 0));
        let (start, len) = (fc as usize, lcb as usize);
        table.get(start..start.saturating_add(len)).unwrap_or(&[])
    }

    /// Where the structure `pair` starts in the table stream.
    pub(crate) fn offset(&self, pair: Pair) -> usize {
        self.pairs.get(pair as usize).map_or(0, |(fc, _)| *fc as usize)
    }
}

/// A piece of the text: characters `cp..cp_end` stored from byte `fc` of
/// the `WordDocument` stream.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// First character.
    cp: u32,
    /// End of its characters.
    cp_end: u32,
    /// Byte of its first character.
    fc: u32,
    /// The text is 8-bit (Windows-1252), else UTF-16.
    compressed: bool,
}

/// The text of the document: every character with the byte it is stored
/// at, which the property pages are indexed by.
pub(crate) struct Text {
    /// Character code units (UTF-16), by character position.
    pub(crate) units: Vec<u16>,
    /// The byte of each character in the `WordDocument` stream.
    pub(crate) fcs: Vec<u32>,
}

impl Text {
    /// Reads the text of all the parts of the document (main text,
    /// footnotes, headers, ...) through the piece table.
    ///
    /// # Errors
    /// When the piece table is missing or damaged.
    pub(crate) fn read(word: &[u8], clx: &[u8]) -> Result<Self> {
        let pieces = pieces(clx)?;
        let total = pieces.iter().map(|piece| piece.cp_end).max().unwrap_or(0) as usize;
        let mut units = vec![0u16; total];
        let mut fcs = vec![0u32; total];
        for piece in pieces {
            for cp in piece.cp..piece.cp_end {
                let offset = cp - piece.cp;
                let (unit, fc) = if piece.compressed {
                    let fc = piece.fc + offset;
                    let byte = word.get(fc as usize).copied().unwrap_or(b' ');
                    (cp1252(byte), fc)
                } else {
                    let fc = piece.fc + offset * 2;
                    (u16_at(word, fc as usize).unwrap_or(u16::from(b' ')), fc)
                };
                units[cp as usize] = unit;
                fcs[cp as usize] = fc;
            }
        }
        Ok(Self { units, fcs })
    }
}

/// The pieces of the piece table (`Pcdt` of the CLX).
fn pieces(clx: &[u8]) -> Result<Vec<Piece>> {
    let mut pos = 0;
    while clx.get(pos) == Some(&CLX_PRC) {
        let size = usize::from(u16_at(clx, pos + 1).unwrap_or(0));
        pos += 3 + size;
    }
    if clx.get(pos) != Some(&CLX_PCDT) {
        bail!("damaged document: no piece table");
    }
    let lcb = u32_at(clx, pos + 1).unwrap_or(0) as usize;
    let plc = clx.get(pos + 5..pos + 5 + lcb).unwrap_or(&[]);
    let count = plc.len().saturating_sub(4) / (4 + PCD_LEN);
    let mut pieces = Vec::with_capacity(count);
    for index in 0..count {
        let (Some(cp), Some(cp_end)) = (u32_at(plc, index * 4), u32_at(plc, index * 4 + 4)) else {
            break;
        };
        let pcd = (count + 1) * 4 + index * PCD_LEN;
        let raw = u32_at(plc, pcd + 2).unwrap_or(0);
        let compressed = raw & FC_COMPRESSED != 0;
        let fc = raw & !FC_COMPRESSED & 0x3FFF_FFFF;
        pieces.push(Piece {
            cp,
            cp_end: cp_end.max(cp),
            fc: if compressed { fc / 2 } else { fc },
            compressed,
        });
    }
    Ok(pieces)
}

/// A byte of 8-bit text as a UTF-16 code unit.
fn cp1252(byte: u8) -> u16 {
    match byte {
        0x80..=0x9F => CP1252_HIGH[usize::from(byte - 0x80)],
        other => u16::from(other),
    }
}

/// The character positions of a PLC (a list of `n + 1` positions followed
/// by `n` data elements of `data_len` bytes): the positions, and the data
/// of each element.
pub(crate) fn plc(bytes: &[u8], data_len: usize) -> (Vec<u32>, Vec<&[u8]>) {
    if bytes.len() < 4 {
        return (Vec::new(), Vec::new());
    }
    let count = (bytes.len() - 4) / (4 + data_len);
    let cps = (0..=count).filter_map(|index| u32_at(bytes, index * 4)).collect();
    let data_start = (count + 1) * 4;
    let data = (0..count)
        .filter_map(|index| bytes.get(data_start + index * data_len..data_start + (index + 1) * data_len))
        .collect();
    (cps, data)
}

/// The little-endian `u16` at `pos`.
pub(crate) fn u16_at(bytes: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(pos..pos + 2)?.try_into().ok()?))
}

/// The little-endian `u32` at `pos`.
pub(crate) fn u32_at(bytes: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?))
}

/// The little-endian `i16` at `pos`.
pub(crate) fn i16_at(bytes: &[u8], pos: usize) -> Option<i16> {
    u16_at(bytes, pos).map(|value| value as i16)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A piece table of two pieces, one 8-bit and one UTF-16, reads as one
    /// text with the byte of each character.
    #[test]
    fn pieces_read_as_one_text() {
        let mut word = vec![0u8; 64];
        word[10..13].copy_from_slice(b"ab\x93");
        word[40..44].copy_from_slice(&[0x3B1, 0x3B2].map(u16::to_le_bytes).concat());
        let mut plc = Vec::new();
        for cp in [0u32, 3, 5] {
            plc.extend_from_slice(&cp.to_le_bytes());
        }
        for (fc, compressed) in [(10u32 * 2, true), (40, false)] {
            plc.extend_from_slice(&[0, 0]);
            let raw = if compressed { fc | FC_COMPRESSED } else { fc };
            plc.extend_from_slice(&raw.to_le_bytes());
            plc.extend_from_slice(&[0, 0]);
        }
        let mut clx = vec![CLX_PCDT];
        clx.extend_from_slice(&(plc.len() as u32).to_le_bytes());
        clx.extend_from_slice(&plc);
        let text = Text::read(&word, &clx).unwrap();
        assert_eq!(String::from_utf16_lossy(&text.units), "ab\u{201C}αβ");
        assert_eq!(text.fcs, [10, 11, 12, 40, 42]);
    }
}
