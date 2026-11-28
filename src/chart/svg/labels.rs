//! Data labels: the value of a bar or a point, the share of a slice, as
//! the file asks for them (`c:dLbls`). They are gathered while the marks
//! are drawn and written after all of them, so that no mark covers one.

use super::write::{Svg, TextStyle, text_width};
use super::{BASELINE_RATIO, INK, INK_SECONDARY, SMALL};
use crate::chart::{Rgb, Series};
use crate::excel::numfmt::{self, NumFmt};

/// The font size of a data label, in points.
pub(super) const LABEL_SIZE_PT: f32 = 8.0;
/// The most data labels drawn in a chart; past it, the others are left
/// out (a label per point of thousands of points is no longer readable).
pub(super) const MAX_DATA_LABELS: usize = 600;
/// The space between a label and its mark, in points.
pub(super) const LABEL_OFFSET_PT: f32 = 3.0;
/// A fill darker than this relative luminance takes white text.
const DARK_LUMINANCE: f32 = 0.18;
/// The text on a dark fill.
const INK_ON_DARK: &str = "#ffffff";
/// The format of a share of a pie: Excel's, a whole percent.
const PERCENT_FORMAT: &str = "0%";
/// Between the value and the share of one label, as Excel joins them.
const SEPARATOR: &str = ", ";

/// A data label, to be written.
pub(super) struct Label {
    /// The middle of its baseline.
    pub(super) at: (f32, f32),
    /// The text.
    pub(super) text: String,
    /// Its colour.
    pub(super) ink: &'static str,
}

/// The data labels of a chart, gathered while its marks are drawn.
pub(super) struct Labels {
    /// The labels so far.
    labels: Vec<Label>,
}

impl Labels {
    /// No labels yet.
    pub(super) fn new() -> Self {
        Self { labels: Vec::new() }
    }

    /// Whether one more label may be drawn.
    pub(super) fn room(&self) -> bool {
        self.labels.len() < MAX_DATA_LABELS
    }

    /// Adds the label `text` centred on `center` (the middle of its line),
    /// in `ink`, when there is room.
    pub(super) fn add(&mut self, center: (f32, f32), text: String, ink: &'static str) {
        if self.room() {
            self.labels.push(Label {
                at: (center.0, center.1 + LABEL_SIZE_PT * BASELINE_RATIO),
                text,
                ink,
            });
        }
    }

    /// Writes the labels.
    pub(super) fn write(self, out: &mut Svg) {
        for label in self.labels {
            let style = TextStyle {
                size: LABEL_SIZE_PT,
                fill: label.ink,
                ..SMALL
            };
            out.text("data-label", label.at, style, &label.text);
        }
    }
}

/// The label of point `index` of `series`, whose value is `value`: its
/// value (in the labels' format, else the series'), its share of the
/// total `share` when it is given (a pie's), or both; `None` when the
/// point shows no label.
pub(super) fn text(series: &Series, index: usize, value: f64, share: Option<f64>, date1904: bool) -> Option<String> {
    let (show_value, show_percent) = series.labels.shown(index);
    let mut parts = Vec::new();
    if show_value && value.is_finite() {
        let format = series
            .labels
            .format
            .as_ref()
            .or_else(|| series.values.as_ref().map(|values| &values.format))
            .unwrap_or(&NumFmt::General);
        parts.push(numfmt::format(value, format, date1904));
    }
    if show_percent && let Some(share) = share.filter(|share| share.is_finite()) {
        parts.push(numfmt::format(share, &NumFmt::Code(PERCENT_FORMAT.into()), date1904));
    }
    let text = parts.join(SEPARATOR);
    (!text.trim().is_empty()).then_some(text)
}

/// The width and height of the label `text`, in points.
pub(super) fn size(text: &str) -> (f32, f32) {
    (text_width(text, LABEL_SIZE_PT), LABEL_SIZE_PT)
}

/// The colour of a label on a fill of `color`: white on a dark fill, else
/// the main text colour.
pub(super) fn ink_on(color: Rgb) -> &'static str {
    let channel = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * channel(color[0]) + 0.7152 * channel(color[1]) + 0.0722 * channel(color[2]);
    if luminance < DARK_LUMINANCE { INK_ON_DARK } else { INK }
}

/// The colour of a label beside its mark, on the chart's surface.
pub(super) const INK_OUTSIDE: &str = INK_SECONDARY;
