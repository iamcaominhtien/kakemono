//! Charts of Office documents: the DrawingML chart part (`c:chartSpace`,
//! the same in a workbook, `xl/charts/chartN.xml`, and in a Word document,
//! `word/charts/chartN.xml`) and the Office 2016 extended chart part
//! (`cx:chartSpace`: waterfall, treemap and the like) read into a
//! [`Chart`], and a chart's data as a table.
//!
//! Only the values the file caches are read (`c:numCache`, `c:strCache`,
//! `c:multiLvlStrCache`, literals): the formulas of the references are not
//! evaluated, so a chart whose part keeps no cache has no data.
//!
//! - `read`: the chart part → [`Chart`].
//! - `chartex`: the extended chart part → [`Chart`], never an error.
//! - `theme`: the colour scheme of a theme part, for the series without a
//!   colour of their own.
//! - `table`: a chart's data as a [`Table`](crate::model::Table).

mod chartex;
mod read;
mod svg;
mod table;
mod theme;

use crate::excel::numfmt::NumFmt;

pub(crate) use read::read;
pub(crate) use svg::svg;
pub(crate) use table::data_table;
pub(crate) use theme::Theme;

/// An RGB colour.
pub(crate) type Rgb = [u8; 3];

/// The limits of the reading of a chart part, so that a hostile or broken
/// file cannot take unbounded time or memory. Past a limit the data is
/// truncated and [`Chart::truncated`] says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChartConfig {
    /// The most bytes of a chart part that are parsed: the whole part is
    /// held as a tree, about ten times its size. A larger part is an
    /// error. The largest chart of the corpus has 41 kB; one series of the
    /// most points below is about 2 MB.
    pub(crate) max_part_bytes: usize,
    /// The most series kept of a chart (over all its plot groups): Excel's
    /// own limit, 255.
    pub(crate) max_series_count: usize,
    /// The most points kept of one series, and of its categories: the
    /// limit of a 2-D chart series in Excel 2007, 32 000.
    pub(crate) max_points_per_series: usize,
    /// The most points kept of a whole chart, over its series, their
    /// categories and category levels: a part can declare the most points
    /// for every series in a few bytes, and each point kept takes some
    /// memory here and a cell of the data table.
    pub(crate) max_points_per_chart: usize,
    /// The most characters kept of one text (title, series name, category
    /// label).
    pub(crate) max_label_chars: usize,
}

impl Default for ChartConfig {
    fn default() -> Self {
        Self {
            max_part_bytes: 16 * 1024 * 1024,
            max_series_count: 255,
            max_points_per_series: 32_000,
            max_points_per_chart: 1_000_000,
            max_label_chars: 1_000,
        }
    }
}

/// A chart, as its part describes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Chart {
    /// The title shown above the chart: its own text, or, for an automatic
    /// title, the name of the only series. `None` when it shows none.
    pub(crate) title: Option<String>,
    /// The plot groups of the plot area, in order: one chart type each
    /// (a bar and line combination has two).
    pub(crate) groups: Vec<PlotGroup>,
    /// The axes of the plot area, in order.
    pub(crate) axes: Vec<Axis>,
    /// Where the legend is; `None` when the chart has no legend.
    pub(crate) legend: Option<LegendPosition>,
    /// Only the visible cells are plotted (`c:plotVisOnly`); the caches
    /// already hold only those.
    pub(crate) plot_visible_only: bool,
    /// The built-in chart style (`c:style`, 1 to 48), when it is given.
    pub(crate) style: Option<u32>,
    /// The workbook counts dates from 1904 (`c:date1904`).
    pub(crate) date1904: bool,
    /// At least one series has cached values: `false` for a chart whose
    /// data is not stored in the file.
    pub(crate) has_data: bool,
    /// A limit of [`ChartConfig`] cut series, points or texts.
    pub(crate) truncated: bool,
}

/// A group of series plotted with one chart type, on one pair of axes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlotGroup {
    /// The chart type and its options.
    pub(crate) kind: ChartKind,
    /// The part names it as a 3-D type (`c:bar3DChart`, `c:pie3DChart`);
    /// its 2-D kind is in `kind`.
    pub(crate) three_d: bool,
    /// Each point has its own colour (`c:varyColors`), as in a pie.
    pub(crate) vary_colors: bool,
    /// The ids of the axes the group is plotted on (`c:axId`), matching
    /// [`Axis::id`].
    pub(crate) axis_ids: Vec<u32>,
    /// The series, in plot order (`c:order`).
    pub(crate) series: Vec<Series>,
}

/// The chart type of a plot group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ChartKind {
    /// Bars (`barDir="bar"`) or columns (`barDir="col"`).
    Bar {
        /// Horizontal bars rather than columns.
        horizontal: bool,
        /// How the series stand to each other.
        grouping: Grouping,
        /// The gap between two categories, as a ratio of a bar's width
        /// (`c:gapWidth`, 150 % by default).
        gap_width_ratio: f32,
        /// How much the bars of one category overlap, as a ratio of a
        /// bar's width, -1 to 1 (`c:overlap`).
        overlap_ratio: f32,
    },
    /// Lines.
    Line {
        /// How the series stand to each other.
        grouping: Grouping,
    },
    /// Areas.
    Area {
        /// How the series stand to each other.
        grouping: Grouping,
    },
    /// A pie.
    Pie {
        /// The angle of the first slice, clockwise from the top, in degrees.
        first_slice_deg: f32,
    },
    /// A doughnut.
    Doughnut {
        /// The angle of the first slice, clockwise from the top, in degrees.
        first_slice_deg: f32,
        /// The hole's diameter as a ratio of the doughnut's.
        hole_ratio: f32,
    },
    /// Points at x, y values.
    Scatter {
        /// The lines between the points are smoothed.
        smooth: bool,
    },
    /// A radar: a spoke per category, a polygon per series.
    Radar {
        /// The polygons are filled (`radarStyle="filled"`).
        filled: bool,
    },
    /// A type that is not drawn (bubble, stock, surface, a pie of
    /// pie, an extended chart): its title and data only. `name` is the
    /// element (`stockChart`) or the extended chart's layout
    /// (`waterfall`).
    NotDrawn {
        /// The name of the type.
        name: String,
    },
}

/// How the series of a group stand to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grouping {
    /// Each series on its own (lines, areas in front of each other).
    Standard,
    /// Side by side (bars).
    Clustered,
    /// Each series on top of the previous ones.
    Stacked,
    /// Stacked, each category scaled to 100 %.
    PercentStacked,
}

/// One series of a plot group.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Series {
    /// The series index (`c:idx`), which picks its automatic colour.
    pub(crate) index: u32,
    /// The name (`c:tx`); `None` when it has none.
    pub(crate) name: Option<String>,
    /// The categories (`c:cat`), or the x values of a scatter series
    /// (`c:xVal`).
    pub(crate) categories: Option<Categories>,
    /// The values (`c:val`), or the y values of a scatter series
    /// (`c:yVal`); `None` when the part caches none.
    pub(crate) values: Option<Numbers>,
    /// The colour of the series: its own fill or line, else the theme's
    /// accent colour of its index.
    pub(crate) color: Rgb,
    /// The colour of each point, when the group varies its colours by
    /// point (a pie: a point's own fill, `c:dPt`, else the accent colour of
    /// its index) or some points have a fill of their own (else the series
    /// colour). Empty when every point takes the series colour.
    pub(crate) point_colors: Vec<Rgb>,
    /// Points are marked (a line or scatter series with markers).
    pub(crate) markers: bool,
    /// The points are joined by a line (a line or scatter series whose
    /// line is not `a:noFill`).
    pub(crate) line: bool,
    /// The labels shown at its points (`c:dLbls`).
    pub(crate) labels: DataLabels,
}

/// The data labels of a series (`c:dLbls` of the series, else of its
/// group): what each point shows beside its mark.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DataLabels {
    /// Each point shows its value (`c:showVal`).
    pub(crate) value: bool,
    /// Each point of a pie or doughnut shows its share of the total
    /// (`c:showPercent`).
    pub(crate) percent: bool,
    /// Where the labels are (`c:dLblPos`); `None` for the chart type's
    /// default.
    pub(crate) position: Option<LabelPosition>,
    /// The number format of the values (`c:numFmt` not linked to the
    /// source); `None` for the series' own.
    pub(crate) format: Option<NumFmt>,
    /// The points whose label says otherwise (`c:dLbl`): their index, and
    /// whether they show their value and their share. Sorted by index.
    pub(crate) points: Vec<(usize, bool, bool)>,
}

impl DataLabels {
    /// Whether point `index` shows its value and its share.
    pub(crate) fn shown(&self, index: usize) -> (bool, bool) {
        match self.points.binary_search_by_key(&index, |(point, _, _)| *point) {
            Ok(found) => (self.points[found].1, self.points[found].2),
            Err(_) => (self.value, self.percent),
        }
    }

    /// Whether any point shows a label.
    pub(crate) fn any(&self) -> bool {
        self.value || self.percent || self.points.iter().any(|(_, value, percent)| *value || *percent)
    }
}

/// Where a data label is, from its mark (`c:dLblPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LabelPosition {
    /// Past the end of a bar, or outside a slice (`outEnd`).
    OutsideEnd,
    /// Inside the end of a bar (`inEnd`).
    InsideEnd,
    /// In the middle of a bar or slice, on a point (`ctr`).
    Center,
    /// Inside the base of a bar (`inBase`).
    InsideBase,
    /// Above a point (`t`).
    Above,
    /// Below a point (`b`).
    Below,
    /// Left of a point (`l`).
    Left,
    /// Right of a point (`r`).
    Right,
    /// Where it fits best (`bestFit`, pies).
    BestFit,
}

/// Categories of a series, or the x values of a scatter series.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Categories {
    /// Text labels, one list per level, the outermost level first (a single
    /// level for most charts); an outer label is given at the first point
    /// of its group only.
    Text(Vec<Vec<Option<String>>>),
    /// Numbers (or dates), with their number format.
    Numbers(Numbers),
}

/// Cached numbers of a series, with their number format.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Numbers {
    /// One per point; `None` for a point the cache leaves out, or one that
    /// is not a finite number.
    pub(crate) points: Vec<Option<f64>>,
    /// The number format of the cache (`c:formatCode`).
    pub(crate) format: NumFmt,
}

/// An axis of the plot area.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Axis {
    /// The axis id, matched by [`PlotGroup::axis_ids`].
    pub(crate) id: u32,
    /// What the axis shows.
    pub(crate) kind: AxisKind,
    /// Which side of the plot area it is on.
    pub(crate) position: AxisPosition,
    /// The axis is not shown (`c:delete`).
    pub(crate) deleted: bool,
    /// The axis runs from its maximum to its minimum (`maxMin`).
    pub(crate) reversed: bool,
    /// The fixed minimum, when the axis has one.
    pub(crate) min: Option<f64>,
    /// The fixed maximum, when the axis has one.
    pub(crate) max: Option<f64>,
    /// The number format of the labels; `None` when they take the format
    /// of the data (`sourceLinked`) or the axis gives none.
    pub(crate) format: Option<NumFmt>,
    /// Major gridlines are drawn across the plot area.
    pub(crate) major_gridlines: bool,
    /// The axis title.
    pub(crate) title: Option<String>,
    /// The id of the axis this one crosses.
    pub(crate) cross_axis: Option<u32>,
    /// The distance between two ticks (`c:majorUnit`), when the file fixes
    /// it.
    pub(crate) major_unit: Option<f64>,
    /// Only every this many category labels is shown (`c:tickLblSkip`),
    /// when the file says so.
    pub(crate) label_skip: Option<usize>,
    /// The tick labels are shown (`c:tickLblPos` is not `none`).
    pub(crate) tick_labels: bool,
    /// The categories cross this value axis between their ticks
    /// (`c:crossBetween="between"`, the default), rather than on them
    /// (`midCat`: an area chart's points on the plot's edges).
    pub(crate) cross_between: bool,
}

/// What an axis shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AxisKind {
    /// Categories (`c:catAx`).
    Category,
    /// Values (`c:valAx`), also both axes of a scatter chart.
    Value,
    /// Dates as categories (`c:dateAx`).
    Date,
    /// The series of a 3-D chart (`c:serAx`).
    Series,
}

/// The side of the plot area an axis is on (`c:axPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AxisPosition {
    /// Below.
    Bottom,
    /// On the left.
    Left,
    /// On the right.
    Right,
    /// Above.
    Top,
}

/// Where the legend is (`c:legendPos`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LegendPosition {
    /// Below the plot area.
    Bottom,
    /// Above the plot area.
    Top,
    /// On the left.
    Left,
    /// On the right (Excel's default).
    Right,
    /// In the top right corner.
    TopRight,
}

impl Chart {
    /// Every series of the chart, group after group.
    pub(crate) fn series(&self) -> impl Iterator<Item = &Series> {
        self.groups.iter().flat_map(|group| &group.series)
    }
}

impl Series {
    /// The name shown for the series: its own, else Excel's `Series N`
    /// (`N` its index from 1).
    pub(crate) fn label(&self) -> String {
        self.name
            .clone()
            .unwrap_or_else(|| format!("Series {}", u64::from(self.index) + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::{Chart, ChartConfig, ChartKind, Theme, data_table, read};
    use crate::word::zip::ZipArchive;
    use std::path::{Path, PathBuf};

    /// The size the corpus charts are drawn at: 6 by 3.5 inches.
    const DRAWN_SIZE_PT: (f32, f32) = (432.0, 252.0);

    /// The workbooks and Word documents under `dir`, sorted.
    fn office_files(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or_default();
                if path.is_dir() {
                    pending.push(path);
                } else if matches!(extension, "xlsx" | "xlsm" | "docx") {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }

    /// One line describing `chart`: its types, series, points per series,
    /// title, whether it has cached data and whether it was truncated.
    fn describe(chart: &Chart) -> String {
        let kinds: Vec<String> = chart
            .groups
            .iter()
            .map(|group| match &group.kind {
                ChartKind::NotDrawn { name } => format!("not-drawn:{name}"),
                kind => format!("{kind:?}{}", if group.three_d { "(3-D)" } else { "" }),
            })
            .collect();
        let points: Vec<usize> = chart
            .series()
            .map(|series| series.values.as_ref().map_or(0, |values| values.points.len()))
            .collect();
        let table = data_table(chart).map(|table| format!("{}x{}", table.rows.len(), table.n_cols));
        let heads: Vec<String> = chart
            .series()
            .map(|series| {
                let first: Vec<Option<f64>> = series
                    .values
                    .iter()
                    .flat_map(|values| values.points.iter().take(3).copied())
                    .collect();
                let [red, green, blue] = series.color;
                format!("{:?} #{red:02X}{green:02X}{blue:02X} {first:?}", series.label())
            })
            .collect();
        format!(
            "types={} series={} points={points:?} title={:?} cached={} truncated={} table={}\n    {}",
            kinds.join("+"),
            points.len(),
            chart.title,
            chart.has_data,
            chart.truncated,
            table.unwrap_or_else(|| "none".into()),
            heads.join("\n    ")
        )
    }

    /// `chart` drawn as SVG without a panic; with `KAKEMONO_CHART_PNG_DIR`,
    /// written there as `name.svg` and `name.png` (drawn by resvg with the
    /// system's fonts, at 2 pixels per point).
    fn draw(chart: &Chart, name: &str) -> bool {
        let Ok(svg) = std::panic::catch_unwind(|| super::svg(chart, DRAWN_SIZE_PT)) else {
            println!("    svg: PANIC");
            return false;
        };
        let Some(svg) = svg else {
            println!("    svg: not drawn");
            return true;
        };
        println!("    svg: {} bytes", svg.len());
        if let Ok(dir) = std::env::var("KAKEMONO_CHART_PNG_DIR") {
            let dir = Path::new(&dir);
            std::fs::write(dir.join(format!("{name}.svg")), &svg).unwrap();
            let mut options = resvg::usvg::Options::default();
            options.fontdb_mut().load_system_fonts();
            let tree = resvg::usvg::Tree::from_str(&svg, &options).unwrap();
            let size = tree.size().to_int_size();
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width() * 2, size.height() * 2).unwrap();
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_scale(2.0, 2.0),
                &mut pixmap.as_mut(),
            );
            pixmap.save_png(dir.join(format!("{name}.png"))).unwrap();
        }
        true
    }

    /// Every chart part of the files under `KAKEMONO_CHART_CORPUS` reads
    /// and draws without a panic; one line per chart is printed.
    #[test]
    #[ignore = "reads the files under KAKEMONO_CHART_CORPUS"]
    fn corpus_charts_read_without_panics() {
        let dir = std::env::var("KAKEMONO_CHART_CORPUS").expect("KAKEMONO_CHART_CORPUS");
        let (mut charts, mut panics) = (0, 0);
        for path in office_files(Path::new(&dir)) {
            let data = std::fs::read(&path).unwrap();
            let Ok(zip) = ZipArchive::new(&data) else { continue };
            let theme_part = ["xl/theme/theme1.xml", "word/theme/theme1.xml"]
                .into_iter()
                .find_map(|name| zip.read(name).ok().flatten());
            let theme = Theme::read(theme_part.as_deref());
            let mut parts: Vec<String> = zip
                .names()
                .into_iter()
                .filter(|name| {
                    let file = name.rsplit('/').next().unwrap_or_default();
                    name.contains("/charts/") && file.starts_with("chart") && file.ends_with(".xml")
                })
                .collect();
            parts.sort();
            for part in parts {
                let Some(xml) = zip.read(&part).ok().flatten() else {
                    continue;
                };
                charts += 1;
                let result = std::panic::catch_unwind(|| read(&xml, &theme, &ChartConfig::default()));
                let file = path.file_name().unwrap().to_string_lossy();
                match result {
                    Ok(Ok(chart)) => {
                        println!("{file} {part}: {}", describe(&chart));
                        let name = format!(
                            "{}-{}",
                            file.trim_end_matches(".xlsx"),
                            part.rsplit('/').next().unwrap_or_default()
                        );
                        panics += usize::from(!draw(&chart, &name.replace(".xml", "")));
                    }
                    Ok(Err(error)) => println!("{file} {part}: error {error:#}"),
                    Err(_) => {
                        panics += 1;
                        println!("{file} {part}: PANIC");
                    }
                }
            }
        }
        println!("{charts} chart parts, {panics} panics");
        assert_eq!(panics, 0);
    }
}
