//! Charts on a plane with axes: bars and columns, lines and areas over
//! categories (a bar and line combination with a secondary value axis
//! among them), and scatter over two value axes.

use super::axis::{Fixed, Scale, Zero, label as tick_label, range, readable};
use super::labels::{self, INK_OUTSIDE, LABEL_OFFSET_PT, Labels, ink_on};
use super::write::{PathData, Svg, TextStyle, fit, hex, num, text_width};
use super::{AXIS_LINE, Area, BASELINE_RATIO, GRID, HAIRLINE_PT, LINE_RATIO, SEGMENT_GAP_PT, SMALL, TEXT_SIZE_PT};
use crate::chart::Rgb;
use crate::chart::{
    Axis, AxisKind, AxisPosition, Categories, Chart, ChartKind, Grouping, LabelPosition, PlotGroup, Series,
};
use crate::excel::numfmt::{self, NumFmt};
use std::fmt::Write as _;
use std::ops::Range;

/// The most bars drawn; past it, categories are gathered into buckets.
const MAX_BARS: usize = 1_500;
/// The most points of lines and areas drawn, over all series; past it,
/// categories are gathered into buckets (or scatter points sampled).
const MAX_VERTICES: usize = 10_000;
/// The most markers drawn.
const MAX_MARKERS: usize = 600;
/// Markers are drawn only when the points are at least this far apart,
/// in points.
const MARKER_SPACING_PT: f32 = 6.0;
/// The radius of a marker, in points: a dot of 8 pixels.
pub(super) const MARKER_RADIUS_PT: f32 = 3.0;
/// The width of a line, in points: 2 pixels.
pub(super) const LINE_WIDTH_PT: f32 = 1.5;
/// The length of a dash of a dashed line, as a ratio of its width.
const DASH_RATIO: f32 = 3.0;
/// The gap between two dashes of a dashed line, as a ratio of its width.
const DASH_GAP_RATIO: f32 = 2.0;
/// The divisor of the neighbours' difference that gives a smoothed line's
/// tangent at a point (Catmull-Rom as Bézier curves).
const SPLINE_DIVISOR: f32 = 6.0;
/// A value axis along the height has a tick every this many points,
/// about.
pub(super) const TICK_SPACING_PT: f32 = 20.0;
/// A value axis along the width has a tick every this many points, about.
const WIDE_TICK_SPACING_PT: f32 = 48.0;
/// The widest a category label is drawn, in points.
const MAX_LABEL_PT: f32 = 110.0;
/// Category labels beside the plot take at most this share of the width.
const SIDE_LABEL_RATIO: f32 = 0.35;
/// Turned category labels take at most this share of the height.
const TURNED_LABEL_RATIO: f32 = 0.35;
/// The angle of turned category labels, in degrees.
const TURN_DEG: f32 = -45.0;
/// The space between a label and the plot, in points.
pub(super) const LABEL_GAP_PT: f32 = 4.0;
/// The opacity of an area in front of others.
const AREA_OPACITY: f32 = 0.75;
/// A clustered bar narrower than this, in points, keeps its full width;
/// a wider one leaves a surface gap to its neighbour.
const MIN_INSET_BAR_PT: f32 = 4.0;
/// The most category gridlines drawn.
const MAX_CATEGORY_GRIDLINES: usize = 100;

/// Categories gathered into buckets of `size`, `count` of them over `total`
/// categories.
#[derive(Debug, Clone, Copy)]
struct Buckets {
    /// Categories per bucket.
    size: usize,
    /// Buckets.
    count: usize,
    /// Categories.
    total: usize,
}

impl Buckets {
    /// The categories of bucket `index`.
    fn range(&self, index: usize) -> Range<usize> {
        let start = index * self.size;
        start..(start + self.size).min(self.total)
    }
}

/// A value axis of the plane, with its scale.
struct ValueAxis<'c> {
    /// The axis of the file, if it has one.
    axis: Option<&'c Axis>,
    /// The number format of its labels.
    format: NumFmt,
    /// The scale.
    scale: Scale,
}

impl ValueAxis<'_> {
    /// The tick labels are shown: the axis is not deleted and its labels
    /// are not hidden (`tickLblPos="none"`).
    fn labelled(&self) -> bool {
        self.axis.is_none_or(|axis| !axis.deleted && axis.tick_labels)
    }

    /// The title, when the axis is shown and has one.
    fn title(&self) -> Option<&str> {
        self.axis.filter(|axis| !axis.deleted)?.title.as_deref()
    }
}

/// The values a group draws per bucket, per series: where each bar or
/// band starts (zero, or the top of the series below it in a stack) and
/// where it ends; `None` for a point not drawn.
type Spans = Vec<Vec<Option<(f64, f64)>>>;

/// Where the plot is and how categories and values map to it.
struct Frame {
    /// The plot area.
    plot: Area,
    /// Categories run up the height (horizontal bars).
    horizontal: bool,
    /// The category axis runs backwards (`maxMin`).
    reversed: bool,
    /// The buckets of categories.
    buckets: Buckets,
    /// The categories sit on the plot's edges and between them
    /// (`crossBetween="midCat"`), not in the middle of bands.
    edges: bool,
}

impl Frame {
    /// The length of the category axis.
    fn length(&self) -> f32 {
        if self.horizontal {
            self.plot.height
        } else {
            self.plot.width
        }
    }

    /// The width of a bucket along the category axis.
    fn band(&self) -> f32 {
        self.length() / self.buckets.count as f32
    }

    /// The middle of bucket `index` along the category axis (or its place
    /// from edge to edge).
    fn center(&self, index: usize) -> f32 {
        let offset = if self.edges && self.buckets.count > 1 {
            index as f32 * self.length() / (self.buckets.count - 1) as f32
        } else {
            (index as f32 + 0.5) * self.band()
        };
        match (self.horizontal, self.reversed) {
            (false, false) => self.plot.x + offset,
            (false, true) => self.plot.right() - offset,
            // The first category of horizontal bars is at the bottom.
            (true, false) => self.plot.bottom() - offset,
            (true, true) => self.plot.y + offset,
        }
    }

    /// Where `value` of `scale` falls along the value axis.
    fn value(&self, scale: &Scale, value: f64) -> f32 {
        if self.horizontal {
            scale.at(value, self.plot.x, self.plot.right())
        } else {
            scale.at(value, self.plot.bottom(), self.plot.y)
        }
    }

    /// The point at `along` the category axis and `across` it.
    fn point(&self, along: f32, across: f32) -> (f32, f32) {
        if self.horizontal {
            (across, along)
        } else {
            (along, across)
        }
    }
}

/// How the category labels are laid out.
#[derive(Debug, Clone, Copy)]
struct LabelLayout {
    /// Every `step`-th bucket is labelled.
    step: usize,
    /// The labels are turned.
    turned: bool,
    /// The room they take beside or below the plot.
    extent: f32,
    /// The widest a label may be.
    max_width: f32,
}

/// How the category axis shows its labels.
struct CategoryText<'c> {
    /// The labels are shown.
    labelled: bool,
    /// The axis title.
    title: Option<&'c str>,
    /// Every this many buckets at least is labelled, when the file says.
    skip: Option<usize>,
}

/// Draws `chart`, whose groups are all on a plane, in `area`.
pub(super) fn draw(out: &mut Svg, chart: &Chart, area: Area) {
    if chart
        .groups
        .iter()
        .all(|group| matches!(group.kind, ChartKind::Scatter { .. }))
    {
        scatter(out, chart, area);
    } else {
        categories(out, chart, area);
    }
}

/// The labels of `count` categories (at least): the innermost level of
/// text categories, numbers in their format, else 1, 2, ...
pub(super) fn category_labels(categories: Option<&Categories>, count: usize, date1904: bool) -> Vec<String> {
    let known = match categories {
        Some(Categories::Text(levels)) => levels.iter().map(Vec::len).max().unwrap_or(0),
        Some(Categories::Numbers(numbers)) => numbers.points.len(),
        None => 0,
    };
    (0..count.max(known))
        .map(|index| match categories {
            Some(Categories::Text(levels)) => levels
                .last()
                .and_then(|level| level.get(index).cloned().flatten())
                .unwrap_or_default(),
            Some(Categories::Numbers(numbers)) => numbers
                .points
                .get(index)
                .copied()
                .flatten()
                .map(|value| numfmt::format(value, &numbers.format, date1904))
                .unwrap_or_default(),
            None => (index + 1).to_string(),
        })
        .collect()
}

/// Draws bars, lines and areas over categories.
fn categories(out: &mut Svg, chart: &Chart, area: Area) {
    let groups = &chart.groups;
    let horizontal = groups
        .iter()
        .any(|group| matches!(group.kind, ChartKind::Bar { horizontal: true, .. }));
    let series: Vec<&Series> = chart.series().collect();
    let total = series
        .iter()
        .map(|series| values_len(series).max(categories_len(series.categories.as_ref())))
        .max()
        .unwrap_or(0)
        .max(1);
    let buckets = buckets(groups, total, area, horizontal);
    let all = category_labels(
        series.iter().find_map(|series| series.categories.as_ref()),
        total,
        chart.date1904,
    );
    let labels: Vec<String> = (0..buckets.count)
        .map(|index| all[buckets.range(index).start].clone())
        .collect();
    // The category axis of the first group; the value axes.
    let category_axis = group_axes(chart, &groups[0]).0;
    let spans: Vec<Spans> = groups.iter().map(|group| group_spans(group, &buckets)).collect();
    let (mut axes, on_axis) = value_axes(chart, &spans);
    let cat_shown = !category_axis.is_some_and(|axis| axis.deleted);
    let cat = CategoryText {
        labelled: cat_shown && category_axis.is_none_or(|axis| axis.tick_labels),
        title: category_axis
            .filter(|axis| !axis.deleted)
            .and_then(|axis| axis.title.as_deref()),
        skip: category_axis
            .and_then(|axis| axis.label_skip)
            .map(|skip| skip.div_ceil(buckets.size)),
    };
    let (plot, layout) = lay_out(area, horizontal, &mut axes, &labels, &cat, groups, &spans, &on_axis);
    let bars_drawn = groups.iter().any(|group| matches!(group.kind, ChartKind::Bar { .. }));
    let frame = Frame {
        plot,
        horizontal,
        reversed: category_axis.is_some_and(|axis| axis.reversed),
        buckets,
        edges: !bars_drawn && axes[0].axis.is_some_and(|axis| !axis.cross_between),
    };
    out.rect("plot", plot.x, plot.y, plot.width, plot.height, r#"fill="none""#);
    gridlines(out, &frame, &axes, category_axis);
    marks(out, &frame, groups, &spans, &axes, &on_axis, chart.date1904);
    if cat_shown {
        let zero = frame.value(&axes[0].scale, 0.0);
        let (from, to) = if horizontal {
            ((zero, plot.y), (zero, plot.bottom()))
        } else {
            ((plot.x, zero), (plot.right(), zero))
        };
        out.line("axis", from, to, AXIS_LINE, HAIRLINE_PT);
    }
    if cat.labelled {
        category_text(out, &frame, &labels, &layout);
    }
    value_text(out, &frame, &axes, chart.date1904);
    titles(out, area, &frame, &axes, cat.title);
}

/// The value axes of the groups of `chart` (the value axis of each group;
/// a second one is the secondary axis, a third shares the primary), and
/// the axis of each group.
fn value_axes<'c>(chart: &'c Chart, spans: &[Spans]) -> (Vec<ValueAxis<'c>>, Vec<usize>) {
    let groups = &chart.groups;
    let mut value_ids: Vec<Option<u32>> = Vec::new();
    let on_axis: Vec<usize> = groups
        .iter()
        .map(|group| {
            let id = group_axes(chart, group).1.map(|axis| axis.id);
            match value_ids.iter().position(|known| *known == id) {
                Some(index) => index,
                None if value_ids.len() < 2 => {
                    value_ids.push(id);
                    value_ids.len() - 1
                }
                None => 0,
            }
        })
        .collect();
    let axes = value_ids
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let axis = id.and_then(|id| chart.axes.iter().find(|axis| axis.id == id));
            let mine: Vec<usize> = (0..groups.len()).filter(|group| on_axis[*group] == index).collect();
            value_axis(axis, &mine, groups, spans, 5)
        })
        .collect();
    (axes, on_axis)
}

/// Draws the marks of `groups`: areas behind bars, bars behind lines, as
/// Excel draws them, then the data labels over them.
fn marks(
    out: &mut Svg,
    frame: &Frame,
    groups: &[PlotGroup],
    spans: &[Spans],
    axes: &[ValueAxis<'_>],
    on_axis: &[usize],
    date1904: bool,
) {
    let mut budget = MAX_MARKERS;
    let mut labels = Labels::new();
    for rank in 0..3 {
        for (index, group) in groups.iter().enumerate() {
            let scale = &axes[on_axis[index]].scale;
            match (&group.kind, rank) {
                (ChartKind::Area { .. }, 0) => areas(out, frame, group, &spans[index], scale),
                (ChartKind::Bar { .. }, 1) => {
                    bars(out, frame, group, &spans[index], scale, &mut labels, date1904);
                }
                (ChartKind::Line { grouping }, 2) => {
                    lines(out, frame, group, &spans[index], *grouping, scale, &mut budget);
                    line_labels(frame, group, &spans[index], *grouping, scale, &mut labels, date1904);
                }
                _ => {}
            }
        }
    }
    labels.write(out);
}

/// The buckets of `total` categories: one category each, unless the bars
/// or the line points would be more than the most drawn.
fn buckets(groups: &[PlotGroup], total: usize, area: Area, horizontal: bool) -> Buckets {
    let count_series = |test: fn(&ChartKind) -> bool| -> usize {
        groups
            .iter()
            .filter(|group| test(&group.kind))
            .map(|group| group.series.len())
            .sum()
    };
    let bar_series = count_series(|kind| matches!(kind, ChartKind::Bar { .. }));
    let line_series = count_series(|kind| matches!(kind, ChartKind::Line { .. } | ChartKind::Area { .. }));
    let length = if horizontal { area.height } else { area.width };
    let by_bars = (total * bar_series).div_ceil(MAX_BARS);
    // A line bucket draws two points, its lowest and highest.
    let by_lines = (2 * total * line_series).div_ceil(MAX_VERTICES);
    // Never more than two buckets per point of length.
    let by_length = total.div_ceil((2.0 * length).max(1.0) as usize);
    let size = by_bars.max(by_lines).max(by_length).max(1);
    Buckets {
        size,
        count: total.div_ceil(size),
        total,
    }
}

/// The category axis and the value axis of `group`, among the chart's
/// axes; `None` for one the file does not have.
fn group_axes<'c>(chart: &'c Chart, group: &PlotGroup) -> (Option<&'c Axis>, Option<&'c Axis>) {
    let mine: Vec<&Axis> = group
        .axis_ids
        .iter()
        .filter_map(|id| chart.axes.iter().find(|axis| axis.id == *id))
        .collect();
    let category = mine
        .iter()
        .find(|axis| matches!(axis.kind, AxisKind::Category | AxisKind::Date))
        .copied();
    let value = mine.iter().find(|axis| axis.kind == AxisKind::Value).copied();
    (category, value)
}

/// The spans `group` draws per bucket: the bar or band of each series
/// (from zero, or stacked on the previous series, scaled to 1 when the
/// group is stacked to 100 %), from the mean of the bucket's values.
fn group_spans(group: &PlotGroup, buckets: &Buckets) -> Spans {
    let grouping = match group.kind {
        ChartKind::Bar { grouping, .. } | ChartKind::Line { grouping } | ChartKind::Area { grouping } => grouping,
        _ => Grouping::Standard,
    };
    let means: Vec<Vec<Option<f64>>> = group
        .series
        .iter()
        .map(|series| {
            (0..buckets.count)
                .map(|index| mean(series, buckets.range(index)))
                .collect()
        })
        .collect();
    if !matches!(grouping, Grouping::Stacked | Grouping::PercentStacked) {
        return means
            .iter()
            .map(|values| values.iter().map(|value| value.map(|value| (0.0, value))).collect())
            .collect();
    }
    let mut spans = vec![vec![None; buckets.count]; means.len()];
    for bucket in 0..buckets.count {
        let whole: f64 = means.iter().filter_map(|values| values[bucket]).map(f64::abs).sum();
        let scale = if grouping == Grouping::PercentStacked && whole > 0.0 {
            1.0 / whole
        } else {
            1.0
        };
        let (mut above, mut below) = (0.0, 0.0);
        for (series, values) in means.iter().enumerate() {
            // A missing point of a stack counts as zero, as in Excel.
            let value = values[bucket].map(|value| value * scale);
            let filled = value.unwrap_or(0.0);
            let span = if filled >= 0.0 {
                above += filled;
                (above - filled, above)
            } else {
                below += filled;
                (below - filled, below)
            };
            spans[series][bucket] = value.map(|_| span);
        }
    }
    spans
}

/// The mean of the values of `series` at the categories `range`; `None`
/// when none is there.
fn mean(series: &Series, range: Range<usize>) -> Option<f64> {
    let points = &series.values.as_ref()?.points;
    let present: Vec<f64> = points.get(range)?.iter().filter_map(|value| *value).collect();
    (!present.is_empty()).then(|| present.iter().sum::<f64>() / present.len() as f64)
}

/// The value axis `axis` of the groups `mine` (indices in `groups`), with
/// the scale of their spans (standard lines: of their points), aiming at
/// `target` ticks.
fn value_axis<'c>(
    axis: Option<&'c Axis>,
    mine: &[usize],
    groups: &[PlotGroup],
    spans: &[Spans],
    target: usize,
) -> ValueAxis<'c> {
    let mut values = Vec::new();
    let mut zero = Zero::Auto;
    let mut percent = false;
    for &index in mine {
        let group = &groups[index];
        match group.kind {
            ChartKind::Bar { grouping, .. } | ChartKind::Area { grouping } => {
                zero = Zero::Always;
                percent |= grouping == Grouping::PercentStacked;
            }
            ChartKind::Line { grouping } => percent |= grouping == Grouping::PercentStacked,
            _ => {}
        }
        let stacked = spans[index].iter().flatten().flatten().any(|(from, _)| *from != 0.0);
        if matches!(
            group.kind,
            ChartKind::Line {
                grouping: Grouping::Standard
            }
        ) || !stacked
        {
            values.extend(
                group
                    .series
                    .iter()
                    .filter_map(|series| series.values.as_ref())
                    .flat_map(|values| values.points.iter().flatten().copied()),
            );
        }
        if !matches!(
            group.kind,
            ChartKind::Line {
                grouping: Grouping::Standard
            }
        ) {
            values.extend(
                spans[index]
                    .iter()
                    .flatten()
                    .flatten()
                    .flat_map(|(from, to)| [*from, *to]),
            );
        }
    }
    let format = axis
        .and_then(|axis| axis.format.clone())
        .or_else(|| percent.then(|| NumFmt::Code("0%".into())))
        .or_else(|| {
            mine.iter()
                .flat_map(|&index| &groups[index].series)
                .find_map(|series| series.values.as_ref().map(|values| values.format.clone()))
        })
        .unwrap_or_default();
    ValueAxis {
        axis,
        scale: readable(
            |target| {
                Scale::nice(
                    range(values.iter().copied()),
                    Fixed::of(axis),
                    zero,
                    target,
                    axis.is_some_and(|axis| axis.reversed),
                )
            },
            target,
            &format,
            false,
        ),
        format,
    }
}

/// The plot area within `area` once the labels and titles of the axes
/// have their room, with the scales of `axes` fitted to its length and
/// the layout of the category labels `labels` (shown as `cat` says).
#[allow(clippy::too_many_arguments, reason = "the parts of one layout")]
fn lay_out(
    area: Area,
    horizontal: bool,
    axes: &mut [ValueAxis<'_>],
    labels: &[String],
    cat: &CategoryText<'_>,
    groups: &[PlotGroup],
    spans: &[Spans],
    on_axis: &[usize],
) -> (Area, LabelLayout) {
    let line = TEXT_SIZE_PT * LINE_RATIO;
    let title_room = |shown: bool| if shown { line } else { 0.0 };
    // Titles: the category title below (beside horizontal bars), the
    // value titles beside (below and above horizontal bars).
    let primary_title = title_room(axes[0].title().is_some());
    let secondary_title = title_room(axes.get(1).is_some_and(|axis| axis.title().is_some()));
    let cat_title_room = title_room(cat.title.is_some());
    let mut plot = area;
    let mut layout = LabelLayout {
        step: 1,
        turned: false,
        extent: 0.0,
        max_width: MAX_LABEL_PT,
    };
    for _ in 0..2 {
        let length = if horizontal { plot.width } else { plot.height };
        let spacing = if horizontal {
            WIDE_TICK_SPACING_PT
        } else {
            TICK_SPACING_PT
        };
        let target = (length / spacing) as usize;
        for (index, axis) in axes.iter_mut().enumerate() {
            let mine: Vec<usize> = (0..groups.len()).filter(|group| on_axis[*group] == index).collect();
            *axis = value_axis(axis.axis, &mine, groups, spans, target);
        }
        let tick_room = |axis: Option<&ValueAxis<'_>>| match axis.filter(|axis| axis.labelled()) {
            None => 0.0,
            Some(_) if horizontal => line,
            Some(axis) => {
                axis.scale
                    .ticks()
                    .iter()
                    .map(|value| text_width(&tick_label(*value, &axis.format, false), TEXT_SIZE_PT))
                    .fold(0.0, f32::max)
                    + LABEL_GAP_PT
            }
        };
        let primary = tick_room(axes.first());
        let secondary = tick_room(axes.get(1));
        let band = if horizontal { plot.height } else { plot.width } / labels.len().max(1) as f32;
        layout = label_layout(labels, band, horizontal, area, cat.skip);
        let cat_room = if cat.labelled { layout.extent } else { 0.0 };
        // The last value label is centred on the plot's right edge.
        let overhang = if horizontal { half_last_label(axes.first()) } else { 0.0 };
        plot = if horizontal {
            area.inset(
                cat_room + cat_title_room,
                secondary + secondary_title,
                overhang,
                primary + primary_title,
            )
        } else {
            area.inset(
                primary + primary_title,
                0.0,
                secondary + secondary_title,
                cat_room + cat_title_room,
            )
        };
    }
    (plot, layout)
}

/// Half the width of the label of the highest tick of `axis`, when its
/// labels are shown.
fn half_last_label(axis: Option<&ValueAxis<'_>>) -> f32 {
    axis.filter(|axis| axis.labelled())
        .and_then(|axis| {
            let last = *axis.scale.ticks().last()?;
            Some(text_width(&tick_label(last, &axis.format, false), TEXT_SIZE_PT) / 2.0)
        })
        .unwrap_or(0.0)
}

/// How the category `labels` are laid out over buckets `band` wide: every
/// `skip`-th at least, when the file says so.
fn label_layout(labels: &[String], band: f32, horizontal: bool, area: Area, skip: Option<usize>) -> LabelLayout {
    let skip = skip.unwrap_or(1).max(1);
    let line = TEXT_SIZE_PT * LINE_RATIO;
    let widest = labels
        .iter()
        .map(|label| text_width(label, TEXT_SIZE_PT))
        .fold(0.0, f32::max)
        .min(MAX_LABEL_PT);
    if horizontal {
        let max_width = widest.min(area.width * SIDE_LABEL_RATIO);
        return LabelLayout {
            step: ((line / band.max(0.01)).ceil().max(1.0) as usize).max(skip),
            turned: false,
            extent: max_width + LABEL_GAP_PT,
            max_width,
        };
    }
    let flat_step = (((widest + LABEL_GAP_PT) / band.max(0.01)).ceil().max(1.0) as usize).max(skip);
    let turned_step = ((line / band.max(0.01)).ceil().max(1.0) as usize).max(skip);
    if flat_step == 1 || flat_step <= turned_step {
        return LabelLayout {
            step: flat_step,
            turned: false,
            extent: line + LABEL_GAP_PT,
            max_width: widest,
        };
    }
    let max_width = widest.min(area.height * TURNED_LABEL_RATIO / std::f32::consts::FRAC_1_SQRT_2);
    LabelLayout {
        step: turned_step,
        turned: true,
        extent: max_width * std::f32::consts::FRAC_1_SQRT_2 + TEXT_SIZE_PT + LABEL_GAP_PT,
        max_width,
    }
}

/// The gridlines of the value axes that have them, and of the category
/// axis when it has them.
fn gridlines(out: &mut Svg, frame: &Frame, axes: &[ValueAxis<'_>], category_axis: Option<&Axis>) {
    let plot = frame.plot;
    for axis in axes
        .iter()
        .filter(|axis| axis.axis.is_none_or(|axis| axis.major_gridlines))
    {
        for tick in axis.scale.ticks() {
            let at = frame.value(&axis.scale, tick);
            let (from, to) = if frame.horizontal {
                ((at, plot.y), (at, plot.bottom()))
            } else {
                ((plot.x, at), (plot.right(), at))
            };
            out.line("grid", from, to, GRID, HAIRLINE_PT);
        }
        // The secondary axis draws none of its own over the primary's.
        if axes.len() > 1 {
            break;
        }
    }
    if category_axis.is_some_and(|axis| axis.major_gridlines) {
        let count = frame.buckets.count;
        let step = count.div_ceil(MAX_CATEGORY_GRIDLINES).max(1);
        for index in (0..=count).step_by(step) {
            let offset = index as f32 * frame.band();
            let (from, to) = if frame.horizontal {
                ((plot.x, plot.y + offset), (plot.right(), plot.y + offset))
            } else {
                ((plot.x + offset, plot.y), (plot.x + offset, plot.bottom()))
            };
            out.line("grid-category", from, to, GRID, HAIRLINE_PT);
        }
    }
}

/// Draws the bars of `group`, from its `spans`, on `scale`; their data
/// labels go to `labels`.
fn bars(
    out: &mut Svg,
    frame: &Frame,
    group: &PlotGroup,
    spans: &Spans,
    scale: &Scale,
    labels: &mut Labels,
    date1904: bool,
) {
    let ChartKind::Bar {
        grouping,
        gap_width_ratio,
        overlap_ratio,
        ..
    } = group.kind
    else {
        return;
    };
    let stacked = matches!(grouping, Grouping::Stacked | Grouping::PercentStacked);
    let slots = if stacked { 1.0 } else { group.series.len().max(1) as f32 };
    let overlap = if stacked { 1.0 } else { overlap_ratio };
    let bar = frame.band() / (slots - (slots - 1.0) * overlap + gap_width_ratio).max(0.1);
    let cluster = bar * (slots - (slots - 1.0) * overlap);
    let inset = if !stacked && overlap >= 0.0 && slots > 1.0 && bar > MIN_INSET_BAR_PT {
        0.5
    } else {
        0.0
    };
    for bucket in 0..frame.buckets.count {
        let center = frame.center(bucket);
        let (mut above, mut below) = (false, false);
        for (index, series) in group.series.iter().enumerate() {
            let Some((from, to)) = spans[index].get(bucket).copied().flatten() else {
                continue;
            };
            let slot = if stacked { 0.0 } else { index as f32 };
            let start = center - cluster / 2.0 + slot * bar * (1.0 - overlap) + inset;
            let width = bar - 2.0 * inset;
            let (mut base, end) = (frame.value(scale, from), frame.value(scale, to));
            // The surface gap between a segment and the one below it.
            let positive = to >= from;
            let follows = if positive {
                std::mem::replace(&mut above, true)
            } else {
                std::mem::replace(&mut below, true)
            };
            if stacked && follows && (end - base).abs() > SEGMENT_GAP_PT {
                base += SEGMENT_GAP_PT * (end - base).signum();
            }
            if (end - base).abs() < 0.01 {
                continue;
            }
            let (x0, y0) = frame.point(start, base);
            let (x1, y1) = frame.point(start + width, end);
            // The colour of the bucket's first point: its own, or the
            // series'.
            let first = frame.buckets.range(bucket).start;
            let color = series.point_colors.get(first).copied().unwrap_or(series.color);
            out.rect(
                "bar",
                x0.min(x1),
                y0.min(y1),
                (x1 - x0).abs(),
                (y1 - y0).abs(),
                &format!(r#"fill="{}""#, hex(color)),
            );
            if frame.buckets.size == 1
                && labels.room()
                && let Some(value) = series
                    .values
                    .as_ref()
                    .and_then(|values| values.points.get(first).copied().flatten())
                && let Some(text) = labels::text(series, first, value, None, date1904)
            {
                let bar = BarMark {
                    along: start + width / 2.0,
                    width,
                    base,
                    end,
                    color,
                };
                bar_label(frame, &bar, stacked, series.labels.position, text, labels);
            }
        }
    }
}

/// A bar drawn, along the category axis and across it.
struct BarMark {
    /// The middle of the bar along the category axis.
    along: f32,
    /// Its width along the category axis.
    width: f32,
    /// Where it starts across (its base).
    base: f32,
    /// Where it ends across.
    end: f32,
    /// Its fill.
    color: Rgb,
}

/// Adds the data label `text` of `bar` to `labels` at `position` (by
/// default past its end, or in its middle when the bars are `stacked`); a
/// label inside that does not fit the bar is left out, one past the end
/// that leaves the plot goes inside the end.
fn bar_label(
    frame: &Frame,
    bar: &BarMark,
    stacked: bool,
    position: Option<LabelPosition>,
    text: String,
    labels: &mut Labels,
) {
    let (width, height) = labels::size(&text);
    // The label's extent across the bar, and along it.
    let (across, along) = if frame.horizontal {
        (width, height)
    } else {
        (height, width)
    };
    let length = (bar.end - bar.base).abs();
    let outward = if bar.end >= bar.base { 1.0 } else { -1.0 };
    let half = across / 2.0 + LABEL_OFFSET_PT;
    let default = if stacked {
        LabelPosition::Center
    } else {
        LabelPosition::OutsideEnd
    };
    let position = position.unwrap_or(default);
    let (low, high) = if frame.horizontal {
        (frame.plot.x, frame.plot.right())
    } else {
        (frame.plot.y, frame.plot.bottom())
    };
    let outside = bar.end + outward * half;
    let (middle, inside) = match position {
        LabelPosition::OutsideEnd | LabelPosition::BestFit
            if outside - across / 2.0 >= low && outside + across / 2.0 <= high =>
        {
            (outside, false)
        }
        LabelPosition::InsideBase => (bar.base + outward * half, true),
        LabelPosition::Center => ((bar.base + bar.end) / 2.0, true),
        _ => (bar.end - outward * half, true),
    };
    // A label inside takes the bar's length and, give or take, its width.
    if inside && (across + 2.0 * LABEL_OFFSET_PT > length || along > bar.width.max(frame.band() * 0.9)) {
        return;
    }
    let ink = if inside { ink_on(bar.color) } else { INK_OUTSIDE };
    labels.add(frame.point(bar.along, middle), text, ink);
}

/// Adds the data labels of the lines of `group` (one point per bucket) to
/// `labels`, beside each point as the file says (right of it by default).
fn line_labels(
    frame: &Frame,
    group: &PlotGroup,
    spans: &Spans,
    grouping: Grouping,
    scale: &Scale,
    labels: &mut Labels,
    date1904: bool,
) {
    if frame.buckets.size != 1 {
        return;
    }
    for (index, series) in group.series.iter().enumerate() {
        if !series.labels.any() {
            continue;
        }
        let points = series
            .values
            .as_ref()
            .map(|values| values.points.as_slice())
            .unwrap_or_default();
        for (bucket, value) in points.iter().enumerate().take(frame.buckets.count) {
            let Some(value) = *value else { continue };
            let Some(text) = labels::text(series, bucket, value, None, date1904) else {
                continue;
            };
            let top = if grouping == Grouping::Standard {
                value
            } else {
                match spans[index].get(bucket).copied().flatten() {
                    Some((_, to)) => to,
                    None => continue,
                }
            };
            let (x, y) = frame.point(frame.center(bucket), frame.value(scale, top));
            let (width, height) = labels::size(&text);
            let gap = MARKER_RADIUS_PT + LABEL_OFFSET_PT;
            let center = match series.labels.position {
                Some(LabelPosition::Above) => (x, y - gap - height / 2.0),
                Some(LabelPosition::Below) => (x, y + gap + height / 2.0),
                Some(LabelPosition::Left) => (x - gap - width / 2.0, y),
                Some(LabelPosition::Center) => (x, y),
                _ => (x + gap + width / 2.0, y),
            };
            labels.add(center, text, INK_OUTSIDE);
        }
    }
}

/// Draws the areas of `group`, from its `spans`, on `scale`: a missing
/// point is zero, as in Excel.
fn areas(out: &mut Svg, frame: &Frame, group: &PlotGroup, spans: &Spans, scale: &Scale) {
    let count = frame.buckets.count;
    for (index, series) in group.series.iter().enumerate() {
        let span = |bucket: usize| spans[index][bucket].unwrap_or((0.0, 0.0));
        let mut path = PathData::new();
        for bucket in 0..count {
            path.point(frame.point(frame.center(bucket), frame.value(scale, span(bucket).1)));
        }
        for bucket in (0..count).rev() {
            path.point(frame.point(frame.center(bucket), frame.value(scale, span(bucket).0)));
        }
        path.close();
        out.path(
            "area",
            &path.into_string(),
            &format!(r#"fill="{}" fill-opacity="{}""#, hex(series.color), num(AREA_OPACITY)),
        );
    }
}

/// Draws the lines of `group` on `scale`: through the bucket's mean when
/// stacked, else through each point, or each bucket's lowest and highest
/// point; a missing point breaks the line. Markers, when the series has
/// them and the points are far enough apart, take from `budget`.
fn lines(
    out: &mut Svg,
    frame: &Frame,
    group: &PlotGroup,
    spans: &Spans,
    grouping: Grouping,
    scale: &Scale,
    budget: &mut usize,
) {
    let buckets = frame.buckets;
    let markers_fit = buckets.size == 1 && frame.band() >= MARKER_SPACING_PT;
    for (index, series) in group.series.iter().enumerate() {
        let points = series
            .values
            .as_ref()
            .map(|values| values.points.as_slice())
            .unwrap_or_default();
        let mut path = PathData::new();
        let mut dots = Vec::new();
        let mut runs: Vec<Vec<(f32, f32)>> = vec![Vec::new()];
        for bucket in 0..buckets.count {
            let along = frame.center(bucket);
            let values: Vec<f64> = if grouping == Grouping::Standard {
                extremes(points, buckets.range(bucket))
            } else {
                spans[index]
                    .get(bucket)
                    .copied()
                    .flatten()
                    .map(|(_, to)| to)
                    .into_iter()
                    .collect()
            };
            if values.is_empty() {
                path.gap();
                runs.push(Vec::new());
                continue;
            }
            for value in values {
                let at = frame.point(along, frame.value(scale, value));
                path.point(at);
                if let Some(run) = runs.last_mut() {
                    run.push(at);
                }
                dots.push(at);
            }
        }
        if series.line {
            let data = if series.smooth {
                smooth_path(&runs)
            } else {
                path.into_string()
            };
            out.path("line", &data, &line_attrs(series));
        }
        if series.markers && markers_fit && dots.len() <= *budget {
            *budget -= dots.len();
            for at in dots {
                out.marker(at, MARKER_RADIUS_PT, series.color);
            }
        }
    }
}

/// The values a line draws for the categories `range` of `points`: the
/// one value, or the lowest and highest in their order; none when all
/// are missing.
fn extremes(points: &[Option<f64>], range: Range<usize>) -> Vec<f64> {
    let present: Vec<(usize, f64)> = range
        .filter_map(|index| Some((index, (*points.get(index)?)?)))
        .collect();
    let Some(&first) = present.first() else {
        return Vec::new();
    };
    if present.len() == 1 {
        return vec![first.1];
    }
    let low = present
        .iter()
        .copied()
        .fold(first, |a, b| if b.1 < a.1 { b } else { a });
    let high = present
        .iter()
        .copied()
        .fold(first, |a, b| if b.1 > a.1 { b } else { a });
    if low.0 <= high.0 {
        vec![low.1, high.1]
    } else {
        vec![high.1, low.1]
    }
}

/// Draws the category labels below the plot (beside horizontal bars).
fn category_text(out: &mut Svg, frame: &Frame, labels: &[String], layout: &LabelLayout) {
    let plot = frame.plot;
    for (bucket, label) in labels.iter().enumerate().step_by(layout.step) {
        if label.is_empty() {
            continue;
        }
        let text = fit(label, TEXT_SIZE_PT, layout.max_width);
        let along = frame.center(bucket);
        if frame.horizontal {
            let style = TextStyle { anchor: "end", ..SMALL };
            out.text(
                "category",
                (plot.x - LABEL_GAP_PT, along + TEXT_SIZE_PT * BASELINE_RATIO),
                style,
                &text,
            );
        } else if layout.turned {
            let style = TextStyle {
                anchor: "end",
                rotate: TURN_DEG,
                ..SMALL
            };
            out.text(
                "category",
                (along, plot.bottom() + LABEL_GAP_PT + TEXT_SIZE_PT * 0.7),
                style,
                &text,
            );
        } else {
            out.text(
                "category",
                (along, plot.bottom() + LABEL_GAP_PT + TEXT_SIZE_PT),
                SMALL,
                &text,
            );
        }
    }
}

/// Draws the tick labels of the value axes that are shown.
fn value_text(out: &mut Svg, frame: &Frame, axes: &[ValueAxis<'_>], date1904: bool) {
    let plot = frame.plot;
    for (index, axis) in axes.iter().enumerate().filter(|(_, axis)| axis.labelled()) {
        let secondary = index == 1;
        let class = if secondary { "tick-secondary" } else { "tick" };
        for tick in axis.scale.ticks() {
            let text = tick_label(tick, &axis.format, date1904);
            let at = frame.value(&axis.scale, tick);
            let (point, anchor) = match (frame.horizontal, secondary) {
                (false, false) => ((plot.x - LABEL_GAP_PT, at + TEXT_SIZE_PT * BASELINE_RATIO), "end"),
                (false, true) => (
                    (plot.right() + LABEL_GAP_PT, at + TEXT_SIZE_PT * BASELINE_RATIO),
                    "start",
                ),
                (true, false) => ((at, plot.bottom() + LABEL_GAP_PT + TEXT_SIZE_PT), "middle"),
                (true, true) => ((at, plot.y - LABEL_GAP_PT), "middle"),
            };
            out.text(class, point, TextStyle { anchor, ..SMALL }, &text);
        }
    }
}

/// Draws the axis titles: the category title below the labels (left of
/// horizontal bars), value titles beside the plot (below and above
/// horizontal bars).
fn titles(out: &mut Svg, area: Area, frame: &Frame, axes: &[ValueAxis<'_>], cat_title: Option<&str>) {
    let plot = frame.plot;
    let line = TEXT_SIZE_PT * LINE_RATIO;
    let middle_x = plot.x + plot.width / 2.0;
    let middle_y = plot.y + plot.height / 2.0;
    let turned = |rotate: f32| TextStyle { rotate, ..SMALL };
    let baseline = TEXT_SIZE_PT * BASELINE_RATIO;
    let along_width = |text: &str| fit(text, TEXT_SIZE_PT, plot.width);
    let along_height = |text: &str| fit(text, TEXT_SIZE_PT, plot.height);
    let bottom = (middle_x, area.bottom() - line / 2.0 + baseline);
    let left = (area.x + line / 2.0 + baseline, middle_y);
    if let Some(title) = cat_title {
        if frame.horizontal {
            out.text("axis-title", left, turned(-90.0), &along_height(title));
        } else {
            out.text("axis-title", bottom, SMALL, &along_width(title));
        }
    }
    if let Some(title) = axes.first().and_then(ValueAxis::title) {
        if frame.horizontal {
            out.text("axis-title", bottom, SMALL, &along_width(title));
        } else {
            out.text("axis-title", left, turned(-90.0), &along_height(title));
        }
    }
    if let Some(title) = axes.get(1).and_then(ValueAxis::title) {
        if frame.horizontal {
            out.text(
                "axis-title",
                (middle_x, area.y + line / 2.0 + baseline),
                SMALL,
                &along_width(title),
            );
        } else {
            out.text(
                "axis-title",
                (area.right() - line / 2.0 + baseline, middle_y),
                turned(90.0),
                &along_height(title),
            );
        }
    }
}

/// The number of values of a series.
fn values_len(series: &Series) -> usize {
    series.values.as_ref().map_or(0, |values| values.points.len())
}

/// The number of categories of a series.
fn categories_len(categories: Option<&Categories>) -> usize {
    match categories {
        Some(Categories::Text(levels)) => levels.iter().map(Vec::len).max().unwrap_or(0),
        Some(Categories::Numbers(numbers)) => numbers.points.len(),
        None => 0,
    }
}

/// The x values of a scatter series: its numbers, else 1, 2, ... (text x
/// values are plotted in order, as Excel does).
fn x_values(series: &Series) -> Vec<Option<f64>> {
    match &series.categories {
        Some(Categories::Numbers(numbers)) => numbers.points.clone(),
        _ => (1..=values_len(series)).map(|index| Some(index as f64)).collect(),
    }
}

/// The x and y pairs of a scatter series, `None` where either is missing.
type Pairs = Vec<Option<(f64, f64)>>;

/// The axes of a scatter chart, with their scales and label formats.
struct ScatterAxes<'c> {
    /// The x axis of the file.
    x_axis: Option<&'c Axis>,
    /// The y axis of the file.
    y_axis: Option<&'c Axis>,
    /// The x scale.
    x_scale: Scale,
    /// The y scale.
    y_scale: Scale,
    /// The number format of the x labels.
    x_format: NumFmt,
    /// The number format of the y labels.
    y_format: NumFmt,
}

/// Whether the tick labels of `axis` are shown.
fn labelled(axis: Option<&Axis>) -> bool {
    axis.is_none_or(|axis| !axis.deleted && axis.tick_labels)
}

/// The title of `axis`, when it is shown and has one.
fn shown_title(axis: Option<&Axis>) -> Option<String> {
    axis.filter(|axis| !axis.deleted).and_then(|axis| axis.title.clone())
}

/// Draws a scatter chart: each series' points at their x and y values,
/// joined in order when it has a line, marked when it has markers.
fn scatter(out: &mut Svg, chart: &Chart, area: Area) {
    let (x_axis, y_axis) = scatter_axes(chart, &chart.groups[0]);
    let series: Vec<(&Series, Pairs)> = chart
        .series()
        .map(|series| {
            let ys = series
                .values
                .as_ref()
                .map(|values| values.points.as_slice())
                .unwrap_or_default();
            let pairs = x_values(series)
                .into_iter()
                .zip(ys)
                .map(|(x, y)| Some((x?, (*y)?)))
                .collect();
            (series, pairs)
        })
        .collect();
    let x_format = chart
        .series()
        .find_map(|series| match &series.categories {
            Some(Categories::Numbers(numbers)) => Some(numbers.format.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let y_format = chart
        .series()
        .find_map(|series| series.values.as_ref().map(|values| values.format.clone()))
        .unwrap_or_default();
    let mut axes = ScatterAxes {
        x_axis,
        y_axis,
        x_scale: Scale::nice(None, Fixed::default(), Zero::Auto, 1, false),
        y_scale: Scale::nice(None, Fixed::default(), Zero::Auto, 1, false),
        x_format: x_axis.and_then(|axis| axis.format.clone()).unwrap_or(x_format),
        y_format: y_axis.and_then(|axis| axis.format.clone()).unwrap_or(y_format),
    };
    let plot = scatter_layout(&mut axes, &series, area, chart.date1904);
    out.rect("plot", plot.x, plot.y, plot.width, plot.height, r#"fill="none""#);
    let (x_scale, y_scale) = (axes.x_scale, axes.y_scale);
    if y_axis.is_none_or(|axis| axis.major_gridlines) {
        for tick in y_scale.ticks() {
            let at = y_scale.at(tick, plot.bottom(), plot.y);
            out.line("grid", (plot.x, at), (plot.right(), at), GRID, HAIRLINE_PT);
        }
    }
    if x_axis.is_some_and(|axis| axis.major_gridlines) {
        for tick in x_scale.ticks() {
            let at = x_scale.at(tick, plot.x, plot.right());
            out.line("grid-x", (at, plot.y), (at, plot.bottom()), GRID, HAIRLINE_PT);
        }
    }
    if !x_axis.is_some_and(|axis| axis.deleted) {
        let (left, right) = ((plot.x, plot.bottom()), (plot.right(), plot.bottom()));
        out.line("axis", left, right, AXIS_LINE, HAIRLINE_PT);
    }
    let to_point = |(x, y): (f64, f64)| {
        (
            x_scale.at(x, plot.x, plot.right()),
            y_scale.at(y, plot.bottom(), plot.y),
        )
    };
    scatter_marks(out, &series, to_point);
    scatter_text(out, &axes, plot, area, chart.date1904);
}

/// The plot area of a scatter chart within `area`, once its labels and
/// titles have their room, with the scales of `axes` fitted to it.
fn scatter_layout(axes: &mut ScatterAxes<'_>, series: &[(&Series, Pairs)], area: Area, date1904: bool) -> Area {
    let xs = range(
        series
            .iter()
            .flat_map(|(_, pairs)| pairs.iter().flatten().map(|pair| pair.0)),
    );
    let ys = range(
        series
            .iter()
            .flat_map(|(_, pairs)| pairs.iter().flatten().map(|pair| pair.1)),
    );
    let line = TEXT_SIZE_PT * LINE_RATIO;
    let scale_of = |axis: Option<&Axis>, data: Option<(f64, f64)>, length: f32, spacing: f32, format: &NumFmt| {
        let build = |target| {
            Scale::nice(
                data,
                Fixed::of(axis),
                Zero::Auto,
                target,
                axis.is_some_and(|axis| axis.reversed),
            )
        };
        readable(build, (length / spacing) as usize, format, date1904)
    };
    let mut plot = area;
    for _ in 0..2 {
        axes.x_scale = scale_of(axes.x_axis, xs, plot.width, WIDE_TICK_SPACING_PT, &axes.x_format);
        axes.y_scale = scale_of(axes.y_axis, ys, plot.height, TICK_SPACING_PT, &axes.y_format);
        let left = if labelled(axes.y_axis) {
            axes.y_scale
                .ticks()
                .iter()
                .map(|tick| text_width(&tick_label(*tick, &axes.y_format, date1904), TEXT_SIZE_PT))
                .fold(0.0, f32::max)
                + LABEL_GAP_PT
        } else {
            0.0
        };
        let bottom = if labelled(axes.x_axis) {
            line + LABEL_GAP_PT
        } else {
            0.0
        };
        let left_title = if shown_title(axes.y_axis).is_some() { line } else { 0.0 };
        let bottom_title = if shown_title(axes.x_axis).is_some() { line } else { 0.0 };
        let overhang = if labelled(axes.x_axis) {
            axes.x_scale.ticks().last().map_or(0.0, |last| {
                text_width(&tick_label(*last, &axes.x_format, date1904), TEXT_SIZE_PT) / 2.0
            })
        } else {
            0.0
        };
        plot = area.inset(left + left_title, 0.0, overhang, bottom + bottom_title);
    }
    plot
}

/// Draws the lines and markers of scatter `series`, each point placed by
/// `to_point`; a series with more points than its share of the most drawn
/// is sampled.
fn scatter_marks(out: &mut Svg, series: &[(&Series, Pairs)], to_point: impl Fn((f64, f64)) -> (f32, f32)) {
    let per_series = (MAX_VERTICES / series.len().max(1)).max(2);
    let mut budget = MAX_MARKERS;
    for (series, pairs) in series {
        let stride = pairs.len().div_ceil(per_series).max(1);
        let kept: Vec<Option<(f32, f32)>> = pairs.iter().step_by(stride).map(|pair| pair.map(&to_point)).collect();
        if series.line {
            let mut path = PathData::new();
            for point in &kept {
                match point {
                    Some(at) => path.point(*at),
                    None => path.gap(),
                }
            }
            out.path("line", &path.into_string(), &line_attrs(series));
        }
        let dots: Vec<(f32, f32)> = kept.iter().flatten().copied().collect();
        if series.markers && dots.len() <= budget {
            budget -= dots.len();
            for at in dots {
                out.marker(at, MARKER_RADIUS_PT, series.color);
            }
        }
    }
}

/// The attributes of the line of `series`.
fn line_attrs(series: &Series) -> String {
    let dashes = if series.dashed {
        format!(
            r#" stroke-dasharray="{} {}""#,
            num(LINE_WIDTH_PT * DASH_RATIO),
            num(LINE_WIDTH_PT * DASH_GAP_RATIO)
        )
    } else {
        String::new()
    };
    format!(
        r#"fill="none" stroke="{}" stroke-width="{}" stroke-linejoin="round" stroke-linecap="round"{dashes}"#,
        hex(series.color),
        num(LINE_WIDTH_PT)
    )
}

/// The path data of lines through each of `runs` (the points between two
/// gaps), smoothed: a cubic curve between two points, its tangents those
/// of a Catmull-Rom spline.
fn smooth_path(runs: &[Vec<(f32, f32)>]) -> String {
    let mut data = String::new();
    for run in runs.iter().filter(|run| !run.is_empty()) {
        if !data.is_empty() {
            data.push(' ');
        }
        let _ = write!(data, "M {} {}", num(run[0].0), num(run[0].1));
        for index in 1..run.len() {
            let before = run[index.saturating_sub(2)];
            let (start, end) = (run[index - 1], run[index]);
            let after = run[(index + 1).min(run.len() - 1)];
            let first = (
                start.0 + (end.0 - before.0) / SPLINE_DIVISOR,
                start.1 + (end.1 - before.1) / SPLINE_DIVISOR,
            );
            let second = (
                end.0 - (after.0 - start.0) / SPLINE_DIVISOR,
                end.1 - (after.1 - start.1) / SPLINE_DIVISOR,
            );
            let _ = write!(
                data,
                " C {} {} {} {} {} {}",
                num(first.0),
                num(first.1),
                num(second.0),
                num(second.1),
                num(end.0),
                num(end.1)
            );
        }
    }
    data
}

/// Draws the tick labels and titles of the axes of a scatter chart.
fn scatter_text(out: &mut Svg, axes: &ScatterAxes<'_>, plot: Area, area: Area, date1904: bool) {
    let line = TEXT_SIZE_PT * LINE_RATIO;
    if labelled(axes.x_axis) {
        for tick in axes.x_scale.ticks() {
            let at = (
                axes.x_scale.at(tick, plot.x, plot.right()),
                plot.bottom() + LABEL_GAP_PT + TEXT_SIZE_PT,
            );
            out.text("tick-x", at, SMALL, &tick_label(tick, &axes.x_format, date1904));
        }
    }
    if labelled(axes.y_axis) {
        for tick in axes.y_scale.ticks() {
            let at = (
                plot.x - LABEL_GAP_PT,
                axes.y_scale.at(tick, plot.bottom(), plot.y) + TEXT_SIZE_PT * BASELINE_RATIO,
            );
            let style = TextStyle { anchor: "end", ..SMALL };
            out.text("tick", at, style, &tick_label(tick, &axes.y_format, date1904));
        }
    }
    let baseline = TEXT_SIZE_PT * BASELINE_RATIO;
    if let Some(text) = shown_title(axes.x_axis) {
        let at = (plot.x + plot.width / 2.0, area.bottom() - line / 2.0 + baseline);
        out.text("axis-title", at, SMALL, &fit(&text, TEXT_SIZE_PT, plot.width));
    }
    if let Some(text) = shown_title(axes.y_axis) {
        let at = (area.x + line / 2.0 + baseline, plot.y + plot.height / 2.0);
        let style = TextStyle { rotate: -90.0, ..SMALL };
        out.text("axis-title", at, style, &fit(&text, TEXT_SIZE_PT, plot.height));
    }
}

/// The x and y axes of a scatter group: the one along the bottom or top
/// is x, the other y; else in the order of the group's ids.
fn scatter_axes<'c>(chart: &'c Chart, group: &PlotGroup) -> (Option<&'c Axis>, Option<&'c Axis>) {
    let mine: Vec<&Axis> = group
        .axis_ids
        .iter()
        .filter_map(|id| chart.axes.iter().find(|axis| axis.id == *id))
        .collect();
    let x = mine
        .iter()
        .find(|axis| matches!(axis.position, AxisPosition::Bottom | AxisPosition::Top))
        .or(mine.first())
        .copied();
    let y = mine
        .iter()
        .find(|axis| Some(axis.id) != x.map(|found| found.id))
        .copied();
    (x, y)
}
