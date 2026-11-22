//! A chart made ready for a document, the same way in a workbook and in a
//! Word document: its picture (the SVG drawing), its caption (its title,
//! else "Chart N"), and its data as a table, or a note when the file
//! keeps no data.

use super::{Chart, ChartConfig, Theme, data_table, drawn_size, read, svg};
use crate::common::geom::Rect;
use crate::model::{Block, BlockKind, FloatKind, ImageSpec, Span, Table};

/// The note of a chart whose part keeps no data.
const NO_DATA_NOTE: &str = "The data of this chart is not stored in the file.";
/// The note of a chart past the most drawn of a document.
const TOO_MANY_NOTE: &str = "This chart is not shown: the document has too many charts.";

/// A chart read and drawn, before it takes its place in a document.
#[derive(Debug, Clone, Default)]
pub(crate) struct Placed {
    /// The chart's title, when it shows one.
    pub(crate) title: Option<String>,
    /// The chart drawn as SVG; `None` when it is not drawn (a kind not
    /// drawn, no data).
    pub(crate) svg: Option<String>,
    /// The size it is drawn at, in points: the size asked for, clamped.
    pub(crate) size_pt: (f32, f32),
    /// The chart's data; `None` when it has none.
    pub(crate) table: Option<Table>,
    /// A sentence to show in place of the data: why there is none.
    pub(crate) note: Option<&'static str>,
}

/// The charts of one document, counted against
/// [`ChartConfig::max_charts`], with the theme their series take their
/// automatic colours from.
#[derive(Debug)]
pub(crate) struct Charts {
    /// The theme of the document.
    theme: Theme,
    /// The limits.
    config: ChartConfig,
    /// Charts read so far.
    count: usize,
}

impl Charts {
    /// No chart read yet, in a document of the theme part `theme` (none
    /// for Office's default).
    pub(crate) fn new(theme: Option<&[u8]>) -> Self {
        Self {
            theme: Theme::read(theme),
            config: ChartConfig::default(),
            count: 0,
        }
    }

    /// The chart part `xml` read and drawn at `size_pt` (width, height, in
    /// points); `None` when it is not a chart part, or a damaged one. Past
    /// the most charts of a document, a chart is not read: it gives only a
    /// note.
    pub(crate) fn place(&mut self, xml: &[u8], size_pt: (f32, f32)) -> Option<Placed> {
        self.count += 1;
        if self.count > self.config.max_charts {
            return Some(Placed {
                note: Some(TOO_MANY_NOTE),
                ..Placed::default()
            });
        }
        let chart = read(xml, &self.theme, &self.config).ok()?;
        Some(placed(&chart, size_pt))
    }
}

/// `chart` drawn at `size_pt`, with its data.
fn placed(chart: &Chart, size_pt: (f32, f32)) -> Placed {
    Placed {
        title: chart.title.clone(),
        svg: svg(chart, size_pt),
        size_pt: drawn_size(size_pt),
        table: data_table(chart),
        note: (!chart.has_data).then_some(NO_DATA_NOTE),
    }
}

/// The figure of a chart: its picture `image` (none for a chart not
/// drawn) with the caption `caption`.
pub(crate) fn figure_block(image: Option<ImageSpec>, caption: &str) -> Block {
    let mut block = Block::new(
        0,
        BlockKind::Figure {
            kind: FloatKind::Figure,
            image,
            label: None,
        },
    );
    block.spans = vec![Span::plain(caption)];
    block
}

/// The data table of a chart, with the caption `caption` above it, as a
/// table of a sheet has its caption.
pub(crate) fn table_block(table: Table, caption: &str) -> Block {
    let mut block = Block::new(
        0,
        BlockKind::Figure {
            kind: FloatKind::Table,
            image: None,
            label: None,
        },
    );
    block.spans = vec![Span::plain(caption)];
    block.caption_above = true;
    block.table = Some(table);
    block
}

/// The blocks of the chart `placed` whose picture, when it was drawn and
/// stored, is the document's picture `picture`, shown at its size: the
/// figure, then its data table (captioned "… (data)"), or the note saying why there is none; a
/// chart not drawn gives its data table alone (with the chart's caption),
/// else its caption and the note. `caption` is its title, else what the
/// caller names it ("Chart 2").
pub(crate) fn blocks(placed: Placed, picture: Option<usize>, caption: &str) -> Vec<Block> {
    let caption = placed.title.as_deref().unwrap_or(caption);
    let paragraph = |text: &str| {
        let mut block = Block::new(0, BlockKind::Paragraph { in_abstract: false });
        block.spans = vec![Span::plain(text)];
        block
    };
    let mut blocks = Vec::new();
    match picture {
        Some(index) => blocks.push(figure_block(Some(image_spec(index, placed.size_pt)), caption)),
        None if placed.table.is_none() => blocks.push(paragraph(caption)),
        None => {}
    }
    match placed.table {
        // Under its figure, the table says it holds the chart's data rather
        // than repeating the figure's caption word for word.
        Some(table) if picture.is_some() => blocks.push(table_block(table, &format!("{caption} (data)"))),
        Some(table) => blocks.push(table_block(table, caption)),
        None => blocks.extend(placed.note.map(paragraph)),
    }
    blocks
}

/// The image of the document's picture `index`, shown `size_pt` large.
fn image_spec(index: usize, (width, height): (f32, f32)) -> ImageSpec {
    ImageSpec {
        page: 0,
        rect: Rect {
            x0: 0.0,
            y0: 0.0,
            x1: width,
            y1: height,
        },
        transparent: false,
        embedded: Some(index),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::read::tests::{AXES, AXIS_PARTS, ser, space};

    /// A column chart titled `title` (none when empty), its values cached
    /// or not.
    fn columns(title: &str, cached: bool) -> String {
        let title = if title.is_empty() {
            r#"<c:autoTitleDeleted val="1"/>"#.to_string()
        } else {
            format!(r#"<c:title><c:tx><c:rich><a:p><a:r><a:t>{title}</a:t></a:r></a:p></c:rich></c:tx></c:title>"#)
        };
        let mut series = ser(0, "S", &["a", "b"], &["1", "2"], "");
        if !cached {
            series = series.replace(
                &series[series.find("<c:val>").unwrap()..series.find("</c:val>").unwrap() + 8],
                "",
            );
        }
        space(
            &title,
            &format!(r#"<c:barChart><c:barDir val="col"/>{series}{AXES}</c:barChart>{AXIS_PARTS}"#),
        )
    }

    /// The kinds and texts of `blocks`.
    fn read(blocks: &[Block]) -> Vec<(String, String)> {
        blocks
            .iter()
            .map(|block| {
                let kind = match &block.kind {
                    BlockKind::Figure { image: Some(_), .. } => "figure",
                    BlockKind::Figure { .. } if block.table.is_some() => "table",
                    _ => "paragraph",
                };
                (kind.to_string(), block.text())
            })
            .collect()
    }

    #[test]
    fn a_chart_is_a_figure_then_its_data() {
        let mut charts = Charts::new(None);
        let placed = charts.place(columns("Sales", true).as_bytes(), (300.0, 200.0)).unwrap();
        assert!(placed.svg.is_some());
        let blocks = blocks(placed, Some(0), "Chart 1");
        assert_eq!(
            read(&blocks),
            [
                ("figure".into(), "Sales".into()),
                ("table".into(), "Sales (data)".into())
            ]
        );
        // Without a title, the caller's name.
        let placed = charts.place(columns("", true).as_bytes(), (300.0, 200.0)).unwrap();
        assert_eq!(read(&super::blocks(placed, Some(1), "Chart 2"))[0].1, "Chart 2");
    }

    #[test]
    fn a_chart_without_data_says_so() {
        let mut charts = Charts::new(None);
        let placed = charts
            .place(columns("Empty", false).as_bytes(), (300.0, 200.0))
            .unwrap();
        assert!(placed.svg.is_none());
        assert_eq!(
            read(&blocks(placed, None, "Chart 1")),
            [
                ("paragraph".into(), "Empty".into()),
                ("paragraph".into(), NO_DATA_NOTE.into())
            ]
        );
        assert!(charts.place(b"<damaged", (1.0, 1.0)).is_none());
    }

    #[test]
    fn charts_past_the_most_of_a_document_are_not_read() {
        let mut charts = Charts::new(None);
        charts.config.max_charts = 1;
        assert!(
            charts
                .place(columns("One", true).as_bytes(), (300.0, 200.0))
                .unwrap()
                .svg
                .is_some()
        );
        let past = charts.place(columns("Two", true).as_bytes(), (300.0, 200.0)).unwrap();
        assert_eq!(past.note, Some(TOO_MANY_NOTE));
        assert!(past.svg.is_none() && past.table.is_none());
    }
}
