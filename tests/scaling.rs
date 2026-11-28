//! Pages that grow take a time in proportion: a rule that compares each
//! node with its parent, or each item with all the ones before it, takes 16
//! times longer on a page 4 times longer. Each page is written here at two
//! sizes, read (and cut into chunks, or laid out in pages as a PDF), and
//! the larger may take at most `SLOWDOWN_MAX` times longer than the
//! smaller.

use kakemono::{ChunkOptions, Paper};
use std::time::{Duration, Instant};

/// How many times larger the second page is.
const GROWTH: usize = 4;
/// Most times longer the larger page may take: 4 in proportion, 16 for a
/// rule whose time grows with the square of the page.
const SLOWDOWN_MAX: f64 = 10.0;
/// Each page is read up to this many times, and the fastest time kept.
const RUNS: usize = 5;
/// A larger page read faster than this passes whatever the ratio: such
/// times are mostly noise.
const NOISE: Duration = Duration::from_millis(40);

/// A page whose article is `body` and a few paragraphs.
fn article(body: &str) -> String {
    let text: String = (0..20)
        .map(|i| format!("<p>Paragraph {i} of the article, with enough words to be one.</p>"))
        .collect();
    format!(
        "<!doctype html><html><head><meta charset='utf-8'><title>Scaling</title></head>\
         <body><main><article><h1>Scaling</h1>{body}{text}</article></main></body></html>"
    )
}

/// `template` with `{i}` replaced by 0, 1, ... `count` times.
fn repeat(template: &str, count: usize) -> String {
    (0..count).map(|i| template.replace("{i}", &i.to_string())).collect()
}

/// A page of the benchmark of `scripts/web-bench/perf.py`, by name, with
/// `count` items.
fn page(name: &str, count: usize) -> String {
    match name {
        "list items" => article(&format!(
            "<ul>{}</ul>",
            repeat("<li>item {i} <a href='/i{i}'>x</a></li>", count)
        )),
        "menu spans" => article(&format!("<p>{}</p>", repeat("<span class='menu'>w{i}</span> ", count))),
        "number paragraphs" => article(&format!("<div>{}</div>", repeat("<p>{i}</p>", count))),
        "line breaks" => article(&format!("<p>{}</p>", repeat("line {i}<br><br>", count))),
        "code lines" => article(&format!("<pre>{}</pre>", repeat("line {i}\n", count))),
        "one note cited" => article(&format!(
            "{}<ol><li id='n0'>Note</li></ol>",
            repeat("<p>Claim {i}<sup><a href='#n0'>[0]</a></sup></p>", count)
        )),
        "same headings" => article(&repeat(
            "<h3>This is a heading that reads like a whole sentence number {i}.</h3>",
            count,
        )),
        "clutter boxes" => article(&repeat(
            "<div class='ad sidebar share'><a href='/x{i}'>x</a></div>",
            count,
        )),
        "links" => article(&format!("<p>{}</p>", repeat("<a href='/p{i}'>link {i}</a> ", count))),
        _ => unreachable!("{name}"),
    }
}

/// What is done with a page.
#[derive(Clone, Copy)]
enum Work {
    /// It is read.
    Read,
    /// It is read and cut into chunks.
    Chunks,
    /// It is read and laid out in pages as a PDF.
    Pdf,
}

/// The fastest of `RUNS` times `work` is done with `html`; the first when
/// it takes longer than `enough` (a page that is read too slowly is not
/// read again).
fn time(html: &str, work: Work, enough: Duration) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..RUNS {
        let start = Instant::now();
        let paper = Paper::from_bytes(html.as_bytes().to_vec()).unwrap();
        match work {
            Work::Read => drop(paper.analyze()),
            Work::Chunks => assert!(!paper.to_chunks(&paper.analyze(), &ChunkOptions::default()).is_empty()),
            Work::Pdf => assert!(!paper.to_pdf().unwrap().is_empty()),
        }
        best = best.min(start.elapsed());
        if best > enough {
            break;
        }
    }
    best
}

/// One test for all the pages, so that they are not timed while the
/// others run.
#[test]
fn pages_take_a_time_in_proportion_to_their_size() {
    // The page, its smaller number of items, and what is done with it.
    let cases = [
        ("list items", 2_000, Work::Read),
        ("menu spans", 4_000, Work::Read),
        ("number paragraphs", 8_000, Work::Read),
        ("line breaks", 4_000, Work::Read),
        ("code lines", 10_000, Work::Chunks),
        ("one note cited", 2_000, Work::Read),
        ("same headings", 2_000, Work::Read),
        ("clutter boxes", 10_000, Work::Read),
        // Headings are kept with the next paragraph: a chain of them.
        ("same headings", 200, Work::Pdf),
        ("links", 3_000, Work::Pdf),
    ];
    let mut slow = Vec::new();
    for (name, count, work) in cases {
        let small = time(&page(name, count), work, Duration::MAX);
        let large = time(&page(name, count * GROWTH), work, small.mul_f64(SLOWDOWN_MAX));
        let ratio = large.as_secs_f64() / small.as_secs_f64().max(1e-6);
        eprintln!("{name}: {small:?} -> {large:?} ({ratio:.1} times)");
        if large > NOISE && ratio > SLOWDOWN_MAX {
            slow.push(format!("{name}: {small:?} -> {large:?} ({ratio:.1} times)"));
        }
    }
    assert!(slow.is_empty(), "slower than in proportion:\n{}", slow.join("\n"));
}
