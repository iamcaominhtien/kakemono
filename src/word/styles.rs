//! Styles (`styles.xml`) and list numbering (`numbering.xml`) of a
//! `.docx`: what a paragraph or run inherits from its style, and the
//! markers of numbered paragraphs.

use super::xml::{WORD_NS, attr, child, children, toggle, val};
use crate::model::{BOLD, ITALIC, MONO, SUB, SUP};
use roxmltree::Node;
use std::collections::HashMap;

/// Font size of the normal text when the document sets none, in points
/// (Word's own default).
pub(crate) const DEFAULT_BODY_SIZE_PT: f32 = 11.0;
/// Deepest chain of `w:basedOn` followed: a guard against loops.
const MAX_STYLE_DEPTH: usize = 16;
/// Word font sizes are in half points.
const HALF_POINTS_PER_PT: f32 = 2.0;
/// Words in the name of a monospaced font (lowercase).
const MONO_FONT_WORDS: &[&str] = &[
    "courier",
    "consolas",
    "mono",
    "menlo",
    "monaco",
    "lucida console",
    "source code",
    "fira code",
    "inconsolata",
    "typewriter",
];
/// Highest list level Word has (levels 0 to 8).
const MAX_LIST_LEVEL: usize = 9;

/// Text properties, each `None` when not set at this level.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct RunProps {
    /// Bold.
    pub(crate) bold: Option<bool>,
    /// Italic.
    pub(crate) italic: Option<bool>,
    /// Raised (`Some(true)`), lowered (`Some(false)`) or on the baseline.
    pub(crate) raised: Option<Option<bool>>,
    /// In a monospaced font.
    pub(crate) mono: Option<bool>,
    /// In the Symbol font, whose codes are not Unicode.
    pub(crate) symbol: Option<bool>,
    /// Text colour.
    pub(crate) color: Option<[u8; 3]>,
    /// Hidden text.
    pub(crate) hidden: Option<bool>,
}

impl RunProps {
    /// The properties set in a `w:rPr` element.
    pub(crate) fn from_node(props: Node<'_, '_>) -> Self {
        let font = child(props, WORD_NS, "rFonts")
            .and_then(|fonts| attr(fonts, WORD_NS, "ascii").or_else(|| attr(fonts, WORD_NS, "hAnsi")));
        let raised = val(props, "vertAlign").map(|align| match align {
            "superscript" => Some(true),
            "subscript" => Some(false),
            _ => None,
        });
        Self {
            bold: toggle(props, "b"),
            italic: toggle(props, "i"),
            raised,
            mono: font.map(is_mono_font),
            symbol: font.map(|font| font.eq_ignore_ascii_case("Symbol")),
            color: val(props, "color").and_then(parse_color),
            hidden: toggle(props, "vanish"),
        }
    }

    /// These properties with those set in `over` replacing them.
    pub(crate) fn with(self, over: &RunProps) -> Self {
        Self {
            bold: over.bold.or(self.bold),
            italic: over.italic.or(self.italic),
            raised: over.raised.or(self.raised),
            mono: over.mono.or(self.mono),
            symbol: over.symbol.or(self.symbol),
            color: over.color.or(self.color),
            hidden: over.hidden.or(self.hidden),
        }
    }

    /// The style flags of [`Span::style`](crate::model::Span::style).
    pub(crate) fn style_bits(&self) -> u8 {
        let mut bits = 0;
        if self.bold == Some(true) {
            bits |= BOLD;
        }
        if self.italic == Some(true) {
            bits |= ITALIC;
        }
        if self.mono == Some(true) {
            bits |= MONO;
        }
        match self.raised {
            Some(Some(true)) => bits |= SUP,
            Some(Some(false)) => bits |= SUB,
            _ => {}
        }
        bits
    }
}

/// One style of the document.
#[derive(Debug, Default)]
pub(crate) struct StyleDef {
    /// Its name as Word shows it.
    pub(crate) name: String,
    /// The style it is based on.
    pub(crate) based_on: Option<String>,
    /// Outline level of its paragraphs.
    pub(crate) outline: Option<u8>,
    /// List numbering of its paragraphs: (`numId`, level).
    pub(crate) numbering: Option<(String, u8)>,
    /// Its text properties.
    pub(crate) run: RunProps,
}

/// The styles of a document.
#[derive(Debug, Default)]
pub(crate) struct Styles {
    /// The styles by id.
    by_id: HashMap<String, StyleDef>,
    /// The default paragraph style.
    default_para: Option<String>,
    /// The text properties of the document defaults.
    defaults: RunProps,
    /// Font size of the normal text, in points.
    pub(crate) body_size_pt: f32,
    /// Font of the normal text.
    pub(crate) body_font: String,
}

impl Styles {
    /// Reads `styles.xml` (its root element `w:styles`); `None` gives the
    /// styles of a document that has none.
    pub(crate) fn parse(root: Option<Node<'_, '_>>) -> Self {
        let mut styles = Styles {
            body_size_pt: DEFAULT_BODY_SIZE_PT,
            ..Default::default()
        };
        let Some(root) = root else {
            return styles;
        };
        let mut default_size = None;
        if let Some(props) = child(root, WORD_NS, "docDefaults")
            .and_then(|defaults| child(defaults, WORD_NS, "rPrDefault"))
            .and_then(|default| child(default, WORD_NS, "rPr"))
        {
            styles.defaults = RunProps::from_node(props);
            default_size = font_size(props);
            styles.body_font = font_name(props).unwrap_or_default();
        }
        for node in children(root, WORD_NS, "style") {
            let Some(id) = attr(node, WORD_NS, "styleId") else {
                continue;
            };
            let para_props = child(node, WORD_NS, "pPr");
            let def = StyleDef {
                name: val(node, "name").unwrap_or(id).to_string(),
                based_on: val(node, "basedOn").map(str::to_string),
                outline: para_props.and_then(outline_level),
                numbering: para_props.and_then(numbering_ref),
                run: child(node, WORD_NS, "rPr").map(RunProps::from_node).unwrap_or_default(),
            };
            let is_default = matches!(attr(node, WORD_NS, "default"), Some("1" | "true" | "on"));
            if is_default && attr(node, WORD_NS, "type") == Some("paragraph") {
                styles.default_para = Some(id.to_string());
                if let Some(props) = child(node, WORD_NS, "rPr") {
                    default_size = font_size(props).or(default_size);
                    if let Some(font) = font_name(props) {
                        styles.body_font = font;
                    }
                }
            }
            styles.by_id.insert(id.to_string(), def);
        }
        styles.body_size_pt = default_size.unwrap_or(DEFAULT_BODY_SIZE_PT);
        styles
    }

    /// Styles to be filled with [`Styles::insert`] (a `.doc` file), with
    /// the size and font of the normal text.
    pub(crate) fn new(body_size_pt: f32, body_font: String) -> Self {
        Styles {
            body_size_pt,
            body_font,
            ..Default::default()
        }
    }

    /// Adds the style `id`; `default` makes it the default paragraph style.
    pub(crate) fn insert(&mut self, id: String, def: StyleDef, default: bool) {
        if default {
            self.default_para = Some(id.clone());
        }
        self.by_id.insert(id, def);
    }

    /// The style of a paragraph with style id `id` (`None` for the
    /// default style): its name, outline level and numbering.
    pub(crate) fn paragraph(&self, id: Option<&str>) -> (String, Option<u8>, Option<(String, u8)>) {
        let id = id.or(self.default_para.as_deref());
        let chain = self.chain(id);
        let name = chain.first().map(|def| def.name.clone()).unwrap_or_default();
        let outline = chain.iter().find_map(|def| def.outline);
        let numbering = chain.iter().find_map(|def| def.numbering.clone());
        (name, outline, numbering)
    }

    /// The text properties of a run of style `run_style` in a paragraph of
    /// style `para_style`, before its own properties.
    pub(crate) fn run_props(&self, para_style: Option<&str>, run_style: Option<&str>) -> RunProps {
        let mut props = self.defaults;
        let para_style = para_style.or(self.default_para.as_deref());
        for def in self.chain(para_style).iter().rev() {
            props = props.with(&def.run);
        }
        for def in self.chain(run_style).iter().rev() {
            props = props.with(&def.run);
        }
        props
    }

    /// The style `id` and the styles it is based on, nearest first.
    fn chain(&self, id: Option<&str>) -> Vec<&StyleDef> {
        let mut chain = Vec::new();
        let mut next = id;
        while let Some(id) = next {
            let Some(def) = self.by_id.get(id) else { break };
            chain.push(def);
            if chain.len() >= MAX_STYLE_DEPTH {
                break;
            }
            next = def.based_on.as_deref();
        }
        chain
    }
}

/// One level of a list definition.
#[derive(Debug, Clone, Default)]
pub(crate) struct Level {
    /// Its first number.
    pub(crate) start: u32,
    /// How its number is written (`decimal`, `lowerRoman`, `bullet`, ...).
    pub(crate) format: String,
    /// The marker with `%1`, `%2`, ... for the numbers of the levels.
    pub(crate) text: String,
}

/// The list definitions of a document and the counters of its lists.
#[derive(Debug, Default)]
pub(crate) struct Numbering {
    /// The levels of each abstract list definition, by its id.
    abstracts: HashMap<String, Vec<Level>>,
    /// For each list (`numId`): its abstract definition and the levels
    /// whose start it overrides.
    lists: HashMap<String, (String, HashMap<u8, u32>)>,
    /// The current number of each level of each abstract list definition:
    /// the lists of one definition number on from one another, as in Word.
    counters: HashMap<String, [Option<u32>; MAX_LIST_LEVEL]>,
    /// The lists already counted from (whose start overrides were applied).
    started: std::collections::HashSet<String>,
}

impl Numbering {
    /// Reads `numbering.xml` (its root element `w:numbering`).
    pub(crate) fn parse(root: Option<Node<'_, '_>>) -> Self {
        let mut numbering = Numbering::default();
        let Some(root) = root else {
            return numbering;
        };
        for node in children(root, WORD_NS, "abstractNum") {
            let Some(id) = attr(node, WORD_NS, "abstractNumId") else {
                continue;
            };
            let mut levels = vec![Level::default(); MAX_LIST_LEVEL];
            for level in children(node, WORD_NS, "lvl") {
                let Some(index) = attr(level, WORD_NS, "ilvl").and_then(|ilvl| ilvl.parse::<usize>().ok()) else {
                    continue;
                };
                if let Some(slot) = levels.get_mut(index) {
                    *slot = Level {
                        start: val(level, "start").and_then(|start| start.parse().ok()).unwrap_or(1),
                        format: val(level, "numFmt").unwrap_or("decimal").to_string(),
                        text: val(level, "lvlText").unwrap_or_default().to_string(),
                    };
                }
            }
            numbering.abstracts.insert(id.to_string(), levels);
        }
        for node in children(root, WORD_NS, "num") {
            let (Some(id), Some(abstract_id)) = (attr(node, WORD_NS, "numId"), val(node, "abstractNumId")) else {
                continue;
            };
            let overrides = children(node, WORD_NS, "lvlOverride")
                .filter_map(|over| {
                    let level = attr(over, WORD_NS, "ilvl")?.parse().ok()?;
                    let start = val(over, "startOverride")?.parse().ok()?;
                    Some((level, start))
                })
                .collect();
            numbering
                .lists
                .insert(id.to_string(), (abstract_id.to_string(), overrides));
        }
        numbering
    }

    /// Adds a list `num_id` of the definition `abstract_id` with the
    /// levels `levels` (a `.doc` file).
    pub(crate) fn insert(&mut self, num_id: String, abstract_id: String, mut levels: Vec<Level>) {
        levels.resize(MAX_LIST_LEVEL, Level::default());
        self.abstracts.insert(abstract_id.clone(), levels);
        self.lists.insert(num_id, (abstract_id, HashMap::new()));
    }

    /// Counts a paragraph of list `num_id` at level `level` and returns its
    /// marker: `Some(None)` for a bullet, `Some(Some("2.1."))` for a number,
    /// `None` when the paragraph is no list item (list 0, or an unknown
    /// list).
    pub(crate) fn next(&mut self, num_id: &str, level: u8) -> Option<Option<String>> {
        let (abstract_id, overrides) = self.lists.get(num_id)?;
        let levels = self.abstracts.get(abstract_id)?;
        let index = usize::from(level).min(MAX_LIST_LEVEL - 1);
        let def = &levels[index];
        if def.format == "none" && def.text.is_empty() {
            return None;
        }
        let counters = self
            .counters
            .entry(abstract_id.clone())
            .or_insert([None; MAX_LIST_LEVEL]);
        let start = |at: usize| overrides.get(&(at as u8)).copied().unwrap_or(levels[at].start);
        // A list that overrides the start of its levels starts them again
        // when it is first used.
        if self.started.insert(num_id.to_string()) {
            for &at in overrides.keys() {
                if let Some(counter) = counters.get_mut(usize::from(at)) {
                    *counter = None;
                }
            }
        }
        counters[index] = Some(counters[index].map_or(start(index), |count| count + 1));
        // The deeper levels start again.
        for deeper in counters.iter_mut().skip(index + 1) {
            *deeper = None;
        }
        if def.format == "bullet" {
            return Some(None);
        }
        let mut marker = def.text.clone();
        for at in 0..=index {
            let number = counters[at].unwrap_or_else(|| start(at));
            marker = marker.replace(&format!("%{}", at + 1), &format_number(number, &levels[at].format));
        }
        let marker = marker.trim().to_string();
        Some((!marker.is_empty()).then_some(marker))
    }
}

/// The outline level set in a `w:pPr` (0 to 8; 9 means body text).
pub(crate) fn outline_level(props: Node<'_, '_>) -> Option<u8> {
    val(props, "outlineLvl")
        .and_then(|level| level.parse::<u8>().ok())
        .filter(|&level| level < 9)
}

/// The list numbering set in a `w:pPr`: (`numId`, level).
pub(crate) fn numbering_ref(props: Node<'_, '_>) -> Option<(String, u8)> {
    let num = child(props, WORD_NS, "numPr")?;
    let id = val(num, "numId")?.to_string();
    let level = val(num, "ilvl").and_then(|level| level.parse().ok()).unwrap_or(0);
    Some((id, level))
}

/// `number` written in the list number format `format`.
fn format_number(number: u32, format: &str) -> String {
    match format {
        "lowerLetter" => letters(number).to_lowercase(),
        "upperLetter" => letters(number),
        "lowerRoman" => roman(number).to_lowercase(),
        "upperRoman" => roman(number),
        "decimalZero" => format!("{number:02}"),
        "none" => String::new(),
        _ => number.to_string(),
    }
}

/// A number as list letters: A to Z, then AA to ZZ, ...
fn letters(number: u32) -> String {
    let number = number.max(1) - 1;
    let letter = char::from(b'A' + (number % 26) as u8);
    letter.to_string().repeat(number as usize / 26 + 1)
}

/// A number in Roman numerals (capitals).
fn roman(mut number: u32) -> String {
    const NUMERALS: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut out = String::new();
    for &(value, numeral) in NUMERALS {
        while number >= value {
            out.push_str(numeral);
            number -= value;
        }
    }
    out
}

/// Whether a font name is that of a monospaced font.
pub(crate) fn is_mono_font(name: &str) -> bool {
    let lower = name.to_lowercase();
    MONO_FONT_WORDS.iter().any(|word| lower.contains(word))
}

/// A `w:color` value as RGB; `None` for `auto`.
fn parse_color(value: &str) -> Option<[u8; 3]> {
    if value.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&value[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

/// The font size set in a `w:rPr`, in points.
fn font_size(props: Node<'_, '_>) -> Option<f32> {
    val(props, "sz")
        .and_then(|size| size.parse::<f32>().ok())
        .map(|half| half / HALF_POINTS_PER_PT)
        .filter(|size| *size > 0.0)
}

/// The font set in a `w:rPr`.
fn font_name(props: Node<'_, '_>) -> Option<String> {
    let fonts = child(props, WORD_NS, "rFonts")?;
    attr(fonts, WORD_NS, "ascii")
        .or_else(|| attr(fonts, WORD_NS, "hAnsi"))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> roxmltree::Document<'_> {
        roxmltree::Document::parse(xml).unwrap()
    }

    /// Numbers count per level; a deeper level starts again under each new
    /// number; bullets have no marker.
    #[test]
    fn list_markers() {
        let xml = format!(
            "<w:numbering xmlns:w=\"{WORD_NS}\"><w:abstractNum w:abstractNumId=\"0\">\
<w:lvl w:ilvl=\"0\"><w:start w:val=\"1\"/><w:numFmt w:val=\"decimal\"/><w:lvlText w:val=\"%1.\"/></w:lvl>\
<w:lvl w:ilvl=\"1\"><w:start w:val=\"1\"/><w:numFmt w:val=\"lowerLetter\"/><w:lvlText w:val=\"(%2)\"/></w:lvl>\
<w:lvl w:ilvl=\"2\"><w:numFmt w:val=\"bullet\"/><w:lvlText w:val=\"•\"/></w:lvl></w:abstractNum>\
<w:num w:numId=\"5\"><w:abstractNumId w:val=\"0\"/></w:num></w:numbering>"
        );
        let doc = parse(&xml);
        let mut numbering = Numbering::parse(Some(doc.root_element()));
        assert_eq!(numbering.next("5", 0), Some(Some("1.".into())));
        assert_eq!(numbering.next("5", 1), Some(Some("(a)".into())));
        assert_eq!(numbering.next("5", 1), Some(Some("(b)".into())));
        assert_eq!(numbering.next("5", 2), Some(None));
        assert_eq!(numbering.next("5", 0), Some(Some("2.".into())));
        assert_eq!(numbering.next("5", 1), Some(Some("(a)".into())));
        assert_eq!(numbering.next("0", 0), None);
        assert_eq!(roman(14), "XIV");
        assert_eq!(letters(28), "BB");
    }

    /// A style inherits the outline level and the text properties of the
    /// style it is based on.
    #[test]
    fn styles_inherit() {
        let xml = format!(
            "<w:styles xmlns:w=\"{WORD_NS}\"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val=\"20\"/></w:rPr>\
</w:rPrDefault></w:docDefaults>\
<w:style w:type=\"paragraph\" w:styleId=\"H1\"><w:name w:val=\"heading 1\"/><w:pPr><w:outlineLvl w:val=\"0\"/></w:pPr>\
<w:rPr><w:b/></w:rPr></w:style>\
<w:style w:type=\"paragraph\" w:styleId=\"MyHead\"><w:name w:val=\"My Head\"/><w:basedOn w:val=\"H1\"/>\
<w:rPr><w:i/></w:rPr></w:style></w:styles>"
        );
        let doc = parse(&xml);
        let styles = Styles::parse(Some(doc.root_element()));
        assert_eq!(styles.body_size_pt, 10.0);
        let (name, outline, _) = styles.paragraph(Some("MyHead"));
        assert_eq!((name.as_str(), outline), ("My Head", Some(0)));
        assert_eq!(styles.run_props(Some("MyHead"), None).style_bits(), BOLD | ITALIC);
    }
}
