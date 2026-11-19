//! Metrics of fonts Word documents often use that no open font matches
//! (Tahoma, Verdana, Segoe UI, Georgia...): the advance width of each
//! character and the vertical metrics, read from the fonts embedded in
//! PDFs Word made (`metrics.txt`, built by `scripts/build-font-metrics.py`).
//! When such a font is not installed, text is measured with these widths,
//! so lines break where Word breaks them, and drawn with a stand-in face.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The metric table.
const TABLE: &str = include_str!("metrics.txt");

/// The metrics of one face.
#[derive(Debug)]
pub struct MetricFace {
    /// Units per em.
    pub upem: f32,
    /// Ascent Word uses, in multiples of the font size.
    pub ascent: f32,
    /// Descent Word uses.
    pub descent: f32,
    /// External leading Word adds.
    pub leading: f32,
    /// Advance of each character, in font units.
    widths: HashMap<u32, u16>,
}

impl MetricFace {
    /// The advance of `ch` in multiples of the font size, when known.
    pub fn advance(&self, ch: char) -> Option<f32> {
        self.widths
            .get(&u32::from(ch))
            .map(|&width| f32::from(width) / self.upem)
    }
}

/// The faces of the table, by (lowercase family, bold, italic).
fn faces() -> &'static HashMap<(String, bool, bool), MetricFace> {
    static FACES: OnceLock<HashMap<(String, bool, bool), MetricFace>> = OnceLock::new();
    FACES.get_or_init(|| parse(TABLE))
}

/// Parses the table.
fn parse(table: &str) -> HashMap<(String, bool, bool), MetricFace> {
    let mut faces = HashMap::new();
    let mut current: Option<((String, bool, bool), MetricFace)> = None;
    for line in table.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "face" && fields.len() >= 10 {
            if let Some((key, face)) = current.take() {
                faces.insert(key, face);
            }
            let number = |at: usize| fields[at].parse::<f32>().unwrap_or(0.0);
            let upem = number(4).max(1.0);
            let (win_ascent, win_descent) = (number(5), number(6));
            let hhea_height = number(7) - number(8);
            let leading = (number(9) - ((win_ascent + win_descent) - hhea_height)).max(0.0);
            current = Some((
                (fields[1].to_lowercase(), fields[2] == "1", fields[3] == "1"),
                MetricFace {
                    upem,
                    ascent: win_ascent / upem,
                    descent: win_descent / upem,
                    leading: leading / upem,
                    widths: HashMap::new(),
                },
            ));
            continue;
        }
        let Some((_, face)) = current.as_mut() else { continue };
        let Some((start, widths)) = line.split_once('\t') else {
            continue;
        };
        let Ok(start) = u32::from_str_radix(start, 16) else {
            continue;
        };
        for (offset, width) in widths.split(' ').enumerate() {
            if let Ok(width) = width.parse() {
                face.widths.insert(start + offset as u32, width);
            }
        }
    }
    if let Some((key, face)) = current {
        faces.insert(key, face);
    }
    faces
}

/// The metrics of `family` in the given style: the face itself, else the
/// same weight, else the regular face (Word fakes the missing styles).
pub fn lookup(family: &str, bold: bool, italic: bool) -> Option<&'static MetricFace> {
    let faces = faces();
    let name = family.to_lowercase();
    [(bold, italic), (bold, false), (false, false)]
        .into_iter()
        .find_map(|(bold, italic)| faces.get(&(name.clone(), bold, italic)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tahoma's widths are known, and missing styles fall back.
    #[test]
    fn tahoma_is_measured() {
        let face = lookup("Tahoma", false, false).expect("Tahoma is in the table");
        let width = face.advance('a').expect("a is measured");
        assert!(width > 0.4 && width < 0.7, "{width}");
        assert!(lookup("tahoma", false, true).is_some());
        assert!(lookup("No Such Font", false, false).is_none());
    }
}
