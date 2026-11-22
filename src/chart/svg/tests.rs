//! Tests of the chart drawing, through [`svg`](super::svg).

use crate::chart::read::tests::{AXES, AXIS_PARTS, chart, num_ref, ser, space, str_ref};
use crate::chart::svg;
use crate::chart::{Categories, Chart, ChartKind, Grouping, Numbers, PlotGroup, Series};
use crate::excel::numfmt::NumFmt;
use roxmltree::{Document, Node};

/// The default size of the tests, in points.
const SIZE: (f32, f32) = (360.0, 216.0);
/// The most bytes a chart's SVG may take.
const MAX_SVG_BYTES: usize = 400_000;

/// `xml` read and drawn at [`SIZE`].
fn draw(xml: &str) -> String {
    svg(&chart(xml), SIZE).expect("drawn")
}

/// `svg` parsed by roxmltree, after checking that usvg reads it too.
fn parsed(svg: &str) -> Document<'_> {
    resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("usvg reads it");
    Document::parse(svg).expect("well-formed")
}

/// The elements of `doc` of the class `class`.
fn of_class<'a, 'i>(doc: &'a Document<'i>, class: &str) -> Vec<Node<'a, 'i>> {
    doc.descendants()
        .filter(|node| node.attribute("class") == Some(class))
        .collect()
}

/// The number attribute `name` of `node`.
fn num(node: Node<'_, '_>, name: &str) -> f32 {
    node.attribute(name).unwrap().parse().unwrap()
}

/// The plot area: x, y, width, height.
fn plot(doc: &Document<'_>) -> (f32, f32, f32, f32) {
    let rect = of_class(doc, "plot")[0];
    (num(rect, "x"), num(rect, "y"), num(rect, "width"), num(rect, "height"))
}

/// The points of the path data `data` (`M`/`L` commands), one list per
/// subpath.
fn subpaths(data: &str) -> Vec<Vec<(f32, f32)>> {
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut tokens = data.split_whitespace();
    while let Some(command) = tokens.next() {
        let x: f32 = tokens.next().unwrap().parse().unwrap();
        let y: f32 = tokens.next().unwrap().parse().unwrap();
        match command {
            "M" => out.push(vec![(x, y)]),
            "L" => out.last_mut().unwrap().push((x, y)),
            other => panic!("command {other}"),
        }
    }
    out
}

/// The text of `node`.
fn text(node: Node<'_, '_>) -> String {
    node.text().unwrap_or_default().to_string()
}

/// A column chart of two series on [`AXIS_PARTS`], in `grouping`.
fn columns(grouping: &str, direction: &str) -> String {
    let plot = format!(
        r#"<c:barChart><c:barDir val="{direction}"/><c:grouping val="{grouping}"/><c:overlap val="100"/>{}{}{AXES}</c:barChart>{AXIS_PARTS}"#,
        ser(0, "North", &["Q1", "Q2", "Q3"], &["1", "2", "3"], ""),
        ser(1, "South", &["Q1", "Q2", "Q3"], &["4", "5", "6"], "")
    );
    space(
        r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>Sales</a:t></a:r></a:p></c:rich></c:tx></c:title>"#,
        &plot,
    )
}

/// A one-series chart of `values` in the plot group element `group`
/// (with its options `options`), on [`AXIS_PARTS`].
fn single(group: &str, options: &str, values: &[&str]) -> String {
    let cats: Vec<String> = (1..=values.len()).map(|n| format!("c{n}")).collect();
    let cats: Vec<&str> = cats.iter().map(String::as_str).collect();
    space(
        "",
        &format!(
            "<c:{group}>{options}{}{AXES}</c:{group}>{AXIS_PARTS}",
            ser(0, "S", &cats, values, "")
        ),
    )
}

#[test]
fn every_drawn_kind_is_well_formed_svg() {
    let pie = format!(
        r#"<c:pieChart><c:varyColors val="1"/>{}</c:pieChart>"#,
        ser(0, "P", &["a", "b", "c"], &["1", "2", "3"], "")
    );
    let doughnut = format!(
        r#"<c:doughnutChart><c:varyColors val="1"/>{}<c:holeSize val="50"/></c:doughnutChart>"#,
        ser(0, "P", &["a", "b"], &["1", "2"], "")
    );
    let charts = [
        columns("clustered", "col"),
        columns("stacked", "col"),
        columns("percentStacked", "bar"),
        columns("clustered", "bar"),
        single(
            "lineChart",
            r#"<c:grouping val="standard"/><c:marker val="1"/>"#,
            &["1", "3", "2"],
        ),
        single("lineChart", r#"<c:grouping val="stacked"/>"#, &["1", "3", "2"]),
        single("areaChart", r#"<c:grouping val="standard"/>"#, &["1", "3", "2"]),
        single("areaChart", r#"<c:grouping val="stacked"/>"#, &["1", "3", "2"]),
        single("radarChart", r#"<c:radarStyle val="marker"/>"#, &["1", "3", "2", "4"]),
        single("radarChart", r#"<c:radarStyle val="filled"/>"#, &["1", "3", "2", "4"]),
        space("", &pie),
        space("", &doughnut),
        scatter("lineMarker"),
        combination(),
    ];
    for xml in &charts {
        let svg = draw(xml);
        let doc = parsed(&svg);
        assert_eq!(doc.root_element().tag_name().name(), "svg");
        assert!(!svg.contains("NaN") && !svg.contains("inf"), "{svg}");
    }
}

/// A scatter chart of two series in the style `style`.
fn scatter(style: &str) -> String {
    let one = format!(
        r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:v>A</c:v></c:tx><c:xVal>{}</c:xVal><c:yVal>{}</c:yVal></c:ser>"#,
        num_ref("General", &["1", "2", "4"]),
        num_ref("General", &["10", "20", "15"])
    );
    let axes = r#"<c:valAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="b"/><c:crossAx val="2"/></c:valAx><c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:axPos val="l"/><c:majorGridlines/><c:crossAx val="1"/></c:valAx>"#;
    space(
        "",
        &format!(r#"<c:scatterChart><c:scatterStyle val="{style}"/>{one}{AXES}</c:scatterChart>{axes}"#),
    )
}

/// Columns on the primary axes and a line on a secondary value axis at
/// the right.
fn combination() -> String {
    let plot = format!(
        r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>{}{AXES}</c:barChart><c:lineChart><c:grouping val="standard"/>{}<c:axId val="3"/><c:axId val="4"/></c:lineChart>{AXIS_PARTS}<c:valAx><c:axId val="4"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="r"/><c:numFmt formatCode="0%" sourceLinked="0"/><c:crossAx val="3"/></c:valAx><c:catAx><c:axId val="3"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="1"/><c:axPos val="b"/><c:crossAx val="4"/></c:catAx>"#,
        ser(0, "Sales", &["a", "b", "c"], &["100", "200", "150"], ""),
        ser(1, "Margin", &["a", "b", "c"], &["0.1", "0.25", "0.2"], "")
    );
    space("", &plot)
}

#[test]
fn kinds_not_drawn_and_charts_without_data_give_nothing() {
    let stock = single("stockChart", "", &["1", "2"]);
    assert!(svg(&chart(&stock), SIZE).is_none());
    let uncached = space(
        "",
        r#"<c:barChart><c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numRef><c:f>S!B1:B3</c:f></c:numRef></c:val></c:ser></c:barChart>"#,
    );
    assert!(svg(&chart(&uncached), SIZE).is_none());
    assert!(svg(&Chart::default(), SIZE).is_none());
}

#[test]
fn value_ticks_are_nice_numbers() {
    let doc_svg = draw(&single("barChart", "", &["12", "37", "5"]));
    let doc = parsed(&doc_svg);
    let ticks: Vec<f64> = of_class(&doc, "tick")
        .into_iter()
        .map(|tick| text(tick).parse().unwrap())
        .collect();
    // From zero to the first nice number past 37, by a nice step.
    let step = ticks[1] - ticks[0];
    assert!([5.0, 10.0].contains(&step), "{ticks:?}");
    assert_eq!((ticks[0], *ticks.last().unwrap()), (0.0, 40.0));
    assert!(ticks.windows(2).all(|pair| pair[1] - pair[0] == step));
    // A gridline at each tick, as the value axis has majorGridlines.
    assert_eq!(of_class(&doc, "grid").len(), ticks.len());
}

#[test]
fn fixed_bounds_and_the_axis_format_set_the_ticks() {
    let axes = AXIS_PARTS.replace(
        r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling>"#,
        r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/><c:max val="0.5"/><c:min val="-0.1"/></c:scaling>"#,
    )
    .replace(r#"<c:numFmt formatCode="General" sourceLinked="1"/>"#, r#"<c:numFmt formatCode="0%" sourceLinked="0"/>"#);
    let xml = space(
        "",
        &format!(
            "<c:lineChart>{}{AXES}</c:lineChart>{axes}",
            ser(0, "S", &["a", "b"], &["0.1", "0.2"], "")
        ),
    );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    let ticks: Vec<String> = of_class(&doc, "tick").into_iter().map(text).collect();
    assert_eq!(ticks, ["-10%", "0%", "10%", "20%", "30%", "40%", "50%"]);
}

#[test]
fn bars_and_lines_stay_inside_the_plot_area() {
    for xml in [columns("clustered", "col"), columns("clustered", "bar"), combination()] {
        let svg = draw(&xml);
        let doc = parsed(&svg);
        let (left, top, width, height) = plot(&doc);
        let inside = |px: f32, py: f32| {
            px >= left - 0.01 && px <= left + width + 0.01 && py >= top - 0.01 && py <= top + height + 0.01
        };
        let bars = of_class(&doc, "bar");
        assert!(!bars.is_empty());
        for bar in bars {
            let (bx, by) = (num(bar, "x"), num(bar, "y"));
            assert!(
                inside(bx, by) && inside(bx + num(bar, "width"), by + num(bar, "height")),
                "{bx} {by}"
            );
        }
        for line in of_class(&doc, "line") {
            for (px, py) in subpaths(line.attribute("d").unwrap()).concat() {
                assert!(inside(px, py), "{px} {py}");
            }
        }
    }
}

#[test]
fn reversed_value_axis_hangs_columns_from_the_top() {
    let xml = single("barChart", "", &["1", "2"]).replace(
        r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/>"#,
        r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="maxMin"/>"#,
    );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    let (_, top, _, _) = plot(&doc);
    for bar in of_class(&doc, "bar") {
        assert!((num(bar, "y") - top).abs() < 0.01);
    }
}

#[test]
fn horizontal_bars_put_the_first_category_at_the_bottom() {
    let svg = draw(&single("barChart", r#"<c:barDir val="bar"/>"#, &["1", "2"]));
    let doc = parsed(&svg);
    let bars = of_class(&doc, "bar");
    assert!(num(bars[0], "y") > num(bars[1], "y"));
    assert!(num(bars[0], "width") < num(bars[1], "width"));
}

#[test]
fn stacked_segments_add_up_to_the_total() {
    let svg = draw(&columns("percentStacked", "col"));
    let doc = parsed(&svg);
    let (_, _, _, height) = plot(&doc);
    let bars = of_class(&doc, "bar");
    assert_eq!(bars.len(), 6);
    let gap = super::SEGMENT_GAP_PT;
    // The bars come category by category. Each category is a whole: its
    // two segments and the gap between them.
    for category in 0..3 {
        let total: f32 = [bars[2 * category], bars[2 * category + 1]]
            .iter()
            .map(|bar| num(*bar, "height"))
            .sum();
        assert!((total + gap - height).abs() < 0.1, "{total} {height}");
    }
    // Stacked: the third category (3 + 6) is 9/5 the first (1 + 4).
    let svg = draw(&columns("stacked", "col"));
    let doc = parsed(&svg);
    let bars = of_class(&doc, "bar");
    let stack =
        |category: usize| -> f32 { num(bars[2 * category], "height") + num(bars[2 * category + 1], "height") + gap };
    assert!((stack(2) / stack(0) - 9.0 / 5.0).abs() < 0.01);
}

/// The sweep of each pie slice of `doc`, in degrees.
fn slice_sweeps(doc: &Document<'_>) -> Vec<f32> {
    of_class(doc, "slice")
        .into_iter()
        .map(|slice| {
            let data = slice.attribute("d").unwrap();
            let numbers: Vec<f32> = data
                .split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
                .filter(|piece| !piece.is_empty())
                .map(|piece| piece.parse().unwrap())
                .collect();
            // M cx cy L x1 y1 A r r 0 large 1 x2 y2 Z
            let (cx, cy, x1, y1, large, x2, y2) = (
                numbers[0],
                numbers[1],
                numbers[2],
                numbers[3],
                numbers[7],
                numbers[9],
                numbers[10],
            );
            let angle = |x: f32, y: f32| (x - cx).atan2(cy - y).to_degrees();
            let mut sweep = (angle(x2, y2) - angle(x1, y1)).rem_euclid(360.0);
            if large == 1.0 && sweep < 180.0 {
                sweep += 360.0;
            }
            sweep
        })
        .collect()
}

#[test]
fn pie_slices_sum_to_a_full_turn() {
    let pie = format!(
        r#"<c:pieChart><c:varyColors val="1"/>{}<c:firstSliceAng val="30"/></c:pieChart>"#,
        ser(0, "P", &["a", "b", "c", "d"], &["1", "2", "3", "#N/A"], "")
    );
    let svg = draw(&space("", &pie));
    let doc = parsed(&svg);
    let sweeps = slice_sweeps(&doc);
    assert_eq!(sweeps.len(), 3);
    assert!((sweeps.iter().sum::<f32>() - 360.0).abs() < 0.1, "{sweeps:?}");
    assert!((sweeps[2] - 180.0).abs() < 0.1);
    // The slices take the point colours (accents 1 to 3).
    let fills: Vec<&str> = of_class(&doc, "slice")
        .iter()
        .map(|slice| slice.attribute("fill").unwrap())
        .collect();
    assert_eq!(fills, ["#4472c4", "#ed7d31", "#a5a5a5"]);
}

#[test]
fn doughnut_rings_leave_the_hole() {
    let doughnut = format!(
        r#"<c:doughnutChart><c:varyColors val="1"/>{}<c:holeSize val="50"/></c:doughnutChart>"#,
        ser(0, "P", &["a", "b"], &["1", "3"], "")
    );
    let svg = draw(&space("", &doughnut));
    let doc = parsed(&svg);
    let slices = of_class(&doc, "slice");
    assert_eq!(slices.len(), 2);
    // Two arcs per slice: the outer one and the inner one.
    assert!(
        slices
            .iter()
            .all(|slice| slice.attribute("d").unwrap().matches('A').count() == 2)
    );
}

#[test]
fn missing_points_break_the_line_and_draw_no_bar() {
    let svg = draw(&single("lineChart", "", &["1", "2", "#N/A", "4", "5"]));
    let doc = parsed(&svg);
    let lines = of_class(&doc, "line");
    assert_eq!(lines.len(), 1);
    assert_eq!(subpaths(lines[0].attribute("d").unwrap()).len(), 2);
    let svg = draw(&single("barChart", "", &["1", "NaN", "inf", "4"]));
    let doc = parsed(&svg);
    assert_eq!(of_class(&doc, "bar").len(), 2);
    // A series whose points are all missing still gives a chart.
    let svg = draw(&single("lineChart", "", &["#N/A", "#N/A"]));
    assert!(of_class(&parsed(&svg), "line").is_empty());
}

#[test]
fn scatter_draws_lines_or_markers_by_its_style() {
    let doc_svg = draw(&scatter("lineMarker"));
    let doc = parsed(&doc_svg);
    assert_eq!(of_class(&doc, "line").len(), 1);
    assert_eq!(of_class(&doc, "marker").len(), 3);
    let doc_svg = draw(&scatter("marker"));
    let doc = parsed(&doc_svg);
    assert!(of_class(&doc, "line").is_empty());
    assert_eq!(of_class(&doc, "marker").len(), 3);
    // The x axis has value ticks too.
    assert!(of_class(&doc, "tick-x").len() >= 2);
}

#[test]
fn secondary_axis_ticks_are_on_the_right() {
    let svg = draw(&combination());
    let doc = parsed(&svg);
    let (left, _, width, _) = plot(&doc);
    let right = of_class(&doc, "tick-secondary");
    assert!(!right.is_empty());
    assert!(right.iter().all(|tick| num(*tick, "x") >= left + width));
    // In the axis's own format.
    assert!(right.iter().all(|tick| text(*tick).ends_with('%')));
    assert_eq!(text(right[0]), "0%");
    // The secondary category axis is deleted: one row of categories.
    assert_eq!(of_class(&doc, "category").len(), 3);
}

#[test]
fn deleted_axes_have_no_labels() {
    let xml = single("barChart", "", &["1", "2"])
        .replace(
            r#"<c:delete val="0"/><c:axPos val="l"/>"#,
            r#"<c:delete val="1"/><c:axPos val="l"/>"#,
        )
        .replace(
            r#"<c:delete val="0"/><c:axPos val="b"/>"#,
            r#"<c:delete val="1"/><c:axPos val="b"/>"#,
        );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    assert!(of_class(&doc, "tick").is_empty());
    assert!(of_class(&doc, "category").is_empty());
}

#[test]
fn crowded_category_labels_are_thinned_or_turned() {
    let values: Vec<String> = (0..216).map(|n| format!("{}", 90 + n % 20)).collect();
    let values: Vec<&str> = values.iter().map(String::as_str).collect();
    let svg = draw(&single("lineChart", "", &values).replace(">c", ">2024-0"));
    let doc = parsed(&svg);
    let labels = of_class(&doc, "category");
    assert!(!labels.is_empty() && labels.len() < 60, "{}", labels.len());
    // Turned labels, or labels far enough apart not to overlap.
    let xs: Vec<f32> = labels.iter().map(|label| num(*label, "x")).collect();
    let turned = labels.iter().all(|label| label.attribute("transform").is_some());
    let spacing = xs.windows(2).map(|pair| pair[1] - pair[0]).fold(f32::MAX, f32::min);
    assert!(turned || spacing > 30.0, "{spacing}");
    assert!(spacing > 8.0, "{spacing}");
}

#[test]
fn legend_follows_the_file() {
    let svg = draw(&columns("clustered", "col"));
    let doc = parsed(&svg);
    let (_, top, _, height) = plot(&doc);
    let items = of_class(&doc, "legend-item");
    assert_eq!(
        items.iter().map(|item| text(*item)).collect::<Vec<_>>(),
        ["North", "South"]
    );
    // The legend is at the bottom (`legendPos="b"`).
    assert!(items.iter().all(|item| num(*item, "y") > top + height));
    let right = draw(&columns("clustered", "col").replace(r#"<c:legendPos val="b"/>"#, r#"<c:legendPos val="r"/>"#));
    let doc = parsed(&right);
    let (left, _, width, _) = plot(&doc);
    assert!(
        of_class(&doc, "legend-item")
            .iter()
            .all(|item| num(*item, "x") > left + width)
    );
    let none = draw(&columns("clustered", "col").replace(r#"<c:legend><c:legendPos val="b"/></c:legend>"#, ""));
    assert!(of_class(&parsed(&none), "legend-item").is_empty());
}

#[test]
fn titles_are_escaped_text() {
    let mut chart = chart(&columns("clustered", "col"));
    chart.title = Some("R&D <2024> \u{1}売上".into());
    let svg = svg(&chart, SIZE).unwrap();
    let doc = parsed(&svg);
    assert_eq!(text(of_class(&doc, "title")[0]), "R&D <2024> 売上");
    assert!(svg.contains("Noto Sans CJK JP"));
}

#[test]
fn size_is_clamped() {
    let chart = chart(&columns("clustered", "col"));
    for size in [(1e9, 1e9), (f32::NAN, -5.0), (0.0, 0.0)] {
        let svg = svg(&chart, size).unwrap();
        let doc = parsed(&svg);
        let width: f32 = doc
            .root_element()
            .attribute("width")
            .unwrap()
            .trim_end_matches("pt")
            .parse()
            .unwrap();
        assert!(
            (super::MIN_SIZE_PT.0..=super::MAX_SIZE_PT.0).contains(&width),
            "{width}"
        );
    }
}

/// A chart of `n_series` series of `n_points` points each, in one group
/// of `kind`.
fn big(kind: ChartKind, n_series: usize, n_points: usize) -> Chart {
    let series = (0..n_series)
        .map(|index| Series {
            index: index as u32,
            name: Some(format!("Series name {index}")),
            categories: Some(Categories::Numbers(Numbers {
                points: (0..n_points).map(|point| Some(point as f64)).collect(),
                format: NumFmt::General,
            })),
            values: Some(Numbers {
                points: (0..n_points)
                    .map(|point| (point % 7 != 3).then(|| ((point * (index + 1)) % 997) as f64))
                    .collect(),
                format: NumFmt::General,
            }),
            color: [10, 20, 30],
            point_colors: if matches!(kind, ChartKind::Pie { .. }) {
                vec![[1, 2, 3]; n_points]
            } else {
                Vec::new()
            },
            markers: true,
            labels: crate::chart::DataLabels::default(),
            line: true,
        })
        .collect();
    Chart {
        title: Some("Big".into()),
        groups: vec![PlotGroup {
            kind,
            three_d: false,
            vary_colors: false,
            axis_ids: Vec::new(),
            series,
        }],
        legend: Some(crate::chart::LegendPosition::Right),
        has_data: true,
        ..Chart::default()
    }
}

#[test]
fn huge_charts_stay_small() {
    let cases = [
        big(
            ChartKind::Line {
                grouping: Grouping::Standard,
            },
            30,
            32_000,
        ),
        big(
            ChartKind::Area {
                grouping: Grouping::Stacked,
            },
            30,
            32_000,
        ),
        big(
            ChartKind::Bar {
                horizontal: false,
                grouping: Grouping::Clustered,
                gap_width_ratio: 1.5,
                overlap_ratio: 0.0,
            },
            30,
            32_000,
        ),
        big(ChartKind::Scatter { smooth: false }, 30, 32_000),
        big(ChartKind::Pie { first_slice_deg: 0.0 }, 1, 32_000),
        big(
            ChartKind::Doughnut {
                first_slice_deg: 0.0,
                hole_ratio: 0.5,
            },
            30,
            32_000,
        ),
        big(ChartKind::Radar { filled: false }, 30, 32_000),
    ];
    for chart in &cases {
        let start = std::time::Instant::now();
        let svg = svg(chart, SIZE).unwrap();
        assert!(
            svg.len() < MAX_SVG_BYTES,
            "{:?}: {} bytes",
            chart.groups[0].kind,
            svg.len()
        );
        assert!(start.elapsed().as_secs_f32() < 2.0, "{:?}", start.elapsed());
        parsed(&svg);
    }
}

#[test]
fn text_categories_label_the_axis() {
    let xml = space(
        "",
        &format!(
            r#"<c:barChart><c:ser><c:idx val="0"/><c:order val="0"/><c:cat>{}</c:cat><c:val>{}</c:val></c:ser>{AXES}</c:barChart>{AXIS_PARTS}"#,
            str_ref(&["東京", "大阪"]),
            num_ref("General", &["1", "2"])
        ),
    );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    let labels: Vec<String> = of_class(&doc, "category").into_iter().map(text).collect();
    assert_eq!(labels, ["東京", "大阪"]);
}

#[test]
fn hidden_tick_labels_keep_their_gridlines() {
    let xml = single("barChart", "", &["1", "2"]).replace(
        r#"<c:numFmt formatCode="General" sourceLinked="1"/>"#,
        r#"<c:numFmt formatCode="General" sourceLinked="1"/><c:tickLblPos val="none"/>"#,
    );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    assert!(of_class(&doc, "tick").is_empty());
    assert!(!of_class(&doc, "grid").is_empty());
}

#[test]
fn the_file_s_label_skip_is_kept() {
    let values: Vec<String> = (1..=12).map(|n| n.to_string()).collect();
    let values: Vec<&str> = values.iter().map(String::as_str).collect();
    let xml = single("lineChart", "", &values).replace(
        r#"<c:axPos val="b"/><c:crossAx val="2"/>"#,
        r#"<c:axPos val="b"/><c:crossAx val="2"/><c:tickLblSkip val="3"/>"#,
    );
    let svg = draw(&xml);
    let labels: Vec<String> = of_class(&parsed(&svg), "category").into_iter().map(text).collect();
    assert_eq!(labels, ["c1", "c4", "c7", "c10"]);
}

#[test]
fn areas_crossing_on_categories_reach_the_edges() {
    let xml = single("areaChart", "", &["1", "3", "2"]).replace(
        r#"<c:crossAx val="1"/></c:valAx>"#,
        r#"<c:crossAx val="1"/><c:crossBetween val="midCat"/></c:valAx>"#,
    );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    let (left, _, width, _) = plot(&doc);
    let data = of_class(&doc, "area")[0]
        .attribute("d")
        .unwrap()
        .trim_end_matches(" Z")
        .to_string();
    let points = subpaths(&data).concat();
    let xs: Vec<f32> = points.iter().map(|point| point.0).collect();
    let (low, high) = xs
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), x| (low.min(*x), high.max(*x)));
    assert!(
        (low - left).abs() < 0.01 && (high - (left + width)).abs() < 0.01,
        "{low} {high} {left} {width}"
    );
}

#[test]
fn a_single_series_with_varied_colours_colours_each_bar() {
    let xml = single("barChart", r#"<c:varyColors val="1"/>"#, &["1", "2", "3"]);
    let svg = draw(&xml);
    let doc = parsed(&svg);
    let fills: Vec<&str> = of_class(&doc, "bar")
        .iter()
        .map(|bar| bar.attribute("fill").unwrap())
        .collect();
    assert_eq!(fills, ["#4472c4", "#ed7d31", "#a5a5a5"]);
    // The legend names the categories, as for a pie.
    let items: Vec<String> = of_class(&doc, "legend-item").into_iter().map(text).collect();
    assert_eq!(items, ["c1", "c2", "c3"]);
}

#[test]
fn radar_rings_follow_the_file_s_unit_and_hidden_labels() {
    let xml = single("radarChart", r#"<c:radarStyle val="marker"/>"#, &["1", "3", "2", "4"]).replace(
        r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling>"#,
        r#"<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/><c:max val="4"/><c:min val="-2"/></c:scaling><c:majorUnit val="1"/><c:tickLblPos val="none"/>"#,
    );
    let svg = draw(&xml);
    let doc = parsed(&svg);
    assert_eq!(of_class(&doc, "grid").len(), 7);
    assert!(of_class(&doc, "tick").is_empty());
    assert_eq!(of_class(&doc, "category").len(), 4);
}

#[test]
fn a_side_legend_of_stacked_columns_lists_the_top_first() {
    let xml = columns("stacked", "col").replace(r#"<c:legendPos val="b"/>"#, r#"<c:legendPos val="r"/>"#);
    let svg = draw(&xml);
    let items: Vec<String> = of_class(&parsed(&svg), "legend-item").into_iter().map(text).collect();
    assert_eq!(items, ["South", "North"]);
}

#[test]
fn a_point_with_its_own_fill_colours_its_bar() {
    let point =
        r#"<c:dPt><c:idx val="1"/><c:spPr><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></c:spPr></c:dPt>"#;
    let plot = format!(
        r#"<c:barChart><c:varyColors val="0"/>{}{}{AXES}</c:barChart>{AXIS_PARTS}"#,
        ser(0, "A", &["a", "b"], &["1", "2"], point),
        ser(1, "B", &["a", "b"], &["3", "4"], "")
    );
    let svg = draw(&space("", &plot));
    let doc = parsed(&svg);
    let fills: Vec<&str> = of_class(&doc, "bar")
        .iter()
        .map(|bar| bar.attribute("fill").unwrap())
        .collect();
    // Category by category: A then B.
    assert_eq!(fills, ["#4472c4", "#ed7d31", "#00ff00", "#ed7d31"]);
}

/// The texts of the data labels of `svg`.
fn data_labels(svg: &str) -> Vec<String> {
    let doc = parsed(svg);
    of_class(&doc, "data-label").into_iter().map(text).collect()
}

#[test]
fn bars_show_their_values_and_pies_their_shares() {
    let shown = r#"<c:dLbls><c:showVal val="1"/><c:showPercent val="0"/></c:dLbls>"#;
    let bars = space(
        "",
        &format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{}{AXES}</c:barChart>{AXIS_PARTS}",
            ser(0, "S", &["a", "b"], &["1200", "300"], shown).replace("General", "#,##0")
        ),
    );
    let svg = draw(&bars);
    assert_eq!(data_labels(&svg), ["1,200", "300"]);
    // Every label is inside the plot area, past or inside the bar's end.
    let doc = parsed(&svg);
    let (_, top, _, height) = plot(&doc);
    for label in of_class(&doc, "data-label") {
        let y = num(label, "y");
        assert!(y >= top && y <= top + height, "{y}");
    }
    let percent = r#"<c:dLbls><c:showVal val="0"/><c:showPercent val="1"/></c:dLbls>"#;
    let doughnut = format!(
        r#"<c:doughnutChart><c:varyColors val="1"/>{}<c:holeSize val="50"/></c:doughnutChart>"#,
        ser(0, "P", &["a", "b", "c"], &["1", "3", "0.001"], percent)
    );
    // The slice too thin for its label shows none.
    assert_eq!(data_labels(&draw(&space("", &doughnut))), ["25%", "75%"]);
    // No labels without dLbls.
    assert!(data_labels(&draw(&columns("clustered", "col"))).is_empty());
}

#[test]
fn stacked_segments_too_short_for_their_label_show_none() {
    let shown = r#"<c:dLbls><c:showVal val="1"/></c:dLbls>"#;
    let plot = format!(
        r#"<c:barChart><c:barDir val="col"/><c:grouping val="stacked"/><c:overlap val="100"/>{}{}{AXES}</c:barChart>{AXIS_PARTS}"#,
        ser(0, "big", &["a"], &["100"], shown),
        ser(1, "small", &["a"], &["0.1"], shown)
    );
    assert_eq!(data_labels(&draw(&space("", &plot))), ["100"]);
}
