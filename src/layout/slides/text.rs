//! A shape's text (`p:txBody`) laid out in its frame as PowerPoint lays it
//! out: the frame less its insets, each paragraph broken into lines by
//! `layout::para` with its alignment, indents, line spacing and space
//! before and after, bullets and numbers hung at `marL + indent`, the
//! sizes and line spacing of an autofit body scaled as PowerPoint wrote
//! (`a:normAutofit`), and the block of lines anchored at the top, middle
//! or bottom of the frame.

use super::look::Look;
use crate::chart::Rgb;
use crate::layout::display::Item;
use crate::layout::para::{self, Ctx};
use crate::powerpoint::inherit::level_props;
use crate::powerpoint::package::EMU_PER_PT;
use crate::powerpoint::text::{A14_NS, Bullet, DEFAULT_SIZE_PT, Numbers, PERCENT_UNITS, bullet};
use crate::word::docx::{Rel, alternate};
use crate::word::fmt::model::{
    Align, BreakKind, Inline, LineSpacing, NumberMark, NumberSuffix, Para, ParaProps, RunFmt, TabKind, TabStop,
    Underline, VertAlign,
};
use crate::word::xml::{DRAWING_NS, MC_NS, REL_NS, attr, child, children};
use roxmltree::Node;
use std::collections::HashMap;

/// PowerPoint's insets of a text frame that names none, in EMU: left,
/// top, right, bottom (0.1 inch and 0.05 inch).
const DEFAULT_INSETS_EMU: [f32; 4] = [91_440.0, 45_720.0, 91_440.0, 45_720.0];
/// The height of a line of single spacing, as a share of its font size,
/// which space before and after given in percent is a share of.
const LINE_HEIGHT_RATIO: f32 = 1.2;
/// The width a text that is not wrapped is laid out in, in points.
const NOWRAP_WIDTH_PT: f32 = 100_000.0;
/// The bullet a picture bullet (`a:buBlip`) is drawn as.
const PICTURE_BULLET: &str = "\u{2022}";
/// What a DrawingML angle is written in: 60 000 is a degree.
const ANGLE_UNITS: f32 = 60_000.0;
/// The least turn of a text drawn turned, in degrees.
const TURN_TOLERANCE_DEG: f32 = 0.01;
/// The deepest level of a paragraph (`lvl` 0 to 8).
const MAX_LEVEL: u8 = 8;

/// A shape's text and what it takes its properties from.
pub(super) struct Body<'a, 'i> {
    /// The text body (`p:txBody`, `a:txBody`).
    pub(super) body: Node<'a, 'i>,
    /// Its body properties, most specific first: its own `a:bodyPr`, then
    /// those of the placeholders it inherits from.
    pub(super) props: Vec<Node<'a, 'i>>,
    /// The list styles of its levels, most specific first.
    pub(super) styles: Vec<Node<'a, 'i>>,
    /// The colour of its text from the shape's style (`a:fontRef`).
    pub(super) font_color: Option<Rgb>,
    /// The theme font its style names: `Some(true)` major, `Some(false)`
    /// minor.
    pub(super) font_major: Option<bool>,
    /// The weight its table style gives its text (`a:tcTxStyle b`), which
    /// a run's own weight overrides.
    pub(super) font_bold: Option<bool>,
    /// The turn of its shape, in degrees clockwise.
    pub(super) turn_deg: f32,
    /// Insets of its own when the body says none (a table cell's margins),
    /// in points: left, top, right, bottom.
    pub(super) insets_pt: Option<[f32; 4]>,
}

/// What a body's text needs besides its XML.
pub(super) struct Context<'s, 'a, 'i> {
    /// The slide's theme.
    pub(super) look: &'s Look<'a, 'i>,
    /// The relationships of the part the text is in, for links.
    pub(super) rels: &'s HashMap<String, Rel>,
    /// The slide's number, for slide number fields.
    pub(super) number: usize,
}

/// The items of the text of `body` laid out in the frame `rect` (left,
/// top, width, height, in points on the page); and the height of its
/// lines.
pub(super) fn draw(
    ctx: &mut Ctx<'_>,
    context: &Context<'_, '_, '_>,
    body: &Body<'_, '_>,
    rect: [f32; 4],
) -> (Vec<Item>, f32) {
    let said = |name: &str| body.props.iter().find_map(|props| props.attribute(name));
    let (vertical, turn) = turns(&said, body);
    // A turned shape's frame is the frame before the turn; a vertical
    // text's is the frame shown.
    let across = vertical;
    let [x, y, width, height] = if across {
        let [x, y, width, height] = rect;
        [x + (width - height) / 2.0, y + (height - width) / 2.0, height, width]
    } else {
        rect
    };
    let inset = |at: usize, name: &str| {
        said(name)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|emu| emu.is_finite() && *emu >= 0.0)
            .map(|emu| emu / EMU_PER_PT)
            .or_else(|| body.insets_pt.map(|insets| insets[at]))
            .unwrap_or(DEFAULT_INSETS_EMU[at] / EMU_PER_PT)
    };
    let [left, top, right, bottom] = [inset(0, "lIns"), inset(1, "tIns"), inset(2, "rIns"), inset(3, "bIns")];
    let nowrap = said("wrap") == Some("none");
    let (scale, reduction) = autofit(body.props.first().copied());
    let inner = (width - left - right).max(1.0);
    let line_width = if nowrap { NOWRAP_WIDTH_PT } else { inner };
    let mut numbers = Numbers::default();
    let mut items = Vec::new();
    let mut pen = 0.0_f32;
    let mut after = 0.0_f32;
    let paragraphs: Vec<Node<'_, '_>> = children(body.body, DRAWING_NS, "p").collect();
    for (index, node) in paragraphs.iter().enumerate() {
        let mut read = paragraph(*node, body, context, &mut numbers, scale, reduction);
        // PowerPoint adds the space after a paragraph and before the next;
        // the first paragraph's space before is not at the top of the frame.
        if index > 0 {
            pen += after + read.before;
        }
        after = read.after;
        // A text that is not wrapped is set on lines as long as they are,
        // each placed in the frame as its alignment says.
        let align = read.para.props.align;
        if nowrap {
            read.para.props.align = Align::Left;
        }
        let laid = para::layout(ctx, &read.para, line_width);
        for mut line in laid.lines {
            let shift = if nowrap {
                let end = line_end(&line.items);
                match align {
                    Align::Center => (inner - end) / 2.0,
                    Align::Right => inner - end,
                    _ => 0.0,
                }
            } else {
                0.0
            };
            translate(&mut line.items, shift, pen);
            items.append(&mut line.items);
            pen += line.height;
        }
    }
    let offset = match said("anchor") {
        Some("ctr") => top + (height - top - bottom - pen) / 2.0,
        Some("b") => height - bottom - pen,
        _ => top,
    };
    translate(&mut items, x + left, y + offset);
    if turn.abs() > TURN_TOLERANCE_DEG && (turn - 360.0).abs() > TURN_TOLERANCE_DEG && !items.is_empty() {
        items.insert(
            0,
            Item::Turn {
                angle_deg: turn,
                x: x + width / 2.0,
                y: y + height / 2.0,
            },
        );
        items.push(Item::EndTurn);
    }
    (items, pen + top + bottom)
}

/// Whether a body's text is vertical, and the turn it is drawn at, in
/// degrees clockwise: vertical text is laid out across the frame turned,
/// then turned; the text of a turned shape turns with it. `said` gives
/// its body properties.
fn turns<'a>(said: &dyn Fn(&str) -> Option<&'a str>, body: &Body<'_, '_>) -> (bool, f32) {
    let vertical = match said("vert") {
        Some("vert" | "eaVert" | "wordArtVert" | "mongolianVert") => 90.0,
        Some("vert270") => 270.0,
        _ => 0.0,
    };
    let own_turn = said("rot")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map_or(0.0, |value| value / ANGLE_UNITS);
    (vertical != 0.0, (body.turn_deg + vertical + own_turn).rem_euclid(360.0))
}

/// The share of their size an autofit body's fonts are drawn at, and the
/// share taken off its line spacing (`a:normAutofit fontScale
/// lnSpcReduction`), from its own body properties `props`.
fn autofit(props: Option<Node<'_, '_>>) -> (f32, f32) {
    let Some(fit) = props.and_then(|props| child(props, DRAWING_NS, "normAutofit")) else {
        return (1.0, 0.0);
    };
    let share = |name: &str, default: f32| {
        fit.attribute(name)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map_or(default, |value| value / PERCENT_UNITS)
    };
    (
        share("fontScale", 1.0).clamp(0.01, 1.0),
        share("lnSpcReduction", 0.0).clamp(0.0, 0.9),
    )
}

/// A paragraph read for layout, with the space PowerPoint puts before and
/// after it.
struct Read {
    /// The paragraph.
    para: Para,
    /// The space before it, in points.
    before: f32,
    /// The space after it, in points.
    after: f32,
}

/// The paragraph `node` (`a:p`) of `body`, its sizes times `scale` and its
/// line spacing less `reduction`.
fn paragraph(
    node: Node<'_, '_>,
    body: &Body<'_, '_>,
    context: &Context<'_, '_, '_>,
    numbers: &mut Numbers,
    scale: f32,
    reduction: f32,
) -> Read {
    let props = child(node, DRAWING_NS, "pPr");
    let level = props
        .and_then(|props| props.attribute("lvl"))
        .and_then(|lvl| lvl.trim().parse::<u8>().ok())
        .unwrap_or(0)
        .min(MAX_LEVEL);
    let chain = level_props(props, &body.styles, level);
    let defaults: Vec<Node<'_, '_>> = chain
        .iter()
        .filter_map(|props| child(*props, DRAWING_NS, "defRPr"))
        .collect();
    let said = |name: &str| chain.iter().find_map(|props| props.attribute(name));
    let emu = |name: &str| {
        said(name)
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .map_or(0.0, |value| value / EMU_PER_PT)
    };
    let mut inlines = Vec::new();
    runs(node, &defaults, body, context, scale, &mut inlines);
    let mark = run_fmt(child(node, DRAWING_NS, "endParaRPr"), &defaults, body, context, scale);
    let first = inlines.iter().find_map(|inline| match inline {
        Inline::Text { fmt, .. } => Some(fmt.clone()),
        _ => None,
    });
    let size = first.as_ref().map_or(mark.size, |fmt| fmt.size);
    let has_text = inlines
        .iter()
        .any(|inline| matches!(inline, Inline::Text { text, .. } if !text.trim().is_empty()));
    // An empty paragraph shows no bullet.
    let number = if has_text {
        number_mark(&chain, level, numbers, first.as_ref().unwrap_or(&mark), context)
    } else {
        None
    };
    // PowerPoint's single line is 1.2 times the size of its text, whatever
    // the font: the paragraph's largest (not its mark's, when it has text).
    let largest = inlines
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text { fmt, .. } => Some(fmt.size),
            _ => None,
        })
        .reduce(f32::max)
        // An empty paragraph is as high as its mark.
        .unwrap_or(mark.size);
    let single = largest * LINE_HEIGHT_RATIO;
    let line = match spacing(&chain, "lnSpc", size, scale) {
        Some(Spacing::Share(share)) => LineSpacing::Exact(single * (share - reduction).max(0.1)),
        Some(Spacing::Points(points)) => LineSpacing::Exact(points),
        None => LineSpacing::Exact(single * (1.0 - reduction)),
    };
    let space = |name: &str| match spacing(&chain, name, size, scale) {
        Some(Spacing::Share(share)) => share * size * LINE_HEIGHT_RATIO,
        Some(Spacing::Points(points)) => points,
        None => 0.0,
    };
    let (before, after) = (space("spcBef") * (1.0 - reduction), space("spcAft") * (1.0 - reduction));
    Read {
        para: Para {
            props: ParaProps {
                align: alignment(said("algn")),
                left: emu("marL").max(0.0),
                first_line: emu("indent"),
                line,
                tabs: tab_stops(&chain),
                bidi: matches!(said("rtl"), Some("1" | "true")),
                ..Default::default()
            },
            mark,
            number,
            inlines,
            ..Default::default()
        },
        before,
        after,
    }
}

/// The bullet or number of a paragraph at `level` with text, whose level
/// properties are `chain` and whose first run's format is `first`;
/// `numbers` counts the numbered paragraphs of its body.
fn number_mark(
    chain: &[Node<'_, '_>],
    level: u8,
    numbers: &mut Numbers,
    first: &RunFmt,
    context: &Context<'_, '_, '_>,
) -> Option<NumberMark> {
    match bullet(chain) {
        Bullet::None => {
            numbers.plain(usize::from(level));
            None
        }
        Bullet::Symbol => {
            numbers.plain(usize::from(level));
            let text = chain
                .iter()
                .find_map(|props| child(*props, DRAWING_NS, "buChar"))
                .and_then(|bullet| bullet.attribute("char"))
                .unwrap_or(PICTURE_BULLET)
                .to_string();
            Some(bullet_mark(text, chain, first, context, true))
        }
        Bullet::Number(scheme, start) => {
            let text = numbers.next(usize::from(level), &scheme, start);
            Some(bullet_mark(text, chain, first, context, false))
        }
    }
}

/// The alignment a paragraph's `algn` says.
fn alignment(said: Option<&str>) -> Align {
    match said {
        Some("ctr") => Align::Center,
        Some("r") => Align::Right,
        Some("just" | "justLow") => Align::Both,
        Some("dist" | "thaiDist") => Align::Distribute,
        _ => Align::Left,
    }
}

/// The tab stops the first of `chain` that has a list of them gives.
fn tab_stops(chain: &[Node<'_, '_>]) -> Vec<TabStop> {
    let Some(list) = chain.iter().find_map(|props| child(*props, DRAWING_NS, "tabLst")) else {
        return Vec::new();
    };
    children(list, DRAWING_NS, "tab")
        .filter_map(|tab| {
            let pos = tab.attribute("pos")?.trim().parse::<f32>().ok()? / EMU_PER_PT;
            let kind = match tab.attribute("algn") {
                Some("ctr") => TabKind::Center,
                Some("r") => TabKind::Right,
                Some("dec") => TabKind::Decimal,
                _ => TabKind::Left,
            };
            pos.is_finite().then_some(TabStop {
                pos,
                kind,
                leader: None,
            })
        })
        .collect()
}

/// Line spacing, or space before or after, as given.
enum Spacing {
    /// A share of single spacing (`a:spcPct`).
    Share(f32),
    /// Points (`a:spcPts`), already scaled.
    Points(f32),
}

/// The spacing `name` (`lnSpc`, `spcBef`, `spcAft`) the first of `chain`
/// gives that says; points given are scaled by `scale` (an autofit body's).
fn spacing(chain: &[Node<'_, '_>], name: &str, _size: f32, scale: f32) -> Option<Spacing> {
    let node = chain.iter().find_map(|props| child(*props, DRAWING_NS, name))?;
    let value = |kind: &str| {
        child(node, DRAWING_NS, kind)?
            .attribute("val")?
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite() && *value >= 0.0)
    };
    if let Some(share) = value("spcPct") {
        return Some(Spacing::Share(share / PERCENT_UNITS));
    }
    value("spcPts").map(|hundredths| Spacing::Points(hundredths / 100.0 * scale))
}

/// The mark of a bulleted or numbered paragraph: `text` in the bullet's
/// font, size and colour (`a:buFont`, `a:buSzPct`, `a:buClr`), else those
/// of the paragraph's first run `first`. A number keeps the text's font.
fn bullet_mark(
    text: String,
    chain: &[Node<'_, '_>],
    first: &RunFmt,
    context: &Context<'_, '_, '_>,
    symbol: bool,
) -> NumberMark {
    let mut fmt = first.clone();
    fmt.underline = None;
    fmt.href = None;
    // The first of the chain that says either the bullet's own or "as the
    // text" (`a:buFontTx`, `a:buSzTx`, `a:buClrTx`) decides.
    let decide = |own: &str, text: &str| {
        chain.iter().find_map(|props| {
            props.children().find(|node| {
                node.is_element() && (node.has_tag_name((DRAWING_NS, own)) || node.has_tag_name((DRAWING_NS, text)))
            })
        })
    };
    let mut text = text;
    if symbol
        && let Some(font) = decide("buFont", "buFontTx").filter(|node| node.tag_name().name() == "buFont")
        && let Some(name) = font
            .attribute("typeface")
            .and_then(|name| context.look.fonts.resolve(name))
    {
        // A symbol font's bullet is its Unicode look-alike, in the text's
        // font: the symbol fonts are seldom there to draw it.
        if let Some(shown) = symbol_bullet(&name, &text) {
            text = shown.to_string();
        } else {
            fmt.font.clone_from(&name);
            fmt.font_hansi.clone_from(&name);
            fmt.font_east_asia.clone_from(&name);
            fmt.font_cs = name;
        }
    }
    if let Some(size) = decide("buSzPct", "buSzTx").filter(|node| node.tag_name().name() != "buSzTx") {
        let value = size.attribute("val").and_then(|value| value.trim().parse::<f32>().ok());
        if let Some(value) = value.filter(|value| value.is_finite() && *value > 0.0) {
            fmt.size = if size.tag_name().name() == "buSzPts" {
                value / 100.0
            } else {
                first.size * value / PERCENT_UNITS
            };
            fmt.size_cs = fmt.size;
        }
    }
    if let Some(color) = decide("buClr", "buClrTx").filter(|node| node.tag_name().name() == "buClr")
        && let Some(rgb) = context.look.color(color, [0; 3])
    {
        fmt.color = Some(rgb);
    }
    NumberMark {
        text,
        fmt,
        suffix: NumberSuffix::Tab,
        align: Align::Left,
    }
}

/// The Unicode character that looks like the bullet `text` of the symbol
/// font `font` (Wingdings, Symbol); `None` for another font.
fn symbol_bullet(font: &str, text: &str) -> Option<char> {
    let font = font.to_lowercase();
    let wingdings = font.starts_with("wingdings");
    if !wingdings && font != "symbol" {
        return None;
    }
    // A symbol font's character may be written in the private use area.
    let code = text.chars().next().map_or(0, |ch| {
        let code = u32::from(ch);
        if (0xF000..=0xF0FF).contains(&code) {
            code - 0xF000
        } else {
            code
        }
    });
    let shown = if wingdings {
        SYMBOL_BULLETS
            .iter()
            .find(|(own, _)| *own == code)
            .map_or(DEFAULT_SYMBOL_BULLET, |(_, shown)| *shown)
    } else {
        match code {
            0xB7 => '\u{2022}',
            0xA8 => '\u{2663}',
            0xAE | 0xDE => '\u{2192}',
            _ => char::from_u32(code)
                .filter(|ch| !ch.is_control() && *ch != ' ')
                .unwrap_or(DEFAULT_SYMBOL_BULLET),
        }
    };
    Some(shown)
}

/// What a Wingdings bullet the table below does not know is drawn as.
const DEFAULT_SYMBOL_BULLET: char = '\u{2022}';
/// The usual bullets of Wingdings, by code, and the Unicode characters
/// that look like them.
const SYMBOL_BULLETS: [(u32, char); 14] = [
    (0x6C, '\u{25CF}'),
    (0x6E, '\u{25A0}'),
    (0x6F, '\u{25A1}'),
    (0x71, '\u{2751}'),
    (0x75, '\u{25C6}'),
    (0x76, '\u{2756}'),
    (0xA7, '\u{25AA}'),
    (0xA8, '\u{25FB}'),
    (0xD8, '\u{27A2}'),
    (0xE0, '\u{2192}'),
    (0xE8, '\u{2794}'),
    (0xFB, '\u{2717}'),
    (0xFC, '\u{2714}'),
    (0xFE, '\u{2611}'),
];

/// Adds the runs, breaks and fields of the paragraph (or branch of an
/// alternative) `node` to `out`.
fn runs(
    node: Node<'_, '_>,
    defaults: &[Node<'_, '_>],
    body: &Body<'_, '_>,
    context: &Context<'_, '_, '_>,
    scale: f32,
    out: &mut Vec<Inline>,
) {
    for item in node.children().filter(Node::is_element) {
        let name = item.tag_name().name();
        if item.tag_name().namespace() == Some(MC_NS) && name == "AlternateContent" {
            // A formula's fallback: its text as runs (the formula itself,
            // `a14:m`, is not drawn).
            if let Some(branch) = child(item, MC_NS, "Fallback").or_else(|| alternate(item)) {
                runs(branch, defaults, body, context, scale, out);
            }
            continue;
        }
        if item.tag_name().namespace() == Some(A14_NS) {
            continue;
        }
        let fmt = run_fmt(child(item, DRAWING_NS, "rPr"), defaults, body, context, scale);
        let text = match name {
            "r" => child(item, DRAWING_NS, "t")
                .and_then(|node| node.text())
                .unwrap_or_default()
                .to_string(),
            "fld" => {
                if item.attribute("type") == Some("slidenum") {
                    context.number.to_string()
                } else {
                    child(item, DRAWING_NS, "t")
                        .and_then(|node| node.text())
                        .unwrap_or_default()
                        .to_string()
                }
            }
            "br" => {
                out.push(Inline::Break {
                    kind: BreakKind::Line,
                    fmt,
                });
                continue;
            }
            _ => continue,
        };
        // A vertical tab in a run is a line break, as PowerPoint writes one.
        for (index, piece) in text.split(['\u{b}', '\n']).enumerate() {
            if index > 0 {
                out.push(Inline::Break {
                    kind: BreakKind::Line,
                    fmt: fmt.clone(),
                });
            }
            for (at, words) in piece.split('\t').enumerate() {
                if at > 0 {
                    out.push(Inline::Tab { fmt: fmt.clone() });
                }
                if !words.is_empty() {
                    out.push(Inline::Text {
                        text: words.to_string(),
                        fmt: fmt.clone(),
                    });
                }
            }
        }
    }
}

/// The format of a run with the properties `props` (`a:rPr`), what it
/// does not say taken from its level's `defaults` (`a:defRPr`, most
/// specific first), then from the shape's style.
fn run_fmt(
    props: Option<Node<'_, '_>>,
    defaults: &[Node<'_, '_>],
    body: &Body<'_, '_>,
    context: &Context<'_, '_, '_>,
    scale: f32,
) -> RunFmt {
    let chain: Vec<Node<'_, '_>> = props.into_iter().chain(defaults.iter().copied()).collect();
    let said = |name: &str| chain.iter().find_map(|node| node.attribute(name));
    let flag = |name: &str| said(name).is_some_and(|value| matches!(value, "1" | "true"));
    // A table style's weight is stronger than the level's.
    let bold = props
        .and_then(|props| props.attribute("b"))
        .map(|value| matches!(value, "1" | "true"))
        .or(body.font_bold)
        .unwrap_or_else(|| flag("b"));
    let size = said("sz")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|size| size.is_finite() && *size > 0.0)
        .map_or(DEFAULT_SIZE_PT, |size| size / 100.0)
        * scale;
    let look = context.look;
    let face = |script: &str, at: usize| {
        chain
            .iter()
            .filter_map(|node| child(*node, DRAWING_NS, script))
            .find_map(|font| font.attribute("typeface").and_then(|name| look.fonts.resolve(name)))
            .or_else(|| body.font_major.and_then(|major| look.fonts.theme(major, at)))
    };
    let latin = face("latin", 0)
        .or_else(|| look.fonts.theme(false, 0))
        .unwrap_or_else(|| crate::powerpoint::DEFAULT_BODY_FONT.to_string());
    let east_asia = face("ea", 1).unwrap_or_else(|| latin.clone());
    let complex = face("cs", 2).unwrap_or_else(|| latin.clone());
    let (link, linked, text_colored) = link_of(props, context);
    let color = if linked && !text_colored {
        Some(look.theme.hyperlink)
    } else {
        run_color(props, defaults, body, look)
    };
    let baseline = said("baseline")
        .and_then(|value| value.trim().parse::<f32>().ok())
        .unwrap_or(0.0);
    let underline = underline(said("u"), linked);
    RunFmt {
        font: latin.clone(),
        font_hansi: latin,
        font_east_asia: east_asia,
        font_cs: complex,
        size,
        size_cs: size,
        bold,
        italic: flag("i"),
        underline,
        strike: said("strike") == Some("sngStrike"),
        dstrike: said("strike") == Some("dblStrike"),
        color,
        vert: if baseline > 0.0 {
            VertAlign::Super
        } else if baseline < 0.0 {
            VertAlign::Sub
        } else {
            VertAlign::Baseline
        },
        caps: said("cap") == Some("all"),
        small_caps: said("cap") == Some("small"),
        spacing: said("spc")
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .map_or(0.0, |value| value / 100.0 * scale),
        kern_from: said("kern")
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value / 100.0),
        href: link,
        highlight: chain
            .iter()
            .find_map(|node| child(*node, DRAWING_NS, "highlight"))
            .and_then(|highlight| look.first_color(highlight)),
        ..RunFmt::default()
    }
}

/// The link of a run with the properties `props`: its web address (a link
/// to another slide only looks like one, it has none in the PDF), whether
/// it has a link, and whether the link keeps the text's colour
/// (`ahyp:hlinkClr val="tx"`, Office 2019; PowerPoint otherwise draws a
/// link in the theme's link colour, over the run's own).
fn link_of(props: Option<Node<'_, '_>>, context: &Context<'_, '_, '_>) -> (Option<String>, bool, bool) {
    let click = props.and_then(|props| child(props, DRAWING_NS, "hlinkClick"));
    let link = click
        .and_then(|click| context.rels.get(attr(click, REL_NS, "id")?))
        .filter(|rel| rel.external)
        .map(|rel| rel.target.clone());
    let linked = click
        .and_then(|click| attr(click, REL_NS, "id"))
        .is_some_and(|id| !id.is_empty());
    let text_colored = click.is_some_and(|click| {
        click
            .descendants()
            .any(|node| node.tag_name().name() == "hlinkClr" && node.attribute("val") == Some("tx"))
    });
    (link, linked, text_colored)
}

/// The colour of a run with the properties `props`: its own; else the
/// shape's style's (or a table style's), which is stronger than its
/// level's `defaults`; else the text colour.
fn run_color(
    props: Option<Node<'_, '_>>,
    defaults: &[Node<'_, '_>],
    body: &Body<'_, '_>,
    look: &Look<'_, '_>,
) -> Option<Rgb> {
    let color_of = |node: &Node<'_, '_>| {
        look.fill(*node, [0; 3])
            .map(|paint| paint.and_then(|paint| paint.rgb()))
    };
    match props.as_ref().and_then(color_of) {
        Some(color) => color,
        None => body
            .font_color
            .or_else(|| defaults.iter().find_map(color_of).flatten())
            .or_else(|| look.theme.scheme("tx1")),
    }
}

/// The underline a run's `u` says, a link's when it says none.
fn underline(said: Option<&str>, linked: bool) -> Option<Underline> {
    match said {
        None | Some("none") => linked.then_some(Underline::Single),
        Some("dbl") => Some(Underline::Double),
        Some("heavy") => Some(Underline::Thick),
        Some(kind) if kind.starts_with("dot") => Some(Underline::Dotted),
        Some(kind) if kind.starts_with("dash") => Some(Underline::Dashed),
        Some(kind) if kind.starts_with("wavy") => Some(Underline::Wave),
        Some(_) => Some(Underline::Single),
    }
}

/// Where the text of a line's items `items` ends, from its left edge.
fn line_end(items: &[Item]) -> f32 {
    items
        .iter()
        .filter_map(|item| match item {
            Item::Text(run) => run.glyphs.last().map(|glyph| run.x + glyph.x + glyph.advance),
            _ => None,
        })
        .fold(0.0, f32::max)
}

/// Moves `items` by `dx`, `dy`.
pub(super) fn translate(items: &mut [Item], dx: f32, dy: f32) {
    for item in items {
        match item {
            Item::Text(run) => {
                run.x += dx;
                run.y += dy;
            }
            Item::Rect { x, y, .. } | Item::Image { x, y, .. } | Item::Link { x, y, .. } | Item::Turn { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Item::Line { x1, y1, x2, y2, .. } => {
                *x1 += dx;
                *x2 += dx;
                *y1 += dy;
                *y2 += dy;
            }
            Item::EndTurn => {}
        }
    }
}
