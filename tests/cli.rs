//! Runs the `kakemono` executable itself: the version, a conversion and the
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
