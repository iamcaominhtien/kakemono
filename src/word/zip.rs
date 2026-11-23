//! A minimal ZIP reader: just what a `.docx` package needs (entries stored
//! or deflated, no encryption, no ZIP64).
//!
//! The entries are found through the central directory at the end of the
//! archive, so data descriptors and junk before the archive do not matter.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;

/// Signature of the end of central directory record.
const END_OF_CENTRAL_DIR_SIG: u32 = 0x0605_4b50;
/// Signature of a central directory entry.
const CENTRAL_HEADER_SIG: u32 = 0x0201_4b50;
/// Signature of a local file header.
const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;
/// Size of the end of central directory record without its comment.
const END_RECORD_LEN: usize = 22;
/// Longest comment after the end record: the record is searched for this
/// far back from the end.
const MAX_COMMENT_LEN: usize = 0xFFFF;
/// Size of a central directory entry before its variable fields.
const CENTRAL_HEADER_LEN: usize = 46;
/// Size of a local file header before its variable fields.
const LOCAL_HEADER_LEN: usize = 30;
/// Compression method: stored.
const METHOD_STORED: u16 = 0;
/// Compression method: deflate.
const METHOD_DEFLATE: u16 = 8;
/// Largest size an entry may claim once extracted: a guard against
/// archives built to expand without end.
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

/// Where an entry's data is and how to extract it.
#[derive(Clone, Copy)]
struct EntryInfo {
    /// Offset of its local header from the start of the data.
    offset: usize,
    /// Size of its data in the archive.
    compressed: usize,
    /// Size once extracted.
    size: u64,
    /// Compression method.
    method: u16,
}

/// A ZIP archive in memory, read entry by entry.
pub(crate) struct ZipArchive<'a> {
    /// The bytes of the archive.
    data: &'a [u8],
    /// The entries by name.
    entries: HashMap<String, EntryInfo>,
}

impl<'a> ZipArchive<'a> {
    /// Reads the central directory of `data`.
    ///
    /// # Errors
    /// When `data` is not a ZIP archive, or its directory is cut short.
    pub(crate) fn new(data: &'a [u8]) -> Result<Self> {
        let end = find_end_record(data).context("not a ZIP archive (no end of central directory)")?;
        let count = usize::from(u16_at(data, end + 10)?);
        let mut pos = u32_at(data, end + 16)? as usize;
        let mut entries = HashMap::with_capacity(count);
        for _ in 0..count {
            if u32_at(data, pos)? != CENTRAL_HEADER_SIG {
                bail!("damaged ZIP archive: bad central directory entry");
            }
            let method = u16_at(data, pos + 10)?;
            let compressed = u32_at(data, pos + 20)? as usize;
            let size = u64::from(u32_at(data, pos + 24)?);
            let name_len = usize::from(u16_at(data, pos + 28)?);
            let extra_len = usize::from(u16_at(data, pos + 30)?);
            let comment_len = usize::from(u16_at(data, pos + 32)?);
            let offset = u32_at(data, pos + 42)? as usize;
            let name_start = pos + CENTRAL_HEADER_LEN;
            let name = data
                .get(name_start..name_start + name_len)
                .context("damaged ZIP archive: name cut short")?;
            let name = String::from_utf8_lossy(name).replace('\\', "/");
            entries.insert(
                name,
                EntryInfo {
                    offset,
                    compressed,
                    size,
                    method,
                },
            );
            pos = name_start + name_len + extra_len + comment_len;
        }
        Ok(Self { data, entries })
    }

    /// The names of the entries, in no order.
    #[cfg(test)]
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Whether the archive has an entry named `name`.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// The extracted contents of the entry `name` (a leading `/` and the
    /// case of the letters do not matter, as in an OPC package); `None`
    /// when there is no such entry.
    ///
    /// # Errors
    /// When the entry is damaged or uses an unknown compression method.
    pub(crate) fn read(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let Some(info) = self.find(name) else {
            return Ok(None);
        };
        if info.size > MAX_ENTRY_BYTES {
            bail!("{name}: entry too large ({} bytes)", info.size);
        }
        let header = info.offset;
        if u32_at(self.data, header)? != LOCAL_HEADER_SIG {
            bail!("{name}: damaged ZIP entry");
        }
        let name_len = usize::from(u16_at(self.data, header + 26)?);
        let extra_len = usize::from(u16_at(self.data, header + 28)?);
        let start = header + LOCAL_HEADER_LEN + name_len + extra_len;
        let body = self
            .data
            .get(start..start + info.compressed)
            .with_context(|| format!("{name}: ZIP entry cut short"))?;
        match info.method {
            METHOD_STORED => Ok(Some(body.to_vec())),
            METHOD_DEFLATE => {
                let mut out = Vec::with_capacity(info.size as usize);
                flate2::read::DeflateDecoder::new(body)
                    .take(MAX_ENTRY_BYTES)
                    .read_to_end(&mut out)
                    .with_context(|| format!("{name}: damaged compressed data"))?;
                Ok(Some(out))
            }
            other => bail!("{name}: unsupported ZIP compression method {other}"),
        }
    }

    /// The contents of the entry `name` as text (UTF-8, a byte order mark
    /// dropped); `None` when there is no such entry.
    ///
    /// # Errors
    /// As [`ZipArchive::read`].
    pub(crate) fn read_text(&self, name: &str) -> Result<Option<String>> {
        Ok(self.read(name)?.map(|bytes| {
            let text = String::from_utf8_lossy(&bytes).into_owned();
            text.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(text)
        }))
    }

    /// The entry named `name`, ignoring a leading `/` and letter case.
    fn find(&self, name: &str) -> Option<EntryInfo> {
        let name = name.trim_start_matches('/');
        self.entries.get(name).copied().or_else(|| {
            self.entries
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, info)| *info)
        })
    }
}

/// The position of the end of central directory record, searched from the
/// end of `data`.
fn find_end_record(data: &[u8]) -> Option<usize> {
    let last = data.len().checked_sub(END_RECORD_LEN)?;
    let first = last.saturating_sub(MAX_COMMENT_LEN);
    (first..=last)
        .rev()
        .find(|&pos| u32_at(data, pos).ok() == Some(END_OF_CENTRAL_DIR_SIG))
}

/// The little-endian `u16` at `pos`.
fn u16_at(data: &[u8], pos: usize) -> Result<u16> {
    let bytes = data.get(pos..pos + 2).context("damaged ZIP archive: cut short")?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// The little-endian `u32` at `pos`.
fn u32_at(data: &[u8], pos: usize) -> Result<u32> {
    let bytes = data.get(pos..pos + 4).context("damaged ZIP archive: cut short")?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::ZipArchive;
    use crate::render::zip::{Compression, ZipWriter};

    /// What the ZIP writer of the outputs stores, the reader gives back.
    #[test]
    fn reads_what_the_writer_wrote() {
        let mut zip = ZipWriter::default();
        zip.add("word/document.xml", b"<w:document/>", Compression::Deflated);
        zip.add("word/media/image1.png", &[1, 2, 3, 4], Compression::Stored);
        let data = zip.finish();
        let archive = ZipArchive::new(&data).unwrap();
        assert_eq!(archive.read("word/document.xml").unwrap().unwrap(), b"<w:document/>");
        assert_eq!(archive.read("/WORD/media/image1.png").unwrap().unwrap(), [1, 2, 3, 4]);
        assert!(archive.read("missing.xml").unwrap().is_none());
        assert!(archive.contains("word/document.xml"));
    }

    /// Bytes that are no ZIP archive are refused.
    #[test]
    fn refuses_other_data() {
        assert!(ZipArchive::new(b"%PDF-1.7 not a zip").is_err());
    }
}
