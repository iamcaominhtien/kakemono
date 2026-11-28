//! PowerPoint presentations as input: the sample deck of
//! `tests/data/powerpoint` (written by `make-sample-pptx.py`), and small
//! decks written here for what the sample does not hold.

use kakemono::model::{BlockKind, Document};
use kakemono::{InputFormat, Paper};
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
    // The sample's 4th slide is hidden: slide 5 follows slide 3.
    let numbers: Vec<String> = headings(&doc)
        .into_iter()
        .filter(|(_, text)| text.starts_with("Slide "))
        .map(|(_, text)| text)
        .collect();
    assert!(!numbers.contains(&"Slide 4".to_string()), "{numbers:?}");
    assert!(numbers.contains(&"Slide 5".to_string()), "{numbers:?}");
    assert!(!doc.blocks.iter().any(|block| block.text().contains("hidden")));
}

#[test]
fn sections_head_their_slides() {
    let doc = sample();
    let headings = headings(&doc);
    assert_eq!(headings.first(), Some(&(1, "Introduction".to_string())), "{headings:?}");
    assert!(headings.contains(&(1, "Results".to_string())), "{headings:?}");
    assert!(
        headings.iter().filter(|(level, _)| *level == 2).count() >= 9,
        "{headings:?}"
    );
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
    let doc = paper.analyze().doc;
    assert_eq!(headings(&doc), vec![(1, "Slide 1".to_string())]);
}
