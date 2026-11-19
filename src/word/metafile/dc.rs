//! The device context a metafile draws with: selected pen, brush and font,
//! text settings, the mapping from logical to picture coordinates, the
//! current position and the path being built. Both EMF and WMF records act
//! on it; it writes what they draw to the SVG.

use super::svg::Svg;

/// A colour, as red, green and blue.
pub(super) type Rgb = [u8; 3];

/// A pen: the colour and width of lines, `None` for no line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Pen {
    /// Colour.
    pub(super) color: Rgb,
    /// Width in logical units; 0 for the thinnest line.
    pub(super) width: f32,
}

/// A font.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Font {
    /// Height in logical units: negative for the character height, positive
    /// for the cell height.
    pub(super) height: f32,
    /// Weight (400 regular, 700 bold).
    pub(super) weight: i32,
    /// Italic.
    pub(super) italic: bool,
    /// Underlined.
    pub(super) underline: bool,
    /// Angle of the text, in tenths of a degree, counterclockwise.
    pub(super) escapement: f32,
    /// Face name.
    pub(super) face: String,
}

impl Default for Font {
    fn default() -> Self {
        Font {
            height: -12.0,
            weight: 400,
            italic: false,
            underline: false,
            escapement: 0.0,
            face: "Arial".into(),
        }
    }
}

/// An object a metafile creates and selects.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Object {
    /// A pen; `None` draws no line.
    Pen(Option<Pen>),
    /// A brush; `None` fills nothing.
    Brush(Option<Rgb>),
    /// A font.
    Font(Font),
    /// Anything else (palette, region): selecting it changes nothing.
    Other,
}

/// How text is aligned on its reference point.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct TextAlign {
    /// The flags (`TA_*`).
    pub(super) flags: u32,
}

/// Text aligned on its baseline.
const TA_BASELINE: u32 = 24;
/// Text aligned on its bottom.
const TA_BOTTOM: u32 = 8;
/// Text centred on its reference point.
const TA_CENTER: u32 = 6;
/// Text ending at its reference point.
const TA_RIGHT: u32 = 2;
/// Text starting at the current position, which then moves past it.
pub(super) const TA_UPDATECP: u32 = 1;

/// Share of a font's cell height that is its character height.
const CELL_TO_EM_RATIO: f32 = 0.87;
/// Ascent of text, as a share of its size, for text aligned on its top.
const ASCENT_RATIO: f32 = 0.9;
/// Descent of text, as a share of its size, for text aligned on its bottom.
const DESCENT_RATIO: f32 = 0.22;

/// A transform: x' = a·x + c·y + e, y' = b·x + d·y + f.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Matrix {
    /// The six coefficients a, b, c, d, e, f.
    pub(super) coefficients: [f32; 6],
}

impl Matrix {
    /// The transform that changes nothing.
    pub(super) const IDENTITY: Matrix = Matrix {
        coefficients: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
    };

    /// The point (x, y) transformed.
    pub(super) fn apply(self, x: f32, y: f32) -> (f32, f32) {
        let [xx, yx, xy, yy, dx, dy] = self.coefficients;
        (xx * x + xy * y + dx, yx * x + yy * y + dy)
    }

    /// This transform, then `then`.
    pub(super) fn then(self, then: Matrix) -> Matrix {
        let [a1, b1, c1, d1, e1, f1] = self.coefficients;
        let [a2, b2, c2, d2, e2, f2] = then.coefficients;
        Matrix {
            coefficients: [
                a1 * a2 + b1 * c2,
                a1 * b2 + b1 * d2,
                c1 * a2 + d1 * c2,
                c1 * b2 + d1 * d2,
                e1 * a2 + f1 * c2 + e2,
                e1 * b2 + f1 * d2 + f2,
            ],
        }
    }

    /// How much the transform scales lengths, on average.
    pub(super) fn scale(self) -> f32 {
        let [xx, yx, xy, yy, _, _] = self.coefficients;
        ((xx * yy - yx * xy).abs()).sqrt()
    }
}

/// The state of the device context, saved and restored as a whole.
#[derive(Debug, Clone)]
pub(super) struct State {
    /// The pen lines are drawn with.
    pub(super) pen: Option<Pen>,
    /// The brush shapes are filled with.
    pub(super) brush: Option<Rgb>,
    /// The font text is drawn with.
    pub(super) font: Font,
    /// Colour of text.
    pub(super) text_color: Rgb,
    /// Alignment of text.
    pub(super) text_align: TextAlign,
    /// Polygons are filled with the even-odd rule (else non-zero).
    pub(super) even_odd: bool,
    /// Origin of the window (logical coordinates).
    pub(super) window_org: (f32, f32),
    /// Extent of the window, when set.
    pub(super) window_ext: Option<(f32, f32)>,
    /// Origin of the viewport (device coordinates).
    pub(super) viewport_org: (f32, f32),
    /// Extent of the viewport, when set.
    pub(super) viewport_ext: Option<(f32, f32)>,
    /// The world transform (EMF).
    pub(super) world: Matrix,
    /// Device units per logical unit for the fixed mapping modes (EMF), by
    /// axis, or `None` for the text, isotropic and anisotropic modes.
    pub(super) fixed_scale: Option<(f32, f32)>,
    /// Current position (logical coordinates).
    pub(super) position: (f32, f32),
}

impl Default for State {
    fn default() -> Self {
        State {
            pen: Some(Pen {
                color: [0, 0, 0],
                width: 0.0,
            }),
            brush: Some([255, 255, 255]),
            font: Font::default(),
            text_color: [0, 0, 0],
            text_align: TextAlign::default(),
            even_odd: true,
            window_org: (0.0, 0.0),
            window_ext: None,
            viewport_org: (0.0, 0.0),
            viewport_ext: None,
            world: Matrix::IDENTITY,
            fixed_scale: None,
            position: (0.0, 0.0),
        }
    }
}

/// The device context.
pub(super) struct Dc {
    /// The current state.
    pub(super) state: State,
    /// Saved states.
    saved: Vec<State>,
    /// The objects created, by index.
    pub(super) objects: Vec<Option<Object>>,
    /// A path being built (between `BEGINPATH` and `ENDPATH`), or the last
    /// one built.
    pub(super) path: Option<String>,
    /// A path is being built.
    pub(super) in_path: bool,
    /// What is drawn.
    pub(super) svg: Svg,
}

impl Dc {
    /// A device context with the default state, drawing to an empty SVG.
    pub(super) fn new() -> Self {
        Dc {
            state: State::default(),
            saved: Vec::new(),
            objects: Vec::new(),
            path: None,
            in_path: false,
            svg: Svg::default(),
        }
    }

    /// Saves the state.
    pub(super) fn save(&mut self) {
        self.saved.push(self.state.clone());
    }

    /// Restores a saved state: `relative` < 0 counts back from the last
    /// one saved, else it is the index of the state.
    pub(super) fn restore(&mut self, relative: i32) {
        let count = self.saved.len() as i32;
        let target = if relative < 0 { count + relative } else { relative - 1 };
        if target < 0 || target >= count {
            return;
        }
        self.saved.truncate(target as usize + 1);
        if let Some(state) = self.saved.pop() {
            self.state = state;
        }
    }

    /// Stores `object` at the first free index (WMF), or at `index` (EMF).
    pub(super) fn create(&mut self, object: Object, index: Option<usize>) {
        let index = index.unwrap_or_else(|| {
            self.objects
                .iter()
                .position(Option::is_none)
                .unwrap_or(self.objects.len())
        });
        if self.objects.len() <= index {
            self.objects.resize(index + 1, None);
        }
        self.objects[index] = Some(object);
    }

    /// Selects the object at `index`.
    pub(super) fn select(&mut self, index: usize) {
        if let Some(Some(object)) = self.objects.get(index).cloned() {
            self.select_object(object);
        }
    }

    /// Makes `object` the current pen, brush or font.
    pub(super) fn select_object(&mut self, object: Object) {
        match object {
            Object::Pen(pen) => self.state.pen = pen,
            Object::Brush(brush) => self.state.brush = brush,
            Object::Font(font) => self.state.font = font,
            Object::Other => {}
        }
    }

    /// Deletes the object at `index`.
    pub(super) fn delete(&mut self, index: usize) {
        if let Some(slot) = self.objects.get_mut(index) {
            *slot = None;
        }
    }

    /// The transform from logical to picture coordinates.
    pub(super) fn transform(&self) -> Matrix {
        let state = &self.state;
        let (wx, wy) = state.window_org;
        let (vx, vy) = state.viewport_org;
        let (sx, sy) = match (state.fixed_scale, state.window_ext, state.viewport_ext) {
            (Some(scale), _, _) => scale,
            (None, Some((wex, wey)), Some((vex, vey))) if wex != 0.0 && wey != 0.0 => (vex / wex, vey / wey),
            _ => (1.0, 1.0),
        };
        let mapping = Matrix {
            coefficients: [sx, 0.0, 0.0, sy, vx - wx * sx, vy - wy * sy],
        };
        state.world.then(mapping)
    }

    /// The logical point (x, y) in picture coordinates.
    pub(super) fn point(&self, x: f32, y: f32) -> (f32, f32) {
        self.transform().apply(x, y)
    }

    /// A logical length in picture units.
    pub(super) fn length(&self, length: f32) -> f32 {
        length * self.transform().scale()
    }

    /// The SVG path data of a polyline through `points` (logical), closed
    /// when `close`.
    pub(super) fn poly_data(&self, points: &[(f32, f32)], close: bool) -> String {
        let mut data = String::new();
        for (at, &(x, y)) in points.iter().enumerate() {
            let (px, py) = self.point(x, y);
            data.push_str(&format!("{}{} {} ", if at == 0 { 'M' } else { 'L' }, num(px), num(py)));
        }
        if close && !points.is_empty() {
            data.push('Z');
        }
        data
    }

    /// The SVG path data of Bézier curves from `start` through `points`
    /// (three per curve), logical coordinates.
    pub(super) fn bezier_data(&self, start: Option<(f32, f32)>, points: &[(f32, f32)]) -> String {
        let mut data = String::new();
        let mut rest = points;
        if let Some((x, y)) = start {
            let (px, py) = self.point(x, y);
            data.push_str(&format!("M{} {} ", num(px), num(py)));
        }
        while rest.len() >= 3 {
            data.push('C');
            for &(x, y) in &rest[..3] {
                let (px, py) = self.point(x, y);
                data.push_str(&format!("{} {} ", num(px), num(py)));
            }
            rest = &rest[3..];
        }
        data
    }

    /// Draws path data: added to the path being built, or filled with the
    /// brush (when `fill`) and stroked with the pen.
    pub(super) fn draw(&mut self, data: &str, fill: bool) {
        if self.in_path {
            self.path.get_or_insert_with(String::new).push_str(data);
            return;
        }
        let brush = if fill { self.state.brush } else { None };
        let pen = self.state.pen.map(|pen| (pen.color, self.length(pen.width)));
        self.svg.path(data, brush, pen, self.state.even_odd);
    }

    /// Fills and strokes the path built (`FILLPATH`, `STROKEPATH`, ...).
    pub(super) fn draw_path(&mut self, fill: bool, stroke: bool) {
        let Some(data) = self.path.take() else { return };
        let brush = if fill { self.state.brush } else { None };
        let pen = if stroke {
            self.state.pen.map(|pen| (pen.color, self.length(pen.width)))
        } else {
            None
        };
        self.svg.path(&data, brush, pen, self.state.even_odd);
    }

    /// The SVG path data of an ellipse in the logical box (left, top, right,
    /// bottom).
    pub(super) fn ellipse_data(&self, bounds: [f32; 4]) -> String {
        let [left, top, right, bottom] = bounds;
        let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
        let (rx, ry) = ((right - left).abs() / 2.0, (bottom - top).abs() / 2.0);
        // Four Bézier quarters, transformed point by point.
        let kappa = 0.552_284_8;
        let points = [
            (cx + rx, cy + ry * kappa),
            (cx + rx * kappa, cy + ry),
            (cx, cy + ry),
            (cx - rx * kappa, cy + ry),
            (cx - rx, cy + ry * kappa),
            (cx - rx, cy),
            (cx - rx, cy - ry * kappa),
            (cx - rx * kappa, cy - ry),
            (cx, cy - ry),
            (cx + rx * kappa, cy - ry),
            (cx + rx, cy - ry * kappa),
            (cx + rx, cy),
        ];
        let mut data = self.bezier_data(Some((cx + rx, cy)), &points);
        data.push('Z');
        data
    }

    /// The SVG path data of an arc of the ellipse in `bounds`, from the
    /// direction of `start` to that of `end` (counterclockwise), as a pie
    /// (`pie`), a chord (`close`) or an open arc.
    pub(super) fn arc_data(
        &self,
        bounds: [f32; 4],
        start: (f32, f32),
        end: (f32, f32),
        pie: bool,
        close: bool,
    ) -> String {
        let [left, top, right, bottom] = bounds;
        let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
        let (rx, ry) = ((right - left).abs() / 2.0, (bottom - top).abs() / 2.0);
        let angle = |(x, y): (f32, f32)| (-(y - cy) / ry.max(1e-6)).atan2((x - cx) / rx.max(1e-6));
        let from = angle(start);
        let mut to = angle(end);
        if to <= from {
            to += std::f32::consts::TAU;
        }
        let steps = (((to - from) / 0.1).ceil() as usize).clamp(2, 400);
        let mut points: Vec<(f32, f32)> = (0..=steps)
            .map(|step| {
                let theta = from + (to - from) * step as f32 / steps as f32;
                (cx + rx * theta.cos(), cy - ry * theta.sin())
            })
            .collect();
        if pie {
            points.push((cx, cy));
        }
        self.poly_data(&points, pie || close)
    }

    /// Draws `text` at the logical point (x, y), with the advances `dx` of
    /// its characters (logical units) when given.
    pub(super) fn text(&mut self, x: f32, y: f32, text: &str, dx: &[f32]) {
        if text.trim().is_empty() {
            return;
        }
        let font = self.state.font.clone();
        let size = if font.height < 0.0 {
            -font.height
        } else {
            font.height * CELL_TO_EM_RATIO
        };
        let flags = self.state.text_align.flags;
        let (mut x, y) = if flags & TA_UPDATECP != 0 {
            self.state.position
        } else {
            (x, y)
        };
        let total: f32 = if dx.is_empty() {
            text.chars().count() as f32 * size * 0.5
        } else {
            dx.iter().sum()
        };
        if flags & TA_CENTER == TA_CENTER {
            x -= total / 2.0;
        } else if flags & TA_RIGHT != 0 {
            x -= total;
        }
        // The baseline: text is aligned on its top unless told otherwise.
        let baseline = if flags & TA_BASELINE == TA_BASELINE {
            y
        } else if flags & TA_BOTTOM != 0 {
            y - size * DESCENT_RATIO
        } else {
            y + size * ASCENT_RATIO
        };
        if flags & TA_UPDATECP != 0 {
            self.state.position.0 = x + total;
        }
        let mut positions = Vec::new();
        let mut pen_x = x;
        for (at, _) in text.chars().enumerate() {
            positions.push(self.point(pen_x, baseline));
            pen_x += dx.get(at).copied().unwrap_or(size * 0.5);
        }
        let picture_size = self.length(size);
        let color = self.state.text_color;
        self.svg
            .text(text, &positions, picture_size, &font, color, !dx.is_empty());
    }
}

/// Raster operation that leaves the destination as it is (Office writes
/// such blits as markers in EMF+ files).
pub(super) const ROP_NOP: u32 = 0x00AA_0029;
/// Raster operation that paints with the brush.
const ROP_PATCOPY: u32 = 0x00F0_0021;
/// Raster operation that paints black.
const ROP_BLACKNESS: u32 = 0x0000_0042;
/// Raster operation that paints white.
const ROP_WHITENESS: u32 = 0x00FF_0062;

impl Dc {
    /// A blit without a bitmap: the logical box (x, y, width, height)
    /// painted as the raster operation `rop` says (only the brush, black or
    /// white; other operations, which combine with what is drawn, are
    /// left out).
    pub(super) fn fill_blit(&mut self, rop: u32, bounds: [f32; 4]) {
        let fill = match rop {
            ROP_PATCOPY => self.state.brush,
            ROP_BLACKNESS => Some([0, 0, 0]),
            ROP_WHITENESS => Some([255, 255, 255]),
            _ => return,
        };
        let [x, y, width, height] = bounds;
        let data = self.poly_data(
            &[(x, y), (x + width, y), (x + width, y + height), (x, y + height)],
            true,
        );
        self.svg.path(&data, fill, None, false);
    }
}

/// A number written compactly for SVG.
pub(super) fn num(value: f32) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded == rounded.trunc() {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded}")
    }
}
