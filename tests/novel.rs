//! End-to-end test on a small novel generated on the fly: chapters headed
//! by a number and a title ("3 ANDOVER"), or by their number alone ("2"),
//! each with an ornament under it, and a lone number in the text that is
//! not a chapter.

use kakemono::Paper;
use kakemono::model::BlockKind;

/// Lines of running text, a paragraph of each chapter.
const TEXT: &[&str] = &[
    "The letter came on a grey morning, folded twice and sealed with plain wax,",
    "and it was addressed to me in a hand that I did not know. I read it at the",
    "window while the tea went cold, and then I read it again more slowly, for",
    "it spoke of a crime that had not yet been done and of a man who knew of it.",
];

/// A line of text in font `font` of size `size` at (`x`, `y`).
fn text(font: &str, size: u32, x: u32, y: u32, line: &str) -> String {
    format!("BT /{font} {size} Tf {x} {y} Td ({line}) Tj ET\n")
}

/// A line centred on the page, its width estimated from its length.
fn centred(font: &str, size: u32, y: u32, line: &str) -> String {
    let width = line.len() as u32 * size * 6 / 10;
    text(font, size, 306 - width / 2, y, line)
}

/// A page of the novel: the chapter heading (bold, centred), the ornament
/// under it, then a paragraph of running text (its lines in another order
/// on each page, so that no line repeats like a running header).
fn chapter(heading: &str, extra: &str) -> String {
    let mut page = centred("F2", 16, 700, heading);
    page.push_str(&centred("F1", 12, 676, "* * *"));
    let shift = heading.len() % TEXT.len();
    for i in 0..TEXT.len() {
        let line = TEXT[(i + shift) % TEXT.len()];
        page.push_str(&text("F1", 11, 72, 640 - 14 * i as u32, line));
    }
    page.push_str(extra);
    page
}

/// The PDF of the novel, one chapter per page.
fn build_pdf(pages: &[String]) -> Vec<u8> {
    let mut objects: Vec<String> = vec![
        String::new(),
        String::new(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold >>".to_string(),
    ];
    let mut kids = Vec::new();
    for content in pages {
        let page_num = objects.len() + 1;
        kids.push(format!("{page_num} 0 R"));
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
             /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>",
            page_num + 1
        ));
        objects.push(format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content));
    }
    objects[0] = "<< /Type /Catalog /Pages 2 0 R >>".to_string();
    objects[1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len());
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for off in offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        objects.len() + 1,
        xref
    ));
    pdf.into_bytes()
}

/// The (kind, text) of every block of the PDF made of `pages`.
fn blocks_of(pages: &[String]) -> Vec<(String, String)> {
    let paper = Paper::from_bytes(build_pdf(pages)).expect("valid pdf");
    paper
        .analyze()
        .doc
        .blocks
        .iter()
        .map(|block| {
            let kind = match block.kind {
                BlockKind::Heading { level } => format!("h{level}"),
                BlockKind::Paragraph { .. } => "p".to_string(),
                _ => format!("{:?}", block.kind),
            };
            (kind, block.text())
        })
        .collect()
}

/// The (kind, text) of every block of the novel.
fn blocks() -> Vec<(String, String)> {
    // A lone "12" in bold, centred inside the third chapter: not a chapter,
    // since no chapter 11 or 13 is around it.
    let decoy = centred("F2", 16, 480, "12");
    let title = centred("F2", 24, 600, "THE NUMBERED CHAPTERS") + &centred("F1", 14, 560, "A. N. Author");
    let pages = [
        title,
        chapter("1 ROGER", ""),
        chapter("2", ""),
        chapter("3 ANDOVER", &decoy),
        chapter("4", ""),
        chapter("5", ""),
        chapter("6 BEXHILL", ""),
    ];
    blocks_of(&pages)
}

#[test]
fn bare_chapter_numbers_in_sequence_are_headings() {
    let blocks = blocks();
    let headings: Vec<&str> = blocks
        .iter()
        .filter(|(kind, _)| kind == "h1")
        .map(|(_, text)| text.as_str())
        .collect();
    assert_eq!(
        headings,
        [
            "1 ROGER * * *",
            "2 * * *",
            "3 ANDOVER * * *",
            "4 * * *",
            "5 * * *",
            "6 BEXHILL * * *"
        ],
        "{blocks:#?}"
    );
    assert!(
        blocks.iter().any(|(kind, text)| kind == "p" && text == "12"),
        "{blocks:#?}"
    );
}

/// A page of a novel made in Word: the chapter label in words and the
/// title on two lines in capitals, in the body font and left-aligned, then
/// the text; the first chapter starts under the book's title.
fn word_chapter(title: Option<&str>, label: &str, name: &str) -> String {
    let mut page = title.map_or(String::new(), |title| text("F2", 20, 72, 740, title));
    page.push_str(&text("F1", 11, 72, 700, label));
    page.push_str(&text("F1", 11, 72, 672, name));
    let shift = name.len() % TEXT.len();
    for i in 0..TEXT.len() {
        page.push_str(&text("F1", 11, 72, 644 - 14 * i as u32, TEXT[(i + shift) % TEXT.len()]));
    }
    page
}

#[test]
fn chapter_labels_in_words_are_headings_not_authors() {
    let pages = [
        word_chapter(Some("A Novel In Chapters"), "CHAPTER ONE", "THE BOY WHO LIVED"),
        word_chapter(None, "CHAPTER TWO", "THE VANISHING GLASS"),
        word_chapter(None, "CHAPTER THREE", "THE LETTERS FROM NO ONE"),
    ];
    let blocks = blocks_of(&pages);
    let headings: Vec<&str> = blocks
        .iter()
        .filter(|(kind, _)| kind == "h1")
        .map(|(_, text)| text.as_str())
        .collect();
    assert_eq!(
        headings,
        [
            "CHAPTER ONE THE BOY WHO LIVED",
            "CHAPTER TWO THE VANISHING GLASS",
            "CHAPTER THREE THE LETTERS FROM NO ONE"
        ],
        "{blocks:#?}"
    );
    assert!(!blocks.iter().any(|(kind, _)| kind == "Authors"), "{blocks:#?}");
}
