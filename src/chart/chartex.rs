//! The extended chart part of Office 2016 (`cx:chartSpace`: waterfall,
//! treemap, sunburst, funnel, histogram, box and whisker, region map). None
//! of them is drawn: the reader takes the title and, when it is simple, the
//! data (`cx:data` with its `cx:strDim` and `cx:numDim` levels) of each
//! series, so that the chart still gives its numbers. It never fails: what
//! it cannot read is left out.

use super::read::{CHARTEX_NS, Limits, cached, legend_position, non_empty, num_fmt, number, rich_text};
use super::theme::Theme;
use super::{Categories, Chart, ChartConfig, ChartKind, Numbers, PlotGroup, Series};
use crate::word::xml::is;
use roxmltree::Node;
use std::collections::HashMap;

/// The kind of an extended chart whose series name no layout.
const UNKNOWN_LAYOUT: &str = "chartex";

/// The chart of the extended chart part whose root is `root`, its series
/// coloured by `theme`'s automatic colours, within the limits of `config`.
pub(super) fn read(root: Node<'_, '_>, theme: &Theme, config: &ChartConfig) -> Chart {
    let mut limits = Limits::new(config);
    let data: HashMap<&str, Node<'_, '_>> = cx(root, "chartData")
        .into_iter()
        .flat_map(|chart_data| cx_children(chart_data, "data"))
        .filter_map(|node| Some((node.attribute("id")?, node)))
        .collect();
    let chart_node = cx(root, "chart");
    let title = chart_node
        .and_then(|node| cx(node, "title"))
        .and_then(|title| text(title, &mut limits));
    let legend = chart_node
        .and_then(|node| cx(node, "legend"))
        .map(|legend| legend_position(legend.attribute("pos")));
    let series_nodes: Vec<Node<'_, '_>> = chart_node
        .and_then(|node| cx(node, "plotArea"))
        .and_then(|area| cx(area, "plotAreaRegion"))
        .map(|region| cx_children(region, "series").collect())
        .unwrap_or_default();
    let mut groups: Vec<PlotGroup> = Vec::new();
    for (position, node) in series_nodes.into_iter().enumerate() {
        if !limits.take_series() {
            break;
        }
        let layout = node.attribute("layoutId").unwrap_or(UNKNOWN_LAYOUT);
        let series = read_series(node, position, &data, theme, &mut limits);
        match groups.last_mut() {
            Some(group) if matches!(&group.kind, ChartKind::NotDrawn { name } if name == layout) => {
                group.series.push(series);
            }
            _ => groups.push(not_drawn(layout, vec![series])),
        }
    }
    if groups.is_empty() {
        groups.push(not_drawn(UNKNOWN_LAYOUT, Vec::new()));
    }
    let has_data = groups
        .iter()
        .flat_map(|group| &group.series)
        .any(|series| series.values.as_ref().is_some_and(|values| !values.points.is_empty()));
    Chart {
        title,
        groups,
        legend,
        has_data,
        truncated: limits.truncated,
        ..Chart::default()
    }
}

/// A group of the extended chart layout `layout`, with `series`.
fn not_drawn(layout: &str, series: Vec<Series>) -> PlotGroup {
    PlotGroup {
        kind: ChartKind::NotDrawn {
            name: layout.to_string(),
        },
        three_d: false,
        vary_colors: false,
        axis_ids: Vec::new(),
        series,
    }
}

/// The series `node`, the `position`-th of the chart, with its data from
/// `data` by its `cx:dataId`.
fn read_series(
    node: Node<'_, '_>,
    position: usize,
    data: &HashMap<&str, Node<'_, '_>>,
    theme: &Theme,
    limits: &mut Limits<'_>,
) -> Series {
    let source = cx(node, "dataId")
        .and_then(|id| id.attribute("val"))
        .and_then(|id| data.get(id).copied());
    let mut categories = None;
    let mut values = None;
    for dim in source
        .into_iter()
        .flat_map(|source| source.children().filter(Node::is_element))
    {
        let kind = dim.attribute("type").unwrap_or_default();
        if is(dim, CHARTEX_NS, "strDim") && categories.is_none() {
            let mut levels: Vec<_> = cx_children(dim, "lvl").map(|level| labels(level, limits)).collect();
            levels.reverse();
            categories = (!levels.is_empty()).then_some(Categories::Text(levels));
        } else if is(dim, CHARTEX_NS, "numDim") {
            let Some(level) = cx(dim, "lvl") else {
                continue;
            };
            if matches!(kind, "cat" | "x") && categories.is_none() {
                categories = Some(Categories::Numbers(numbers(level, limits)));
            } else if values.is_none() {
                values = Some(numbers(level, limits));
            }
        }
    }
    let index = u32::try_from(position).unwrap_or(u32::MAX);
    Series {
        index,
        name: cx(node, "tx").and_then(|tx| tx_text(tx, limits)),
        categories,
        values,
        color: theme.automatic(position),
        point_colors: Vec::new(),
        markers: false,
        line: false,
    }
}

/// The text of a title: its `cx:tx`.
fn text(title: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<String> {
    tx_text(cx(title, "tx")?, limits)
}

/// The text of a `cx:tx`: its rich text, or the value of its `cx:txData`.
fn tx_text(tx: Node<'_, '_>, limits: &mut Limits<'_>) -> Option<String> {
    if let Some(rich) = cx(tx, "rich") {
        return non_empty(limits.clip(&rich_text(rich)));
    }
    let value = cx(cx(tx, "txData")?, "v")?.text()?;
    non_empty(limits.clip(value.trim()))
}

/// The strings of the level `level` of a `cx:strDim`.
fn labels(level: Node<'_, '_>, limits: &mut Limits<'_>) -> Vec<Option<String>> {
    cached(cx_children(level, "pt"), count(level), limits, |pt, limits| {
        Some(limits.clip(pt.text().unwrap_or_default()))
    })
}

/// The numbers of the level `level` of a `cx:numDim`, with its format.
fn numbers(level: Node<'_, '_>, limits: &mut Limits<'_>) -> Numbers {
    Numbers {
        points: cached(cx_children(level, "pt"), count(level), limits, |pt, _| {
            number(pt.text()?)
        }),
        format: num_fmt(level.attribute("formatCode").unwrap_or_default()),
    }
}

/// The `ptCount` of a level.
fn count(level: Node<'_, '_>) -> Option<u64> {
    level.attribute("ptCount")?.parse().ok()
}

/// The first extended chart element `name` of `node`.
fn cx<'a, 'i: 'a>(node: Node<'a, 'i>, name: &str) -> Option<Node<'a, 'i>> {
    node.children().find(|item| is(*item, CHARTEX_NS, name))
}

/// The extended chart elements `name` of `node`, in order.
fn cx_children<'a, 'i: 'a>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |item| is(*item, CHARTEX_NS, name))
}

#[cfg(test)]
mod tests {
    use crate::chart::{Categories, ChartConfig, ChartKind, LegendPosition, Theme, read};

    /// An extended chart part with `chart_data` and `chart` inside.
    fn space(chart_data: &str, chart: &str) -> String {
        format!(
            r#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><cx:chartData>{chart_data}</cx:chartData><cx:chart>{chart}</cx:chart></cx:chartSpace>"#
        )
    }

    /// The data `0` of a waterfall: four labels and four values (one of
    /// them damaged).
    const DATA: &str = r##"<cx:data id="0"><cx:strDim type="cat"><cx:f>Sheet1!$A$2:$A$5</cx:f><cx:lvl ptCount="4"><cx:pt idx="0">Start</cx:pt><cx:pt idx="1">Sales</cx:pt><cx:pt idx="2">Costs</cx:pt><cx:pt idx="3">End</cx:pt></cx:lvl></cx:strDim><cx:numDim type="val"><cx:f>Sheet1!$B$2:$B$5</cx:f><cx:lvl ptCount="4" formatCode="#,##0"><cx:pt idx="0">100</cx:pt><cx:pt idx="1">50</cx:pt><cx:pt idx="2">NaN</cx:pt><cx:pt idx="3">120</cx:pt></cx:lvl></cx:numDim></cx:data>"##;

    /// A waterfall series named `Amount` on the data `0`.
    const SERIES: &str = r#"<cx:plotArea><cx:plotAreaRegion><cx:series layoutId="waterfall" uniqueId="{1}"><cx:tx><cx:txData><cx:f>Sheet1!$B$1</cx:f><cx:v>Amount</cx:v></cx:txData></cx:tx><cx:dataId val="0"/></cx:series></cx:plotAreaRegion></cx:plotArea><cx:legend pos="b" align="ctr" overlay="0"/>"#;

    /// `xml` read with the Office theme and the default limits.
    fn chart(xml: &str) -> crate::chart::Chart {
        read(xml.as_bytes(), &Theme::default(), &ChartConfig::default()).unwrap()
    }

    #[test]
    fn waterfall_gives_its_title_and_data() {
        let title = r#"<cx:title pos="t"><cx:tx><cx:txData><cx:v>Cash flow</cx:v></cx:txData></cx:tx></cx:title>"#;
        let chart = chart(&space(DATA, &format!("{title}{SERIES}")));
        assert_eq!(chart.title.as_deref(), Some("Cash flow"));
        assert_eq!(chart.groups.len(), 1);
        assert_eq!(
            chart.groups[0].kind,
            ChartKind::NotDrawn {
                name: "waterfall".into()
            }
        );
        let series = &chart.groups[0].series[0];
        assert_eq!(series.name.as_deref(), Some("Amount"));
        assert_eq!(series.color, Theme::default().accents[0]);
        let values = series.values.as_ref().unwrap();
        assert_eq!(values.points, vec![Some(100.0), Some(50.0), None, Some(120.0)]);
        assert_eq!(values.format, crate::excel::numfmt::NumFmt::Code("#,##0".into()));
        let Some(Categories::Text(levels)) = &series.categories else {
            panic!("labels")
        };
        assert_eq!(levels[0][3].as_deref(), Some("End"));
        assert!(chart.has_data);
        assert_eq!(chart.legend, Some(LegendPosition::Bottom));
    }

    #[test]
    fn rich_title_and_treemap_levels() {
        let data = r#"<cx:data id="1"><cx:strDim type="cat"><cx:lvl ptCount="2"><cx:pt idx="0">Apples</cx:pt><cx:pt idx="1">Pears</cx:pt></cx:lvl><cx:lvl ptCount="2"><cx:pt idx="0">Fruit</cx:pt><cx:pt idx="1">Fruit</cx:pt></cx:lvl></cx:strDim><cx:numDim type="size"><cx:lvl ptCount="2"><cx:pt idx="0">3</cx:pt><cx:pt idx="1">4</cx:pt></cx:lvl></cx:numDim></cx:data>"#;
        let body = r#"<cx:title><cx:tx><cx:rich><a:p><a:r><a:t>Market</a:t></a:r></a:p></cx:rich></cx:tx></cx:title><cx:plotArea><cx:plotAreaRegion><cx:series layoutId="treemap"><cx:dataId val="1"/></cx:series></cx:plotAreaRegion></cx:plotArea>"#;
        let chart = chart(&space(data, body));
        assert_eq!(chart.title.as_deref(), Some("Market"));
        let series = &chart.groups[0].series[0];
        let Some(Categories::Text(levels)) = &series.categories else {
            panic!("labels")
        };
        assert_eq!(levels[0][0].as_deref(), Some("Fruit"));
        assert_eq!(levels[1][1].as_deref(), Some("Pears"));
        assert_eq!(series.values.as_ref().unwrap().points, vec![Some(3.0), Some(4.0)]);
        assert_eq!(chart.legend, None);
    }

    #[test]
    fn unreadable_extended_chart_still_gives_a_chart() {
        // No data for the series' id, no layout: a chart with no data.
        let body = r#"<cx:title><cx:tx><cx:txData><cx:v>Odd</cx:v></cx:txData></cx:tx></cx:title><cx:plotArea><cx:plotAreaRegion><cx:series><cx:dataId val="7"/></cx:series></cx:plotAreaRegion></cx:plotArea>"#;
        let chart = chart(&space("<cx:data id=\"x\"><cx:numDim/></cx:data>", body));
        assert_eq!(chart.title.as_deref(), Some("Odd"));
        assert!(!chart.has_data);
        assert!(matches!(&chart.groups[0].kind, ChartKind::NotDrawn { .. }));
        // An empty part: still a chart.
        let empty = chart_of(r#"<cx:chartSpace xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"/>"#);
        assert!(
            empty
                .groups
                .iter()
                .all(|group| matches!(group.kind, ChartKind::NotDrawn { .. }))
        );
        assert!(!empty.has_data);
    }

    /// `xml` read, which must not be an error.
    fn chart_of(xml: &str) -> crate::chart::Chart {
        chart(xml)
    }

    #[test]
    fn extended_chart_points_are_capped() {
        let mut pts = String::new();
        for idx in 0..50 {
            pts.push_str(&format!(r#"<cx:pt idx="{idx}">{idx}</cx:pt>"#));
        }
        let data = format!(
            r#"<cx:data id="0"><cx:numDim type="val"><cx:lvl ptCount="50">{pts}</cx:lvl></cx:numDim></cx:data>"#
        );
        let body = r#"<cx:plotArea><cx:plotAreaRegion><cx:series layoutId="funnel"><cx:dataId val="0"/></cx:series></cx:plotAreaRegion></cx:plotArea>"#;
        let config = ChartConfig {
            max_points_per_series: 10,
            ..ChartConfig::default()
        };
        let chart = read(space(&data, body).as_bytes(), &Theme::default(), &config).unwrap();
        assert_eq!(chart.groups[0].series[0].values.as_ref().unwrap().points.len(), 10);
        assert!(chart.truncated);
    }
}
