//! Conditional formats (`conditionalFormatting`): the rules of a sheet,
//! and the cells each one formats, tested on the values read. The sheet
//! view and the print show them: a rule that matches gives its cells the
//! format of its `dxf` (font colour and style, fill, borders), or a data
//! bar. Rules on a formula of their own (`expression`) and the ones Excel
//! computes over the whole range in other ways (top ten, above average,
//! colour scales, icon sets) are left out.

use super::grid::{DataBar, Grid, Merge};
use super::sheet::{cell_ref, range};
use super::styles::{Color, color_of};
use super::workbook::is_sml;
use crate::word::docx::parse;
use roxmltree::Node;
use std::collections::{BTreeMap, HashMap};

/// The most cells a rule that also formats empty cells (`containsBlanks`)
/// tests: the part of its range within the cells read.
const MAX_BLANK_CELLS: u64 = 200_000;
/// The share of a cell's width the shortest and the longest data bar of
/// Excel 2007 fill; Excel 2010's (with an `extLst`) go from none to all.
const BAR_LENGTH_2007_RATIO: (f32, f32) = (0.1, 0.9);

/// A rule of a conditional format.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rule {
    /// The ranges it formats (`sqref`).
    ranges: Vec<Merge>,
    /// Its priority: the lower first.
    priority: u32,
    /// The rules after it are not tried on a cell it formats.
    stop: bool,
    /// What a cell must hold.
    test: Test,
    /// The format it gives (`dxfId`).
    dxf: Option<u32>,
}

/// What a rule tests.
#[derive(Debug, Clone, PartialEq)]
enum Test {
    /// The value compared with one or two operands (`cellIs`).
    Compare(Operator, Vec<Operand>),
    /// The text contains, begins or ends with a text, or does not
    /// contain it.
    Text(TextTest, String),
    /// The cell is empty (`true`) or not.
    Blank(bool),
    /// The value is in the range more than once (`true`) or once.
    Duplicate(bool),
    /// A data bar, from the lower bound to the upper, in a colour; the
    /// lengths of Excel 2010 (none to all) or 2007.
    Bar(Bound, Bound, Color, bool),
}

/// The comparison of a `cellIs` rule.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Operator {
    Less,
    LessOrEqual,
    Equal,
    NotEqual,
    GreaterOrEqual,
    Greater,
    Between,
    NotBetween,
}

/// The test of a text rule.
#[derive(Debug, Clone, Copy, PartialEq)]
enum TextTest {
    Contains,
    NotContains,
    Begins,
    Ends,
}

/// An operand of a comparison.
#[derive(Debug, Clone, PartialEq)]
enum Operand {
    /// A number.
    Number(f64),
    /// A text, compared without case.
    Text(String),
    /// A cell's value: the cell, and whether its row and its column are
    /// fixed (`$`); a relative part moves with the cell tested from the
    /// top left of the rule's first range.
    Cell((u32, u32), bool, bool),
}

/// A bound of a data bar (`cfvo`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Bound {
    /// The least or the greatest value of the range.
    Lowest,
    Highest,
    /// A share of the way from the least to the greatest value, in %.
    Percent(f64),
    /// A number.
    Number(f64),
}

/// The rules of a `conditionalFormatting` element, `xml` holding it in an
/// element of the sheet's namespaces, in no order.
pub(crate) fn read(xml: &str) -> Vec<Rule> {
    let Ok(doc) = parse(xml) else {
        return Vec::new();
    };
    // The element comes wrapped, with the namespaces of the sheet.
    let wrapper = doc.root_element();
    let Some(root) = wrapper.children().find(|node| is_sml(*node, "conditionalFormatting")) else {
        return Vec::new();
    };
    let ranges: Vec<Merge> = root
        .attribute("sqref")
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(range)
        .collect();
    if ranges.is_empty() {
        return Vec::new();
    }
    root.children()
        .filter(|node| is_sml(*node, "cfRule"))
        .filter_map(|node| {
            Some(Rule {
                ranges: ranges.clone(),
                priority: node
                    .attribute("priority")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(u32::MAX),
                stop: matches!(node.attribute("stopIfTrue"), Some("1" | "true")),
                test: test(node)?,
                dxf: node.attribute("dxfId").and_then(|value| value.parse().ok()),
            })
        })
        .collect()
}

/// What the rule `node` tests; `None` for a rule left out.
fn test(node: Node<'_, '_>) -> Option<Test> {
    let text = || node.attribute("text").map(str::to_string);
    Some(match node.attribute("type")? {
        "cellIs" => {
            let operator = match node.attribute("operator")? {
                "lessThan" => Operator::Less,
                "lessThanOrEqual" => Operator::LessOrEqual,
                "equal" => Operator::Equal,
                "notEqual" => Operator::NotEqual,
                "greaterThanOrEqual" => Operator::GreaterOrEqual,
                "greaterThan" => Operator::Greater,
                "between" => Operator::Between,
                "notBetween" => Operator::NotBetween,
                _ => return None,
            };
            let operands = node
                .children()
                .filter(|child| is_sml(*child, "formula"))
                .map(|child| operand(child.text().unwrap_or_default()))
                .collect::<Option<Vec<_>>>()?;
            let wanted = if matches!(operator, Operator::Between | Operator::NotBetween) {
                2
            } else {
                1
            };
            if operands.len() < wanted {
                return None;
            }
            Test::Compare(operator, operands)
        }
        "containsText" => Test::Text(TextTest::Contains, text()?),
        "notContainsText" => Test::Text(TextTest::NotContains, text()?),
        "beginsWith" => Test::Text(TextTest::Begins, text()?),
        "endsWith" => Test::Text(TextTest::Ends, text()?),
        "containsBlanks" => Test::Blank(true),
        "notContainsBlanks" => Test::Blank(false),
        "duplicateValues" => Test::Duplicate(true),
        "uniqueValues" => Test::Duplicate(false),
        "dataBar" => {
            let bar = node.children().find(|child| is_sml(*child, "dataBar"))?;
            let mut bounds = bar.children().filter(|child| is_sml(*child, "cfvo")).map(bound);
            let (low, high) = (bounds.next()??, bounds.next()??);
            let color = bar
                .children()
                .find(|child| is_sml(*child, "color"))
                .and_then(color_of)?;
            let wide = node.children().any(|child| is_sml(child, "extLst"));
            Test::Bar(low, high, color, wide)
        }
        _ => return None,
    })
}

/// An operand written `formula`: a number, a quoted text or one cell;
/// `None` for another formula.
fn operand(formula: &str) -> Option<Operand> {
    let formula = formula.trim();
    if let Ok(number) = formula.parse::<f64>() {
        return number.is_finite().then_some(Operand::Number(number));
    }
    if let Some(text) = formula.strip_prefix('"').and_then(|rest| rest.strip_suffix('"')) {
        return Some(Operand::Text(text.replace("\"\"", "\"")));
    }
    if formula.contains(['!', ':', '(']) {
        return None;
    }
    let at = cell_ref(formula)?;
    let fixed_col = formula.starts_with('$');
    let fixed_row = formula.trim_start_matches('$').contains('$');
    Some(Operand::Cell(at, fixed_row, fixed_col))
}

/// A bound of a data bar (`cfvo`); `None` for a formula or a percentile
/// that is not a number.
fn bound(node: Node<'_, '_>) -> Option<Bound> {
    let value = || node.attribute("val").and_then(|value| value.trim().parse::<f64>().ok());
    Some(match node.attribute("type")? {
        "min" | "autoMin" => Bound::Lowest,
        "max" | "autoMax" => Bound::Highest,
        "percent" | "percentile" => Bound::Percent(value()?),
        "num" => Bound::Number(value()?),
        _ => return None,
    })
}

/// Tests `rules` on the cells of `grid` (`numbers`: the numbers of its
/// numeric cells) and records what they give: the formats of each cell,
/// in order of priority ([`Looks::conditional`](super::grid::Looks)), and
/// its data bar.
pub(crate) fn apply(rules: &mut [Rule], numbers: &HashMap<(u32, u32), f64>, grid: &mut Grid) {
    rules.sort_by_key(|rule| rule.priority);
    let text_of = |grid: &Grid, at: (u32, u32)| -> String {
        grid.cells
            .get(&at)
            .map(|cell| cell.spans.iter().map(|span| span.text.as_str()).collect())
            .unwrap_or_default()
    };
    let mut stopped: Vec<(u32, u32)> = Vec::new();
    let mut formats: BTreeMap<(u32, u32), Vec<u32>> = BTreeMap::new();
    for rule in rules.iter() {
        let cells = rule_cells(rule, grid);
        let values: Vec<Value> = cells
            .iter()
            .map(|at| match numbers.get(at) {
                Some(number) => Value::Number(*number),
                None => Value::Text(text_of(grid, *at)),
            })
            .collect();
        if let Test::Bar(low, high, color, wide) = &rule.test {
            bars(&cells, &values, (*low, *high), *color, *wide, grid);
            continue;
        }
        let counts = duplicates(&values);
        let origin = rule.ranges.first().map_or((0, 0), |range| (range.top, range.left));
        for (at, value) in cells.iter().zip(&values) {
            if stopped.binary_search(at).is_ok() {
                continue;
            }
            let operand = |operand: &Operand| -> Value {
                match operand {
                    Operand::Number(number) => Value::Number(*number),
                    Operand::Text(text) => Value::Text(text.clone()),
                    Operand::Cell((row, col), fixed_row, fixed_col) => {
                        let row = if *fixed_row {
                            *row
                        } else {
                            row + at.0.saturating_sub(origin.0)
                        };
                        let col = if *fixed_col {
                            *col
                        } else {
                            col + at.1.saturating_sub(origin.1)
                        };
                        numbers.get(&(row, col)).map_or_else(
                            || Value::Text(text_of(grid, (row, col))),
                            |number| Value::Number(*number),
                        )
                    }
                }
            };
            if !matches(&rule.test, value, &operand, &counts) {
                continue;
            }
            if let Some(dxf) = rule.dxf {
                formats.entry(*at).or_default().push(dxf);
            }
            if rule.stop
                && let Err(index) = stopped.binary_search(at)
            {
                stopped.insert(index, *at);
            }
        }
    }
    grid.looks.conditional = formats;
}

/// A value of a cell, as a rule tests it.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    /// A number (a date, a time too).
    Number(f64),
    /// A text, empty for an empty cell.
    Text(String),
}

impl Value {
    /// The value to count duplicates by: a number's bits, a text in
    /// lower case.
    fn key(&self) -> String {
        match self {
            Self::Number(number) => format!("n{number}"),
            Self::Text(text) => format!("t{}", text.to_lowercase()),
        }
    }

    /// Whether the cell is empty.
    fn is_blank(&self) -> bool {
        matches!(self, Self::Text(text) if text.trim().is_empty())
    }
}

/// The cells a rule tests: those of its ranges with a value, and for a
/// rule on empty cells also the empty ones within the cells read.
fn rule_cells(rule: &Rule, grid: &Grid) -> Vec<(u32, u32)> {
    let mut cells: Vec<(u32, u32)> = Vec::new();
    if matches!(rule.test, Test::Blank(_)) {
        let (Some(&(last_row, _)), Some(last_col)) =
            (grid.cells.keys().next_back(), grid.cells.keys().map(|at| at.1).max())
        else {
            return cells;
        };
        let mut left = MAX_BLANK_CELLS;
        for range in &rule.ranges {
            for row in range.top..=range.bottom.min(last_row) {
                for col in range.left..=range.right.min(last_col) {
                    if left == 0 {
                        return cells;
                    }
                    left -= 1;
                    cells.push((row, col));
                }
            }
        }
    } else {
        for range in &rule.ranges {
            cells.extend(
                grid.cells
                    .range((range.top, 0)..=(range.bottom, u32::MAX))
                    .map(|(at, _)| *at)
                    .filter(|at| (range.left..=range.right).contains(&at.1)),
            );
        }
    }
    cells.sort_unstable();
    cells.dedup();
    cells
}

/// How many times each value is in `values`.
fn duplicates(values: &[Value]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for value in values.iter().filter(|value| !value.is_blank()) {
        *counts.entry(value.key()).or_insert(0) += 1;
    }
    counts
}

/// Whether `value` passes `test` (`operand`: the value of an operand for
/// this cell; `counts`: how many times each value is in the range).
fn matches(test: &Test, value: &Value, operand: &dyn Fn(&Operand) -> Value, counts: &HashMap<String, usize>) -> bool {
    match test {
        Test::Compare(operator, operands) => {
            let first = operand(&operands[0]);
            let compare = |other: &Value| compare(value, other);
            match operator {
                Operator::Between | Operator::NotBetween => {
                    let second = operand(&operands[1]);
                    let inside = match (compare(&first), compare(&second)) {
                        (Some(low), Some(high)) => (low.is_ge() && high.is_le()) || (low.is_le() && high.is_ge()),
                        _ => false,
                    };
                    inside == (*operator == Operator::Between)
                }
                _ => compare(&first).is_some_and(|order| match operator {
                    Operator::Less => order.is_lt(),
                    Operator::LessOrEqual => order.is_le(),
                    Operator::Equal => order.is_eq(),
                    Operator::NotEqual => order.is_ne(),
                    Operator::GreaterOrEqual => order.is_ge(),
                    _ => order.is_gt(),
                }),
            }
        }
        Test::Text(how, wanted) => {
            // A number is searched as Excel writes it plainly (12.5).
            let text = match value {
                Value::Text(text) => text.to_lowercase(),
                Value::Number(number) => number.to_string(),
            };
            let wanted = wanted.to_lowercase();
            match how {
                TextTest::Contains => text.contains(&wanted),
                TextTest::NotContains => !text.contains(&wanted),
                TextTest::Begins => text.starts_with(&wanted),
                TextTest::Ends => text.ends_with(&wanted),
            }
        }
        Test::Blank(blank) => value.is_blank() == *blank,
        Test::Duplicate(twice) => !value.is_blank() && (counts.get(&value.key()).copied().unwrap_or(0) > 1) == *twice,
        Test::Bar(..) => false,
    }
}

/// How `value` compares with `other`: numbers as numbers, texts without
/// case, a number before a text as Excel orders them; an empty cell is 0
/// next to a number.
fn compare(value: &Value, other: &Value) -> Option<std::cmp::Ordering> {
    match (value, other) {
        (Value::Number(left), Value::Number(right)) => left.partial_cmp(right),
        (Value::Text(left), Value::Text(right)) => Some(left.to_lowercase().cmp(&right.to_lowercase())),
        (Value::Text(text), Value::Number(right)) if text.trim().is_empty() => 0f64.partial_cmp(right),
        (Value::Number(_), Value::Text(_)) => Some(std::cmp::Ordering::Less),
        (Value::Text(_), Value::Number(_)) => Some(std::cmp::Ordering::Greater),
    }
}

/// Records the data bars of the cells `cells` (`values`) between the
/// bounds `bounds`, in `color`, as long as Excel 2010 draws them (`wide`)
/// or 2007.
fn bars(cells: &[(u32, u32)], values: &[Value], bounds: (Bound, Bound), color: Color, wide: bool, grid: &mut Grid) {
    let numbers: Vec<((u32, u32), f64)> = cells
        .iter()
        .zip(values)
        .filter_map(|(at, value)| match value {
            Value::Number(number) => Some((*at, *number)),
            Value::Text(_) => None,
        })
        .collect();
    let Some(lowest) = numbers.iter().map(|(_, number)| *number).reduce(f64::min) else {
        return;
    };
    let highest = numbers.iter().map(|(_, number)| *number).fold(lowest, f64::max);
    let at_bound = |bound: Bound| match bound {
        Bound::Lowest => lowest,
        Bound::Highest => highest,
        Bound::Percent(percent) => lowest + (highest - lowest) * percent / 100.0,
        Bound::Number(number) => number,
    };
    let (low, high) = (at_bound(bounds.0), at_bound(bounds.1));
    let (shortest, longest) = if wide { (0.0, 1.0) } else { BAR_LENGTH_2007_RATIO };
    for (at, number) in numbers {
        let share = if high > low {
            ((number - low) / (high - low)).clamp(0.0, 1.0)
        } else {
            1.0
        };
        #[allow(clippy::cast_possible_truncation)]
        let length = shortest + (longest - shortest) * share as f32;
        if length > 0.0 {
            grid.looks.bars.insert(at, DataBar { length, color });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::excel::grid::{CellKind, GridCell};
    use crate::model::Span;

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    fn grid_of(values: &[((u32, u32), &str)]) -> Grid {
        let mut grid = Grid::default();
        for (at, text) in values {
            grid.cells.insert(
                *at,
                GridCell {
                    spans: vec![Span::plain(*text)],
                    kind: CellKind::Text,
                    center_continuous: false,
                    own_font: Vec::new(),
                    overflow_pt: 0.0,
                },
            );
        }
        grid
    }

    #[test]
    fn a_rule_formats_the_cells_whose_value_passes_it() {
        let mut rules = read(&format!(
            r#"<x {NS}><conditionalFormatting sqref="A1:A4"><cfRule type="cellIs" dxfId="2" priority="2" operator="greaterThan"><formula>10</formula></cfRule><cfRule type="containsText" dxfId="5" priority="1" text="late" stopIfTrue="1"/></conditionalFormatting></x>"#
        ));
        let mut grid = grid_of(&[((0, 0), "5"), ((1, 0), "12"), ((2, 0), "Late!"), ((3, 0), "20")]);
        let numbers = HashMap::from([((0, 0), 5.0), ((1, 0), 12.0), ((3, 0), 20.0)]);
        apply(&mut rules, &numbers, &mut grid);
        assert_eq!(grid.looks.conditional.get(&(0, 0)), None);
        assert_eq!(grid.looks.conditional.get(&(1, 0)), Some(&vec![2]));
        // A text is greater than any number, but the text rule stops.
        assert_eq!(grid.looks.conditional.get(&(2, 0)), Some(&vec![5]));
        assert_eq!(grid.looks.conditional.get(&(3, 0)), Some(&vec![2]));
    }

    #[test]
    fn a_data_bar_is_as_long_as_its_value_between_the_bounds() {
        let mut rules = read(&format!(
            r#"<x {NS}><conditionalFormatting sqref="B1:B3"><cfRule type="dataBar" priority="1"><dataBar><cfvo type="percent" val="0"/><cfvo type="percent" val="100"/><color rgb="FF638EC6"/></dataBar><extLst/></cfRule></conditionalFormatting></x>"#
        ));
        let mut grid = grid_of(&[((0, 1), "0%"), ((1, 1), "50%"), ((2, 1), "100%")]);
        let numbers = HashMap::from([((0, 1), 0.0), ((1, 1), 0.5), ((2, 1), 1.0)]);
        apply(&mut rules, &numbers, &mut grid);
        assert!(!grid.looks.bars.contains_key(&(0, 1)));
        assert_eq!(grid.looks.bars[&(1, 1)].length, 0.5);
        assert_eq!(grid.looks.bars[&(2, 1)].length, 1.0);
    }

    #[test]
    fn a_relative_cell_moves_with_the_cell_tested() {
        let mut rules = read(&format!(
            r#"<x {NS}><conditionalFormatting sqref="B1:B2"><cfRule type="cellIs" dxfId="0" priority="1" operator="lessThan"><formula>A1</formula></cfRule></conditionalFormatting></x>"#
        ));
        let mut grid = grid_of(&[((0, 0), "3"), ((0, 1), "1"), ((1, 0), "3"), ((1, 1), "4")]);
        let numbers = HashMap::from([((0, 0), 3.0), ((0, 1), 1.0), ((1, 0), 3.0), ((1, 1), 4.0)]);
        apply(&mut rules, &numbers, &mut grid);
        assert!(grid.looks.conditional.contains_key(&(0, 1)));
        assert!(!grid.looks.conditional.contains_key(&(1, 1)));
    }
}
