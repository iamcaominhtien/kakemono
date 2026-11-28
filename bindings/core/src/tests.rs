//! Tests of the façade on the committed samples: the LIGO paper (PDF) and
//! the Word, Excel and web-page samples.

use super::{CancelToken, Document, ErrorKind, OpenOptions, RenderOptions, Source, Target, stop_token, timeout_millis};

/// A file of the repository's test data.
fn data(path: &str) -> Vec<u8> {
    std::fs::read(format!("{}/../../tests/data/{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The LIGO paper, opened with one job.
fn ligo() -> Document {
    let opts = OpenOptions {
        jobs: Some(1),
        ..Default::default()
    };
    Document::open(Source::Data(data("1602.03837v1.pdf")), &opts).unwrap()
}

#[test]
fn a_pdf_is_written_to_every_text_target() {
    let doc = ligo();
    assert_eq!(doc.format(), "pdf");
    assert!(doc.page_count() > 10);
    let markdown = doc.render(Target::Markdown, &RenderOptions::default()).unwrap();
    let text = markdown.text.unwrap();
    assert!(text.contains("Observation of Gravitational Waves"), "{}", &text[..200]);
    assert!(markdown.files.iter().all(|file| file.path.starts_with("images/")));
    assert!(!markdown.files.is_empty());
    for target in [
        Target::Json,
        Target::Latex,
        Target::Typst,
        Target::Chunks,
        Target::Structure,
    ] {
        let out = doc.render(target, &RenderOptions::default()).unwrap();
        assert!(out.text.is_some_and(|text| !text.is_empty()), "{target:?}");
        assert!(out.data.is_none());
    }
    let html = doc.render_json("html", Some(r#"{"mode":"hard-95"}"#)).unwrap();
    assert!(html.text.unwrap().starts_with("<!DOCTYPE html>"));
}

#[test]
fn binary_targets_give_bytes() {
    let doc = ligo();
    let docx = doc.render(Target::Docx, &RenderOptions::default()).unwrap();
    assert!(docx.data.unwrap().starts_with(b"PK"));
    assert!(docx.files.is_empty());
    let epub = doc
        .render_json("epub", Some(r#"{"modified":"2026-01-31T12:00:00Z"}"#))
        .unwrap();
    assert!(epub.data.unwrap().starts_with(b"PK"));
    let err = doc.render(Target::Pdf, &RenderOptions::default()).unwrap_err();
    assert!(err.message().contains("already a PDF"), "{err}");
}

#[test]
fn wrong_options_are_refused_with_their_name() {
    let doc = ligo();
    let cases = [
        ("epub", r#"{"imageDir":"x"}"#, "imageDir does not apply to epub"),
        ("markdown", r#"{"mode":"hard"}"#, "Markdown has no pages"),
        ("markdown", r#"{"scale":9}"#, "scale must be between"),
        (
            "markdown",
            r#"{"imageDir":"../up"}"#,
            "imageDir must be a relative folder",
        ),
        ("markdown", r#"{"imgDir":"x"}"#, "unknown field `imgDir`"),
        ("typst", r#"{"layout":"poster"}"#, "layout must be"),
        ("epub", r#"{"modified":"yesterday"}"#, "modified must look like"),
        ("chunks", r#"{"maxTokens":0}"#, "maxTokens must be at least 1"),
        ("pptx", "{}", "unknown target 'pptx'"),
    ];
    for (target, json, message) in cases {
        let err = doc.render_json(target, Some(json)).unwrap_err();
        assert!(err.message().contains(message), "{target} {json}: {err}");
    }
}

#[test]
fn a_word_document_has_no_pages_but_lays_out_as_a_pdf() {
    let doc = Document::open(Source::Data(data("word/sample.docx")), &OpenOptions::default()).unwrap();
    assert_eq!(doc.format(), "docx");
    assert_eq!(doc.page_count(), 0);
    // Its own layout in every mode: hard draws its pages.
    let hard = doc.render_json("html", Some(r#"{"mode":"hard"}"#)).unwrap();
    assert!(hard.text.unwrap().starts_with("<!DOCTYPE html>"));
    let pdf = doc.render(Target::Pdf, &RenderOptions::default()).unwrap();
    assert!(pdf.data.unwrap().starts_with(b"%PDF"));
}

#[test]
fn a_path_records_its_file_name_and_content_edits_the_text() {
    let path = format!("{}/../../tests/data/word/sample.docx", env!("CARGO_MANIFEST_DIR"));
    let doc = Document::open(Source::Path(path.clone()), &OpenOptions::default()).unwrap();
    let json = doc
        .render(Target::Json, &RenderOptions::default())
        .unwrap()
        .text
        .unwrap();
    assert!(json.contains("sample.docx"));
    // Edit the first block's text and open again with it.
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let block = value["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["text"].is_string())
        .unwrap();
    block["text"] = "Đã dịch".into();
    let opts = OpenOptions {
        content: Some(value.to_string()),
        ..Default::default()
    };
    let edited = Document::open(Source::Path(path), &opts).unwrap();
    assert!(edited.warnings().is_empty(), "{:?}", edited.warnings());
    let markdown = edited.render(Target::Markdown, &RenderOptions::default()).unwrap();
    assert!(markdown.text.unwrap().contains("Đã dịch"));
    // Word's own layout does not show edited text.
    let err = edited.render_json("html", Some(r#"{"mode":"medium"}"#)).unwrap_err();
    assert!(err.message().contains("cannot show edited text"), "{err}");
    assert_eq!(err.kind(), super::ErrorKind::InvalidArgument);
}

#[test]
fn unreadable_data_and_unknown_options_are_errors() {
    let err = Document::open(Source::Data(b"not a document".to_vec()), &OpenOptions::default()).unwrap_err();
    assert!(!err.message().is_empty());
    assert_eq!(err.kind(), super::ErrorKind::Document);
    let err = OpenOptions::from_json(Some(r#"{"table":false}"#)).unwrap_err();
    assert!(err.message().contains("unknown field `table`"), "{err}");
    assert!(OpenOptions::from_json(Some("null")).is_ok());
    assert!(Source::guess("no/such/file.pdf").is_err());
    assert!(matches!(Source::guess("1706.03762"), Ok(Source::Arxiv(_))));
    assert!(matches!(Source::guess("https://example.org/"), Ok(Source::Url(_))));
}

#[test]
fn workbooks_and_web_pages_open() {
    let book = Document::open(Source::Data(data("excel/sample.xlsx")), &OpenOptions::default()).unwrap();
    assert_eq!(book.format(), "xlsx");
    assert!(
        book.render(Target::Markdown, &RenderOptions::default())
            .unwrap()
            .text
            .unwrap()
            .contains('|')
    );
    let page = Document::open(Source::Data(data("html/sample.html")), &OpenOptions::default()).unwrap();
    assert_eq!(page.format(), "html");
}

#[test]
fn every_target_name_reads_back() {
    for (name, target) in Target::ALL {
        assert_eq!(name.parse::<Target>().unwrap(), target);
        assert_eq!(target.name(), name);
    }
}

#[test]
fn chunks_come_as_objects_and_the_title_is_read() {
    let doc = ligo();
    assert!(
        doc.title().is_some_and(|title| title.contains("Gravitational")),
        "{:?}",
        doc.title()
    );
    let opts = RenderOptions {
        max_tokens: Some(256),
        ..Default::default()
    };
    let chunks = doc.chunks(&opts).unwrap();
    assert!(chunks.len() > 10);
    assert!(chunks.iter().all(|chunk| chunk.tokens <= 256 + 64));
    let lines = doc.render(super::Target::Chunks, &opts).unwrap().text.unwrap();
    assert_eq!(lines.lines().count(), chunks.len());
    let err = doc
        .chunks(&RenderOptions {
            scale: Some(2.0),
            ..Default::default()
        })
        .unwrap_err();
    assert!(err.message().contains("scale does not apply to chunks"), "{err}");
}

#[test]
fn a_panic_on_the_worker_thread_is_an_internal_error() {
    let err = super::guarded::<()>(|| panic!("boom")).unwrap_err();
    assert_eq!(err.kind(), super::ErrorKind::Internal);
    assert!(err.message().contains("boom"), "{err}");
}

#[test]
fn a_font_that_is_not_one_is_refused() {
    let err = super::add_font(b"not a font".to_vec()).unwrap_err();
    assert_eq!(err.kind(), super::ErrorKind::InvalidArgument);
}

#[test]
fn a_cancelled_conversion_stops_and_the_document_stays_usable() {
    let doc = ligo();
    let token = CancelToken::new();
    let stopper = token.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(30));
        stopper.cancel();
    });
    let started = std::time::Instant::now();
    let opts = RenderOptions {
        mode: Some("hard".into()),
        ..Default::default()
    };
    // Each round of the hard mode takes far longer than the cancellation.
    let err = loop {
        match doc.render_with(Target::Html, &opts, Some(&token)) {
            Ok(_) => assert!(started.elapsed().as_secs() < 60, "never cancelled"),
            Err(err) => break err,
        }
    };
    canceller.join().unwrap();
    assert_eq!(err.kind(), ErrorKind::Cancelled, "{err}");
    // A token already cancelled stops the next call before it starts.
    let err = doc
        .render_with(Target::Markdown, &RenderOptions::default(), Some(&token))
        .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::Cancelled);
    // The document is still whole.
    let md = doc.render(Target::Markdown, &RenderOptions::default()).unwrap();
    assert!(md.text.unwrap().contains("gravitational"));
}

#[test]
fn a_time_limit_stops_opening_and_rendering() {
    let token = stop_token(None, timeout_millis(Some(1.0)).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let err = Document::open_with(
        Source::Data(data("1602.03837v1.pdf")),
        &OpenOptions::default(),
        Some(&token),
    )
    .unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TimedOut, "{err}");
    let doc = ligo();
    let err = doc.chunks_with(&RenderOptions::default(), Some(&token)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::TimedOut);
    // A child of a cancelled token stops; cancelling a child does not stop its parent.
    let parent = CancelToken::new();
    let child = stop_token(Some(&parent), Some(std::time::Duration::from_secs(60))).unwrap();
    child.cancel();
    assert!(parent.state().is_none());
    assert!(doc.chunks_with(&RenderOptions::default(), Some(&parent)).is_ok());
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert_eq!(
            timeout_millis(Some(bad)).unwrap_err().kind(),
            ErrorKind::InvalidArgument
        );
    }
}
