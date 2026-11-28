//! A chart's data as a table, the form in which readers and models take
//! its numbers: one row per category, a column per series, the series
//! names as the header row; for a scatter chart, the x and y values of
//! each series side by side.

use super::{AxisKind, AxisPosition, Categories, Chart, ChartKind, Numbers, Series};
use crate::excel::numfmt;
use crate::model::{Span, Table, TableCell};

/// The data of `chart` as a table; `None` when it has no cached data.
///
/// Rows are the categories (each level of multi-level categories in a
/// column of its own, the outermost first; numbers 1, 2, ... when no
/// series has categories), a column per series, the series names as the
/// header row. A chart whose groups are all scatter gives, per series, its
/// x and y values side by side under its name. A missing point has no
/// cell. Numbers are written in their cache's format, dates ISO.
pub(crate) fn data_table(chart: &Chart) -> Option<Table> {
    if !chart.has_data {
        return None;
    }
    let series: Vec<&Series> = chart.series().collect();
    let scatter = !chart.groups.is_empty()
        && chart
            .groups
            .iter()
            .all(|group| matches!(group.kind, ChartKind::Scatter { .. }));
    Some(if scatter {
        scatter_table(chart, &series)
    } else {
        category_table(chart, &series)
    })
}

/// The table of a chart by categories: a row per category, a column per
/// series.
fn category_table(chart: &Chart, series: &[&Series]) -> Table {
    let categories = series.iter().find_map(|series| series.categories.as_ref());
    let n_levels = match categories {
        Some(Categories::Text(levels)) => levels.len().max(1),
        _ => 1,
    };
    let n_rows = series
        .iter()
        .map(|series| values_len(series).max(categories_len(series.categories.as_ref())))
        .max()
        .unwrap_or(0);
    let n_cols = n_levels + series.len();
    let axis_title = chart
        .axes
        .iter()
        .find(|axis| matches!(axis.kind, AxisKind::Category | AxisKind::Date))
        .and_then(|axis| axis.title.clone())
        .unwrap_or_default();
    let mut header = vec![cell(0, 1, axis_title)];
    header.extend((1..n_levels).map(|col| cell(col, 1, String::new())));
    header.extend(
        series
            .iter()
            .enumerate()
            .map(|(at, series)| cell(n_levels + at, 1, series.label())),
    );
    let mut rows = vec![header];
    for index in 0..n_rows {
        let mut row = Vec::with_capacity(n_cols);
        match categories {
            Some(Categories::Text(levels)) => row.extend(levels.iter().enumerate().filter_map(|(col, level)| {
                let label = level.get(index)?.clone()?;
                Some(cell(col, 1, label))
            })),
            Some(Categories::Numbers(numbers)) => row.extend(number_cell(0, numbers, index, chart.date1904)),
            None => row.push(cell(0, 1, (index + 1).to_string())),
        }
        for (at, series) in series.iter().enumerate() {
            if let Some(values) = &series.values {
                row.extend(number_cell(n_levels + at, values, index, chart.date1904));
            }
        }
        rows.push(row);
    }
    let mut col_align = vec!["left"; n_levels];
    col_align.resize(n_cols, "right");
    Table::ruled(rows, n_cols, 1, col_align)
}

/// The table of a scatter chart: two columns per series, its x and y
/// values, under its name and the titles of the axes.
fn scatter_table(chart: &Chart, series: &[&Series]) -> Table {
    let axis_title = |horizontal: bool, default: &str| {
        chart
            .axes
            .iter()
            .filter(|axis| !axis.deleted || axis.title.is_some())
            .find(|axis| matches!(axis.position, AxisPosition::Bottom | AxisPosition::Top) == horizontal)
            .and_then(|axis| axis.title.clone())
            .unwrap_or_else(|| default.to_string())
    };
    let (x_title, y_title) = (axis_title(true, "x"), axis_title(false, "y"));
    let n_cols = 2 * series.len();
    let names = series
        .iter()
        .enumerate()
        .map(|(at, series)| cell(2 * at, 2, series.label()))
        .collect();
    let titles = (0..series.len())
        .flat_map(|at| [cell(2 * at, 1, x_title.clone()), cell(2 * at + 1, 1, y_title.clone())])
        .collect();
    let mut rows = vec![names, titles];
    let n_rows = series
        .iter()
        .map(|series| values_len(series).max(categories_len(series.categories.as_ref())))
        .max()
        .unwrap_or(0);
    for index in 0..n_rows {
        let mut row = Vec::new();
        for (at, series) in series.iter().enumerate() {
            let col = 2 * at;
            match &series.categories {
                Some(Categories::Numbers(numbers)) => row.extend(number_cell(col, numbers, index, chart.date1904)),
                Some(Categories::Text(levels)) => {
                    let label = levels.last().and_then(|level| level.get(index)?.clone());
                    row.extend(label.map(|label| cell(col, 1, label)));
                }
                // No x values: Excel plots the points at 1, 2, ...
                None if index < values_len(series) => row.push(cell(col, 1, (index + 1).to_string())),
                None => {}
            }
            if let Some(values) = &series.values {
                row.extend(number_cell(col + 1, values, index, chart.date1904));
            }
        }
        rows.push(row);
    }
    Table::ruled(rows, n_cols, 2, vec!["right"; n_cols])
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

/// The cell at `col` of the point `index` of `numbers`, in their format;
/// `None` when the point is missing.
fn number_cell(col: usize, numbers: &Numbers, index: usize, date1904: bool) -> Option<TableCell> {
    let value = (*numbers.points.get(index)?)?;
    Some(cell(col, 1, numfmt::format(value, &numbers.format, date1904)))
}

/// A cell at `col`, over `colspan` columns, of the plain text `text`.
fn cell(col: usize, colspan: usize, text: String) -> TableCell {
    TableCell {
        col,
        colspan,
        rowspan: 1,
        spans: if text.is_empty() {
            Vec::new()
        } else {
            vec![Span::plain(text)]
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::chart::data_table;
    use crate::chart::read::tests::{AXES, chart, num_ref, ser, space, str_ref};
    use crate::model::Table;

    /// The text at each position of `table`, row by row: empty where no
    /// cell starts.
    fn texts(table: &Table) -> Vec<Vec<String>> {
        table
            .rows
            .iter()
            .map(|row| {
                let mut out = vec![String::new(); table.n_cols];
                for cell in row {
                    out[cell.col] = cell.spans.iter().map(|span| span.text.as_str()).collect();
                }
                out
            })
            .collect()
    }

    /// `rows` of `&str` as owned strings.
    fn owned(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|text| (*text).to_string()).collect())
            .collect()
    }

    #[test]
    fn categories_as_rows_and_series_as_columns() {
        let axes = r#"<c:catAx><c:axId val="1"/><c:axPos val="b"/><c:title><c:tx><c:rich><a:p><a:r><a:t>Quarter</a:t></a:r></a:p></c:rich></c:tx></c:title><c:crossAx val="2"/></c:catAx>"#;
        let plot = format!(
            "<c:barChart>{}{}{AXES}</c:barChart>{axes}",
            ser(0, "North", &["Q1", "Q2", "Q3"], &["1", "2.5", "1234.5"], ""),
            ser(1, "South", &["Q1", "Q2"], &["4", "5"], "")
        );
        let table = data_table(&chart(&space("", &plot))).unwrap();
        assert_eq!(table.n_cols, 3);
        assert_eq!(table.header_rows, 1);
        assert_eq!(table.col_align, vec!["left", "right", "right"]);
        assert_eq!(
            texts(&table),
            owned(&[
                &["Quarter", "North", "South"],
                &["Q1", "1", "4"],
                &["Q2", "2.5", "5"],
                &["Q3", "1234.5", ""],
            ])
        );
        // A missing point has no cell.
        assert_eq!(table.rows[3].len(), 2);
        assert_eq!(table.rule_above[0][0], crate::model::HEAVY_RULE);
    }

    #[test]
    fn values_and_date_categories_take_their_cache_formats() {
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:v>Rate</c:v></c:tx><c:cat>{}</c:cat><c:val>{}</c:val></c:ser>"#,
            num_ref("m/d/yyyy", &["45292", "45323"]),
            num_ref("0.0%", &["0.25", "0.5"])
        );
        let table = data_table(&chart(&space(
            "",
            &format!("<c:lineChart>{series}{AXES}</c:lineChart>"),
        )))
        .unwrap();
        assert_eq!(
            texts(&table),
            owned(&[&["", "Rate"], &["2024-01-01", "25.0%"], &["2024-02-01", "50.0%"]])
        );
    }

    #[test]
    fn dates_of_a_1904_workbook() {
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:cat>{}</c:cat><c:val>{}</c:val></c:ser>"#,
            num_ref("yyyy-mm-dd", &["0"]),
            num_ref("General", &["1"])
        );
        let xml = space("", &format!("<c:lineChart>{series}</c:lineChart>"))
            .replace("<c:chart>", r#"<c:date1904 val="1"/><c:chart>"#);
        let table = data_table(&chart(&xml)).unwrap();
        assert_eq!(texts(&table)[1][0], "1904-01-01");
    }

    #[test]
    fn multi_level_categories_take_a_column_each() {
        let cat = r#"<c:cat><c:multiLvlStrRef><c:multiLvlStrCache><c:ptCount val="3"/><c:lvl><c:pt idx="0"><c:v>Jan</c:v></c:pt><c:pt idx="1"><c:v>Feb</c:v></c:pt><c:pt idx="2"><c:v>Jan</c:v></c:pt></c:lvl><c:lvl><c:pt idx="0"><c:v>2023</c:v></c:pt><c:pt idx="2"><c:v>2024</c:v></c:pt></c:lvl></c:multiLvlStrCache></c:multiLvlStrRef></c:cat>"#;
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/>{cat}<c:val>{}</c:val></c:ser>"#,
            num_ref("General", &["1", "2", "3"])
        );
        let table = data_table(&chart(&space("", &format!("<c:barChart>{series}</c:barChart>")))).unwrap();
        assert_eq!(table.n_cols, 3);
        assert_eq!(
            texts(&table),
            owned(&[
                &["", "", "Series 1"],
                &["2023", "Jan", "1"],
                &["", "Feb", "2"],
                &["2024", "Jan", "3"]
            ])
        );
    }

    #[test]
    fn scatter_gives_x_then_y_per_series() {
        let one = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:v>A</c:v></c:tx><c:xVal>{}</c:xVal><c:yVal>{}</c:yVal></c:ser>"#,
            num_ref("0.0", &["1", "2"]),
            num_ref("General", &["10", "20"])
        );
        let two = format!(
            r#"<c:ser><c:idx val="1"/><c:order val="1"/><c:tx><c:v>B</c:v></c:tx><c:xVal>{}</c:xVal><c:yVal>{}</c:yVal></c:ser>"#,
            str_ref(&["p", "q", "r"]),
            num_ref("General", &["5", "6", "7"])
        );
        let table = data_table(&chart(&space(
            "",
            &format!("<c:scatterChart>{one}{two}</c:scatterChart>"),
        )))
        .unwrap();
        assert_eq!(table.n_cols, 4);
        assert_eq!(table.header_rows, 2);
        let rows = texts(&table);
        assert_eq!(rows[0], owned(&[&["A", "", "B", ""]])[0]);
        assert_eq!(table.rows[0][0].colspan, 2);
        assert_eq!(table.rows[0][1].col, 2);
        assert_eq!(rows[1], owned(&[&["x", "y", "x", "y"]])[0]);
        assert_eq!(rows[2], owned(&[&["1.0", "10", "p", "5"]])[0]);
        assert_eq!(rows[4], owned(&[&["", "", "r", "7"]])[0]);
    }

    #[test]
    fn series_without_categories_are_numbered() {
        let series = format!(
            r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:val>{}</c:val></c:ser>"#,
            num_ref("General", &["7", "8"])
        );
        let table = data_table(&chart(&space("", &format!("<c:barChart>{series}</c:barChart>")))).unwrap();
        assert_eq!(texts(&table), owned(&[&["", "Series 1"], &["1", "7"], &["2", "8"]]));
    }

    #[test]
    fn chart_without_cached_data_has_no_table() {
        let series = r#"<c:ser><c:idx val="0"/><c:order val="0"/><c:val><c:numRef><c:f>S!B2:B5</c:f></c:numRef></c:val></c:ser>"#;
        assert!(data_table(&chart(&space("", &format!("<c:barChart>{series}</c:barChart>")))).is_none());
    }
}
