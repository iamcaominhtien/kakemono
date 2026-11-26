//! Web pages as input: the sample page of `tests/data/html` (written by
//! pandoc from `tests/data/word/sample.md`, the source of the sample Word
//! document and of the sample EPUB book), and small pages written here
//! with the markup of the sites most read (clutter around an article, lazy
//! pictures, Wikipedia's notes, arXiv's LaTeXML, encodings).

use kakemono::html::{Origin, read_with};
use kakemono::model::{Block, BlockKind, Document, FloatKind};
use kakemono::{
    Analysis, DocxOptions, EpubOptions, HtmlOptions, InputFormat, JsonOptions, LatexOptions, MarkdownOptions, Mode,
    Paper, TypstOptions,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The sample page.
const SAMPLE_HTML: &str = "html/sample.html";
/// The same document as a Word file.
const SAMPLE_DOCX: &str = "word/sample.docx";
/// The same document as an EPUB book.
const SAMPLE_EPUB: &str = "epub/sample.epub";

/// The path of a file of `tests/data`.
fn data(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
}

/// Loads (from its file, its picture next to it) and analyses a file of
/// `tests/data`.
fn load(name: &str) -> (Paper, Analysis) {
    let paper = Paper::from_path(data(name)).unwrap();
    let analysis = paper.analyze();
    (paper, analysis)
}

/// The document of the page `html` at the address `url`, its pictures
/// fetched from `pictures` (by address) instead of the Web.
fn page_at(html: &str, url: &str, pictures: &[(&str, &[u8])]) -> Document {
    let pictures: HashMap<String, Vec<u8>> = pictures
        .iter()
        .map(|(url, data)| ((*url).to_string(), data.to_vec()))
        .collect();
    let fetch = move |urls: &[String]| -> HashMap<String, Vec<u8>> {
        urls.iter()
            .filter_map(|url| Some((url.clone(), pictures.get(url)?.clone())))
            .collect()
    };
    let origin = Origin::Url { url, charset: None };
    read_with(html.as_bytes(), origin, None, &fetch).unwrap()
}

/// The document of the page `html`, read from its bytes.
fn page(html: &str) -> Document {
    Paper::from_bytes(html.as_bytes().to_vec()).unwrap().analyze().doc
}

/// The texts of the blocks of `doc`.
fn texts(doc: &Document) -> Vec<String> {
    doc.blocks.iter().map(Block::text).collect()
}

/// The block whose text starts with `start`.
fn block<'a>(doc: &'a Document, start: &str) -> &'a Block {
    doc.blocks
        .iter()
        .find(|block| block.text().starts_with(start))
        .unwrap_or_else(|| panic!("no block starting with {start:?} in {:#?}", texts(doc)))
}

/// Whether some block of `doc` contains `text`.
fn has_text(doc: &Document, text: &str) -> bool {
    doc.blocks.iter().any(|block| block.text().contains(text))
}

#[test]
fn html_sample_reads_like_the_docx() {
    let summary = |paper: Paper| -> Vec<String> {
        paper
            .analyze()
            .doc
            .blocks
            .iter()
            .filter(|block| !matches!(block.kind, BlockKind::Equation { .. }))
            .map(|block| format!("{:?} {} {:?}", block.kind, block.text(), block.anchor))
            .collect()
    };
    let html = summary(Paper::from_path(data(SAMPLE_HTML)).unwrap());
    let docx = summary(Paper::from_bytes(std::fs::read(data(SAMPLE_DOCX)).unwrap()).unwrap());
    let epub = summary(Paper::from_bytes(std::fs::read(data(SAMPLE_EPUB)).unwrap()).unwrap());
    assert_eq!(html, docx);
    assert_eq!(html, epub);
}

#[test]
fn html_sample_has_its_structure() {
    let (paper, analysis) = load(SAMPLE_HTML);
    let doc = &analysis.doc;
    assert_eq!(paper.format(), InputFormat::Html);
    assert_eq!(paper.format().name(), "html");
    assert!(!paper.format().has_pages());
    assert_eq!(doc.meta.source_format.as_deref(), Some("html"));
    assert_eq!(
        doc.meta.title.as_deref(),
        Some("Convergence of Gradient Descent on Smooth Functions")
    );
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title));
    assert!(matches!(doc.blocks[1].kind, BlockKind::Authors));
    // The picture is read next to the page (`../word/curve.png`), at the
    // size of its style (3 inches).
    let figure = block(doc, "Figure 1:");
    let BlockKind::Figure {
        kind: FloatKind::Figure,
        image: Some(image),
        ..
    } = &figure.kind
    else {
        panic!("the figure has a picture")
    };
    assert!((image.rect.width() - 216.0).abs() < 1.0, "3 inches wide");
    assert!(doc.pictures[0].data.starts_with(b"\x89PNG"));
    // pandoc's note: a footnote after its paragraph.
    let note_at = doc
        .blocks
        .iter()
        .position(|block| block.text().starts_with("It goes back"))
        .unwrap();
    assert!(matches!(&doc.blocks[note_at].kind, BlockKind::Footnote { marker } if marker == "1"));
}

#[test]
fn from_bytes_reads_a_page_without_its_files() {
    let bytes = std::fs::read(data(SAMPLE_HTML)).unwrap();
    let paper = Paper::from_bytes(bytes).unwrap();
    assert_eq!(paper.format(), InputFormat::Html);
    let doc = paper.analyze().doc;
    // The picture next to the file is not known: the figure keeps its
    // caption.
    assert!(has_text(&doc, "Figure 1: The loss along the iterations."));
    assert!(has_text(&doc, "It goes back to Cauchy (1847)."));
}

#[test]
fn every_output_can_be_written_from_html() {
    let (paper, analysis) = load(SAMPLE_HTML);
    for mode in [Mode::Hard, Mode::Hard95, Mode::Medium, Mode::Friendly] {
        let opts = HtmlOptions {
            mode,
            ..Default::default()
        };
        let html = paper.to_html(&analysis, &opts).html;
        assert!(html.contains("Convergence of Gradient Descent"), "{mode:?}");
    }
    assert!(paper.to_docx(&analysis, &DocxOptions::default()).starts_with(b"PK"));
    let markdown = paper.to_markdown(&analysis, &MarkdownOptions::default()).markdown;
    assert!(markdown.contains("def step(x, grad, eta):"));
    assert!(paper.to_epub(&analysis, &EpubOptions::default()).starts_with(b"PK"));
    let json = paper.to_json(&analysis, &JsonOptions::default()).json;
    assert!(json.contains(r#""format": "html""#));
    assert!(paper.to_pdf().unwrap().starts_with(b"%PDF"));
    // The colophons name a web page.
    let friendly = paper.to_html(&analysis, &HtmlOptions::default()).html;
    assert!(friendly.contains("Converted from HTML by kakemono"));
    let latex = paper.to_latex(&analysis, &LatexOptions::default()).latex;
    assert!(latex.contains("Converted from a web page"));
    let typst = paper.to_typst(&analysis, &TypstOptions::default()).typst;
    assert!(typst.contains("Converted from a web page"));
}

/// A news article as many sites write it: the article among a menu, a
/// cookie notice, sharing buttons, a list of related stories, a sidebar and
/// a footer, with lazy pictures.
const NEWS: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<title>Rivers rise after the storm | The Daily Example</title>
<meta property="og:title" content="Rivers rise after the storm">
<meta name="author" content="Ann Lee, Bo Chen">
<style>.promo-box{display:none}</style>
</head><body>
<div id="cookie-banner"><p>We use cookies to give you the best experience on our website, please accept them all now.</p><button>Accept</button></div>
<header class="site-header"><a href="/">The Daily Example</a><nav><ul><li><a href="/world">World</a></li><li><a href="/sport">Sport</a></li></ul></nav></header>
<main>
<article>
<h1>Rivers rise after the storm</h1>
<div class="share-tools"><a href="https://twitter.com/share">Share on X</a><span>Share this story with your friends and family today please</span></div>
<p>Heavy rain fell across the valley overnight, and the rivers rose to their highest level in a decade, officials said on Monday morning.</p>
<figure><img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="/img/river-small.png" data-srcset="/img/river-640.png 640w, /img/river-1920.png 1920w" width="1920" height="1080" alt="The river"><figcaption>The river at dawn.</figcaption></figure>
<p>Residents of the lower town were asked to leave their homes before noon, and schools stayed closed for the rest of the week.<br><br>The water is expected to fall slowly over the next three days, according to the forecast of the weather service.</p>
<img src="/img/pixel.gif" width="1" height="1" alt="">
<p hidden>Hidden text that is not part of the article at all, never shown to readers.</p>
<div class="related-stories"><h2>Related</h2><p>Another story about another storm that happened somewhere else last year in the hills.</p></div>
</article>
</main>
<aside class="sidebar"><h2>Most read</h2><p>A story that many people read today, about something else entirely, on another page.</p></aside>
<footer><p>Copyright The Daily Example. All rights reserved, everywhere, forever and ever.</p></footer>
</body></html>"#;

#[test]
fn the_article_is_found_among_the_clutter() {
    let png = std::fs::read(data("word/curve.png")).unwrap();
    let doc = page_at(
        NEWS,
        "https://news.example.org/2026/storm.html",
        &[("https://news.example.org/img/river-1920.png", &png)],
    );
    assert_eq!(doc.meta.title.as_deref(), Some("Rivers rise after the storm"));
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title));
    let authors: Vec<String> = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
        .map(Block::text)
        .collect();
    assert_eq!(authors.join(", "), "Ann Lee, Bo Chen");
    assert!(has_text(&doc, "Heavy rain fell"));
    // Two line breaks end a paragraph.
    assert!(block(&doc, "Residents").text().ends_with("rest of the week."));
    assert!(
        block(&doc, "The water is expected")
            .text()
            .ends_with("weather service.")
    );
    for clutter in [
        "cookies",
        "World",
        "Share",
        "Related",
        "Another story",
        "Most read",
        "Copyright",
        "Hidden text",
    ] {
        assert!(!has_text(&doc, clutter), "{clutter:?} in {:#?}", texts(&doc));
    }
    // The lazy picture is its best version (1920 pixels wide), fitted to
    // the page; the tracking pixel is gone.
    assert_eq!(doc.pictures.len(), 1);
    let figure = block(&doc, "The river at dawn.");
    let BlockKind::Figure { image: Some(image), .. } = &figure.kind else {
        panic!("the figure has its picture: {:#?}", figure.kind)
    };
    assert!(image.rect.width() <= 6.5 * 72.0 + 1.0, "{}", image.rect.width());
}

/// A Vietnamese news page as Dân trí lays it out: its summary in an `<h2>`,
/// the name of the site as its `author`, and after the article, teasers
/// for other articles and tags whose headings are links, in its column.
const NEWS_VI: &str = r#"<!doctype html><html lang="vi"><head>
<title>Cháy nhà 4 tầng trên phố cổ | Báo Dân trí</title>
<meta name="author" content="Dân trí"><meta property="og:site_name" content="Báo điện tử Dân trí">
</head><body><main><div class="col">
<section><ul><li><a href="/thoi-su.htm">Thời sự</a></li></ul><time>Thứ ba, 24/11/2026 - 16:59</time></section>
<section><article id="articleContent"><h1>Cháy nhà 4 tầng trên phố cổ</h1>
<div><a rel="author" href="/tac-gia/tran-thanh.htm">Trần Thanh</a></div>
<h2 data-slot="sapo">(Dân trí) - Theo cảnh sát, diện tích đám cháy khoảng 20m2, ngôi nhà bị cháy cao 4 tầng nằm ở phố Hàng Thiếc.</h2>
<div data-slot="content"><p>Khoảng 14h24 ngày 24/11, tại căn nhà cao 4 tầng nằm ở số 62 phố Hàng Thiếc, phường Hoàn Kiếm, xảy ra hỏa hoạn.</p>
<p>Sau khi nhận tin báo, cảnh sát điều nhiều xe chữa cháy tới hiện trường. Tới khoảng 15h, đám cháy được khống chế hoàn toàn.</p>
<p>Theo cảnh sát, ngôi nhà bị cháy một phần ở tầng 2 và tầng 3. Rất may vụ hỏa hoạn không gây thiệt hại về người.</p></div>
</article></section>
<section><article><h3><a href="/thoi-su/nu-chu-quan.htm">Nữ chủ quán bún tử vong trong đám cháy</a></h3>
<a href="/thoi-su/nu-chu-quan.htm">Đám cháy bùng phát tại quán bún bò lúc rạng sáng.</a></article></section>
<section><ul><li><h3>Tags:</h3></li><li><h4><a href="/tim-kiem/chay-nha.htm">cháy nhà</a></h4></li></ul></section>
</div></main></body></html>"#;

#[test]
fn news_summary_is_a_paragraph_and_teasers_are_gone() {
    let doc = page_at(NEWS_VI, "https://dantri.com.vn/thoi-su/chay-nha.htm", &[]);
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title));
    // The summary is a paragraph, not a heading.
    let summary = block(&doc, "(Dân trí) - Theo cảnh sát");
    assert!(
        matches!(summary.kind, BlockKind::Paragraph { .. }),
        "{:?}",
        summary.kind
    );
    // The name of the site is not an author.
    let authors: Vec<String> = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
        .map(Block::text)
        .collect();
    assert!(!authors.iter().any(|author| author.contains("Dân trí")), "{authors:?}");
    assert!(has_text(&doc, "Rất may vụ hỏa hoạn"));
    for clutter in ["Thời sự", "16:59", "Nữ chủ quán bún", "Tags:", "cháy nhà"] {
        assert!(!has_text(&doc, clutter), "{clutter:?} in {:#?}", texts(&doc));
    }
}

/// A news page as Tuổi Trẻ, Thanh Niên and Vietnamnet lay it out: a date
/// before the title, the author twice, the site's domain as its `author`,
/// teasers in the body (a nested `<article>`, a box whose heading is a
/// link, a paragraph that is a link), and after the body counts of
/// reactions, tags and a newsletter box.
const NEWS_WIDGETS: &str = r#"<!doctype html><html lang="vi"><head>
<title>Truy tặng huân chương cho hai người cứu hộ - Báo Thanh Niên</title>
<meta name="author" content="thanhnien.vn"><link rel="canonical" href="https://thanhnien.vn/truy-tang.htm">
</head><body><main><div class="detail">
<div class="detail-top"><time>24/11/2026 15:52 GMT+7</time></div>
<h1>Truy tặng huân chương cho hai người cứu hộ</h1>
<div class="info"><a href="/tac-gia/xuan-hoat.htm">XUÂN HOÁT</a> <a href="/tac-gia/xuan-hoat.htm">XUÂN HOÁT</a></div>
<p class="sapo">Hai cán bộ cứu hộ đã hy sinh khi cứu người đuối nước ở biển Nha Trang, được truy tặng huân chương.</p>
<div class="detail-content">
<p>Ngày 24-11, Chủ tịch nước đã có quyết định truy tặng Huân chương Dũng cảm cho hai cán bộ của đội cứu nạn, cứu hộ.</p>
<div class="box"><ul><li><a href="/loi-tu-biet.htm"><img src="/thumb.jpg" alt=""></a>
<h4><a href="/loi-tu-biet.htm">Lời từ biệt lặng lẽ của người cứu hộ</a><a href="/loi-tu-biet.htm">ĐỌC NGAY</a></h4></li></ul></div>
<p>Hai cá nhân đó là ông An và ông Sáng, cán bộ đội cứu nạn, cứu hộ thuộc ban quản lý bãi biển Nha Trang.</p>
<article class="insert-news"><a href="/cdv-nga-mu.htm">Người dân Nha Trang tưởng nhớ hai người cứu hộ</a>
<div>Hàng trăm người đã đến viếng.</div></article>
<p><a href="/khanh-hoa-to-trinh.htm">Khánh Hòa có tờ trình về việc truy tặng huân chương</a></p>
<div type="RelatedOneNews"><a href="/xom-chai.htm"><img src="/xom-chai.jpg" alt=""></a><a href="/xom-chai.htm">Xóm chài Cửa Bé tiễn biệt người cứu hộ</a>
<p>Khoảnh khắc anh lao vào sóng dữ để cùng đồng đội đưa hai nạn nhân vào bờ.</p></div>
<p>Trước nghĩa cử cao đẹp, không quản hiểm nguy để cứu người, lãnh đạo tỉnh đã đề nghị khen thưởng cả hai người.</p>
<p>Anh ra đi ngay trước ngày kỷ niệm 20 năm ngày cưới, để lại vợ, mẹ già và hai con nhỏ trong căn nhà ven biển.</p>
<p>Gia đình hai cán bộ cứu hộ đã nhận được sự hỗ trợ của chính quyền địa phương và nhiều nhà hảo tâm trong tỉnh.</p>
</div>
<div class="reactions"><div><span>0</span> <span>0</span> <span>0</span></div><span>Tặng sao cho bài viết hay</span></div>
<div class="detail-tab"><h4>Khám phá thêm chủ đề</h4><a href="/cuu-nan.html">Cứu nạn</a></div>
<div class="newsletter"><h3>Đăng ký ngay để nhận gói tin tức mới</h3><p>Chúng tôi sẽ gởi đến bạn những tin nổi bật nhất.</p></div>
</div></main></body></html>"#;

#[test]
fn news_widgets_and_teasers_are_gone() {
    let doc = page(NEWS_WIDGETS);
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title), "{:#?}", texts(&doc));
    assert!(has_text(&doc, "Hai cán bộ cứu hộ đã hy sinh"));
    assert!(has_text(&doc, "Gia đình hai cán bộ"));
    assert_eq!(
        texts(&doc).iter().filter(|text| text.contains("XUÂN HOÁT")).count(),
        1,
        "{:#?}",
        texts(&doc)
    );
    for clutter in [
        "15:52",
        "thanhnien.vn",
        "Lời từ biệt",
        "ĐỌC NGAY",
        "tưởng nhớ",
        "tờ trình",
        "Xóm chài",
        "sóng dữ",
        "Tặng sao",
        "Khám phá thêm",
        "Đăng ký ngay",
    ] {
        assert!(!has_text(&doc, clutter), "{clutter:?} in {:#?}", texts(&doc));
    }
    assert!(
        !texts(&doc).iter().any(|text| text.trim() == "0 0 0"),
        "{:#?}",
        texts(&doc)
    );
}

#[test]
fn a_page_without_article_reads_its_description() {
    let html = r#"<!doctype html><html><head><title>Khoảnh khắc quán lẩu phát nổ</title>
<meta name="description" content="Camera nhà dân ghi lại khoảnh khắc quán lẩu đang sửa chữa phát nổ khiến mảnh vỡ văng khắp đoạn đường hơn 20 m.">
</head><body><h1>Khoảnh khắc quán lẩu phát nổ</h1><div class="player"><div>Phím tắt</div>
<ul><li>Tới video tiếp theo</li><li>Tắt âm thanh</li></ul></div></body></html>"#;
    let doc = page(html);
    assert!(has_text(&doc, "Camera nhà dân ghi lại"), "{:#?}", texts(&doc));
    assert!(!has_text(&doc, "Phím tắt"), "{:#?}", texts(&doc));
}

#[test]
fn a_picture_that_fails_is_fetched_in_another_version() {
    let png = std::fs::read(data("word/curve.png")).unwrap();
    let html = r#"<!doctype html><html><head><title>Storm</title></head><body><article><h1>Storm</h1>
<p>The river rose through the night and reached the bridge before dawn, the highest in ten years.</p>
<figure><img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" data-src="/img/river-1020.png"
 data-srcset="/img/river-1020.png 1x, /img/river-1360.png 2x"><figcaption>The river at dawn.</figcaption></figure>
<p>Residents of the lower town were asked to leave their homes until the water goes down again.</p>
</article></body></html>"#;
    // The best version (2x) fails; the 1x one is served.
    let doc = page_at(
        html,
        "https://news.example.org/storm.html",
        &[("https://news.example.org/img/river-1020.png", &png)],
    );
    let figure = block(&doc, "The river at dawn.");
    assert!(
        matches!(&figure.kind, BlockKind::Figure { image: Some(_), .. }),
        "{:?}",
        figure.kind
    );
    assert_eq!(doc.pictures.len(), 1);
}

/// A technology news page as TechCrunch, The Verge and Ars Technica lay it
/// out: a summary and a caption shown twice, a byline whose name is in a
/// pop-up, a caption in a font named by a custom property, a link to the
/// comments, an affiliate disclaimer and the box about the author after
/// the body.
const TECH_NEWS: &str = r#"<!doctype html><html lang="en"><head><title>Hacker sentenced</title>
<style>.credits { font-family: var(--wp--custom--font--family--mono) }</style></head><body><main><div class="column">
<div class="body"><h1>The hacker behind the leaks has been sentenced</h1>
<p>Arion Kurtaj, who hacked a game studio and leaked its videos, was released after pleading guilty.</p>
<p>Arion Kurtaj, who hacked a game studio and leaked its videos, was released after pleading guilty.</p>
<p>by</p><p><a href="/article#comments">43 Comments</a></p>
<p class="byline"><a href="/author/beth/">Beth Mole</a> – Nov 23, 2026 4:58 pm <span>|</span> <a href="/article#comments">163</a></p>
<figure><img src="data:image/png;base64,iVBORw0KGgo=" width="600" height="300"><figcaption><span class="credits"><strong>Image Credits:</strong> Rockstar Games</span></figcaption></figure>
<div class="caption-content">Credit: NASA</div><div class="caption-content">Credit: NASA</div>
<p>The judge said the hacker would stay in a secure hospital, where he had already spent time before his trial.</p>
<p>Rockstar launched the game last week to positive reviews, and said the leaks had not hurt its sales at all.</p>
<p>The studio declined to comment further on the case, which had been followed closely by players around the world.</p>
<p>Prosecutors had argued that the hacker posed a continuing risk, as he had kept hacking while he was on bail last year.</p>
<p>His lawyers said that he had cooperated with the investigation and that he had not profited from any of the leaks.</p>
<p>The court heard that the leaks had cost the studio millions of dollars and thousands of hours of work by its staff.</p>
<p>The game, the most expensive ever made, sold more copies in its first week than any game before it, the studio said.</p>
<div class="follow"><span><strong>Follow topics and authors</strong> from this story to see more like this in your feed.</span>
<ul><li><span role="button">Jay Peters</span></li></ul></div>
</div>
<p class="affiliate-disclaimer-text">When you purchase through links in our articles, we may earn a small commission.</p>
<div class="post-authors"><a href="/author/jay-peters/">Jay Peters</a><p>Senior Reporter</p>
<p>Jay is a senior reporter covering technology, gaming, and more. He joined the site in 2019 after two years elsewhere.</p></div>
</div></main></body></html>"#;

#[test]
fn tech_news_repeats_bylines_and_author_boxes_are_gone() {
    let doc = page(TECH_NEWS);
    let summary = "Arion Kurtaj, who hacked";
    assert_eq!(
        texts(&doc).iter().filter(|text| text.starts_with(summary)).count(),
        1,
        "{:#?}",
        texts(&doc)
    );
    assert_eq!(
        texts(&doc).iter().filter(|text| text.contains("Credit: NASA")).count(),
        1,
        "{:#?}",
        texts(&doc)
    );
    assert!(has_text(&doc, "studio declined to comment"));
    for clutter in [
        "43 Comments",
        "purchase through links",
        "Senior Reporter",
        "joined the site",
        "Follow topics",
        "163",
        "pm |",
    ] {
        assert!(!has_text(&doc, clutter), "{clutter:?} in {:#?}", texts(&doc));
    }
    assert!(
        !texts(&doc).iter().any(|text| text.trim() == "by"),
        "{:#?}",
        texts(&doc)
    );
    // The caption is not code.
    let caption = doc
        .blocks
        .iter()
        .find(|block| block.text().contains("Rockstar Games"))
        .expect("the caption");
    assert!(
        caption.spans.iter().all(|span| span.style & kakemono::model::MONO == 0),
        "{:?}",
        caption.spans
    );
}

#[test]
fn the_photo_above_an_article_is_its_lead_picture() {
    let png = std::fs::read(data("word/curve.png")).unwrap();
    let body =
        "The engineer is suing the company over how it handled her complaints, the lawsuit filed on Monday says. ";
    let html = format!(
        r#"<!doctype html><html><head><title>Lawsuit</title>
<meta property="og:image" content="https://news.example.org/uploads/lawsuit.png?resize=1200,877"></head><body>
<div class="hero"><figure><img src="https://news.example.org/uploads/lawsuit.png?w=1024" width="1024" height="749">
<figcaption>Image Credits: Getty Images</figcaption></figure></div>
<main><div class="entry-content"><h1>Engineer sues</h1><p>{body}</p><p>{body}</p><p>{body}</p></div></main></body></html>"#
    );
    let doc = page_at(
        &html,
        "https://news.example.org/lawsuit",
        &[("https://news.example.org/uploads/lawsuit.png?w=1024", &png)],
    );
    let figure = block(&doc, "Image Credits");
    assert!(
        matches!(&figure.kind, BlockKind::Figure { image: Some(_), .. }),
        "{:?}",
        figure.kind
    );
    // After the title, before the text.
    let at = |start: &str| {
        doc.blocks
            .iter()
            .position(|block| block.text().starts_with(start))
            .unwrap()
    };
    assert!(at("Engineer sues") < at("Image Credits") && at("Image Credits") < at("The engineer"));
}

#[test]
fn links_become_absolute_or_internal() {
    let html = r##"<html><body><article><h1>Links</h1>
        <p>See <a href="../other.html">another page</a>, <a href="#part">the part below</a>,
        <a href="https://news.example.org/a/page.html#part">this very part</a> and
        <a href="javascript:void(0)">nothing</a>.</p>
        <h2 id="part">Part</h2><p>The part of the page that the links go to, with enough text to be read.</p>
        </article></body></html>"##;
    let doc = page_at(html, "https://news.example.org/a/page.html", &[]);
    let para = block(&doc, "See ");
    let href = |text: &str| {
        para.spans
            .iter()
            .find(|span| span.text.contains(text))
            .and_then(|span| span.href.clone())
    };
    assert_eq!(
        href("another page").as_deref(),
        Some("https://news.example.org/other.html")
    );
    // The anchor of a heading is named as the reader names it.
    assert_eq!(href("the part below").as_deref(), Some("#sec-part"));
    assert_eq!(href("this very part").as_deref(), Some("#sec-part"));
    assert_eq!(href("nothing"), None);
}

#[test]
fn wikipedia_notes_are_footnotes() {
    // Wikipedia's markup (Parsoid): a superscript link to an item of the
    // list of references, which links back.
    let html = r##"<!DOCTYPE html><html><head><title>Example - Wikipedia</title></head>
        <body class="mediawiki"><div id="content"><h1 id="firstHeading"><span class="mw-page-title-main">Example</span></h1>
        <div id="mw-content-text"><div class="mw-parser-output">
        <div class="mw-heading mw-heading2"><h2 id="History">History</h2><span class="mw-editsection">[<a href="/w/edit">edit</a>]</span></div>
        <p>The town was founded in 1820 by settlers from the coast, who built a mill on the river.<sup id="cite_ref-1" class="mw-ref reference"><a href="#cite_note-1"><span class="cite-bracket">[</span>1<span class="cite-bracket">]</span></a></sup> It grew quickly.</p>
        <div class="navbox" role="navigation"><p>Towns of the region: one, two, three, four, five and six and more.</p></div>
        <div class="mw-heading mw-heading2"><h2 id="References">References</h2></div>
        <div class="mw-references-wrap"><ol class="mw-references references">
        <li id="cite_note-1"><span class="mw-cite-backlink"><a href="#cite_ref-1">↑</a></span> <span class="reference-text">Smith, A History of the Town, 1901.</span></li>
        </ol></div></div></div></div></body></html>"##;
    let doc = page(html);
    assert_eq!(doc.meta.title.as_deref(), Some("Example"));
    assert!(matches!(doc.blocks[0].kind, BlockKind::Title));
    let note_at = doc
        .blocks
        .iter()
        .position(|block| block.text().starts_with("Smith, A History"))
        .unwrap_or_else(|| panic!("{:#?}", texts(&doc)));
    assert!(matches!(&doc.blocks[note_at].kind, BlockKind::Footnote { .. }));
    assert_eq!(doc.blocks[note_at].text(), "Smith, A History of the Town, 1901.");
    assert!(doc.blocks[note_at - 1].text().starts_with("The town was founded"));
    assert!(!has_text(&doc, "[1]"));
    assert!(!has_text(&doc, "edit"));
    assert!(!has_text(&doc, "Towns of the region"));
}

#[test]
fn latexml_notes_and_authors() {
    // arXiv's HTML papers (LaTeXML): a note written where it is called,
    // authors as spans with their addresses.
    let html = r##"<!DOCTYPE html><html><head><title>A Paper</title></head><body>
        <div class="ltx_page_main"><div class="ltx_page_content"><article class="ltx_document">
        <h1 class="ltx_title ltx_title_document">A Paper</h1>
        <div class="ltx_authors"><span class="ltx_creator ltx_role_author"><span class="ltx_personname">Ada Byron<span class="ltx_note ltx_role_thanks"><sup class="ltx_note_mark">1</sup><span class="ltx_note_outer"><span class="ltx_note_content"><sup class="ltx_note_mark">1</sup><span class="ltx_note_type">thanks: </span>Equal contribution.</span></span></span></span><span class="ltx_author_notes"><span class="ltx_contact ltx_role_email"><span class="ltx_contact_name">Email: </span><a href="mailto:">ada@example.org</a></span></span></span>
        <span class="ltx_author_before"> and </span><span class="ltx_creator ltx_role_author"><span class="ltx_personname">Alan Turing<span class="ltx_note ltx_role_footnotemark"><sup class="ltx_note_mark">1</sup><span class="ltx_note_outer"><span class="ltx_note_content"><sup class="ltx_note_mark">1</sup><span class="ltx_note_type">footnotemark: </span></span></span></span></span></span></div>
        <div class="ltx_abstract"><h6 class="ltx_title ltx_title_abstract">Abstract</h6><p class="ltx_p">We show how a result follows from another one in few steps.</p></div>
        <section class="ltx_section"><h2 class="ltx_title ltx_title_section">1 Introduction</h2>
        <div class="ltx_para"><p class="ltx_p">The method is old<span class="ltx_note ltx_role_footnote"><sup class="ltx_note_mark">2</sup><span class="ltx_note_outer"><span class="ltx_note_content"><sup class="ltx_note_mark">2</sup><span class="ltx_tag ltx_tag_note">2</span>It goes back to Cauchy.</span></span></span> and simple.</p></div>
        </section></article></div></div></body></html>"##;
    let doc = page(html);
    let authors: Vec<String> = doc
        .blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
        .map(Block::text)
        .collect();
    assert_eq!(authors.len(), 2, "{:#?}", texts(&doc));
    assert!(authors[0].starts_with("Ada Byron"));
    assert!(authors[1].starts_with("Alan Turing"));
    assert!(!has_text(&doc, "thanks:") && !has_text(&doc, "footnotemark") && !has_text(&doc, "Email:"));
    let email = doc
        .blocks
        .iter()
        .flat_map(|block| &block.spans)
        .find(|span| span.text.contains("ada@example.org"))
        .and_then(|span| span.href.as_deref());
    assert_eq!(email, Some("mailto:ada@example.org"));
    let note = block(&doc, "It goes back to Cauchy.");
    assert!(matches!(&note.kind, BlockKind::Footnote { .. }));
    assert_eq!(
        block(&doc, "The method is old").text(),
        "The method is old2 and simple."
    );
    assert!(matches!(
        block(&doc, "Equal contribution.").kind,
        BlockKind::Footnote { .. }
    ));
    assert!(matches!(block(&doc, "Abstract").kind, BlockKind::Heading { .. }));
}

#[test]
fn encodings_and_inline_pictures() {
    // Windows-1252, as an old page without a declaration.
    let mut bytes = b"<html><body><h1>Caf\xe9</h1><p>A \x93quoted\x94 word in a page that is old.</p>".to_vec();
    // A picture inside the page, as a `data:` URL.
    let png = std::fs::read(data("word/curve.png")).unwrap();
    let encoded = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD.encode(&png)
    };
    bytes.extend_from_slice(format!(r#"<figure><img src="data:image/png;base64,{encoded}" alt="A curve"><figcaption>Figure 1: A curve.</figcaption></figure></body></html>"#).as_bytes());
    let paper = Paper::from_bytes(bytes).unwrap();
    let doc = paper.analyze().doc;
    assert_eq!(doc.meta.title.as_deref(), Some("Café"));
    assert!(has_text(&doc, "A “quoted” word"));
    assert_eq!(doc.pictures.len(), 1);
    assert!(doc.pictures[0].data.starts_with(b"\x89PNG"));
}

#[test]
fn html_outputs_match_their_snapshot() {
    let (paper, analysis) = load(SAMPLE_HTML);
    let html_opts = HtmlOptions {
        embed_fonts: false,
        ..Default::default()
    };
    let outputs: Vec<(&str, Vec<u8>)> = vec![
        ("friendly", paper.to_html(&analysis, &html_opts).html.into_bytes()),
        ("docx", paper.to_docx(&analysis, &DocxOptions::default())),
        (
            "markdown",
            paper
                .to_markdown(&analysis, &MarkdownOptions::default())
                .markdown
                .into_bytes(),
        ),
        (
            "latex",
            paper.to_latex(&analysis, &LatexOptions::default()).latex.into_bytes(),
        ),
        (
            "typst",
            paper.to_typst(&analysis, &TypstOptions::default()).typst.into_bytes(),
        ),
        ("epub", paper.to_epub(&analysis, &EpubOptions::default())),
        (
            "content",
            paper.to_json(&analysis, &JsonOptions::default()).json.into_bytes(),
        ),
    ];
    let mut text = String::new();
    for (output, bytes) in outputs {
        let bytes = String::from_utf8_lossy(&bytes)
            .replace(env!("CARGO_PKG_VERSION"), "VERSION")
            .into_bytes();
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        text.push_str(&format!("{SAMPLE_HTML} {output} {hash}\n"));
    }
    let file = data("snapshots/html-sample.txt");
    if std::env::var("KAKEMONO_UPDATE_SNAPSHOTS").is_ok_and(|value| value == "1") {
        std::fs::write(&file, &text).unwrap();
        return;
    }
    let old = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(
        old, text,
        "the output of the HTML sample changed; if this is intended, run \
         KAKEMONO_UPDATE_SNAPSHOTS=1 cargo test --release --test html"
    );
}

/// A page of a government site as GOV.UK and usa.gov lay it out: a link to
/// skip the contents, cards that are links with a heading in a
/// `…__header`, a hidden heading that names a tab, and a feedback form.
const GOV_GUIDE: &str = r##"<!doctype html><html lang="en"><head><title>How VAT works - GOV.UK</title></head>
<body><main><h1>How VAT works</h1>
<a class="gem-c-skip-link govuk-skip-link" href="#guide-contents">Skip contents</a>
<p>VAT (Value Added Tax) is a tax added to most products and services sold by VAT-registered businesses.
Businesses have to register for VAT if their taxable turnover is more than the threshold.</p>
<h2 class="fr-sr-only">In the UK</h2>
<p>As a VAT-registered business<span role="tooltip">: titleContent</span> you must include VAT in the price of all goods and services at the correct rate.</p>
<ul class="usa-list"><li><a href="/apply-adult-passport"><div class="usa-card__header"><h2>Apply for a new adult passport</h2></div>
<div class="usa-card__body"><p>You need a passport to travel to most countries outside the U.S.</p></div></a></li></ul>
<div class="gem-c-feedback"><h2>Help us improve GOV.UK</h2>
<form><p class="govuk-visually-hidden">This field is for robots only. Please leave blank</p>
<label>What were you doing?</label><textarea></textarea></form></div>
</main></body></html>"##;

#[test]
fn gov_guide_keeps_card_headings_and_loses_its_feedback_form() {
    let doc = page(GOV_GUIDE);
    for heading in ["Apply for a new adult passport", "In the UK"] {
        let found = block(&doc, heading);
        assert!(matches!(found.kind, BlockKind::Heading { .. }), "{:?}", found.kind);
    }
    assert!(has_text(&doc, "You need a passport to travel"));
    for clutter in [
        "Skip contents",
        "Help us improve",
        "robots only",
        "What were you doing",
        "titleContent",
    ] {
        assert!(!has_text(&doc, clutter), "{clutter:?} in {:#?}", texts(&doc));
    }
}

/// A page of chinhphu.vn: its heading in capitals out of the article, the
/// title again in the article, and the site, which holds the copyright, as
/// the author.
const GOV_VI: &str = r#"<!doctype html><html lang="vi"><head><title></title>
<meta property="og:title" content="Giới thiệu Cục Thông tin và Truyền thông Chính phủ">
<meta name="author" content="Cổng Thông tin điện tử Chính phủ">
<meta name="copyright" content="Cổng Thông tin điện tử Chính phủ - Cục Thông tin và Truyền thông Chính phủ">
</head><body><div class="category"><h1>GIỚI THIỆU CỤC THÔNG TIN VÀ TRUYỀN THÔNG CHÍNH PHỦ</h1></div>
<div class="content"><div class="title">Giới thiệu Cục Thông tin và Truyền thông Chính phủ</div>
<p>Cục Thông tin và Truyền thông Chính phủ là tổ chức hành chính trực thuộc Văn phòng Chính phủ, có chức năng tham mưu, giúp Bộ trưởng, Chủ nhiệm Văn phòng Chính phủ quản lý nhà nước về công báo.</p>
<p>Tên giao dịch quốc tế bằng tiếng Anh là Department of Government Information and Communications.</p></div>
</body></html>"#;

#[test]
fn gov_page_title_is_said_once_and_the_site_is_no_author() {
    let doc = page(GOV_VI);
    let title = "Giới thiệu Cục Thông tin và Truyền thông Chính phủ";
    assert_eq!(doc.meta.title.as_deref(), Some(title));
    assert_eq!(
        texts(&doc)
            .iter()
            .filter(|text| text.to_lowercase() == title.to_lowercase())
            .count(),
        1,
        "{:#?}",
        texts(&doc)
    );
    assert!(has_text(&doc, "Tên giao dịch quốc tế"));
    assert!(!has_text(&doc, "Cổng Thông tin điện tử"), "{:#?}", texts(&doc));
}
