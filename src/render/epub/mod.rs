//! EPUB output: the document as an EPUB 3 book (`.epub`), built from the
//! same [`Document`] as the HTML modes and with the same flowing markup as
//! the friendly mode, written as XHTML. The book has one content document
//! per top-level section, a table of contents (navigation document, and an
//! NCX for EPUB 2 readers), formulas as MathML where their structure is
//! known (pictures elsewhere), rebuilt tables as tables, figures as
//! pictures of their area of the page, and the first page as its cover.
//!
//! The style sheet sizes everything relative to the text, so that the
//! reader's font, size and theme apply.

mod package;
mod xhtml;

use super::notes::{NOTES_CSS, epub_asides, has_notes, number_blocks};
use super::zip::{Compression, ZipWriter};
use super::{
    Equations, FlowCtx, HtmlOptions, ImageStore, Mode, PageImage, WidthUnit, arxiv_abs_link, authors_html, doc_title,
    is_abstract_text, plain, render_flow, side_figures,
};
use crate::analysis::pipeline::Analysis;
use crate::backend::PdfFile;
use crate::common::geom::Rect;
use crate::model::{Block, BlockKind, Document, ImageSpec, SUP};
use crate::render::Pictures;
use package::{BookInfo, CONTAINER, CSS_FILE, ContentDoc, NAV_FILE, NavEntry, nav_xhtml, package_opf, toc_ncx};
use std::collections::HashMap;

/// The style sheet of the book.
const CSS: &str = include_str!("epub.css");
/// Separates the content documents (and the cover) in the text handed to
/// the picture store; a control character never written to the HTML.
const SPLIT: char = '\u{2}';
/// Directory of the pictures inside `EPUB/`.
const IMAGE_DIR: &str = "images";
/// `dcterms:modified` when [`EpubOptions::modified`] is not set: a fixed
/// date, so that the same paper always gives the same bytes.
const FIXED_MODIFIED: &str = "2000-01-01T00:00:00Z";
/// The word before the author's name on the title page of a book ("By Jane
/// Austen"), left out of the author recorded in the book.
const BYLINE: &str = "by ";
/// Most words in a line of author names (a line may name several people:
/// "Kaiming He Xiangyu Zhang Shaoqing Ren Jian Sun").
const NAME_MAX_WORDS_COUNT: usize = 12;
/// Lowercase words inside names ("van", "de la", "bin").
const NAME_PARTICLES: &[&str] = &[
    "van", "von", "der", "den", "de", "del", "della", "di", "da", "du", "la", "le", "bin", "binti", "y", "and", "&",
    "et", "al.",
];
/// Prefix of the anchors given to the headings that have none, so that the
/// table of contents can point to them.
const HEADING_ANCHOR_PREFIX: &str = "toc-";

/// Everything that controls how a document is written as an EPUB book.
#[derive(Debug, Clone)]
pub struct EpubOptions {
    /// Pixels per PDF point of the pictures of figures, formulas and the
    /// cover.
    pub scale: f32,
    /// Worker threads for drawing the pictures (0 = one per CPU).
    pub jobs: usize,
    /// How displayed formulas are written: [`Equations::MathMl`] gives
    /// MathML where the structure of the formula is clear (and inside the
    /// text), pictures elsewhere.
    pub equations: Equations,
    /// Widest displayed formula written as MathML, in multiples of the font
    /// size of the text (em). A MathML formula cannot break over lines, and
    /// many readers lay out pages that do not scroll sideways, so a wider
    /// formula would be cut at the edge of the screen: it is a picture
    /// instead, which shrinks to the width of the page. The default, 20 em,
    /// is about the width of a phone screen.
    pub max_math_width_em: f32,
    /// Draw the first page as the cover picture, which readers show in
    /// their library.
    pub cover: bool,
    /// Date of last modification recorded in the book, as
    /// `CCYY-MM-DDThh:mm:ssZ` (UTC). `None` records 1 January 2000, so that
    /// the same paper always gives the same file.
    pub modified: Option<String>,
}

impl Default for EpubOptions {
    /// Pictures at 3 pixels per point, formulas as MathML where their
    /// structure is clear and they are at most 20 em wide, the first page as the cover, a fixed date and
    /// one worker thread per CPU.
    fn default() -> Self {
        Self {
            scale: 3.0,
            jobs: 0,
            equations: Equations::MathMl,
            max_math_width_em: 20.0,
            cover: true,
            modified: None,
        }
    }
}

/// Writes an analysed document as an EPUB book and returns the bytes of
/// the file.
///
/// The text is written first, with placeholders for the pictures; the
/// pictures are then drawn from `pdf` (on `opts.jobs` threads) and stored
/// in the book.
pub fn render_epub(analysis: &Analysis, pdf: Option<&PdfFile>, opts: &EpubOptions) -> Vec<u8> {
    let doc = &analysis.doc;
    let html_opts = HtmlOptions {
        mode: Mode::Friendly,
        inline_images: false,
        asset_dir: IMAGE_DIR.into(),
        scale: opts.scale,
        page_image: PageImage::Svg,
        jobs: opts.jobs,
        embed_fonts: false,
        equations: opts.equations,
    };
    let mut images = ImageStore {
        opts: &html_opts,
        jobs: Vec::new(),
    };
    let blocks = book_blocks(doc, opts.max_math_width_em);
    let chapters = split_chapters(&blocks);
    let all: Vec<&Block> = blocks.iter().collect();
    let ctx = FlowCtx {
        unit: WidthUnit::EmFit {
            pt_per_em: doc.body_size.max(1.0),
        },
        offprint: true,
        covers: (None, None),
        sides: side_figures(doc, &all),
        math: opts.equations == Equations::MathMl,
        note_asides: false,
    };
    let mut text = String::new();
    for (i, chapter) in chapters.iter().enumerate() {
        if i > 0 {
            text.push(SPLIT);
        }
        // The added notes are numbered from 1 in each chapter, and
        // set at its end as EPUB footnotes.
        if chapter.iter().any(|block| has_notes(block)) {
            let mut numbered: Vec<Block> = chapter.iter().map(|block| (*block).clone()).collect();
            number_blocks(&mut numbered, &format!("c{}-", i + 1));
            let numbered: Vec<&Block> = numbered.iter().collect();
            let html = chapter_html(doc, &numbered, &mut images, &ctx)
                .replace("<a class=\"an-call\"", "<a epub:type=\"noteref\" class=\"an-call\"");
            text.push_str(&html);
            text.push_str(&epub_asides(&numbered, ctx.math));
        } else {
            text.push_str(&chapter_html(doc, chapter, &mut images, &ctx));
        }
    }
    let cover = (opts.cover && !doc.pages.is_empty()).then(|| {
        text.push(SPLIT);
        text.push_str(&images.region(&cover_spec(doc), "cover"));
    });
    let out = images.finish(Pictures::new(pdf, &analysis.doc), text);
    let mut parts: Vec<&str> = out.html.split(SPLIT).collect();
    let cover_file = cover.and_then(|()| parts.pop()).filter(|src| !src.is_empty());
    let bodies: Vec<String> = parts.iter().map(|part| xhtml::from_html(part)).collect();
    let image_files: Vec<String> = out.files.iter().map(|(name, _)| name.clone()).collect();
    let book = Book {
        doc,
        chapters: &chapters,
        bodies,
        cover: cover_file,
        image_files: &image_files,
        modified: opts.modified.as_deref().unwrap_or(FIXED_MODIFIED),
    };
    book.package(&out.files)
}

/// Everything written into the book once the pictures are drawn.
struct Book<'a> {
    /// The document.
    doc: &'a Document,
    /// The blocks of each content document.
    chapters: &'a [Vec<&'a Block>],
    /// The XHTML body of each content document.
    bodies: Vec<String>,
    /// The picture file of the cover (inside `EPUB/`), if any.
    cover: Option<&'a str>,
    /// The picture files (inside `EPUB/`), in order.
    image_files: &'a [String],
    /// `dcterms:modified`.
    modified: &'a str,
}

impl Book<'_> {
    /// The EPUB file: the `mimetype` entry first and stored, as the format
    /// requires, then the container, the package, the navigation, the
    /// content documents and the pictures (`files`).
    fn package(&self, files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let doc = self.doc;
        let title = doc_title(doc);
        let identifier = identifier(doc);
        let authors = authors(doc);
        let source = doc
            .meta
            .arxiv_id
            .as_ref()
            .map(|id| format!("https://arxiv.org/abs/{id}"));
        let info = BookInfo {
            identifier: &identifier,
            title: &title,
            authors: &authors,
            source: source.as_deref(),
            modified: self.modified,
        };
        let mut place = HashMap::new();
        for (i, body) in self.bodies.iter().enumerate() {
            for id in xhtml::ids(body) {
                place.entry(id).or_insert(i);
            }
        }
        let entries = self.nav_entries(&place);
        let mut zip = ZipWriter::default();
        zip.add("mimetype", b"application/epub+zip", Compression::Stored);
        zip.add("META-INF/container.xml", CONTAINER.as_bytes(), Compression::Deflated);
        let mut docs = Vec::new();
        for (i, body) in self.bodies.iter().enumerate() {
            let body = xhtml::relink(body, i, &place, file_name);
            let doc_title = self.chapters[i]
                .iter()
                .find(|block| matches!(block.kind, BlockKind::Heading { .. }))
                .map(|block| plain(&block.spans))
                .unwrap_or_else(|| title.clone());
            let page = content_xhtml(&doc_title, &body);
            zip.add(
                &format!("EPUB/{}", file_name(i)),
                page.as_bytes(),
                Compression::Deflated,
            );
            docs.push(ContentDoc {
                file: file_name(i),
                mathml: body.contains("<math "),
            });
        }
        let opf = package_opf(&info, &docs, self.image_files, self.cover);
        zip.add("EPUB/package.opf", opf.as_bytes(), Compression::Deflated);
        zip.add(
            &format!("EPUB/{NAV_FILE}"),
            nav_xhtml(&title, &entries).as_bytes(),
            Compression::Deflated,
        );
        zip.add(
            "EPUB/toc.ncx",
            toc_ncx(&info, &entries).as_bytes(),
            Compression::Deflated,
        );
        let css = if self.bodies.iter().any(|body| body.contains("<aside class=\"an\"")) {
            format!("{CSS}{NOTES_CSS}")
        } else {
            CSS.to_string()
        };
        zip.add(&format!("EPUB/{CSS_FILE}"), css.as_bytes(), Compression::Deflated);
        for (name, data) in files {
            zip.add(&format!("EPUB/{name}"), data, Compression::Stored);
        }
        zip.finish()
    }

    /// The table of contents: the title page, then every heading whose
    /// anchor is in a content document (`place`), nested by level.
    fn nav_entries(&self, place: &HashMap<String, usize>) -> Vec<NavEntry> {
        let mut entries = Vec::new();
        let starts_with_heading = self
            .chapters
            .first()
            .and_then(|chapter| chapter.first())
            .is_some_and(|block| matches!(block.kind, BlockKind::Heading { .. }));
        if !starts_with_heading {
            entries.push(NavEntry {
                depth: 0,
                label: doc_title(self.doc),
                href: file_name(0),
            });
        }
        let headings = self.chapters.iter().flatten().filter_map(|block| match block.kind {
            BlockKind::Heading { level } => Some((level, *block)),
            _ => None,
        });
        let top = headings.clone().map(|(level, _)| level).min().unwrap_or(1);
        for (level, block) in headings {
            let label = plain(&block.spans);
            let Some(anchor) = block.anchor.as_ref().filter(|_| !label.is_empty()) else {
                continue;
            };
            let Some(&doc) = place.get(&super::esc(anchor)) else {
                continue;
            };
            // A level may be skipped (a section straight to a
            // subsubsection): the entry goes one deeper than the last.
            let max_depth = entries.last().map_or(0, |entry: &NavEntry| entry.depth + 1);
            entries.push(NavEntry {
                depth: usize::from(level - top).min(max_depth),
                label,
                href: format!("{}#{anchor}", file_name(doc)),
            });
        }
        entries
    }
}

/// The file name of content document number `index`, inside `EPUB/`.
fn file_name(index: usize) -> String {
    format!("c{:03}.xhtml", index + 1)
}

/// A whole content document around the XHTML `body`.
fn content_xhtml(title: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n\
<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\" lang=\"en\" xml:lang=\"en\">\n\
<head>\n<meta charset=\"utf-8\"/>\n<title>{}</title>\n<link rel=\"stylesheet\" type=\"text/css\" href=\"{CSS_FILE}\"/>\n</head>\n\
<body>\n{body}</body>\n</html>\n",
        super::esc(title)
    )
}

/// The blocks of the document as the book shows them: an anchor on every
/// heading that has none, so that the table of contents can point to each
/// heading, and no MathML for the displayed formulas wider than
/// `max_math_width_em`, which are pictures instead.
fn book_blocks(doc: &Document, max_math_width_em: f32) -> Vec<Block> {
    doc.blocks
        .iter()
        .map(|block| {
            let mut block = block.clone();
            if matches!(block.kind, BlockKind::Heading { .. }) && block.anchor.is_none() {
                block.anchor = Some(format!("{HEADING_ANCHOR_PREFIX}{}", block.id));
            }
            if let BlockKind::Equation { image: Some(image), .. } = &block.kind
                && image.rect.width() > max_math_width_em * doc.body_size
            {
                block.math = None;
            }
            block
        })
        .collect()
}

/// The blocks of each content document: a new document starts at every
/// heading of the outermost level (other than an "Abstract" heading). What
/// comes before the first one (title, authors, abstract) is the title
/// page.
fn split_chapters(blocks: &[Block]) -> Vec<Vec<&Block>> {
    let starts = |block: &Block| match block.kind {
        BlockKind::Heading { level } => (!is_abstract_text(&block.spans)).then_some(level),
        _ => None,
    };
    let top = blocks.iter().filter_map(starts).min();
    let mut chapters: Vec<Vec<&Block>> = vec![Vec::new()];
    for block in blocks {
        let current = chapters.last_mut().expect("at least one chapter");
        if top.is_some() && starts(block) == top && !current.is_empty() {
            chapters.push(vec![block]);
        } else {
            current.push(block);
        }
    }
    chapters
}

/// The HTML of the blocks of one content document. Runs of title and
/// author blocks get their own markup (with a link to the arXiv page after
/// the title), everything else flows.
fn chapter_html(doc: &Document, blocks: &[&Block], images: &mut ImageStore<'_>, ctx: &FlowCtx) -> String {
    /// The kind of run a block belongs to: 0 title, 1 authors, 2 the rest.
    fn run_kind(block: &Block) -> u8 {
        match block.kind {
            BlockKind::Title => 0,
            BlockKind::Authors => 1,
            _ => 2,
        }
    }
    let mut out = String::new();
    for run in blocks.chunk_by(|a, b| run_kind(a) == run_kind(b)) {
        match run_kind(run[0]) {
            0 => {
                let parts: Vec<String> = run.iter().map(|block| super::spans_html(&block.spans)).collect();
                out.push_str(&format!("<h1 class=\"title\">{}</h1>\n", parts.join(" ")));
                let link = arxiv_abs_link(doc);
                if !link.is_empty() {
                    out.push_str(&format!("<p class=\"source\">{link}</p>\n"));
                }
            }
            1 => out.push_str(&authors_html(run)),
            _ => out.push_str(&render_flow(run, images, ctx)),
        }
    }
    out
}

/// The whole first page, drawn as the cover.
fn cover_spec(doc: &Document) -> ImageSpec {
    let page = doc.pages[0];
    let rect = Rect {
        x0: 0.0,
        y0: 0.0,
        x1: page.width,
        y1: page.height,
    };
    ImageSpec::region(0, rect, false)
}

/// The authors of the paper: the first line of each author group, without
/// its footnote marks and without a leading "By".
fn authors(doc: &Document) -> Vec<String> {
    doc.blocks
        .iter()
        .filter(|block| matches!(block.kind, BlockKind::Authors))
        .filter_map(|block| block.rows.first())
        .map(|row| {
            let spans: Vec<_> = row.iter().filter(|span| span.style & SUP == 0).cloned().collect();
            let name = plain(&spans);
            let name = name.trim_end_matches([',', '*', '†', '‡']).trim();
            // "By Jane Austen" on a title page.
            match name.get(..BYLINE.len()) {
                Some(head) if head.eq_ignore_ascii_case(BYLINE) => name[BYLINE.len()..].trim().to_string(),
                _ => name.to_string(),
            }
        })
        .filter(|name| looks_like_name(name))
        .collect()
}

/// Whether `text` reads as the names of people or of a group: a few words
/// with capitals, no digits and no address ("Jane Austen", "Aidan N.
/// Gomez", "Ludwig van Beethoven"). The lines under the title of a book's
/// cover ("How to Think Like a Computer Scientist", "2nd Edition") are
/// read as author groups by the analysis but are none.
fn looks_like_name(text: &str) -> bool {
    let words: Vec<&str> = text.split_whitespace().collect();
    !words.is_empty()
        && words.len() <= NAME_MAX_WORDS_COUNT
        && !text.contains(|ch: char| ch.is_ascii_digit() || ch == '@' || ch == '/')
        && words.iter().all(|word| {
            word.starts_with(|ch: char| !ch.is_lowercase()) || NAME_PARTICLES.contains(&word.trim_end_matches(','))
        })
}

/// The unique identifier of the book: a UUID made from the title, the
/// arXiv id and the number of pages, so that the same paper always gets the
/// same one.
fn identifier(doc: &Document) -> String {
    let key = format!(
        "{}\n{}\n{}",
        doc_title(doc),
        doc.meta.arxiv_id.as_deref().unwrap_or_default(),
        doc.pages.len()
    );
    let words: Vec<u32> = (0u8..4)
        .map(|salt| {
            let mut hasher = crc32fast::Hasher::new();
            hasher.update(&[salt]);
            hasher.update(key.as_bytes());
            hasher.finalize()
        })
        .collect();
    // Version 4 and the RFC 4122 variant, as a random UUID would have.
    let (time_hi, clock) = ((words[1] & 0x0fff) | 0x4000, ((words[2] >> 16) & 0x3fff) | 0x8000);
    format!(
        "urn:uuid:{:08x}-{:04x}-{time_hi:04x}-{clock:04x}-{:04x}{:08x}",
        words[0],
        words[1] >> 16,
        words[2] & 0xffff,
        words[3]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byline_is_not_part_of_the_author() {
        let block = |text: &str| Block {
            kind: BlockKind::Authors,
            rows: vec![vec![crate::model::Span::plain(text)]],
            ..empty_block()
        };
        let doc = Document {
            blocks: vec![block("By Jane Austen"), block("Bysshe Shelley"), block("Ada Lovelace*")],
            ..empty_doc()
        };
        assert_eq!(authors(&doc), ["Jane Austen", "Bysshe Shelley", "Ada Lovelace"]);
        for not_a_name in [
            "How to Think Like a Computer Scientist",
            "2nd Edition, Version 2.4.1",
            "The third edition of Think Python is available at thinkpython.com",
        ] {
            assert!(!looks_like_name(not_a_name), "{not_a_name}");
        }
        for name in [
            "Ludwig van Beethoven",
            "Aidan N. Gomez",
            "Łukasz Kaiser",
            "LIGO Scientific Collaboration",
            "B. P. Abbott et al.",
            "Shaoqing Ren, Kaiming He, Ross Girshick, and Jian Sun",
            "Kaiming He Xiangyu Zhang Shaoqing Ren Jian Sun",
        ] {
            assert!(looks_like_name(name), "{name}");
        }
    }

    /// A document without pages or blocks.
    fn empty_doc() -> Document {
        Document {
            meta: Default::default(),
            pages: Vec::new(),
            body_size: 10.0,
            leading: 12.0,
            body_font: String::new(),
            math_font: String::new(),
            blocks: Vec::new(),
            pictures: Vec::new(),
        }
    }

    /// A paragraph block with no text.
    fn empty_block() -> Block {
        Block::new(0, BlockKind::Paragraph { in_abstract: false })
    }

    #[test]
    fn identifier_is_a_uuid() {
        let doc = empty_doc();
        let id = identifier(&doc);
        let uuid = id.strip_prefix("urn:uuid:").unwrap();
        let groups: Vec<usize> = uuid.split('-').map(str::len).collect();
        assert_eq!(groups, [8, 4, 4, 4, 12]);
        assert_eq!(&uuid[14..15], "4");
        assert_eq!(identifier(&doc), id);
    }
}
