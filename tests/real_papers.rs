//! Regression tests on real arXiv papers.
//!
//! The PDFs are listed (with pinned versions and checksums) in
//! `tests/data/papers.txt`. Tests never download anything: papers that are
//! not in `tests/data` are skipped with a note, unless
//! `KAKEMONO_REQUIRE_ALL_PAPERS=1` is set (CI), in which case a missing paper is
//! an error. `scripts/fetch-test-pdfs.sh` downloads them once.
//!
//! The expected values are the results of the current code. Counts of things
//! that should not happen (broken paragraphs, glyphs left in page pictures)
//! are upper bounds and counts of things that should happen (tables rebuilt)
//! are lower bounds, so an improvement passes and a regression fails. When a
//! change improves a number, tighten the bound here. Set
//! `KAKEMONO_PRINT_METRICS=1` to print the measured values.
//!
//! On top of these bounds, every output is compared with a snapshot in
//! `tests/data/snapshots`: a fingerprint (sha256) of the HTML of each mode, of
//! the Word, Markdown and LaTeX documents and of the JSON, so that any change
//! in the output, however small, is noticed.
//! A restructuring of the code must leave them unchanged. When a change of
//! output is intended, regenerate them with
//! `KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --test real_papers` and commit them; the
//! diff then shows which papers and modes changed. For papers whose licence
//! allows it the full JSON is kept too, so `git diff` shows what changed.

use kakemono::model::{BlockKind, FloatKind, MathNode, Span};
use kakemono::{
    AnalyzeOptions, ChunkOptions, DocxOptions, EpubOptions, Equations, HtmlOptions, JsonOptions, LatexOptions,
    MarkdownOptions, Mode, PageImage, Paper,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex, OnceLock, PoisonError};

struct Expect {
    id: &'static str,
    /// Text the detected title must contain.
    title: &'static str,
    min_tables: usize,
    max_broken_paragraphs: usize,
    max_split_words: usize,
    max_glyphs_left: usize,
    max_hyphenated: usize,
    /// Fewest displayed formulas whose structure is rebuilt (MathML).
    min_math: usize,
}

#[derive(Debug)]
struct Metrics {
    title: String,
    tables: usize,
    broken_paragraphs: usize,
    split_words: usize,
    glyphs_left: usize,
    hyphenated: usize,
    math: usize,
}

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("data")
}

/// Loads a test paper, or returns `None` (with a note) if it has not been
/// downloaded and downloads are not required.
fn load(id: &str) -> Option<Paper> {
    let path = data_dir().join(format!("{id}.pdf"));
    match std::fs::read(&path) {
        Ok(data) => Some(Paper::from_bytes(data).expect("valid PDF")),
        Err(_) if std::env::var("KAKEMONO_REQUIRE_ALL_PAPERS").is_ok_and(|v| v == "1") => {
            panic!("{} is missing: run scripts/fetch-test-pdfs.sh", path.display())
        }
        Err(_) => {
            eprintln!("skipping {id}: not downloaded (run scripts/fetch-test-pdfs.sh)");
            None
        }
    }
}

/// A test paper and its analysis.
struct Analysed {
    paper: Paper,
    analysis: kakemono::Analysis,
}

/// A test paper, loaded and analysed once and shared by every test of this
/// file: the analysis is the slow part (several times slower when coverage
/// is measured), and each paper is used by several tests. Different papers
/// are analysed in parallel; a test that needs a paper another test is
/// analysing waits for it. `None` when the paper is not downloaded.
fn analysed(id: &str) -> Option<&'static Analysed> {
    type Cells = HashMap<String, &'static OnceLock<Option<Analysed>>>;
    static PAPERS: LazyLock<Mutex<Cells>> = LazyLock::new(Mutex::default);
    let cell: &'static OnceLock<Option<Analysed>> = PAPERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(id.to_string())
        .or_insert_with(|| Box::leak(Box::default()));
    cell.get_or_init(|| {
        load(id).map(|paper| {
            let analysis = paper.analyze();
            Analysed { paper, analysis }
        })
    })
    .as_ref()
}

/// A copy of a shared analysis, for a test that changes it.
fn copy(analysis: &kakemono::Analysis) -> kakemono::Analysis {
    kakemono::Analysis {
        doc: analysis.doc.clone(),
        loose: analysis.loose.clone(),
        rotated: analysis.rotated.clone(),
    }
}

/// Visible characters of the text inside figures in a hard-95 page: the
/// HTML copy of glyphs that stay in the page pictures.
fn figure_text_chars(html: &str) -> usize {
    let tag = Regex::new(r"<[^>]*>").unwrap();
    html.split("data-kind=\"figure-text\"")
        .skip(1)
        .map(|rest| {
            let body = &rest[rest.find('>').map_or(0, |i| i + 1)..];
            let body = &body[..body.find("</div>").unwrap_or(body.len())];
            tag.replace_all(body, "").chars().filter(|c| !c.is_whitespace()).count()
        })
        .sum()
}

/// Characters of the glyphs that hard-95 leaves in the page picture on
/// purpose (radicals, big delimiters and operators): each is written as an
/// unseen copy in a `ph` element.
fn picture_glyph_chars(html: &str) -> usize {
    let tag = Regex::new(r"<[^>]*>").unwrap();
    let open = Regex::new(r#"<span class="(?:sg )?ph"[^>]*>"#).unwrap();
    open.find_iter(html)
        .map(|m| {
            let rest = &html[m.end()..];
            let body = &rest[..rest.find("</span>").unwrap_or(rest.len())];
            tag.replace_all(body, "").chars().filter(|c| !c.is_whitespace()).count()
        })
        .sum()
}

fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .map(|s| s.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Papers whose licence allows the full JSON output to be kept in the
/// repository (CC BY 4.0), so that a change shows up in `git diff`.
const FULL_JSON: &[&str] = &["1602.03837v1"];

/// Fingerprints of the outputs of one paper: (mode, sha256), plus the JSON.
struct Snapshot {
    fingerprints: Vec<(&'static str, String)>,
    json: String,
}

fn hex(digest: impl AsRef<[u8]>) -> String {
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// Fingerprint of a rendered mode: the HTML and every file it refers to,
/// except the pixels of images. Those can differ slightly between CPUs (the
/// image decoders and the rasteriser use SIMD code chosen at run time), so
/// only the size of a PNG and the vector content of an SVG count.
fn fingerprint(html: &str, files: &[(String, Vec<u8>)]) -> String {
    let embedded = regex::bytes::Regex::new(r"data:image/[a-z+]+;base64,[A-Za-z0-9+/=]*").unwrap();
    let mut h = Sha256::new();
    h.update(html.as_bytes());
    for (name, data) in files {
        h.update(name.as_bytes());
        if name.ends_with(".svg") {
            h.update(embedded.replace_all(data, &b"data:image"[..]));
        } else if data.starts_with(b"\x89PNG") && data.len() >= 24 {
            h.update(&data[16..24]); // width and height
        }
    }
    hex(h.finalize())
}

/// The files of a `.docx` or `.epub` package (a ZIP archive): (name, contents),
/// inflated. Reads the local headers one after another, which is enough
/// for the archives kakemono writes (no data descriptors).
fn zip_parts(docx: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let le16 = |at: usize| usize::from(u16::from_le_bytes([docx[at], docx[at + 1]]));
    let le32 = |at: usize| u32::from_le_bytes(docx[at..at + 4].try_into().unwrap()) as usize;
    let mut parts = Vec::new();
    let mut at = 0;
    while docx[at..].starts_with(b"PK\x03\x04") {
        let (method, size, name_len, extra_len) = (le16(at + 8), le32(at + 18), le16(at + 26), le16(at + 28));
        let name = String::from_utf8(docx[at + 30..at + 30 + name_len].to_vec()).unwrap();
        let start = at + 30 + name_len + extra_len;
        let raw = &docx[start..start + size];
        let data = match method {
            0 => raw.to_vec(),
            8 => {
                let mut out = Vec::new();
                flate2::read::DeflateDecoder::new(raw).read_to_end(&mut out).unwrap();
                out
            }
            other => panic!("{name}: unexpected compression method {other}"),
        };
        parts.push((name, data));
        at = start + size;
    }
    parts
}

/// Fingerprint of a Word document or an EPUB book: its text parts (XML,
/// XHTML, CSS) and the sizes of its pictures (as for the HTML, not their
/// pixels).
fn package_fingerprint(parts: &[(String, Vec<u8>)]) -> String {
    let mut h = Sha256::new();
    for (name, data) in parts {
        h.update(name.as_bytes());
        let text = [".xml", ".rels", ".xhtml", ".opf", ".ncx", ".css"];
        if text.iter().any(|ext| name.ends_with(ext)) {
            h.update(data);
        } else if data.starts_with(b"\x89PNG") && data.len() >= 24 {
            h.update(&data[16..24]); // width and height
        }
    }
    hex(h.finalize())
}

/// Checks a Word document of a paper with `math` rebuilt formulas: every
/// picture is a file of the package, every rebuilt formula is an equation.
fn check_docx(parts: &[(String, Vec<u8>)], math: usize) {
    let part = |name: &str| {
        let data = &parts
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .1;
        String::from_utf8(data.clone()).unwrap()
    };
    let document = part("word/document.xml");
    let rels = part("word/_rels/document.xml.rels");
    assert!(
        document.ends_with("</w:sectPr></w:body></w:document>"),
        "docx: incomplete document"
    );
    assert!(
        !document.contains('\u{1}'),
        "docx: picture placeholder left in the document"
    );
    assert_eq!(
        document.matches("<m:oMathPara>").count(),
        math,
        "docx: every rebuilt formula is an equation"
    );
    let pictures = document.matches("<a:blip r:embed=").count();
    let media: Vec<&str> = parts
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| n.starts_with("word/media/"))
        .collect();
    assert_eq!(pictures, media.len(), "docx: one file per picture");
    for name in media {
        assert!(
            rels.contains(&format!("Target=\"{}\"", &name["word/".len()..])),
            "docx: {name} has no relationship"
        );
    }
}

/// Checks an EPUB book of a paper with `math` rebuilt formulas: the
/// `mimetype` entry first, every content document complete and well nested,
/// every picture in the manifest, every link between documents pointing to
/// an id, and every rebuilt formula written as MathML.
fn check_epub(parts: &[(String, Vec<u8>)], math: usize) {
    assert_eq!(parts[0], ("mimetype".to_string(), b"application/epub+zip".to_vec()));
    let text = |name: &str| {
        let data = &parts
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("epub: no {name}"))
            .1;
        String::from_utf8(data.clone()).unwrap()
    };
    let opf = text("EPUB/package.opf");
    let docs: Vec<(&str, String)> = parts
        .iter()
        .filter(|(name, _)| name.ends_with(".xhtml"))
        .map(|(name, data)| (name.as_str(), String::from_utf8(data.clone()).unwrap()))
        .collect();
    let ids: Vec<String> = docs
        .iter()
        .flat_map(|(name, xhtml)| {
            let file = name.trim_start_matches("EPUB/");
            Regex::new(r#" id="([^"]*)""#)
                .unwrap()
                .captures_iter(xhtml)
                .map(|caps| format!("{file}#{}", &caps[1]))
                .collect::<Vec<_>>()
        })
        .collect();
    let link = Regex::new(r#"href="(c\d{3}\.xhtml#[^"]*)""#).unwrap();
    let mut blocks = 0;
    for (name, xhtml) in &docs {
        assert!(!xhtml.contains('\u{1}'), "epub: {name}: picture placeholder left");
        assert!(xhtml.ends_with("</html>\n"), "epub: {name}: incomplete document");
        check_nesting(name, xhtml);
        for caps in link.captures_iter(xhtml) {
            assert!(
                ids.contains(&caps[1].to_string()),
                "epub: {name}: {} points nowhere",
                &caps[1]
            );
        }
        blocks += xhtml
            .matches("<math xmlns=\"http://www.w3.org/1998/Math/MathML\" display=\"block\"")
            .count();
    }
    assert_eq!(blocks, math, "epub: every rebuilt formula is written as MathML");
    for (name, data) in parts.iter().filter(|(name, _)| name.starts_with("EPUB/images/")) {
        let href = name.trim_start_matches("EPUB/");
        assert!(
            opf.contains(&format!("href=\"{href}\"")),
            "epub: {name} not in the manifest"
        );
        assert!(!data.is_empty(), "epub: {name} is empty");
    }
}

/// Checks that the elements of an XHTML document are closed in order.
fn check_nesting(name: &str, xhtml: &str) {
    let tag = Regex::new(r"<(/?)([a-zA-Z][a-zA-Z0-9]*)[^>]*?(/?)>").unwrap();
    let mut open: Vec<String> = Vec::new();
    for caps in tag.captures_iter(xhtml) {
        let element = caps[2].to_string();
        if &caps[3] == "/" {
            continue;
        }
        if &caps[1] == "/" {
            let last = open.pop();
            assert_eq!(
                last.as_deref(),
                Some(element.as_str()),
                "epub: {name}: </{element}> closes nothing"
            );
        } else {
            open.push(element);
        }
    }
    assert!(open.is_empty(), "epub: {name}: elements left open: {open:?}");
}

/// Checks a Markdown document: no placeholder left, every picture file
/// referenced, and every rebuilt formula written as LaTeX.
fn check_markdown(markdown: &str, files: &[(String, Vec<u8>)], math: usize) {
    assert!(!markdown.contains('\u{1}'), "markdown: picture placeholder left");
    assert!(
        !markdown.contains("data:image"),
        "markdown: pictures are files by default"
    );
    for (name, data) in files {
        assert!(name.starts_with("images/"), "markdown: {name} is not in images/");
        assert!(
            markdown.contains(&format!("]({name})")),
            "markdown: {name} is not referenced"
        );
        assert!(!data.is_empty(), "markdown: {name} is empty");
    }
    assert_eq!(
        markdown.matches("$$\n").count(),
        2 * math,
        "markdown: every rebuilt formula is written as LaTeX"
    );
    assert!(!markdown.contains("\n\n\n"), "markdown: no runs of blank lines");
}

/// Checks a LaTeX document: no placeholder left, every picture file
/// referenced, every rebuilt formula written as LaTeX math, and the braces
/// and environments balanced outside the code listings.
fn check_latex(latex: &str, files: &[(String, Vec<u8>)], math: usize) {
    assert!(!latex.contains('\u{1}'), "latex: picture placeholder left");
    assert!(latex.ends_with("\\end{document}\n"), "latex: incomplete document");
    for (name, data) in files {
        assert!(name.starts_with("images/"), "latex: {name} is not in images/");
        assert!(
            latex.contains(&format!("{{{name}}}")),
            "latex: {name} is not referenced"
        );
        assert!(!data.is_empty(), "latex: {name} is empty");
    }
    let displays = latex.matches("\\[\n").count() + latex.matches("\\begin{equation*}\n").count();
    assert_eq!(
        displays - latex.matches("\\vcenter{\\hbox{").count(),
        math,
        "latex: every rebuilt formula is written as LaTeX"
    );
    assert!(!latex.contains("\n\n\n"), "latex: no runs of blank lines");
    // Outside the listings, which are verbatim, and the comments.
    let mut code = String::new();
    let mut in_verbatim = false;
    for line in latex.lines() {
        match line {
            "\\begin{verbatim}" => in_verbatim = true,
            "\\end{verbatim}" => in_verbatim = false,
            _ if in_verbatim || line.trim_start().starts_with('%') => {}
            _ => {
                code.push_str(line);
                code.push('\n');
            }
        }
    }
    let mut depth = 0i32;
    let mut chars = code.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                assert!(depth >= 0, "latex: unbalanced closing brace");
            }
            _ => {}
        }
    }
    assert_eq!(depth, 0, "latex: unbalanced braces");
    let environment = Regex::new(r"\\(begin|end)\{([a-zA-Z*]+)\}").unwrap();
    let mut open: Vec<String> = Vec::new();
    for found in environment.captures_iter(&code) {
        if &found[1] == "begin" {
            open.push(found[2].to_string());
        } else {
            assert_eq!(
                open.pop().as_deref(),
                Some(&found[2]),
                "latex: \\end{{{}}} closes nothing",
                &found[2]
            );
        }
    }
    assert!(open.is_empty(), "latex: environments left open: {open:?}");
}

/// Compares the outputs of a paper with its stored snapshot, or stores them
/// when `KAKEMONO_UPDATE_SNAPSHOTS=1`.
/// Checks the content JSON: it parses, has the format name, a block for
/// every block of the document, and every picture file is referenced.
fn check_content(json: &str, files: &[(String, Vec<u8>)]) {
    let value: serde_json::Value = serde_json::from_str(json).expect("the content JSON parses");
    assert_eq!(value["format"], "kakemono-content");
    assert!(value["blocks"].as_array().is_some_and(|blocks| !blocks.is_empty()));
    assert!(!json.contains('\u{1}'), "image placeholder left in the JSON");
    for (name, data) in files {
        assert!(json.contains(name.as_str()), "content JSON: {name} is not referenced");
        assert!(!data.is_empty(), "content JSON: {name} is empty");
    }
}

/// Checks the chunks: numbered in order, none empty or over the size, each
/// from existing blocks in reading order, and most of them under a heading.
fn check_chunks(chunks: &[kakemono::Chunk], n_blocks: usize, max_tokens: usize) {
    assert!(!chunks.is_empty(), "chunks: none");
    let mut last_block = 0;
    for (i, chunk) in chunks.iter().enumerate() {
        assert_eq!(chunk.id, i, "chunks: numbered out of order");
        assert!(!chunk.text.trim().is_empty(), "chunks: chunk {i} is empty");
        assert!(
            chunk.tokens <= max_tokens,
            "chunks: chunk {i} has {} tokens",
            chunk.tokens
        );
        assert!(
            chunk.blocks.iter().all(|&id| id < n_blocks),
            "chunks: chunk {i} names a missing block"
        );
        assert!(
            chunk.blocks.first().is_some_and(|&id| id >= last_block),
            "chunks: chunk {i} goes back"
        );
        last_block = *chunk.blocks.last().unwrap();
    }
    let under_heading = chunks.iter().filter(|chunk| !chunk.section.is_empty()).count();
    assert!(
        under_heading * 2 > chunks.len(),
        "chunks: only {under_heading} of {} under a heading",
        chunks.len()
    );
}

fn check_snapshot(id: &str, snap: &Snapshot) {
    let dir = data_dir().join("snapshots");
    let file = dir.join(format!("{id}.txt"));
    let json_file = dir.join(format!("{id}.json"));
    let keep_json = FULL_JSON.contains(&id);
    let text: String = snap.fingerprints.iter().map(|(m, h)| format!("{m} {h}\n")).collect();

    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, &text).unwrap();
        if keep_json {
            std::fs::write(&json_file, &snap.json).unwrap();
        }
        return;
    }

    let old = std::fs::read_to_string(&file).unwrap_or_else(|_| {
        panic!(
            "{id}: no snapshot at {}; create it with KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --test real_papers",
            file.display()
        )
    });
    if old == text {
        return;
    }
    let changed: Vec<&str> = snap
        .fingerprints
        .iter()
        .filter(|(m, h)| !old.lines().any(|l| l == format!("{m} {h}")))
        .map(|(m, _)| *m)
        .collect();
    let mut msg = format!(
        "{id}: the output changed ({}).\n\
         If the change is intended, run KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --test real_papers \
         and commit tests/data/snapshots.",
        changed.join(", ")
    );
    if keep_json && let Ok(old_json) = std::fs::read_to_string(&json_file) {
        let (a, b): (Vec<&str>, Vec<&str>) = (old_json.lines().collect(), snap.json.lines().collect());
        if let Some(i) = (0..a.len().max(b.len())).find(|&i| a.get(i) != b.get(i)) {
            msg.push_str(&format!("\nFirst difference in the JSON, line {}:", i + 1));
            for l in a.iter().skip(i).take(6) {
                msg.push_str(&format!("\n  - {l}"));
            }
            for l in b.iter().skip(i).take(6) {
                msg.push_str(&format!("\n  + {l}"));
            }
        }
    }
    panic!("{msg}");
}

/// Renders every mode of an analysed paper and measures it.
fn measure(paper: &Paper, analysis: &kakemono::Analysis) -> (Metrics, Snapshot) {
    let doc = &analysis.doc;

    let title = doc
        .blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Title))
        .map(|b| plain(&b.spans))
        .unwrap_or_default();
    let tables = doc.blocks.iter().filter(|b| b.table.is_some()).count();
    let math = doc.blocks.iter().filter(|b| b.math.is_some()).count();

    // A paragraph that stops mid-sentence and is followed by one that
    // continues it: the text was split where it should not have been.
    let paragraphs: Vec<String> = doc
        .blocks
        .iter()
        .filter(|b| matches!(b.kind, BlockKind::Paragraph { .. }))
        .map(|b| plain(&b.spans))
        .collect();
    let broken_paragraphs = paragraphs
        .windows(2)
        .filter(|w| {
            let ends_open = !w[0].ends_with(['.', '!', '?', ':', ';', ')', ']', '"', '\u{201d}']);
            let continues = w[1].chars().next().is_some_and(|c| c.is_lowercase());
            ends_open && continues
        })
        .count();
    // Words still split by a line-end hyphen ("trans- lation"), leaving out
    // shared prefixes such as "first- and second-order".
    let split = Regex::new(r"\b[a-z]{2,}- ([a-z]{2,})\b").unwrap();
    let split_words = paragraphs
        .iter()
        .flat_map(|p| split.captures_iter(p))
        .filter(|c| !matches!(&c[1], "and" | "or" | "to"))
        .count();

    // Lower-case words joined by a hyphen ("two-stage", "trans-lation"). Real
    // compounds keep this count stable; a word broken at the end of a line
    // and not joined back raises it.
    let compound = Regex::new(r"\b[a-z]{2,}-[a-z]{2,}\b").unwrap();
    let hyphenated = paragraphs.iter().map(|p| compound.find_iter(p).count()).sum();

    let json = serde_json::to_string_pretty(doc).expect("serialisable document");
    // Edited text is recognised by its missing per-line text (see
    // render::json::apply): the analysis itself never leaves it out.
    for block in &doc.blocks {
        assert!(
            matches!(block.kind, BlockKind::Equation { .. })
                || block.lines.is_empty()
                || block.spans.is_empty()
                || !block.hlines.is_empty(),
            "block {} has lines but no per-line text: it would read as edited",
            block.id
        );
        assert!(
            block.inner_lines.is_empty() || !block.inner_spans.is_empty(),
            "float {} has lines but no per-line text: its table would read as edited",
            block.id
        );
    }
    let mut fingerprints = Vec::new();
    let mut glyphs_left = 0;
    for mode in [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly] {
        let out = paper.to_html(
            analysis,
            &HtmlOptions {
                mode,
                inline_images: false,
                ..Default::default()
            },
        );
        assert!(out.html.contains("</html>"), "{mode:?}: incomplete document");
        assert!(
            !out.html.contains('\u{1}'),
            "{mode:?}: image placeholder left in the HTML"
        );
        for (name, data) in &out.files {
            assert!(out.html.contains(name.as_str()), "{mode:?}: {name} is not referenced");
            assert!(!data.is_empty(), "{mode:?}: {name} is empty");
        }
        if matches!(mode, Mode::Medium | Mode::Friendly) {
            assert_eq!(
                out.html.matches("<math display=\"block\"").count(),
                math,
                "{mode:?}: every rebuilt formula is written as MathML"
            );
        }
        fingerprints.push((mode.name(), fingerprint(&out.html, &out.files)));
        if mode == Mode::Hard95 {
            // Text inside figures stays in the page pictures on purpose (it
            // is drawn in the figure's own order); the rest should be HTML.
            // Glyphs are counted in SVG pictures, so every page is one.
            let out = paper.to_html(
                analysis,
                &HtmlOptions {
                    mode,
                    inline_images: false,
                    page_image: PageImage::SvgOnly,
                    ..Default::default()
                },
            );
            let in_pictures: usize = out
                .files
                .iter()
                .filter(|(n, _)| n.ends_with(".svg"))
                .map(|(_, d)| d.windows(5).filter(|w| w == b"<use ").count())
                .sum();
            glyphs_left = in_pictures.saturating_sub(figure_text_chars(&out.html) + picture_glyph_chars(&out.html));
        }
    }
    // Friendly mode with every formula as a picture.
    let out = paper.to_html(
        analysis,
        &HtmlOptions {
            mode: Mode::Friendly,
            inline_images: false,
            equations: Equations::Image,
            ..Default::default()
        },
    );
    assert!(!out.html.contains("<math "), "formulas as pictures only");
    fingerprints.push(("friendly-image", fingerprint(&out.html, &out.files)));
    fingerprints.push(("json", hex(Sha256::digest(json.as_bytes()))));
    // The Word document.
    let docx = paper.to_docx(analysis, &DocxOptions::default());
    let parts = zip_parts(&docx);
    check_docx(&parts, math);
    fingerprints.push(("docx", package_fingerprint(&parts)));
    // The Markdown document, with its pictures as files.
    let md = paper.to_markdown(analysis, &MarkdownOptions::default());
    check_markdown(&md.markdown, &md.files, math);
    fingerprints.push(("markdown", fingerprint(&md.markdown, &md.files)));
    // Friendly Markdown adapts every table to a pipe table; medium keeps
    // the HTML tables of those with spanning cells.
    assert!(
        !md.markdown.contains("<table>"),
        "markdown: friendly tables are pipe tables"
    );
    let medium = paper.to_markdown(
        analysis,
        &MarkdownOptions {
            mode: Mode::Medium,
            ..MarkdownOptions::default()
        },
    );
    check_markdown(&medium.markdown, &medium.files, math);
    fingerprints.push(("markdown-medium", fingerprint(&medium.markdown, &medium.files)));
    // The LaTeX document, with its pictures.
    let tex = paper.to_latex(analysis, &LatexOptions::default());
    check_latex(&tex.latex, &tex.files, math);
    fingerprints.push(("latex", fingerprint(&tex.latex, &tex.files)));
    // The EPUB book.
    // Formulas wider than a phone screen are pictures in the book.
    let epub_opts = EpubOptions::default();
    let narrow_math = doc
        .blocks
        .iter()
        .filter(|b| match &b.kind {
            BlockKind::Equation { image, .. } => {
                b.math.is_some()
                    && image.is_none_or(|image| image.rect.width() <= epub_opts.max_math_width_em * doc.body_size)
            }
            _ => b.math.is_some(),
        })
        .count();
    let epub = zip_parts(&paper.to_epub(analysis, &epub_opts));
    check_epub(&epub, narrow_math);
    fingerprints.push(("epub", package_fingerprint(&epub)));
    // The content as JSON, with its pictures as files.
    let content = paper.to_json(analysis, &JsonOptions::default());
    check_content(&content.json, &content.files);
    // The `generator` line holds the crate version: leave it out, or every
    // release would change the fingerprint.
    let generator = regex::Regex::new(r#"(?m)^  "generator": "kakemono [^"]*",\n"#).unwrap();
    let stable_json = generator.replace(&content.json, "");
    assert_ne!(stable_json, content.json, "the content JSON has a generator line");
    fingerprints.push(("content", fingerprint(&stable_json, &content.files)));
    // The chunks for retrieval.
    let chunk_opts = ChunkOptions::default();
    let chunks = paper.to_chunks(analysis, &chunk_opts);
    check_chunks(&chunks, doc.blocks.len(), chunk_opts.max_tokens);
    fingerprints.push((
        "chunks",
        hex(Sha256::digest(kakemono::chunks_jsonl(&chunks).as_bytes())),
    ));
    let metrics = Metrics {
        title,
        tables,
        broken_paragraphs,
        split_words,
        glyphs_left,
        hyphenated,
        math,
    };
    (metrics, Snapshot { fingerprints, json })
}

fn check(e: &Expect) -> Option<Metrics> {
    let Analysed { paper, analysis } = analysed(e.id)?;
    let (m, snapshot) = measure(paper, analysis);
    if std::env::var("KAKEMONO_PRINT_METRICS").is_ok() {
        eprintln!("{}: {m:?}", e.id);
    }
    assert!(m.title.contains(e.title), "{}: title {:?}", e.id, m.title);
    assert!(
        m.tables >= e.min_tables,
        "{}: {} tables rebuilt, expected at least {}",
        e.id,
        m.tables,
        e.min_tables
    );
    assert!(
        m.broken_paragraphs <= e.max_broken_paragraphs,
        "{}: {} paragraphs broken mid-sentence, expected at most {}",
        e.id,
        m.broken_paragraphs,
        e.max_broken_paragraphs
    );
    assert!(
        m.split_words <= e.max_split_words,
        "{}: {} words still split by a hyphen, expected at most {}",
        e.id,
        m.split_words,
        e.max_split_words
    );
    assert!(
        m.glyphs_left <= e.max_glyphs_left,
        "{}: {} glyphs left in hard-95 page pictures, expected at most {}",
        e.id,
        m.glyphs_left,
        e.max_glyphs_left
    );
    assert!(
        m.hyphenated <= e.max_hyphenated,
        "{}: {} hyphenated words, expected at most {} (line-end hyphens not removed?)",
        e.id,
        m.hyphenated,
        e.max_hyphenated
    );
    assert!(
        m.math >= e.min_math,
        "{}: {} formulas rebuilt as MathML, expected at least {}",
        e.id,
        m.math,
        e.min_math
    );
    check_snapshot(e.id, &snapshot);
    Some(m)
}

/// The rebuilt table whose caption starts with `label` ("Table 2", followed by
/// a colon or a full stop), as rows of cell texts. `None` when the paper is
/// not downloaded.
fn table(id: &str, label: &str) -> Option<Vec<Vec<String>>> {
    let Analysed { analysis, .. } = analysed(id)?;
    let block = analysis
        .doc
        .blocks
        .iter()
        .find(|b| {
            let caption = plain(&b.spans);
            caption.starts_with(&format!("{label}:")) || caption.starts_with(&format!("{label}."))
        })
        .unwrap_or_else(|| panic!("{id}: no caption starting with {label:?}"));
    let t = block
        .table
        .as_ref()
        .unwrap_or_else(|| panic!("{id}: {label} was not rebuilt"));
    Some(
        t.rows
            .iter()
            .map(|r| r.iter().map(|c| plain(&c.spans)).collect())
            .collect(),
    )
}

macro_rules! paper_test {
    ($name:ident, $id:literal, $title:literal, tables >= $t:literal, broken <= $b:literal, split <= $s:literal, glyphs <= $g:literal, hyphenated <= $h:literal, math >= $m:literal) => {
        #[test]
        fn $name() {
            check(&Expect {
                id: $id,
                title: $title,
                min_tables: $t,
                max_broken_paragraphs: $b,
                max_split_words: $s,
                max_glyphs_left: $g,
                max_hyphenated: $h,
                min_math: $m,
            });
        }
    };
}

paper_test!(
    attention,
    "1706.03762v7",
    "Attention Is All You Need",
    tables >= 4,
    broken <= 1,
    split <= 0,
    glyphs <= 85,
    // "sequence-aligned" twice, broken at its hyphen.
    hyphenated <= 91,
    math >= 5
);
paper_test!(
    resnet,
    "1512.03385v1",
    "Deep Residual Learning",
    tables >= 13,
    broken <= 0,
    split <= 0,
    glyphs <= 0,
    hyphenated <= 87,
    math >= 2
);
paper_test!(
    bert,
    "1810.04805v2",
    "BERT",
    tables >= 8,
    broken <= 2,
    split <= 0,
    glyphs <= 0,
    hyphenated <= 228,
    math >= 1
);
paper_test!(
    vit,
    "2010.11929v2",
    "AN IMAGE IS WORTH 16X16 WORDS",
    tables >= 8,
    broken <= 2,
    split <= 0,
    glyphs <= 2381,
    hyphenated <= 186,
    math >= 6
);
paper_test!(
    ligo,
    "1602.03837v1",
    "Observation of Gravitational Waves",
    tables >= 1,
    broken <= 1,
    split <= 0,
    glyphs <= 3,
    // Two compounds ("gravitational-wave", "signal-to-noise") are in the
    // column beside Fig. 2, which the figure no longer swallows.
    hyphenated <= 76,
    // Its one formula has brackets without a Unicode value: a picture.
    math >= 0
);
paper_test!(
    faster_rcnn,
    "1506.01497v3",
    "Faster R-CNN",
    tables >= 12,
    broken <= 3,
    split <= 0,
    // 188 on p10, the rest are labels in the detection examples of p12-p13,
    // drawn twice (outline and fill) and kept in the figure pictures. The
    // labels at the top of p12-p13 are in them since they stopped being taken
    // for running heads (15 more glyphs).
    glyphs <= 410,
    hyphenated <= 162,
    math >= 2
);
paper_test!(
    gpt4,
    "2303.08774v6",
    "GPT-4 Technical Report",
    tables >= 8,
    // The lines of its code listings are no longer paragraphs (45 before).
    broken <= 20,
    split <= 0,
    glyphs <= 498,
    hyphenated <= 302,
    math >= 7
);
paper_test!(
    adam,
    "1412.6980v9",
    "ADAM",
    tables >= 0,
    broken <= 0,
    split <= 0,
    glyphs <= 806,
    hyphenated <= 54,
    math >= 34
);

/// Table 2 of the Transformer paper: "EN-DE" and "EN-FR" are two columns
/// under a shared header ("BLEU"), not one merged cell.
#[test]
fn attention_table_columns_are_split() {
    let Some(rows) = table("1706.03762v7", "Table 2") else {
        return;
    };
    assert!(
        rows.iter()
            .any(|r| r.contains(&"EN-DE".to_string()) && r.contains(&"EN-FR".to_string())),
        "no row with separate EN-DE and EN-FR cells: {:?}",
        &rows[..rows.len().min(3)]
    );
}

/// The header rows of BERT's Tables 2, 3 and 4 sit at the top of a page and
/// repeat the same words; they must not be removed as running heads.
#[test]
fn bert_table_headers_are_kept() {
    for label in ["Table 2", "Table 3", "Table 4"] {
        let Some(rows) = table("1810.04805v2", label) else {
            return;
        };
        assert_eq!(rows[0], ["System", "Dev", "Test"], "{label}: header row");
    }
}

/// The tables of BERT whose numbers are set tightly: "QNLI" and "MRPC", and
/// the two scores under them, are cells of their own, not one cell of two
/// values (Table 5), and so are "MNLI" and "NER" with their scores (Table 8).
/// The header that centres "Dev Set" over several columns must not hide the
/// corridors between them.
#[test]
fn bert_tight_numeric_columns_are_split() {
    let Some(rows) = table("1810.04805v2", "Table 5") else {
        return;
    };
    assert_eq!(
        rows[1],
        ["Tasks", "MNLI-m", "QNLI", "MRPC", "SST-2", "SQuAD"],
        "Table 5: header"
    );
    assert_eq!(
        rows[3],
        ["BERTBASE", "84.4", "88.4", "86.7", "92.7", "88.5"],
        "Table 5: first row"
    );
    let Some(rows) = table("1810.04805v2", "Table 8") else {
        return;
    };
    assert_eq!(rows[1], ["MASK", "SAME", "RND", "MNLI", "NER"], "Table 8: header");
    assert_eq!(
        rows[3],
        ["80%", "10%", "10%", "84.2", "95.4", "94.9"],
        "Table 8: first row"
    );
}

/// Table 11 of the GPT-4 report: "Contamination" and "GPT-4 (non-" are two
/// columns, and so are the two values of a row such as "25%" and "65.58%".
#[test]
fn gpt4_contamination_table_columns_are_split() {
    let Some(rows) = table("2303.08774v6", "Table 11") else {
        return;
    };
    assert_eq!(
        rows[0],
        [
            "Benchmark",
            "GPT-4",
            "GPT-3.5",
            "Contamination",
            "GPT-4 (non-",
            "Degradation"
        ],
        "header"
    );
    let human_eval = rows.iter().find(|row| row[0] == "HumanEval").expect("a HumanEval row");
    assert_eq!(human_eval, &["HumanEval", "67.0%", "48.1%", "25%", "65.58%", "-2.12%"]);
}

/// The header rows of the detection tables of Faster R-CNN and ResNet hold
/// the twenty class names, one cell each: "person" and "plant", and "train"
/// and "tv", are not merged, and the numbers below them are not either.
#[test]
fn detection_tables_keep_every_class_column() {
    for (id, label, first, second) in [
        ("1506.01497v3", "Table 6", "person", "plant"),
        ("1512.03385v1", "Table 10", "train", "tv"),
    ] {
        let Some(rows) = table(id, label) else {
            return;
        };
        let header = &rows[0];
        let at = header
            .iter()
            .position(|cell| cell == first)
            .unwrap_or_else(|| panic!("{label}: no {first:?} cell in {header:?}"));
        assert_eq!(header[at + 1], second, "{label}: {first:?} is followed by {second:?}");
        let merged = rows[1..]
            .iter()
            .flat_map(|row| row.iter().skip(2))
            .find(|cell| cell.contains(' '));
        assert!(merged.is_none(), "{label}: a cell holds two numbers: {merged:?}");
    }
}

/// The APS fonts of the LIGO paper declare a text encoding but keep the TeX
/// math layout: the glyph coded as `=` is a slash and, in the Math Pi font,
/// `f` and `g` are braces. The text must say what the glyphs show.
#[test]
fn mislabelled_math_glyphs_are_read_by_shape() {
    let Some(Analysed { analysis, .. }) = analysed("1602.03837v1") else {
        return;
    };
    let text: String = analysis
        .doc
        .blocks
        .iter()
        .map(|block| plain(&block.spans) + "\n")
        .collect();
    for expected in ["ρ/{[1 + (χ", "erg/s", "(m1 + m2)1/5"] {
        assert!(text.contains(expected), "missing {expected:?}");
    }
    // The first page's foot is furniture, out of the text, read the same way.
    let foot: String = analysis
        .loose
        .iter()
        .flatten()
        .filter(|line| line.line.page == 0)
        .map(|line| line.line.text() + "\n")
        .collect();
    assert!(
        foot.contains("0031-9007/16/116(6)/061102"),
        "{foot} / {:?}",
        analysis
            .loose
            .iter()
            .map(|page| page.iter().map(|line| line.line.text()).collect::<Vec<_>>())
            .take(2)
            .collect::<Vec<_>>()
    );
    assert!(!text.contains("061102-1"), "the first page's foot is in the text");
    assert!(!text.contains("erg=s"), "a slash read as an equals sign");
}

/// The algorithms of the Adam paper are rebuilt as text in the friendly
/// mode: numbered statements, indentation and right-aligned comments.
#[test]
fn adam_algorithms_are_text() {
    let Some(Analysed { paper, analysis }) = analysed("1412.6980v9") else {
        return;
    };
    let html = paper.to_html(analysis, &HtmlOptions::default()).html;
    assert_eq!(
        html.matches("<div class=\"algo\">").count(),
        2,
        "both algorithms rebuilt"
    );
    assert!(html.contains("<span class=\"cmt\">(Update parameters)</span>"));
    assert!(html.contains("<div class=\"ln req\"><i></i><div class=\"st in0\"><span><strong>Require: </strong>"));
    assert!(html.contains("<div class=\"st in1\"><span><em>t </em>← <em>t </em>+ 1</span>"));
    assert!(!html.contains("alt=\"Algorithm 1"), "no picture of the algorithm");
}

/// The appendix of the Adam paper stacks fractions of the same width in
/// several equations: their bars are not the rules of a table, so no
/// picture is cut out of the text. A wrapped "T ≥ 1." stays in its
/// paragraph instead of joining the display below it.
#[test]
fn adam_fraction_bars_are_not_tables() {
    let Some(Analysed { analysis, .. }) = analysed("1412.6980v9") else {
        return;
    };
    let graphics: Vec<usize> = analysis
        .doc
        .blocks
        .iter()
        .filter(|block| {
            matches!(
                block.kind,
                BlockKind::Figure {
                    kind: FloatKind::Graphic,
                    ..
                }
            )
        })
        .map(|block| block.page + 1)
        .collect();
    assert!(
        graphics.is_empty(),
        "graphics cut out of equations on pages {graphics:?}"
    );
    let corollary = analysis
        .doc
        .blocks
        .iter()
        .map(|block| plain(&block.spans))
        .find(|text| text.starts_with("Corollary 4.2."))
        .expect("Corollary 4.2");
    assert!(corollary.trim_end().ends_with("T ≥ 1."), "{corollary}");
}

/// The affiliations at the foot of the first page of Faster R-CNN are set as
/// bulleted footnotes below a rule that is centred in the column, not flush
/// with its left edge, and in a smaller size. They are footnotes without a
/// mark (the bullet is dropped), not a bulleted list in the middle of the
/// introduction.
#[test]
fn faster_rcnn_affiliations_are_footnotes() {
    let Some(Analysed { analysis, .. }) = analysed("1506.01497v3") else {
        return;
    };
    let blocks = &analysis.doc.blocks;
    for start in ["S. Ren is with", "K. He and J. Sun are with", "R. Girshick is with"] {
        let block = blocks
            .iter()
            .find(|block| {
                plain(&block.spans)
                    .trim_start_matches(['\u{2022}', ' '])
                    .starts_with(start)
            })
            .unwrap_or_else(|| panic!("no block starting {start:?}"));
        assert!(
            matches!(&block.kind, BlockKind::Footnote { marker } if marker.is_empty()),
            "{start:?} is {:?}",
            block.kind
        );
        assert!(!plain(&block.spans).starts_with('\u{2022}'), "bullet left in {start:?}");
    }
    assert!(
        !blocks
            .iter()
            .any(|block| matches!(block.kind, BlockKind::ListItem { .. }) && block.page == 0),
        "a list left on the first page"
    );
}

/// Equation (2) of Faster R-CNN is two lines with its number between them
/// ("(2)" is a row of its own at the right margin): one numbered equation,
/// not two pictures with a paragraph "(2)" between them.
#[test]
fn faster_rcnn_equation_number_is_attached() {
    let Some(Analysed { analysis, .. }) = analysed("1506.01497v3") else {
        return;
    };
    let blocks = &analysis.doc.blocks;
    assert!(
        blocks
            .iter()
            .any(|block| matches!(&block.kind, BlockKind::Equation { number: Some(n), .. } if n == "(2)")),
        "equation (2) has no number"
    );
    assert!(
        !blocks.iter().any(|block| plain(&block.spans) == "(2)"),
        "the number of equation (2) is a paragraph of its own"
    );
}

/// Faster R-CNN numbers its footnotes on the baseline ("1. Since ..."):
/// they are footnotes, not list items, and the paragraph that runs past
/// one into the next column stays whole. The caption of Table 4 ends in
/// URLs set small; it still pairs with the table below it, not with
/// Table 3 above. Its bold run-in heads that wrap onto a second line are
/// not headings.
#[test]
fn faster_rcnn_footnotes_and_tables() {
    let Some(Analysed { paper, analysis }) = analysed("1506.01497v3") else {
        return;
    };
    let blocks = &analysis.doc.blocks;
    let markers: Vec<&str> = blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Footnote { marker } if !marker.is_empty() => Some(marker.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(markers, ["1", "2", "3", "4", "5", "6", "7", "8"]);
    assert!(
        blocks
            .iter()
            .any(|block| plain(&block.spans).contains("built in commercial systems such as at Pinterests")),
        "paragraph split by footnote 1"
    );
    // Bold run-in heads long enough to fill a line stay in their paragraph.
    for head in [
        "One-Stage Detection vs. Two-Stage Proposal + Detection. The OverFeat paper",
        "Faster R-CNN in ILSVRC & COCO 2015 competitions We have demonstrated",
    ] {
        assert!(
            blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Paragraph { .. }) && plain(&block.spans).starts_with(head)),
            "run-in head not kept in its paragraph: {head:?}"
        );
    }
    let tables: Vec<(String, f32)> = blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Figure { label: Some(label), .. } if block.page == 7 => Some((label.clone(), block.bbox.y0)),
            _ => None,
        })
        .collect();
    let y = |label: &str| tables.iter().find(|(l, _)| l == label).map(|(_, y)| *y);
    assert!(y("Table 4").unwrap() > y("Table 3").unwrap() + 100.0, "{tables:?}");
    let graphics = blocks
        .iter()
        .filter(|block| block.page == 7 && matches!(block.kind, BlockKind::Figure { label: None, .. }))
        .count();
    assert_eq!(graphics, 0, "Table 4 left without its caption");
    // A URL broken across lines after "_" is one link.
    let html = paper.to_html(analysis, &HtmlOptions::default()).html;
    assert!(
        html.contains("href=\"https://github.com/shaoqingren/faster_rcnn\""),
        "broken GitHub link"
    );
    assert!(html.contains("href=\"https://github.com/rbgirshick/py-faster-rcnn\""));
}

/// GPT-4's references break URLs across lines after ".", "-" and "%": each
/// is one link, with its hyphens, except where TeX hyphenated a word
/// ("ope-" / "nai.com").
#[test]
fn urls_broken_across_lines_are_whole() {
    let Some(Analysed { paper, analysis }) = analysed("2303.08774v6") else {
        return;
    };
    let html = paper.to_html(analysis, &HtmlOptions::default()).html;
    for url in [
        "https://github.com/inverse-scaling/prize",
        "https://bounded-regret.ghost.io/emergent-deception-optimization/",
        "https://satsuite.collegeboard.org/media/pdf/understanding-sat-scores.pdf",
        "https://openai.com/research/language-unsupervised",
    ] {
        assert!(html.contains(&format!("href=\"{url}\"")), "missing link {url}");
    }
    assert!(!html.contains("href=\"https://github\""), "a link cut at a line break");
}

/// No picture cut out of a page (equation or figure) cuts through a glyph
/// of something else: the pictures show whole letters, never halves of
/// the text around them.
#[test]
fn pictures_do_not_cut_text() {
    let mut report = Vec::new();
    for id in [
        "1412.6980v9",
        "1506.01497v3",
        "1512.03385v1",
        "1602.03837v1",
        "1706.03762v7",
        "1810.04805v2",
        "2010.11929v2",
        "2303.08774v6",
    ] {
        let Some(Analysed { analysis, .. }) = analysed(id) else {
            continue;
        };
        for cut in kakemono::analysis::crops::cut_glyphs(&analysis.doc, &analysis.loose) {
            report.push(format!(
                "{id} p{} {} #{}: {} glyphs cut by [{:.0},{:.0} {:.0}x{:.0}]: {}",
                cut.page + 1,
                cut.kind,
                cut.block,
                cut.glyphs,
                cut.rect.x0,
                cut.rect.y0,
                cut.rect.width(),
                cut.rect.height(),
                cut.examples.join(" ")
            ));
        }
    }
    eprintln!("{}", report.join("\n"));
    assert!(report.is_empty(), "{} pictures cut text", report.len());
}

/// The output does not depend on the number of threads (on a real paper).
#[test]
fn threads_do_not_change_real_output() {
    let Some(paper) = load("1602.03837v1") else { return };
    let render = |jobs: usize| {
        let analysis = paper.analyze_with(AnalyzeOptions {
            jobs,
            ..Default::default()
        });
        [Mode::Hard95, Mode::Friendly].map(|mode| {
            paper
                .to_html(
                    &analysis,
                    &HtmlOptions {
                        mode,
                        jobs,
                        ..Default::default()
                    },
                )
                .html
        })
    };
    assert!(render(1) == render(4), "output differs between 1 and 4 threads");
}

/// The first line of a caption set next to a figure has few words, and
/// justification stretches its spaces past the gap that separates columns
/// ("Figure 3:   Transfer to ImageNet.     While"): the caption still runs
/// on to its last line, and the paragraph cut by the figures stays whole.
/// In hard-95 the text inside figures stays in the page picture (a legend
/// box covers tick labels there), with a transparent copy for selecting.
#[test]
fn vit_captions_and_figure_text() {
    let Some(Analysed { paper, analysis }) = analysed("2010.11929v2") else {
        return;
    };
    let blocks = &analysis.doc.blocks;
    let caption = |label: &str| {
        blocks
            .iter()
            .find(|block| matches!(&block.kind, BlockKind::Figure { label: Some(l), .. } if l == label))
            .map(|block| plain(&block.spans))
            .unwrap_or_default()
    };
    assert!(
        caption("Figure 3").ends_with("overtake smaller ones as the dataset grows."),
        "{}",
        caption("Figure 3")
    );
    assert!(
        blocks
            .iter()
            .any(|block| plain(&block.spans)
                .contains("Figure 3 also shows the performance region spanned by BiT models")),
        "paragraph cut by Figures 3 and 4"
    );
    let html = paper
        .to_html(
            analysis,
            &HtmlOptions {
                mode: Mode::Hard95,
                ..Default::default()
            },
        )
        .html;
    assert!(html.contains("data-kind=\"figure-text\""));
    assert!(
        html.contains(".b[data-kind=\"figure-text\"],.b[data-kind=\"figure-text\"] *{color:transparent!important}")
    );
    // The rotated axis titles of Figs. 3 to 5 are part of the page picture
    // with a transparent copy, not drawn a second time as visible text.
    let rotated = html
        .split("data-kind=\"rotated-text\"")
        .skip(1)
        .map(|part| part.split("</div>").next().unwrap_or_default())
        .collect::<String>();
    assert!(!rotated.contains("Transfer accuracy"), "axis title drawn twice");
    assert!(
        html.contains("Transfer accuracy [%]"),
        "axis title is still there to select"
    );
}

/// A word space that a tightly set line shrank below a space (the PDF has
/// no space character there, only a gap) is still a space: "LIGO and
/// construction" in the acknowledgements of the LIGO paper.
#[test]
fn ligo_tight_word_spaces() {
    let Some(Analysed { analysis, .. }) = analysed("1602.03837v1") else {
        return;
    };
    let text: String = analysis
        .doc
        .blocks
        .iter()
        .map(|block| plain(&block.spans))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("LIGO and construction and operation of the GEO"),
        "{text}"
    );
    assert!(!text.contains("andconstruction"));
}

/// The rule under the running head of the LIGO paper does not join the
/// figure below it: Fig. 2 stays in its column and the text of the other
/// column stays text. The display equation beside it is a picture.
#[test]
fn ligo_figure_keeps_to_its_column() {
    let Some(Analysed { analysis, .. }) = analysed("1602.03837v1") else {
        return;
    };
    let blocks = &analysis.doc.blocks;
    let fig = blocks
        .iter()
        .find(|block| matches!(&block.kind, BlockKind::Figure { label: Some(l), .. } if l == "FIG. 2"))
        .expect("Fig. 2");
    assert!(fig.bbox.x0 > 300.0, "Fig. 2 spans the page: {:?}", fig.bbox);
    assert!(
        blocks
            .iter()
            .any(|block| plain(&block.spans).starts_with("Only the LIGO detectors were observing"))
    );
    assert!(blocks.iter().any(|block| block.page == 2
        && matches!(block.kind, BlockKind::Equation { .. })
        && plain(&block.spans).contains("(m1 + m2)1/5")));
}

/// Chart titles and axis titles above a chart are part of the figure, not
/// headings or paragraphs (GPT-4 Figures 1-3).
#[test]
fn gpt4_chart_titles_stay_in_figures() {
    let Some(Analysed { analysis, .. }) = analysed("2303.08774v6") else {
        return;
    };
    for title in [
        "OpenAI codebase next word prediction",
        "Capability prediction on 23 coding problems",
        "Inverse scaling prize, hindsight neglect",
    ] {
        assert!(
            !analysis
                .doc
                .blocks
                .iter()
                .any(|block| !matches!(block.kind, BlockKind::Figure { .. }) && plain(&block.spans).trim() == title),
            "{title:?} left out of its figure"
        );
    }
}

/// The first page's foot of the LIGO paper sets the journal line on the
/// row of the page id; the whole row is furniture, not the end of the
/// paragraph above it.
#[test]
fn ligo_first_page_foot_is_furniture() {
    let Some(Analysed { analysis, .. }) = analysed("1602.03837v1") else {
        return;
    };
    assert!(!analysis.doc.blocks.iter().any(|block| {
        plain(&block.spans).contains("Published by the American Physical Society")
            && !plain(&block.spans).contains("Creative Commons")
    }));
}

/// Text of the blocks of the GPT-4 paper whose kind satisfies `pred`.
fn gpt4_blocks_where(pred: impl Fn(&BlockKind) -> bool) -> Option<Vec<String>> {
    let Analysed { analysis, .. } = analysed("2303.08774v6")?;
    Some(
        analysis
            .doc
            .blocks
            .iter()
            .filter(|block| pred(&block.kind))
            .map(|block| plain(&block.spans).trim().to_string())
            .collect(),
    )
}

/// The numbered harm list, the outline and the RBRM steps of the system
/// card are list items, not level-1 headings (they are set larger than the
/// 10 pt of the report, but they are the body size of the system card).
#[test]
fn gpt4_numbered_lists_are_not_headings() {
    let Some(headings) = gpt4_blocks_where(|kind| matches!(kind, BlockKind::Heading { .. })) else {
        return;
    };
    for label in [
        "Advice or encouragement for self harm",
        "Graphic material such as erotic",
        "Evaluation Approach (As Described Above)",
        "2. Model Mitigations",
        "3. System Safety",
        "Pass a prompt through GPT-4 model",
        "Pass prompt + new response through GPT-4",
    ] {
        assert!(
            !headings.iter().any(|heading| heading.contains(label)),
            "{label:?} is a heading"
        );
    }
    let items = gpt4_blocks_where(|kind| matches!(kind, BlockKind::ListItem { .. })).unwrap();
    assert!(
        items
            .iter()
            .any(|item| item.contains("Advice or encouragement for self harm"))
    );
    assert!(
        items
            .iter()
            .any(|item| item.contains("Pass a prompt through GPT-4 model"))
    );
}

/// Lead-in sentences ending in a colon, the tail of a bullet, bold notes
/// and reference entries are not headings; neither are the running titles
/// at the head of the system card.
#[test]
fn gpt4_lead_ins_and_labels_are_not_headings() {
    let Some(headings) = gpt4_blocks_where(|kind| matches!(kind, BlockKind::Heading { .. })) else {
        return;
    };
    for text in [
        "Some of the tasks ARC tested include:",
        "The red teamer augmented GPT-4 with a set of tools:",
        "safe usage.",
        "GPT-4 System Card",
        "OpenAI",
    ] {
        assert!(!headings.iter().any(|heading| heading == text), "{text:?} is a heading");
    }
    for prefix in ["CW:", "[2]", "[93]"] {
        assert!(
            !headings.iter().any(|heading| heading.starts_with(prefix)),
            "{prefix:?} starts a heading"
        );
    }
}

/// The "GPT-4 (early) response" labels of the appendix repeat dozens of
/// times: they are not sections, while "D.1 Prompt 1" is.
#[test]
fn gpt4_response_labels_are_not_headings() {
    let Some(headings) = gpt4_blocks_where(|kind| matches!(kind, BlockKind::Heading { .. })) else {
        return;
    };
    assert!(!headings.iter().any(|heading| heading.starts_with("GPT-4 (")));
    assert!(headings.iter().any(|heading| heading == "D.1 Prompt 1"));
    assert!(headings.iter().any(|heading| heading == "2.3 Harmful Content"));
    assert!(headings.iter().any(|heading| heading == "A Exam Benchmark Methodology"));
}

/// Formulas inside running text are written as math: LaTeX in Markdown,
/// MathML in HTML, Office Math in Word.
#[test]
fn adam_inline_formulas_are_math() {
    let Some(Analysed { paper, analysis }) = analysed("1412.6980v9") else {
        return;
    };
    // "the online prediction fₜ(θₜ) and": one formula, brackets included.
    let md = paper.to_markdown(analysis, &MarkdownOptions::default()).markdown;
    for formula in [
        "online prediction $f_{t}(\\theta_{t})$ and",
        "$\\alpha=0.001$, $\\beta_{1}=0.9$, $\\beta_{2}=0.999$ and $\\varepsilon=10^{-8}$",
        "$O(\\sqrt{T})$ regret bound",
        "\\lambda\\in(0,1)$",
    ] {
        assert!(md.contains(formula), "markdown: {formula} not found");
    }
    let html = paper.to_html(analysis, &HtmlOptions::default()).html;
    assert!(html.contains(
        "online prediction <math class=\"eq-math im\" alttext=\"ft(θt)\"><mrow><msub><mi>f</mi><mi>t</mi></msub>"
    ));
    let parts = zip_parts(&paper.to_docx(analysis, &DocxOptions::default()));
    let document = &parts.iter().find(|(name, _)| name == "word/document.xml").unwrap().1;
    let document = String::from_utf8_lossy(document);
    assert!(document.matches("<m:oMath>").count() > document.matches("<m:oMathPara>").count() + 100);
}

/// Small fractions inside the text are read whole, in every format: terms
/// with scripts of their own (β₁² over √β₂), a fraction in a superscript
/// (t^(−1/2)), a radical over a term, a bracket with a power closing the
/// formula, a term with many scripts (g⁴_{T,i} over 4‖g₁:T,i‖²₂). They used to come out as loose scripts ("β2 √β12", "t−12").
#[test]
fn adam_inline_fractions_are_read_whole() {
    let Some(Analysed { paper, analysis }) = analysed("1412.6980v9") else {
        return;
    };
    let md = paper.to_markdown(analysis, &MarkdownOptions::default()).markdown;
    for formula in [
        "$\\gamma\\triangleq\\beta_{1}^{2}/\\sqrt{\\beta_{2}}$",
        "rate of $t^{-1/2}$ and",
        "$\\sqrt{1-\\beta_{2}^{t}}/((1-\\beta_{1}^{t})^{2})\\le1/((1-\\beta_{1})^{2})$",
        "$\\sum_{t}t\\gamma^{t}\\lt1/((1-\\gamma)^{2})$:",
        "+g_{T,i}^{4}/(4\\|g_{1:T,i}\\|_{2}^{2})\\ge",
    ] {
        assert!(md.contains(formula), "markdown: {formula} not found");
    }
    let tex = paper.to_latex(analysis, &LatexOptions::default()).latex;
    assert!(tex.contains("\\gamma\\triangleq\\beta_{1}^{2}/\\sqrt{\\beta_{2}}"));
    let html = paper.to_html(analysis, &HtmlOptions::default()).html;
    assert!(html.contains(
        "<mo>≜</mo><msubsup><mi>β</mi><mn>1</mn><mn>2</mn></msubsup><mo stretchy=\"false\" lspace=\"0\" rspace=\"0\">/</mo><msqrt>"
    ));
    let parts = zip_parts(&paper.to_docx(analysis, &DocxOptions::default()));
    let document = &parts.iter().find(|(name, _)| name == "word/document.xml").unwrap().1;
    assert!(
        !String::from_utf8_lossy(document).contains("<m:t>12</m:t>"),
        "docx: β₁² read as β12"
    );
}

/// "scaled by 1/√dₖ": the fraction is one formula, not "1" and "√dₖ".
#[test]
fn attention_scaling_factor_is_a_fraction() {
    let Some(Analysed { paper, analysis }) = analysed("1706.03762v7") else {
        return;
    };
    let md = paper.to_markdown(analysis, &MarkdownOptions::default()).markdown;
    assert!(md.contains("scaling factor of $1/\\sqrt{d_{k}}$."));
    assert!(md.contains("dot products by $1/\\sqrt{d_{k}}$."));
}

/// The APS radical font sets the radical sign on "p" and the pieces of its
/// bar on "ffi": the formula reads with a square root, not "pffi2ffiffi".
#[test]
fn ligo_radical_is_a_square_root() {
    let Some(Analysed { analysis, .. }) = analysed("1602.03837v1") else {
        return;
    };
    let text: String = analysis
        .doc
        .blocks
        .iter()
        .map(|block| plain(&block.spans) + "\n")
        .collect();
    assert!(!text.contains("pffi"), "radical bar read as text");
    assert!(text.contains("= √2"), "radical sign missing");
}

/// A rebuilt formula written in a compact TeX-like notation, to compare
/// structures in tests: `\frac{a}{b}`, `\sqrt{a}`, `x_{i}^{2}`,
/// `\sum\limits_{i=1}^{n}`, `\accent{^}{x}`, rows of a table separated by
/// `\\` and cells by `&`.
fn tex(node: &MathNode) -> String {
    let group = |node: &MathNode| format!("{{{}}}", tex(node));
    match node {
        MathNode::Ident { text, .. } | MathNode::Number(text) => text.clone(),
        MathNode::Operator { text, .. } => text.clone(),
        MathNode::Text(text) => format!("\\text{{{text}}}"),
        MathNode::Space(_) => " ".into(),
        MathNode::Row(nodes) => nodes.iter().map(tex).collect(),
        MathNode::Frac { num, den } => format!("\\frac{}{}", group(num), group(den)),
        MathNode::Sqrt(base) => format!("\\sqrt{}", group(base)),
        MathNode::Root { base, index } => format!("\\sqrt[{}]{}", tex(index), group(base)),
        MathNode::Scripts { base, sub, sup } => {
            let mut out = tex(base);
            if let Some(sub) = sub {
                out.push_str(&format!("_{}", group(sub)));
            }
            if let Some(sup) = sup {
                out.push_str(&format!("^{}", group(sup)));
            }
            out
        }
        MathNode::UnderOver {
            base,
            over: Some(over),
            accent: true,
            ..
        } => format!("\\accent{}{}", group(over), group(base)),
        MathNode::UnderOver { base, under, over, .. } => {
            let mut out = format!("{}\\limits", tex(base));
            if let Some(under) = under {
                out.push_str(&format!("_{}", group(under)));
            }
            if let Some(over) = over {
                out.push_str(&format!("^{}", group(over)));
            }
            out
        }
        MathNode::Table { rows, .. } => rows
            .iter()
            .map(|row| row.iter().map(tex).collect::<Vec<_>>().join("&"))
            .collect::<Vec<_>>()
            .join("\\\\"),
    }
}

/// The rebuilt formulas of a paper, in the notation of [`tex`] (`None` for
/// the formulas that stay pictures), or `None` when the paper is not
/// downloaded.
fn formulas(id: &str) -> Option<Vec<Option<String>>> {
    let Analysed { analysis, .. } = analysed(id)?;
    let all = analysis
        .doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Equation { .. }))
        .map(|block| block.math.as_ref().map(tex))
        .collect();
    Some(all)
}

/// Scaled dot-product attention (Eq. 1 of the Transformer paper) is a
/// fraction over a square root, and the two lines of multi-head attention
/// are aligned on their "=", the second starting with text.
#[test]
fn attention_formulas_are_rebuilt() {
    let Some(all) = formulas("1706.03762v7") else { return };
    assert_eq!(
        all[0].as_deref(),
        Some(r"Attention(Q,K,V)=softmax(\frac{QK^{T}}{\sqrt{d_{k}}})V")
    );
    assert_eq!(
        all[1].as_deref(),
        Some(
            r"MultiHead(Q,K,V)&=Concat(head_{1},…,head_{h})W^{O}\\\text{where} head_{i}&=Attention(QW_{i}^{Q},KW_{i}^{K},VW_{i}^{V})"
        )
    );
}

/// Adam's formulas: sums and `lim` with limits above and below, scripts
/// stacked on one base, tall parentheses whose scripts go to the whole
/// group, hats and nested radicals, lines aligned on their relation.
#[test]
fn adam_formulas_are_rebuilt() {
    let Some(all) = formulas("1412.6980v9") else { return };
    assert_eq!(
        all[0].as_deref(),
        Some(r"v_{t}=(1−β_{2})∑\limits_{i=1}^{t} β_{2}^{t−i}·g_{i}^{2}")
    );
    // Eqs. (8) to (10); the numbers of the last two are inside the display.
    assert_eq!(
        all[9].as_deref(),
        Some(concat!(
            r"u_{t}=lim\limits_{p→∞}(v_{t})^{1/p}&=lim\limits_{p→∞}((1−β_{2}^{p})∑\limits_{i=1}^{t} β_{2}^{p(t−i)}·|g_{i}|^{p})^{1/p}\\",
            r"&=lim\limits_{p→∞}(1−β_{2}^{p})^{1/p}(∑\limits_{i=1}^{t} β_{2}^{p(t−i)}·|g_{i}|^{p})^{1/p} (9)\\",
            r"&=lim\limits_{p→∞}(∑\limits_{i=1}^{t}(β_{2}^{(t−i)}·|g_{i}|)^{p})^{1/p} (10)"
        ))
    );
    assert_eq!(
        all[18].as_deref(),
        Some(
            r"∑\limits_{t=1}^{T}\frac{\accent{^}{m}_{t,i}^{2}}{\sqrt{t\accent{^}{v}_{t,i}}}≤\frac{2}{1−γ}\frac{1}{\sqrt{1−β_{2}}}‖g_{1:T,i}‖_{2}"
        )
    );
    // The picture of this formula also holds the upper limit of the sum
    // of the next one: it stays a picture.
    assert_eq!(all[1], None);
}

/// Bold, calligraphic and blackboard letters are written with the Unicode
/// mathematical alphabets.
#[test]
fn math_alphabets_are_unicode_letters() {
    if let Some(all) = formulas("2010.11929v2") {
        assert_eq!(all[0].as_deref(), Some(r"𝐄∈ℝ^{(P^{2}·C)×D},𝐄_{pos}∈ℝ^{(N+1)×D}"));
    }
    if let Some(all) = formulas("1512.03385v1") {
        assert_eq!(all[0].as_deref(), Some(r"𝐲=ℱ(𝐱,{W_{i}})+𝐱."));
    }
}

/// The LIGO formula has big brackets without a Unicode value: it stays a
/// picture, though formulas are written as MathML.
#[test]
fn ligo_formula_with_unmapped_brackets_stays_a_picture() {
    let Some(Analysed { paper, analysis }) = analysed("1602.03837v1") else {
        return;
    };
    let equations: Vec<_> = analysis
        .doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Equation { .. }))
        .collect();
    assert_eq!(equations.len(), 1);
    assert!(equations[0].math.is_none());
    let out = paper.to_html(analysis, &HtmlOptions::default());
    assert!(!out.html.contains("<math display=\"block\""));
    assert!(out.html.contains("<div class=\"eq\""));
}

/// Figure 2 of the Transformer paper (two diagrams, each with a title above
/// it): the picture takes in the whole title of the left diagram, "Scaled"
/// included, and the title is not left over as a paragraph after the caption.
#[test]
fn attention_figure_keeps_its_panel_titles() {
    let Some(Analysed { analysis, .. }) = analysed("1706.03762v7") else {
        return;
    };
    let blocks = &analysis.doc.blocks;
    let fig = blocks
        .iter()
        .find(|block| matches!(&block.kind, BlockKind::Figure { label: Some(l), .. } if l == "Figure 2"))
        .expect("Figure 2");
    let BlockKind::Figure { image: Some(image), .. } = &fig.kind else {
        panic!("Figure 2 has no picture");
    };
    let title = fig
        .inner_lines
        .iter()
        .find(|line| line.text() == "Scaled Dot-Product Attention")
        .expect("the title of the left diagram is inside the figure");
    assert!(
        image.rect.x0 <= title.bbox.x0 && image.rect.x1 >= title.bbox.x1,
        "the picture {:?} cuts the title {:?}",
        image.rect,
        title.bbox
    );
    assert!(
        !blocks
            .iter()
            .any(|block| block.page == fig.page && plain(&block.spans) == "Scaled Dot-Product Attention"),
        "the title is left over as a paragraph"
    );
}

/// Figure 4 of BERT is four panels with sub-captions "(a)" to "(d)" under a
/// single caption: one picture with all of them, not one per column.
#[test]
fn bert_four_panel_figure_is_one_picture() {
    let Some(Analysed { analysis, .. }) = analysed("1810.04805v2") else {
        return;
    };
    let figures: Vec<_> = analysis
        .doc
        .blocks
        .iter()
        .filter(|block| block.page == 14 && matches!(block.kind, BlockKind::Figure { .. }))
        .collect();
    assert_eq!(figures.len(), 1, "page 15 has one figure, found {}", figures.len());
    let BlockKind::Figure {
        label,
        image: Some(image),
        ..
    } = &figures[0].kind
    else {
        panic!("the figure has no picture");
    };
    assert_eq!(label.as_deref(), Some("Figure 4"));
    assert!(
        image.rect.width() > 350.0 && image.rect.height() > 350.0,
        "the picture is only a part of the figure: {:?}",
        image.rect
    );
    // The left panels start at x = 113 and the lowest sub-captions, "(c)" and
    // "(d)", end at y = 431 (they are drawn as outlines, not text).
    assert!(
        image.rect.x0 <= 113.0 && image.rect.x1 >= 483.0 && image.rect.y0 <= 66.0 && image.rect.y1 >= 431.0,
        "the picture does not hold all four panels: {:?}",
        image.rect
    );
}

/// The Go listing of the GPT-4 report (appendix F.4) is set in a
/// proportional font on fixed columns, its words spread over them. It is
/// read as one listing of Go, with whole words ("string", not
/// "s t r i n g"), and nothing else in the report is.
#[test]
fn gpt4_go_listing_is_code() {
    let Some(Analysed { paper, analysis }) = analysed("2303.08774v6") else {
        return;
    };
    let html = paper.to_html(analysis, &HtmlOptions::default()).html;
    assert_eq!(html.matches("<pre class=\"listing").count(), 1, "one listing");
    assert!(html.contains("data-lang=\"go\""), "recognised as Go");
    let text = html_text(&html);
    for line in [
        "type User struct {",
        "func main() {",
        "if err != nil {",
        "\"github.com/gorilla/mux\"",
    ] {
        assert!(text.contains(line), "{line:?} missing from the listing");
    }
    assert!(!text.contains("s t r i n g"), "words of the listing still spread");
}

/// The text of some HTML: tags removed, the usual entities decoded.
fn html_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    text.replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// The HTML written as the pictures are drawn (`Paper::write_html`) is the
/// same, byte for byte, as `Paper::to_html`, in every mode, with pictures
/// inlined or as files, and on one thread or several.
#[test]
fn written_html_matches_to_html() {
    let Some(Analysed { paper, analysis }) = analysed("1602.03837v1") else {
        return;
    };
    for mode in [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly] {
        for inline_images in [true, false] {
            for jobs in [1, 3] {
                let opts = HtmlOptions {
                    mode,
                    inline_images,
                    jobs,
                    ..Default::default()
                };
                let expected = paper.to_html(analysis, &opts);
                let mut html = Vec::new();
                let mut files: Vec<(String, Vec<u8>)> = Vec::new();
                paper
                    .write_html(analysis, &opts, &mut html, &mut |name, data| {
                        files.push((name.to_string(), data.to_vec()));
                        Ok(())
                    })
                    .unwrap();
                assert!(
                    html == expected.html.as_bytes(),
                    "{mode:?}, inline {inline_images}, -j {jobs}"
                );
                assert!(files == expected.files, "{mode:?}, inline {inline_images}, -j {jobs}");
                assert_eq!(files.is_empty(), inline_images);
            }
        }
    }
}

/// The PNG compression level changes the size of the pictures, never their
/// pixels, and the default level is `Balanced`.
#[test]
fn png_compression_keeps_the_pixels() {
    use base64::Engine;
    use kakemono::PngCompression;
    let Some(Analysed { paper, analysis }) = analysed("1602.03837v1") else {
        return;
    };
    let opts = HtmlOptions {
        mode: Mode::Friendly,
        ..Default::default()
    };
    let default = paper.to_html(analysis, &opts).html;
    let pictures = |compression: PngCompression| {
        let paper = load("1602.03837v1").unwrap().with_png_compression(compression);
        let html = paper.to_html(analysis, &opts).html;
        let uris: Vec<String> = Regex::new(r#"data:image/(png|jpeg);base64,([A-Za-z0-9+/=]+)"#)
            .unwrap()
            .captures_iter(&html)
            .map(|found| format!("{}:{}", &found[1], &found[2]))
            .collect();
        (html, uris)
    };
    let (balanced, balanced_uris) = pictures(PngCompression::Balanced);
    assert!(balanced == default, "Balanced is the default");
    let (_, fast_uris) = pictures(PngCompression::Fast);
    assert_eq!(fast_uris.len(), balanced_uris.len());
    assert!(fast_uris != balanced_uris, "Fast must give other files");
    let decode = |uri: &str| {
        let data = base64::engine::general_purpose::STANDARD
            .decode(uri.trim_start_matches("png:"))
            .unwrap();
        image::load_from_memory(&data).unwrap().to_rgba8()
    };
    let mut compared = 0;
    for (fast, balanced) in fast_uris.iter().zip(&balanced_uris) {
        if fast.starts_with("png:") && balanced.starts_with("png:") {
            assert!(decode(fast) == decode(balanced), "a picture changed");
            compared += 1;
        }
    }
    assert!(compared > 0);
}

/// The ids of all test papers (`tests/data/papers.txt`).
fn paper_ids() -> Vec<String> {
    std::fs::read_to_string(data_dir().join("papers.txt"))
        .expect("papers.txt")
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect()
}

/// A content JSON with each editable text (block text, author rows, table
/// cells) replaced by `edit(block id, text)`.
fn edit_content(json: &str, edit: impl Fn(u64, &str) -> String) -> String {
    let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
    for block in value["blocks"].as_array_mut().unwrap() {
        let id = block["id"].as_u64().unwrap();
        if matches!(block["type"].as_str(), Some("equation" | "code")) {
            continue;
        }
        if let Some(text) = block.get_mut("text")
            && let Some(old) = text.as_str()
        {
            *text = edit(id, old).into();
        }
        if let Some(rows) = block.get_mut("rows").and_then(|rows| rows.as_array_mut()) {
            for row in rows {
                *row = edit(id, row.as_str().unwrap()).into();
            }
        }
        if let Some(rows) = block.pointer_mut("/table/rows").and_then(|rows| rows.as_array_mut()) {
            for cell in rows.iter_mut().flat_map(|row| row.as_array_mut().unwrap()) {
                let old = cell["text"].as_str().unwrap().to_string();
                cell["text"] = edit(id, &old).into();
            }
        }
    }
    serde_json::to_string(&value).unwrap()
}

/// The content JSON of every paper, applied back unedited, changes no block;
/// edited, its text comes back exactly as written (styles, links and
/// formulas included), with no warning.
#[test]
fn content_json_round_trips() {
    for id in paper_ids() {
        let Some(Analysed { paper, analysis }) = analysed(&id) else {
            continue;
        };
        let mut analysis = copy(analysis);
        let json = paper.to_json(&analysis, &JsonOptions::default()).json;
        let report = paper.apply_content(&mut analysis, &json).unwrap();
        assert_eq!(report.edited_blocks, 0, "{id}: an unedited JSON changed blocks");
        assert!(report.warnings.is_empty(), "{id}: {:?}", report.warnings);

        let edited = edit_content(&json, |block, text| format!("{text} ÐÃSỬA{block}"));
        let report = paper.apply_content(&mut analysis, &edited).unwrap();
        assert!(report.warnings.is_empty(), "{id}: {:?}", report.warnings);
        let again = paper.to_json(&analysis, &JsonOptions::default()).json;
        let report = paper.apply_content(&mut analysis, &again).unwrap();
        assert_eq!(
            report.edited_blocks, 0,
            "{id}: the edited JSON did not come back as written"
        );
        let expected: serde_json::Value = serde_json::from_str(&edited).unwrap();
        let actual: serde_json::Value = serde_json::from_str(&again).unwrap();
        for (old, new) in expected["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .zip(actual["blocks"].as_array().unwrap())
        {
            assert_eq!(old["text"], new["text"], "{id}: block {}", old["id"]);
            assert_eq!(old["rows"], new["rows"], "{id}: block {}", old["id"]);
            assert_eq!(
                without_plain(&old["table"]),
                without_plain(&new["table"]),
                "{id}: block {}",
                old["id"]
            );
        }
    }
}

/// A table of the JSON output without the plain text of its cells, which
/// an edit of `text` leaves behind.
fn without_plain(table: &serde_json::Value) -> serde_json::Value {
    let mut table = table.clone();
    if let Some(rows) = table["rows"].as_array_mut() {
        for cell in rows.iter_mut().flat_map(|row| row.as_array_mut().unwrap()) {
            cell.as_object_mut().unwrap().remove("plain");
        }
    }
    table
}

/// Edited text reaches every output, formulas and links stay where they
/// were, and hard-95 flows every edited block in its box.
#[test]
fn edited_content_reaches_every_output() {
    let tag = Regex::new(r"<m\b[^>]*>.*?</m>|<[^>]*>|&[#\w]+;").unwrap();
    for id in ["1602.03837v1", "1706.03762v7", "1412.6980v9"] {
        let Some(Analysed { paper, analysis }) = analysed(id) else {
            continue;
        };
        let mut analysis = copy(analysis);
        let friendly = |analysis: &kakemono::Analysis, mode: Mode| {
            paper
                .to_html(
                    analysis,
                    &HtmlOptions {
                        mode,
                        ..Default::default()
                    },
                )
                .html
        };
        let before = friendly(&analysis, Mode::Friendly);
        let json = paper.to_json(&analysis, &JsonOptions::default()).json;
        // Every word in upper case, and a marker at the end of each text.
        let edited = edit_content(&json, |block, text| {
            let mut out = String::new();
            let mut last = 0;
            for found in tag.find_iter(text) {
                out.push_str(&text[last..found.start()].to_uppercase());
                out.push_str(found.as_str());
                last = found.end();
            }
            out.push_str(&text[last..].to_uppercase());
            format!("{out} ÐÃSỬA{block}")
        });
        let report = paper.apply_content(&mut analysis, &edited).unwrap();
        assert!(report.warnings.is_empty(), "{id}: {:?}", report.warnings);
        let paragraphs: Vec<usize> = analysis
            .doc
            .blocks
            .iter()
            .filter(|b| matches!(b.kind, BlockKind::Paragraph { .. }) && !b.spans.is_empty())
            .map(|b| b.id)
            .collect();
        let after = friendly(&analysis, Mode::Friendly);
        let count = |html: &str, pat: &str| html.matches(pat).count();
        assert_eq!(
            count(&before, "<math"),
            count(&after, "<math"),
            "{id}: formulas changed"
        );
        assert_eq!(
            count(&before, "href=\"#"),
            count(&after, "href=\"#"),
            "{id}: links changed"
        );
        let hard95 = friendly(&analysis, Mode::Hard95);
        let medium = friendly(&analysis, Mode::Medium);
        let markdown = paper.to_markdown(&analysis, &MarkdownOptions::default()).markdown;
        let latex = paper.to_latex(&analysis, &LatexOptions::default()).latex;
        let docx = zip_parts(&paper.to_docx(&analysis, &DocxOptions::default()));
        // Word cuts text into runs where formulas may start: its text
        // without the markup, a paragraph per line.
        let docx_xml = String::from_utf8_lossy(&docx.iter().find(|(n, _)| n == "word/document.xml").unwrap().1)
            .replace("</w:p>", "<\n")
            .split('<')
            .map(|part| part.split_once('>').map_or(part, |(_, text)| text))
            .collect::<Vec<_>>()
            .join("");
        let epub: String = zip_parts(&paper.to_epub(&analysis, &EpubOptions::default()))
            .iter()
            .filter(|(n, _)| n.ends_with(".xhtml"))
            .map(|(_, d)| String::from_utf8_lossy(d).into_owned())
            .collect();
        for block in &paragraphs {
            let marker = format!("ÐÃSỬA{block}<");
            let marker_docx = format!("ÐÃSỬA{block}\n");
            let marker_md = format!("ÐÃSỬA{block}\n");
            for (name, out) in [
                ("friendly", &after),
                ("hard-95", &hard95),
                ("medium", &medium),
                ("epub", &epub),
            ] {
                assert!(out.contains(&marker), "{id}: paragraph {block} not edited in {name}");
            }
            assert!(
                docx_xml.contains(&marker_docx),
                "{id}: paragraph {block} not edited in docx"
            );
            assert!(
                markdown.contains(&marker_md) || markdown.contains(&format!("ÐÃSỬA{block} ")),
                "{id}: paragraph {block} not edited in markdown"
            );
            assert!(
                latex.contains(&format!("ÐÃSỬA{block}")),
                "{id}: paragraph {block} not edited in latex"
            );
        }
        assert!(
            count(&hard95, " rf ed\"") >= paragraphs.len() / 2,
            "{id}: edited blocks do not flow in hard-95"
        );
    }
}

/// The JSON output of `id`, parsed.
fn content_json(id: &str) -> Option<serde_json::Value> {
    let Analysed { paper, analysis } = analysed(id)?;
    Some(serde_json::from_str(&paper.to_json(analysis, &JsonOptions::default()).json).unwrap())
}

/// The front matter gives the authors with their affiliations, addresses
/// and notes, the arXiv category and date, the DOI and the keywords.
#[test]
fn json_meta_reads_the_front_matter() {
    if let Some(json) = content_json("1706.03762v7") {
        let meta = &json["meta"];
        assert_eq!(meta["arxiv_category"], "cs.CL");
        assert_eq!(meta["date"], "2023-08-02");
        let authors = meta["authors"].as_array().unwrap();
        let names: Vec<&str> = authors.iter().map(|a| a["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "Ashish Vaswani",
                "Noam Shazeer",
                "Niki Parmar",
                "Jakob Uszkoreit",
                "Llion Jones",
                "Aidan N. Gomez",
                "Łukasz Kaiser",
                "Illia Polosukhin"
            ]
        );
        assert_eq!(authors[0]["emails"][0], "avaswani@google.com");
        let affiliation = authors[0]["affiliations"][0].as_u64().unwrap() as usize;
        assert_eq!(meta["affiliations"][affiliation], "Google Brain");
        // Every author has the "Equal contribution" note (∗).
        assert!(
            authors
                .iter()
                .all(|a| a["notes"].as_array().is_some_and(|n| !n.is_empty()))
        );
        assert!(
            meta["abstract"]
                .as_str()
                .unwrap()
                .starts_with("The dominant sequence transduction")
        );
    }
    if let Some(json) = content_json("1512.03385v1") {
        // Names side by side without commas, and addresses in braces.
        let authors = json["meta"]["authors"].as_array().unwrap();
        let emails: Vec<&str> = authors.iter().map(|a| a["emails"][0].as_str().unwrap()).collect();
        assert_eq!(
            emails,
            [
                "kahe@microsoft.com",
                "v-xiangz@microsoft.com",
                "v-shren@microsoft.com",
                "jiansun@microsoft.com"
            ]
        );
    }
    if let Some(json) = content_json("1602.03837v1") {
        assert_eq!(json["meta"]["doi"], "10.1103/PhysRevLett.116.061102");
        assert_eq!(json["meta"]["date"], "2016-02-11");
        assert_eq!(json["meta"]["authors"][0]["name"], "B. P. Abbott");
    }
    if let Some(json) = content_json("1506.01497v3") {
        assert_eq!(
            json["meta"]["keywords"],
            serde_json::json!(["Object Detection", "Region Proposal", "Convolutional Neural Network"])
        );
    }
}

/// Almost every reference of the papers with a usual reference list gives
/// its authors, title and year.
#[test]
fn json_references_have_citations() {
    for (id, min_ratio) in [
        ("1412.6980v9", 1.0),
        ("1506.01497v3", 1.0),
        ("1512.03385v1", 1.0),
        ("1706.03762v7", 1.0),
        ("2010.11929v2", 1.0),
        ("2303.08774v6", 0.9),
    ] {
        let Some(json) = content_json(id) else { continue };
        let refs: Vec<&serde_json::Value> = json["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "reference")
            .collect();
        let full = refs
            .iter()
            .filter(|b| {
                let c = &b["citation"];
                c["authors"].is_array() && c["title"].is_string() && c["year"].is_u64()
            })
            .count();
        let ratio = full as f64 / refs.len() as f64;
        assert!(
            ratio >= min_ratio,
            "{id}: {full} of {} references are whole",
            refs.len()
        );
    }
    // Physical Review style: no titles, but authors, venue, volume and year.
    if let Some(json) = content_json("1602.03837v1") {
        let refs: Vec<&serde_json::Value> = json["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["type"] == "reference")
            .collect();
        let dated = refs
            .iter()
            .filter(|b| b["citation"]["authors"].is_array() && b["citation"]["year"].is_u64())
            .count();
        assert!(
            dated >= 100,
            "LIGO: {dated} of {} references have authors and a year",
            refs.len()
        );
    }
    if let Some(json) = content_json("1706.03762v7") {
        let shazeer = json["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["type"] == "reference" && b["label"] == "32")
            .unwrap();
        assert_eq!(
            shazeer["citation"],
            serde_json::json!({
                "authors": ["Noam Shazeer", "Azalia Mirhoseini", "Krzysztof Maziarz", "Andy Davis", "Quoc Le",
                            "Geoffrey Hinton", "Jeff Dean"],
                "title": "Outrageously large neural networks: The sparsely-gated mixture-of-experts layer",
                "venue": "arXiv",
                "year": 2017,
                "arxiv": "1701.06538"
            })
        );
    }
}

/// Every block with text has it as plain text too, formulas as LaTeX; the
/// plain text is not read back, and editing it alone gives a warning.
#[test]
fn json_plain_text_is_read_only() {
    let Some(Analysed { paper, analysis }) = analysed("1412.6980v9") else {
        return;
    };
    let mut analysis = copy(analysis);
    let json = paper.to_json(&analysis, &JsonOptions::default()).json;
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let blocks = value["blocks"].as_array_mut().unwrap();
    assert!(
        blocks
            .iter()
            .filter(|b| b["text"].is_string())
            .all(|b| b["plain"].is_string())
    );
    let with_math = blocks
        .iter_mut()
        .find(|b| b["type"] == "paragraph" && b["text"].as_str().unwrap().contains("<m "))
        .unwrap();
    assert!(with_math["plain"].as_str().unwrap().contains('$'));
    assert!(!with_math["plain"].as_str().unwrap().contains('<'));
    with_math["plain"] = "Đã sửa".into();
    let report = paper.apply_content(&mut analysis, &value.to_string()).unwrap();
    assert_eq!(report.edited_blocks, 0);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("`plain`"));
}

/// The bibliography of a Physical Review Letter has no heading: its
/// entries, labelled [1] to [118], are references all the same.
#[test]
fn ligo_references_without_heading_are_references() {
    let Some(Analysed { analysis, .. }) = analysed("1602.03837v1") else {
        return;
    };
    let doc = &analysis.doc;
    let labels: Vec<String> = doc
        .blocks
        .iter()
        .filter_map(|block| match &block.kind {
            BlockKind::Reference { label } => label.clone(),
            _ => None,
        })
        .collect();
    assert_eq!(labels.len(), 118);
    assert_eq!(labels.first().map(String::as_str), Some("1"));
    assert_eq!(labels.last().map(String::as_str), Some("118"));
    // The last page's notes are still footnotes.
    let notes = doc
        .blocks
        .iter()
        .filter(|block| matches!(&block.kind, BlockKind::Footnote { marker } if marker == "a"))
        .count();
    assert_eq!(notes, 1);
}

/// An added note (`<note>` in the editable JSON) added to two
/// paragraphs and a table cell of the LIGO paper reaches every output that shows edited
/// text: a numbered call with the note after its block (HTML, Markdown), a
/// popup (hard-95), real footnotes (Word, LaTeX, EPUB); the JSON written
/// again gives the tag back, and the plain text and the chunks leave the
/// note out.
#[test]
fn ligo_added_notes_reach_every_output() {
    let Some(Analysed { paper, analysis }) = analysed("1602.03837v1") else {
        return;
    };
    let mut analysis = copy(analysis);
    let json = paper.to_json(&analysis, &JsonOptions::default()).json;
    let note = "<note>In English: <i>gravitational waves</i>.</note>";
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let mut noted = Vec::new();
    for block in value["blocks"].as_array_mut().unwrap() {
        if block["type"] == "paragraph" && noted.len() < 2 {
            let text = block["text"].as_str().unwrap().to_string();
            block["text"] = format!("{text}{note}").into();
            noted.push(text);
        }
        // And one in the first cell of the table.
        if let Some(cell) = block.pointer_mut("/table/rows/0/0/text") {
            *cell = format!("{}<note>Table cell</note>", cell.as_str().unwrap()).into();
        }
    }
    let report = paper
        .apply_content(&mut analysis, &serde_json::to_string(&value).unwrap())
        .unwrap();
    assert_eq!(report.edited_blocks, 3);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    // The JSON written again holds the tag; its `plain` text does not.
    let again = paper.to_json(&analysis, &JsonOptions::default()).json;
    let again_value: serde_json::Value = serde_json::from_str(&again).unwrap();
    let block = again_value["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|block| block["text"].as_str().is_some_and(|text| text.ends_with(note)))
        .expect("the note comes back in the JSON");
    assert!(!block["plain"].as_str().unwrap().contains("In English"));
    assert_eq!(paper.apply_content(&mut analysis, &again).unwrap().edited_blocks, 0);

    let html = |mode| {
        paper
            .to_html(
                &analysis,
                &HtmlOptions {
                    mode,
                    inline_images: false,
                    ..Default::default()
                },
            )
            .html
    };
    for mode in [Mode::Friendly, Mode::Medium] {
        let page = html(mode);
        assert!(
            page.contains("<a class=\"an-call\" id=\"anref-2\" href=\"#an-2\""),
            "{mode:?}"
        );
        assert_eq!(
            page.matches("<aside class=\"an\" role=\"note\">").count(),
            3,
            "{mode:?}"
        );
        assert!(
            page.contains("<p id=\"an-1\"><a class=\"an-back\" href=\"#anref-1\""),
            "{mode:?}"
        );
        assert!(page.contains("In English: <em>gravitational waves</em>."), "{mode:?}");
        assert!(page.contains("aside.an{"), "{mode:?}: the style of the notes");
    }
    let hard95 = html(Mode::Hard95);
    assert!(hard95.contains("<sup class=\"an\" tabindex=\"0\""));
    assert!(hard95.contains("title=\"In English: gravitational waves.\""));
    assert!(hard95.contains("class=\"an-pop\"") || hard95.contains("'an-pop'"));

    let docx = zip_parts(&paper.to_docx(&analysis, &DocxOptions::default()));
    let part = |name: &str| {
        docx.iter()
            .find(|(part, _)| part == name)
            .map(|(_, data)| String::from_utf8(data.clone()).unwrap())
            .unwrap_or_else(|| panic!("{name} is missing"))
    };
    let footnotes = part("word/footnotes.xml");
    assert!(footnotes.contains("w:type=\"separator\" w:id=\"-1\""));
    assert!(footnotes.contains("<w:footnote w:id=\"3\">"));
    assert!(footnotes.contains("In English: "));
    assert!(part("word/document.xml").contains("<w:footnoteReference w:id=\"1\"/>"));
    assert!(part("[Content_Types].xml").contains("/word/footnotes.xml"));
    assert!(part("word/_rels/document.xml.rels").contains("Target=\"footnotes.xml\""));

    let md = paper.to_markdown(&analysis, &MarkdownOptions::default()).markdown;
    assert!(md.contains("[^n1]"));
    assert!(md.contains("\n[^n2]: In English: *gravitational waves*.\n"), "markdown");

    let tex = paper.to_latex(&analysis, &LatexOptions::default()).latex;
    assert_eq!(
        tex.matches("\\footnote{In English: \\textit{gravitational waves}.}")
            .count(),
        2
    );
    // In the table, the cell holds the mark and the note follows the table.
    assert!(tex.contains("\\footnotemark{}"));
    assert!(tex.contains("\\addtocounter{footnote}{-1}\\stepcounter{footnote}\\footnotetext{Table cell}"));

    let epub = zip_parts(&paper.to_epub(&analysis, &EpubOptions::default()));
    let book: String = epub
        .iter()
        .filter(|(name, _)| name.ends_with(".xhtml"))
        .map(|(_, data)| String::from_utf8_lossy(data).into_owned())
        .collect();
    assert_eq!(book.matches("epub:type=\"footnote\"").count(), 3);
    assert_eq!(book.matches("epub:type=\"noteref\"").count(), 3);

    let chunks = kakemono::chunks_jsonl(&paper.to_chunks(&analysis, &ChunkOptions::default()));
    assert!(!chunks.contains("In English"));
    assert!(chunks.contains(noted[0].split('<').next().unwrap().trim()));
}

/// The text inside a figure is edited in place: Adam's chart titles, given
/// in Vietnamese in `figure_lines`, make the figure an SVG that draws the
/// new titles and no longer the glyphs of the old ones; the other runs are
/// left as they are.
#[test]
fn adam_chart_titles_are_translated_in_place() {
    let Some(Analysed { paper, analysis }) = analysed("1412.6980v9") else {
        return;
    };
    let mut analysis = copy(analysis);
    let json = paper.to_json(&analysis, &JsonOptions::default()).json;
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    let figure = value["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| {
            block["figure_lines"].as_array().is_some_and(|runs| {
                runs.iter().any(|run| {
                    run["text"]
                        .as_str()
                        .is_some_and(|text| text.contains("MNIST Logistic Regression"))
                })
            })
        })
        .expect("Adam's Figure 1 lists its titles");
    let runs = figure["figure_lines"].as_array_mut().unwrap();
    assert!(
        runs.len() > 10,
        "the ticks, legends and titles are runs: {}",
        runs.len()
    );
    let title = runs
        .iter_mut()
        .find(|run| run["text"].as_str().unwrap().contains("MNIST Logistic Regression"))
        .unwrap();
    title["text"] = "Hồi quy logistic trên MNIST".into();
    // The axis titles written upwards are runs too, after the others.
    let axis = runs
        .iter_mut()
        .find(|run| run["rotation"].as_f64() == Some(-90.0))
        .expect("the rotated axis title is a run");
    assert!(axis["text"].as_str().unwrap().contains("training cost"));
    axis["text"] = "chi phí huấn luyện".into();
    let report = paper.apply_content(&mut analysis, &value.to_string()).unwrap();
    assert_eq!(report.edited_blocks, 1, "{:?}", report.warnings);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let svg = String::from_utf8(analysis.doc.pictures.last().unwrap().data.clone()).unwrap();
    assert!(svg.contains("Hồi quy logistic trên MNIST"));
    assert!(svg.contains("rotate(-90)") && svg.contains("chi phí huấn luyện"));
    // The JSON written afterwards still lists the text of the figure.
    let again = paper.to_json(&analysis, &JsonOptions::default()).json;
    assert!(again.contains("figure_lines"));
}
