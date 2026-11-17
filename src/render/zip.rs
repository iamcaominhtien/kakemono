//! A minimal ZIP writer: just what a `.docx` or `.epub` package needs (files stored or
//! deflated, no directories, no encryption, no ZIP64).
//!
//! Every entry gets the same fixed timestamp, so that the same document
//! always gives the same bytes.

use std::io::Write;

/// Signature of a local file header.
const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;
/// Signature of a central directory entry.
const CENTRAL_HEADER_SIG: u32 = 0x0201_4b50;
/// Signature of the end of central directory record.
const END_OF_CENTRAL_DIR_SIG: u32 = 0x0605_4b50;
/// "Version needed to extract" 2.0: deflate.
const VERSION_NEEDED: u16 = 20;
/// Compression method: stored.
const METHOD_STORED: u16 = 0;
/// Compression method: deflate.
const METHOD_DEFLATE: u16 = 8;
/// MS-DOS date of every entry: 1 January 1980, the earliest date ZIP can
/// hold (`(year - 1980) << 9 | month << 5 | day`).
const DOS_DATE: u16 = (1 << 5) | 1;
/// MS-DOS time of every entry: midnight.
const DOS_TIME: u16 = 0;
/// General purpose flag: the file name is UTF-8.
const FLAG_UTF8: u16 = 1 << 11;

/// How an entry is written.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Compression {
    /// As it is: for data that is compressed already (PNG, JPEG).
    Stored,
    /// Deflated: for text (XML).
    Deflated,
}

/// An entry already written, kept for the central directory.
struct Entry {
    /// File name inside the archive.
    name: String,
    /// CRC-32 of the uncompressed data.
    crc: u32,
    /// Size of the data in the archive.
    compressed: u32,
    /// Size of the data once extracted.
    size: u32,
    /// Compression method.
    method: u16,
    /// Offset of the entry's local header from the start of the archive.
    offset: u32,
}

/// Builds a ZIP archive in memory, entry by entry.
#[derive(Default)]
pub(crate) struct ZipWriter {
    /// The archive so far.
    out: Vec<u8>,
    /// The entries written so far.
    entries: Vec<Entry>,
}

impl ZipWriter {
    /// Adds the file `name` with the contents `data`.
    ///
    /// # Panics
    /// When the archive would pass 4 GB, which ZIP without ZIP64 cannot
    /// hold.
    pub(crate) fn add(&mut self, name: &str, data: &[u8], compression: Compression) {
        let (method, body) = match compression {
            Compression::Stored => (METHOD_STORED, data.to_vec()),
            Compression::Deflated => (METHOD_DEFLATE, deflate(data)),
        };
        let entry = Entry {
            name: name.to_string(),
            crc: crc32fast::hash(data),
            compressed: u32::try_from(body.len()).expect("entry smaller than 4 GB"),
            size: u32::try_from(data.len()).expect("entry smaller than 4 GB"),
            method,
            offset: u32::try_from(self.out.len()).expect("archive smaller than 4 GB"),
        };
        let out = &mut self.out;
        put32(out, LOCAL_HEADER_SIG);
        put16(out, VERSION_NEEDED);
        write_common(out, &entry);
        put16(out, 0); // length of the extra field
        out.extend_from_slice(entry.name.as_bytes());
        out.extend_from_slice(&body);
        self.entries.push(entry);
    }

    /// Writes the central directory and returns the whole archive.
    ///
    /// # Panics
    /// When the archive has more than 65535 entries or passes 4 GB.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        let start = u32::try_from(self.out.len()).expect("archive smaller than 4 GB");
        let out = &mut self.out;
        for entry in &self.entries {
            put32(out, CENTRAL_HEADER_SIG);
            put16(out, VERSION_NEEDED); // version made by
            put16(out, VERSION_NEEDED);
            write_common(out, entry);
            put16(out, 0); // extra field length
            put16(out, 0); // comment length
            put16(out, 0); // disk number
            put16(out, 0); // internal attributes
            put32(out, 0); // external attributes
            put32(out, entry.offset);
            out.extend_from_slice(entry.name.as_bytes());
        }
        let size = u32::try_from(out.len()).expect("archive smaller than 4 GB") - start;
        let count = u16::try_from(self.entries.len()).expect("fewer than 65536 entries");
        put32(out, END_OF_CENTRAL_DIR_SIG);
        put16(out, 0); // this disk
        put16(out, 0); // disk of the central directory
        put16(out, count);
        put16(out, count);
        put32(out, size);
        put32(out, start);
        put16(out, 0); // comment length
        self.out
    }
}

/// Writes the fields shared by the local header and the central directory
/// entry, from the flags to the length of the file name.
fn write_common(out: &mut Vec<u8>, entry: &Entry) {
    let flags = if entry.name.is_ascii() { 0 } else { FLAG_UTF8 };
    put16(out, flags);
    put16(out, entry.method);
    put16(out, DOS_TIME);
    put16(out, DOS_DATE);
    put32(out, entry.crc);
    put32(out, entry.compressed);
    put32(out, entry.size);
    put16(
        out,
        u16::try_from(entry.name.len()).expect("file name shorter than 64 KB"),
    );
}

/// The raw deflate stream of `data` (no zlib header), as ZIP stores it.
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

/// Appends a little-endian `u16`.
fn put16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends a little-endian `u32`.
fn put32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn entries_can_be_read_back() {
        let mut zip = ZipWriter::default();
        let text = "<a>hello</a>".repeat(50);
        zip.add("word/document.xml", text.as_bytes(), Compression::Deflated);
        zip.add("media/x.png", b"\x89PNG data", Compression::Stored);
        let bytes = zip.finish();

        // The end record points at two entries.
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], &END_OF_CENTRAL_DIR_SIG.to_le_bytes());
        assert_eq!(u16::from_le_bytes([bytes[end + 10], bytes[end + 11]]), 2);

        // The first entry inflates back to the text and its CRC matches.
        let name_len = usize::from(u16::from_le_bytes([bytes[26], bytes[27]]));
        let size = u32::from_le_bytes(bytes[18..22].try_into().unwrap()) as usize;
        let data = &bytes[30 + name_len..30 + name_len + size];
        let mut inflated = String::new();
        flate2::read::DeflateDecoder::new(data)
            .read_to_string(&mut inflated)
            .unwrap();
        assert_eq!(inflated, text);
        assert_eq!(
            u32::from_le_bytes(bytes[14..18].try_into().unwrap()),
            crc32fast::hash(text.as_bytes())
        );
    }

    #[test]
    fn same_input_gives_same_bytes() {
        let build = || {
            let mut zip = ZipWriter::default();
            zip.add("a.xml", b"<a/>", Compression::Deflated);
            zip.finish()
        };
        assert_eq!(build(), build());
    }
}
