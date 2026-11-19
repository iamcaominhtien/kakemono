//! Splitting reference blocks into one entry each.
//!
//! The block analysis often groups several bibliography entries into one
//! block (their lines are equally spaced). Each entry starts with a label
//! (`[12]`, "12.") at the left edge of the first entry. Without labels, an
//! entry set with a hanging indent (A&A, ApJ: its first line at the left
//! edge, the others indented) starts at each line at the left edge; the
//! block analysis, which takes an indented line for the start of a
//! paragraph, cuts such entries the other way round.

use super::patterns::*;
use super::*;

/// Tuning parameters of reference splitting. Part of [`SemanticsConfig`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReferencesConfig {
    /// A labelled line starts a new entry when its left edge is at most this
    /// far (multiple of the first line's font size) right of the block's
    /// first line; labels further right are wrapped text such as `[12]`
    /// inside an entry.
    pub entry_indent_tolerance_em: f32,
    /// In unlabelled references, a line at least this far (multiple of its
    /// font size) right of the leftmost line continues an entry set with a
    /// hanging indent.
    pub hanging_indent_min_em: f32,
    /// ... provided at least this share of the lines at the left edge start
    /// with a capital, as the names of authors do (lines of prose taken for
    /// references often start lowercase).
    pub hanging_entry_capital_min_ratio: f32,
}

impl Default for ReferencesConfig {
    fn default() -> Self {
        Self {
            entry_indent_tolerance_em: 0.6,
            hanging_indent_min_em: 0.8,
            hanging_entry_capital_min_ratio: 0.9,
        }
    }
}

/// Splits reference blocks that contain several labelled entries. A block is
/// split only if its first line carries a label; the part after the cut is
/// inserted as a new reference node right after it and is split in turn.
pub(crate) fn split_references(nodes: &mut Vec<Node>, cfg: &SemanticsConfig) {
    regroup_hanging_entries(nodes, &cfg.references);
    let mut i = 0;
    while i < nodes.len() {
        if nodes[i].class == Class::Reference
            && let Some(block) = nodes[i].text_block()
            && let Some(cut) = next_entry_start(block, &cfg.references)
        {
            split_node_at(nodes, i, cut);
        }
        i += 1;
    }
}

/// Index of the first line after the first that starts a new labelled
/// entry. Returns `None` when the block's first line has no label or no
/// later line starts an entry.
fn next_entry_start(block: &TextBlock, cfg: &ReferencesConfig) -> Option<usize> {
    let first_x0 = block.lines[0].bbox.x0;
    let em = block.lines[0].size;
    if !REF_LABEL.is_match(block.lines[0].text().trim()) {
        return None;
    }
    block
        .lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| {
            line.bbox.x0 <= first_x0 + cfg.entry_indent_tolerance_em * em && REF_LABEL.is_match(line.text().trim())
        })
        .map(|(line_idx, _)| line_idx)
}

/// Moves the lines from `cut` on of the text node at `idx` into a new
/// reference node inserted right after it.
///
/// # Panics
///
/// If the node at `idx` is not a [`NodeKind::Text`] node.
fn split_node_at(nodes: &mut Vec<Node>, idx: usize, cut: usize) {
    let NodeKind::Text(block) = &mut nodes[idx].kind else {
        unreachable!()
    };
    let rest_lines = block.lines.split_off(cut);
    let mut rest = block.clone();
    rest.lines = rest_lines;
    let node = Node {
        kind: NodeKind::Text(rest),
        loc: nodes[idx].loc,
        class: Class::Reference,
        tail: nodes[idx].tail,
        leaf_left: nodes[idx].leaf_left,
        leaf_right: nodes[idx].leaf_right,
        strip_prefix: 0,
    };
    nodes.insert(idx + 1, node);
}

/// Cuts each run of unlabelled reference blocks of one layout leaf into one
/// block per entry when the entries have a hanging indent: an entry starts
/// at each line at the left edge, and the indented lines continue it.
/// Runs whose first line is indented, with no indented line, or whose lines
/// at the left edge do not start like the names of authors are left as
/// they are.
fn regroup_hanging_entries(nodes: &mut Vec<Node>, cfg: &ReferencesConfig) {
    let mut i = 0;
    while i < nodes.len() {
        let mut end = i;
        while end < nodes.len() && unlabelled_reference(&nodes[end]) && same_leaf(&nodes[i], &nodes[end]) {
            end += 1;
        }
        if end == i {
            i += 1;
            continue;
        }
        let entries = hanging_entries(&nodes[i..end], cfg);
        let count = match entries {
            Some(entries) => {
                let template = &nodes[i];
                let new_nodes: Vec<Node> = entries
                    .into_iter()
                    .map(|lines| reference_node(template, lines))
                    .collect();
                let count = new_nodes.len();
                nodes.splice(i..end, new_nodes);
                count
            }
            None => end - i,
        };
        i += count;
    }
}

/// Whether a node is a reference text block whose first line has no label.
fn unlabelled_reference(node: &Node) -> bool {
    node.class == Class::Reference
        && node
            .text_block()
            .is_some_and(|block| !REF_LABEL.is_match(block.lines[0].text().trim()))
}

/// Whether two nodes start in the same layout leaf of the same page.
fn same_leaf(a: &Node, b: &Node) -> bool {
    a.loc.page == b.loc.page && a.loc.leaf == b.loc.leaf
}

/// The lines of each entry of a run of reference blocks set with a hanging
/// indent, or `None` when the run is not set so or is already one block per
/// entry.
fn hanging_entries(run: &[Node], cfg: &ReferencesConfig) -> Option<Vec<Vec<Line>>> {
    let lines: Vec<&Line> = run
        .iter()
        .filter_map(Node::text_block)
        .flat_map(|block| &block.lines)
        .collect();
    let left = lines.iter().map(|line| line.bbox.x0).fold(f32::INFINITY, f32::min);
    let indented = |line: &Line| line.bbox.x0 > left + cfg.hanging_indent_min_em * line.size;
    if indented(lines[0]) || !lines.iter().any(|line| indented(line)) {
        return None;
    }
    // Entries start with the name of their first author: not with a bullet
    // (a list) or, mostly, a lowercase word (prose).
    let starts: Vec<String> = lines
        .iter()
        .filter(|line| !indented(line))
        .map(|line| line.text().trim_start().to_string())
        .collect();
    let capital = starts
        .iter()
        .filter(|text| text.starts_with(char::is_uppercase))
        .count();
    let bullet = starts
        .iter()
        .any(|text| text.starts_with(['•', '·', '–', '-', '◦', '▪']));
    if bullet || (capital as f32) < cfg.hanging_entry_capital_min_ratio * starts.len() as f32 {
        return None;
    }
    let mut entries: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        if indented(line) {
            entries.last_mut().unwrap().push(line.clone());
        } else {
            entries.push(vec![line.clone()]);
        }
    }
    // Already one block per entry: nothing to do.
    let same = entries.len() == run.len()
        && entries
            .iter()
            .zip(run)
            .all(|(entry, node)| node.text_block().is_some_and(|block| block.lines.len() == entry.len()));
    (!same).then_some(entries)
}

/// A reference node like `template` with the lines of one entry.
fn reference_node(template: &Node, lines: Vec<Line>) -> Node {
    let NodeKind::Text(block) = &template.kind else {
        unreachable!("a reference run holds text blocks")
    };
    let mut block = block.clone();
    block.bbox = lines
        .iter()
        .skip(1)
        .fold(lines[0].bbox, |bbox, line| bbox.union(&line.bbox));
    block.lines = lines;
    Node {
        kind: NodeKind::Text(block),
        loc: template.loc,
        class: Class::Reference,
        tail: template.tail,
        leaf_left: template.leaf_left,
        leaf_right: template.leaf_right,
        strip_prefix: 0,
    }
}
