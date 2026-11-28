//! The pages of small decks written here: what is drawn where.

use super::draw;
use crate::layout::display::{Item, Page, TextRun};
use crate::render::zip::{Compression, ZipWriter};

const NS: &str = "xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" \
                  xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
                  xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// EMU in a point.
const EMU: f32 = 12_700.0;

/// A deck of 960 × 540 points whose master has the background `master_bg`
/// (a `p:bg`, or nothing) and the shapes `master_tree`, one layout, and
/// the slides `slides`: each the attributes of its `p:sld` and its shapes
/// (after its own `p:bg`, when it has one).
fn deck(master_bg: &str, master_tree: &str, slides: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = ZipWriter::default();
    let rel = |id: &str, kind: &str, target: &str| {
        format!("<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{target}\"/>")
    };
    let rels = |items: &[String]| format!("<Relationships xmlns=\"{PKG}\">{}</Relationships>", items.concat());
    let mut add = |name: &str, text: String| zip.add(name, text.as_bytes(), Compression::Stored);
    add(
        "_rels/.rels",
        rels(&[rel("rId1", "officeDocument", "ppt/presentation.xml")]),
    );
    let ids: String = (1..=slides.len())
        .map(|number| format!("<p:sldId id=\"{}\" r:id=\"rId{number}\"/>", 255 + number))
        .collect();
    add(
        "ppt/presentation.xml",
        format!(
            "<p:presentation {NS}><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"12192000\" cy=\"6858000\"/></p:presentation>"
        ),
    );
    let slide_rels: Vec<String> = (1..=slides.len())
        .map(|number| rel(&format!("rId{number}"), "slide", &format!("slides/slide{number}.xml")))
        .collect();
    add("ppt/_rels/presentation.xml.rels", rels(&slide_rels));
    for (index, (attributes, tree)) in slides.iter().enumerate() {
        let number = index + 1;
        // A slide's own background comes first in its tree.
        let (bg, tree) = match tree.split_once("</p:bg>") {
            Some((bg, tree)) => (format!("{bg}</p:bg>"), tree),
            None => (String::new(), *tree),
        };
        add(
            &format!("ppt/slides/slide{number}.xml"),
            format!("<p:sld {NS} {attributes}><p:cSld>{bg}<p:spTree>{tree}</p:spTree></p:cSld></p:sld>"),
        );
        add(
            &format!("ppt/slides/_rels/slide{number}.xml.rels"),
            rels(&[rel("rId1", "slideLayout", "../slideLayouts/slideLayout1.xml")]),
        );
    }
    add(
        "ppt/slideLayouts/slideLayout1.xml",
        format!("<p:sldLayout {NS}><p:cSld><p:spTree/></p:cSld></p:sldLayout>"),
    );
    add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[rel("rId1", "slideMaster", "../slideMasters/slideMaster1.xml")]),
    );
    add(
        "ppt/slideMasters/slideMaster1.xml",
        format!(
            "<p:sldMaster {NS}><p:cSld>{master_bg}<p:spTree>{master_tree}</p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\"/></p:sldMaster>"
        ),
    );
    zip.finish()
}

/// A shape at `x`, `y` (points), `width` × `height`, with the shape
/// properties `props` besides its frame and the text body `body`.
fn shape(frame: [f32; 4], props: &str, body: &str) -> String {
    let [x, y, width, height] = frame.map(|value| (value * EMU) as i64);
    format!(
        "<p:sp><p:nvSpPr><p:cNvPr id=\"2\" name=\"s\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{width}\" cy=\"{height}\"/></a:xfrm>{props}</p:spPr>{body}</p:sp>"
    )
}

/// A text body of the body properties `body_props` and the paragraphs
/// `paras` (the XML inside each `a:p`).
fn text(body_props: &str, paras: &[&str]) -> String {
    let paras: String = paras.iter().map(|para| format!("<a:p>{para}</a:p>")).collect();
    format!("<p:txBody><a:bodyPr {body_props}/><a:lstStyle/>{paras}</p:txBody>")
}

/// The pages of `data`.
fn pages(data: &[u8]) -> Vec<Page> {
    draw(data).expect("a deck").pages
}

/// The text runs of `page`.
fn runs(page: &Page) -> Vec<&TextRun> {
    page.items
        .iter()
        .filter_map(|item| match item {
            Item::Text(run) => Some(run),
            _ => None,
        })
        .collect()
}

/// The text of `page`'s runs, joined.
fn page_text(page: &Page) -> String {
    runs(page).iter().map(|run| run.text.as_str()).collect()
}

/// The number of pictures (outlines of shapes among them) of `page`.
fn pictures(page: &Page) -> usize {
    page.items
        .iter()
        .filter(|item| matches!(item, Item::Image { .. }))
        .count()
}

#[test]
fn pages_have_the_slide_size() {
    let data = deck("", "", &[("", ""), ("show=\"0\"", ""), ("", "")]);
    let drawn = pages(&data);
    // The hidden slide gives no page.
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|page| page.width == 960.0 && page.height == 540.0));
}

#[test]
fn background_is_inherited_from_the_master() {
    let bg = "<p:bg><p:bgPr><a:solidFill><a:srgbClr val=\"112233\"/></a:solidFill></p:bgPr></p:bg>";
    let own = "<p:bg><p:bgPr><a:solidFill><a:schemeClr val=\"accent2\"/></a:solidFill></p:bgPr></p:bg>";
    let data = deck(bg, "", &[("", ""), ("", "")]);
    let drawn = pages(&data);
    assert!(matches!(
        drawn[0].items.first(),
        Some(Item::Rect {
            width: 960.0,
            height: 540.0,
            color: [0x11, 0x22, 0x33],
            ..
        })
    ));
    // A slide's own background wins.
    let data = deck(bg, "", &[("", ""), ("", own)]);
    let drawn = pages(&data);
    assert!(matches!(
        drawn[1].items.first(),
        Some(Item::Rect {
            color: [0xED, 0x7D, 0x31],
            ..
        })
    ));
}

#[test]
fn master_shapes_hidden_when_show_master_shapes_is_off() {
    let filled = shape(
        [10.0, 10.0, 100.0, 50.0],
        "<a:prstGeom prst=\"rect\"/><a:solidFill><a:srgbClr val=\"FF0000\"/></a:solidFill>",
        "",
    );
    // A placeholder of the master is a template: neither its outline nor
    // its prompt is drawn.
    let prompt = "<p:sp><p:nvSpPr><p:cNvPr id=\"3\" name=\"t\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"100\" cy=\"100\"/></a:xfrm><a:solidFill><a:srgbClr val=\"00FF00\"/></a:solidFill></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:t>Click to edit</a:t></a:r></a:p></p:txBody></p:sp>";
    let data = deck(
        "",
        &format!("{filled}{prompt}"),
        &[("", ""), ("showMasterSp=\"0\"", "")],
    );
    let drawn = pages(&data);
    assert_eq!(pictures(&drawn[0]), 1);
    assert_eq!(page_text(&drawn[0]), "");
    assert_eq!(pictures(&drawn[1]), 0);
}

#[test]
fn slide_number_field_shows_the_number() {
    let field = text("", &["<a:fld id=\"{1}\" type=\"slidenum\"><a:t>‹#›</a:t></a:fld>"]);
    let body = shape([800.0, 500.0, 100.0, 30.0], "", &field);
    let data = deck("", "", &[("", ""), ("", &body)]);
    assert_eq!(page_text(&pages(&data)[1]), "2");
}

#[test]
fn bullets_hang_at_their_indent() {
    let para = "<a:pPr marL=\"457200\" indent=\"-228600\"><a:buFont typeface=\"Arial\"/><a:buChar char=\"•\"/></a:pPr><a:r><a:rPr sz=\"2000\"/><a:t>Item</a:t></a:r>";
    let body = shape(
        [100.0, 100.0, 400.0, 100.0],
        "",
        &text("lIns=\"0\" tIns=\"0\"", &[para]),
    );
    let drawn = pages(&deck("", "", &[("", &body)]));
    let found = runs(&drawn[0]);
    let bullet = found.iter().find(|run| run.text == "•").expect("a bullet");
    let item = found.iter().find(|run| run.text.starts_with("Item")).expect("the text");
    // The bullet at marL + indent, the text at marL.
    assert!((bullet.x - 118.0).abs() < 0.5, "{}", bullet.x);
    assert!((item.x - 136.0).abs() < 0.5, "{}", item.x);
}

#[test]
fn lines_are_one_point_two_times_their_size() {
    let para = "<a:r><a:rPr sz=\"2000\"/><a:t>Line</a:t></a:r>";
    let body = shape([100.0, 100.0, 400.0, 200.0], "", &text("", &[para, para]));
    let drawn = pages(&deck("", "", &[("", &body)]));
    let found = runs(&drawn[0]);
    assert_eq!(found.len(), 2);
    assert!(
        (found[1].y - found[0].y - 24.0).abs() < 0.01,
        "{} {}",
        found[0].y,
        found[1].y
    );
}

#[test]
fn autofit_text_stays_in_its_frame() {
    let para =
        "<a:r><a:rPr sz=\"4000\"/><a:t>A long line of text that PowerPoint shrank to fit the box it is in</a:t></a:r>";
    let fit = "<a:normAutofit fontScale=\"50000\" lnSpcReduction=\"20000\"/>";
    let body = format!("<p:txBody><a:bodyPr>{fit}</a:bodyPr><a:lstStyle/><a:p>{para}</a:p></p:txBody>");
    let frame = [100.0, 100.0, 300.0, 80.0];
    let drawn = pages(&deck("", "", &[("", &shape(frame, "", &body))]));
    let found = runs(&drawn[0]);
    assert!(!found.is_empty());
    for run in found {
        assert!((run.size - 20.0).abs() < 0.01, "{}", run.size);
        assert!(run.y > frame[1] && run.y < frame[1] + frame[3], "{}", run.y);
        let end = run.x + run.glyphs.last().map_or(0.0, |glyph| glyph.x + glyph.advance);
        assert!(run.x >= frame[0] && end <= frame[0] + frame[2], "{} {end}", run.x);
    }
}

#[test]
fn vertical_text_is_turned_about_its_frame() {
    let para = "<a:r><a:t>Up</a:t></a:r>";
    let body = shape([100.0, 100.0, 40.0, 200.0], "", &text("vert=\"vert270\"", &[para]));
    let drawn = pages(&deck("", "", &[("", &body)]));
    let turn = drawn[0].items.iter().find_map(|item| match item {
        Item::Turn { angle_deg, x, y } => Some((*angle_deg, *x, *y)),
        _ => None,
    });
    assert_eq!(turn, Some((270.0, 120.0, 200.0)));
    assert!(matches!(drawn[0].items.last(), Some(Item::EndTurn)));
}

#[test]
fn group_with_empty_child_extent_is_not_scaled() {
    let inner = shape(
        [50.0, 60.0, 200.0, 40.0],
        "",
        &text("lIns=\"0\" tIns=\"0\"", &["<a:r><a:t>In</a:t></a:r>"]),
    );
    let off = |value: f32| (value * EMU) as i64;
    let group = format!(
        "<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"5\" name=\"g\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/><a:chOff x=\"{}\" y=\"{}\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>{inner}</p:grpSp>",
        off(50.0),
        off(60.0),
        off(400.0),
        off(300.0),
        off(50.0),
        off(60.0)
    );
    let drawn = pages(&deck("", "", &[("", &group)]));
    let found = runs(&drawn[0]);
    assert!((found[0].x - 50.0).abs() < 0.01, "{}", found[0].x);
}

#[test]
fn a_fill_that_is_not_seen_draws_nothing() {
    let clear = "<a:prstGeom prst=\"rect\"/><a:solidFill><a:srgbClr val=\"FFFFFF\"><a:alpha val=\"0\"/></a:srgbClr></a:solidFill><a:ln><a:noFill/></a:ln>";
    let half = "<a:prstGeom prst=\"rect\"/><a:solidFill><a:srgbClr val=\"FFFFFF\"><a:alpha val=\"50000\"/></a:srgbClr></a:solidFill>";
    let body = format!(
        "{}{}",
        shape([0.0, 0.0, 960.0, 540.0], clear, ""),
        shape([0.0, 0.0, 10.0, 10.0], half, "")
    );
    let drawn = pages(&deck("", "", &[("", &body)]));
    assert_eq!(pictures(&drawn[0]), 1);
}
