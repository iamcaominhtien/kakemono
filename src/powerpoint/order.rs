//! The reading order of a slide's shapes, from where they lie: few decks
//! set PowerPoint's reading order (the order of the shape tree, which is
//! also the order shapes were drawn in), so text boxes copied from other
//! slides, or drawn last, would come in any order. The shapes are cut
//! recursively into columns or rows by the widest empty band between them
//! (the XY-cut of page layout analysis): two columns with a heading each
//! read column by column, rows of a label and its text row by row. Shapes
//! that overlap so that no band separates them keep their tree order.

/// Where a shape lies on its slide, in points from the top left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// Left edge.
    pub(crate) x: f32,
    /// Top edge.
    pub(crate) y: f32,
    /// Width.
    pub(crate) width: f32,
    /// Height.
    pub(crate) height: f32,
}

impl Frame {
    /// The start and end of the frame along the horizontal axis
    /// (`across`) or the vertical one.
    fn span(&self, across: bool) -> (f32, f32) {
        if across {
            (self.x, self.x + self.width)
        } else {
            (self.y, self.y + self.height)
        }
    }
}

/// The reading order of the shapes of `frames` (given in tree order), as
/// indices into it. A shape without a frame keeps the shapes in tree
/// order: there is nothing to place it by. Shapes may overlap by
/// `overlap_pt` and still be told apart.
pub(crate) fn reading_order(frames: &[Option<Frame>], overlap_pt: f32) -> Vec<usize> {
    let all: Vec<usize> = (0..frames.len()).collect();
    let Some(frames) = frames.iter().copied().collect::<Option<Vec<Frame>>>() else {
        return all;
    };
    let mut out = Vec::with_capacity(frames.len());
    cut(&frames, all, overlap_pt, &mut out);
    out
}

/// Adds the shapes `indices` of `frames` to `out` in reading order.
fn cut(frames: &[Frame], indices: Vec<usize>, overlap_pt: f32, out: &mut Vec<usize>) {
    if indices.len() <= 1 {
        out.extend(indices);
        return;
    }
    let columns = groups(frames, &indices, true, overlap_pt);
    let rows = groups(frames, &indices, false, overlap_pt);
    // The widest empty band decides: a gutter between columns, or the
    // space between rows; rows when they are as wide.
    let chosen = match (columns, rows) {
        (Some(columns), Some(rows)) => {
            if columns.0 > rows.0 {
                columns.1
            } else {
                rows.1
            }
        }
        (Some(columns), None) => columns.1,
        (None, Some(rows)) => rows.1,
        (None, None) => {
            let mut indices = indices;
            indices.sort_unstable();
            out.extend(indices);
            return;
        }
    };
    for group in chosen {
        cut(frames, group, overlap_pt, out);
    }
}

/// The shapes `indices` cut into groups along one axis (columns, left to
/// right, when `across`; else rows, top to bottom) at every empty band
/// between them, with the width of the widest band; `None` when no band
/// separates them.
fn groups(frames: &[Frame], indices: &[usize], across: bool, overlap_pt: f32) -> Option<(f32, Vec<Vec<usize>>)> {
    let mut sorted = indices.to_vec();
    sorted.sort_by(|a, b| {
        let (a_start, _) = frames[*a].span(across);
        let (b_start, _) = frames[*b].span(across);
        a_start.total_cmp(&b_start).then(a.cmp(b))
    });
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut widest = f32::NEG_INFINITY;
    let mut end = f32::NEG_INFINITY;
    for index in sorted {
        let (start, stop) = frames[index].span(across);
        if out.is_empty() || start < end - overlap_pt {
            if out.is_empty() {
                out.push(Vec::new());
            }
        } else {
            widest = widest.max(start - end);
            out.push(Vec::new());
        }
        out.last_mut().expect("a group").push(index);
        end = end.max(stop);
    }
    (out.len() > 1).then_some((widest, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(x: f32, y: f32, width: f32, height: f32) -> Option<Frame> {
        Some(Frame { x, y, width, height })
    }

    #[test]
    fn columns_are_read_one_after_the_other() {
        // Two columns, each a heading over its text, the headings first in
        // the tree; the gutter (40) is wider than the space under the
        // headings (10).
        let frames = [
            frame(0.0, 0.0, 300.0, 30.0),
            frame(340.0, 0.0, 300.0, 30.0),
            frame(0.0, 40.0, 300.0, 200.0),
            frame(340.0, 40.0, 300.0, 200.0),
        ];
        assert_eq!(reading_order(&frames, 2.0), [0, 2, 1, 3]);
    }

    #[test]
    fn rows_of_a_label_and_its_text_are_read_row_by_row() {
        // A label left of its text, two rows; the rows are 40 apart, the
        // label 10 from its text.
        let frames = [
            frame(0.0, 0.0, 100.0, 100.0),
            frame(0.0, 140.0, 100.0, 100.0),
            frame(110.0, 0.0, 400.0, 100.0),
            frame(110.0, 140.0, 400.0, 100.0),
        ];
        assert_eq!(reading_order(&frames, 2.0), [0, 2, 1, 3]);
    }

    #[test]
    fn a_band_across_the_top_comes_first() {
        let frames = [
            frame(0.0, 100.0, 300.0, 200.0),
            frame(340.0, 100.0, 300.0, 200.0),
            frame(0.0, 0.0, 640.0, 60.0),
        ];
        assert_eq!(reading_order(&frames, 2.0), [2, 0, 1]);
    }

    #[test]
    fn overlapping_shapes_keep_their_tree_order() {
        let frames = [frame(0.0, 0.0, 300.0, 300.0), frame(50.0, 50.0, 100.0, 100.0)];
        assert_eq!(reading_order(&frames, 2.0), [0, 1]);
        // A shape without a frame: the tree order.
        assert_eq!(reading_order(&[frame(0.0, 100.0, 1.0, 1.0), None], 2.0), [0, 1]);
    }
}
