//! Charts of Office documents: the DrawingML chart part (`c:chartSpace`,
//! the same in a workbook, `xl/charts/chartN.xml`, and in a Word document,
//! `word/charts/chartN.xml`) and the Office 2016 extended chart part
//! (`cx:chartSpace`: waterfall, treemap and the like) read into a
//! [`Chart`].
//!
//! Only the values the file caches are read (`c:numCache`, `c:strCache`,
//! `c:multiLvlStrCache`, literals): the formulas of the references are not
//! evaluated, so a chart whose part keeps no cache has no data.
//!
//! - `read`: the chart part → [`Chart`].
//! - `chartex`: the extended chart part → [`Chart`], never an error.
//! - `theme`: the colour scheme of a theme part, for the series without a
//!   colour of their own.

mod chartex;
mod read;
mod theme;

use crate::excel::numfmt::NumFmt;

pub(crate) use read::read;
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
    /// A type that is not drawn (radar, bubble, stock, surface, a pie of
    /// pie, an extended chart): its title and data only. `name` is the
    /// element (`radarChart`) or the extended chart's layout
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
    /// point (a pie): a point's own fill (`c:dPt`), else the accent colour
    /// of its index. Empty otherwise.
    pub(crate) point_colors: Vec<Rgb>,
    /// Points are marked (a line or scatter series with markers).
    pub(crate) markers: bool,
    /// The points are joined by a line (a line or scatter series whose
    /// line is not `a:noFill`).
    pub(crate) line: bool,
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
