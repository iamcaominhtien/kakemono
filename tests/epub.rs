//! EPUB books as input: the sample book of `tests/data/epub` (written by
//! pandoc from `tests/data/word/sample.md`, the source of the sample Word
//! document), small books written here for what pandoc does not write, and
//! the LIGO paper made into a book by kakemono and read back.

use kakemono::model::{Block, BlockKind, Document, FloatKind};
use kakemono::{
    Analysis, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions, MarkdownOptions, Mode,
    Paper,
};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The sample book.
const SAMPLE_EPUB: &str = "epub/sample.epub";
/// The same document as a Word file.
const SAMPLE_DOCX: &str = "word/sample.docx";
/// The LIGO paper, the one test PDF in the repository.
const LIGO: &str = "1602.03837v1.pdf";

/// The path of a file of `tests/data`.
fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

/// Loads and analyses a file of `tests/data`.
fn load(name: &str) -> (Paper, Analysis) {
    let paper = Paper::from_bytes(std::fs::read(data(name)).unwrap()).unwrap();
    let analysis = paper.analyze();
    (paper, analysis)
}

/// The block whose text starts with `start`.
fn block<'a>(doc: &'a Document, start: &str) -> &'a Block {
    doc.blocks
        .iter()
        .find(|block| block.text().starts_with(start))
        .unwrap_or_else(|| panic!("no block starting with {start:?}"))
}

/// The link of the first span of `block` whose text contains `text`.
fn href<'a>(block: &'a Block, text: &str) -> Option<&'a str> {
    block
        .spans
        .iter()
        .find(|span| span.text.contains(text))
        .unwrap_or_else(|| panic!("no span {text:?} in {:?}", block.text()))
        .href
        .as_deref()
}

/// An EPUB of the content documents `chapters` (name, XHTML body; a name
/// ending in `.svg` is an SVG document, given whole), with the style
/// sheet `css` and the files `files`.
fn book(chapters: &[(&str, &str)], css: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        ("mimetype".into(), b"application/epub+zip".to_vec()),
        (
            "META-INF/container.xml".into(),
            br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.to_vec(),
        ),
    ];
    let manifest: String = chapters
        .iter()
        .map(|(name, _)| match name.strip_suffix(".svg") {
            Some(id) => format!(r#"<item id="{id}" href="{name}" media-type="image/svg+xml"/>"#),
            None => format!(r#"<item id="{name}" href="{name}.xhtml" media-type="application/xhtml+xml"/>"#),
        })
        .collect();
    let spine: String = chapters
        .iter()
        .map(|(name, _)| format!(r#"<itemref idref="{}"/>"#, name.trim_end_matches(".svg")))
        .collect();
    entries.push((
        "OEBPS/content.opf".into(),
        format!(
            r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>A Small Book</dc:title><dc:creator>Ann Author</dc:creator></metadata><manifest>{manifest}<item id="css" href="style.css" media-type="text/css"/></manifest><spine>{spine}</spine></package>"#
        )
        .into_bytes(),
    ));
    for (name, body) in chapters {
        if name.ends_with(".svg") {
            entries.push((format!("OEBPS/{name}"), body.as_bytes().to_vec()));
            continue;
        }
        entries.push((
            format!("OEBPS/{name}.xhtml"),
            format!(
                r#"<?xml version="1.0" encoding="utf-8"?><!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>{name}</title><link rel="stylesheet" href="style.css"/></head><body>{body}</body></html>"#
            )
            .into_bytes(),
        ));
    }
    entries.push(("OEBPS/style.css".into(), css.as_bytes().to_vec()));
    for (name, bytes) in files {
        entries.push((format!("OEBPS/{name}"), bytes.to_vec()));
    }
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
fn epub_sample_has_its_structure() {
    let (paper, analysis) = load(SAMPLE_EPUB);
    let doc = &analysis.doc;
    assert_eq!(paper.format(), InputFormat::Epub);
    assert_eq!(paper.format().name(), "epub");
    assert!(!paper.format().has_pages());
    assert_eq!(paper.page_count(), 0);
    assert!(paper.renderer().is_none());
    assert_eq!(
        doc.meta.title.as_deref(),
        Some("Convergence of Gradient Descent on Smooth Functions")
    );
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title));
    assert!(matches!(doc.blocks[1].kind, BlockKind::Authors));
    assert!(matches!(
        block(doc, "We study").kind,
        BlockKind::Paragraph { in_abstract: true }
    ));
    let intro = block(doc, "1 Introduction");
    assert!(matches!(intro.kind, BlockKind::Heading { level: 1 }));
    assert_eq!(intro.anchor.as_deref(), Some("sec-1"));

    // MathML: a displayed formula, and formulas in running text.
    let equation = doc
        .blocks
        .iter()
        .find(|block| matches!(block.kind, BlockKind::Equation { image: None, .. }))
        .expect("an equation");
    assert!(equation.math.is_some());
    let item = block(doc, "1. A tight bound");
    assert!(matches!(&item.kind, BlockKind::ListItem { marker: Some(marker), level: 0 } if marker == "1."));
    assert!(item.spans.iter().any(|span| span.math.is_some()));

    // A citation, a note (moved after its paragraph, without its link
    // back), a web address, a link to another content document.
    assert_eq!(href(block(doc, "Let "), "1"), Some("#ref-1"));
    let note_paragraph = block(doc, "The method is old");
    assert_eq!(href(note_paragraph, "1"), Some("#fn-1"));
    assert_eq!(href(note_paragraph, "the textbook"), Some("https://example.org/book"));
    assert_eq!(href(block(doc, "A second bullet"), "the introduction"), Some("#sec-1"));
    let note_at = doc
        .blocks
        .iter()
        .position(|block| block.text().starts_with("It goes back"))
        .unwrap();
    assert!(matches!(&doc.blocks[note_at].kind, BlockKind::Footnote { marker } if marker == "1"));
    assert!(doc.blocks[note_at - 1].text().starts_with("The method is old"));

    // The table with its caption, the figure with its caption and size.
    let table = block(doc, "Table 1:");
    assert!(matches!(
        &table.kind,
        BlockKind::Figure { kind: FloatKind::Table, label: Some(label), .. } if label == "Table 1"
    ));
    let grid = table.table.as_ref().expect("a rebuilt table");
    assert_eq!((grid.n_cols, grid.header_rows, grid.rows.len()), (3, 1, 3));
    let figure = block(doc, "Figure 1:");
    let BlockKind::Figure {
        kind: FloatKind::Figure,
        image: Some(image),
        ..
    } = &figure.kind
    else {
        panic!("the figure has a picture")
    };
    assert!((image.rect.width() - 216.0).abs() < 1.0, "3 inches wide");
    assert!(doc.pictures[0].data.starts_with(b"\x89PNG"));

    let code = block(doc, "def step");
    assert!(matches!(code.kind, BlockKind::Code));
    assert_eq!(code.rows.len(), 2);
    assert!(matches!(block(doc, "[2] S. Boyd").kind, BlockKind::Reference { .. }));
}

#[test]
fn epub_sample_reads_like_the_docx() {
    let summary = |name: &str| -> Vec<String> {
        load(name)
            .1
            .doc
            .blocks
            .iter()
            .filter(|block| !matches!(block.kind, BlockKind::Equation { .. }))
            .map(|block| format!("{:?} {} {:?}", block.kind, block.text(), block.anchor))
            .collect()
    };
    assert_eq!(summary(SAMPLE_EPUB), summary(SAMPLE_DOCX));
}

#[test]
fn every_output_can_be_written_from_epub() {
    let (paper, analysis) = load(SAMPLE_EPUB);
    for mode in [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly] {
        let opts = HtmlOptions {
            mode,
            ..Default::default()
        };
        let html = paper.to_html(&analysis, &opts).html;
        assert!(html.contains("Convergence of Gradient Descent"), "{mode:?}");
    }
    assert!(paper.to_docx(&analysis, &DocxOptions::default()).starts_with(b"PK"));
    assert!(
        paper
            .to_markdown(&analysis, &MarkdownOptions::default())
            .markdown
            .contains("def step(x, grad, eta):")
    );
    assert!(
        paper
            .to_latex(&analysis, &LatexOptions::default())
            .latex
            .contains("\\section")
    );
    assert!(paper.to_epub(&analysis, &EpubOptions::default()).starts_with(b"PK"));
    let json = paper.to_json(&analysis, &JsonOptions::default()).json;
    assert!(json.contains(r#""format": "epub""#));
    assert!(paper.to_pdf().unwrap().starts_with(b"%PDF"));
    // The colophons name the book, not Word.
    let friendly = paper.to_html(&analysis, &HtmlOptions::default()).html;
    assert!(friendly.contains("Converted from EPUB by kakemono"));
    assert!(!friendly.contains("from Word"));
    let latex = paper.to_latex(&analysis, &LatexOptions::default()).latex;
    assert!(latex.contains("Converted from an EPUB book"));
}

#[test]
fn book_markup_gives_styles_lists_tables_and_notes() {
    let chapter_one = r#"
        <section><h2>One</h2>
        <p>Plain <span class="it">styled</span> and <b>bold</b>&nbsp;text&mdash;with a
           <a epub:type="noteref" href="two.xhtml#n1" id="r1">*</a> note and a
           <a href="two.xhtml#later">link</a>.</p>
        <span epub:type="pagebreak" id="p7" title="7"/>
        <ol start="3" type="a"><li>third<ol><li>inner</li></ol></li><li value="9">ninth</li></ol>
        <ul class="bare"><li>[1] Not a list item.</li></ul>
        <table><tr><th>A</th><th>B</th><th>C</th></tr>
          <tr><td rowspan="2">tall</td><td colspan="2">wide</td></tr>
          <tr><td>b3</td><td>c3</td></tr></table>
        <p class="gone">Hidden.</p>
        </section>"#;
    let chapter_two = r#"
        <h2 id="later">Two</h2>
        <aside epub:type="footnote" id="n1"><p><a href="one.xhtml#r1">*</a> The note.</p></aside>"#;
    let data = book(
        &[("one", chapter_one), ("two", chapter_two)],
        ".it { font-style: italic } ul.bare { list-style: none } .gone { display: none }",
        &[],
    );
    let paper = Paper::from_bytes(data).unwrap();
    let doc = paper.analyze().doc;
    // The metadata gives the title and the author; the headings start at
    // level 1.
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title));
    assert_eq!(doc.blocks[0].text(), "A Small Book");
    assert_eq!(doc.blocks[1].text(), "Ann Author");
    assert!(matches!(block(&doc, "One").kind, BlockKind::Heading { level: 1 }));

    let para = block(&doc, "Plain");
    assert_eq!(
        para.text().split(" note").next(),
        Some("Plain styled and bold\u{a0}text—with a 1")
    );
    let styled = para.spans.iter().find(|span| span.text == "styled").unwrap();
    assert_eq!(styled.style, kakemono::model::ITALIC);
    assert_eq!(href(para, "1"), Some("#fn-1"));
    let later = block(&doc, "Two").anchor.clone().unwrap();
    assert_eq!(href(para, "link"), Some(format!("#{later}").as_str()));
    let note = doc
        .blocks
        .iter()
        .position(|block| block.text() == "The note.")
        .expect("the note");
    assert!(matches!(&doc.blocks[note].kind, BlockKind::Footnote { marker } if marker == "1"));
    assert!(doc.blocks[note - 1].text().starts_with("Plain"));

    let items: Vec<(String, u8)> = doc
        .blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::ListItem { level, .. } => Some((block.text(), *level)),
            _ => None,
        })
        .collect();
    assert_eq!(
        items,
        [
            ("c. third".to_string(), 0),
            ("1. inner".to_string(), 1),
            ("i. ninth".to_string(), 0)
        ]
    );
    assert!(matches!(block(&doc, "[1] Not").kind, BlockKind::Paragraph { .. }));

    let table = doc
        .blocks
        .iter()
        .find_map(|block| block.table.as_ref())
        .expect("a table");
    assert_eq!((table.n_cols, table.header_rows), (3, 1));
    assert_eq!(table.rows[1][0].rowspan, 2);
    assert_eq!(table.rows[1][1].colspan, 2);
    assert_eq!(table.rows[2][0].col, 1);
    assert!(doc.blocks.iter().all(|block| !block.text().contains("Hidden")));
}

#[test]
fn book_pictures_come_from_the_archive() {
    let png = std::fs::read(data("word/curve.png")).unwrap();
    let body = r#"<h1>A Small Book</h1>
        <figure><img src="images/curve%20one.png" alt="A curve" width="200"/>
        <figcaption>Figure 1: A curve.</figcaption></figure>
        <p><svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg></p>"#;
    let data = book(&[("one", body)], "", &[("images/curve one.png", &png)]);
    let doc = Paper::from_bytes(data).unwrap().analyze().doc;
    assert!(
        matches!(doc.blocks[0].kind, BlockKind::Title),
        "the heading is the title"
    );
    let figure = block(&doc, "Figure 1:");
    let BlockKind::Figure { image: Some(image), .. } = &figure.kind else {
        panic!("a figure with its picture")
    };
    assert!((image.rect.width() - 150.0).abs() < 0.5);
    assert!(image.rect.height() > 0.0, "the height follows the picture's shape");
    assert_eq!(doc.pictures.len(), 2, "the picture file and the drawing");
}

#[test]
fn svg_pages_of_the_spine_are_pictures() {
    let png = std::fs::read(data("word/curve.png")).unwrap();
    let linked = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="300" height="200"><image width="300" height="200" xlink:href="curve.png"/></svg>"#;
    let drawn =
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>"#;
    let data = book(
        &[("page1.svg", linked), ("page2.svg", drawn)],
        "",
        &[("curve.png", &png)],
    );
    let doc = Paper::from_bytes(data).unwrap().analyze().doc;
    let figures = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Figure { image: Some(_), .. }))
        .count();
    assert_eq!(figures, 2, "one figure per page");
    assert!(doc.pictures[0].data.starts_with(b"\x89PNG"));
}

#[test]
fn ligo_survives_a_round_trip_through_epub() {
    let (pdf_paper, pdf) = load(LIGO);
    let epub = pdf_paper.to_epub(&pdf, &EpubOptions::default());
    let paper = Paper::from_bytes(epub).unwrap();
    assert_eq!(paper.format(), InputFormat::Epub);
    let doc = paper.analyze().doc;
    let headings = |doc: &Document| -> Vec<String> {
        doc.blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Heading { .. }))
            .map(Block::text)
            .collect()
    };
    assert_eq!(headings(&doc), headings(&pdf.doc));
    assert_eq!(doc.meta.title, pdf.doc.meta.title);
    // Formulas in running text come back as MathML; the displayed
    // formula, a picture in the book, as a picture.
    assert!(
        doc.blocks
            .iter()
            .any(|block| block.spans.iter().any(|span| span.math.is_some()))
    );
    assert!(!doc.pictures.is_empty());
    assert!(
        block(&doc, "In 1916, the year after")
            .spans
            .iter()
            .any(|span| span.href.is_some())
    );
}

#[test]
fn epub_outputs_match_their_snapshot() {
    let (paper, analysis) = load(SAMPLE_EPUB);
    let html_opts = HtmlOptions {
        embed_fonts: false,
        ..Default::default()
    };
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
        ("epub", paper.to_epub(&analysis, &EpubOptions::default())),
        (
            "content",
            paper.to_json(&analysis, &JsonOptions::default()).json.into_bytes(),
        ),
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
        text.push_str(&format!("{SAMPLE_EPUB} {output} {hash}\n"));
    }
    let file = data("snapshots/epub-sample.txt");
    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        old, text,
        "the output of the EPUB sample changed; if this is intended, run \
         KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test epub"
    );
}
