//! The text inside a figure as editable runs (`figure_lines` of the JSON):
//! a label, or the lines of one paragraph inside a box of the figure.
//!
//! A figure keeps the lines of text drawn inside it ([`Block::inner_lines`])
//! with their styled text ([`Block::inner_spans`]). They are grouped into
//! runs by geometry: lines one under the other, at a regular pitch, of the
//! same size and aligned on the left or on the centre. An edited run is
//! applied by drawing the figure again ([`figure_svg`]): the page region
//! without the glyphs of the edited runs, and their new text in their box,
//! broken into lines that fit its width. The result replaces the figure's
//! picture, as a `replace_image` does.

use super::text_of;
use crate::backend::PageRenderer;
use crate::model::{BOLD, Block, ITALIC, Line, MONO, Span};
use crate::render::assets::origin_key;
use crate::render::escape::esc;
use std::collections::HashSet;

/// Largest distance between the baselines of two lines of one run, in
/// multiples of their font size (ems): a wider gap starts another run.
const RUN_PITCH_MAX_EM: f32 = 1.8;
/// Largest difference between the font sizes of two lines of one run, as a
/// share of the larger.
const RUN_SIZE_TOLERANCE_RATIO: f32 = 0.15;
/// Largest offset of the left edges (or of the centres) of two lines of one
/// run, in ems.
const RUN_ALIGN_TOLERANCE_EM: f32 = 1.0;
/// Smallest font size an edited run may be shrunk to, so that its text fits
/// the lines of its box, as a share of its original size; below it, the
/// text runs past the box.
const RUN_MIN_SCALE_RATIO: f32 = 0.75;
/// Step by which an edited run is shrunk while its text does not fit.
const RUN_SHRINK_STEP_RATIO: f32 = 0.05;
/// Distance between the baselines of the lines of a one-line run once
/// broken, in ems.
const SINGLE_LINE_PITCH_EM: f32 = 1.2;
/// How much wider than the original a one-line label may be written before
/// it is made smaller: a label is not broken, and its new text is often a
/// little longer (the box is the ink of the old text, with no margin).
const LABEL_MAX_GROWTH_RATIO: f32 = 1.6;

/// The runs of the text inside `block`: each the indexes of its lines in
/// [`Block::inner_lines`], top to bottom. Empty for a block whose text is
/// not that of a figure's picture (a rebuilt table).
pub(crate) fn runs(block: &Block) -> Vec<Vec<usize>> {
    if block.table.is_some() || block.inner_lines.len() != block.inner_spans.len() {
        return Vec::new();
    }
    // Lines come top to bottom across the whole figure, so the lines of a
    // run may be interleaved with those of a run beside it: a line joins
    // the run it continues, wherever that run is.
    let mut order: Vec<usize> = (0..block.inner_lines.len()).collect();
    order.sort_by(|&a, &b| block.inner_lines[a].baseline.total_cmp(&block.inner_lines[b].baseline));
    let mut runs: Vec<Vec<usize>> = Vec::new();
    for idx in order {
        let line = &block.inner_lines[idx];
        let continued = runs.iter_mut().rev().find(|run| {
            let prev = &block.inner_lines[*run.last().expect("a run has a line")];
            same_run(prev, line)
        });
        match continued {
            Some(run) => run.push(idx),
            None => runs.push(vec![idx]),
        }
    }
    // In reading order: by the top of their first line, then from the left.
    runs.sort_by(|a, b| {
        let (la, lb) = (&block.inner_lines[a[0]], &block.inner_lines[b[0]]);
        la.bbox
            .y0
            .total_cmp(&lb.bbox.y0)
            .then(la.bbox.x0.total_cmp(&lb.bbox.x0))
    });
    runs
}

/// Whether `line` continues the run whose last line is `prev`.
fn same_run(prev: &Line, line: &Line) -> bool {
    let size = prev.size.max(line.size).max(f32::EPSILON);
    let gap = line.baseline - prev.baseline;
    let left = (line.bbox.x0 - prev.bbox.x0).abs();
    let centre = ((line.bbox.x0 + line.bbox.x1) - (prev.bbox.x0 + prev.bbox.x1)).abs() / 2.0;
    prev.page == line.page
        && gap > 0.0
        && gap <= RUN_PITCH_MAX_EM * size
        && (prev.size - line.size).abs() <= RUN_SIZE_TOLERANCE_RATIO * size
        && left.min(centre) <= RUN_ALIGN_TOLERANCE_EM * size
}

/// The styled text of a run: its lines joined by spaces.
pub(crate) fn run_spans(block: &Block, run: &[usize]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (pos, &idx) in run.iter().enumerate() {
        if pos > 0 {
            spans.push(Span::plain(" "));
        }
        spans.extend(block.inner_spans[idx].iter().cloned());
    }
    // Lines of the same style make one span, not one per line.
    let mut merged: Vec<Span> = Vec::with_capacity(spans.len());
    for span in spans {
        match merged.last_mut() {
            Some(last)
                if last.style == span.style
                    && last.href == span.href
                    && last.color == span.color
                    && last.math.is_none()
                    && span.math.is_none()
                    && last.note.is_none()
                    && span.note.is_none() =>
            {
                last.text.push_str(&span.text);
            }
            _ => merged.push(span),
        }
    }
    merged
}

/// One run as the JSON writes it.
#[derive(serde::Serialize)]
pub(crate) struct RunOut {
    /// Its text with markup (editable).
    text: String,
    /// Its box on the page, in points: left, top, right, bottom.
    #[serde(rename = "box")]
    bbox: [f32; 4],
    /// Its font size in points.
    size: f32,
}

/// The runs of `block` as the JSON writes them; `None` when it has none.
pub(crate) fn runs_out(block: &Block) -> Option<Vec<RunOut>> {
    let runs = runs(block);
    let out: Vec<RunOut> = runs
        .iter()
        .map(|run| {
            let lines = run.iter().map(|&idx| &block.inner_lines[idx]);
            let round = |value: f32| (value * 100.0).round() / 100.0;
            let (x0, y0, x1, y1) = lines.fold((f32::MAX, f32::MAX, f32::MIN, f32::MIN), |acc, line| {
                (
                    acc.0.min(line.bbox.x0),
                    acc.1.min(line.bbox.y0),
                    acc.2.max(line.bbox.x1),
                    acc.3.max(line.bbox.y1),
                )
            });
            RunOut {
                text: text_of(&run_spans(block, run)),
                bbox: [round(x0), round(y0), round(x1), round(y1)],
                size: round(block.inner_lines[run[0]].size),
            }
        })
        .collect();
    (!out.is_empty()).then_some(out)
}

/// The edited runs of one figure: the lines of each and its new spans.
pub(crate) struct FigureEdit {
    /// Index of the figure in [`crate::model::Document::blocks`].
    pub(crate) block_idx: usize,
    /// Each edited run: its line indexes and its new text.
    pub(crate) runs: Vec<(Vec<usize>, Vec<Span>)>,
}

/// The figure `block` drawn again with its edited runs, as an SVG of its
/// picture's box: the page region without their glyphs, and their new text.
/// Warnings (a text that does not fit) go to `warnings`. `None` when the
/// page cannot be drawn.
pub(crate) fn figure_svg(
    renderer: &PageRenderer<'_>,
    block: &Block,
    edit: &FigureEdit,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let crate::model::BlockKind::Figure { image: Some(spec), .. } = &block.kind else {
        return None;
    };
    let mut removed: HashSet<(i32, i32)> = HashSet::new();
    for (run, _) in &edit.runs {
        for &idx in run {
            for glyph in &block.inner_lines[idx].glyphs {
                removed.insert(origin_key(glyph.ox, glyph.oy));
                for &(x, y) in &glyph.extra {
                    removed.insert(origin_key(x, y));
                }
            }
        }
    }
    let remove = |x: f32, y: f32| {
        let (qx, qy) = origin_key(x, y);
        (-1..=1).any(|dx| (-1..=1).any(|dy| removed.contains(&(qx + dx, qy + dy))))
    };
    let page = renderer.page_svg_without_all_text(spec.page, &remove)?;
    let rect = spec.rect;
    // The page in points, seen through the figure's box.
    let open_end = page.find('>')?;
    let root = &page[..open_end];
    let body = &page[open_end + 1..page.rfind("</svg>")?];
    let xlink = if root.contains("xmlns:xlink") {
        " xmlns:xlink=\"http://www.w3.org/1999/xlink\""
    } else {
        ""
    };
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\"{xlink} width=\"{w}\" height=\"{h}\" viewBox=\"{x} {y} {w} {h}\">{body}",
        x = rect.x0,
        y = rect.y0,
        w = rect.width(),
        h = rect.height(),
    );
    for (run, spans) in &edit.runs {
        out.push_str(&run_text(block, run, spans, block.id, warnings));
    }
    out.push_str("</svg>");
    Some(out)
}

/// The SVG `<text>` of an edited run: its words broken into lines that fit
/// the width of its box, at its size (shrunk down to
/// [`RUN_MIN_SCALE_RATIO`] when they need more lines than it had), on its
/// lines' baselines and pitch, aligned as its lines were.
fn run_text(block: &Block, run: &[usize], spans: &[Span], id: usize, warnings: &mut Vec<String>) -> String {
    let lines: Vec<&Line> = run.iter().map(|&idx| &block.inner_lines[idx]).collect();
    let first = lines[0];
    let x0 = lines.iter().map(|line| line.bbox.x0).fold(f32::MAX, f32::min);
    let x1 = lines.iter().map(|line| line.bbox.x1).fold(f32::MIN, f32::max);
    let width = x1 - x0;
    let pitch = if lines.len() > 1 {
        (lines[lines.len() - 1].baseline - first.baseline) / (lines.len() - 1) as f32
    } else {
        first.size * SINGLE_LINE_PITCH_EM
    };
    let centred = lines.len() == 1 || {
        let spread = |key: &dyn Fn(&Line) -> f32| {
            let values: Vec<f32> = lines.iter().map(|line| key(line)).collect();
            values.iter().copied().fold(f32::MIN, f32::max) - values.iter().copied().fold(f32::MAX, f32::min)
        };
        spread(&|line| line.bbox.x0) > spread(&|line| (line.bbox.x0 + line.bbox.x1) / 2.0)
    };
    let style = spans.first().map_or(0, |span| span.style);
    let font = TextFont::of(style);
    let text: String = spans.iter().map(|span| span.text.as_str()).collect();
    let words: Vec<&str> = text.split_whitespace().collect();
    // One line may be a little wider than the original: the box is the ink
    // of the old text, and a short label's new text often runs past it.
    let max_lines = lines.len().max(1);
    let mut size = first.size;
    let fits = |broken: &[String], size: f32| {
        if max_lines == 1 {
            text_width(&broken.join(" "), size, &font) <= width * LABEL_MAX_GROWTH_RATIO
        } else {
            broken.len() <= max_lines
        }
    };
    let layout = |size: f32| {
        if max_lines == 1 {
            vec![words.join(" ")]
        } else {
            break_words(&words, width, size, &font)
        }
    };
    let mut broken = layout(size);
    while !fits(&broken, size) && size > first.size * RUN_MIN_SCALE_RATIO {
        size -= first.size * RUN_SHRINK_STEP_RATIO;
        broken = layout(size);
    }
    if !fits(&broken, size) {
        warnings.push(format!(
            "block {id}: the edited text inside the figure (\"{}\") does not fit its box",
            text.chars().take(40).collect::<String>()
        ));
    }
    let pitch = pitch * size / first.size;
    let (anchor, x) = if centred {
        ("middle", (x0 + x1) / 2.0)
    } else {
        ("start", x0)
    };
    let mut out = format!(
        "<text font-family=\"{}\" font-size=\"{size}\" font-weight=\"{}\" font-style=\"{}\" text-anchor=\"{anchor}\" fill=\"#000\">",
        font.family, font.weight, font.style
    );
    for (pos, line) in broken.iter().enumerate() {
        out.push_str(&format!(
            "<tspan x=\"{x}\" y=\"{}\">{}</tspan>",
            first.baseline + pos as f32 * pitch,
            esc(line)
        ));
    }
    out.push_str("</text>");
    out
}

/// The font an edited run is written in, from the style of its first span.
struct TextFont {
    /// CSS font family.
    family: &'static str,
    /// CSS font weight.
    weight: &'static str,
    /// CSS font style.
    style: &'static str,
}

impl TextFont {
    /// The font for the style bits `style`.
    fn of(style: u8) -> Self {
        Self {
            // Most figures are labelled in a sans-serif font (and Type 3
            // fonts, as matplotlib writes, are not marked as such).
            family: if style & MONO != 0 { "monospace" } else { "sans-serif" },
            weight: if style & BOLD != 0 { "bold" } else { "normal" },
            style: if style & ITALIC != 0 { "italic" } else { "normal" },
        }
    }
}

/// `words` broken into lines no wider than `width` at `size` (a word wider
/// than the width has a line of its own).
fn break_words(words: &[&str], width: f32, size: f32, font: &TextFont) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in words {
        let candidate = if current.is_empty() {
            (*word).to_string()
        } else {
            format!("{current} {word}")
        };
        if !current.is_empty() && text_width(&candidate, size, font) > width {
            lines.push(std::mem::replace(&mut current, (*word).to_string()));
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// The width of `text` at `size` in `font`, in points, measured with the
/// fonts SVG text is drawn with; a rough estimate when it cannot be.
fn text_width(text: &str, size: f32, font: &TextFont) -> f32 {
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10000\" height=\"100\"><text x=\"0\" y=\"50\" font-family=\"{}\" font-size=\"{size}\" font-weight=\"{}\" font-style=\"{}\">{}</text></svg>",
        font.family,
        font.weight,
        font.style,
        esc(text)
    );
    let options = resvg::usvg::Options {
        fontdb: crate::backend::render::svg_fonts(),
        ..Default::default()
    };
    resvg::usvg::Tree::from_str(&svg, &options)
        .ok()
        .map(|tree| tree.root().abs_bounding_box().width())
        .filter(|width| *width > 0.0)
        .unwrap_or_else(|| text.chars().count() as f32 * size * 0.5)
}
