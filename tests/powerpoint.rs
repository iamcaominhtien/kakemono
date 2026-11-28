//! PowerPoint presentations as input: the sample deck of
//! `tests/data/powerpoint` (written by `make-sample-pptx.py`), and small
//! decks written here for what the sample does not hold.

use kakemono::model::{BlockKind, Document};
use kakemono::{
    ChunkOptions, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions, MarkdownOptions,
    Paper, TypstOptions,
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The sample deck.
const SAMPLE: &str = "powerpoint/sample.pptx";
/// Namespaces of the parts written here.
const P_NS: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";

/// The path of a file of `tests/data`.
fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

/// The document of the sample deck.
fn sample() -> Document {
    Paper::from_bytes(std::fs::read(data(SAMPLE)).unwrap())
        .unwrap()
        .analyze()
        .doc
}

/// The headings of `doc`, as (level, text).
fn headings(doc: &Document) -> Vec<(u8, String)> {
    doc.blocks
        .iter()
        .filter_map(|block| match block.kind {
            BlockKind::Heading { level } => Some((level, block.text())),
            _ => None,
        })
        .collect()
}

/// A deck of the slides `slides` (the XML inside each `p:sld`'s shape
/// tree), with the extra relationships `rels` of the presentation part
/// (id, type, target, external).
fn deck(slides: &[&str], rels: &[(&str, &str, &str, bool)]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    let rel = |id: &str, kind: &str, target: &str, external: bool| {
        let mode = if external { " TargetMode=\"External\"" } else { "" };
        format!("<Relationship Id=\"{id}\" Type=\"{R_NS}/{kind}\" Target=\"{target}\"{mode}/>")
    };
    entries.push((
        "_rels/.rels".into(),
        format!(
            "<Relationships xmlns=\"{PKG_NS}\">{}</Relationships>",
            rel("rId1", "officeDocument", "ppt/presentation.xml", false)
        )
        .into_bytes(),
    ));
    let mut ids = String::new();
    let mut slide_rels = String::new();
    for (index, body) in slides.iter().enumerate() {
        let number = index + 1;
        ids += &format!("<p:sldId id=\"{}\" r:id=\"rIdS{number}\"/>", 255 + number);
        slide_rels += &rel(
            &format!("rIdS{number}"),
            "slide",
            &format!("slides/slide{number}.xml"),
            false,
        );
        entries.push((
            format!("ppt/slides/slide{number}.xml"),
            format!(
                "<p:sld xmlns:p=\"{P_NS}\" xmlns:r=\"{R_NS}\"><p:cSld><p:spTree>{body}</p:spTree></p:cSld></p:sld>"
            )
            .into_bytes(),
        ));
    }
    for (id, kind, target, external) in rels {
        ids += &format!("<p:sldId id=\"9{id}\" r:id=\"{id}\"/>", id = &id[3..]);
        slide_rels += &rel(id, kind, target, *external);
    }
    entries.push((
        "ppt/presentation.xml".into(),
        format!(
            "<p:presentation xmlns:p=\"{P_NS}\" xmlns:r=\"{R_NS}\"><p:sldIdLst>{ids}</p:sldIdLst></p:presentation>"
        )
        .into_bytes(),
    ));
    entries.push((
        "ppt/_rels/presentation.xml.rels".into(),
        format!("<Relationships xmlns=\"{PKG_NS}\">{slide_rels}</Relationships>").into_bytes(),
    ));
    stored_zip(&entries)
}

/// A ZIP archive of `entries`, stored without compression.
fn stored_zip(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, bytes) in entries {
        let offset = out.len() as u32;
        let crc = crc32fast::hash(bytes);
        let header = |signature: u32, central: bool| {
            let mut head = Vec::new();
            head.write_all(&signature.to_le_bytes()).unwrap();
            if central {
                head.write_all(&20u16.to_le_bytes()).unwrap();
            }
            for value in [20u16, 0, 0, 0, 0] {
                head.write_all(&value.to_le_bytes()).unwrap();
            }
            for value in [crc, bytes.len() as u32, bytes.len() as u32] {
                head.write_all(&value.to_le_bytes()).unwrap();
            }
            head.write_all(&(name.len() as u16).to_le_bytes()).unwrap();
            head.write_all(&0u16.to_le_bytes()).unwrap();
            if central {
                for value in [0u16, 0, 0] {
                    head.write_all(&value.to_le_bytes()).unwrap();
                }
                head.write_all(&0u32.to_le_bytes()).unwrap();
                head.write_all(&offset.to_le_bytes()).unwrap();
            }
            head.extend_from_slice(name.as_bytes());
            head
        };
        out.extend(header(0x0403_4b50, false));
        out.extend_from_slice(bytes);
        directory.extend(header(0x0201_4b50, true));
    }
    let start = out.len() as u32;
    out.extend_from_slice(&directory);
    out.write_all(&0x0605_4b50u32.to_le_bytes()).unwrap();
    for value in [0u16, 0, entries.len() as u16, entries.len() as u16] {
        out.write_all(&value.to_le_bytes()).unwrap();
    }
    out.write_all(&(directory.len() as u32).to_le_bytes()).unwrap();
    out.write_all(&start.to_le_bytes()).unwrap();
    out.write_all(&0u16.to_le_bytes()).unwrap();
    out
}

#[test]
fn sample_is_detected_as_pptx() {
    let paper = Paper::from_bytes(std::fs::read(data(SAMPLE)).unwrap()).unwrap();
    assert_eq!(paper.format(), InputFormat::Pptx);
    assert_eq!(paper.format().name(), "pptx");
    assert!(!paper.format().has_pages());
}

#[test]
fn docx_and_xlsx_are_not_taken_for_a_deck() {
    for (name, format) in [
        ("word/sample.docx", InputFormat::Docx),
        ("excel/sample.xlsx", InputFormat::Xlsx),
    ] {
        let paper = Paper::from_bytes(std::fs::read(data(name)).unwrap()).unwrap();
        assert_eq!(paper.format(), format, "{name}");
    }
}

#[test]
fn hidden_slides_are_left_out() {
    let doc = sample();
    assert!(!doc.blocks.iter().any(|block| block.text().contains("hidden")));
    assert!(!headings(&doc).iter().any(|(_, text)| text == "Slide 4"));
}

#[test]
fn title_slide_gives_the_document_title() {
    let doc = sample();
    assert_eq!(doc.meta.title.as_deref(), Some("Coral Sea Survey 2026"));
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title), "{:?}", doc.blocks[0]);
    assert_eq!(doc.blocks[1].text(), "Marine Science Team");
    assert_eq!(doc.blocks[2].text(), "Quarterly review");
    // The title slide is not a section.
    assert!(!headings(&doc).iter().any(|(_, text)| text.contains("Coral Sea Survey")));
}

#[test]
fn sections_are_headings_above_slide_titles() {
    let found = headings(&sample());
    let expected: Vec<(u8, String)> = [
        (1, "Introduction"),
        (2, "Goals of the survey"),
        (2, "Steps of a dive"),
        (2, "Slide 5"),
        // The section's header slide repeats its name: one heading.
        (1, "Results"),
        (2, "Results by site"),
        (2, "Coral cover by year"),
        (2, "How cover is computed"),
        (2, "How a survey runs"),
        (2, "Before and after"),
    ]
    .into_iter()
    .map(|(level, text)| (level, text.to_string()))
    .collect();
    assert_eq!(found, expected);
}

#[test]
fn slides_without_title_are_numbered() {
    let doc = sample();
    let at = doc.blocks.iter().position(|block| block.text() == "Slide 5").unwrap();
    assert!(doc.blocks[at + 1].text().starts_with("A slide without a title"));
}

/// The list items of `doc` between the headings `from` and the next, as
/// (level, marker, text).
fn items_under(doc: &Document, from: &str) -> Vec<(u8, Option<String>, String)> {
    let start = doc.blocks.iter().position(|block| block.text() == from).unwrap() + 1;
    doc.blocks[start..]
        .iter()
        .take_while(|block| !matches!(block.kind, BlockKind::Heading { .. }))
        .filter_map(|block| match &block.kind {
            BlockKind::ListItem { marker, level } => Some((*level, marker.clone(), block.text())),
            _ => None,
        })
        .collect()
}

#[test]
fn bullet_levels_become_list_levels() {
    let items = items_under(&sample(), "Goals of the survey");
    let levels: Vec<u8> = items.iter().map(|(level, _, _)| *level).collect();
    assert_eq!(levels, [0, 1, 2, 0, 0], "{items:?}");
    assert!(items.iter().all(|(_, marker, _)| marker.is_none()));
}

#[test]
fn auto_numbers_keep_their_marker() {
    let doc = sample();
    let items = items_under(&doc, "Steps of a dive");
    let markers: Vec<(u8, String)> = items
        .iter()
        .map(|(level, marker, _)| (*level, marker.clone().unwrap_or_default()))
        .collect();
    let expected: Vec<(u8, String)> = [(0, "3."), (1, "a)"), (1, "b)"), (0, "4."), (0, "5.")]
        .into_iter()
        .map(|(level, marker)| (level, marker.to_string()))
        .collect();
    assert_eq!(markers, expected);
    // A paragraph with `buNone` is not an item.
    let closing = doc
        .blocks
        .iter()
        .find(|block| block.text() == "No bullet on this closing line")
        .unwrap();
    assert!(matches!(closing.kind, BlockKind::Paragraph { .. }));
}

#[test]
fn runs_keep_their_style_and_links() {
    let doc = sample();
    let link = doc
        .blocks
        .iter()
        .flat_map(|block| &block.spans)
        .find(|span| span.text.contains("open data portal"))
        .unwrap();
    assert_eq!(link.href.as_deref(), Some("https://example.org/coral-data"));
    let bold = doc
        .blocks
        .iter()
        .flat_map(|block| &block.spans)
        .find(|span| span.text == "twelve")
        .unwrap();
    assert_ne!(bold.style & kakemono::model::style::BOLD, 0);
}

#[test]
fn placeholder_prompts_are_not_text() {
    let doc = sample();
    assert!(!doc.blocks.iter().any(|block| block.text().contains("Click to")));
}

#[test]
fn footer_date_and_number_are_left_out() {
    let doc = sample();
    let text: String = doc.blocks.iter().map(|block| block.text() + "\n").collect();
    assert!(!text.contains("Confidential"), "{text}");
    assert!(!text.contains("Dive manual"), "{text}");
    assert!(!text.contains("11/28/2026"), "{text}");
    assert!(!text.lines().any(|line| line.trim() == "2"), "{text}");
}

#[test]
fn reading_order_is_the_shape_tree_with_the_title_first() {
    let shape = |id: u32, ph: &str, text: &str| {
        format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"s{id}\"/><p:cNvSpPr/><p:nvPr>{ph}</p:nvPr></p:nvSpPr><p:spPr/>\
             <p:txBody xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:bodyPr/><a:lstStyle/>\
             <a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"
        )
    };
    let tree = [
        shape(2, "", "First box"),
        shape(3, "<p:ph type=\"title\"/>", "The title"),
        shape(4, "", "Second box"),
    ]
    .concat();
    let doc = Paper::from_bytes(deck(&[&tree], &[])).unwrap().analyze().doc;
    let texts: Vec<String> = doc.blocks.iter().map(|block| block.text()).collect();
    assert_eq!(texts, ["The title", "First box", "Second box"]);
}

#[test]
fn missing_and_external_parts_are_skipped() {
    let bytes = deck(
        &["<p:sp/>"],
        &[
            ("rIdX1", "slide", "../../etc/passwd", false),
            ("rIdX2", "slide", "slides/missing.xml", false),
            ("rIdX3", "slide", "https://example.org/slide.xml", true),
        ],
    );
    let paper = Paper::from_bytes(bytes).unwrap();
    assert_eq!(paper.format(), InputFormat::Pptx);
    // The one slide has no text: the deck gives nothing.
    assert!(paper.analyze().doc.blocks.is_empty());
}

#[test]
fn powerpoint_outputs_match_their_snapshot() {
    let paper = Paper::from_bytes(std::fs::read(data(SAMPLE)).unwrap()).unwrap();
    let analysis = paper.analyze();
    let html_opts = HtmlOptions {
        embed_fonts: false,
        ..Default::default()
    };
    let chunks = paper.to_chunks(&analysis, &ChunkOptions::default());
    let outputs: Vec<(&str, Vec<u8>)> = vec![
        ("friendly", paper.to_html(&analysis, &html_opts).html.into_bytes()),
        ("docx", paper.to_docx(&analysis, &DocxOptions::default())),
        (
            "markdown",
            paper
                .to_markdown(&analysis, &MarkdownOptions::default())
                .markdown
                .into_bytes(),
        ),
        (
            "latex",
            paper.to_latex(&analysis, &LatexOptions::default()).latex.into_bytes(),
        ),
        (
            "typst",
            paper.to_typst(&analysis, &TypstOptions::default()).typst.into_bytes(),
        ),
        ("epub", paper.to_epub(&analysis, &EpubOptions::default())),
        (
            "content",
            paper.to_json(&analysis, &JsonOptions::default()).json.into_bytes(),
        ),
        ("chunks", kakemono::chunks_jsonl(&chunks).into_bytes()),
    ];
    let mut text = String::new();
    for (output, bytes) in outputs {
        let bytes = String::from_utf8_lossy(&bytes)
            .replace(env!("CARGO_PKG_VERSION"), "VERSION")
            .into_bytes();
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        text.push_str(&format!("{SAMPLE} {output} {hash}\n"));
    }
    let file = data("snapshots/powerpoint-sample.txt");
    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        old, text,
        "the output of the PowerPoint sample changed; if this is intended, run \
         KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test powerpoint"
    );
}
