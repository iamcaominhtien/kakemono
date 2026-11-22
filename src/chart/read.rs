//! The DrawingML chart part (`c:chartSpace`) read into a [`Chart`]: its
//! title, plot groups with their series and cached data, axes and legend.
//!
//! The elements of an `mc:AlternateContent` are read from its fallback
//! (`c:style` of Office 2007 behind the `c14:style` of Office 2010).

use super::theme::{Theme, color};
use super::{
    Axis, AxisKind, AxisPosition, Categories, Chart, ChartConfig, ChartKind, Grouping, LegendPosition, Numbers,
    PlotGroup, Rgb, Series, chartex,
};
use crate::excel::numfmt::NumFmt;
use crate::word::docx::parse;
use crate::word::xml::{DRAWING_NS, MC_NS, child, is};
use anyhow::{Context, Result, bail};
use roxmltree::Node;

/// Namespace of DrawingML charts (`c:`).
pub(super) const CHART_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
/// Namespace of the extended charts of Office 2016 (`cx:`).
pub(super) const CHARTEX_NS: &str = "http://schemas.microsoft.com/office/drawing/2014/chartex";
/// The gap between two categories of a bar chart when the part gives none
/// (`c:gapWidth`), in percent of a bar's width.
const DEFAULT_GAP_WIDTH_PERCENT: f32 = 150.0;
/// The widest gap the schema allows, in percent of a bar's width.
const MAX_GAP_WIDTH_PERCENT: f32 = 500.0;
/// A doughnut's hole when the part gives none (`c:holeSize`), in percent.
const DEFAULT_HOLE_PERCENT: f32 = 10.0;
/// The smallest and largest hole the schema allows, in percent.
const HOLE_PERCENT_RANGE: (f32, f32) = (10.0, 90.0);
/// Degrees in a turn, the range of `c:firstSliceAng`.
const FULL_TURN_DEG: f32 = 360.0;

/// What the limits of [`ChartConfig`] leave while a part is read, and
/// whether they cut anything.
pub(super) struct Limits<'c> {
    /// The limits.
    config: &'c ChartConfig,
    /// Series that may still be kept.
    series_left: usize,
    /// Points that may still be kept, over the whole chart.
    points_left: usize,
    /// A limit cut something.
    pub(super) truncated: bool,
}

impl<'c> Limits<'c> {
    /// The full limits of `config`.
    pub(super) fn new(config: &'c ChartConfig) -> Self {
        Self {
            config,
            series_left: config.max_series_count,
            points_left: config.max_points_per_chart,
            truncated: false,
        }
    }

    /// Whether one more series may be kept; counts it when it may.
    pub(super) fn take_series(&mut self) -> bool {
        if self.series_left == 0 {
            self.truncated = true;
            return false;
        }
        self.series_left -= 1;
        true
    }

    /// `text` cut to the most characters of a label.
    pub(super) fn clip(&mut self, text: &str) -> String {
        let max = self.config.max_label_chars;
        match text.char_indices().nth(max) {
            Some((end, _)) => {
                self.truncated = true;
                text[..end].to_string()
            }
            None => text.to_string(),
        }
    }
}

/// The chart of the chart part `xml` (a `c:chartSpace`, or an extended
/// chart's `cx:chartSpace`), its series without a colour of their own given
/// the automatic colours of `theme`, within the limits of `config`.
///
/// # Errors
/// When the part is larger than [`ChartConfig::max_part_bytes`], is not
/// UTF-8 XML, or is not a chart part. A damaged extended chart is not an
/// error: it gives what could be read of it.
pub(crate) fn read(xml: &[u8], theme: &Theme, config: &ChartConfig) -> Result<Chart> {
    if xml.len() > config.max_part_bytes {
        bail!(
            "chart part of {} bytes, more than the {} read",
            xml.len(),
            config.max_part_bytes
        );
    }
    let text = std::str::from_utf8(xml).context("chart part is not UTF-8")?;
    let doc = parse(text.trim_start_matches('\u{feff}'))?;
    let root = doc.root_element();
    if root.tag_name().name() != "chartSpace" {
        bail!("not a chart part");
    }
    match root.tag_name().namespace() {
        Some(CHART_NS) => Ok(chart_space(root, theme, config)),
        Some(CHARTEX_NS) => Ok(chartex::read(root, theme, config)),
        _ => bail!("not a chart part"),
    }
}

/// The chart of the `c:chartSpace` element `root`.
fn chart_space(root: Node<'_, '_>, theme: &Theme, config: &ChartConfig) -> Chart {
    let mut limits = Limits::new(config);
    let mut chart = Chart {
        date1904: flag(root, "date1904").unwrap_or(false),
        style: val(root, "style").and_then(|text| text.parse().ok()),
        ..Chart::default()
    };
    let Some(chart_node) = chart_el(root, "chart") else {
        return chart;
    };
    if let Some(plot) = chart_el(chart_node, "plotArea") {
        chart.groups = kids(plot)
            .filter_map(|node| plot_group(node, theme, &mut limits))
            .collect();
        chart.axes = kids(plot).filter_map(|node| axis(node, &mut limits)).collect();
    }
    chart.legend = chart_el(chart_node, "legend").map(|legend| legend_position(val(legend, "legendPos")));
    chart.plot_visible_only = flag(chart_node, "plotVisOnly").unwrap_or(false);
    let title = chart_el(chart_node, "title")
        .and_then(|title| title_text(title, &mut limits).or_else(|| automatic_title(chart_node, &chart)));
    let has_data = chart
        .series()
        .any(|series| series.values.as_ref().is_some_and(|values| !values.points.is_empty()));
    chart.title = title;
    chart.has_data = has_data;
    chart.truncated = limits.truncated;
    chart
}

/// The automatic title of a chart whose `c:title` has no text: the name of
/// its only series, unless the automatic title was deleted.
fn automatic_title(chart_node: Node<'_, '_>, chart: &Chart) -> Option<String> {
    if flag(chart_node, "autoTitleDeleted") == Some(true) {
        return None;
    }
    let mut series = chart.series();
    match (series.next(), series.next()) {
        (Some(only), None) => only.name.clone(),
        _ => None,
    }
}

/// The text of a title (`c:title`, or the `c:tx` of a series): its rich
/// text, or the cached text of its reference.
fn title_text(title: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<String> {
    let tx = chart_el(title, "tx")?;
    if let Some(rich) = chart_el(tx, "rich") {
        return non_empty(limits.clip(&rich_text(rich)));
    }
    text_source(tx, limits)
}

/// The text of a `c:tx` that holds a string reference or a value: the
/// cached cells joined by spaces.
fn text_source(tx: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<String> {
    if let Some(value) = chart_el(tx, "v") {
        return non_empty(limits.clip(value.text().unwrap_or_default().trim()));
    }
    let cache = chart_el(chart_el(tx, "strRef")?, "strCache")?;
    let texts: Vec<&str> = c_children(cache, "pt")
        .filter_map(|pt| chart_el(pt, "v").and_then(|value| value.text()))
        .collect();
    non_empty(limits.clip(texts.join(" ").trim()))
}

/// The text of the paragraphs of DrawingML rich text (`c:rich`,
/// `cx:rich`), joined by spaces, its runs of white space as one.
pub(super) fn rich_text(rich: Node<'_, '_>) -> String {
    let paragraphs: Vec<String> = rich
        .descendants()
        .filter(|node| is(*node, DRAWING_NS, "p"))
        .map(|paragraph| {
            paragraph
                .descendants()
                .filter(|node| is(*node, DRAWING_NS, "t"))
                .filter_map(|node| node.text())
                .collect()
        })
        .collect();
    paragraphs.join(" ").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text`, unless it is empty.
pub(super) fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

/// The plot group `node` of a plot area (`c:barChart`, `c:pieChart`, ...);
/// `None` for an element that is not one.
fn plot_group(node: Node<'_, '_>, theme: &Theme, limits: &mut Limits<'_>) -> Option<PlotGroup> {
    if node.tag_name().namespace() != Some(CHART_NS) {
        return None;
    }
    let name = node.tag_name().name();
    let kind = match name {
        "barChart" | "bar3DChart" => bar_kind(node),
        "lineChart" | "line3DChart" => ChartKind::Line {
            grouping: grouping(node, Grouping::Standard),
        },
        "areaChart" | "area3DChart" => ChartKind::Area {
            grouping: grouping(node, Grouping::Standard),
        },
        "pieChart" | "pie3DChart" => ChartKind::Pie {
            first_slice_deg: first_slice(node),
        },
        "doughnutChart" => ChartKind::Doughnut {
            first_slice_deg: first_slice(node),
            hole_ratio: percent_val(node, "holeSize", DEFAULT_HOLE_PERCENT, HOLE_PERCENT_RANGE) / 100.0,
        },
        "scatterChart" => ChartKind::Scatter {
            smooth: matches!(val(node, "scatterStyle"), Some("smooth" | "smoothMarker")),
        },
        "radarChart" => ChartKind::Radar {
            filled: val(node, "radarStyle") == Some("filled"),
        },
        "ofPieChart" | "bubbleChart" | "stockChart" | "surfaceChart" | "surface3DChart" => {
            ChartKind::NotDrawn { name: name.to_string() }
        }
        _ => return None,
    };
    let vary_colors = flag(node, "varyColors").unwrap_or(false);
    let mut series: Vec<(u32, Series)> = Vec::new();
    for (position, ser) in c_children(node, "ser").enumerate() {
        if !limits.take_series() {
            break;
        }
        series.push(read_series(ser, position, node, &kind, vary_colors, theme, limits));
    }
    series.sort_by_key(|(order, _)| *order);
    Some(PlotGroup {
        kind,
        three_d: name.contains("3D"),
        vary_colors,
        axis_ids: c_children(node, "axId")
            .filter_map(|id| id.attribute("val")?.parse().ok())
            .collect(),
        series: series.into_iter().map(|(_, series)| series).collect(),
    })
}

/// The kind of the bar group `node`: its direction, grouping, gap and
/// overlap.
fn bar_kind(node: Node<'_, '_>) -> ChartKind {
    ChartKind::Bar {
        horizontal: val(node, "barDir") == Some("bar"),
        grouping: grouping(node, Grouping::Clustered),
        gap_width_ratio: percent_val(
            node,
            "gapWidth",
            DEFAULT_GAP_WIDTH_PERCENT,
            (0.0, MAX_GAP_WIDTH_PERCENT),
        ) / 100.0,
        overlap_ratio: percent_val(node, "overlap", 0.0, (-100.0, 100.0)) / 100.0,
    }
}

/// The `c:grouping` of a group, `default` when it has none.
fn grouping(node: Node<'_, '_>, default: Grouping) -> Grouping {
    match val(node, "grouping") {
        Some("standard") => Grouping::Standard,
        Some("clustered") => Grouping::Clustered,
        Some("stacked") => Grouping::Stacked,
        Some("percentStacked") => Grouping::PercentStacked,
        _ => default,
    }
}

/// The angle of the first slice of a pie or doughnut, in degrees.
fn first_slice(node: Node<'_, '_>) -> f32 {
    percent_val(node, "firstSliceAng", 0.0, (0.0, FULL_TURN_DEG))
}

/// The number `val` of the child `name` of `node`, within `range`;
/// `default` when it is missing or not a number.
fn percent_val(node: Node<'_, '_>, name: &str, default: f32, range: (f32, f32)) -> f32 {
    val(node, name)
        .and_then(|text| text.parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map_or(default, |value| value.clamp(range.0, range.1))
}

/// The series `ser`, the `position`-th of the group `group` of kind
/// `kind`, with its plot order (`c:order`).
fn read_series(
    ser: Node<'_, '_>,
    position: usize,
    group: Node<'_, '_>,
    kind: &ChartKind,
    vary_colors: bool,
    theme: &Theme,
    limits: &mut Limits<'_>,
) -> (u32, Series) {
    let fallback_index = u32::try_from(position).unwrap_or(u32::MAX);
    let index = val(ser, "idx")
        .and_then(|text| text.parse().ok())
        .unwrap_or(fallback_index);
    let order = val(ser, "order").and_then(|text| text.parse().ok()).unwrap_or(index);
    let name = chart_el(ser, "tx").and_then(|tx| text_source(tx, limits));
    let categories = chart_el(ser, "cat")
        .or_else(|| chart_el(ser, "xVal"))
        .and_then(|source| categories(source, limits));
    let values = chart_el(ser, "val")
        .or_else(|| chart_el(ser, "yVal"))
        .and_then(|source| numbers(source, limits));
    let line_like = matches!(
        kind,
        ChartKind::Line { .. } | ChartKind::Scatter { .. } | ChartKind::Radar { filled: false }
    );
    let automatic = theme.automatic(index as usize);
    let props = chart_el(ser, "spPr");
    let color = series_color(ser, props, line_like, theme, automatic);
    let point_colors = if vary_colors {
        let count = values.as_ref().map_or(0, |values| values.points.len());
        point_colors(ser, count, theme)
    } else {
        Vec::new()
    };
    let no_line = props
        .and_then(|props| child(props, DRAWING_NS, "ln"))
        .is_some_and(|ln| child(ln, DRAWING_NS, "noFill").is_some());
    let scatter_style = val(group, "scatterStyle").unwrap_or("lineMarker");
    let symbol = chart_el(ser, "marker").and_then(|marker| val(marker, "symbol"));
    let group_markers = match kind {
        ChartKind::Line { .. } => flag(group, "marker").unwrap_or(true),
        ChartKind::Scatter { .. } => matches!(scatter_style, "lineMarker" | "marker" | "smoothMarker"),
        ChartKind::Radar { filled } => !filled && val(group, "radarStyle") == Some("marker"),
        _ => false,
    };
    let series = Series {
        index,
        name,
        categories,
        values,
        color,
        point_colors,
        markers: symbol.map_or(group_markers, |symbol| symbol != "none"),
        line: line_like && !no_line && !matches!(scatter_style, "marker" | "none"),
    };
    (order, series)
}

/// The colour of a series: for a line, its line, else its markers, else
/// its fill; for others, its fill, else its outline; else `automatic`.
fn series_color(ser: Node<'_, '_>, props: Option<Node<'_, '_>>, line_like: bool, theme: &Theme, automatic: Rgb) -> Rgb {
    let fill = props.and_then(|props| fill_color(props, theme, automatic));
    let outline = props
        .and_then(|props| child(props, DRAWING_NS, "ln"))
        .and_then(|ln| fill_color(ln, theme, automatic));
    let marker = chart_el(ser, "marker")
        .and_then(|marker| chart_el(marker, "spPr"))
        .and_then(|props| fill_color(props, theme, automatic));
    let chosen = if line_like {
        outline.or(marker).or(fill)
    } else {
        fill.or(outline)
    };
    chosen.unwrap_or(automatic)
}

/// The colour of each of the `count` points of a series whose group
/// varies its colours: a point's own fill (`c:dPt`), else the automatic
/// colour of its index.
fn point_colors(ser: Node<'_, '_>, count: usize, theme: &Theme) -> Vec<Rgb> {
    let mut colors: Vec<Rgb> = (0..count).map(|index| theme.automatic(index)).collect();
    for point in c_children(ser, "dPt") {
        let Some(index) = val(point, "idx").and_then(|text| text.parse::<usize>().ok()) else {
            continue;
        };
        let Some(slot) = colors.get_mut(index) else {
            continue;
        };
        if let Some(rgb) = chart_el(point, "spPr").and_then(|props| fill_color(props, theme, *slot)) {
            *slot = rgb;
        }
    }
    colors
}

/// The colour of a shape's properties or line (`c:spPr`, `a:ln`): its
/// solid fill, the first stop of its gradient, or the foreground of its
/// pattern; `None` when it has no fill or none of these.
fn fill_color(props: Node<'_, '_>, theme: &Theme, placeholder: Rgb) -> Option<Rgb> {
    for fill in props
        .children()
        .filter(|node| node.tag_name().namespace() == Some(DRAWING_NS))
    {
        match fill.tag_name().name() {
            "solidFill" => return color(fill, theme, placeholder),
            "gradFill" => {
                let stop = child(fill, DRAWING_NS, "gsLst").and_then(|list| child(list, DRAWING_NS, "gs"))?;
                return color(stop, theme, placeholder);
            }
            "pattFill" => return color(child(fill, DRAWING_NS, "fgClr")?, theme, placeholder),
            "noFill" => return None,
            _ => {}
        }
    }
    None
}

/// The categories (or x values) of the data source `source` (`c:cat`,
/// `c:xVal`): its cached strings, numbers or levels of strings, or its
/// literals; `None` when it caches none.
fn categories(source: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<Categories> {
    kids(source).find_map(|node| match node.tag_name().name() {
        "strRef" => chart_el(node, "strCache").map(|cache| Categories::Text(vec![labels(cache, None, limits)])),
        "strLit" => Some(Categories::Text(vec![labels(node, None, limits)])),
        "numRef" => chart_el(node, "numCache").map(|cache| Categories::Numbers(cached_numbers(cache, limits))),
        "numLit" => Some(Categories::Numbers(cached_numbers(node, limits))),
        "multiLvlStrRef" => chart_el(node, "multiLvlStrCache").map(|cache| {
            let declared = count(cache);
            let mut levels: Vec<_> = c_children(cache, "lvl")
                .map(|level| labels(level, declared, limits))
                .collect();
            levels.reverse();
            Categories::Text(levels)
        }),
        _ => None,
    })
}

/// The values of the data source `source` (`c:val`, `c:yVal`): its cached
/// numbers or its literals; `None` when it caches none.
fn numbers(source: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<Numbers> {
    kids(source).find_map(|node| match node.tag_name().name() {
        "numRef" => chart_el(node, "numCache").map(|cache| cached_numbers(cache, limits)),
        "numLit" => Some(cached_numbers(node, limits)),
        _ => None,
    })
}

/// The numbers of a cache (`c:numCache`, `c:numLit`) with its format.
fn cached_numbers(cache: Node<'_, '_>, limits: &mut Limits<'_>) -> Numbers {
    let format = num_fmt(
        chart_el(cache, "formatCode")
            .and_then(|code| code.text())
            .unwrap_or_default(),
    );
    let points = cached(c_children(cache, "pt"), count(cache), limits, |pt, _| {
        number(chart_el(pt, "v")?.text()?)
    });
    Numbers { points, format }
}

/// The strings of a cache (`c:strCache`, `c:strLit`, a `c:lvl`), counted
/// by its own `c:ptCount` or else by `declared`.
fn labels(cache: Node<'_, '_>, declared: Option<u64>, limits: &mut Limits<'_>) -> Vec<Option<String>> {
    let declared = count(cache).or(declared);
    cached(c_children(cache, "pt"), declared, limits, |pt, limits| {
        Some(limits.clip(chart_el(pt, "v")?.text().unwrap_or_default()))
    })
}

/// The `c:ptCount` of a cache.
fn count(cache: Node<'_, '_>) -> Option<u64> {
    val(cache, "ptCount")?.parse().ok()
}

/// The points `pts` of a cache, in a list of `declared` places (else as
/// many as the highest index needs), each read by `read`: within the
/// limits, a missing or unreadable point `None`.
pub(super) fn cached<'a, 'i: 'a, T>(
    pts: impl Iterator<Item = Node<'a, 'i>>,
    declared: Option<u64>,
    limits: &mut Limits<'_>,
    mut read: impl FnMut(Node<'a, 'i>, &mut Limits<'_>) -> Option<T>,
) -> Vec<Option<T>> {
    let cap = limits.config.max_points_per_series.min(limits.points_left);
    let declared = declared.map(|count| usize::try_from(count).unwrap_or(usize::MAX));
    if declared.is_some_and(|count| count > cap) {
        limits.truncated = true;
    }
    let bound = declared.map_or(cap, |count| count.min(cap));
    let mut kept = Vec::new();
    let mut highest = 0;
    for pt in pts {
        let Some(idx) = pt.attribute("idx").and_then(|text| text.parse::<usize>().ok()) else {
            continue;
        };
        if idx >= bound {
            // A point past the declared count is not part of the cache.
            limits.truncated |= declared.is_none_or(|count| idx < count);
            continue;
        }
        highest = highest.max(idx + 1);
        if let Some(value) = read(pt, limits) {
            kept.push((idx, value));
        }
    }
    let len = declared.map_or(highest, |count| count.min(cap));
    limits.points_left -= len;
    let mut points: Vec<Option<T>> = std::iter::repeat_with(|| None).take(len).collect();
    for (idx, value) in kept {
        points[idx] = Some(value);
    }
    points
}

/// A cached number; `None` when it is not a finite number.
pub(super) fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|value| value.is_finite())
}

/// The number format of the format code `code`.
pub(super) fn num_fmt(code: &str) -> NumFmt {
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        NumFmt::General
    } else {
        NumFmt::Code(code.to_string())
    }
}

/// The axis `node` of a plot area (`c:catAx`, `c:valAx`, `c:dateAx`,
/// `c:serAx`); `None` for an element that is not one.
fn axis(node: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<Axis> {
    let kind = match node.tag_name().name() {
        "catAx" => AxisKind::Category,
        "valAx" => AxisKind::Value,
        "dateAx" => AxisKind::Date,
        "serAx" => AxisKind::Series,
        _ => return None,
    };
    let scaling = chart_el(node, "scaling");
    let bound = |name| scaling.and_then(|scaling| val(scaling, name)).and_then(number);
    let format = chart_el(node, "numFmt")
        .filter(|fmt| !matches!(fmt.attribute("sourceLinked"), Some("1" | "true")))
        .and_then(|fmt| fmt.attribute("formatCode"))
        .map(num_fmt);
    Some(Axis {
        id: val(node, "axId").and_then(|text| text.parse().ok()).unwrap_or(0),
        kind,
        position: match val(node, "axPos") {
            Some("l") => AxisPosition::Left,
            Some("r") => AxisPosition::Right,
            Some("t") => AxisPosition::Top,
            _ => AxisPosition::Bottom,
        },
        deleted: flag(node, "delete").unwrap_or(false),
        reversed: scaling.and_then(|scaling| val(scaling, "orientation")) == Some("maxMin"),
        min: bound("min"),
        max: bound("max"),
        format,
        major_gridlines: chart_el(node, "majorGridlines").is_some(),
        title: chart_el(node, "title").and_then(|title| title_text(title, limits)),
        cross_axis: val(node, "crossAx").and_then(|text| text.parse().ok()),
    })
}

/// The legend position `pos` (`c:legendPos`); Excel's default, the right,
/// when it is missing.
pub(super) fn legend_position(pos: Option<&str>) -> LegendPosition {
    match pos {
        Some("b") => LegendPosition::Bottom,
        Some("t") => LegendPosition::Top,
        Some("l") => LegendPosition::Left,
        Some("tr") => LegendPosition::TopRight,
        _ => LegendPosition::Right,
    }
}

/// The child elements of `node`, those of an `mc:AlternateContent` taken
/// from its fallback (else its first choice) in its place.
fn kids<'a, 'i: 'a>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(Node::is_element).flat_map(|item| {
        let branch = is(item, MC_NS, "AlternateContent")
            .then(|| child(item, MC_NS, "Fallback").or_else(|| child(item, MC_NS, "Choice")))
            .flatten();
        let own = (!is(item, MC_NS, "AlternateContent")).then_some(item);
        own.into_iter().chain(
            branch
                .into_iter()
                .flat_map(|branch| branch.children().filter(Node::is_element)),
        )
    })
}

/// The first chart element `name` among the [`kids`] of `node`.
fn chart_el<'a, 'i: 'a>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    kids(node).find(|item| is(*item, CHART_NS, name))
}

/// The chart elements `name` that are direct children of `node` (the
/// points of a cache, the series of a group).
fn c_children<'a, 'i: 'a>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |item| is(*item, CHART_NS, name))
}

/// The `val` of the chart element `name` of `node`.
fn val<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    chart_el(node, name)?.attribute("val")
}

/// The boolean chart element `name` of `node`: `None` when it is missing;
/// true when its `val` is, or when it has none (the schema's default).
fn flag(node: Node<'_, '_>, name: &str) -> Option<bool> {
    let element = chart_el(node, name)?;
    Some(!matches!(element.attribute("val"), Some("0" | "false")))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::chart::{AxisKind, AxisPosition, Categories, ChartKind, Grouping, LegendPosition};
    use crate::excel::numfmt::NumFmt;
    use std::fmt::Write as _;

    /// The namespaces of a chart part.
    const NS: &str = r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006""#;

    /// A chart part with `inside` in its `c:chart` and `plot` in its plot
    /// area.
    pub(crate) fn space(inside: &str, plot: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><c:chartSpace {NS}><c:chart>{inside}<c:plotArea><c:layout/>{plot}</c:plotArea><c:legend><c:legendPos val="b"/></c:legend><c:plotVisOnly val="1"/></c:chart></c:chartSpace>"#
        )
    }

    /// The points `values` of a cache, `c:pt` with `c:v`.
    fn points(values: &[&str]) -> String {
        let mut out = String::new();
        for (idx, value) in values.iter().enumerate() {
            write!(out, r#"<c:pt idx="{idx}"><c:v>{value}</c:v></c:pt>"#).unwrap();
        }
        out
    }

    /// A string reference with its cache of `labels`.
    pub(crate) fn str_ref(labels: &[&str]) -> String {
        format!(
            r#"<c:strRef><c:f>Sheet1!$A$2:$A$9</c:f><c:strCache><c:ptCount val="{}"/>{}</c:strCache></c:strRef>"#,
            labels.len(),
            points(labels)
        )
    }

    /// A number reference with its cache of `values`, in the format `code`.
    pub(crate) fn num_ref(code: &str, values: &[&str]) -> String {
        format!(
            r#"<c:numRef><c:f>Sheet1!$B$2:$B$9</c:f><c:numCache><c:formatCode>{code}</c:formatCode><c:ptCount val="{}"/>{}</c:numCache></c:numRef>"#,
            values.len(),
            points(values)
        )
    }

    /// A series `idx` named `name` (by a string reference), with the
    /// categories `cats`, the values `vals`, and `more` (`c:spPr`, ...).
    pub(crate) fn ser(idx: u32, name: &str, cats: &[&str], vals: &[&str], more: &str) -> String {
        format!(
            r#"<c:ser><c:idx val="{idx}"/><c:order val="{idx}"/><c:tx>{}</c:tx>{more}<c:cat>{}</c:cat><c:val>{}</c:val></c:ser>"#,
            str_ref(&[name]),
            str_ref(cats),
            num_ref("General", vals)
        )
    }

    /// The two axes 1 (categories, bottom) and 2 (values, left).
    pub(crate) const AXES: &str = r#"<c:axId val="1"/><c:axId val="2"/>"#;
    /// The category and value axes of [`AXES`].
    pub(crate) const AXIS_PARTS: &str = r#"<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="l"/><c:majorGridlines/><c:numFmt formatCode="General" sourceLinked="1"/><c:crossAx val="1"/></c:valAx>"#;

    /// `xml` read with the Office theme and the default limits.
    pub(crate) fn chart(xml: &str) -> Chart {
        read(xml.as_bytes(), &Theme::default(), &ChartConfig::default()).unwrap()
    }

    /// The values of the series `series` of the group `group`.
    fn values(chart: &Chart, group: usize, series: usize) -> Vec<Option<f64>> {
        chart.groups[group].series[series]
            .values
            .as_ref()
            .unwrap()
            .points
            .clone()
    }

    /// The single-level text categories of a series.
    fn labels(categories: Option<&Categories>) -> Vec<Option<String>> {
        match categories {
            Some(Categories::Text(levels)) if levels.len() == 1 => levels[0].clone(),
            other => panic!("not one level of text: {other:?}"),
        }
    }

    #[test]
    fn stacked_bar_chart_with_two_series() {
        let plot = format!(
            r#"<c:barChart><c:barDir val="bar"/><c:grouping val="stacked"/><c:varyColors val="0"/>{}{}<c:gapWidth val="50"/><c:overlap val="100"/>{AXES}</c:barChart>{AXIS_PARTS}"#,
            ser(0, "North", &["Q1", "Q2", "Q3"], &["1", "2.5", "3"], ""),
            ser(1, "South", &["Q1", "Q2", "Q3"], &["4", "5", "6"], "")
        );
        let chart = chart(&space("", &plot));
        assert_eq!(chart.groups.len(), 1);
        let group = &chart.groups[0];
        assert_eq!(
            group.kind,
            ChartKind::Bar {
                horizontal: true,
                grouping: Grouping::Stacked,
                gap_width_ratio: 0.5,
                overlap_ratio: 1.0
            }
        );
        assert!(!group.three_d && !group.vary_colors);
        assert_eq!(group.axis_ids, vec![1, 2]);
        assert_eq!(group.series[0].name.as_deref(), Some("North"));
        assert_eq!(group.series[1].label(), "South");
        assert_eq!(
            labels(group.series[0].categories.as_ref()),
            vec![Some("Q1".into()), Some("Q2".into()), Some("Q3".into())]
        );
        assert_eq!(values(&chart, 0, 0), vec![Some(1.0), Some(2.5), Some(3.0)]);
        assert_eq!(group.series[0].values.as_ref().unwrap().format, NumFmt::General);
        assert!(chart.has_data && !chart.truncated);
        assert_eq!(chart.legend, Some(LegendPosition::Bottom));
        assert!(chart.plot_visible_only);
    }

    #[test]
    fn column_chart_defaults_to_clustered_columns() {
        let plot = format!(r#"<c:barChart>{}{AXES}</c:barChart>"#, ser(0, "A", &["x"], &["1"], ""));
        let chart = chart(&space("", &plot));
        assert_eq!(
            chart.groups[0].kind,
            ChartKind::Bar {
                horizontal: false,
                grouping: Grouping::Clustered,
                gap_width_ratio: 1.5,
                overlap_ratio: 0.0
            }
        );
    }

    #[test]
    fn line_chart_markers_follow_the_group_and_the_series() {
        let no_marker = r#"<c:marker><c:symbol val="none"/></c:marker>"#;
        let plot = format!(
            r#"<c:lineChart><c:grouping val="stacked"/>{}{}<c:marker val="1"/>{AXES}</c:lineChart>{AXIS_PARTS}"#,
            ser(0, "A", &["a", "b"], &["1", "2"], ""),
            ser(1, "B", &["a", "b"], &["3", "4"], no_marker)
        );
        let chart = chart(&space("", &plot));
        assert_eq!(
            chart.groups[0].kind,
            ChartKind::Line {
                grouping: Grouping::Stacked
            }
        );
        assert!(chart.groups[0].series[0].markers);
        assert!(!chart.groups[0].series[1].markers);
        assert!(chart.groups[0].series.iter().all(|series| series.line));
    }

    #[test]
    fn line_without_a_line_and_area_grouping() {
        let no_line = r#"<c:spPr><a:ln><a:noFill/></a:ln></c:spPr>"#;
        let plot = format!(
            r#"<c:lineChart><c:grouping val="standard"/>{}{AXES}</c:lineChart><c:areaChart><c:grouping val="percentStacked"/>{}{AXES}</c:areaChart>"#,
            ser(0, "A", &["a"], &["1"], no_line),
            ser(1, "B", &["a"], &["2"], "")
        );
        let chart = chart(&space("", &plot));
        assert!(!chart.groups[0].series[0].line);
        assert_eq!(
            chart.groups[1].kind,
            ChartKind::Area {
                grouping: Grouping::PercentStacked
            }
        );
    }

    #[test]
    fn pie_points_take_the_accents_or_their_own_colour() {
        let point =
            r#"<c:dPt><c:idx val="1"/><c:spPr><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></c:spPr></c:dPt>"#;
        let plot = format!(
            r#"<c:pieChart><c:varyColors val="1"/>{}<c:firstSliceAng val="90"/></c:pieChart>"#,
            ser(0, "Share", &["a", "b", "c"], &["1", "2", "3"], point)
        );
        let chart = chart(&space("", &plot));
        let group = &chart.groups[0];
        assert_eq!(group.kind, ChartKind::Pie { first_slice_deg: 90.0 });
        assert!(group.vary_colors);
        let theme = Theme::default();
        assert_eq!(
            group.series[0].point_colors,
            vec![theme.accents[0], [0, 255, 0], theme.accents[2]]
        );
    }

    #[test]
    fn three_d_pie_is_a_pie_marked_three_d() {
        let plot = format!(
            r#"<c:pie3DChart><c:varyColors val="1"/>{}</c:pie3DChart>"#,
            ser(0, "S", &["a"], &["1"], "")
        );
        let chart = chart(&space("", &plot));
        assert_eq!(chart.groups[0].kind, ChartKind::Pie { first_slice_deg: 0.0 });
        assert!(chart.groups[0].three_d);
    }

    #[test]
    fn doughnut_hole_size() {
        let plot = format!(
            r#"<c:doughnutChart><c:varyColors val="1"/>{}<c:firstSliceAng val="0"/><c:holeSize val="75"/></c:doughnutChart>"#,
            ser(0, "S", &["a", "b"], &["1", "1"], "")
        );
        let chart = chart(&space("", &plot));
        assert_eq!(
            chart.groups[0].kind,
            ChartKind::Doughnut {
                first_slice_deg: 0.0,
                hole_ratio: 0.75
            }
        );
    }

    #[test]
    fn scatter_series_has_x_and_y_values() {
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:v>Fit</c:v></c:tx><c:xVal>{}</c:xVal><c:yVal>{}</c:yVal><c:smooth val="0"/></c:ser>"#,
            num_ref("0.0", &["1", "2", "4"]),
            num_ref("General", &["10", "20", "40"])
        );
        let plot = format!(
            r#"<c:scatterChart><c:scatterStyle val="smoothMarker"/>{series}{AXES}</c:scatterChart><c:valAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:crossAx val="2"/></c:valAx><c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="l"/><c:crossAx val="1"/></c:valAx>"#
        );
        let chart = chart(&space("", &plot));
        let group = &chart.groups[0];
        assert_eq!(group.kind, ChartKind::Scatter { smooth: true });
        let series = &group.series[0];
        assert_eq!(series.name.as_deref(), Some("Fit"));
        let Some(Categories::Numbers(x)) = &series.categories else {
            panic!("x values")
        };
        assert_eq!(x.points, vec![Some(1.0), Some(2.0), Some(4.0)]);
        assert_eq!(x.format, NumFmt::Code("0.0".into()));
        assert_eq!(values(&chart, 0, 0), vec![Some(10.0), Some(20.0), Some(40.0)]);
        assert!(series.markers && series.line);
        assert_eq!(chart.axes.len(), 2);
        assert!(chart.axes.iter().all(|axis| axis.kind == AxisKind::Value));
    }

    #[test]
    fn scatter_with_markers_only_has_no_line() {
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:xVal>{}</c:xVal><c:yVal>{}</c:yVal></c:ser>"#,
            num_ref("General", &["1"]),
            num_ref("General", &["2"])
        );
        let chart = chart(&space(
            "",
            &format!(r#"<c:scatterChart><c:scatterStyle val="marker"/>{series}</c:scatterChart>"#),
        ));
        assert!(!chart.groups[0].series[0].line);
        assert!(chart.groups[0].series[0].markers);
        assert_eq!(chart.groups[0].series[0].label(), "Series 1");
    }

    #[test]
    fn radar_chart_keeps_its_style_and_data() {
        let plot = format!(
            r#"<c:radarChart><c:radarStyle val="marker"/>{}{AXES}</c:radarChart>"#,
            ser(0, "R", &["a", "b"], &["1", "2"], "")
        );
        let chart = chart(&space("", &plot));
        assert_eq!(chart.groups[0].kind, ChartKind::Radar { filled: false });
        assert_eq!(values(&chart, 0, 0), vec![Some(1.0), Some(2.0)]);
        assert!(chart.groups[0].series[0].markers && chart.groups[0].series[0].line);
        let filled = format!(
            r#"<c:radarChart><c:radarStyle val="filled"/>{}</c:radarChart>"#,
            ser(0, "R", &["a"], &["1"], "")
        );
        let chart = super::tests::chart(&space("", &filled));
        assert_eq!(chart.groups[0].kind, ChartKind::Radar { filled: true });
        assert!(!chart.groups[0].series[0].markers);
    }

    #[test]
    fn stock_chart_is_not_drawn_but_keeps_its_data() {
        let plot = format!(
            r#"<c:stockChart>{}{AXES}</c:stockChart>"#,
            ser(0, "S", &["a"], &["1"], "")
        );
        let chart = chart(&space("", &plot));
        assert_eq!(
            chart.groups[0].kind,
            ChartKind::NotDrawn {
                name: "stockChart".into()
            }
        );
        assert_eq!(values(&chart, 0, 0), vec![Some(1.0)]);
    }

    #[test]
    fn bar_and_line_combination_on_two_value_axes() {
        let plot = format!(
            r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>{}{AXES}</c:barChart><c:lineChart><c:grouping val="standard"/>{}<c:axId val="3"/><c:axId val="4"/></c:lineChart>{AXIS_PARTS}<c:valAx><c:axId val="4"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="r"/><c:crossAx val="3"/></c:valAx><c:catAx><c:axId val="3"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="1"/><c:axPos val="b"/><c:crossAx val="4"/></c:catAx>"#,
            ser(0, "Sales", &["a", "b"], &["10", "20"], ""),
            ser(1, "Margin", &["a", "b"], &["0.1", "0.2"], "")
        );
        let chart = chart(&space("", &plot));
        assert_eq!(chart.groups.len(), 2);
        assert_eq!(chart.groups[1].axis_ids, vec![3, 4]);
        assert_eq!(chart.axes.len(), 4);
        let right = chart.axes.iter().find(|axis| axis.id == 4).unwrap();
        assert_eq!(right.position, AxisPosition::Right);
        assert!(chart.axes.iter().find(|axis| axis.id == 3).unwrap().deleted);
        assert_eq!(chart.series().count(), 2);
    }

    #[test]
    fn multi_level_categories_outermost_first() {
        let cat = r#"<c:cat><c:multiLvlStrRef><c:f>S!A1:B4</c:f><c:multiLvlStrCache><c:ptCount val="4"/><c:lvl><c:pt idx="0"><c:v>Jan</c:v></c:pt><c:pt idx="1"><c:v>Feb</c:v></c:pt><c:pt idx="2"><c:v>Jan</c:v></c:pt><c:pt idx="3"><c:v>Feb</c:v></c:pt></c:lvl><c:lvl><c:pt idx="0"><c:v>2023</c:v></c:pt><c:pt idx="2"><c:v>2024</c:v></c:pt></c:lvl></c:multiLvlStrCache></c:multiLvlStrRef></c:cat>"#;
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/>{cat}<c:val>{}</c:val></c:ser>"#,
            num_ref("General", &["1", "2", "3", "4"])
        );
        let chart = chart(&space("", &format!("<c:barChart>{series}{AXES}</c:barChart>")));
        let Some(Categories::Text(levels)) = &chart.groups[0].series[0].categories else {
            panic!("text categories")
        };
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[0], vec![Some("2023".into()), None, Some("2024".into()), None]);
        assert_eq!(levels[1][3].as_deref(), Some("Feb"));
    }

    #[test]
    fn numeric_categories_keep_their_format() {
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:cat>{}</c:cat><c:val>{}</c:val></c:ser>"#,
            num_ref("m/d/yyyy", &["45292", "45323"]),
            num_ref("0.0%", &["0.25", "0.5"])
        );
        let chart = chart(&space("", &format!("<c:lineChart>{series}{AXES}</c:lineChart>")));
        let series = &chart.groups[0].series[0];
        let Some(Categories::Numbers(dates)) = &series.categories else {
            panic!("numbers")
        };
        assert_eq!(dates.format, NumFmt::Code("m/d/yyyy".into()));
        assert_eq!(series.values.as_ref().unwrap().format, NumFmt::Code("0.0%".into()));
    }

    #[test]
    fn literal_data_counts_as_cached() {
        let series = r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:cat><c:strLit><c:ptCount val="2"/><c:pt idx="0"><c:v>x</c:v></c:pt><c:pt idx="1"><c:v>y</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>5</c:v></c:pt><c:pt idx="1"><c:v>6</c:v></c:pt></c:numLit></c:val></c:ser>"#;
        let chart = chart(&space("", &format!("<c:barChart>{series}</c:barChart>")));
        assert_eq!(values(&chart, 0, 0), vec![Some(5.0), Some(6.0)]);
        assert_eq!(
            labels(chart.groups[0].series[0].categories.as_ref())[1].as_deref(),
            Some("y")
        );
        assert!(chart.has_data);
    }

    #[test]
    fn references_without_caches_give_no_data() {
        let series = r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>S!A1</c:f></c:strRef></c:tx><c:cat><c:strRef><c:f>S!A2:A5</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>S!B2:B5</c:f></c:numRef></c:val></c:ser>"#;
        let chart = chart(&space(
            r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title>"#,
            &format!("<c:barChart>{series}</c:barChart>"),
        ));
        assert!(!chart.has_data);
        assert_eq!(chart.title.as_deref(), Some("Sales"));
        let series = &chart.groups[0].series[0];
        assert!(series.values.is_none() && series.categories.is_none() && series.name.is_none());
    }

    #[test]
    fn sparse_points_and_bad_numbers_are_missing() {
        let val = r#"<c:val><c:numRef><c:f>S!B1:B6</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="6"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>NaN</c:v></c:pt><c:pt idx="2"><c:v>inf</c:v></c:pt><c:pt idx="3"><c:v>#N/A</c:v></c:pt><c:pt idx="5"><c:v>1e400</c:v></c:pt><c:pt idx="99"><c:v>7</c:v></c:pt></c:numCache></c:numRef></c:val>"#;
        let series = format!(r#"<c:ser><c:idx val="0"/><c:order val="0"/>{val}</c:ser>"#);
        let chart = chart(&space("", &format!("<c:lineChart>{series}</c:lineChart>")));
        // The point past ptCount is not part of the series.
        assert_eq!(values(&chart, 0, 0), vec![Some(1.0), None, None, None, None, None]);
    }

    #[test]
    fn series_colours_explicit_theme_and_automatic() {
        let explicit = r#"<c:spPr><a:solidFill><a:srgbClr val="123456"/></a:solidFill></c:spPr>"#;
        let scheme = r#"<c:spPr><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></c:spPr>"#;
        let line = r#"<c:spPr><a:ln w="28575"><a:solidFill><a:srgbClr val="ABCDEF"/></a:solidFill></a:ln></c:spPr>"#;
        let plot = format!(
            r#"<c:barChart>{}{}{}{}</c:barChart><c:lineChart>{}</c:lineChart>"#,
            ser(0, "a", &["x"], &["1"], explicit),
            ser(1, "b", &["x"], &["1"], scheme),
            ser(2, "c", &["x"], &["1"], ""),
            ser(6, "d", &["x"], &["1"], ""),
            ser(3, "e", &["x"], &["1"], line)
        );
        let theme_xml = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme name="T"><a:accent2><a:srgbClr val="FF0000"/></a:accent2><a:accent3><a:srgbClr val="00FF00"/></a:accent3></a:clrScheme></a:themeElements></a:theme>"#;
        let theme = Theme::read(Some(theme_xml.as_bytes()));
        let chart = read(space("", &plot).as_bytes(), &theme, &ChartConfig::default()).unwrap();
        let bars = &chart.groups[0].series;
        assert_eq!(bars[0].color, [0x12, 0x34, 0x56]);
        assert_eq!(bars[1].color, [255, 0, 0]);
        assert_eq!(bars[2].color, [0, 255, 0]);
        assert_eq!(bars[3].color, theme.automatic(6));
        // A line series is the colour of its line.
        assert_eq!(chart.groups[1].series[0].color, [0xAB, 0xCD, 0xEF]);
    }

    #[test]
    fn axes_titles_scaling_formats_and_gridlines() {
        let axes = r#"<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="maxMin"/></c:scaling><c:delete val="1"/><c:axPos val="l"/><c:title><c:tx><c:rich><a:p><a:r><a:t>Region</a:t></a:r></a:p></c:rich></c:tx></c:title><c:crossAx val="2"/></c:catAx><c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/><c:max val="100"/><c:min val="-5.5"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:numFmt formatCode="0%" sourceLinked="0"/><c:crossAx val="1"/></c:valAx><c:dateAx><c:axId val="3"/><c:scaling/><c:delete/><c:axPos val="t"/><c:majorGridlines/></c:dateAx>"#;
        let plot = format!(
            r#"<c:barChart><c:barDir val="bar"/>{}{AXES}</c:barChart>{axes}"#,
            ser(0, "a", &["x"], &["1"], "")
        );
        let chart = chart(&space("", &plot));
        let [cat, val, date] = &chart.axes[..] else {
            panic!("three axes")
        };
        assert_eq!(cat.kind, AxisKind::Category);
        assert!(cat.deleted && cat.reversed && !cat.major_gridlines);
        assert_eq!(cat.title.as_deref(), Some("Region"));
        assert_eq!(cat.position, AxisPosition::Left);
        assert_eq!(cat.cross_axis, Some(2));
        assert_eq!((val.min, val.max), (Some(-5.5), Some(100.0)));
        assert_eq!(val.format, Some(NumFmt::Code("0%".into())));
        assert!(!val.deleted && !val.reversed && val.title.is_none());
        // `c:delete` with no value is true, as the schema's default.
        assert_eq!(date.kind, AxisKind::Date);
        assert!(date.deleted && date.major_gridlines);
        assert_eq!(date.position, AxisPosition::Top);
        // Linked to the source: no format of its own.
        let plain = chart_with_axes();
        assert_eq!(plain.axes[1].format, None);
        assert!(plain.axes[1].major_gridlines);
    }

    /// A bar chart with [`AXIS_PARTS`].
    fn chart_with_axes() -> Chart {
        chart(&space(
            "",
            &format!(
                r#"<c:barChart>{}{AXES}</c:barChart>{AXIS_PARTS}"#,
                ser(0, "a", &["x"], &["1"], "")
            ),
        ))
    }

    #[test]
    fn titles_rich_referenced_automatic_or_deleted() {
        let bar = format!(r#"<c:barChart>{}</c:barChart>"#, ser(0, "Only", &["x"], &["1"], ""));
        let rich = r#"<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Energy </a:t></a:r><a:r><a:t>prices</a:t></a:r></a:p><a:p><a:r><a:t>2024</a:t></a:r></a:p></c:rich></c:tx></c:title>"#;
        assert_eq!(chart(&space(rich, &bar)).title.as_deref(), Some("Energy prices 2024"));
        let referenced = format!(r#"<c:title><c:tx>{}</c:tx></c:title>"#, str_ref(&["From a cell"]));
        assert_eq!(chart(&space(&referenced, &bar)).title.as_deref(), Some("From a cell"));
        // An automatic title is the name of the only series.
        assert_eq!(chart(&space("<c:title/>", &bar)).title.as_deref(), Some("Only"));
        assert_eq!(
            chart(&space(r#"<c:title/><c:autoTitleDeleted val="1"/>"#, &bar)).title,
            None
        );
        assert_eq!(chart(&space("", &bar)).title, None);
    }

    #[test]
    fn legend_absent_style_and_date1904() {
        let xml = format!(
            r#"<c:chartSpace {NS}><c:date1904 val="1"/><mc:AlternateContent><mc:Choice Requires="c14"><c14:style xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart" val="102"/></mc:Choice><mc:Fallback><c:style val="2"/></mc:Fallback></mc:AlternateContent><c:chart><c:plotArea><c:barChart>{}</c:barChart></c:plotArea></c:chart></c:chartSpace>"#,
            ser(0, "a", &["x"], &["1"], "")
        );
        let chart = chart(&xml);
        assert_eq!(chart.legend, None);
        assert_eq!(chart.style, Some(2));
        assert!(chart.date1904 && !chart.plot_visible_only);
    }

    #[test]
    fn damaged_or_foreign_parts_are_errors() {
        let config = ChartConfig::default();
        let theme = Theme::default();
        assert!(read(b"<c:chartSpace", &theme, &config).is_err());
        assert!(read(b"<root/>", &theme, &config).is_err());
        assert!(read(&[0xFF, 0xFE, 0x00], &theme, &config).is_err());
        let small = ChartConfig {
            max_part_bytes: 10,
            ..config
        };
        assert!(read(chart_space_bytes().as_bytes(), &theme, &small).is_err());
    }

    /// The bytes of a small chart part.
    fn chart_space_bytes() -> String {
        space(
            "",
            &format!("<c:barChart>{}</c:barChart>", ser(0, "a", &["x"], &["1"], "")),
        )
    }

    #[test]
    fn a_hundred_thousand_points_are_truncated_quickly() {
        let n = 100_000;
        let mut pts = String::with_capacity(n * 40);
        for idx in 0..n {
            write!(pts, r#"<c:pt idx="{idx}"><c:v>{idx}.5</c:v></c:pt>"#).unwrap();
        }
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:cat><c:numRef><c:numCache><c:ptCount val="{n}"/>{}</c:numCache></c:numRef></c:cat><c:val><c:numRef><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="{n}"/>{pts}</c:numCache></c:numRef></c:val></c:ser>"#,
            pts
        );
        let xml = space("", &format!("<c:lineChart>{series}</c:lineChart>"));
        let start = std::time::Instant::now();
        let chart = chart(&xml);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            start.elapsed()
        );
        let max = ChartConfig::default().max_points_per_series;
        assert_eq!(values(&chart, 0, 0).len(), max);
        assert!(chart.truncated);
        let Some(Categories::Numbers(cats)) = &chart.groups[0].series[0].categories else {
            panic!("numbers")
        };
        assert_eq!(cats.points.len(), max);
    }

    #[test]
    fn an_absurd_point_count_allocates_nothing_absurd() {
        let val = r#"<c:val><c:numRef><c:numCache><c:ptCount val="4000000000"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="3999999999"><c:v>2</c:v></c:pt></c:numCache></c:numRef></c:val>"#;
        let cat = r#"<c:cat><c:strRef><c:strCache><c:ptCount val="999999999999999999999"/><c:pt idx="0"><c:v>a</c:v></c:pt></c:strCache></c:strRef></c:cat>"#;
        let series = format!(r#"<c:ser><c:idx val="4294967295"/><c:order val="0"/>{cat}{val}</c:ser>"#);
        let small = ChartConfig {
            max_points_per_series: 5,
            ..ChartConfig::default()
        };
        let xml = space("", &format!("<c:barChart>{series}</c:barChart>"));
        let chart = read(xml.as_bytes(), &Theme::default(), &small).unwrap();
        assert_eq!(values(&chart, 0, 0), vec![Some(1.0), None, None, None, None]);
        assert!(chart.truncated);
        assert_eq!(chart.groups[0].series[0].label(), "Series 4294967296");
    }

    #[test]
    fn too_many_series_and_long_texts_are_cut() {
        let long = "x".repeat(50);
        let mut plot = String::from("<c:lineChart>");
        for idx in 0..10 {
            plot.push_str(&ser(idx, &long, &["a"], &["1"], ""));
        }
        plot.push_str("</c:lineChart><c:barChart>");
        plot.push_str(&ser(10, "b", &["a"], &["1"], ""));
        plot.push_str("</c:barChart>");
        let config = ChartConfig {
            max_series_count: 3,
            max_label_chars: 10,
            ..ChartConfig::default()
        };
        let chart = read(space("", &plot).as_bytes(), &Theme::default(), &config).unwrap();
        assert_eq!(chart.series().count(), 3);
        assert!(chart.truncated);
        assert_eq!(chart.groups[0].series[0].name.as_deref().map(str::len), Some(10));
    }

    #[test]
    fn series_are_in_plot_order() {
        let first = ser(0, "first", &["a"], &["1"], "").replace(r#"<c:order val="0"/>"#, r#"<c:order val="1"/>"#);
        let second = ser(1, "second", &["a"], &["1"], "").replace(r#"<c:order val="1"/>"#, r#"<c:order val="0"/>"#);
        let chart = chart(&space("", &format!("<c:barChart>{first}{second}</c:barChart>")));
        assert_eq!(chart.groups[0].series[0].label(), "second");
    }
}
