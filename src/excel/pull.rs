//! A pull reader of XML: the start and end tags and the text of a part,
//! one at a time, without building a tree of the whole part. A worksheet
//! can hold millions of cells, and a tree of them takes many times the
//! size of the text.
//!
//! It reads what a SpreadsheetML part is made of: elements and their
//! attributes, text with the predefined and numeric entities, CDATA
//! sections, comments and processing instructions (both skipped). A
//! document type declaration (`<!DOCTYPE`) is refused, as
//! [`crate::word::docx::parse`] refuses it, and so is a part that is not
//! well-formed in the ways that matter here (an unclosed or mismatched
//! tag, an unknown entity).

use anyhow::{Context, Result, bail};
use std::borrow::Cow;

/// The longest entity reference read (`&#x10FFFF;`), in bytes, without
/// its `&`.
const MAX_ENTITY_LEN: usize = 10;

/// A piece of a part.
#[derive(Debug, PartialEq)]
pub(crate) enum Event<'a> {
    /// A start tag, or an empty element (`<c/>`, [`Tag::empty`]), which
    /// has no [`Event::End`].
    Start(Tag<'a>),
    /// An end tag; the byte offset just after it.
    End(usize),
    /// Text, its entities decoded; a run of text can come as several
    /// events (around a CDATA section or a comment).
    Text(Cow<'a, str>),
}

/// A start tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tag<'a> {
    /// The qualified name (`c`, `x:c`).
    pub(crate) name: &'a str,
    /// The attributes, as written.
    pub(crate) attrs: &'a str,
    /// An empty element (`<c/>`).
    pub(crate) empty: bool,
    /// The byte offset of its `<`.
    pub(crate) start: usize,
}

impl<'a> Tag<'a> {
    /// The namespace prefix of the name, empty when there is none.
    pub(crate) fn prefix(&self) -> &'a str {
        self.name.split_once(':').map_or("", |(prefix, _)| prefix)
    }

    /// The name without its prefix.
    pub(crate) fn local(&self) -> &'a str {
        self.name.split_once(':').map_or(self.name, |(_, local)| local)
    }

    /// The attributes: qualified name and decoded value.
    pub(crate) fn attributes(&self) -> Attributes<'a> {
        Attributes { rest: self.attrs }
    }

    /// The value of the attribute `name` (qualified name, as written).
    pub(crate) fn attr(&self, name: &str) -> Option<Cow<'a, str>> {
        self.attributes().find(|(key, _)| *key == name).map(|(_, value)| value)
    }
}

/// The attributes of a tag. A value with a damaged entity is given as
/// written; a damaged attribute ends the list.
pub(crate) struct Attributes<'a> {
    /// The attributes not read yet.
    rest: &'a str,
}

impl<'a> Iterator for Attributes<'a> {
    type Item = (&'a str, Cow<'a, str>);

    fn next(&mut self) -> Option<Self::Item> {
        let rest = self.rest.trim_start();
        self.rest = "";
        let (name, after) = rest.split_once('=')?;
        let after = after.trim_start();
        let quote = after.chars().next().filter(|ch| matches!(ch, '"' | '\''))?;
        let body = &after[1..];
        let close = body.find(quote)?;
        let raw = &body[..close];
        self.rest = &body[close + 1..];
        Some((name.trim(), decode(raw).unwrap_or(Cow::Borrowed(raw))))
    }
}

/// Reads a part one [`Event`] at a time.
pub(crate) struct Reader<'a> {
    /// The text of the part.
    xml: &'a str,
    /// Where the next event starts.
    pos: usize,
    /// The names of the elements open, outermost first.
    open: Vec<&'a str>,
    /// The root element has started.
    seen_root: bool,
}

impl<'a> Reader<'a> {
    /// A reader of `xml`.
    pub(crate) fn new(xml: &'a str) -> Self {
        Self {
            xml,
            pos: if xml.starts_with('\u{feff}') {
                '\u{feff}'.len_utf8()
            } else {
                0
            },
            open: Vec::new(),
            seen_root: false,
        }
    }

    /// The text of the part from byte `start` to byte `end`.
    pub(crate) fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.xml[start..end]
    }

    /// The next event; `None` at the end of a well-formed part.
    ///
    /// # Errors
    /// When the part is not well-formed, or has a document type
    /// declaration.
    pub(crate) fn next_event(&mut self) -> Result<Option<Event<'a>>> {
        loop {
            let rest = &self.xml[self.pos..];
            if rest.is_empty() {
                if !self.open.is_empty() || !self.seen_root {
                    bail!("damaged XML: the part ends inside an element");
                }
                return Ok(None);
            }
            if !rest.starts_with('<') {
                match self.text(rest)? {
                    Some(event) => return Ok(Some(event)),
                    None => continue,
                }
            }
            if self.skip_instruction_or_comment(rest)? {
                continue;
            }
            if let Some(body) = rest.strip_prefix("<![CDATA[") {
                return self.cdata(body).map(Some);
            }
            if rest.starts_with("<!") {
                bail!("damaged XML: a document type declaration is not allowed");
            }
            if let Some(body) = rest.strip_prefix("</") {
                return self.end_tag(body).map(Some);
            }
            let start = self.pos;
            return self.start_tag(rest, start).map(|tag| Some(Event::Start(tag)));
        }
    }

    /// The text at the start of `rest`, up to the next tag; `None` for
    /// white space outside the root element, which is skipped.
    fn text(&mut self, rest: &'a str) -> Result<Option<Event<'a>>> {
        let end = rest.find('<').unwrap_or(rest.len());
        let raw = &rest[..end];
        self.pos += end;
        if self.open.is_empty() {
            if raw.trim().is_empty() {
                return Ok(None);
            }
            bail!("damaged XML: text outside the root element");
        }
        Ok(Some(Event::Text(decode(raw)?)))
    }

    /// Skips the processing instruction or comment at the start of `rest`;
    /// `false` when there is none.
    fn skip_instruction_or_comment(&mut self, rest: &str) -> Result<bool> {
        if let Some(body) = rest.strip_prefix("<?") {
            let end = body
                .find("?>")
                .context("damaged XML: unclosed processing instruction")?;
            self.pos += "<?".len() + end + "?>".len();
            return Ok(true);
        }
        if let Some(body) = rest.strip_prefix("<!--") {
            let end = body.find("-->").context("damaged XML: unclosed comment")?;
            self.pos += "<!--".len() + end + "-->".len();
            return Ok(true);
        }
        Ok(false)
    }

    /// The CDATA section whose body starts `body`, as text.
    fn cdata(&mut self, body: &'a str) -> Result<Event<'a>> {
        let end = body.find("]]>").context("damaged XML: unclosed CDATA section")?;
        self.pos += "<![CDATA[".len() + end + "]]>".len();
        if self.open.is_empty() {
            bail!("damaged XML: CDATA outside the root element");
        }
        Ok(Event::Text(Cow::Borrowed(&body[..end])))
    }

    /// The end tag whose name starts `body`, which must close the open
    /// element.
    fn end_tag(&mut self, body: &str) -> Result<Event<'a>> {
        let end = body.find('>').context("damaged XML: unclosed end tag")?;
        let name = body[..end].trim_end();
        self.pos += "</".len() + end + 1;
        match self.open.pop() {
            Some(open) if open == name => Ok(Event::End(self.pos)),
            _ => bail!("damaged XML: mismatched end tag </{name}>"),
        }
    }

    /// Reads the start tag at the start of `rest`, at byte `start`.
    fn start_tag(&mut self, rest: &'a str, start: usize) -> Result<Tag<'a>> {
        let end = tag_end(rest).context("damaged XML: unclosed tag")?;
        self.pos += end + 1;
        let inner = &rest[1..end];
        let (inner, empty) = match inner.strip_suffix('/') {
            Some(inner) => (inner, true),
            None => (inner, false),
        };
        let name_end = inner.find(|ch: char| ch.is_ascii_whitespace()).unwrap_or(inner.len());
        let name = &inner[..name_end];
        if name.is_empty() {
            bail!("damaged XML: a tag without a name");
        }
        if self.open.is_empty() {
            if self.seen_root {
                bail!("damaged XML: a second root element");
            }
            self.seen_root = true;
        }
        if !empty {
            self.open.push(name);
        }
        Ok(Tag {
            name,
            attrs: &inner[name_end..],
            empty,
            start,
        })
    }
}

/// The offset of the `>` that closes the tag `tag` starts, outside the
/// quoted attribute values.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (at, byte) in tag.bytes().enumerate().skip(1) {
        match (quote, byte) {
            (None, b'"' | b'\'') => quote = Some(byte),
            (Some(open), _) if open == byte => quote = None,
            (None, b'>') => return Some(at),
            (None, b'<') => return None,
            _ => {}
        }
    }
    None
}

/// `raw` with its entity references decoded and its line ends made `\n`.
///
/// # Errors
/// When an entity is unknown or damaged.
pub(crate) fn decode(raw: &str) -> Result<Cow<'_, str>> {
    if !raw.contains(['&', '\r']) {
        return Ok(Cow::Borrowed(raw));
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        // Searched in bytes: the text after a short entity may start
        // with a character of several bytes.
        let end = after
            .bytes()
            .take(MAX_ENTITY_LEN + 1)
            .position(|byte| byte == b';')
            .context("damaged XML: an unclosed entity")?;
        let entity = &after[..end];
        let ch = match entity {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let code = if let Some(hex) = entity.strip_prefix("#x") {
                    u32::from_str_radix(hex, 16).ok()
                } else if let Some(decimal) = entity.strip_prefix('#') {
                    decimal.parse::<u32>().ok()
                } else {
                    None
                };
                code.and_then(char::from_u32)
                    .with_context(|| format!("damaged XML: unknown entity &{entity};"))?
            }
        };
        out.push(ch);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    if out.contains('\r') {
        out = out.replace("\r\n", "\n").replace('\r', "\n");
    }
    Ok(Cow::Owned(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every event of `xml`, written short: `<name/>` or `<name>`, `</>`,
    /// and the text.
    fn events(xml: &str) -> Result<Vec<String>> {
        let mut reader = Reader::new(xml);
        let mut out = Vec::new();
        while let Some(event) = reader.next_event()? {
            out.push(match event {
                Event::Start(tag) if tag.empty => format!("<{}/>", tag.name),
                Event::Start(tag) => format!("<{}>", tag.name),
                Event::End(_) => "</>".to_string(),
                Event::Text(text) => text.into_owned(),
            });
        }
        Ok(out)
    }

    #[test]
    fn tags_text_and_entities_are_read() {
        let xml = "\u{feff}<?xml version=\"1.0\"?>\n<a><!-- note --><b x=\"1 &amp; 2\"/>t&lt;&#65;&#x42;&quot;&apos;&gt;<![CDATA[<raw&>]]>\r\n</a>\n";
        assert_eq!(events(xml).unwrap(), ["<a>", "<b/>", "t<AB\"'>", "<raw&>", "\n", "</>"]);
    }

    #[test]
    fn attributes_are_decoded_and_may_hold_a_bracket() {
        let mut reader = Reader::new(r#"<x:c r = "A1" t='s' v="a>b&lt;" x:id="rId1"/>"#);
        let Some(Event::Start(tag)) = reader.next_event().unwrap() else {
            panic!("no start tag");
        };
        assert!(tag.empty);
        assert_eq!((tag.prefix(), tag.local()), ("x", "c"));
        assert_eq!(tag.attr("r").as_deref(), Some("A1"));
        assert_eq!(tag.attr("t").as_deref(), Some("s"));
        assert_eq!(tag.attr("v").as_deref(), Some("a>b<"));
        assert_eq!(tag.attr("x:id").as_deref(), Some("rId1"));
        assert_eq!(tag.attr("id"), None);
    }

    #[test]
    fn an_entity_before_east_asian_text_is_decoded() {
        assert_eq!(
            events("<a>A&amp;日本語のテキスト</a>").unwrap(),
            ["<a>", "A&日本語のテキスト", "</>"]
        );
        assert_eq!(decode("&lt;日").unwrap(), "<日");
    }

    #[test]
    fn damaged_parts_are_an_error() {
        for xml in [
            "<a",
            "<a>",
            "<a></b>",
            "<a>&nope;</a>",
            "<a>&amp</a>",
            "<a/><b/>",
            "<!DOCTYPE a [<!ENTITY e \"x\">]><a>&e;</a>",
            "<a><!-- x</a>",
            "",
            "text",
        ] {
            assert!(events(xml).is_err(), "{xml:?} was read");
        }
    }
}
