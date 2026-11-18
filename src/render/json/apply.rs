//! Applying an edited JSON file (see [`super`]) to the analysis of its
//! source: the edited text replaces the text of the blocks, and every
//! output written afterwards shows it.
//!
//! Only what differs from the text the JSON output would write is applied,
//! so an unedited file changes nothing, and a file may hold only some of
//! the blocks. What can be edited: the `text` of every block that has one
//! (formulas keep their own text, see [`super::markup`]), the `rows` of an
//! author group and the `text` of table cells. Other fields (code
//! listings, formulas, labels, geometry) are read-only.
//!
//! Edited text no longer matches the lines of the PDF, so its per-line
//! text is dropped (`hlines`, `line_texts` of the block, `inner_spans` of
//! a table): the outputs that place text line by line (hard-95) flow it
//! inside the block's box instead, and the hard mode (whose text layer is
//! line by line) has no text for it.

use super::markup::{Parsed, from_markup, to_markup};
use super::{FORMAT_NAME, FORMAT_VERSION, SourceInfo, kind_name};
use crate::analysis::pipeline::Analysis;
use crate::model::{Block, BlockKind, Span};
use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::HashMap;

/// What applying an edited JSON file did.
#[derive(Debug, Clone, Default)]
pub struct ContentReport {
    /// Number of blocks whose text changed.
    pub edited_blocks: usize,
    /// Problems that did not stop the rest from being applied: a block whose
    /// text could not be read (it keeps its original text), formulas left
    /// out of an edited text, tags not closed.
    pub warnings: Vec<String>,
}

/// The parts of the JSON file that are read back.
#[derive(Deserialize)]
struct FileIn {
    /// [`FORMAT_NAME`].
    #[serde(default)]
    format: Option<String>,
    /// [`FORMAT_VERSION`] of the writer.
    #[serde(default)]
    version: Option<u32>,
    /// The source the file was made from.
    #[serde(default)]
    source: Option<SourceIn>,
    /// The blocks, any subset of them.
    #[serde(default)]
    blocks: Vec<BlockIn>,
}

/// The source named by the file.
#[derive(Deserialize)]
struct SourceIn {
    /// SHA-256 of the source bytes.
    #[serde(default)]
    sha256: Option<String>,
}

/// A block of the file.
#[derive(Deserialize)]
struct BlockIn {
    /// Number of the block in reading order.
    id: usize,
    /// Its type, which must be the type of the block with that id.
    #[serde(rename = "type")]
    kind: String,
    /// Its text with markup.
    #[serde(default)]
    text: Option<String>,
    /// The lines of an author group, with markup.
    #[serde(default)]
    rows: Option<Vec<String>>,
    /// A rebuilt table.
    #[serde(default)]
    table: Option<TableIn>,
}

/// A table of the file.
#[derive(Deserialize)]
struct TableIn {
    /// The cells of each row.
    #[serde(default)]
    rows: Vec<Vec<CellIn>>,
}

/// A table cell of the file.
#[derive(Deserialize)]
struct CellIn {
    /// Its text with markup.
    #[serde(default)]
    text: Option<String>,
}

/// Applies the edited JSON `json` to `analysis`, the analysis of the source
/// described by `source`.
///
/// # Errors
/// When `json` is not JSON, is not a kakemono content file, is of a newer
/// format version, was made from another source (its SHA-256 differs), or
/// names a block that does not exist or has another type (the file was
/// made with other analysis options or by another version of kakemono).
pub(crate) fn apply_content(analysis: &mut Analysis, json: &str, source: &SourceInfo) -> Result<ContentReport> {
    let file: FileIn = serde_json::from_str(json).map_err(|err| anyhow::anyhow!("reading the JSON: {err}"))?;
    check_header(&file, source)?;
    let index: HashMap<usize, usize> = analysis
        .doc
        .blocks
        .iter()
        .enumerate()
        .map(|(idx, block)| (block.id, idx))
        .collect();
    for block_in in &file.blocks {
        let Some(&idx) = index.get(&block_in.id) else {
            bail!(
                "block {} of the JSON does not exist in the document: was the JSON made with other options \
                 (--tables) or another version of kakemono?",
                block_in.id
            );
        };
        let actual = kind_name(&analysis.doc.blocks[idx].kind);
        if actual != block_in.kind {
            bail!(
                "block {} is a {} in the JSON but a {actual} in the document: was the JSON made with other \
                 options (--tables) or another version of kakemono?",
                block_in.id,
                block_in.kind
            );
        }
    }
    let mut report = ContentReport::default();
    let mut title_edited = false;
    for block_in in &file.blocks {
        let block = &mut analysis.doc.blocks[index[&block_in.id]];
        if apply_block(block, block_in, &mut report) {
            report.edited_blocks += 1;
            title_edited |= matches!(block.kind, BlockKind::Title);
        }
    }
    if title_edited {
        let title: Vec<String> = analysis
            .doc
            .blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Title))
            .map(Block::text)
            .collect();
        analysis.doc.meta.title = Some(title.join(" ").split_whitespace().collect::<Vec<_>>().join(" "));
    }
    Ok(report)
}

/// Checks the format, version and source of the file.
fn check_header(file: &FileIn, source: &SourceInfo) -> Result<()> {
    if file.format.as_deref() != Some(FORMAT_NAME) {
        bail!(
            "the JSON is not a kakemono content file (\"format\": \"{FORMAT_NAME}\" is missing); \
             write one with --format json (the --json output cannot be applied)"
        );
    }
    if let Some(version) = file.version
        && version > FORMAT_VERSION
    {
        bail!("the JSON has format version {version}; this kakemono reads up to version {FORMAT_VERSION}");
    }
    if let Some(sha) = file.source.as_ref().and_then(|source| source.sha256.as_deref())
        && !sha.eq_ignore_ascii_case(&source.sha256)
    {
        bail!(
            "the JSON was made from another file (its SHA-256 is {sha}, this one's is {})",
            source.sha256
        );
    }
    Ok(())
}

/// Applies the edited fields of one block. Returns whether its text changed.
fn apply_block(block: &mut Block, block_in: &BlockIn, report: &mut ContentReport) -> bool {
    let id = block.id;
    let mut changed = false;
    match &block.kind {
        BlockKind::Equation { .. } | BlockKind::Code => return false,
        BlockKind::Authors if !block.rows.is_empty() => {
            if let Some(rows) = &block_in.rows
                && let Some(new_rows) = edited_rows(&block.rows, rows, id, report)
            {
                block.spans = join_rows(&new_rows);
                block.rows = new_rows;
                changed = true;
            }
        }
        _ => {
            if let Some(text) = &block_in.text
                && let Some(spans) = edited_spans(&block.spans, text, &format!("block {id}"), report)
            {
                block.spans = spans;
                changed = true;
            }
        }
    }
    if changed {
        block.hlines.clear();
        block.line_texts.clear();
    }
    if let Some(table_in) = &block_in.table
        && apply_table(block, table_in, report)
    {
        block.inner_spans.clear();
        changed = true;
    }
    changed
}

/// The new rows of an author group, or `None` when none was edited (or
/// one could not be read).
fn edited_rows(old: &[Vec<Span>], rows: &[String], id: usize, report: &mut ContentReport) -> Option<Vec<Vec<Span>>> {
    let mut changed = rows.len() != old.len();
    let mut new_rows = Vec::with_capacity(rows.len());
    for (row_idx, text) in rows.iter().enumerate() {
        let original = old.get(row_idx).map_or(&[][..], Vec::as_slice);
        match edited_spans(original, text, &format!("block {id}, row {}", row_idx + 1), report) {
            Some(spans) => {
                changed = true;
                new_rows.push(spans);
            }
            None if row_idx < old.len() && to_markup(original).text == *text => new_rows.push(original.to_vec()),
            // The row could not be read (a warning says why): the group
            // keeps all its original rows.
            None => return None,
        }
    }
    changed.then_some(new_rows)
}

/// The rows of an author group as one run of text, a space between rows.
fn join_rows(rows: &[Vec<Span>]) -> Vec<Span> {
    let mut spans = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            spans.push(Span::plain(" "));
        }
        spans.extend(row.iter().cloned());
    }
    spans
}

/// Applies the edited cells of a table. Returns whether a cell changed.
/// A table whose rows or cells are not those of the document is left as
/// it is, with a warning.
fn apply_table(block: &mut Block, table_in: &TableIn, report: &mut ContentReport) -> bool {
    let id = block.id;
    let Some(table) = block.table.as_mut() else {
        return false;
    };
    let same_shape = table.rows.len() == table_in.rows.len()
        && table
            .rows
            .iter()
            .zip(&table_in.rows)
            .all(|(row, row_in)| row.len() == row_in.len());
    if !same_shape {
        report.warnings.push(format!(
            "block {id}: the table has other rows or cells than the document's; it keeps its original text"
        ));
        return false;
    }
    let mut changed = false;
    for (row_idx, (row, row_in)) in table.rows.iter_mut().zip(&table_in.rows).enumerate() {
        for (cell_idx, (cell, cell_in)) in row.iter_mut().zip(row_in).enumerate() {
            let place = format!("block {id}, table row {}, cell {}", row_idx + 1, cell_idx + 1);
            if let Some(text) = &cell_in.text
                && let Some(spans) = edited_spans(&cell.spans, text, &place, report)
            {
                cell.spans = spans;
                changed = true;
            }
        }
    }
    changed
}

/// The spans of an edited text, or `None` when `text` is the text the JSON
/// output writes for `original` (nothing was edited) or cannot be read
/// (then a warning naming `place` says why). Formulas of `original` left
/// out of the edited text are reported.
fn edited_spans(original: &[Span], text: &str, place: &str, report: &mut ContentReport) -> Option<Vec<Span>> {
    let markup = to_markup(original);
    if markup.text == text {
        return None;
    }
    let parsed: Parsed = match from_markup(text, &markup.formulas) {
        Ok(parsed) => parsed,
        Err(err) => {
            report.warnings.push(format!("{place}: kept the original text: {err}"));
            return None;
        }
    };
    for warning in &parsed.warnings {
        report.warnings.push(format!("{place}: {warning}"));
    }
    let missing: Vec<String> = (1..=markup.formulas.len())
        .filter(|id| !parsed.formulas_used.contains(id))
        .map(|id| id.to_string())
        .collect();
    if !missing.is_empty() {
        report.warnings.push(format!(
            "{place}: the edited text leaves out formula {}",
            missing.join(", ")
        ));
    }
    Some(parsed.spans)
}
