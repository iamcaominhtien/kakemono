//! The text of a shape (`p:txBody`) as paragraphs of `word::ir`: each
//! paragraph with a bullet or a number becomes an item of a list at its
//! level, the others plain paragraphs; runs keep their style and links;
//! fields show their text (a slide number field, the slide's number);
//! formulas (`a14:m`) keep their structure.

use super::inherit::level_props;
use crate::model::Span;
use crate::model::style::{BOLD, ITALIC, SUB, SUP};
use crate::word::docx::Rel;
use crate::word::ir::{Inline, ListMark, Para};
use crate::word::omml;
use crate::word::xml::{DRAWING_NS, MATH_NS, REL_NS, attr, child, children};
use roxmltree::Node;
use std::collections::HashMap;

/// Namespace of the Office 2010 drawing extensions (`a14:`), which hold a
/// paragraph's formulas.
const A14_NS: &str = "http://schemas.microsoft.com/office/drawing/2010/main";
/// The deepest level a paragraph can have (`lvl` 0 to 8).
const LEVELS: usize = 9;

/// What the text of a slide needs besides its XML.
pub(crate) struct TextContext<'r> {
    /// The slide's number in the deck, for slide number fields.
    pub(crate) number: usize,
    /// The slide's relationships, for links.
    pub(crate) rels: &'r HashMap<String, Rel>,
}

/// What marks a paragraph, from the first of its level properties that
/// says.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Bullet {
    /// None (`a:buNone`, or nothing said).
    None,
    /// A character or a picture (`a:buChar`, `a:buBlip`).
    Symbol,
    /// A number (`a:buAutoNum`): its scheme and the number of the first
    /// paragraph.
    Number(String, usize),
}

/// The paragraphs of the text body `body` (`p:txBody`, `a:txBody`), whose
/// levels take their properties from `styles` (most specific first, see
/// `inherit::Inheritance::list_styles`). Empty paragraphs are left out.
pub(crate) fn paragraphs(body: Node<'_, '_>, styles: &[Node<'_, '_>], context: &TextContext<'_>) -> Vec<Para> {
    let mut numbers = Numbers::default();
    let mut out = Vec::new();
    for para in children(body, DRAWING_NS, "p") {
        let props = child(para, DRAWING_NS, "pPr");
        let level = props
            .and_then(|props| props.attribute("lvl"))
            .and_then(|lvl| lvl.parse::<usize>().ok())
            .unwrap_or(0)
            .min(LEVELS - 1);
        let chain = level_props(props, styles, level as u8);
        let inlines = runs(para, &chain, context);
        if inlines_text(&inlines).trim().is_empty()
            && !inlines.iter().any(|inline| matches!(inline, Inline::Math { .. }))
        {
            continue;
        }
        let list = match bullet(&chain) {
            Bullet::None => {
                numbers.plain(level);
                None
            }
            Bullet::Symbol => {
                numbers.plain(level);
                Some(ListMark {
                    level: level as u8,
                    marker: None,
                })
            }
            Bullet::Number(scheme, start) => Some(ListMark {
                level: level as u8,
                marker: Some(numbers.next(level, &scheme, start)),
            }),
        };
        out.push(Para {
            style: if list.is_some() {
                "List Paragraph".to_string()
            } else {
                String::new()
            },
            list,
            inlines,
            ..Default::default()
        });
    }
    out
}

/// The plain text of `inlines`.
pub(crate) fn inlines_text(inlines: &[Inline]) -> String {
    inlines
        .iter()
        .map(|inline| match inline {
            Inline::Text(span) => span.text.as_str(),
            _ => "",
        })
        .collect()
}

/// The numbers of a text body's numbered paragraphs, level by level: a
/// number continues the one before at its level when the scheme is the
/// same, across deeper paragraphs; a paragraph without a number at a level
/// ends the numbering there, and any paragraph ends the deeper ones'.
#[derive(Default)]
struct Numbers {
    /// The scheme and last number of each level.
    levels: [Option<(String, usize)>; LEVELS],
}

impl Numbers {
    /// The marker of a numbered paragraph at `level`.
    fn next(&mut self, level: usize, scheme: &str, start: usize) -> String {
        self.end_deeper(level);
        let number = match &self.levels[level] {
            Some((last_scheme, last)) if last_scheme == scheme => last + 1,
            _ => start,
        };
        self.levels[level] = Some((scheme.to_string(), number));
        marker(scheme, number)
    }

    /// A paragraph without a number at `level`.
    fn plain(&mut self, level: usize) {
        self.end_deeper(level);
        self.levels[level] = None;
    }

    /// Ends the numbering of the levels deeper than `level`.
    fn end_deeper(&mut self, level: usize) {
        for deeper in &mut self.levels[level + 1..] {
            *deeper = None;
        }
    }
}

/// The bullet the first of `chain` that says gives.
fn bullet(chain: &[Node<'_, '_>]) -> Bullet {
    for props in chain {
        for item in props.children().filter(Node::is_element) {
            match item.tag_name().name() {
                "buNone" => return Bullet::None,
                "buChar" | "buBlip" => return Bullet::Symbol,
                "buAutoNum" => {
                    let scheme = item.attribute("type").unwrap_or("arabicPeriod").to_string();
                    let start = item
                        .attribute("startAt")
                        .and_then(|value| value.parse().ok())
                        .filter(|start| *start > 0)
                        .unwrap_or(1);
                    return Bullet::Number(scheme, start);
                }
                _ => {}
            }
        }
    }
    Bullet::None
}

/// The text of the number `number` in the scheme `scheme` (`arabicPeriod`
/// "3.", `alphaLcParenR` "c)", `romanUcParenBoth` "(III)"); a scheme of
/// another script is written with Arabic numerals.
fn marker(scheme: &str, number: usize) -> String {
    let digits = if scheme.starts_with("alphaLc") {
        letters(number)
    } else if scheme.starts_with("alphaUc") {
        letters(number).to_uppercase()
    } else if scheme.starts_with("romanLc") {
        crate::word::roman_lower(number)
    } else if scheme.starts_with("romanUc") {
        crate::word::roman_lower(number).to_uppercase()
    } else {
        number.to_string()
    };
    if scheme.ends_with("ParenBoth") {
        format!("({digits})")
    } else if scheme.ends_with("ParenR") {
        format!("{digits})")
    } else if scheme.ends_with("Plain") {
        digits
    } else {
        format!("{digits}.")
    }
}

/// The letters of `number` as a list counts them: a … z, then aa … zz,
/// aaa …, as PowerPoint and Word do.
fn letters(number: usize) -> String {
    let number = number.max(1);
    let letter = char::from(b'a' + ((number - 1) % 26) as u8);
    std::iter::repeat_n(letter, (number - 1) / 26 + 1).collect()
}

/// The runs, line breaks and fields of the paragraph `para`, as spans.
fn runs(para: Node<'_, '_>, chain: &[Node<'_, '_>], context: &TextContext<'_>) -> Vec<Inline> {
    let defaults = chain
        .iter()
        .filter_map(|props| child(*props, DRAWING_NS, "defRPr"))
        .collect::<Vec<_>>();
    let mut out: Vec<Inline> = Vec::new();
    for item in para.children().filter(Node::is_element) {
        // A formula (`a14:m`, read in the choice of an `mc:AlternateContent`).
        if item.tag_name().namespace() == Some(A14_NS) && item.tag_name().name() == "m" {
            out.extend(
                item.children()
                    .filter(|node| node.tag_name().namespace() == Some(MATH_NS))
                    .map(|node| Inline::Math {
                        node: omml::formula(node),
                        display: node.tag_name().name() == "oMathPara",
                    }),
            );
            continue;
        }
        let text = match item.tag_name().name() {
            // A tab separates words, as in a Word document.
            "r" => child(item, DRAWING_NS, "t")
                .and_then(|node| node.text())
                .unwrap_or_default()
                .replace('\t', " "),
            "fld" => field_text(item, context),
            "br" => "\n".to_string(),
            _ => continue,
        };
        if text.is_empty() {
            continue;
        }
        let props = child(item, DRAWING_NS, "rPr");
        let span = Span {
            href: props.and_then(|props| link(props, context)),
            ..Span::styled(text, run_style(props, &defaults))
        };
        // Neighbouring runs of one style and link make one span.
        match out.last_mut() {
            Some(Inline::Text(last)) if last.style == span.style && last.href == span.href => {
                last.text.push_str(&span.text);
            }
            _ => out.push(Inline::Text(span)),
        }
    }
    out
}

/// The text a field shows: the slide's number for a slide number field,
/// else the text PowerPoint stored (a date as it was last shown).
fn field_text(field: Node<'_, '_>, context: &TextContext<'_>) -> String {
    if field.attribute("type") == Some("slidenum") {
        return context.number.to_string();
    }
    child(field, DRAWING_NS, "t")
        .and_then(|node| node.text())
        .unwrap_or_default()
        .to_string()
}

/// The style bits of a run with the properties `props`, bold and italic
/// inherited from the level's `defaults` when the run does not say.
fn run_style(props: Option<Node<'_, '_>>, defaults: &[Node<'_, '_>]) -> u8 {
    let flag = |name: &str| {
        props
            .into_iter()
            .chain(defaults.iter().copied())
            .find_map(|node| node.attribute(name))
            .is_some_and(|value| matches!(value, "1" | "true"))
    };
    let mut style = 0;
    if flag("b") {
        style |= BOLD;
    }
    if flag("i") {
        style |= ITALIC;
    }
    let baseline = props
        .and_then(|props| props.attribute("baseline"))
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(0);
    if baseline > 0 {
        style |= SUP;
    } else if baseline < 0 {
        style |= SUB;
    }
    style
}

/// The target of a run's link (`a:hlinkClick r:id`): an address outside
/// the deck. A link to another slide or an action has none.
fn link(props: Node<'_, '_>, context: &TextContext<'_>) -> Option<String> {
    let click = child(props, DRAWING_NS, "hlinkClick")?;
    let rel = context.rels.get(attr(click, REL_NS, "id")?)?;
    rel.external.then(|| rel.target.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::word::docx::parse;

    const NS: &str = "xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                      xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";

    /// The paragraphs of a text body of `paras`, with the list style `style`.
    fn read(paras: &str, style: &str) -> Vec<Para> {
        let xml = format!("<a:txBody {NS}><a:bodyPr/><a:lstStyle>{style}</a:lstStyle>{paras}</a:txBody>");
        let doc = parse(&xml).unwrap();
        let body = doc.root_element();
        let styles: Vec<Node<'_, '_>> = child(body, DRAWING_NS, "lstStyle").into_iter().collect();
        let mut rels = HashMap::new();
        rels.insert(
            "rId1".to_string(),
            Rel {
                target: "https://example.org/".to_string(),
                external: true,
            },
        );
        paragraphs(body, &styles, &TextContext { number: 7, rels: &rels })
    }

    fn markers(paras: &[Para]) -> Vec<Option<(u8, Option<String>)>> {
        paras
            .iter()
            .map(|para| para.list.as_ref().map(|list| (list.level, list.marker.clone())))
            .collect()
    }

    fn numbered(text: &str, kind: &str, lvl: u8, start: Option<u8>) -> String {
        let start = start.map(|start| format!(" startAt=\"{start}\"")).unwrap_or_default();
        format!(
            "<a:p><a:pPr lvl=\"{lvl}\"><a:buAutoNum type=\"{kind}\"{start}/></a:pPr><a:r><a:t>{text}</a:t></a:r></a:p>"
        )
    }

    #[test]
    fn bullet_levels_become_list_levels() {
        let style = "<a:lvl1pPr><a:buChar char=\"•\"/></a:lvl1pPr><a:lvl2pPr><a:buChar char=\"–\"/></a:lvl2pPr>";
        let paras = read(
            "<a:p><a:r><a:t>One</a:t></a:r></a:p><a:p><a:pPr lvl=\"1\"/><a:r><a:t>Two</a:t></a:r></a:p>\
             <a:p><a:pPr lvl=\"2\"/><a:r><a:t>Three</a:t></a:r></a:p>\
             <a:p><a:pPr><a:buNone/></a:pPr><a:r><a:t>Plain</a:t></a:r></a:p><a:p/>",
            style,
        );
        assert_eq!(
            markers(&paras),
            [Some((0, None)), Some((1, None)), None, None],
            "level 3 has no bullet in this style; the empty paragraph is left out"
        );
    }

    #[test]
    fn auto_numbers_keep_their_marker() {
        let paras = read(
            &[
                numbered("a", "arabicPeriod", 0, Some(3)),
                numbered("b", "alphaLcParenR", 1, None),
                numbered("c", "alphaLcParenR", 1, None),
                numbered("d", "arabicPeriod", 0, Some(3)),
                numbered("e", "alphaLcParenR", 1, None),
                numbered("f", "romanUcParenBoth", 0, None),
            ]
            .concat(),
            "",
        );
        let found: Vec<String> = paras
            .iter()
            .map(|para| para.list.as_ref().unwrap().marker.clone().unwrap())
            .collect();
        assert_eq!(found, ["3.", "a)", "b)", "4.", "a)", "(I)"]);
    }

    #[test]
    fn markers_of_large_numbers() {
        assert_eq!(marker("alphaUcPeriod", 28), "BB.");
        assert_eq!(marker("romanLcPlain", 14), "xiv");
        assert_eq!(marker("circleNumDbPlain", 5), "5");
    }

    #[test]
    fn runs_keep_style_links_and_fields() {
        let paras = read(
            "<a:p><a:r><a:rPr b=\"1\"/><a:t>Bo\tld</a:t></a:r><a:r><a:rPr baseline=\"30000\"/><a:t>2</a:t></a:r>\
             <a:br/><a:r><a:rPr><a:hlinkClick r:id=\"rId1\"/></a:rPr><a:t>link</a:t></a:r>\
             <a:fld type=\"slidenum\"><a:t>‹#›</a:t></a:fld></a:p>",
            "<a:lvl1pPr><a:defRPr i=\"1\"/></a:lvl1pPr>",
        );
        let spans: Vec<(String, u8, Option<String>)> = paras[0]
            .inlines
            .iter()
            .map(|inline| match inline {
                Inline::Text(span) => (span.text.clone(), span.style, span.href.clone()),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(
            spans,
            [
                ("Bo ld".to_string(), BOLD | ITALIC, None),
                ("2".to_string(), ITALIC | SUP, None),
                ("\n".to_string(), ITALIC, None),
                ("link".to_string(), ITALIC, Some("https://example.org/".to_string())),
                ("7".to_string(), ITALIC, None),
            ]
        );
    }
}
