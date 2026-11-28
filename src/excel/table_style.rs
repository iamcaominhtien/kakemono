//! The look Excel's built-in table styles (`tableStyleInfo`, Light 1 to
//! 21 and Medium 1 to 28) give the cells of an Excel Table: a header row
//! in the style's colour, banded rows, lines. The style is under the
//! cells' own formats: a cell's own fill, borders and font colour win.
//! The dark styles and a workbook's own table styles are left out.

use super::grid::Merge;
use super::styles::{Color, Dxf, Edge, Line};
use crate::word::docx::parse;
use roxmltree::Node;

/// The tint of the light fill of banded rows and of a medium style's
/// body.
const BAND_TINT: f32 = 0.8;
/// The tint of the darker band of the medium styles 8 to 14 and 22 to 28.
const DARK_BAND_TINT: f32 = 0.6;
/// The tint of a black style's bands (Light 1, Medium 1...): a grey.
const GREY_BAND_TINT: f32 = 0.85;
/// The tint of the lines of the medium styles 1 to 7 and 22 to 28.
const LINE_TINT: f32 = 0.4;
/// The shade of a light style's text (Light 2 to 7): its colour darkened.
const TEXT_SHADE: f32 = -0.25;
/// The number of styles of each family (one per colour: black and the six
/// accents).
const FAMILY: u32 = 7;
/// The theme index of dark 1 (black) and of the first accent.
const DARK_1: u8 = 1;
/// The theme index of light 1 (white).
const LIGHT_1: u8 = 0;
/// The theme index of accent 1.
const ACCENT_1: u8 = 4;

/// An Excel Table's range, rows and built-in style.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableStyle {
    /// Its range.
    pub(crate) range: Merge,
    /// The header rows at its top.
    header_rows: u32,
    /// The total rows at its foot.
    total_rows: u32,
    /// Its style: the family and the number in it.
    kind: Kind,
    /// Its colour: black (0) or an accent (1 to 6).
    color: u32,
    /// Its rows are banded (`showRowStripes`).
    row_stripes: bool,
    /// Its columns are banded (`showColumnStripes`).
    col_stripes: bool,
    /// Its first column stands out (`showFirstColumn`).
    first_col: bool,
    /// Its last column stands out (`showLastColumn`).
    last_col: bool,
}

/// The families of built-in table styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Light 1 to 7: lines above and below, a line under the header.
    LightLines,
    /// Light 8 to 14: a header in the colour, the table outlined.
    LightHeader,
    /// Light 15 to 21: every cell outlined.
    LightGrid,
    /// Medium 1 to 7 (and 15 to 21): a header in the colour, light lines.
    MediumLines,
    /// Medium 8 to 14: a body in a light tint, white lines.
    MediumBody,
    /// Medium 22 to 28: a body in a light tint, light lines, no header
    /// fill.
    MediumGrid,
}

/// The table style of the table part `xml`; `None` when it has none that
/// is drawn.
pub(crate) fn read(xml: &str, header_rows: u32) -> Option<TableStyle> {
    let doc = parse(xml).ok()?;
    let table = doc.root_element();
    let info = table
        .children()
        .find(|node| node.tag_name().name() == "tableStyleInfo")?;
    let (kind, color) = kind(info.attribute("name")?)?;
    let on = |node: Node<'_, '_>, name| matches!(node.attribute(name), Some("1" | "true"));
    let count = |name| table.attribute(name).and_then(|value| value.parse::<u32>().ok());
    Some(TableStyle {
        range: table.attribute("ref").and_then(super::sheet::range)?,
        header_rows,
        total_rows: count("totalsRowCount").unwrap_or(0),
        kind,
        color,
        row_stripes: on(info, "showRowStripes"),
        col_stripes: on(info, "showColumnStripes"),
        first_col: on(info, "showFirstColumn"),
        last_col: on(info, "showLastColumn"),
    })
}

/// The family and colour of the built-in style `name`
/// (`TableStyleMedium2`); `None` for a dark style or another name.
fn kind(name: &str) -> Option<(Kind, u32)> {
    let (family, number) = if let Some(number) = name.strip_prefix("TableStyleLight") {
        ("light", number)
    } else {
        ("medium", name.strip_prefix("TableStyleMedium")?)
    };
    let number = number.parse::<u32>().ok().filter(|number| *number >= 1)?;
    let (group, color) = ((number - 1) / FAMILY, (number - 1) % FAMILY);
    let kind = match (family, group) {
        ("light", 0) => Kind::LightLines,
        ("light", 1) => Kind::LightHeader,
        ("light", 2) => Kind::LightGrid,
        ("medium", 0 | 2) => Kind::MediumLines,
        ("medium", 1) => Kind::MediumBody,
        ("medium", 3) => Kind::MediumGrid,
        _ => return None,
    };
    Some((kind, color))
}

impl TableStyle {
    /// The theme colour of the style, `tint`ed.
    fn paint(&self, tint: f32) -> Color {
        let index = if self.color == 0 {
            DARK_1
        } else {
            ACCENT_1 + u8::try_from(self.color - 1).unwrap_or(0)
        };
        Color::Theme(index, tint)
    }

    /// The tint of its light bands.
    fn band_tint(&self) -> f32 {
        if self.color == 0 { GREY_BAND_TINT } else { BAND_TINT }
    }

    /// The look the style gives the cell at `at`, under its own; `None`
    /// outside the table.
    pub(crate) fn cell(&self, (row, col): (u32, u32)) -> Option<Dxf> {
        let range = self.range;
        if !(range.top..=range.bottom).contains(&row) || !(range.left..=range.right).contains(&col) {
            return None;
        }
        let header = row < range.top + self.header_rows;
        let total = self.total_rows > 0 && row + self.total_rows > range.bottom;
        let body_row = row - range.top - self.header_rows.min(row - range.top);
        let banded = !header && !total && self.row_stripes && body_row.is_multiple_of(2);
        let col_banded = !header && !total && self.col_stripes && (col - range.left).is_multiple_of(2);
        let edges = Edges {
            top: row == range.top,
            bottom: row == range.bottom,
            left: col == range.left,
            right: col == range.right,
        };
        let mut dxf = self.whole(edges);
        if col_banded || banded {
            self.band(&mut dxf);
        }
        if (self.first_col && col == range.left) || (self.last_col && col == range.right) {
            dxf.bold = Some(true);
        }
        if header {
            self.header(&mut dxf);
        }
        if total {
            dxf.bold = Some(true);
            dxf.borders[2] = Some(edge(Line::Double, self.paint(0.0)));
        }
        Some(dxf)
    }

    /// What the whole table has, for a cell at `edges`.
    fn whole(&self, edges: Edges) -> Dxf {
        let mut dxf = Dxf::default();
        let color = self.paint(0.0);
        // Lines: left, right, top, bottom; each on the table's edge, or
        // between cells too.
        let (outline, inside_h, inside_v) = match self.kind {
            Kind::LightLines => {
                if self.color != 0 {
                    dxf.color = Some(self.paint(TEXT_SHADE));
                }
                (Some(([false, false, true, true], color)), None, None)
            }
            Kind::LightHeader => (Some(([true; 4], color)), None, None),
            Kind::LightGrid => (Some(([true; 4], color)), Some(color), Some(color)),
            Kind::MediumLines => {
                let line = self.paint(LINE_TINT);
                (Some(([true; 4], line)), Some(line), None)
            }
            Kind::MediumBody => {
                let white = Color::Theme(LIGHT_1, 0.0);
                dxf.fill = Some(self.paint(self.band_tint()));
                (Some(([true; 4], white)), Some(white), Some(white))
            }
            Kind::MediumGrid => {
                let line = self.paint(LINE_TINT);
                dxf.fill = Some(self.paint(self.band_tint()));
                (Some(([true; 4], line)), Some(line), Some(line))
            }
        };
        let at_edge = [edges.left, edges.right, edges.top, edges.bottom];
        for side in 0..4 {
            let inside = if side < 2 { inside_v } else { inside_h };
            let color = match outline {
                Some((sides, color)) if at_edge[side] && sides[side] => Some(color),
                _ if !at_edge[side] => inside,
                _ => None,
            };
            dxf.borders[side] = color.map(|color| edge(Line::Solid, color));
        }
        dxf
    }

    /// Sets the fill (or lines) of a banded row or column over `dxf`.
    fn band(&self, dxf: &mut Dxf) {
        match self.kind {
            Kind::LightHeader => {
                dxf.borders[2] = Some(edge(Line::Solid, self.paint(0.0)));
                dxf.borders[3] = Some(edge(Line::Solid, self.paint(0.0)));
            }
            Kind::MediumBody | Kind::MediumGrid => {
                let tint = if self.color == 0 {
                    DARK_BAND_TINT + 0.1
                } else {
                    DARK_BAND_TINT
                };
                dxf.fill = Some(self.paint(tint));
            }
            Kind::LightLines | Kind::LightGrid | Kind::MediumLines => dxf.fill = Some(self.paint(self.band_tint())),
        }
    }

    /// Sets the header row's look over `dxf`.
    fn header(&self, dxf: &mut Dxf) {
        dxf.bold = Some(true);
        match self.kind {
            Kind::LightHeader | Kind::MediumLines | Kind::MediumBody => {
                dxf.fill = Some(self.paint(0.0));
                dxf.color = Some(Color::Theme(LIGHT_1, 0.0));
            }
            Kind::LightLines | Kind::LightGrid => {
                dxf.fill = None;
                dxf.borders[3] = Some(edge(Line::Solid, self.paint(0.0)));
            }
            Kind::MediumGrid => dxf.fill = None,
        }
    }
}

/// Which edges of a table a cell is on.
#[derive(Debug, Clone, Copy)]
struct Edges {
    top: bool,
    bottom: bool,
    left: bool,
    right: bool,
}

/// A thin line (or double, as the line over a total row) of `color`.
fn edge(line: Line, color: Color) -> Edge {
    Edge {
        line,
        width_px: if line == Line::Double { 3.0 } else { 1.0 },
        color: Some(color),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(info: &str) -> TableStyle {
        read(
            &format!(
                r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" ref="B2:D5"><tableStyleInfo {info}/></table>"#
            ),
            1,
        )
        .unwrap()
    }

    #[test]
    fn medium_2_has_a_blue_header_and_banded_rows() {
        let style = table(r#"name="TableStyleMedium2" showRowStripes="1""#);
        let header = style.cell((1, 1)).unwrap();
        assert_eq!(header.fill, Some(Color::Theme(ACCENT_1, 0.0)));
        assert_eq!(header.color, Some(Color::Theme(LIGHT_1, 0.0)));
        assert_eq!(header.bold, Some(true));
        let first = style.cell((2, 2)).unwrap();
        assert_eq!(first.fill, Some(Color::Theme(ACCENT_1, BAND_TINT)));
        assert_eq!(style.cell((3, 2)).unwrap().fill, None);
        // Outside the table.
        assert!(style.cell((0, 1)).is_none());
        assert!(style.cell((2, 4)).is_none());
    }

    #[test]
    fn dark_and_unknown_styles_are_left_out() {
        let xml = |name: &str| {
            format!(
                r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" ref="A1:B2"><tableStyleInfo name="{name}"/></table>"#
            )
        };
        assert!(read(&xml("TableStyleDark1"), 1).is_none());
        assert!(read(&xml("MyStyle"), 1).is_none());
        assert!(read(&xml("TableStyleLight1"), 1).is_some());
    }
}
