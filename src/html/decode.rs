//! The character encoding of a page, found as a browser finds it: its
//! byte order mark, the `charset` of its HTTP response, a `<meta>`
//! element near its start; otherwise UTF-8 when the bytes are UTF-8, and
//! Windows-1252, the default of the Web, when they are not.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};
use regex::bytes::Regex;
use std::sync::LazyLock;

/// How far into a page a `<meta>` naming its encoding is looked for, as
/// browsers do.
const META_SNIFF_BYTES: usize = 1024;

/// `<meta charset="...">`, or the `charset=` in the `content` of
/// `<meta http-equiv="Content-Type">`.
static META_CHARSET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<meta\b[^>]*?\bcharset\s*=\s*["']?\s*([A-Za-z0-9._:-]+)"#).expect("valid regex")
});

/// The text of the page `data`; `charset` is the one its HTTP response
/// names, if any.
pub(crate) fn decode(data: &[u8], charset: Option<&str>) -> String {
    let encoding = encoding(data, charset);
    let (text, _, _) = encoding.decode(data);
    text.into_owned()
}

/// The encoding of `data` (see the module).
fn encoding(data: &[u8], charset: Option<&str>) -> &'static Encoding {
    if let Some((encoding, _)) = Encoding::for_bom(data) {
        return encoding;
    }
    let label = charset.map(str::to_string).or_else(|| {
        let head = &data[..data.len().min(META_SNIFF_BYTES)];
        META_CHARSET
            .captures(head)
            .and_then(|caps| caps.get(1))
            .map(|label| String::from_utf8_lossy(label.as_bytes()).into_owned())
    });
    if let Some(encoding) = label.and_then(|label| Encoding::for_label(label.trim().as_bytes())) {
        // A page that says UTF-16 without a byte order mark is read as
        // UTF-8, as the HTML standard says.
        return if encoding == encoding_rs::UTF_16LE || encoding == encoding_rs::UTF_16BE {
            UTF_8
        } else {
            encoding
        };
    }
    if std::str::from_utf8(data).is_ok() {
        UTF_8
    } else {
        WINDOWS_1252
    }
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn encodings_come_from_the_page() {
        assert_eq!(decode("Hà Nội".as_bytes(), None), "Hà Nội");
        // Latin-1 bytes without any declaration: Windows-1252.
        assert_eq!(decode(b"caf\xe9 \x93q\x94", None), "café “q”");
        let page = b"<html><head><meta charset=\"iso-8859-15\"></head>\xa4";
        assert!(decode(page, None).ends_with('€'));
        let page = b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=Shift_JIS\">\x93\xfa\x96\x7b";
        assert!(decode(page, None).ends_with("日本"));
        // The HTTP header wins over the page.
        assert_eq!(
            decode(b"<meta charset=utf-8>\xe9", Some("windows-1252")),
            "<meta charset=utf-8>é"
        );
    }
}
