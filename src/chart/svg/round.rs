//! Round charts: pies, doughnuts (a ring per series) and radars (a spoke
//! per category, a polygon per series).

use super::axis::{Fixed, Scale, Zero, label as tick_label, range};
use super::labels::{self, LABEL_OFFSET_PT, Labels, ink_on};
use super::plane::{LINE_WIDTH_PT, MARKER_RADIUS_PT, TICK_SPACING_PT, category_labels};
use super::write::{PathData, Svg, TextStyle, fit, hex, num, text_width};
use super::{Area, BASELINE_RATIO, GRID, HAIRLINE_PT, LINE_RATIO, SMALL, TEXT_SIZE_PT};
use crate::chart::{Axis, AxisKind, Chart, ChartKind, PlotGroup, Rgb, Series};
use crate::excel::numfmt::NumFmt;

/// The most slices drawn over all rings; the smallest ones past it are
/// folded into one.
const MAX_SLICES: usize = 720;
/// The most rings of a doughnut.
const MAX_RINGS: usize = 10;
/// The labels of a pie are this share of the radius out from its centre.
const PIE_LABEL_RATIO: f32 = 0.66;
/// The colour of the slice the smallest ones are folded into.
const FOLDED_COLOR: Rgb = [0xc3, 0xc2, 0xb7];
/// The surface line between two slices, in points.
const SLICE_GAP_PT: f32 = 1.0;
/// A slice of a full turn is drawn a hair short of it, so that its arc
/// has two distinct ends.
const FULL_TURN_DEG: f32 = 359.95;
/// The most spokes of a radar; past it, categories are gathered.
const MAX_SPOKES: usize = 180;
/// The most markers of a radar.
const MAX_MARKERS: usize = 600;
/// The opacity of a filled radar.
const RADAR_FILL_OPACITY: f32 = 0.4;
/// Radar category labels take at most this share of the width, on each
/// side.
const RADAR_LABEL_RATIO: f32 = 0.2;
/// A radar label further than this (the sine of its angle) from the
/// vertical spoke starts at its spoke's end, away from the radar; nearer,
/// it is centred on it.
const RADAR_SIDE_SIN: f32 = 0.05;
/// The space between a radar and its labels, in points.
const RADAR_LABEL_GAP_PT: f32 = 4.0;
/// How many times a radar label that overlaps another moves away from the
/// radar before it is left out.
const RADAR_LABEL_NUDGES: usize = 4;
/// How far a radar label moves each time, in lines of its text.
const RADAR_NUDGE_RATIO: f32 = 0.5;

/// Draws the pie or doughnut `group` in `area`: the first series of a
/// pie, a ring per series (the first outermost) of a doughnut, with the
/// data labels the file asks for inside the slices they fit in.
pub(super) fn pie(out: &mut Svg, group: &PlotGroup, area: Area, date1904: bool) {
    let (start, hole) = match group.kind {
        ChartKind::Pie { first_slice_deg } => (first_slice_deg, 0.0),
        ChartKind::Doughnut {
            first_slice_deg,
            hole_ratio,
        } => (first_slice_deg, hole_ratio),
        _ => return,
    };
    let center = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let radius = (area.width.min(area.height) / 2.0 - SLICE_GAP_PT).max(1.0);
    let rings: Vec<&Series> = if hole > 0.0 {
        group.series.iter().take(MAX_RINGS).collect()
    } else {
        group.series.iter().take(1).collect()
    };
    let thickness = radius * (1.0 - hole) / rings.len().max(1) as f32;
    let per_ring = MAX_SLICES / rings.len().max(1);
    let mut labels = Labels::new();
    for (ring, series) in rings.iter().enumerate() {
        let outer = radius - ring as f32 * thickness;
        let inner = if hole > 0.0 { outer - thickness } else { 0.0 };
        let slices = slices(series, per_ring);
        let total: f64 = slices.iter().map(|(_, value, _)| value).sum();
        if total <= 0.0 {
            continue;
        }
        let mut angle = start;
        for (index, value, color) in slices {
            let sweep = (value / total * 360.0) as f32;
            if let Some(text) = labels::text(series, index, value, Some(value / total), date1904) {
                let ring = Ring { outer, inner };
                slice_label(center, ring, angle + sweep / 2.0, sweep, text, color, &mut labels);
            }
            let data = slice_path(center, outer, inner, angle, sweep.min(FULL_TURN_DEG));
            out.path(
                "slice",
                &data,
                &format!(
                    r##"fill="{}" stroke="#ffffff" stroke-width="{}""##,
                    hex(color),
                    num(SLICE_GAP_PT)
                ),
            );
            angle += sweep;
        }
    }
    labels.write(out);
}

/// The radii of a ring of a doughnut, or of a pie (`inner` 0).
#[derive(Debug, Clone, Copy)]
struct Ring {
    /// The outer radius.
    outer: f32,
    /// The inner radius.
    inner: f32,
}

/// Adds the label `text` of a slice of `ring` about `center`, whose middle
/// is at `angle` and which sweeps `sweep` degrees, to `labels`, inside the
/// slice when it fits there (half way across a ring, two thirds of the
/// way out of a pie); a label that does not fit is left out.
fn slice_label(center: (f32, f32), ring: Ring, angle: f32, sweep: f32, text: String, color: Rgb, labels: &mut Labels) {
    let radius = if ring.inner > 0.0 {
        f32::midpoint(ring.inner, ring.outer)
    } else {
        ring.outer * PIE_LABEL_RATIO
    };
    let (width, height) = labels::size(&text);
    // The room across the slice at that radius, and along its radius.
    let chord = 2.0 * radius * (sweep.min(180.0).to_radians() / 2.0).sin();
    let depth = if ring.inner > 0.0 {
        ring.outer - ring.inner
    } else {
        2.0 * (ring.outer - radius)
    };
    if chord < width.min(height * 2.0) || depth < height + LABEL_OFFSET_PT {
        return;
    }
    labels.add(polar(center, radius, angle), text, ink_on(color));
}

/// The slices of `series`: its positive values with their point index and
/// colours, the smallest folded into one past `max` slices (whose index is
/// `usize::MAX`).
fn slices(series: &Series, max: usize) -> Vec<(usize, f64, Rgb)> {
    let points = series
        .values
        .as_ref()
        .map(|values| values.points.as_slice())
        .unwrap_or_default();
    let mut slices: Vec<(usize, f64, Rgb)> = points
        .iter()
        .enumerate()
        .filter_map(|(index, value)| {
            let value = (*value)?;
            let color = series.point_colors.get(index).copied().unwrap_or(series.color);
            (value > 0.0).then_some((index, value, color))
        })
        .collect();
    let max = max.max(2);
    if slices.len() > max {
        // Keep the largest, in their order; fold the rest.
        let mut by_size: Vec<usize> = (0..slices.len()).collect();
        by_size.sort_by(|a, b| slices[*b].1.total_cmp(&slices[*a].1));
        let mut kept = vec![false; slices.len()];
        for &index in &by_size[..max - 1] {
            kept[index] = true;
        }
        let folded: f64 = slices
            .iter()
            .zip(&kept)
            .filter(|(_, kept)| !**kept)
            .map(|(slice, _)| slice.1)
            .sum();
        let mut index = 0;
        slices.retain(|_| {
            index += 1;
            kept[index - 1]
        });
        slices.push((usize::MAX, folded, FOLDED_COLOR));
    }
    slices
}

/// The point at `angle` degrees clockwise from the top, `radius` from
/// `center`.
fn polar(center: (f32, f32), radius: f32, angle: f32) -> (f32, f32) {
    let radians = angle.to_radians();
    (center.0 + radius * radians.sin(), center.1 - radius * radians.cos())
}

/// The path of a slice from `start` over `sweep` degrees: a wedge from
/// the centre, or a band between `inner` and `outer` radii.
fn slice_path(center: (f32, f32), outer: f32, inner: f32, start: f32, sweep: f32) -> String {
    let large = u8::from(sweep > 180.0);
    let (x1, y1) = polar(center, outer, start);
    let (x2, y2) = polar(center, outer, start + sweep);
    let outer_r = num(outer);
    if inner <= 0.0 {
        return format!(
            "M {} {} L {} {} A {outer_r} {outer_r} 0 {large} 1 {} {} Z",
            num(center.0),
            num(center.1),
            num(x1),
            num(y1),
            num(x2),
            num(y2)
        );
    }
    let (x3, y3) = polar(center, inner, start + sweep);
    let (x4, y4) = polar(center, inner, start);
    let ri = num(inner);
    format!(
        "M {} {} A {outer_r} {outer_r} 0 {large} 1 {} {} L {} {} A {ri} {ri} 0 {large} 0 {} {} Z",
        num(x1),
        num(y1),
        num(x2),
        num(y2),
        num(x3),
        num(y3),
        num(x4),
        num(y4)
    )
}

/// Draws the radar groups of `chart` in `area`: the value rings and the
/// spokes, the category labels around, a polygon per series.
pub(super) fn radar(out: &mut Svg, chart: &Chart, area: Area) {
    let series: Vec<&Series> = chart.series().collect();
    let total = series
        .iter()
        .map(|series| series.values.as_ref().map_or(0, |values| values.points.len()))
        .max()
        .unwrap_or(0)
        .max(1);
    let size = total.div_ceil(MAX_SPOKES).max(1);
    let spokes = total.div_ceil(size);
    let all_labels = category_labels(
        series.iter().find_map(|series| series.categories.as_ref()),
        total,
        chart.date1904,
    );
    let labels: Vec<String> = (0..spokes).map(|spoke| all_labels[spoke * size].clone()).collect();
    let axis = value_axis(chart);
    let widest = labels
        .iter()
        .map(|label| text_width(label, TEXT_SIZE_PT))
        .fold(0.0, f32::max)
        .min(area.width * RADAR_LABEL_RATIO);
    let line = TEXT_SIZE_PT * LINE_RATIO;
    let radius = ((area.width / 2.0 - widest - RADAR_LABEL_GAP_PT).min(area.height / 2.0 - line)).max(4.0);
    let center = (area.x + area.width / 2.0, area.y + area.height / 2.0);
    let means: Vec<Vec<Option<f64>>> = series.iter().map(|series| spoke_means(series, spokes, size)).collect();
    let scale = Scale::nice(
        range(means.iter().flatten().flatten().copied()),
        Fixed::of(axis),
        Zero::Always,
        // Excel's radar scale is not known to keep room past the data.
        ((radius / TICK_SPACING_PT) as usize, false),
        false,
    );
    grid(out, center, radius, spokes, &scale, axis);
    let filled = chart
        .groups
        .iter()
        .any(|group| matches!(group.kind, ChartKind::Radar { filled: true }));
    polygons(out, (center, radius), &series, &means, &scale, filled);
    radar_labels(
        out,
        center,
        radius,
        &labels,
        widest,
        axis,
        &scale,
        chart.date1904,
        &series,
    );
}

/// The mean of the values of `series` at each of `spokes` spokes, each
/// gathering `size` categories.
fn spoke_means(series: &Series, spokes: usize, size: usize) -> Vec<Option<f64>> {
    let points = series
        .values
        .as_ref()
        .map(|values| values.points.as_slice())
        .unwrap_or_default();
    (0..spokes)
        .map(|spoke| {
            let present: Vec<f64> = points
                .iter()
                .skip(spoke * size)
                .take(size)
                .filter_map(|value| *value)
                .collect();
            (!present.is_empty()).then(|| present.iter().sum::<f64>() / present.len() as f64)
        })
        .collect()
}

/// Draws a polygon per series of a radar of `circle` (centre, radius)
/// through its `means` on `scale`, filled or not, with its markers; a
/// missing value opens the polygon.
fn polygons(
    out: &mut Svg,
    circle: ((f32, f32), f32),
    series: &[&Series],
    means: &[Vec<Option<f64>>],
    scale: &Scale,
    filled: bool,
) {
    let (center, radius) = circle;
    let mut budget = MAX_MARKERS;
    for (series, values) in series.iter().zip(means) {
        let angle = |spoke: usize| 360.0 * spoke as f32 / values.len() as f32;
        let mut path = PathData::new();
        let mut dots = Vec::new();
        for (spoke, value) in values.iter().enumerate() {
            match value {
                Some(value) => {
                    let at = polar(center, scale.at(*value, 0.0, radius), angle(spoke));
                    path.point(at);
                    dots.push(at);
                }
                None => path.gap(),
            }
        }
        if values.iter().all(Option::is_some) && !values.is_empty() {
            path.close();
        }
        let color = hex(series.color);
        let attrs = if filled {
            format!(
                r#"fill="{color}" fill-opacity="{}" stroke="{color}" stroke-width="{}""#,
                num(RADAR_FILL_OPACITY),
                num(HAIRLINE_PT)
            )
        } else {
            format!(
                r#"fill="none" stroke="{color}" stroke-width="{}" stroke-linejoin="round""#,
                num(LINE_WIDTH_PT)
            )
        };
        out.path("radar", &path.into_string(), &attrs);
        if series.markers && dots.len() <= budget {
            budget -= dots.len();
            for at in dots {
                out.marker(at, MARKER_RADIUS_PT, series.color);
            }
        }
    }
}

/// The value axis of a radar chart.
fn value_axis(chart: &Chart) -> Option<&Axis> {
    let ids = &chart.groups.first()?.axis_ids;
    chart
        .axes
        .iter()
        .find(|axis| axis.kind == AxisKind::Value && ids.contains(&axis.id))
}

/// The rings at the ticks of `scale` (polygons over the spokes, as Excel
/// draws them, when the axis has gridlines) and the spokes.
fn grid(out: &mut Svg, center: (f32, f32), radius: f32, spokes: usize, scale: &Scale, axis: Option<&Axis>) {
    let angle = |spoke: usize| 360.0 * spoke as f32 / spokes as f32;
    if axis.is_none_or(|axis| axis.major_gridlines) {
        for tick in scale.ticks() {
            let ring = scale.at(tick, 0.0, radius);
            let mut path = PathData::new();
            for spoke in 0..spokes {
                path.point(polar(center, ring, angle(spoke)));
            }
            path.close();
            out.path(
                "grid",
                &path.into_string(),
                &format!(r#"fill="none" stroke="{GRID}" stroke-width="{}""#, num(HAIRLINE_PT)),
            );
        }
    }
    for spoke in 0..spokes {
        out.line("spoke", center, polar(center, radius, angle(spoke)), GRID, HAIRLINE_PT);
    }
}

/// The category labels around a radar, thinned to fit, and the value
/// labels up its first spoke.
#[allow(clippy::too_many_arguments, reason = "the parts of one layout")]
fn radar_labels(
    out: &mut Svg,
    center: (f32, f32),
    radius: f32,
    labels: &[String],
    widest: f32,
    axis: Option<&Axis>,
    scale: &Scale,
    date1904: bool,
    series: &[&Series],
) {
    let spokes = labels.len();
    let spacing = std::f32::consts::TAU * radius / spokes as f32;
    let step = ((TEXT_SIZE_PT * LINE_RATIO) / spacing.max(0.01)).ceil().max(1.0) as usize;
    let mut placed: Vec<[f32; 4]> = Vec::new();
    for (spoke, label) in labels.iter().enumerate().step_by(step) {
        let angle = 360.0 * spoke as f32 / spokes as f32;
        let text = fit(label, TEXT_SIZE_PT, widest.max(TEXT_SIZE_PT));
        // A label that would overlap one already placed moves away from
        // the radar, as far as a line or two; further, it is left out.
        let spot = (0..=RADAR_LABEL_NUDGES).find_map(|nudge| {
            #[allow(clippy::cast_precision_loss)]
            let out_by = nudge as f32 * TEXT_SIZE_PT * RADAR_NUDGE_RATIO;
            let spot = radar_label_spot(center, radius + RADAR_LABEL_GAP_PT + out_by, angle, &text);
            (!placed.iter().any(|other| overlap(&spot.2, other))).then_some(spot)
        });
        let Some((at, anchor, bounds)) = spot else {
            continue;
        };
        placed.push(bounds);
        out.text("category", at, TextStyle { anchor, ..SMALL }, &text);
    }
    if axis.is_some_and(|axis| axis.deleted || !axis.tick_labels) {
        return;
    }
    let format = axis
        .and_then(|axis| axis.format.clone())
        .or_else(|| {
            series
                .iter()
                .find_map(|series| series.values.as_ref().map(|values| values.format.clone()))
        })
        .unwrap_or(NumFmt::General);
    for tick in scale.ticks() {
        let at = (
            center.0 - RADAR_LABEL_GAP_PT,
            center.1 - scale.at(tick, 0.0, radius) + TEXT_SIZE_PT * BASELINE_RATIO,
        );
        out.text(
            "tick",
            at,
            TextStyle { anchor: "end", ..SMALL },
            &tick_label(tick, &format, date1904),
        );
    }
}

/// Where the label `text` of the spoke at `angle` degrees goes, `reach`
/// points from `center`: its point, its anchor and its bounds.
fn radar_label_spot(center: (f32, f32), reach: f32, angle: f32, text: &str) -> ((f32, f32), &'static str, [f32; 4]) {
    let (x, y) = polar(center, reach, angle);
    let sin = angle.to_radians().sin();
    let cos = angle.to_radians().cos();
    let anchor = if sin > RADAR_SIDE_SIN {
        "start"
    } else if sin < -RADAR_SIDE_SIN {
        "end"
    } else {
        "middle"
    };
    // Above the top, below the bottom, centred on the sides.
    let lift = if cos > 0.0 {
        BASELINE_RATIO
    } else {
        1.0 - BASELINE_RATIO
    };
    let y = y + TEXT_SIZE_PT * (BASELINE_RATIO - cos * lift);
    let width = text_width(text, TEXT_SIZE_PT);
    let left = match anchor {
        "start" => x,
        "end" => x - width,
        _ => x - width / 2.0,
    };
    let bounds = [
        left,
        y - TEXT_SIZE_PT,
        left + width,
        y + TEXT_SIZE_PT * (1.0 - BASELINE_RATIO) / 2.0,
    ];
    ((x, y), anchor, bounds)
}

/// Whether the boxes `one` and `other` (left, top, right, bottom) overlap.
fn overlap(one: &[f32; 4], other: &[f32; 4]) -> bool {
    one[0] < other[2] && other[0] < one[2] && one[1] < other[3] && other[1] < one[3]
}
