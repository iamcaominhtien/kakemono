//! Word documents as input: the sample `.docx` and `.doc` of
//! `tests/data/word` (written from `sample.md` by pandoc, and saved as
//! `.doc` by LibreOffice), and the LIGO paper made into a Word document by
//! kakemono and read back.

use kakemono::model::{Block, BlockKind, Document, FloatKind, Span};
use kakemono::{
    Analysis, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions, MarkdownOptions, Mode,
    Paper,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The sample Word document.
const SAMPLE_DOCX: &str = "word/sample.docx";
/// The same document saved as a Word 97–2003 file.
const SAMPLE_DOC: &str = "word/sample.doc";
/// Lists inside lists, written by pandoc from `lists.md`.
const LISTS_DOCX: &str = "word/lists.docx";
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

/// The blocks of kind `kind` (compared by variant).
fn blocks_of<'a>(doc: &'a Document, pred: impl Fn(&BlockKind) -> bool + 'a) -> impl Iterator<Item = &'a Block> + 'a {
    doc.blocks.iter().filter(move |block| pred(&block.kind))
}

/// The span of `block` whose text is `text`.
fn span<'a>(block: &'a Block, text: &str) -> &'a Span {
    block
        .spans
        .iter()
        .find(|span| span.text.contains(text))
        .unwrap_or_else(|| panic!("no span {text:?} in {:?}", block.text()))
}

/// The block whose text starts with `start`.
fn block<'a>(doc: &'a Document, start: &str) -> &'a Block {
    doc.blocks
        .iter()
        .find(|block| block.text().starts_with(start))
        .unwrap_or_else(|| panic!("no block starting with {start:?}"))
}

#[test]
fn docx_sample_has_its_structure() {
    let (paper, analysis) = load(SAMPLE_DOCX);
    let doc = &analysis.doc;
    assert_eq!(paper.format(), InputFormat::Docx);
    assert_eq!(paper.page_count(), 0);
    assert!(paper.renderer().is_none());
    assert!(doc.pages.is_empty());
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

    // The displayed formula has its structure and no picture.
    let equation = blocks_of(doc, |kind| matches!(kind, BlockKind::Equation { .. }))
        .next()
        .expect("an equation");
    assert!(matches!(equation.kind, BlockKind::Equation { image: None, .. }));
    assert!(equation.math.is_some());
    // A formula in running text is one span with its structure.
    let item = block(doc, "1. A tight bound");
    assert!(matches!(&item.kind, BlockKind::ListItem { marker: Some(marker), level: 0 } if marker == "1."));
    assert!(item.spans.iter().any(|span| span.math.is_some()));

    // Links: a citation, a note mark, a link inside the document, a web
    // address.
    assert_eq!(span(block(doc, "Let "), "1").href.as_deref(), Some("#ref-1"));
    let note_paragraph = block(doc, "The method is old");
    assert_eq!(span(note_paragraph, "1").href.as_deref(), Some("#fn-1"));
    assert_eq!(
        span(note_paragraph, "the textbook").href.as_deref(),
        Some("https://example.org/book")
    );
    assert_eq!(
        span(block(doc, "A second bullet"), "the introduction").href.as_deref(),
        Some("#sec-1")
    );
    let note = block(doc, "It goes back to Cauchy");
    assert!(matches!(&note.kind, BlockKind::Footnote { marker } if marker == "1"));
    assert_eq!(note.anchor.as_deref(), Some("fn-1"));

    // The table and the figure, with their captions.
    let table = blocks_of(doc, |kind| {
        matches!(
            kind,
            BlockKind::Figure {
                kind: FloatKind::Table,
                ..
            }
        )
    })
    .next()
    .expect("a table");
    assert!(matches!(&table.kind, BlockKind::Figure { label: Some(label), .. } if label == "Table 1"));
    assert_eq!(table.anchor.as_deref(), Some("tab-1"));
    let grid = table.table.as_ref().expect("a rebuilt table");
    assert_eq!((grid.n_cols, grid.header_rows, grid.rows.len()), (3, 1, 3));
    assert_eq!(grid.rows[1][1].spans[0].text, "91.2");
    let figure = blocks_of(doc, |kind| {
        matches!(
            kind,
            BlockKind::Figure {
                kind: FloatKind::Figure,
                ..
            }
        )
    })
    .next()
    .expect("a figure");
    let BlockKind::Figure {
        image: Some(image),
        label: Some(label),
        ..
    } = &figure.kind
    else {
        panic!("the figure has a picture and a label")
    };
    assert_eq!(label, "Figure 1");
    assert_eq!(image.embedded, Some(0));
    assert!((image.rect.width() - 216.0).abs() < 1.0, "3 inches wide");
    assert_eq!(doc.pictures.len(), 1);
    assert!(doc.pictures[0].data.starts_with(b"\x89PNG"));

    // The code listing and the references.
    let code = blocks_of(doc, |kind| matches!(kind, BlockKind::Code))
        .next()
        .expect("code");
    assert_eq!(code.rows.len(), 2);
    let labels: Vec<Option<String>> = blocks_of(doc, |kind| matches!(kind, BlockKind::Reference { .. }))
        .map(|block| match &block.kind {
            BlockKind::Reference { label } => label.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(labels, [Some("1".to_string()), Some("2".to_string())]);
}

#[test]
fn doc_sample_reads_like_the_docx() {
    let (paper, doc_analysis) = load(SAMPLE_DOC);
    assert_eq!(paper.format(), InputFormat::Doc);
    let (_, docx_analysis) = load(SAMPLE_DOCX);
    // LibreOffice saves no formulas in a .doc without its Math module: the
    // blocks with formulas are left out of the comparison.
    let summary = |doc: &Document| -> Vec<String> {
        doc.blocks
            .iter()
            .filter(|block| {
                !matches!(block.kind, BlockKind::Equation { .. })
                    && !block.spans.iter().any(|span| span.math.is_some())
                    && !block.text().starts_with("Let ")
                    && !block.text().contains("tight bound")
            })
            .map(|block| {
                // LibreOffice saves the two items of the numbered list as
                // two lists, numbered "1." each: the numbers are left out.
                let (kind, text) = match &block.kind {
                    BlockKind::ListItem { marker, .. } => {
                        let text = block.text();
                        let text = marker.as_ref().map_or(text.clone(), |marker| {
                            text.strip_prefix(marker.as_str()).unwrap_or(&text).trim().to_string()
                        });
                        ("ListItem".to_string(), text)
                    }
                    other => (format!("{other:?}"), block.text()),
                };
                format!("{kind} {text} {:?}", block.anchor)
            })
            .collect()
    };
    assert_eq!(summary(&doc_analysis.doc), summary(&docx_analysis.doc));
    assert_eq!(doc_analysis.doc.pictures.len(), 1);
}

#[test]
fn ligo_survives_a_round_trip_through_word() {
    let (pdf_paper, pdf) = load(LIGO);
    let docx = pdf_paper.to_docx(&pdf, &DocxOptions::default());
    let paper = Paper::from_bytes(docx).unwrap();
    assert_eq!(paper.format(), InputFormat::Docx);
    let word = paper.analyze();
    let (pdf, word) = (&pdf.doc, &word.doc);
    assert_eq!(word.meta.title, pdf.meta.title);
    let count = |doc: &Document, pred: &dyn Fn(&BlockKind) -> bool| doc.blocks.iter().filter(|b| pred(&b.kind)).count();
    let references = |kind: &BlockKind| matches!(kind, BlockKind::Reference { .. });
    let footnotes = |kind: &BlockKind| matches!(kind, BlockKind::Footnote { .. });
    let figures = |kind: &BlockKind| matches!(kind, BlockKind::Figure { label: Some(_), .. });
    assert_eq!(count(word, &references), count(pdf, &references));
    assert_eq!(count(word, &footnotes), count(pdf, &footnotes));
    assert_eq!(count(word, &figures), count(pdf, &figures));
    // The headings come back, with an "Abstract" heading the Word document
    // writes over the abstract.
    let headings = |doc: &Document| -> Vec<String> {
        doc.blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Heading { .. }) && block.text() != "Abstract")
            .map(Block::text)
            .collect()
    };
    assert_eq!(headings(word), headings(pdf));
    let texts = |doc: &Document| -> Vec<String> {
        doc.blocks
            .iter()
            .filter(|block| references(&block.kind))
            .map(Block::text)
            .collect()
    };
    assert_eq!(texts(word), texts(pdf));
    // Formulas written as Office Math come back with their structure.
    let pdf_math = pdf
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Equation { .. }) && block.math.is_some())
        .count();
    let word_math = word
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Equation { .. }) && block.math.is_some())
        .count();
    assert_eq!(word_math, pdf_math);
    let inline = word
        .blocks
        .iter()
        .flat_map(|block| &block.spans)
        .filter(|span| span.math.is_some())
        .count();
    assert!(inline > 20, "formulas in running text: {inline}");
    // Every picture of the figures is stored.
    assert!(word.pictures.len() >= count(pdf, &figures));
}

#[test]
fn every_output_can_be_written_from_word() {
    for name in [SAMPLE_DOCX, SAMPLE_DOC] {
        let (paper, mut analysis) = load(name);
        let friendly = paper.to_html(&analysis, &HtmlOptions::default()).html;
        assert!(friendly.contains("Converted from Word by kakemono"), "{name}");
        assert!(
            !friendly.contains(" pages</span>"),
            "{name}: a Word document has no pages"
        );
        // The page modes show the document laid out as Word lays it out:
        // its pages (hard, hard-95), or its formatting on one sheet (medium).
        for mode in [Mode::Hard, Mode::Hard95] {
            let opts = HtmlOptions {
                mode,
                ..Default::default()
            };
            let html = paper.to_html(&analysis, &opts).html;
            assert!(
                html.contains(" pages</span>") || html.contains(" page</span>"),
                "{name} {mode:?}: pages"
            );
            assert!(html.contains("Convergence of Gradient Descent"), "{name} {mode:?}");
        }
        let medium = HtmlOptions {
            mode: Mode::Medium,
            ..Default::default()
        };
        let medium = paper.to_html(&analysis, &medium).html;
        assert!(medium.contains("class=\"sheet\""), "{name}: medium is one sheet");
        assert!(medium.contains("Convergence of Gradient Descent"), "{name}");
        let docx = paper.to_docx(&analysis, &DocxOptions::default());
        assert!(docx.starts_with(b"PK"));
        let markdown = paper.to_markdown(&analysis, &MarkdownOptions::default());
        assert!(markdown.markdown.contains("# Convergence of Gradient Descent"));
        assert_eq!(markdown.files.len(), 1, "{name}: the picture as a file");
        let latex = paper.to_latex(&analysis, &LatexOptions::default());
        assert!(latex.latex.contains("Converted from a Word document"));
        assert!(latex.latex.contains("\\begin{tabular}"));
        let epub = paper.to_epub(&analysis, &EpubOptions::default());
        assert!(epub.starts_with(b"PK"));
        let content = paper.to_json(&analysis, &JsonOptions::default());
        let expected = if name.ends_with(".docx") { "docx" } else { "doc" };
        assert!(content.json.contains(&format!("\"format\": \"{expected}\"")), "{name}");

        // An edited abstract is applied back and shown.
        let mut json: serde_json::Value = serde_json::from_str(&content.json).unwrap();
        let blocks = json["blocks"].as_array_mut().unwrap();
        let abstract_block = blocks
            .iter_mut()
            .find(|block| block["abstract"] == serde_json::Value::Bool(true))
            .unwrap();
        abstract_block["text"] = "Nous étudions la descente de gradient.".into();
        let report = paper.apply_content(&mut analysis, &json.to_string()).unwrap();
        assert_eq!(report.edited_blocks, 1, "{name}: {:?}", report.warnings);
        let html = paper.to_html(&analysis, &HtmlOptions::default()).html;
        assert!(html.contains("Nous étudions la descente de gradient."), "{name}");
    }
}

#[test]
fn word_outputs_match_their_snapshot() {
    let mut text = String::new();
    for name in [SAMPLE_DOCX, SAMPLE_DOC] {
        let (paper, analysis) = load(name);
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
        for (output, bytes) in outputs {
            let bytes = String::from_utf8_lossy(&bytes)
                .replace(env!("CARGO_PKG_VERSION"), "VERSION")
                .into_bytes();
            let hash: String = Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            text.push_str(&format!("{name} {output} {hash}\n"));
        }
    }
    let file = data("snapshots/word-sample.txt");
    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        old, text,
        "the output of the Word samples changed; if this is intended, run \
         KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test word"
    );
}

/// Items of lists inside lists keep their nesting level.
#[test]
fn nested_list_items_keep_their_level() {
    let (_, analysis) = load(LISTS_DOCX);
    let levels: Vec<(String, u8)> = analysis
        .doc
        .blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::ListItem { level, .. } => Some((block.text(), *level)),
            _ => None,
        })
        .collect();
    assert_eq!(
        levels,
        [
            ("1. First item".to_string(), 0),
            ("A bullet inside it".to_string(), 1),
            ("A bullet two levels in".to_string(), 2),
            ("2. Second item".to_string(), 0),
        ]
    );
}
