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
