//! End-to-end test on a small book generated on the fly: a title page, a
//! table of contents, two chapters with sections, a small picture set
//! beside a paragraph and a link annotation.

use kakemono::model::{Block, BlockKind};
use kakemono::{DocxOptions, HtmlOptions, Mode, Paper};

/// The text lines of the contents page: (font, size, x, y, text). Leaders
/// for the chapters, a wide gap for the sections, one title that wraps.
const CONTENTS: &[(&str, u32, u32, u32, &str)] = &[
    ("F2", 18, 72, 700, "Contents"),
    (
        "F2",
        11,
        72,
        650,
        "1. Getting Started . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . 3",
    ),
    ("F1", 10, 90, 634, "First Steps"),
    ("F1", 10, 90, 618, "Next Steps"),
    (
        "F1",
        10,
        90,
        602,
        "A Rather Long Section Title That Needs More Than One Line To Be Set In",
    ),
    ("F1", 10, 90, 590, "Full"),
    (
        "F2",
        11,
        72,
        566,
        "2. Going Further . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . . 4",
    ),
    ("F1", 10, 90, 550, "Deeper Ideas"),
    ("F1", 10, 90, 534, "Summary"),
];

/// Page numbers of the section entries, set at the right edge.
const SECTION_PAGES: &[(u32, &str)] = &[(634, "3"), (618, "3"), (590, "3"), (550, "4"), (534, "4")];

/// A paragraph of running text (several lines of the body font).
fn paragraph(x: u32, top: u32, lines: &[&str]) -> String {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| format!("BT /F1 10 Tf {x} {} Td ({line}) Tj ET\n", top - 12 * i as u32))
        .collect()
}

/// A line of text in font `font` of size `size`.
fn text(font: &str, size: u32, x: u32, y: u32, line: &str) -> String {
    format!("BT /{font} {size} Tf {x} {y} Td ({line}) Tj ET\n")
}

/// The pages of the book: content streams and link annotations.
fn pages() -> Vec<(String, String)> {
    let title = text("F2", 24, 200, 600, "A Small Book") + &text("F1", 12, 260, 560, "Jane Doe");

    let mut contents = String::new();
    for &(font, size, x, y, line) in CONTENTS {
        contents.push_str(&text(font, size, x, y, line));
    }
    for &(y, page) in SECTION_PAGES {
        contents.push_str(&text("F1", 10, 534, y, page));
    }

    let mut first = text("F2", 22, 72, 700, "Getting Started");
    first.push_str(&text("F2", 14, 72, 660, "First Steps"));
    first.push_str(&paragraph(
        72,
        640,
        &[
            "The first steps of any journey are the hardest ones, and this short chapter",
            "walks through them slowly, one at a time, with a link to the web for more.",
            "Each step is explained before the next one starts, so that nothing is lost.",
        ],
    ));
    first.push_str(&text("F2", 14, 72, 590, "Next Steps"));
    first.push_str(&paragraph(
        72,
        570,
        &[
            "Once the first steps are behind you, the next ones come more easily and the",
            "pace can grow a little, though there is never any need to hurry through them.",
        ],
    ));
    first.push_str(&text("F2", 11, 72, 530, "A small aside"));
    first.push_str(&paragraph(
        72,
        512,
        &[
            "An aside is a remark that stands a little apart from the rest of the text",
            "and that a reader in a hurry may skip without missing anything important.",
        ],
    ));
    // A small picture with a note beside it.
    first.push_str("q 40 0 0 40 72 420 cm /Im1 Do Q\n");
    first.push_str(&paragraph(
        124,
        452,
        &[
            "This note stands beside its icon, as notes do in many books,",
            "and its lines keep to the right of the picture all the way down.",
            "A third line makes the note as tall as the icon next to it.",
        ],
    ));
    first.push_str(&text(
        "F2",
        14,
        72,
        380,
        "A Rather Long Section Title That Needs More Than One Line To Be Set In Full",
    ));
    first.push_str(&paragraph(
        72,
        360,
        &[
            "A section with a long title shows that an entry of the contents can wrap",
            "onto a second line and still be found and linked to its heading here.",
        ],
    ));
    // The words "the web" on the second line of the first paragraph link out.
    let link = "<< /Type /Annot /Subtype /Link /Rect [314 625 351 637] /Border [0 0 0] \
                /A << /S /URI /URI (https://example.org/more) >> >>"
        .to_string();

    let mut second = text("F2", 22, 72, 700, "Going Further");
    second.push_str(&text("F2", 14, 72, 660, "Deeper Ideas"));
    second.push_str(&paragraph(
        72,
        640,
        &[
            "Going further means looking at the ideas behind the steps, which this",
            "chapter does with a few short examples and a little more detail than before.",
        ],
    ));
    second.push_str(&text("F2", 14, 72, 600, "Summary"));
    second.push_str(&paragraph(
        72,
        580,
        &[
            "The summary of the book is short: start slowly, keep going, and look",
            "further whenever the steps start to feel easy and familiar to you.",
        ],
    ));

    vec![
        (title, String::new()),
        (contents, String::new()),
        (first, link),
        (second, String::new()),
    ]
}

/// A PDF of `pages` (content stream, annotations dictionary or empty), with
/// the fonts F1 (Helvetica) and F2 (Helvetica-Bold) and a small picture
/// /Im1.
fn build_pdf(pages: &[(String, String)]) -> Vec<u8> {
    // Objects: 1 catalog, 2 pages, 3 F1, 4 F2, 5 Im1, then per page: page,
    // contents, and its annotation when it has one.
    let mut objects: Vec<String> = vec![
        String::new(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string(),
        {
            let pixels = "FF6600 3366FF 33CC33 FFCC00 993399 00CCCC 666666 CC3333 FFFFFF>";
            format!(
                "<< /Type /XObject /Subtype /Image /Width 3 /Height 3 /ColorSpace /DeviceRGB \
                 /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{pixels}\nendstream",
                pixels.len() + 1
            )
        },
    ];
    let mut kids = Vec::new();
    for (content, annot) in pages {
        let page_num = objects.len() + 1;
        kids.push(format!("{page_num} 0 R"));
        let annots = if annot.is_empty() {
            String::new()
        } else {
            format!(" /Annots [{} 0 R]", page_num + 2)
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
             /Resources << /Font << /F1 3 0 R /F2 4 0 R >> /XObject << /Im1 5 0 R >> >>{annots} >>",
            page_num + 1
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content));
        if !annot.is_empty() {
            objects.push(annot.clone());
        }
    }
    objects[0] = "<< /Type /Catalog /Pages 2 0 R >>".to_string();
    objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len());

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref
    ));
    pdf.into_bytes()
}

/// The book, analysed.
fn book() -> (Paper, kakemono::Analysis) {
    let paper = Paper::from_bytes(build_pdf(&pages())).expect("valid pdf");
    let analysis = paper.analyze();
    (paper, analysis)
}

/// The (level, page, title) of every contents entry.
fn entries(blocks: &[Block]) -> Vec<(u8, String, String)> {
    blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::ContentsEntry { level, page } => Some((*level, page.clone(), block.text())),
            _ => None,
        })
        .collect()
}

#[test]
fn contents_entries_have_levels_pages_and_links() {
    let (_, analysis) = book();
    let blocks = &analysis.doc.blocks;
    let found = entries(blocks);
    let expected: Vec<(u8, String, String)> = [
        (1, "3", "1. Getting Started"),
        (2, "3", "First Steps"),
        (2, "3", "Next Steps"),
        (
            2,
            "3",
            "A Rather Long Section Title That Needs More Than One Line To Be Set In Full",
        ),
        (1, "4", "2. Going Further"),
        (2, "4", "Deeper Ideas"),
        (2, "4", "Summary"),
    ]
    .iter()
    .map(|&(level, page, title)| (level, page.to_string(), title.to_string()))
    .collect();
    assert_eq!(found, expected);

    // Each entry links to the heading it names.
    for block in blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::ContentsEntry { .. }))
    {
        let href = block
            .spans
            .iter()
            .find_map(|span| span.href.clone())
            .expect("linked entry");
        let target = blocks
            .iter()
            .find(|other| other.anchor.as_deref() == href.strip_prefix('#'))
            .expect("existing heading");
        assert!(matches!(target.kind, BlockKind::Heading { .. }), "{href}");
    }
}

#[test]
fn heading_levels_follow_the_contents() {
    let (_, analysis) = book();
    let levels: Vec<(u8, String)> = analysis
        .doc
        .blocks
        .iter()
        .filter_map(|block| match block.kind {
            BlockKind::Heading { level } => Some((level, block.text())),
            _ => None,
        })
        .filter(|(_, text)| text != "Contents" && text != "A Small Book")
        .collect();
    let expected: Vec<(u8, String)> = [
        (1, "Getting Started"),
        (2, "First Steps"),
        (2, "Next Steps"),
        (3, "A small aside"),
        (
            2,
            "A Rather Long Section Title That Needs More Than One Line To Be Set In Full",
        ),
        (1, "Going Further"),
        (2, "Deeper Ideas"),
        (2, "Summary"),
    ]
    .iter()
    .map(|&(level, text)| (level, text.to_string()))
    .collect();
    assert_eq!(levels, expected);
}

#[test]
fn link_annotations_become_hrefs() {
    let (_, analysis) = book();
    let linked: Vec<&str> = analysis
        .doc
        .blocks
        .iter()
        .flat_map(|block| &block.spans)
        .filter(|span| span.href.as_deref() == Some("https://example.org/more"))
        .map(|span| span.text.trim())
        .collect();
    assert_eq!(linked, ["the web"]);
}

#[test]
fn renders_contents_and_side_pictures() {
    let (paper, analysis) = book();
    for mode in [Mode::Medium, Mode::Friendly] {
        let html = paper
            .to_html(
                &analysis,
                &HtmlOptions {
                    mode,
                    ..Default::default()
                },
            )
            .html;
        assert!(html.contains("class=\"toc toc1\""), "{mode:?}");
        assert!(html.contains("<span class=\"toc-p\">4</span>"), "{mode:?}");
        assert!(
            html.contains("<div class=\"beside\"><figure class=\"float gfx side-l\""),
            "{mode:?}"
        );
        assert!(html.contains("href=\"https://example.org/more\""), "{mode:?}");
    }
    let docx = paper.to_docx(&analysis, &DocxOptions::default());
    assert!(docx.starts_with(b"PK"));
}
