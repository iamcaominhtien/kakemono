//! Stopping a conversion with a [`CancelToken`]: on the LIGO paper (the one
//! test paper committed to the repository) and the Word and Excel samples,
//! from another thread and by a time limit, at every stage.

use kakemono::{AnalyzeOptions, CancelToken, Cancelled, MarkdownOptions, Paper};
use std::time::{Duration, Instant};

/// The LIGO paper.
const LIGO: &str = "tests/data/1602.03837v1.pdf";

/// The whole conversion of `path` to Markdown, as one piece of work.
fn markdown(path: &str, jobs: usize) -> String {
    let paper = Paper::from_path(path).unwrap();
    let analysis = paper.analyze_with(AnalyzeOptions {
        jobs,
        ..Default::default()
    });
    let opts = MarkdownOptions {
        jobs,
        ..Default::default()
    };
    paper.to_markdown(&analysis, &opts).markdown
}

#[test]
fn a_token_that_is_not_cancelled_changes_nothing() {
    for jobs in [1, 4] {
        let plain = markdown(LIGO, jobs);
        let token = CancelToken::new();
        assert_eq!(token.run(|| markdown(LIGO, jobs)), Ok(plain));
    }
}

#[test]
fn a_cancelled_conversion_stops_soon_at_any_point() {
    for jobs in [1, 4] {
        let start = Instant::now();
        let _ = markdown(LIGO, jobs);
        let full = start.elapsed();
        // The longest step (drawing one figure) is a share of the whole
        // run, whatever the speed of the build (a debug build with coverage
        // is tens of times slower than a release build).
        let limit = Duration::from_millis(200) + full / 4;
        for tenth in 1..10 {
            let token = CancelToken::new();
            let stopper = token.clone();
            let at = full * tenth / 10;
            let timer = std::thread::spawn(move || {
                std::thread::sleep(at);
                stopper.cancel();
            });
            let start = Instant::now();
            let outcome = token.run(|| markdown(LIGO, jobs));
            let took = start.elapsed();
            timer.join().unwrap();
            // The run can end before the cancellation comes; otherwise it
            // does not go on long after it.
            if outcome.is_err() {
                assert_eq!(outcome, Err(Cancelled::Requested));
                assert!(
                    took < at + limit,
                    "jobs {jobs}, run of {full:?}, cancelled at {at:?}, stopped after {took:?}"
                );
            }
        }
    }
}

#[test]
fn a_time_limit_stops_the_conversion() {
    let start = Instant::now();
    let _ = markdown(LIGO, 4);
    let full = start.elapsed();
    let token = CancelToken::with_timeout(full / 10);
    let start = Instant::now();
    assert_eq!(token.run(|| markdown(LIGO, 4)), Err(Cancelled::TimedOut));
    assert!(start.elapsed() < full / 10 + Duration::from_millis(200) + full / 4);
}

#[test]
fn word_and_excel_documents_stop_too() {
    for path in [
        "tests/data/word/sample.docx",
        "tests/data/word/sample.doc",
        "tests/data/excel/sample.xlsx",
        "tests/data/epub/sample.epub",
        "tests/data/html/sample.html",
    ] {
        let token = CancelToken::new();
        token.cancel();
        assert_eq!(token.run(|| markdown(path, 1)), Err(Cancelled::Requested), "{path}");
        // Cancelled once loaded: the layout of its pages stops too.
        let paper = Paper::from_path(path).unwrap();
        let token = CancelToken::new();
        let outcome = token.run(|| {
            token.cancel();
            paper.to_pdf()
        });
        assert_eq!(outcome.err(), Some(Cancelled::Requested), "{path}");
        // The paper is still whole afterwards.
        assert!(!markdown(path, 1).is_empty());
        assert!(paper.to_pdf().unwrap().starts_with(b"%PDF-"), "{path}");
    }
}

#[test]
fn a_paper_stays_usable_after_a_cancelled_rendering() {
    let paper = Paper::from_path(LIGO).unwrap();
    let analysis = paper.analyze();
    let opts = MarkdownOptions::default();
    let token = CancelToken::new();
    let outcome = token.run(|| {
        token.cancel();
        paper.to_markdown(&analysis, &opts).markdown
    });
    assert_eq!(outcome, Err(Cancelled::Requested));
    assert_eq!(paper.to_markdown(&analysis, &opts).markdown, markdown(LIGO, 0));
}

#[test]
fn a_parent_token_stops_its_children() {
    let parent = CancelToken::new();
    let child = parent.child(Some(Duration::from_secs(60)));
    parent.cancel();
    assert_eq!(child.run(|| markdown(LIGO, 4)), Err(Cancelled::Requested));
}
