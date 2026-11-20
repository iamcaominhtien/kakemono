//! The page layout of Word documents against the PDFs Word made of them: a
//! few documents of the fidelity corpus (`tests/data/word-corpus.tsv`,
//! downloaded by `scripts/fetch-word-corpus.sh` into
//! `tests/data/word-corpus/`, not committed), chosen for what they hold.
//! Each is laid out, compared line by line with Word's PDF (as
//! `scripts/compare-layout.py` does) and written to every other output.
//! Documents that are not downloaded are skipped.
//!
//! The workflows download them with
//! `scripts/fetch-word-corpus.sh 8 $(cat tests/data/word-layout-fixtures.txt)`.
//! `KAKEMONO_PRINT_METRICS=1` prints each document's numbers.

use kakemono::layout::lines::{TextLine, pdf_lines};
use kakemono::{DocxOptions, EpubOptions, HtmlOptions, JsonOptions, LatexOptions, MarkdownOptions, Mode, Paper};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A document of the corpus: its id and extension, the least share of
/// Word's lines it must break as Word does (the current result, rounded
/// down), and whether its page count is Word's.
struct Fixture {
    id: &'static str,
    ext: &'static str,
    min_line_ratio: f64,
    same_pages: bool,
}

/// The documents, all from gov.uk.
const FIXTURES: &[Fixture] = &[
    // Sections in three columns, tables.
    Fixture {
        id: "govuk-0027",
        ext: "docx",
        min_line_ratio: 0.99,
        same_pages: true,
    },
    // An EMF picture, text boxes, tables.
    Fixture {
        id: "govuk-0055",
        ext: "docx",
        min_line_ratio: 0.97,
        same_pages: true,
    },
    // An EMF picture, text boxes, footnotes, six headers.
    Fixture {
        id: "govuk-0320",
        ext: "docx",
        min_line_ratio: 0.98,
        same_pages: false,
    },
    // Floating shapes, frames, nested tables.
    Fixture {
        id: "govuk-0113",
        ext: "doc",
        min_line_ratio: 0.96,
        same_pages: true,
    },
    // Floating shapes and text boxes.
    Fixture {
        id: "govuk2-0029",
        ext: "doc",
        min_line_ratio: 0.99,
        same_pages: true,
    },
    // Frames, tables.
    Fixture {
        id: "govuk2-0128",
        ext: "doc",
        min_line_ratio: 0.98,
        same_pages: false,
    },
    // Rows of exact height, lists set by styles, a header taller than the
    // top margin.
    Fixture {
        id: "govuk2-0131",
        ext: "doc",
        min_line_ratio: 0.88,
        same_pages: true,
    },
];

/// The ids the workflows download (`scripts/fetch-word-corpus.sh 8 $(cat …)`).
const FIXTURE_LIST: &str = include_str!("data/word-layout-fixtures.txt");

/// The path of a file of the downloaded corpus.
fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/word-corpus")
        .join(name)
}

/// A line's text as compared: plain quotes, single spaces, a lone symbol
/// (a bullet, which Word's PDFs often draw with no Unicode for it) as one
/// mark, and tab leaders without the spaces around them.
fn normalized(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|word| {
            let word: String = word
                .chars()
                .map(|ch| match ch {
                    // Word's PDFs sometimes give these two the codes of '‟' and '„'.
                    '\u{2019}' | '\u{2018}' | '\u{201F}' | '\u{201E}' => '\'',
                    '\u{201C}' | '\u{201D}' => '"',
                    other => other,
                })
                .collect();
            let mut chars = word.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) if !ch.is_alphanumeric() && !ch.is_ascii_punctuation() => "•".to_string(),
                _ => word,
            }
        })
        .collect();
    let joined = words.join(" ");
    // A run of three leader characters or more, without its spaces.
    let mut out = String::with_capacity(joined.len());
    let chars: Vec<char> = joined.chars().collect();
    let leader = |ch: char| matches!(ch, '.' | '-' | '_' | '·' | '…');
    let mut at = 0;
    while at < chars.len() {
        let run = chars[at..].iter().take_while(|&&ch| leader(ch)).count();
        if run >= 3 {
            if out.ends_with(' ') {
                out.pop();
            }
            out.extend(&chars[at..at + run]);
            at += run;
            if chars.get(at) == Some(&' ') {
                at += 1;
            }
        } else {
            out.push(chars[at]);
            at += 1;
        }
    }
    out
}

/// The share of `reference`'s lines that `ours` has with the same words
/// (each line of ours matched once).
fn line_ratio(reference: &[TextLine], ours: &[TextLine]) -> f64 {
    let mut available: HashMap<String, usize> = HashMap::new();
    for line in ours {
        *available.entry(normalized(&line.text)).or_default() += 1;
    }
    let mut total = 0usize;
    let mut matched = 0usize;
    for line in reference {
        let text = normalized(&line.text);
        if text.is_empty() {
            continue;
        }
        total += 1;
        if let Some(count) = available.get_mut(&text)
            && *count > 0
        {
            *count -= 1;
            matched += 1;
        }
    }
    matched as f64 / total.max(1) as f64
}

#[test]
fn fixture_list_names_every_fixture() {
    let listed: Vec<&str> = FIXTURE_LIST.split_whitespace().collect();
    let ids: Vec<&str> = FIXTURES.iter().map(|fixture| fixture.id).collect();
    assert_eq!(listed, ids, "tests/data/word-layout-fixtures.txt");
}

#[test]
fn word_layout_matches_word_on_corpus_fixtures() {
    let print = std::env::var_os("KAKEMONO_PRINT_METRICS").is_some();
    let mut checked = 0;
    for fixture in FIXTURES {
        let word = corpus(&format!("{}.{}", fixture.id, fixture.ext));
        let reference = corpus(&format!("{}.pdf", fixture.id));
        let (Ok(word), Ok(reference)) = (std::fs::read(&word), std::fs::read(&reference)) else {
            eprintln!("{}: not downloaded, skipped", fixture.id);
            continue;
        };
        let paper = Paper::from_bytes(word).unwrap();
        let pdf = paper.to_pdf().unwrap();
        let (ours, our_pages) = pdf_lines(pdf).unwrap();
        let (theirs, their_pages) = pdf_lines(reference).unwrap();
        let ratio = line_ratio(&theirs, &ours);
        if print {
            eprintln!("{}: lines {ratio:.4}, pages {our_pages}/{their_pages}", fixture.id);
        }
        assert!(
            ratio >= fixture.min_line_ratio,
            "{}: {ratio:.4} of Word's lines broken as Word, at least {} expected",
            fixture.id,
            fixture.min_line_ratio
        );
        if fixture.same_pages {
            assert_eq!(our_pages, their_pages, "{}: Word's page count", fixture.id);
        }
        checked += 1;
    }
    eprintln!("{checked} of {} documents checked", FIXTURES.len());
}

#[test]
fn every_output_of_corpus_fixtures_can_be_written() {
    for fixture in FIXTURES {
        let Ok(word) = std::fs::read(corpus(&format!("{}.{}", fixture.id, fixture.ext))) else {
            continue;
        };
        let paper = Paper::from_bytes(word).unwrap();
        let analysis = paper.analyze();
        for mode in [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly] {
            let opts = HtmlOptions {
                mode,
                ..Default::default()
            };
            let html = paper.to_html(&analysis, &opts).html;
            assert!(html.len() > 1000, "{} {mode:?}", fixture.id);
        }
        assert!(paper.to_docx(&analysis, &DocxOptions::default()).starts_with(b"PK"));
        assert!(
            !paper
                .to_markdown(&analysis, &MarkdownOptions::default())
                .markdown
                .is_empty()
        );
        assert!(!paper.to_latex(&analysis, &LatexOptions::default()).latex.is_empty());
        assert!(paper.to_epub(&analysis, &EpubOptions::default()).starts_with(b"PK"));
        assert!(!paper.to_json(&analysis, &JsonOptions::default()).json.is_empty());
    }
}
