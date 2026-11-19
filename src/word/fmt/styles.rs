//! The styles and list definitions of a `.docx`, with every property kept,
//! and the inheritance that resolves the properties of a paragraph, a run,
//! a table and a list number.

use super::model::{Align, NumFormat, NumberSuffix};
use super::props::{RawCell, RawPara, RawRun, RawTable, ThemeFonts};
use crate::word::xml::{WORD_NS, attr, child, children, val};
use roxmltree::Node;
use std::collections::{HashMap, HashSet};

/// Deepest chain of `w:basedOn` followed: a guard against loops.
const MAX_STYLE_DEPTH: usize = 16;
/// Number of list levels (0 to 8).
pub(crate) const LIST_LEVELS: usize = 9;

/// The kind of a style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StyleKind {
    /// For paragraphs.
    #[default]
    Paragraph,
    /// For runs.
    Character,
    /// For tables.
    Table,
    /// For lists.
    Numbering,
}

/// Properties of a part of a table styled by condition (`w:tblStylePr`):
/// the first row, odd banded rows...
#[derive(Debug, Clone, Default)]
pub(crate) struct Conditional {
    pub(crate) run: RawRun,
    pub(crate) para: RawPara,
    pub(crate) cell: RawCell,
}

/// One style.
#[derive(Debug, Clone, Default)]
pub(crate) struct Style {
    pub(crate) kind: StyleKind,
    pub(crate) based_on: Option<String>,
    pub(crate) run: RawRun,
    pub(crate) para: RawPara,
    pub(crate) table: RawTable,
    pub(crate) cell: RawCell,
    pub(crate) conditions: HashMap<String, Conditional>,
}

/// The styles of a document.
#[derive(Debug, Clone, Default)]
pub(crate) struct Styles {
    /// Text properties of the document defaults.
    pub(crate) default_run: RawRun,
    /// Paragraph properties of the document defaults.
    pub(crate) default_para: RawPara,
    by_id: HashMap<String, Style>,
    default_para_style: Option<String>,
    default_char_style: Option<String>,
    default_table_style: Option<String>,
}

impl Styles {
    /// Reads `styles.xml` (its root `w:styles`); `None` for a document
    /// without styles.
    pub(crate) fn parse(root: Option<Node<'_, '_>>, theme: &ThemeFonts) -> Self {
        let mut styles = Styles::default();
        let Some(root) = root else { return styles };
        if let Some(defaults) = child(root, WORD_NS, "docDefaults") {
            if let Some(props) = child(defaults, WORD_NS, "rPrDefault").and_then(|d| child(d, WORD_NS, "rPr")) {
                styles.default_run = RawRun::parse(props, theme);
            }
            if let Some(props) = child(defaults, WORD_NS, "pPrDefault").and_then(|d| child(d, WORD_NS, "pPr")) {
                styles.default_para = RawPara::parse(props);
            }
        }
        for node in children(root, WORD_NS, "style") {
            let Some(id) = attr(node, WORD_NS, "styleId") else {
                continue;
            };
            let kind = match attr(node, WORD_NS, "type").unwrap_or("paragraph") {
                "character" => StyleKind::Character,
                "table" => StyleKind::Table,
                "numbering" => StyleKind::Numbering,
                _ => StyleKind::Paragraph,
            };
            let mut style = Style {
                kind,
                based_on: val(node, "basedOn").map(str::to_string),
                run: child(node, WORD_NS, "rPr")
                    .map(|props| RawRun::parse(props, theme))
                    .unwrap_or_default(),
                para: child(node, WORD_NS, "pPr").map(RawPara::parse).unwrap_or_default(),
                table: child(node, WORD_NS, "tblPr").map(RawTable::parse).unwrap_or_default(),
                cell: child(node, WORD_NS, "tcPr").map(RawCell::parse).unwrap_or_default(),
                conditions: HashMap::new(),
            };
            for cond in children(node, WORD_NS, "tblStylePr") {
                let Some(kind) = attr(cond, WORD_NS, "type") else {
                    continue;
                };
                style.conditions.insert(
                    kind.to_string(),
                    Conditional {
                        run: child(cond, WORD_NS, "rPr")
                            .map(|props| RawRun::parse(props, theme))
                            .unwrap_or_default(),
                        para: child(cond, WORD_NS, "pPr").map(RawPara::parse).unwrap_or_default(),
                        cell: child(cond, WORD_NS, "tcPr").map(RawCell::parse).unwrap_or_default(),
                    },
                );
            }
            if matches!(attr(node, WORD_NS, "default"), Some("1" | "true" | "on")) {
                let slot = match kind {
                    StyleKind::Paragraph => Some(&mut styles.default_para_style),
                    StyleKind::Character => Some(&mut styles.default_char_style),
                    StyleKind::Table => Some(&mut styles.default_table_style),
                    StyleKind::Numbering => None,
                };
                if let Some(slot) = slot {
                    *slot = Some(id.to_string());
                }
            }
            styles.by_id.insert(id.to_string(), style);
        }
        styles
    }

    /// The style `id`, or the default paragraph style when `id` is unknown.
    fn chain(&self, id: Option<&str>, fallback: Option<&str>) -> Vec<&Style> {
        let mut chain = Vec::new();
        let mut seen = HashSet::new();
        let mut next = id.filter(|id| self.by_id.contains_key(*id)).or(fallback);
        while let Some(current) = next {
            if chain.len() >= MAX_STYLE_DEPTH || !seen.insert(current) {
                break;
            }
            let Some(style) = self.by_id.get(current) else { break };
            chain.push(style);
            next = style.based_on.as_deref();
        }
        chain.reverse();
        chain
    }

    /// The id of the paragraph style a paragraph has: its own, else the
    /// default one.
    pub(crate) fn para_style_id(&self, id: Option<&str>) -> String {
        id.filter(|id| self.by_id.contains_key(*id))
            .or(self.default_para_style.as_deref())
            .unwrap_or_default()
            .to_string()
    }

    /// The paragraph and text properties of the paragraph style `id`
    /// (with what it inherits).
    pub(crate) fn paragraph(&self, id: Option<&str>) -> (RawPara, RawRun) {
        let mut para = RawPara::default();
        let mut run = RawRun::default();
        for style in self.chain(id, self.default_para_style.as_deref()) {
            para.overlay(&style.para);
            run.overlay(&style.run);
        }
        (para, run)
    }

    /// The text properties of the character style `id`.
    pub(crate) fn character(&self, id: &str) -> RawRun {
        let mut run = RawRun::default();
        for style in self.chain(Some(id), None) {
            if style.kind == StyleKind::Character || style.kind == StyleKind::Paragraph {
                run.overlay(&style.run);
            }
        }
        run
    }

    /// The table style `id` (or the default table style), resolved with
    /// what it inherits: its table, cell, paragraph and text properties,
    /// and its conditional parts.
    pub(crate) fn table(&self, id: Option<&str>) -> TableStyle {
        let mut out = TableStyle::default();
        for style in self.chain(id, self.default_table_style.as_deref()) {
            out.table.overlay(&style.table);
            out.cell.overlay(&style.cell);
            out.para.overlay(&style.para);
            out.run.overlay(&style.run);
            for (kind, cond) in &style.conditions {
                let slot = out.conditions.entry(kind.clone()).or_default();
                slot.run.overlay(&cond.run);
                slot.para.overlay(&cond.para);
                slot.cell.overlay(&cond.cell);
            }
        }
        out
    }

    /// The list (`numId`) a numbering style refers to.
    pub(crate) fn numbering_style_list(&self, id: &str) -> Option<String> {
        self.chain(Some(id), None)
            .iter()
            .rev()
            .find_map(|style| style.para.numbering.as_ref().map(|(id, _)| id.clone()))
    }
}

/// A table style, resolved.
#[derive(Debug, Clone, Default)]
pub(crate) struct TableStyle {
    pub(crate) table: RawTable,
    pub(crate) cell: RawCell,
    pub(crate) para: RawPara,
    pub(crate) run: RawRun,
    pub(crate) conditions: HashMap<String, Conditional>,
}

/// One level of a list definition.
#[derive(Debug, Clone, Default)]
pub(crate) struct Level {
    pub(crate) start: u32,
    pub(crate) format: NumFormat,
    pub(crate) text: String,
    pub(crate) align: Align,
    pub(crate) suffix: NumberSuffix,
    pub(crate) para: RawPara,
    pub(crate) run: RawRun,
    /// The level that restarts it (`w:lvlRestart`): `None` for the level
    /// above, `Some(0)` never.
    pub(crate) restart: Option<u32>,
    /// Numbers of the levels above are written in decimal.
    pub(crate) legal: bool,
}

/// The list definitions and their counters.
#[derive(Debug, Clone, Default)]
pub(crate) struct Numbering {
    abstracts: HashMap<String, Vec<Level>>,
    /// The style an abstract definition takes its levels from.
    style_links: HashMap<String, String>,
    /// For each list (`numId`): its abstract definition, and its levels
    /// that override the definition's (start, whole level).
    lists: HashMap<String, (String, HashMap<u8, (Option<u32>, Option<Level>)>)>,
    counters: HashMap<String, [Option<u32>; LIST_LEVELS]>,
    started: HashSet<String>,
}

impl Numbering {
    /// Reads `numbering.xml` (its root `w:numbering`).
    pub(crate) fn parse(root: Option<Node<'_, '_>>, theme: &ThemeFonts) -> Self {
        let mut numbering = Numbering::default();
        let Some(root) = root else { return numbering };
        for node in children(root, WORD_NS, "abstractNum") {
            let Some(id) = attr(node, WORD_NS, "abstractNumId") else {
                continue;
            };
            let mut levels = vec![Level::default(); LIST_LEVELS];
            for level in children(node, WORD_NS, "lvl") {
                let Some(index) = attr(level, WORD_NS, "ilvl").and_then(|ilvl| ilvl.parse::<usize>().ok()) else {
                    continue;
                };
                if let Some(slot) = levels.get_mut(index) {
                    *slot = parse_level(level, theme);
                }
            }
            if let Some(link) = val(node, "numStyleLink") {
                numbering.style_links.insert(id.to_string(), link.to_string());
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
                    let start = val(over, "startOverride").and_then(|start| start.parse().ok());
                    let whole = child(over, WORD_NS, "lvl").map(|lvl| parse_level(lvl, theme));
                    Some((level, (start, whole)))
                })
                .collect();
            numbering
                .lists
                .insert(id.to_string(), (abstract_id.to_string(), overrides));
        }
        numbering
    }

    /// The levels of list `num_id`, following a numbering style link.
    fn levels(&self, num_id: &str, styles: &Styles) -> Option<(String, Vec<Level>)> {
        let (abstract_id, overrides) = self.lists.get(num_id)?;
        let mut abstract_id = abstract_id.clone();
        let mut levels = self.abstracts.get(&abstract_id)?.clone();
        if let Some(link) = self.style_links.get(&abstract_id)
            && let Some(linked) = styles.numbering_style_list(link)
            && linked != num_id
            && let Some((linked_abstract, _)) = self.lists.get(&linked)
            && let Some(linked_levels) = self.abstracts.get(linked_abstract)
        {
            abstract_id = linked_abstract.clone();
            levels = linked_levels.clone();
        }
        for (&at, (start, whole)) in overrides {
            let Some(slot) = levels.get_mut(usize::from(at)) else {
                continue;
            };
            if let Some(whole) = whole {
                *slot = whole.clone();
            }
            if let Some(start) = start {
                slot.start = *start;
            }
        }
        Some((abstract_id, levels))
    }

    /// The level `level` of list `num_id`, without counting.
    pub(crate) fn level(&self, num_id: &str, level: u8, styles: &Styles) -> Option<Level> {
        let (_, levels) = self.levels(num_id, styles)?;
        levels.get(usize::from(level).min(LIST_LEVELS - 1)).cloned()
    }

    /// Counts a paragraph of list `num_id` at level `level`, and returns
    /// its level definition and number text; `None` when it is no list item.
    pub(crate) fn next(&mut self, num_id: &str, level: u8, styles: &Styles) -> Option<(Level, String)> {
        let (abstract_id, levels) = self.levels(num_id, styles)?;
        let index = usize::from(level).min(LIST_LEVELS - 1);
        let def = levels[index].clone();
        let counters = self.counters.entry(abstract_id).or_insert([None; LIST_LEVELS]);
        if self.started.insert(num_id.to_string())
            && let Some((_, overrides)) = self.lists.get(num_id)
        {
            for (&at, (start, _)) in overrides {
                if start.is_some()
                    && let Some(counter) = counters.get_mut(usize::from(at))
                {
                    *counter = None;
                }
            }
        }
        counters[index] = Some(counters[index].map_or(def.start, |count| count + 1));
        for (deeper, counter) in counters.iter_mut().enumerate().skip(index + 1) {
            // A deeper level restarts after a higher one, unless it says
            // otherwise.
            let restart = levels[deeper].restart;
            if restart.is_none_or(|restart| restart != 0 && (restart as usize) > index) {
                *counter = None;
            }
        }
        let mut text = def.text.clone();
        for at in 0..=index {
            let number = counters[at].unwrap_or(levels[at].start);
            let format = if def.legal && at < index {
                &NumFormat::Decimal
            } else {
                &levels[at].format
            };
            text = text.replace(&format!("%{}", at + 1), &format_number(number, format));
        }
        Some((def, text))
    }
}

/// A `w:lvl` element.
fn parse_level(node: Node<'_, '_>, theme: &ThemeFonts) -> Level {
    Level {
        start: val(node, "start").and_then(|start| start.parse().ok()).unwrap_or(0),
        format: val(node, "numFmt").map_or(NumFormat::Decimal, parse_num_format),
        text: val(node, "lvlText").unwrap_or_default().to_string(),
        align: match val(node, "lvlJc") {
            Some("center") => Align::Center,
            Some("right" | "end") => Align::Right,
            _ => Align::Left,
        },
        suffix: match val(node, "suff") {
            Some("space") => NumberSuffix::Space,
            Some("nothing") => NumberSuffix::Nothing,
            _ => NumberSuffix::Tab,
        },
        para: child(node, WORD_NS, "pPr").map(RawPara::parse).unwrap_or_default(),
        run: child(node, WORD_NS, "rPr")
            .map(|props| RawRun::parse(props, theme))
            .unwrap_or_default(),
        restart: val(node, "lvlRestart").and_then(|restart| restart.parse().ok()),
        legal: crate::word::xml::toggle(node, "isLgl").unwrap_or(false),
    }
}

/// A number format name (`w:numFmt`).
pub(crate) fn parse_num_format(name: &str) -> NumFormat {
    match name {
        "decimalZero" => NumFormat::DecimalZero,
        "lowerLetter" => NumFormat::LowerLetter,
        "upperLetter" => NumFormat::UpperLetter,
        "lowerRoman" => NumFormat::LowerRoman,
        "upperRoman" => NumFormat::UpperRoman,
        "bullet" => NumFormat::Bullet,
        "none" => NumFormat::None,
        "chicago" => NumFormat::Chicago,
        "cardinalText" => NumFormat::CardinalText,
        "ordinalText" => NumFormat::OrdinalText,
        "ordinal" => NumFormat::Ordinal,
        _ => NumFormat::Decimal,
    }
}

/// `number` written in `format`.
pub(crate) fn format_number(number: u32, format: &NumFormat) -> String {
    match format {
        NumFormat::LowerLetter => letters(number).to_lowercase(),
        NumFormat::UpperLetter => letters(number),
        NumFormat::LowerRoman => roman(number).to_lowercase(),
        NumFormat::UpperRoman => roman(number),
        NumFormat::DecimalZero => format!("{number:02}"),
        NumFormat::None | NumFormat::Bullet => String::new(),
        NumFormat::Chicago => {
            let symbols = ['*', '†', '‡', '§'];
            let n = number.max(1) as usize - 1;
            symbols[n % 4].to_string().repeat(n / 4 + 1)
        }
        NumFormat::Ordinal => {
            let suffix = match (number % 10, number % 100) {
                (_, 11..=13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            };
            format!("{number}{suffix}")
        }
        NumFormat::Decimal | NumFormat::CardinalText | NumFormat::OrdinalText => number.to_string(),
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
