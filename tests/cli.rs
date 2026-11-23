//! Runs the `kakemono` executable itself: the version, conversions and the
//! `formula` subcommand.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The LIGO paper, the one test paper committed to the repository.
const LIGO: &str = "tests/data/1602.03837v1.pdf";

/// Runs `kakemono` with `args`.
fn kakemono(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kakemono"))
        .args(args)
        .output()
        .expect("the kakemono executable runs")
}

/// A fresh, empty directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Writes a blank white picture, which holds no formula.
fn blank_picture(path: &Path) {
    image::RgbImage::from_pixel(40, 20, image::Rgb([255, 255, 255]))
        .save(path)
        .unwrap();
}

#[test]
fn version_names_kakemono() {
    let out = kakemono(&["--version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("kakemono "));
}

#[test]
fn convert_writes_html_signed_by_kakemono() {
    let dir = scratch("cli-convert");
    let html = dir.join("ligo.friendly.html");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-m",
        "friendly",
        "--fonts",
        "system",
        "-j",
        "1",
        "-o",
        html.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&html).unwrap();
    assert!(text.contains(r#"<meta name="generator" content="kakemono (friendly)">"#));
    assert!(text.contains("Converted from PDF by kakemono · friendly mode<"));
}

#[test]
fn convert_writes_a_word_document_when_asked() {
    let dir = scratch("cli-docx");
    // `-o x.docx` is enough to choose the format.
    let docx = dir.join("ligo.docx");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", docx.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let data = std::fs::read(&docx).unwrap();
    assert!(data.starts_with(b"PK\x03\x04"), "a ZIP archive");
    assert!(String::from_utf8_lossy(&data).contains("word/document.xml"));

    // `--format docx` with a directory gives `<stem>.docx`.
    let out = kakemono(&["convert", LIGO, "-j", "1", "-f", "docx", "-o", dir.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.join("1602.03837v1.docx").is_file());

    let all = kakemono(&["convert", LIGO, "--all", "-f", "docx", "-o", dir.to_str().unwrap()]);
    assert!(!all.status.success());
    assert!(String::from_utf8_lossy(&all.stderr).contains("--all"));
}

#[test]
fn convert_writes_an_epub_book() {
    let dir = scratch("cli-epub");
    // `-o x.epub` is enough to choose the format.
    let epub = dir.join("ligo.epub");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", epub.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let data = std::fs::read(&epub).unwrap();
    // The `mimetype` entry comes first, stored, as the format requires.
    assert!(data.starts_with(b"PK\x03\x04"), "a ZIP archive");
    assert_eq!(&data[30..38], b"mimetype");
    assert_eq!(&data[38..58], b"application/epub+zip");
    let text = String::from_utf8_lossy(&data);
    assert!(text.contains("EPUB/package.opf") && text.contains("EPUB/nav.xhtml"));

    // `--format epub` with a directory gives `<stem>.epub`.
    let out = kakemono(&["convert", LIGO, "-j", "1", "-f", "epub", "-o", dir.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.join("1602.03837v1.epub").is_file());

    // The book holds its pictures.
    let files = kakemono(&["convert", LIGO, "--images", "files", "-o", epub.to_str().unwrap()]);
    assert!(!files.status.success());
    assert!(String::from_utf8_lossy(&files.stderr).contains("--images files"));
}

#[test]
fn convert_writes_markdown_with_an_image_folder() {
    let dir = scratch("cli-markdown");
    // `-o x.md` is enough to choose the format; pictures go to a folder.
    let md = dir.join("ligo.md");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--image-dir",
        "images",
        "-o",
        md.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.starts_with("Selected for a Viewpoint"), "{}", &text[..200]);
    assert!(text.contains("\n# Observation of Gravitational Waves from a Binary Black Hole Merger\n"));
    assert!(text.contains("](images/001-fig.png)"));
    assert!(dir.join("images/001-fig.png").is_file());

    // `--images inline` embeds them instead.
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "-f",
        "md",
        "--images",
        "inline",
        "-o",
        dir.join("inline").to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(dir.join("inline/1602.03837v1.md")).unwrap();
    assert!(text.contains("](data:image/png;base64,"));
    assert!(!dir.join("inline/1602.03837v1_assets").exists());

    let outside = kakemono(&["convert", LIGO, "--image-dir", "../x", "-o", md.to_str().unwrap()]);
    assert!(!outside.status.success());
}

#[test]
fn markdown_mode_chooses_how_tables_are_written() {
    let dir = scratch("cli-markdown-mode");
    let md = dir.join("ligo.md");
    let convert = |mode: &str| {
        let out = kakemono(&["convert", LIGO, "-j", "1", "-m", mode, "-o", md.to_str().unwrap()]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        std::fs::read_to_string(&md).unwrap()
    };
    // Table I of the LIGO paper has no header row: medium keeps it as an
    // HTML table, friendly adapts it to a pipe table with an empty header.
    let medium = convert("medium");
    assert!(medium.contains("<tr><td>Primary black hole mass</td>"));
    let friendly = convert("friendly");
    assert!(!friendly.contains("<table>"));
    assert!(friendly.contains("|  |  |\n| --- | --- |\n| Primary black hole mass | "));
    // Markdown has no pages to keep.
    let hard = kakemono(&["convert", LIGO, "-m", "hard-95", "-o", md.to_str().unwrap()]);
    assert!(!hard.status.success());
    assert!(String::from_utf8_lossy(&hard.stderr).contains("friendly or medium"));
}

#[test]
fn convert_writes_latex_that_compiles() {
    let dir = scratch("cli-latex");
    // `-o x.tex` is enough to choose the format; pictures go to a folder.
    let tex = dir.join("ligo.tex");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", tex.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&tex).unwrap();
    assert!(text.contains("\\documentclass[11pt]{article}"));
    assert!(
        text.contains("{\\LARGE\\bfseries Observation of Gravitational Waves from a Binary Black Hole Merger\\par}")
    );
    assert!(text.contains("{ligo_assets/001-fig.png}"));
    assert!(dir.join("ligo_assets/001-fig.png").is_file());
    assert!(text.ends_with("\\end{document}\n"));

    // LaTeX cannot embed pictures.
    let inline = kakemono(&[
        "convert",
        LIGO,
        "-f",
        "tex",
        "--images",
        "inline",
        "-o",
        tex.to_str().unwrap(),
    ]);
    assert!(!inline.status.success());
    assert!(String::from_utf8_lossy(&inline.stderr).contains("--images inline"));

    // Compiled when pdfLaTeX is installed (it is not needed to build).
    let compiled = Command::new("pdflatex")
        .args(["-interaction=nonstopmode", "-halt-on-error", "ligo.tex"])
        .current_dir(&dir)
        .output();
    match compiled {
        Ok(out) => assert!(
            out.status.success() && dir.join("ligo.pdf").is_file(),
            "pdflatex failed:\n{}",
            String::from_utf8_lossy(&out.stdout)
        ),
        Err(err) => eprintln!("pdflatex not run ({err}): the LaTeX output was not compiled"),
    }
}

/// Runs `typst compile` on `file` in `dir`, when Typst is installed: the
/// output is a PDF. Returns whether it ran.
fn compile_typst(dir: &Path, file: &str) -> bool {
    match Command::new("typst").args(["compile", file]).current_dir(dir).output() {
        Ok(out) => {
            assert!(
                out.status.success(),
                "typst failed:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            true
        }
        Err(err) => {
            eprintln!("typst not run ({err}): the Typst output was not compiled");
            false
        }
    }
}

#[test]
fn convert_writes_typst_that_compiles() {
    let dir = scratch("cli-typst");
    // `-o x.typ` is enough to choose the format; pictures go to a folder.
    let typ = dir.join("ligo.typ");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", typ.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&typ).unwrap();
    assert!(text.contains("#let doc-layout = \"article\""));
    assert!(text.contains("#kakemono-title[Observation of Gravitational Waves from a Binary Black Hole Merger]"));
    assert!(text.contains("\"ligo_assets/001-fig.png\""));
    assert!(dir.join("ligo_assets/001-fig.png").is_file());
    if compile_typst(&dir, "ligo.typ") {
        assert!(dir.join("ligo.pdf").is_file(), "typst wrote no PDF");
    }

    // The layout can be chosen, for Typst only; Typst cannot embed pictures.
    let book = dir.join("book.typ");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--layout",
        "book",
        "-o",
        book.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        std::fs::read_to_string(&book)
            .unwrap()
            .contains("#let doc-layout = \"book\"")
    );
    if compile_typst(&dir, "book.typ") {
        assert!(dir.join("book.pdf").is_file(), "typst wrote no PDF");
    }
    let md = dir.join("ligo.md");
    let wrong = kakemono(&["convert", LIGO, "--layout", "book", "-o", md.to_str().unwrap()]);
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("--layout"));
    let inline = kakemono(&[
        "convert",
        LIGO,
        "-f",
        "typst",
        "--images",
        "inline",
        "-o",
        typ.to_str().unwrap(),
    ]);
    assert!(!inline.status.success());
    assert!(String::from_utf8_lossy(&inline.stderr).contains("--images inline"));
}

#[test]
fn word_document_can_also_write_its_pictures() {
    let dir = scratch("cli-docx-images");
    let docx = dir.join("ligo.docx");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--images",
        "files",
        "-o",
        docx.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pictures: Vec<_> = std::fs::read_dir(dir.join("ligo_assets")).unwrap().collect();
    assert_eq!(pictures.len(), 5, "4 figures and 1 formula");
    // The pictures stay in the document too.
    assert!(String::from_utf8_lossy(&std::fs::read(&docx).unwrap()).contains("word/media/image5.png"));
}

#[test]
fn formula_reports_a_picture_it_cannot_read() {
    let dir = scratch("cli-formula");
    let (one, two) = (dir.join("one.png"), dir.join("two.png"));
    blank_picture(&one);
    blank_picture(&two);
    let (one, two) = (one.to_str().unwrap(), two.to_str().unwrap());

    let single = kakemono(&["formula", one]);
    assert!(!single.status.success());
    assert!(String::from_utf8_lossy(&single.stderr).contains("cannot read the formula"));

    let several = kakemono(&["formula", one, two]);
    assert!(several.status.success());
    assert_eq!(String::from_utf8_lossy(&several.stdout).lines().count(), 2);

    let scores = kakemono(&["formula", "--scores", one]);
    assert!(scores.status.success());
    assert!(String::from_utf8_lossy(&scores.stdout).contains("NaN"));
}

/// Runs a TeX engine on `file` in `dir`, when it is installed: the output is
/// a PDF. Returns whether the engine ran.
fn compile_tex(engine: &str, dir: &Path, file: &str) -> bool {
    match Command::new(engine)
        .args(["-interaction=nonstopmode", "-halt-on-error", file])
        .current_dir(dir)
        .output()
    {
        Ok(out) => {
            assert!(
                out.status.success(),
                "{engine} failed:\n{}",
                String::from_utf8_lossy(&out.stdout)
            );
            true
        }
        Err(err) => {
            eprintln!("{engine} not run ({err}): the LaTeX output was not compiled");
            false
        }
    }
}

/// The editable JSON is written with `-o x.json`, applied back with
/// `--content`; edited Vietnamese text (letters that pdfLaTeX's T1 fonts
/// lack) next to Polish (letters that T5 lacks) compiles with pdfLaTeX and
/// XeLaTeX.
#[test]
fn edited_json_gives_latex_that_compiles_in_vietnamese() {
    let dir = scratch("cli-json-vi");
    let json = dir.join("ligo.json");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", json.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.join("ligo_assets/001-fig.png").is_file());
    let mut content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    for block in content["blocks"].as_array_mut().unwrap() {
        match block["type"].as_str() {
            Some("title") => block["text"] = "Quan sát sóng hấp dẫn từ sự sáp nhập của một hệ hai lỗ đen".into(),
            Some("heading") => block["text"] = "Giới thiệu — Łódź, Straße, œuvre, ặ ữ ỹ Ư".into(),
            _ => {}
        }
    }
    let edited = dir.join("ligo.vi.json");
    std::fs::write(&edited, serde_json::to_string(&content).unwrap()).unwrap();

    let tex = dir.join("vi.tex");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--content",
        edited.to_str().unwrap(),
        "-o",
        tex.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&tex).unwrap();
    assert!(text.contains("Quan sát sóng hấp dẫn"));
    assert!(text.contains("\\usepackage[T5,T1]{fontenc}"));
    assert!(text.contains("\\DeclareUnicodeCharacter{1EB7}{\\kakemonovn{\\d{\\abreve}}}"));
    // Letters that T5 has are no longer replaced by a question mark.
    assert!(!text.contains("\\DeclareUnicodeCharacter{1EB7}{\\kakemonomissing}"));
    for engine in ["pdflatex", "xelatex"] {
        let _ = std::fs::remove_file(dir.join("vi.pdf"));
        if compile_tex(engine, &dir, "vi.tex") {
            assert!(dir.join("vi.pdf").is_file(), "{engine} wrote no PDF");
        }
    }

    // In Typst, every paragraph translated, with a translator's note, which
    // is a footnote; the language is Vietnamese.
    let mut noted = content.clone();
    for block in noted["blocks"].as_array_mut().unwrap() {
        if block["type"] == "paragraph" {
            block["text"] =
                "Sóng hấp dẫn<note>Gravitational waves: <i>sóng hấp dẫn</i>.</note> được dự đoán từ năm 1916.".into();
        }
    }
    std::fs::write(&edited, serde_json::to_string(&noted).unwrap()).unwrap();
    let typ = dir.join("vi.typ");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--content",
        edited.to_str().unwrap(),
        "-o",
        typ.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = std::fs::read_to_string(&typ).unwrap();
    assert!(text.contains("#let lang = \"vi\""));
    assert!(text.contains("Sóng hấp dẫn#footnote[Gravitational waves: #emph[sóng hấp dẫn];.] được dự đoán"));
    if compile_typst(&dir, "vi.typ") {
        assert!(dir.join("vi.pdf").is_file(), "typst wrote no PDF");
    }

    // A JSON file of another PDF, or the --json document, is refused.
    let mut other = content.clone();
    other["source"]["sha256"] = "0".repeat(64).into();
    std::fs::write(&edited, serde_json::to_string(&other).unwrap()).unwrap();
    let out = kakemono(&[
        "convert",
        LIGO,
        "--content",
        edited.to_str().unwrap(),
        "-o",
        tex.to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("another file"));
    let hard = kakemono(&["convert", LIGO, "-m", "hard", "--content", json.to_str().unwrap()]);
    assert!(!hard.status.success());
    assert!(String::from_utf8_lossy(&hard.stderr).contains("hard mode"));
}

#[test]
fn convert_writes_chunks_as_json_lines() {
    let dir = scratch("cli-chunks");
    // `-o x.jsonl` is enough to choose the format.
    let file = dir.join("ligo.jsonl");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--chunk-tokens",
        "128",
        "-o",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let chunks: Vec<serde_json::Value> = std::fs::read_to_string(&file)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(chunks.len() > 10);
    assert!(chunks.iter().all(|chunk| chunk["tokens"].as_u64().unwrap() <= 128));
    assert!(
        chunks
            .iter()
            .any(|chunk| chunk["text"].as_str().unwrap().starts_with("I. INTRODUCTION\n\n"))
    );

    // `--format chunks` with a directory gives `<stem>.chunks.jsonl`.
    let out = kakemono(&["convert", LIGO, "-j", "1", "-f", "chunks", "-o", dir.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.join("1602.03837v1.chunks.jsonl").is_file());

    // A Word document gives chunks too.
    let word = dir.join("sample.jsonl");
    let out = kakemono(&["convert", WORD_SAMPLE, "-o", word.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(
        std::fs::read_to_string(&word)
            .unwrap()
            .contains("\"text\":\"Abstract\\n\\n")
    );

    // A tokenizer counts the tokens of chunks only.
    let html = kakemono(&[
        "convert",
        LIGO,
        "--tokenizer",
        "o200k_base",
        "-o",
        dir.join("x.html").to_str().unwrap(),
    ]);
    assert!(!html.status.success());
    assert!(String::from_utf8_lossy(&html.stderr).contains("--tokenizer"));
}

#[cfg(feature = "openai-tokenizers")]
#[test]
fn chunks_can_be_sized_by_an_openai_tokenizer() {
    let dir = scratch("cli-chunks-openai");
    let file = dir.join("ligo.jsonl");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-j",
        "1",
        "--tokenizer",
        "cl100k_base",
        "-o",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("tokens,") && !stderr.contains("estimated"), "{stderr}");
}

/// The sample Word document of `tests/data/word`.
const WORD_SAMPLE: &str = "tests/data/word/sample.docx";

#[test]
fn word_document_converts_to_friendly_html() {
    let dir = scratch("cli-word");
    let html = dir.join("sample.friendly.html");
    let out = kakemono(&[
        "convert",
        WORD_SAMPLE,
        "--fonts",
        "system",
        "-o",
        html.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("read docx document"));
    let text = std::fs::read_to_string(&html).unwrap();
    assert!(text.contains("Converted from Word by kakemono · friendly mode"));
}

#[test]
fn word_document_has_every_mode() {
    let dir = scratch("cli-word-modes");
    let out = kakemono(&[
        "convert",
        WORD_SAMPLE,
        "--all",
        "--fonts",
        "system",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut written: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    written.sort();
    assert_eq!(
        written,
        [
            "sample.friendly.html",
            "sample.hard-95.html",
            "sample.hard.html",
            "sample.medium.html"
        ]
    );
    // hard and hard-95 show the pages Word would print; medium shows Word's
    // formatting on one continuous sheet.
    let hard95 = std::fs::read_to_string(dir.join("sample.hard-95.html")).unwrap();
    assert!(hard95.contains("pages") || hard95.contains("page"));
    let medium = std::fs::read_to_string(dir.join("sample.medium.html")).unwrap();
    assert!(medium.contains("class=\"sheet\""), "medium is one sheet");
    assert!(medium.contains("font-family:"), "medium keeps Word's fonts");
}

/// Runs `kakemono` with `args` from the folder `dir`.
fn kakemono_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kakemono"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the kakemono executable runs")
}

/// The files of a ZIP package (a `.docx`): (name, contents).
fn zip_parts(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let le16 = |at: usize| usize::from(u16::from_le_bytes([zip[at], zip[at + 1]]));
    let le32 = |at: usize| u32::from_le_bytes(zip[at..at + 4].try_into().unwrap()) as usize;
    let mut parts = Vec::new();
    let mut at = 0;
    while zip[at..].starts_with(b"PK\x03\x04") {
        let (method, size, name_len, extra_len) = (le16(at + 8), le32(at + 18), le16(at + 26), le16(at + 28));
        let name = String::from_utf8(zip[at + 30..at + 30 + name_len].to_vec()).unwrap();
        let start = at + 30 + name_len + extra_len;
        let raw = &zip[start..start + size];
        let data = if method == 8 {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(raw).read_to_end(&mut out).unwrap();
            out
        } else {
            raw.to_vec()
        };
        parts.push((name, data));
        at = start + size;
    }
    parts
}

/// The colour of the pixel in the middle of a picture file.
fn middle_pixel(data: &[u8]) -> Option<[u8; 3]> {
    let picture = image::load_from_memory(data).ok()?.to_rgb8();
    Some(picture.get_pixel(picture.width() / 2, picture.height() / 2).0)
}

/// A 10 by 10 PNG.
fn square_png() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(10, 10, image::Rgb([0, 128, 255]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
    out.into_inner()
}

/// The LIGO paper as an editable JSON in `dir`, with replacements: an SVG
/// of the size of the first figure's picture, a missing file for the
/// second, a square PNG for the wide third one, and a PNG on an equation.
/// Returns the edited JSON's path and the SVG.
fn ligo_with_replacements(dir: &Path) -> (PathBuf, String) {
    let json = dir.join("ligo.json");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", json.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    let blocks = content["blocks"].as_array().unwrap();
    let index_of = |kind: &str, nth: usize| {
        blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| block["type"] == kind)
            .nth(nth)
            .unwrap()
            .0
    };
    let (first, second, third, equation) = (
        index_of("figure", 0),
        index_of("figure", 1),
        index_of("figure", 2),
        index_of("equation", 0),
    );
    let picture = dir.join(blocks[first]["image"].as_str().unwrap());
    let (width, height) = image::image_dimensions(picture).unwrap();
    let svg = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><rect width="{width}" height="{height}" fill="#eef"/><text x="10" y="40" font-size="30">Hình mới</text></svg>"##
    );
    std::fs::create_dir_all(dir.join("vi")).unwrap();
    std::fs::write(dir.join("vi/fig1.svg"), &svg).unwrap();
    std::fs::write(dir.join("vi/square.png"), square_png()).unwrap();
    content["blocks"][first]["replace_image"] = "vi/fig1.svg".into();
    content["blocks"][second]["replace_image"] = "vi/missing.png".into();
    content["blocks"][third]["replace_image"] = "vi/square.png".into();
    content["blocks"][equation]["replace_image"] = "vi/square.png".into();
    let edited = dir.join("ligo.vi.json");
    std::fs::write(&edited, serde_json::to_string(&content).unwrap()).unwrap();
    (edited, svg)
}

/// A figure's `replace_image`, a path relative to the JSON's folder,
/// replaces its picture in Markdown, Word and friendly HTML; a missing
/// file, a picture of another shape and a block that is not a figure are
/// warnings.
#[test]
fn replace_image_swaps_a_figure_in_every_format() {
    let dir = scratch("cli-replace");
    let (edited, _) = ligo_with_replacements(&dir);
    let edited = edited.to_str().unwrap();
    let elsewhere = std::env::temp_dir();

    let md = dir.join("out/x.md");
    let out = kakemono_in(
        &elsewhere,
        &["convert", ligo_abs(), "--content", edited, "-o", md.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("missing.png"), "{stderr}");
    assert!(stderr.contains("aspect"), "{stderr}");
    assert!(stderr.contains("not a figure"), "{stderr}");
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(text.contains(".svg)"), "the replaced figure is an SVG file: {text}");

    let docx = dir.join("out/x.docx");
    let out = kakemono_in(
        &elsewhere,
        &["convert", ligo_abs(), "--content", edited, "-o", docx.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // The SVG, drawn as a PNG: its light blue fill is in the middle.
    let parts = zip_parts(&std::fs::read(&docx).unwrap());
    assert!(
        parts
            .iter()
            .any(|(name, data)| name.starts_with("word/media/") && middle_pixel(data) == Some([238, 238, 255]))
    );

    let html = dir.join("out/x.friendly.html");
    let out = kakemono_in(
        &elsewhere,
        &[
            "convert",
            ligo_abs(),
            "-m",
            "friendly",
            "--content",
            edited,
            "-o",
            html.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read_to_string(&html).unwrap().contains("data:image/svg+xml"));
}

/// The LIGO paper's absolute path, for commands run from another folder.
fn ligo_abs() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/1602.03837v1.pdf")
}

/// In hard-95, a replaced figure is drawn over its box, and the old text of
/// the figure (its transparent copy) is gone.
#[test]
fn hard95_draws_a_replaced_figure_over_its_box_without_its_old_text() {
    let dir = scratch("cli-replace-hard95");
    let (edited, _) = ligo_with_replacements(&dir);
    let content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&edited).unwrap()).unwrap();
    let id_of = |file: &str| {
        content["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|block| block["type"] == "figure" && block["replace_image"] == file)
            .unwrap()["id"]
            .as_u64()
            .unwrap()
    };
    // The first figure has no text of its own; the third (the square PNG) has.
    let (replaced, with_text) = (id_of("vi/fig1.svg"), id_of("vi/square.png"));
    let html = dir.join("x.hard-95.html");
    let out = kakemono(&[
        "convert",
        LIGO,
        "-m",
        "hard-95",
        "--content",
        edited.to_str().unwrap(),
        "-o",
        html.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let html = std::fs::read_to_string(&html).unwrap();
    assert!(html.contains(&format!(r#"data-kind="figure-picture" data-block="{replaced}""#)));
    assert!(html.contains(&format!(r#"data-kind="figure-picture" data-block="{with_text}""#)));
    assert!(!html.contains(&format!(r#"data-kind="figure-text" data-block="{with_text}""#)));
    assert!(html.contains("data:image/svg+xml"));
}

/// `render-svg` draws an SVG as a PNG of its size times `--scale`, with a
/// picture it names by a path relative to the SVG; a file that is not an
/// SVG is an error that says so.
#[test]
fn render_svg_writes_a_png_of_the_scaled_size() {
    let dir = scratch("cli-render-svg");
    std::fs::write(dir.join("square.png"), square_png()).unwrap();
    std::fs::write(
        dir.join("a.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="40" height="20"><image href="square.png" width="20" height="20"/></svg>"#,
    )
    .unwrap();
    let png = dir.join("a.png");
    let out = kakemono_in(
        &std::env::temp_dir(),
        &[
            "render-svg",
            dir.join("a.svg").to_str().unwrap(),
            "-o",
            png.to_str().unwrap(),
            "--scale",
            "2",
        ],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(image::image_dimensions(&png).unwrap(), (80, 40));
    // The picture it names is drawn: the left half is not white.
    let pixel = image::open(&png).unwrap().to_rgb8().get_pixel(10, 20).0;
    assert_eq!(pixel, [0, 128, 255]);

    std::fs::write(dir.join("bad.svg"), "not svg").unwrap();
    let out = kakemono(&[
        "render-svg",
        dir.join("bad.svg").to_str().unwrap(),
        "-o",
        dir.join("b.png").to_str().unwrap(),
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read"));
}

/// The LIGO paper as an editable JSON in `dir`, with the first figure's
/// `replace_image` set to `value`; returns the edited JSON's path and the
/// figure's picture size (pixels of kakemono's own picture of it).
fn ligo_with_one_replacement(dir: &Path, value: &str) -> (PathBuf, (u32, u32)) {
    let json = dir.join("ligo.json");
    let out = kakemono(&["convert", LIGO, "-j", "1", "-o", json.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    let figure = content["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["type"] == "figure")
        .unwrap();
    let size = image::image_dimensions(dir.join(figure["image"].as_str().unwrap())).unwrap();
    figure["replace_image"] = value.into();
    let edited = dir.join("ligo.vi.json");
    std::fs::write(&edited, serde_json::to_string(&content).unwrap()).unwrap();
    (edited, size)
}

/// An SVG too large to draw is refused with a warning, and the Word
/// document is still written, with the figure's own picture.
#[test]
fn a_huge_replacement_svg_is_refused_and_the_conversion_succeeds() {
    let dir = scratch("cli-replace-huge");
    std::fs::write(
        dir.join("huge.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="400000" height="400000"/>"#,
    )
    .unwrap();
    let (edited, _) = ligo_with_one_replacement(&dir, "huge.svg");
    let docx = dir.join("x.docx");
    let out = kakemono(&[
        "convert",
        LIGO,
        "--content",
        edited.to_str().unwrap(),
        "-o",
        docx.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stderr).contains("too large"));
    assert!(docx.is_file());
}

/// A picture that a replacement SVG names by a path relative to the SVG is
/// found whatever folder kakemono runs from, and kept inside the SVG.
#[test]
fn a_replacement_svg_keeps_the_pictures_it_names() {
    let dir = scratch("cli-replace-href");
    std::fs::create_dir_all(dir.join("vi")).unwrap();
    std::fs::write(dir.join("vi/square.png"), square_png()).unwrap();
    let (edited, (width, height)) = ligo_with_one_replacement(&dir, "vi/fig.svg");
    std::fs::write(
        dir.join("vi/fig.svg"),
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"><image href="square.png" width="{width}" height="{height}"/></svg>"#
        ),
    )
    .unwrap();
    let md = dir.join("out/x.md");
    let out = kakemono_in(
        &std::env::temp_dir(),
        &[
            "convert",
            ligo_abs(),
            "--content",
            edited.to_str().unwrap(),
            "-o",
            md.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let svg = std::fs::read_dir(dir.join("out/x_assets"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "svg"))
        .expect("the replaced figure is an SVG file");
    // Drawn, the SVG shows the square picture it names.
    let png = kakemono::backend::svg_to_png(&std::fs::read_to_string(svg).unwrap(), 1.0).unwrap();
    assert_eq!(middle_pixel(&png), Some([0, 128, 255]));
}

/// A replacement of another shape than the figure's box is fitted into it,
/// not stretched: in Word, the picture has the box's shape, the
/// replacement centred in it.
#[test]
fn a_replacement_of_another_shape_is_fitted_not_stretched() {
    let dir = scratch("cli-replace-shape");
    std::fs::write(dir.join("square.png"), square_png()).unwrap();
    let (edited, (width, height)) = ligo_with_one_replacement(&dir, "square.png");
    let docx = dir.join("x.docx");
    let out = kakemono(&[
        "convert",
        LIGO,
        "--content",
        edited.to_str().unwrap(),
        "-o",
        docx.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let box_aspect = width as f32 / height as f32;
    let parts = zip_parts(&std::fs::read(&docx).unwrap());
    let fitted = parts
        .iter()
        .filter(|(name, _)| name.starts_with("word/media/"))
        .any(|(_, data)| {
            image::load_from_memory(data).is_ok_and(|picture| {
                let aspect = picture.width() as f32 / picture.height() as f32;
                (aspect / box_aspect - 1.0).abs() < 0.02
                    && picture.to_rgb8().get_pixel(picture.width() / 2, picture.height() / 2).0 == [0, 128, 255]
            })
        });
    assert!(fitted, "no picture of the box's shape with the square in its middle");
}

/// A Word document laid out as a PDF shows the text of `--content`.
#[test]
fn word_pdf_shows_the_edited_text() {
    let dir = scratch("cli-word-pdf-content");
    let sample = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/word/sample.docx");
    let json = dir.join("sample.json");
    assert!(
        kakemono(&["convert", sample, "-o", json.to_str().unwrap()])
            .status
            .success()
    );
    let mut content: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    let heading = content["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|block| block["type"] == "heading")
        .unwrap();
    heading["text"] = "An edited heading".into();
    let edited = dir.join("sample.vi.json");
    std::fs::write(&edited, serde_json::to_string(&content).unwrap()).unwrap();
    let pdf = dir.join("sample.vi.pdf");
    let out = kakemono(&[
        "convert",
        sample,
        "--content",
        edited.to_str().unwrap(),
        "-o",
        pdf.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let lines = kakemono(&["lines", pdf.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&lines.stdout).contains("An edited heading"));
}
