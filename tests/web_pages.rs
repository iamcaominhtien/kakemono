//! Real web pages (`tests/data/web-pages.tsv`, downloaded by
//! `scripts/fetch-test-pages.sh` into `tests/data/web/`, not committed):
//! per page, the title, the text of the article that must be read, the
//! clutter around it that must not, and lower bounds on the words and
//! footnotes. A page that is not downloaded is skipped. Pages change: the
//! bounds leave room for that, and a page whose text changed is fetched
//! again by deleting its file.
//!
//! `KAKEMONO_PRINT_METRICS=1 cargo test --release --test web_pages -- --nocapture`
//! prints each page's numbers.

use kakemono::Paper;
use kakemono::model::{BlockKind, Document};
use std::path::{Path, PathBuf};

/// What is expected of one page.
struct Expected {
    /// Name of the file in `tests/data/web` (without `.html`).
    name: &'static str,
    /// The title.
    title: &'static str,
    /// Fewest words of the article.
    min_words: usize,
    /// Fewest footnotes.
    min_notes: usize,
    /// Texts of the article that must be read.
    present: &'static [&'static str],
    /// Texts around the article that must not.
    absent: &'static [&'static str],
}

/// The pages and what is expected of them.
const PAGES: &[Expected] = &[
    Expected {
        name: "anthropic-tools",
        title: "Writing effective tools for agents — with agents",
        min_words: 3000,
        min_notes: 0,
        present: &["Model Context Protocol", "Looking ahead", "Acknowledgements"],
        absent: &["Try Claude", "Newsroom", "Cookie"],
    },
    Expected {
        name: "cnn-huang",
        title: "Inside the Trump pressure campaign that restored Jensen Huang as Nvidia’s CEO",
        min_words: 1400,
        min_notes: 0,
        present: &["Senate Banking Committee"],
        absent: &["Ad Feedback", "Most read", "Terms of Use", "Sign in"],
    },
    Expected {
        name: "wiki-fourier",
        title: "Fourier transform",
        min_words: 15000,
        min_notes: 100,
        present: &["Plancherel theorem", "Uncertainty principle"],
        absent: &[
            "Jump to content",
            "From Wikipedia, the free encyclopedia",
            "[edit]",
            "Retrieved from",
        ],
    },
    Expected {
        name: "wiki-gravitational-wave",
        title: "Gravitational wave",
        min_words: 10000,
        min_notes: 100,
        present: &["LIGO"],
        absent: &["Jump to content", "[edit]", "Retrieved from"],
    },
    Expected {
        name: "wiki-rust",
        title: "Rust (programming language)",
        min_words: 9000,
        min_notes: 150,
        present: &["Ownership", "borrow checker"],
        absent: &["Jump to content", "[edit]", "Retrieved from"],
    },
    Expected {
        name: "wiki-vi-hanoi",
        title: "Hà Nội",
        min_words: 25000,
        min_notes: 100,
        present: &["Thăng Long", "Hồ Gươm"],
        absent: &["Bước tới nội dung", "cần thêm chú thích nguồn gốc", "[sửa"],
    },
    Expected {
        name: "arxiv-attention",
        title: "Attention Is All You Need",
        min_words: 5000,
        min_notes: 4,
        present: &["Scaled Dot-Product Attention", "Ashish Vaswani"],
        absent: &["Report GitHub Issue", "footnotemark", "thanks:", "Back to arXiv"],
    },
    Expected {
        name: "arxiv-mistral",
        title: "Mistral 7B",
        min_words: 3200,
        min_notes: 0,
        present: &["Sliding Window Attention", "Abstract"],
        absent: &["Report GitHub Issue", "Back to arXiv"],
    },
    Expected {
        name: "rust-book-ownership",
        title: "What Is Ownership?",
        min_words: 4000,
        min_notes: 0,
        present: &["The Stack and the Heap", "Ownership Rules"],
        absent: &["Keyboard shortcuts", "Press ← or →"],
    },
    Expected {
        name: "python-classes",
        title: "9. Classes",
        min_words: 5000,
        min_notes: 0,
        present: &["Private Variables", "class ClassName:"],
        absent: &["¶", "Table of Contents", "Previous topic", "Report a bug"],
    },
    Expected {
        name: "pg-great-work",
        title: "How to Do Great Work",
        min_words: 10000,
        min_notes: 0,
        present: &["The first step is to decide what to work on."],
        absent: &[],
    },
    Expected {
        name: "willison-llms-2024",
        title: "Things we learned about LLMs in 2024",
        min_words: 6000,
        min_notes: 0,
        present: &["The GPT-4 barrier was comprehensively broken"],
        absent: &["Subscribe", "Monthly briefing"],
    },
];

/// The file of a page.
fn file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/web")
        .join(format!("{name}.html"))
}

/// The words of the text of `doc`.
fn words(doc: &Document) -> usize {
    doc.blocks
        .iter()
        .map(|block| block.text().split_whitespace().count())
        .sum()
}

#[test]
fn web_pages_read_their_article() {
    let print = std::env::var("KAKEMONO_PRINT_METRICS").is_ok_and(|value| value == "1");
    let mut failures = Vec::new();
    for page in PAGES {
        let path = file(page.name);
        if !path.is_file() {
            eprintln!("skipping {} (run scripts/fetch-test-pages.sh)", page.name);
            continue;
        }
        let doc = Paper::from_path(&path).unwrap().analyze().doc;
        let text: String = doc.blocks.iter().map(|block| block.text() + "\n").collect();
        let words = words(&doc);
        let notes = doc
            .blocks
            .iter()
            .filter(|block| matches!(block.kind, BlockKind::Footnote { .. }))
            .count();
        let title = doc.meta.title.clone().unwrap_or_default();
        if print {
            eprintln!("{:<26} words {words:>6}  notes {notes:>4}  title {title:?}", page.name);
        }
        let mut fail = |what: String| failures.push(format!("{}: {what}", page.name));
        if title != page.title {
            fail(format!("title {title:?}, expected {:?}", page.title));
        }
        if !matches!(doc.blocks.first().map(|block| &block.kind), Some(BlockKind::Title)) {
            fail("the first block is not the title".into());
        }
        if words < page.min_words {
            fail(format!("{words} words, expected at least {}", page.min_words));
        }
        if notes < page.min_notes {
            fail(format!("{notes} footnotes, expected at least {}", page.min_notes));
        }
        for present in page.present {
            if !text.contains(present) {
                fail(format!("{present:?} is missing"));
            }
        }
        for absent in page.absent {
            if text.contains(absent) {
                fail(format!("{absent:?} is there"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
