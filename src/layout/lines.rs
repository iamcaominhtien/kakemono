//! The text lines of a PDF with their baselines: what the layout is
//! compared on, between the PDF Word made of a document and ours.

use crate::backend::{Dir, PdfFile};
use anyhow::Result;

/// Glyphs whose baselines differ by less than this are on one line, in
/// points.
const SAME_BASELINE_PT: f32 = 1.0;
/// A horizontal gap wider than this splits a line in two, in points (table
/// cells, text boxes side by side).
const SPLIT_GAP_PT: f32 = 30.0;
/// A gap wider than this share of the font size between two glyphs is a
/// space.
const SPACE_GAP_EM: f32 = 0.15;

/// A line of text on a page.
#[derive(Debug, Clone)]
pub struct TextLine {
    /// Zero-based page index.
    pub page: usize,
    /// Its baseline, from the top of the page.
    pub baseline: f32,
    /// Its left end.
    pub left: f32,
    /// Its right end.
    pub right: f32,
    /// Its words, separated by single spaces.
    pub text: String,
}

/// The lines of text of a PDF, page by page, top to bottom.
///
/// # Errors
/// When `data` is not a readable PDF.
pub fn pdf_lines(data: Vec<u8>) -> Result<(Vec<TextLine>, usize)> {
    let pdf = PdfFile::open(data)?;
    let (pages, _) = pdf.extract(0);
    let count = pages.len();
    let mut out = Vec::new();
    for page in pages {
        let mut glyphs: Vec<_> = page
            .glyphs
            .iter()
            .filter(|glyph| glyph.dir == Dir::Horizontal && !glyph.invisible && glyph.angle.abs() < 1.0)
            .collect();
        glyphs.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        // Group by baseline, then split at wide gaps.
        let mut rows: Vec<Vec<&crate::backend::Glyph>> = Vec::new();
        for glyph in glyphs {
            match rows.last_mut() {
                Some(row) if (row[0].y - glyph.y).abs() < SAME_BASELINE_PT => row.push(glyph),
                _ => rows.push(vec![glyph]),
            }
        }
        for mut row in rows {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut current: Option<TextLine> = None;
            let mut end = f32::MIN;
            for glyph in row {
                let gap = glyph.x - end;
                if let Some(line) = &mut current
                    && gap > SPLIT_GAP_PT
                {
                    finish(line);
                    out.push(current.take().expect("a line is open"));
                }
                let line = current.get_or_insert_with(|| TextLine {
                    page: page.index,
                    baseline: glyph.y,
                    left: glyph.x,
                    right: glyph.x,
                    text: String::new(),
                });
                if !line.text.is_empty() && gap > SPACE_GAP_EM * glyph.size {
                    line.text.push(' ');
                }
                line.text.push_str(&glyph.text);
                line.right = glyph.x + glyph.adv;
                end = glyph.x + glyph.adv;
            }
            if let Some(mut line) = current {
                finish(&mut line);
                out.push(line);
            }
        }
    }
    out.retain(|line| !line.text.is_empty());
    Ok((out, count))
}

/// Normalises the text of a finished line: single spaces, no blank ends.
fn finish(line: &mut TextLine) {
    line.text = line.text.split_whitespace().collect::<Vec<_>>().join(" ");
}
