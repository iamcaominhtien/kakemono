//! Word documents as input: the sample `.docx` and `.doc` of
//! `tests/data/word` (written from `sample.md` by pandoc, and saved as
//! `.doc` by LibreOffice), the vector pictures of `vector.docx` (written by
//! `make-vector-docx.py`), the charts of `chart.docx` (written by
//! `make-chart-docx.py`), and the LIGO paper made into a Word document by
//! kakemono and read back.

use kakemono::model::{Block, BlockKind, Document, FloatKind, PictureFormat, Span};
use kakemono::{
    Analysis, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions, MarkdownOptions, Mode,
    Paper, TypstOptions,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// The sample Word document.
const SAMPLE_DOCX: &str = "word/sample.docx";
/// The same document saved as a Word 97–2003 file.
const SAMPLE_DOC: &str = "word/sample.doc";
/// An SVG drawing with a PNG fallback, an EMF metafile and an SVG alone,
/// written by `make-vector-docx.py`.
const VECTOR_DOCX: &str = "word/vector.docx";
/// A column chart with a caption, an anchored doughnut, an extended chart
/// with a fallback picture and one without, written by
/// `make-chart-docx.py`.
const CHART_DOCX: &str = "word/chart.docx";
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
        let typst = paper.to_typst(&analysis, &TypstOptions::default());
        assert!(typst.typst.contains("Converted from a Word document"));
        assert!(typst.typst.contains("#kakemono-table("));
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
    for name in [SAMPLE_DOCX, SAMPLE_DOC, VECTOR_DOCX, CHART_DOCX] {
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
            (
                "typst",
                paper.to_typst(&analysis, &TypstOptions::default()).typst.into_bytes(),
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

/// The SVG of each picture of `doc`, which must all be SVG.
fn svg_pictures(doc: &Document) -> Vec<String> {
    doc.pictures
        .iter()
        .map(|picture| {
            assert_eq!(picture.format, PictureFormat::Svg);
            String::from_utf8(picture.data.clone()).unwrap()
        })
        .collect()
}

/// The pictures of the PDF `pdf` (`/Subtype/Image`, as krilla writes it).
fn pdf_bitmaps(pdf: &[u8]) -> usize {
    String::from_utf8_lossy(pdf).matches("/Subtype/Image").count()
}

/// An SVG picture (the SVG of a picture that has a PNG fallback too), an EMF
/// and an SVG alone are read as SVG, without what could run or reach out,
/// and stay vector in every output.
#[test]
fn vector_pictures_stay_vector_in_every_output() {
    let (paper, analysis) = load(VECTOR_DOCX);
    let svgs = svg_pictures(&analysis.doc);
    assert_eq!(svgs.len(), 3);
    // The SVG, not its grey PNG: its blue rectangle and green triangle.
    assert!(
        svgs[0].contains("#1f77b4") && svgs[0].contains("#2ca02c"),
        "{}",
        svgs[0]
    );
    for svg in &svgs {
        for gone in [
            "script",
            "alert",
            "foreignObject",
            "file:",
            "https:",
            "javascript",
            "<style",
        ] {
            assert!(!svg.contains(gone), "{gone} in {svg}");
        }
    }

    // HTML: each picture an <img> of its SVG, as a data URI.
    let html = paper.to_html(&analysis, &HtmlOptions::default()).html;
    assert_eq!(html.matches("<img src=\"data:image/svg+xml;base64,").count(), 3);
    // Markdown, JSON: SVG files.
    let markdown = paper.to_markdown(&analysis, &MarkdownOptions::default());
    let content = paper.to_json(&analysis, &JsonOptions::default());
    for files in [&markdown.files, &content.files] {
        assert_eq!(files.len(), 3);
        assert!(
            files
                .iter()
                .all(|(name, data)| name.ends_with(".svg") && data.starts_with(b"<svg"))
        );
    }
    // LaTeX: each picture a PDF of one page, still vector.
    let latex = paper.to_latex(&analysis, &LatexOptions::default());
    assert_eq!(latex.files.len(), 3);
    for (name, pdf) in &latex.files {
        assert!(name.ends_with(".pdf") && latex.latex.contains(name.as_str()), "{name}");
        assert!(pdf.starts_with(b"%PDF") && pdf_bitmaps(pdf) == 0, "{name}");
    }
    // EPUB: the SVG files in the book.
    let epub = paper.to_epub(&analysis, &EpubOptions::default());
    assert!(String::from_utf8_lossy(&epub).matches(".svg").count() >= 3);
    // Word: each picture as Word writes an SVG, a PNG fallback with the
    // SVG beside it, which a reader that knows SVG (kakemono) takes.
    let docx = paper.to_docx(&analysis, &DocxOptions::default());
    let text = String::from_utf8_lossy(&docx);
    for media in ["word/media/image1.png", "word/media/image2.svg"] {
        assert!(text.contains(media), "{media}");
    }
    let again = Paper::from_bytes(docx).unwrap();
    assert_eq!(svg_pictures(&again.analyze().doc).len(), 3);
    // The page layout: drawn as vector, no bitmap in the PDF.
    let pdf = paper.to_pdf().unwrap();
    assert_eq!(pdf_bitmaps(&pdf), 0);
}

/// Each chart of a Word document is a figure of its SVG drawing followed
/// by its data table, at its place in the text, in every output; an
/// extended chart shows its fallback picture, or its data alone; the page
/// layout draws the charts as vector.
#[test]
fn charts_are_figures_with_their_data_in_every_output() {
    let (paper, analysis) = load(CHART_DOCX);
    let doc = &analysis.doc;
    let floats: Vec<(String, String)> = blocks_of(doc, |kind| matches!(kind, BlockKind::Figure { .. }))
        .map(|block| {
            let what = match (&block.kind, &block.table) {
                (_, Some(_)) => "table".to_string(),
                (BlockKind::Figure { image: Some(image), .. }, None) => {
                    let picture = &doc.pictures[image.embedded.expect("a picture of the document")];
                    format!("{:?}", picture.format)
                }
                _ => "none".to_string(),
            };
            (what, block.text())
        })
        .collect();
    let expected = [
        ("Svg", "Figure 1: Sales by quarter."),
        ("table", "Figure 1: Sales by quarter."),
        ("Svg", "Share of sales"),
        ("table", "Share of sales"),
        ("Png", ""),
        ("table", "Cash again"),
    ];
    let expected: Vec<(String, String)> = expected
        .iter()
        .map(|(what, text)| ((*what).to_string(), (*text).to_string()))
        .collect();
    assert_eq!(floats, expected);
    // The caption under the chart is the chart's, with its label.
    assert!(matches!(
        &block(doc, "Figure 1").kind,
        BlockKind::Figure { kind: FloatKind::Figure, label: Some(label), .. } if label == "Figure 1"
    ));
    // The drawings: the file's theme colours, the values on the bars and
    // the shares of the doughnut.
    let svgs: Vec<String> = doc
        .pictures
        .iter()
        .filter(|picture| picture.format == PictureFormat::Svg)
        .map(|picture| String::from_utf8(picture.data.clone()).unwrap())
        .collect();
    assert!(
        svgs[0].contains("#2a6f97") && svgs[0].contains(">170</text>"),
        "{}",
        svgs[0]
    );
    assert!(svgs[1].contains(">45%</text>"), "{}", svgs[1]);
    // Markdown: the drawings as SVG files, the data as tables.
    let markdown = paper.to_markdown(&analysis, &MarkdownOptions::default());
    assert_eq!(
        markdown.files.iter().filter(|(name, _)| name.ends_with(".svg")).count(),
        2
    );
    assert!(
        markdown.markdown.contains("| Q4 | 170 | 140 |"),
        "{}",
        markdown.markdown
    );
    // The page layout: both charts drawn as vector; the one bitmap is the
    // fallback picture.
    let pdf = paper.to_pdf().unwrap();
    assert_eq!(pdf_bitmaps(&pdf), 1);
    let html = paper.to_html(&analysis, &HtmlOptions::default()).html;
    assert_eq!(html.matches("<img src=\"data:image/svg+xml;base64,").count(), 2);
}

/// A Word document's picture, already a stored picture, is replaced by a
/// `replace_image` given as a data URI, like a PDF's figure.
#[test]
fn replace_image_works_on_a_word_picture() {
    use base64::Engine;
    let (paper, mut analysis) = load(SAMPLE_DOCX);
    let json = paper.to_json(&analysis, &JsonOptions::default()).json;
    let mut content: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(30, 20, image::Rgb([200, 30, 30]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let png = png.into_inner();
    let uri = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    let figure = content["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["type"] == "figure" && block.get("image").is_some())
        .expect("the sample has a picture");
    figure["replace_image"] = uri.into();
    let report = paper.apply_content(&mut analysis, &content.to_string()).unwrap();
    assert_eq!(report.edited_blocks, 1, "{:?}", report.warnings);
    let out = paper.to_json(&analysis, &JsonOptions::default());
    // The new picture is written (fitted into the figure's box, as an SVG
    // when its shape differs): drawn, its red is in the middle.
    let red = out.files.iter().any(|(_, data)| {
        let png = match std::str::from_utf8(data) {
            Ok(svg) => kakemono::backend::svg_to_png(svg, 1.0),
            Err(_) => Some(data.clone()),
        };
        png.and_then(|png| image::load_from_memory(&png).ok())
            .map(|picture| picture.to_rgb8())
            .is_some_and(|picture| picture.get_pixel(picture.width() / 2, picture.height() / 2).0 == [200, 30, 30])
    });
    assert!(red, "the new picture is written");
}

/// The number of text objects (`BT` operators) in the content streams of
/// `pdf`, the streams decompressed.
fn text_objects(pdf: &[u8]) -> usize {
    use std::io::Read;
    let mut count = 0;
    let mut rest = pdf;
    while let Some(start) = rest.windows(7).position(|window| window == b"stream\n") {
        let body = &rest[start + 7..];
        let end = body
            .windows(9)
            .position(|window| window == b"endstream")
            .unwrap_or(body.len());
        let mut content = Vec::new();
        if flate2::read::ZlibDecoder::new(&body[..end])
            .read_to_end(&mut content)
            .is_ok()
        {
            count += content
                .split(|byte| byte.is_ascii_whitespace())
                .filter(|token| *token == b"BT")
                .count();
        }
        rest = &body[end..];
    }
    count
}

/// The page layout draws the words of a line set in one font as one text
/// object, not one per word and per space: a few per line, the same text.
#[test]
fn word_pdf_draws_a_line_of_one_font_as_one_text_object() {
    let pdf = Paper::from_bytes(std::fs::read(data(SAMPLE_DOCX)).unwrap())
        .unwrap()
        .to_pdf()
        .unwrap();
    let (lines, _) = kakemono::layout::lines::pdf_lines(pdf.clone()).unwrap();
    let words: usize = lines.iter().map(|line| line.text.split_whitespace().count()).sum();
    let objects = text_objects(&pdf);
    assert!(lines.len() > 20 && words > 120, "{} lines, {words} words", lines.len());
    assert!(
        objects <= 2 * lines.len(),
        "{objects} text objects for {} lines and {words} words",
        lines.len()
    );
    assert!(
        lines
            .iter()
            .any(|line| line.text.starts_with("We study the convergence of gradient descent")),
        "a line keeps its words"
    );
}
