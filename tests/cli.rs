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
fn word_document_has_no_page_modes() {
    let dir = scratch("cli-word-modes");
    let html = dir.join("sample.hard.html");
    let out = kakemono(&["convert", WORD_SAMPLE, "-m", "hard", "-o", html.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("a Word document has no pages"));
    assert!(!html.exists());
    // With --all, only the friendly page is written.
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
    let written: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(written, ["sample.friendly.html"]);
}
