//! The style sheets of a book, read only for what gives a run of text its
//! style: bold, italics, monospace, raised or lowered text, small capitals,
//! and hidden elements. Many books (those made by Calibre, for one) set
//! italics with a class (`<span class="calibre3">`) rather than `<i>`.
//!
//! Only simple selectors are understood: `tag`, `.class`, `tag.class` (and
//! several classes); a rule with any other selector does not apply.

use crate::model::{BOLD, ITALIC, MONO, SMALL_CAPS, SUB, SUP};

/// Lightest numeric font weight that is bold.
const BOLD_WEIGHT: u32 = 600;

/// What a rule sets: for each style flag, set (`true`), cleared (`false`)
/// or left as inherited (absent).
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Props {
    /// Flags set or cleared: (flag, on).
    flags: Vec<(u8, bool)>,
    /// `display: none`.
    pub(crate) hidden: bool,
    /// A space on the right (`margin-right`, `padding-right`, a
    /// `min-width`): the text after the element is apart from it.
    pub(crate) gap: bool,
    /// `list-style: none`: the items of the list show no marker.
    pub(crate) no_markers: bool,
}

/// A rule with a simple selector.
#[derive(Debug, Clone)]
struct Rule {
    /// The element name, lower case; `None` for any element.
    tag: Option<String>,
    /// The classes the element must all have.
    classes: Vec<String>,
    /// What it sets.
    props: Props,
}

/// The rules of the style sheets of one content document, in order.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sheet {
    /// The rules, in the order of the sheets.
    rules: Vec<Rule>,
}

impl Props {
    /// `style` with these properties applied.
    pub(crate) fn apply(&self, mut style: u8) -> u8 {
        for &(flag, on) in &self.flags {
            if on {
                style |= flag;
            } else {
                style &= !flag;
            }
        }
        style
    }

    /// Adds what `other` sets, after (and so over) what this sets.
    fn extend(&mut self, other: &Props) {
        self.flags.extend_from_slice(&other.flags);
        self.hidden |= other.hidden;
        self.gap |= other.gap;
        self.no_markers |= other.no_markers;
    }
}

impl Sheet {
    /// Adds the rules of the style sheet `text`.
    pub(crate) fn add(&mut self, text: &str) {
        let text = without_comments(text);
        self.add_block(&text);
    }

    /// What the rules set for an element named `tag` with the classes
    /// `classes`, the more specific rules last; then its own `style`
    /// attribute.
    pub(crate) fn props(&self, tag: &str, classes: &[&str], inline: Option<&str>) -> Props {
        let mut matching: Vec<&Rule> = self
            .rules
            .iter()
            .filter(|rule| rule.tag.as_deref().is_none_or(|name| name == tag))
            .filter(|rule| rule.classes.iter().all(|class| classes.contains(&class.as_str())))
            .collect();
        // Specificity: classes first, then the element name; the sort is
        // stable, so rules of equal specificity keep their order.
        matching.sort_by_key(|rule| (rule.classes.len(), rule.tag.is_some()));
        let mut props = Props::default();
        for rule in matching {
            props.extend(&rule.props);
        }
        if let Some(inline) = inline {
            props.extend(&declarations(inline));
        }
        props
    }

    /// Adds the rules of `text`, a list of rules and at-rules; the rules
    /// inside `@media` and `@supports` blocks apply too.
    fn add_block(&mut self, text: &str) {
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let selector = rest[..open].trim();
            let Some(close) = matching_brace(&rest[open..]) else {
                return;
            };
            let body = &rest[open + 1..open + close];
            rest = &rest[open + close + 1..];
            // A statement at-rule (`@import ...;`) before the selector.
            let selector = selector.rsplit(';').next().unwrap_or(selector).trim();
            if let Some(at_rule) = selector.strip_prefix('@') {
                if at_rule.starts_with("media") || at_rule.starts_with("supports") {
                    self.add_block(body);
                }
                continue;
            }
            let props = declarations(body);
            if props == Props::default() {
                continue;
            }
            for part in selector.split(',') {
                if let Some((tag, classes)) = simple_selector(part.trim()) {
                    self.rules.push(Rule {
                        tag,
                        classes,
                        props: props.clone(),
                    });
                }
            }
        }
    }
}

/// `text` without its `/* ... */` comments.
fn without_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..]
            .find("*/")
            .map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

/// The offset of the brace that closes the one `text` starts with.
fn matching_brace(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (at, ch) in text.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The element name and classes of a simple selector (`p`, `.it`,
/// `span.it.b`); `None` for any other selector.
fn simple_selector(selector: &str) -> Option<(Option<String>, Vec<String>)> {
    let valid = |part: &str| {
        !part.is_empty()
            && part
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    };
    let mut parts = selector.split('.');
    let tag = parts.next()?;
    if !(tag.is_empty() || valid(tag)) {
        return None;
    }
    let classes: Vec<String> = parts.map(str::to_string).collect();
    if !classes.iter().all(|class| valid(class)) || (tag.is_empty() && classes.is_empty()) {
        return None;
    }
    let tag = (!tag.is_empty() && tag != "*").then(|| tag.to_ascii_lowercase());
    Some((tag, classes))
}

/// What the declarations `body` (`font-style: italic; ...`) set.
fn declarations(body: &str) -> Props {
    let mut props = Props::default();
    for declaration in body.split(';') {
        let Some((name, value)) = declaration.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.replace("!important", "").trim().to_ascii_lowercase();
        let flag = match name.as_str() {
            "font-weight" => Some((BOLD, font_weight_is_bold(&value))),
            "font-style" => Some((ITALIC, Some(value == "italic" || value == "oblique"))),
            "font-family" => Some((MONO, is_monospace(&value).then_some(true))),
            "font-variant" | "font-variant-caps" => Some((SMALL_CAPS, Some(value.contains("small-caps")))),
            "vertical-align" => match value.as_str() {
                "super" => Some((SUP, Some(true))),
                "sub" => Some((SUB, Some(true))),
                "baseline" => {
                    props.flags.push((SUP, false));
                    Some((SUB, Some(false)))
                }
                _ => None,
            },
            "display" => {
                props.hidden |= value == "none";
                None
            }
            "margin-right" | "padding-right" | "min-width" => {
                props.gap |= value.starts_with(|ch: char| ch.is_ascii_digit() || ch == '.')
                    && value
                        .trim_start_matches(['0', '.'])
                        .starts_with(|ch: char| ch.is_ascii_digit());
                None
            }
            "list-style" | "list-style-type" => {
                props.no_markers |= value.split_whitespace().any(|word| word == "none");
                None
            }
            _ => None,
        };
        if let Some((flag, Some(on))) = flag {
            props.flags.push((flag, on));
        }
    }
    props
}

/// Whether a `font-weight` is bold; `None` when the value is not known.
fn font_weight_is_bold(value: &str) -> Option<bool> {
    match value {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        _ => value.parse::<u32>().ok().map(|weight| weight >= BOLD_WEIGHT),
    }
}

/// Whether a `font-family` names a monospaced font. A custom property
/// (`var(--font-family-mono)`) names none: what it holds is not known, and
/// its name is the choice of a designer (TechCrunch sets its captions so).
fn is_monospace(value: &str) -> bool {
    !value.contains("var(") && (value.contains("monospace") || value.contains("courier") || value.contains("mono"))
}

#[cfg(test)]
mod tests {
    use super::Sheet;
    use crate::model::{BOLD, ITALIC};

    #[test]
    fn a_custom_property_names_no_monospaced_font() {
        assert!(super::is_monospace("\"courier new\", monospace"));
        assert!(!super::is_monospace("var(--wp--custom--font--family--mono)"));
    }

    #[test]
    fn classes_and_inline_styles_give_flags() {
        let mut sheet = Sheet::default();
        sheet.add(
            "/* c */ @import url(x.css); .it { font-style: italic } span.b, .x > y { font-weight: 700 }\n\
             @media print { .it { font-weight: bold } } .gone { display: none }",
        );
        assert_eq!(sheet.props("span", &["it"], None).apply(0), ITALIC | BOLD);
        assert_eq!(sheet.props("span", &["b"], None).apply(0), BOLD);
        assert_eq!(sheet.props("p", &["b"], None).apply(0), 0);
        assert_eq!(sheet.props("em", &[], Some("font-style: normal")).apply(ITALIC), 0);
        assert!(sheet.props("div", &["gone"], None).hidden);
        sheet.add(".no { margin-right: 0 } .yes { margin-right: .5em } ol.refs { list-style: none }");
        assert!(!sheet.props("span", &["no"], None).gap);
        assert!(sheet.props("span", &["yes"], None).gap);
        assert!(sheet.props("ol", &["refs"], None).no_markers);
    }
}
