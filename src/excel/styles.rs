//! The cell styles of a workbook (`xl/styles.xml`): for each cell format
//! (`cellXfs`), the number format, whether its font is bold or italic, and
//! whether it centres across the selection (`centerContinuous`).

use crate::word::docx::parse;
use anyhow::{Context, Result};
use roxmltree::Node;
use std::collections::HashMap;

use super::numfmt::{NumFmt, builtin};
use super::workbook::is_sml;

/// The cell formats of a workbook, by the index a cell's `s` gives.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub(crate) struct Styles {
    /// The cell formats (`cellXfs`), in order.
    xfs: Vec<Xf>,
}

/// A cell format: what of it shows in the text.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Xf {
    /// The number format.
    pub(crate) num_fmt: NumFmt,
    /// The font is bold.
    pub(crate) bold: bool,
    /// The font is italic.
    pub(crate) italic: bool,
    /// The text is centred across the selection (`centerContinuous`): over
    /// the empty cells to its right that have the same alignment.
    pub(crate) center_continuous: bool,
}

/// The format of a cell without a style.
static DEFAULT_XF: Xf = Xf {
    num_fmt: NumFmt::General,
    bold: false,
    italic: false,
    center_continuous: false,
};

#[cfg_attr(not(test), allow(dead_code))]
impl Styles {
    /// The cell format of index `index` (a cell's `s`); the default one
    /// when there is none.
    pub(crate) fn get(&self, index: usize) -> &Xf {
        self.xfs.get(index).unwrap_or(&DEFAULT_XF)
    }
}

/// The cell formats of the styles part `xml`.
///
/// # Errors
/// When `xml` is not well-formed.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn read(xml: &str) -> Result<Styles> {
    let doc = parse(xml).context("reading the styles")?;
    let root = doc.root_element();
    let child = |name: &str| root.children().find(|node| is_sml(*node, name));
    // Custom number formats, by id; they may also replace a built-in one.
    let custom: HashMap<u32, String> = items(child("numFmts"), "numFmt")
        .filter_map(|node| {
            Some((
                node.attribute("numFmtId")?.parse().ok()?,
                node.attribute("formatCode")?.to_string(),
            ))
        })
        .collect();
    let fonts: Vec<(bool, bool)> = items(child("fonts"), "font").map(font_style).collect();
    let xfs = items(child("cellXfs"), "xf")
        .map(|xf| {
            let id = |name| {
                xf.attribute(name)
                    .and_then(|value| value.parse::<u32>().ok())
                    .unwrap_or(0)
            };
            let num_fmt_id = id("numFmtId");
            let num_fmt = match custom.get(&num_fmt_id) {
                Some(code) if code.eq_ignore_ascii_case("general") => NumFmt::General,
                Some(code) => NumFmt::Code(code.clone()),
                None => builtin(num_fmt_id),
            };
            let (bold, italic) = usize::try_from(id("fontId"))
                .ok()
                .and_then(|font| fonts.get(font))
                .copied()
                .unwrap_or_default();
            let center_continuous = xf
                .children()
                .find(|node| is_sml(*node, "alignment"))
                .is_some_and(|alignment| alignment.attribute("horizontal") == Some("centerContinuous"));
            Xf {
                num_fmt,
                bold,
                italic,
                center_continuous,
            }
        })
        .collect();
    Ok(Styles { xfs })
}

/// The child elements `name` of `list`, none when there is no `list`.
fn items<'a, 'input>(list: Option<Node<'a, 'input>>, name: &'static str) -> impl Iterator<Item = Node<'a, 'input>> {
    list.into_iter()
        .flat_map(|list| list.children())
        .filter(move |node| is_sml(*node, name))
}

/// Whether a font (`font`) is bold, and whether it is italic.
fn font_style(font: Node<'_, '_>) -> (bool, bool) {
    let on = |name| {
        font.children()
            .find(|node| is_sml(*node, name))
            .is_some_and(|node| !matches!(node.attribute("val"), Some("0" | "false" | "off")))
    };
    (on("b"), on("i"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `body` as a styles part.
    fn styles(body: &str) -> Styles {
        read(&format!(
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{body}</styleSheet>"#
        ))
        .unwrap()
    }

    #[test]
    fn cell_formats_have_their_number_format_and_font() {
        let styles = styles(concat!(
            r#"<numFmts count="2"><numFmt numFmtId="164" formatCode="&quot;¥&quot;#,##0"/><numFmt numFmtId="3" formatCode="0.0"/></numFmts>"#,
            r#"<fonts count="3"><font><sz val="11"/></font><font><b/><i val="1"/></font><font><b val="0"/><i/></font></fonts>"#,
            r#"<cellXfs count="5"><xf numFmtId="0" fontId="0"/><xf numFmtId="164" fontId="1"/><xf numFmtId="14" fontId="2"/>"#,
            r#"<xf numFmtId="3" fontId="9"><alignment horizontal="centerContinuous"/></xf><xf numFmtId="7"><alignment horizontal="center"/></xf></cellXfs>"#,
        ));
        assert_eq!(*styles.get(0), Xf::default());
        let xf = styles.get(1);
        assert_eq!(xf.num_fmt, NumFmt::Code("\"¥\"#,##0".into()));
        assert!(xf.bold && xf.italic && !xf.center_continuous);
        let xf = styles.get(2);
        assert_eq!(xf.num_fmt, builtin(14));
        assert!(!xf.bold && xf.italic);
        // A custom code replaces the built-in one of its id; a missing font
        // is the default one.
        let xf = styles.get(3);
        assert_eq!(xf.num_fmt, NumFmt::Code("0.0".into()));
        assert!(xf.center_continuous && !xf.bold);
        assert!(!styles.get(4).center_continuous);
        assert_eq!(styles.get(4).num_fmt, builtin(7));
    }

    #[test]
    fn an_index_out_of_range_is_the_default_format() {
        let styles = styles(r#"<cellXfs count="1"><xf numFmtId="10"/></cellXfs>"#);
        assert_eq!(styles.get(0).num_fmt, builtin(10));
        assert_eq!(*styles.get(7), Xf::default());
        assert_eq!(*Styles::default().get(0), Xf::default());
    }

    #[test]
    fn strict_namespace_is_read() {
        let xml = r#"<styleSheet xmlns="http://purl.oclc.org/ooxml/spreadsheetml/main"><fonts><font><b/></font></fonts><cellXfs><xf numFmtId="9" fontId="0"/></cellXfs></styleSheet>"#;
        let styles = read(xml).unwrap();
        assert!(styles.get(0).bold);
        assert_eq!(styles.get(0).num_fmt, builtin(9));
    }

    #[test]
    fn damaged_styles_are_an_error() {
        assert!(read("<styleSheet").is_err());
    }
}
