//! A [`Chart`] drawn as SVG by our own renderer: bars and columns, lines,
//! areas and scatter on a plane (`plane`), pies, doughnuts and radars
//! (`round`), with the title and the legend the file asks for.
//!
//! The look follows the file: the series colours, the chart type and its
//! grouping, orientation, gap width and overlap, the legend position, the
//! axes' bounds, direction, deletion, gridlines and number formats. The
//! chrome (gridlines, axis lines, text) is quiet: hairline grey rules and
//! dark grey text, so that only the data is loud. Text is `<text>`, so it
//! stays selectable in HTML and vector in a PDF.
//!
//! The output stays small whatever the data: past a few thousand marks,
//! points are gathered into buckets (bars and areas show a bucket's mean,
//! lines its lowest and highest values), pies fold their smallest slices,
//! and legends and labels are cut to what fits.

mod axis;
mod labels;
mod plane;
mod round;
mod write;

use super::{Chart, ChartKind, Grouping, LegendPosition, PlotGroup, Rgb};
use write::{Svg, TextStyle, fit, hex, text_width};

/// The smallest size a chart is drawn at, in points.
pub(super) const MIN_SIZE_PT: (f32, f32) = (72.0, 54.0);
/// The largest size a chart is drawn at, in points.
pub(super) const MAX_SIZE_PT: (f32, f32) = (1440.0, 1440.0);
/// The size of a chart whose caller gives no usable one: Excel's default
/// chart, 5 by 3 inches.
const DEFAULT_SIZE_PT: (f32, f32) = (360.0, 216.0);
/// The surface gap between two segments of a stacked bar, in points.
pub(super) const SEGMENT_GAP_PT: f32 = 1.0;
/// The margin around the chart's content, in points.
const PADDING_PT: f32 = 8.0;
/// The space between the title or the legend and the plot, in points.
const SPACING_PT: f32 = 6.0;
/// The font size of the chart title, in points.
const TITLE_SIZE_PT: f32 = 13.0;
/// The font size of every other text (labels, legend, axis titles), in
/// points.
pub(super) const TEXT_SIZE_PT: f32 = 9.0;
/// The height of a line of text, as a ratio of its font size.
pub(super) const LINE_RATIO: f32 = 1.4;
/// Where the baseline of a text sits below the middle of its line, as a
/// ratio of the font size.
pub(super) const BASELINE_RATIO: f32 = 0.35;
/// The side of a legend swatch, in points.
const SWATCH_PT: f32 = 8.0;
/// The space between a swatch and its label, and between two legend
/// entries side by side, in points.
const LEGEND_GAP_PT: f32 = 4.0;
/// The most legend entries shown; the others are counted in a last
/// entry.
const MAX_LEGEND_ENTRIES: usize = 24;
/// The most rows of a legend above or below the plot.
const MAX_LEGEND_ROWS: usize = 3;
/// The widest a legend beside the plot may be, as a ratio of the chart's
/// width.
const SIDE_LEGEND_RATIO: f32 = 0.3;
/// The smallest plot, in points, that leaves room for a legend.
const MIN_PLOT_PT: f32 = 40.0;
/// The colour of the main text (the title).
pub(super) const INK: &str = "#0b0b0b";
/// The colour of the other texts.
pub(super) const INK_SECONDARY: &str = "#52514e";
/// The colour of the gridlines: one step off the surface.
pub(super) const GRID: &str = "#e1e0d9";
/// The colour of the axis lines.
pub(super) const AXIS_LINE: &str = "#c3c2b7";
/// The width of a hairline rule (gridlines, axes), in points: 1 pixel.
pub(super) const HAIRLINE_PT: f32 = 0.75;

/// A rectangle, in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Area {
    /// The left edge.
    pub(super) x: f32,
    /// The top edge.
    pub(super) y: f32,
    /// The width.
    pub(super) width: f32,
    /// The height.
    pub(super) height: f32,
}

impl Area {
    /// The right edge.
    pub(super) fn right(&self) -> f32 {
        self.x + self.width
    }

    /// The bottom edge.
    pub(super) fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// The area less `left`, `top`, `right` and `bottom` points, never
    /// less than nothing.
    pub(super) fn inset(&self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            x: self.x + left,
            y: self.y + top,
            width: (self.width - left - right).max(1.0),
            height: (self.height - top - bottom).max(1.0),
        }
    }
}

/// How a chart is drawn.
enum Plan<'c> {
    /// On a plane, with axes: bars, lines, areas, or scatter.
    Plane,
    /// A pie or a doughnut: the group.
    Round(&'c PlotGroup),
    /// A radar: a spoke per category.
    Radar,
}

/// One entry of the legend: a label and its colour.
struct Entry {
    /// The label.
    label: String,
    /// The colour of its swatch.
    color: Rgb,
}

/// The style of the small texts.
pub(super) const SMALL: TextStyle = TextStyle {
    size: TEXT_SIZE_PT,
    fill: INK_SECONDARY,
    anchor: "middle",
    bold: false,
    rotate: 0.0,
};

/// `chart` drawn as SVG at `size_pt` (width, height in points, clamped to
/// [`MIN_SIZE_PT`]..[`MAX_SIZE_PT`]); `None` for a chart that is not
/// drawn: one without cached data, one with a kind that is not drawn
/// (stock, bubble, surface, an extended chart), or one mixing kinds that
/// do not share a plot (a pie with bars).
pub(crate) fn svg(chart: &Chart, size_pt: (f32, f32)) -> Option<String> {
    let plan = plan(chart)?;
    let (width, height) = drawn_size(size_pt);
    let mut out = Svg::new(width, height);
    let mut area = Area {
        x: PADDING_PT,
        y: PADDING_PT,
        width: width - 2.0 * PADDING_PT,
        height: height - 2.0 * PADDING_PT,
    };
    if let Some(title) = &chart.title {
        let style = TextStyle {
            size: TITLE_SIZE_PT,
            fill: INK,
            bold: true,
            ..SMALL
        };
        let line = TITLE_SIZE_PT * LINE_RATIO;
        let text = fit(title, TITLE_SIZE_PT, area.width);
        out.text(
            "title",
            (
                area.x + area.width / 2.0,
                area.y + line / 2.0 + TITLE_SIZE_PT * BASELINE_RATIO,
            ),
            style,
            &text,
        );
        area = area.inset(0.0, line + SPACING_PT, 0.0, 0.0);
    }
    if let Some(position) = chart.legend {
        let (shown, total) = entries(chart, &plan, position);
        area = legend(&mut out, &shown, total, position, area);
    }
    match plan {
        Plan::Plane => plane::draw(&mut out, chart, area),
        Plan::Round(group) => round::pie(&mut out, group, area, chart.date1904),
        Plan::Radar => round::radar(&mut out, chart, area),
    }
    Some(out.finish())
}

/// The size a chart asked to be drawn `size_pt` large is drawn at: within
/// [`MIN_SIZE_PT`]..[`MAX_SIZE_PT`], Excel's default chart size for a side
/// that is not a positive number.
pub(crate) fn drawn_size(size_pt: (f32, f32)) -> (f32, f32) {
    (
        clamp(size_pt.0, MIN_SIZE_PT.0, MAX_SIZE_PT.0, DEFAULT_SIZE_PT.0),
        clamp(size_pt.1, MIN_SIZE_PT.1, MAX_SIZE_PT.1, DEFAULT_SIZE_PT.1),
    )
}

/// How `chart` is drawn; `None` when it is not.
fn plan(chart: &Chart) -> Option<Plan<'_>> {
    if !chart.has_data || chart.groups.is_empty() {
        return None;
    }
    let count = |test: fn(&ChartKind) -> bool| chart.groups.iter().filter(|group| test(&group.kind)).count();
    let all = chart.groups.len();
    let round = count(|kind| matches!(kind, ChartKind::Pie { .. } | ChartKind::Doughnut { .. }));
    let radar = count(|kind| matches!(kind, ChartKind::Radar { .. }));
    let scatter = count(|kind| matches!(kind, ChartKind::Scatter { .. }));
    let categories = count(|kind| {
        matches!(
            kind,
            ChartKind::Bar { .. } | ChartKind::Line { .. } | ChartKind::Area { .. }
        )
    });
    if round == all && all == 1 {
        Some(Plan::Round(&chart.groups[0]))
    } else if radar == all {
        Some(Plan::Radar)
    } else if scatter == all || categories == all {
        Some(Plan::Plane)
    } else {
        None
    }
}

/// `value` within `min..=max`; `default` when it is not a number.
fn clamp(value: f32, min: f32, max: f32, default: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value.clamp(min, max)
    } else {
        default
    }
}

/// The first entries of the legend of `chart` at `position`, one more
/// than the most shown, and how many there are in all. The entries are
/// the points (categories) of a pie or doughnut, or of a single series
/// whose colours vary by point, else the series: the top of a stack of
/// columns first in a legend beside them, as Excel lists them.
fn entries(chart: &Chart, plan: &Plan<'_>, position: LegendPosition) -> (Vec<Entry>, usize) {
    let mut series = chart.series();
    let varied = match plan {
        Plan::Round(group) => group.series.first(),
        Plan::Plane => match (series.next(), series.next()) {
            (Some(only), None) if chart.groups.iter().any(|group| group.vary_colors) => Some(only),
            _ => None,
        },
        Plan::Radar => None,
    };
    if let Some(series) = varied {
        let count = series.values.as_ref().map_or(0, |values| values.points.len());
        let labels = plane::category_labels(series.categories.as_ref(), count, chart.date1904);
        let total = labels.len();
        let shown = labels
            .into_iter()
            .enumerate()
            .take(MAX_LEGEND_ENTRIES + 1)
            .map(|(index, label)| Entry {
                label,
                color: series.point_colors.get(index).copied().unwrap_or(series.color),
            })
            .collect();
        return (shown, total);
    }
    let mut all: Vec<Entry> = chart
        .series()
        .map(|series| Entry {
            label: series.label(),
            color: series.color,
        })
        .collect();
    let stacked_columns = chart.groups.iter().any(|group| {
        matches!(
            group.kind,
            ChartKind::Bar {
                horizontal: false,
                grouping: Grouping::Stacked | Grouping::PercentStacked,
                ..
            }
        )
    });
    let side = matches!(
        position,
        LegendPosition::Left | LegendPosition::Right | LegendPosition::TopRight
    );
    if stacked_columns && side {
        all.reverse();
    }
    let total = all.len();
    all.truncate(MAX_LEGEND_ENTRIES + 1);
    (all, total)
}

/// Draws the legend `entries` (of `total` in all) at `position` of
/// `area`; returns what is left of the area for the plot. A legend that
/// would leave too small a plot is not drawn.
fn legend(out: &mut Svg, entries: &[Entry], total: usize, position: LegendPosition, area: Area) -> Area {
    if entries.is_empty() {
        return area;
    }
    let line = TEXT_SIZE_PT * LINE_RATIO;
    let entry_width = |label: &str| SWATCH_PT + LEGEND_GAP_PT + text_width(label, TEXT_SIZE_PT);
    let side = matches!(
        position,
        LegendPosition::Left | LegendPosition::Right | LegendPosition::TopRight
    );
    if side {
        let max_label = area.width * SIDE_LEGEND_RATIO - SWATCH_PT - LEGEND_GAP_PT;
        let rows = ((area.height / line).floor() as usize).clamp(1, MAX_LEGEND_ENTRIES);
        let shown = shown_entries(entries, total, rows, |label| fit(label, TEXT_SIZE_PT, max_label));
        let width = shown.iter().map(|(label, _)| entry_width(label)).fold(0.0, f32::max);
        if area.width - width - SPACING_PT < MIN_PLOT_PT {
            return area;
        }
        let x = if position == LegendPosition::Left {
            area.x
        } else {
            area.right() - width
        };
        let height = line * shown.len() as f32;
        let top = if position == LegendPosition::TopRight {
            area.y
        } else {
            area.y + (area.height - height) / 2.0
        };
        for (row, (label, color)) in shown.iter().enumerate() {
            entry(out, (x, top + line * row as f32 + line / 2.0), label, *color);
        }
        return if position == LegendPosition::Left {
            area.inset(width + SPACING_PT, 0.0, 0.0, 0.0)
        } else {
            area.inset(0.0, 0.0, width + SPACING_PT, 0.0)
        };
    }
    // Above or below: entries side by side, in rows, centred.
    let max_label = area.width - SWATCH_PT - LEGEND_GAP_PT;
    let mut rows: Vec<Vec<(String, Option<Rgb>)>> = vec![Vec::new()];
    let mut used = 0.0;
    for (label, color) in shown_entries(entries, total, MAX_LEGEND_ENTRIES, |label| {
        fit(label, TEXT_SIZE_PT, max_label)
    }) {
        let width = entry_width(&label) + 3.0 * LEGEND_GAP_PT;
        if used + width > area.width && !rows.last().is_none_or(Vec::is_empty) {
            if rows.len() == MAX_LEGEND_ROWS {
                break;
            }
            rows.push(Vec::new());
            used = 0.0;
        }
        used += width;
        rows.last_mut().expect("a row").push((label, color));
    }
    let height = line * rows.len() as f32;
    if area.height - height - SPACING_PT < MIN_PLOT_PT {
        return area;
    }
    let top = if position == LegendPosition::Top {
        area.y
    } else {
        area.bottom() - height
    };
    for (index, row) in rows.iter().enumerate() {
        let total: f32 = row
            .iter()
            .map(|(label, _)| entry_width(label) + 3.0 * LEGEND_GAP_PT)
            .sum::<f32>()
            - 3.0 * LEGEND_GAP_PT;
        let mut x = area.x + (area.width - total) / 2.0;
        for (label, color) in row {
            entry(out, (x, top + line * index as f32 + line / 2.0), label, *color);
            x += entry_width(label) + 3.0 * LEGEND_GAP_PT;
        }
    }
    if position == LegendPosition::Top {
        area.inset(0.0, height + SPACING_PT, 0.0, 0.0)
    } else {
        area.inset(0.0, 0.0, 0.0, height + SPACING_PT)
    }
}

/// The first of `entries` (of `total` in all) that fit in `room` rows,
/// their labels cut by `cut`; when some are left out, the last row counts
/// them instead.
fn shown_entries(
    entries: &[Entry],
    total: usize,
    room: usize,
    cut: impl Fn(&str) -> String,
) -> Vec<(String, Option<Rgb>)> {
    let room = room.max(1);
    if total <= room {
        return entries
            .iter()
            .map(|entry| (cut(&entry.label), Some(entry.color)))
            .collect();
    }
    let mut shown: Vec<(String, Option<Rgb>)> = entries[..(room - 1).min(entries.len())]
        .iter()
        .map(|entry| (cut(&entry.label), Some(entry.color)))
        .collect();
    let more = total - (room - 1);
    shown.push((format!("+{more} more"), None));
    shown
}

/// One legend entry at `at` (the left of its swatch, the middle of its
/// line): a swatch of `color` (none for the count of the entries left
/// out) and the label.
fn entry(out: &mut Svg, at: (f32, f32), label: &str, color: Option<Rgb>) {
    let (x, middle) = at;
    if let Some(color) = color {
        out.rect(
            "swatch",
            x,
            middle - SWATCH_PT / 2.0,
            SWATCH_PT,
            SWATCH_PT,
            &format!(r#"fill="{}""#, hex(color)),
        );
    }
    let style = TextStyle {
        anchor: "start",
        ..SMALL
    };
    out.text(
        "legend-item",
        (x + SWATCH_PT + LEGEND_GAP_PT, middle + TEXT_SIZE_PT * BASELINE_RATIO),
        style,
        label,
    );
}

#[cfg(test)]
mod tests;
