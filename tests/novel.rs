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

/// The advance of `ch` in thousandths of an em: the Times-Bold widths of
/// capitals, figures and the ornament's characters (Times-Roman has the
/// same for " " and "*"), else an average.
fn advance(ch: char) -> u32 {
    let capitals = [
        722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, 722, 778, 611, 778, 722, 556, 667, 722, 722,
        1000, 722, 722, 667,
    ];
    match ch {
        'A'..='Z' => capitals[ch as usize - 'A' as usize],
        '0'..='9' | '*' => 500,
        ' ' => 250,
        _ => 600,
    }
}

/// A line centred on the page, its width taken from [`advance`].
fn centred(font: &str, size: u32, y: u32, line: &str) -> String {
    let width = line.chars().map(advance).sum::<u32>() as f32 * size as f32 / 1000.0;
    text_at(font, size, 306.0 - width / 2.0, y, line)
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
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Bold /Encoding /WinAnsiEncoding >>".to_string(),
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

/// The pages of the novel: a title page, a prologue and six chapters.
fn pages() -> Vec<String> {
    // A lone "12" in bold, centred inside the third chapter: not a chapter,
    // since no chapter 11 or 13 is around it.
    let decoy = centred("F2", 16, 480, "12");
    let title = centred("F2", 24, 600, "THE NUMBERED CHAPTERS") + &centred("F1", 14, 560, "A. N. Author");
    // An unnumbered prologue, its title on three lines with the ornament
    // right under them in the same size, in the same block (four lines in
    // all).
    let mut prologue = centred("F2", 16, 700, "PROLOGUE");
    prologue.push_str(&centred("F2", 16, 681, "THE NARRATOR SPEAKS"));
    prologue.push_str(&centred("F2", 16, 662, "OF THE CRIMES"));
    prologue.push_str(&centred("F1", 16, 643, "* * *"));
    for (i, line) in TEXT.iter().enumerate() {
        prologue.push_str(&text("F1", 11, 72, 610 - 14 * i as u32, line));
    }
    vec![
        title,
        prologue,
        chapter("1 ROGER", ""),
        chapter("2", ""),
        chapter("3 ANDOVER", &decoy),
        chapter("4", ""),
        chapter("5", ""),
        chapter("6 BEXHILL", ""),
    ]
}

/// The (kind, text) of every block of the novel.
fn blocks() -> Vec<(String, String)> {
    blocks_of(&pages())
}

/// A novel is written in Typst as a book: chapters as headings of the
/// outermost level, their opening words marked, the title on a title page.
#[test]
fn novel_is_a_typst_book() {
    let paper = Paper::from_bytes(build_pdf(&pages())).expect("valid pdf");
    let analysis = paper.analyze();
    let typst = paper.to_typst(&analysis, &kakemono::TypstOptions::default()).typst;
    assert!(typst.contains("#let doc-layout = \"book\""), "{typst}");
    assert!(typst.contains("#let doc-title = \"THE NUMBERED CHAPTERS\""), "{typst}");
    assert!(typst.contains("\n#kakemono-title[THE NUMBERED CHAPTERS]\n"), "{typst}");
    assert!(typst.contains("\n= 3 ANDOVER"), "{typst}");
    assert!(typst.contains("\n#kakemono-opening[The letter] came"), "{typst}");
    let forced = kakemono::TypstOptions {
        layout: Some(kakemono::TypstLayout::Article),
        ..Default::default()
    };
    assert!(
        paper
            .to_typst(&analysis, &forced)
            .typst
            .contains("#let doc-layout = \"article\"")
    );
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
            "PROLOGUE THE NARRATOR SPEAKS OF THE CRIMES * * *",
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

/// A line of text in font `font` of size `size` at (`x`, `y`), `x` in
/// fractions of a point.
fn text_at(font: &str, size: u32, x: f32, y: u32, line: &str) -> String {
    format!("BT /{font} {size} Tf {x:.3} {y} Td ({line}) Tj ET\n")
}

/// A chapter opening with a drop cap `initial` (Times-Bold of `size`, its
/// advance `advance_em` wide) whose baseline lies `drop` points under the
/// first line: the `beside` lines start where its advance ends, the `rest`
/// at the margin. A last line apart (a blank line above it) follows.
fn drop_cap_chapter(
    heading: &str,
    initial: &str,
    size: u32,
    advance_em: f32,
    drop: u32,
    beside: &[&str],
    rest: &[&str],
) -> String {
    let mut page = centred("F2", 16, 700, heading);
    page.push_str(&text("F2", size, 72, 640 - drop, initial));
    let indent = 72.0 + advance_em * size as f32;
    let mut y = 640;
    for line in beside {
        page.push_str(&text_at("F1", 11, indent, y, line));
        y -= 14;
    }
    for line in rest {
        page.push_str(&text("F1", 11, 72, y, line));
        y -= 14;
    }
    page.push_str(&text(
        "F1",
        11,
        72,
        y - 14,
        "Nobody came to the door again that day, and the house was quiet.",
    ));
    page
}

/// The kind, text and scripts (any glyph raised or lowered) of the first
/// block after the heading of a one-chapter PDF.
fn opening(page: String) -> (String, String, bool) {
    let paper =
        Paper::from_bytes(build_pdf(&[centred("F2", 24, 600, "THE NUMBERED CHAPTERS"), page])).expect("valid pdf");
    let analysis = paper.analyze();
    let blocks = &analysis.doc.blocks;
    let at = blocks
        .iter()
        .position(|block| matches!(block.kind, BlockKind::Heading { .. }))
        .expect("a heading");
    let block = &blocks[at + 1];
    let scripts = block
        .lines
        .iter()
        .any(|line| line.glyphs.iter().any(|glyph| glyph.script != 0));
    (format!("{:?}", block.kind), block.text(), scripts)
}

#[test]
fn drop_cap_over_one_line_starts_its_paragraph() {
    // As calibre sets it: the cap reaches a little under the first line,
    // the second line already runs at the margin under it.
    for (size, drop) in [(22, 3), (28, 6), (34, 9)] {
        let page = drop_cap_chapter(
            "1 ROGER",
            "T",
            size,
            0.667,
            drop,
            &["he letter came on a grey morning, folded twice and sealed with plain wax, and"],
            &["unsurprisingly it was addressed to me in a hand that I did not know at all."],
        );
        let (kind, text, scripts) = opening(page);
        assert_eq!(kind, "Paragraph { in_abstract: false }", "size {size}");
        assert_eq!(
            text,
            "The letter came on a grey morning, folded twice and sealed with plain wax, and \
             unsurprisingly it was addressed to me in a hand that I did not know at all.",
            "size {size}"
        );
        assert!(!scripts, "size {size}");
    }
}

#[test]
fn drop_cap_opening_a_line_of_dialogue_is_not_a_heading() {
    for (size, drop) in [(22, 3), (28, 6), (34, 9)] {
        let page = drop_cap_chapter(
            "2 ANDOVER",
            "\\223G",
            size,
            0.5 + 0.778,
            drop,
            &["o away?\\224 I asked him."],
            &[],
        );
        let (kind, text, scripts) = opening(page);
        assert_eq!(
            (kind.as_str(), text.as_str()),
            (
                "Paragraph { in_abstract: false }",
                "\u{201C}Go away?\u{201D} I asked him."
            ),
            "size {size}"
        );
        assert!(!scripts, "size {size}");
    }
}

#[test]
fn drop_cap_over_three_lines_keeps_its_paragraph_whole() {
    let page = drop_cap_chapter(
        "1 ROGER",
        "T",
        48,
        0.667,
        28,
        &[
            "he letter came on a grey morning, folded twice and sealed with plain",
            "wax, and it was addressed to me in a hand that I did not know. I read",
            "it at the window while the tea went cold, and then I read it again,",
        ],
        &["more slowly, for it spoke of a crime that had not yet been done at all, and of"],
    );
    let (kind, text, _) = opening(page);
    assert_eq!(kind, "Paragraph { in_abstract: false }");
    assert!(text.starts_with("The letter came"), "{text}");
    assert!(text.ends_with("done at all, and of"), "{text}");
}

#[test]
fn prologue_with_its_ornament_apart_is_a_heading() {
    // The title of the prologue on three lines, the smaller ornament a
    // block of its own under them.
    let mut prologue = centred("F2", 16, 700, "PROLOGUE");
    prologue.push_str(&centred("F2", 16, 681, "THE NARRATOR SPEAKS"));
    prologue.push_str(&centred("F2", 16, 662, "OF THE CRIMES"));
    prologue.push_str(&centred("F1", 12, 638, "* * *"));
    for (i, line) in TEXT.iter().enumerate() {
        prologue.push_str(&text("F1", 11, 72, 604 - 14 * i as u32, line));
    }
    let pages = [
        centred("F2", 24, 600, "THE NUMBERED CHAPTERS"),
        prologue,
        chapter("1 ROGER", ""),
    ];
    let blocks = blocks_of(&pages);
    let headings: Vec<&str> = blocks
        .iter()
        .filter(|(kind, _)| kind == "h1")
        .map(|(_, text)| text.as_str())
        .collect();
    assert_eq!(
        headings,
        ["PROLOGUE THE NARRATOR SPEAKS OF THE CRIMES * * *", "1 ROGER * * *"],
        "{blocks:#?}"
    );
}
