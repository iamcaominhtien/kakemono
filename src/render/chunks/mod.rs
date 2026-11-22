//! Chunks output: the text of the document cut into pieces ready to embed
//! for retrieval (RAG), one JSON object per line (`.jsonl`).
//!
//! Chunks follow the structure of the document: blocks are grouped by
//! section, a chunk never holds two sections, and a block is cut only when
//! it does not fit in one chunk (at sentence ends, else between words; a
//! table between rows, with its caption and header repeated).
//! Each chunk starts with its section path ("3 Model > 3.2 Attention"), so
//! that a piece of text keeps the context a reader has. The paper title is
//! not repeated in the text: retrieval within one paper gains nothing from
//! it, and measured on Qasper (100 papers, two embedding models) a title on
//! every chunk lost 3 to 5 points of evidence recall. It is in each chunk's
//! metadata instead, to filter or prefix when searching several papers.
//!
//! Figures and tables give their caption and, for a rebuilt table, its rows
//! (one field per column, joined by ` | `, empty where no cell starts);
//! formulas are written as `$$LaTeX$$` when their structure is known.
//! Pictures are left out.

mod tokens;

pub use tokens::{NAMED_TOKENIZERS, TokenCounter, estimate};

use super::json::plain_text::{block_plain, plain_of};
use super::plain;
use crate::model::{Block, BlockKind, Document, Table};
use serde::Serialize;

/// Separator of the parts of a section path in a chunk's text.
const PATH_SEPARATOR: &str = " > ";

/// Separator between the section path and the text, and between blocks.
const BLOCK_SEPARATOR: &str = "\n\n";

/// Tokens kept free in a chunk for the break before a piece: counts are
/// not quite additive (the estimate counts the space before a number, real
/// tokenizers merge across the break).
const JOIN_SLACK_TOKENS: usize = 1;

/// Separator between the cells of a table row.
const CELL_SEPARATOR: &str = " | ";

/// Everything that controls how a document is cut into chunks.
#[derive(Debug, Clone)]
pub struct ChunkOptions {
    /// Largest size of a chunk, in tokens, section path included.
    pub max_tokens: usize,
    /// How tokens are counted: an estimate by default (see
    /// [`TokenCounter`]).
    pub counter: TokenCounter,
}

impl Default for ChunkOptions {
    /// At most 512 tokens, the input limit of most embedding models,
    /// counted by the estimate.
    fn default() -> Self {
        Self {
            max_tokens: 512,
            counter: TokenCounter::estimate(),
        }
    }
}

/// One chunk: its text and where it comes from.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Chunk {
    /// Number of the chunk, in reading order, from 0.
    pub id: usize,
    /// The text to embed: the section path, a blank line, then the text of
    /// the blocks, separated by blank lines.
    pub text: String,
    /// Size of `text` in tokens, as [`ChunkOptions::counter`] counts them.
    pub tokens: usize,
    /// Title of the document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Headings of the sections the chunk is in, outermost first; empty
    /// before the first heading.
    pub section: Vec<String>,
    /// Ids of the blocks the text comes from (the `id` of the JSON output).
    pub blocks: Vec<usize>,
    /// Pages the text is on, from 0.
    pub pages: Vec<usize>,
}

/// Cuts a document into chunks.
pub fn render_chunks(doc: &Document, opts: &ChunkOptions) -> Vec<Chunk> {
    let mut builder = Builder::new(opts, doc.meta.title.clone());
    // The headings open above the current block: (level, text).
    let mut headings: Vec<(u8, String)> = Vec::new();
    for block in &doc.blocks {
        if let BlockKind::Heading { level } = block.kind {
            builder.flush();
            while headings.last().is_some_and(|&(open, _)| open >= level) {
                headings.pop();
            }
            headings.push((level, plain_of(&block.spans)));
            continue;
        }
        let text = block_text(block);
        if text.is_empty() {
            continue;
        }
        builder.enter(headings.iter().map(|(_, heading)| heading.clone()).collect());
        builder.add_block(block, &text);
    }
    builder.flush();
    builder.chunks
}

/// The chunks as JSON Lines: one object per line, each line ending with a
/// line break.
pub fn chunks_jsonl(chunks: &[Chunk]) -> String {
    let mut out = String::new();
    for chunk in chunks {
        out.push_str(&serde_json::to_string(chunk).expect("a chunk serialises"));
        out.push('\n');
    }
    out
}

/// The text of one block as a reader (or a language model) reads it; empty
/// for a block without text.
fn block_text(block: &Block) -> String {
    let BlockKind::Figure { .. } = block.kind else {
        return block_plain(block).unwrap_or_default().trim().to_string();
    };
    let mut parts = vec![plain_of(&block.spans)];
    match &block.table {
        Some(table) => parts.extend(table_rows(table)),
        None => parts.extend(block.rows.first().map(|row| plain(row))),
    }
    parts.retain(|part| !part.trim().is_empty());
    parts.join("\n").trim().to_string()
}

/// The rows of `table` as text, one string per row: one field per column
/// of the table, joined by [`CELL_SEPARATOR`], so that a field is under
/// its header in every row. A cell's text is at its first column; the
/// other columns it spans, those covered by a cell from a row above, and
/// those without a cell are empty fields. The line is then trimmed: a row
/// whose last fields are empty ends with `" |"`, but every row has as many
/// `|` as the table has columns less one.
fn table_rows(table: &Table) -> Vec<String> {
    let n_cols = table.n_cols.max(1);
    table
        .rows
        .iter()
        .map(|row| {
            let mut fields = vec![String::new(); n_cols];
            for cell in row.iter().filter(|cell| cell.col < n_cols) {
                fields[cell.col] = plain_of(&cell.spans);
            }
            fields.join(CELL_SEPARATOR).trim().to_string()
        })
        .collect()
}

/// Cuts the text of a rebuilt table into pieces of at most `room` tokens,
/// between rows, each piece starting with the caption and the header rows
/// (dropped when they take more than half of `room`). A row that does not
/// fit even alone is cut between words, the head repeated on each part; a
/// part with nothing but the separators of empty cells is left out.
/// `None` when `block` has no rebuilt table, or no row to cut at.
fn table_pieces(block: &Block, room: usize, counter: &TokenCounter) -> Option<Vec<String>> {
    let table = block.table.as_ref()?;
    let rows = table_rows(table);
    let (header, body) = rows.split_at(table.header_rows.min(rows.len()));
    let mut head_lines = vec![plain_of(&block.spans)];
    head_lines.extend(header.iter().cloned());
    head_lines.retain(|line| !line.trim().is_empty());
    let mut head = head_lines.join("\n");
    if counter.count(&head) * 2 > room {
        head.clear();
    }
    let with_head = |text: &str| {
        if head.is_empty() {
            text.to_string()
        } else {
            format!("{head}\n{text}")
        }
    };
    let mut pieces = Vec::new();
    // The body rows of the piece being built.
    let mut current: Vec<&str> = Vec::new();
    for row in body.iter().filter(|row| !row.trim().is_empty()) {
        current.push(row);
        if counter.count(&with_head(&current.join("\n"))) <= room {
            continue;
        }
        current.pop();
        if !current.is_empty() {
            pieces.push(with_head(&current.join("\n")));
            current.clear();
        }
        if counter.count(&with_head(row)) <= room {
            current.push(row);
            continue;
        }
        let budget = room.saturating_sub(counter.count(&format!("{head}\n"))).max(1);
        // A part made only of the separators of empty cells says nothing.
        pieces.extend(
            split_long(row, budget, counter)
                .iter()
                .filter(|part| part.chars().any(|ch| !(ch.is_whitespace() || ch == '|')))
                .map(|part| with_head(part)),
        );
    }
    if !current.is_empty() {
        pieces.push(with_head(&current.join("\n")));
    }
    (!pieces.is_empty()).then_some(pieces)
}

/// Builds the chunks one block at a time.
struct Builder<'a> {
    /// The options.
    opts: &'a ChunkOptions,
    /// Title of the document, copied into each chunk.
    title: Option<String>,
    /// The chunks written so far.
    chunks: Vec<Chunk>,
    /// Headings of the current section, outermost first.
    section: Vec<String>,
    /// The section path at the start of the current chunk's text (empty
    /// when it is left out).
    head: String,
    /// The texts of the current chunk.
    body: Vec<String>,
    /// Ids of the blocks of the current chunk.
    blocks: Vec<usize>,
    /// Pages of the current chunk.
    pages: Vec<usize>,
}

impl<'a> Builder<'a> {
    /// An empty builder.
    fn new(opts: &'a ChunkOptions, title: Option<String>) -> Self {
        Self {
            opts,
            title,
            chunks: Vec::new(),
            section: Vec::new(),
            head: String::new(),
            body: Vec::new(),
            blocks: Vec::new(),
            pages: Vec::new(),
        }
    }

    /// Moves to the section `section`: a new section starts a new chunk.
    /// The section path heads the chunks of the section, unless it takes
    /// more than half of a chunk.
    fn enter(&mut self, section: Vec<String>) {
        if section == self.section {
            return;
        }
        self.flush();
        let head = section.join(PATH_SEPARATOR);
        self.head = if self.count(&head) * 2 > self.opts.max_tokens {
            String::new()
        } else {
            head
        };
        self.section = section;
    }

    /// Adds the text of `block` to the chunks, cut into pieces when it does
    /// not fit in one.
    fn add_block(&mut self, block: &Block, text: &str) {
        // What a chunk holds besides its section path, less the token that
        // the break before a piece can cost (before a number).
        let room = self
            .opts
            .max_tokens
            .saturating_sub(self.head_tokens() + JOIN_SLACK_TOKENS)
            .max(1);
        let pieces = if self.count(text) > room {
            // A table too long for a chunk is cut between rows.
            table_pieces(block, room, &self.opts.counter).unwrap_or_else(|| split_long(text, room, &self.opts.counter))
        } else {
            vec![text.to_string()]
        };
        for piece in pieces {
            if !self.body.is_empty() && self.count(&self.text_with(&[&piece])) > self.opts.max_tokens {
                self.flush();
            }
            self.body.push(piece);
            if self.blocks.last() != Some(&block.id) {
                self.blocks.push(block.id);
            }
            if !self.pages.contains(&block.page) {
                self.pages.push(block.page);
            }
        }
    }

    /// Ends the current chunk, if it has text.
    fn flush(&mut self) {
        if self.body.is_empty() {
            return;
        }
        let text = self.text_with(&[]);
        self.pages.sort_unstable();
        self.chunks.push(Chunk {
            id: self.chunks.len(),
            tokens: self.count(&text),
            text,
            title: self.title.clone(),
            section: self.section.clone(),
            blocks: std::mem::take(&mut self.blocks),
            pages: std::mem::take(&mut self.pages),
        });
        self.body.clear();
    }

    /// The text of the current chunk with `more` pieces added: the section
    /// path, then the pieces.
    fn text_with(&self, more: &[&str]) -> String {
        let body = self.body.iter().map(String::as_str).chain(more.iter().copied());
        std::iter::once(self.head.as_str())
            .filter(|head| !head.is_empty())
            .chain(body)
            .collect::<Vec<_>>()
            .join(BLOCK_SEPARATOR)
    }

    /// Tokens taken by the section path at the start of a chunk.
    fn head_tokens(&self) -> usize {
        if self.head.is_empty() {
            0
        } else {
            self.count(&format!("{}{BLOCK_SEPARATOR}", self.head))
        }
    }

    /// The number of tokens of `text`.
    fn count(&self, text: &str) -> usize {
        self.opts.counter.count(text)
    }
}

/// Cuts `text` into pieces of at most `budget` tokens: whole sentences
/// where they fit, a sentence longer than the budget between words, and a
/// word longer than the budget between characters.
fn split_long(text: &str, budget: usize, counter: &TokenCounter) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    for sentence in sentences(text) {
        let candidate = if current.is_empty() {
            sentence.to_string()
        } else {
            format!("{current} {sentence}")
        };
        if counter.count(&candidate) <= budget {
            current = candidate;
            continue;
        }
        if !current.is_empty() {
            pieces.push(std::mem::take(&mut current));
        }
        let mut rest = sentence;
        while counter.count(rest) > budget {
            let (head, tail) = cut(rest, budget, counter);
            pieces.push(head.to_string());
            rest = tail;
        }
        current = rest.to_string();
    }
    if !current.is_empty() {
        pieces.push(current);
    }
    pieces
}

/// The sentences of `text`: it is cut at white space that follows `.`, `!`
/// or `?` and comes before a capital, a digit or an opening bracket, so
/// that "e.g. the" and "Fig. 2a" are not cut while "end. The" is (a cut
/// after an abbreviation before a capital is harmless).
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch.is_whitespace() && prev.is_some_and(|last| matches!(last, '.' | '!' | '?')) {
            let mut end_ws = i + ch.len_utf8();
            while let Some(&(j, next)) = chars.peek() {
                if !next.is_whitespace() {
                    break;
                }
                end_ws = j + next.len_utf8();
                chars.next();
            }
            let next = text[end_ws..].chars().next();
            if next
                .is_some_and(|first| first.is_ascii_uppercase() || first.is_ascii_digit() || matches!(first, '(' | '['))
            {
                out.push(&text[start..i]);
                start = end_ws;
            }
            prev = Some(' ');
            continue;
        }
        prev = Some(ch);
    }
    out.push(&text[start..]);
    out.retain(|sentence| !sentence.trim().is_empty());
    out
}

/// Cuts `text` (more than `budget` tokens) into its longest head of at most
/// `budget` tokens and the rest: between words, or between characters when
/// the first word alone is too long. The head is never empty.
fn cut<'t>(text: &'t str, budget: usize, counter: &TokenCounter) -> (&'t str, &'t str) {
    // Ends of the candidate heads: after each word, else after each char.
    let mut ends: Vec<usize> = text
        .char_indices()
        .filter(|&(i, ch)| ch.is_whitespace() && i > 0 && !text[..i].ends_with(char::is_whitespace))
        .map(|(i, _)| i)
        .collect();
    if ends.first().is_none_or(|&end| counter.count(&text[..end]) > budget) {
        ends = text.char_indices().skip(1).map(|(i, _)| i).collect();
    }
    // The count grows with the head: find the last end that fits.
    let fits = ends.partition_point(|&end| counter.count(&text[..end]) <= budget);
    let end = if fits == 0 {
        ends.first().copied().unwrap_or(text.len())
    } else {
        ends[fits - 1]
    };
    (text[..end].trim_end(), text[end..].trim_start())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Span;

    /// A block of `kind` with `text`, on page 0.
    fn block(id: usize, kind: BlockKind, text: &str) -> Block {
        let mut block = Block::new(id, kind);
        block.spans = vec![Span::plain(text)];
        block
    }

    /// A document titled "A Paper" with these blocks.
    fn doc(blocks: Vec<Block>) -> Document {
        Document {
            meta: crate::model::Meta {
                title: Some("A Paper".into()),
                ..Default::default()
            },
            pages: Vec::new(),
            body_size: 10.0,
            leading: 12.0,
            body_font: String::new(),
            math_font: String::new(),
            blocks,
            pictures: Vec::new(),
        }
    }

    #[test]
    fn chunks_follow_sections_and_carry_their_path() {
        let doc = doc(vec![
            block(0, BlockKind::Heading { level: 1 }, "1 Intro"),
            block(1, BlockKind::Paragraph { in_abstract: false }, "First."),
            block(2, BlockKind::Heading { level: 2 }, "1.1 More"),
            block(3, BlockKind::Paragraph { in_abstract: false }, "Second."),
        ]);
        let chunks = render_chunks(&doc, &ChunkOptions::default());
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].text, "1 Intro\n\nFirst.");
        assert_eq!(chunks[1].text, "1 Intro > 1.1 More\n\nSecond.");
        assert_eq!(chunks[1].section, ["1 Intro", "1.1 More"]);
        assert_eq!(chunks[1].blocks, [3]);
        assert_eq!(chunks[1].title.as_deref(), Some("A Paper"));
    }

    #[test]
    fn blocks_of_a_section_share_a_chunk_until_it_is_full() {
        let para = |id| block(id, BlockKind::Paragraph { in_abstract: false }, "one two three four");
        let doc = doc(vec![
            block(0, BlockKind::Heading { level: 1 }, "S"),
            para(1),
            para(2),
            para(3),
        ]);
        let opts = ChunkOptions {
            max_tokens: 9,
            ..Default::default()
        };
        // "S" + two paragraphs of 4 tokens = 9 tokens; the third goes on.
        let chunks = render_chunks(&doc, &opts);
        assert_eq!(
            chunks.iter().map(|chunk| chunk.blocks.clone()).collect::<Vec<_>>(),
            [vec![1, 2], vec![3]]
        );
        assert!(chunks.iter().all(|chunk| chunk.tokens <= 9));
    }

    #[test]
    fn a_long_block_is_cut_at_sentence_ends() {
        // Each word and each full stop is one token: two sentences make 6.
        let pieces = split_long("One two. Three four. Five six.", 6, &TokenCounter::estimate());
        assert_eq!(pieces, ["One two. Three four.", "Five six."]);
    }

    #[test]
    fn a_long_sentence_is_cut_between_words() {
        let pieces = split_long("a b c d e", 2, &TokenCounter::estimate());
        assert_eq!(pieces, ["a b", "c d", "e"]);
    }

    #[test]
    fn abbreviations_before_lower_case_do_not_end_sentences() {
        assert_eq!(
            sentences("See e.g. the table. Then go."),
            ["See e.g. the table.", "Then go."]
        );
    }

    /// A table float: caption `caption` (none when empty), `header_rows`
    /// header rows ["Year", "Price"] and `body` rows ["2000+i", "i.5"].
    fn table_block(caption: &str, header_rows: usize, body: usize) -> Block {
        use crate::model::{FloatKind, Table, TableCell, TableGeom};
        let cell = |text: &str, col: usize| TableCell {
            col,
            colspan: 1,
            rowspan: 1,
            spans: vec![Span::plain(text)],
        };
        let mut rows: Vec<Vec<TableCell>> = (0..header_rows)
            .map(|_| vec![cell("Year", 0), cell("Price", 1)])
            .collect();
        rows.extend((0..body).map(|i| vec![cell(&format!("{}", 2000 + i), 0), cell(&format!("{i}.5"), 1)]));
        let kind = BlockKind::Figure {
            kind: FloatKind::Table,
            image: None,
            label: None,
        };
        let mut block = block(0, kind, caption);
        block.table = Some(Table {
            n_cols: 2,
            header_rows,
            col_align: vec!["left", "right"],
            rows,
            rule_above: Vec::new(),
            rule_bottom: Vec::new(),
            rule_left: Vec::new(),
            rule_right: Vec::new(),
            geom: TableGeom::default(),
        });
        block
    }

    /// Options with chunks of at most `max_tokens`.
    fn opts(max_tokens: usize) -> ChunkOptions {
        ChunkOptions {
            max_tokens,
            ..Default::default()
        }
    }

    #[test]
    fn empty_and_spanned_cells_keep_the_columns_of_a_row() {
        use crate::model::TableCell;
        let cell = |col: usize, text: &str, colspan: usize, rowspan: usize| TableCell {
            col,
            colspan,
            rowspan,
            spans: vec![Span::plain(text)],
        };
        let mut block = table_block("", 1, 0);
        let table = block.table.as_mut().unwrap();
        table.n_cols = 5;
        table.rows = vec![
            ["STT", "Chỉ tiêu", "Đơn vị tính", "Cơ quan báo cáo", "Kỳ báo cáo"]
                .iter()
                .enumerate()
                .map(|(col, text)| cell(col, text, 1, 1))
                .collect(),
            // The unit and the period are empty: no cell is stored there.
            vec![
                cell(0, "I.", 1, 1),
                cell(1, "TỔNG SẢN PHẨM", 1, 1),
                cell(3, "BKHĐT", 1, 1),
            ],
            // A cell over two columns, and one over two rows.
            vec![
                cell(0, "1", 1, 2),
                cell(1, "GDP", 2, 1),
                cell(3, "TCTK", 1, 1),
                cell(4, "Năm", 1, 1),
            ],
            vec![
                cell(1, "Tốc độ", 1, 1),
                cell(2, "%", 1, 1),
                cell(3, "TCTK", 1, 1),
                cell(4, "Quý", 1, 1),
            ],
        ];
        let text = block_text(&block);
        assert_eq!(
            text.lines().collect::<Vec<_>>(),
            [
                "STT | Chỉ tiêu | Đơn vị tính | Cơ quan báo cáo | Kỳ báo cáo",
                "I. | TỔNG SẢN PHẨM |  | BKHĐT |",
                "1 | GDP |  | TCTK | Năm",
                "| Tốc độ | % | TCTK | Quý",
            ]
        );
    }

    #[test]
    fn a_long_row_cut_between_words_gives_no_piece_of_separators_only() {
        use crate::model::TableCell;
        let mut block = table_block("", 0, 0);
        let table = block.table.as_mut().unwrap();
        table.n_cols = 40;
        // One long cell, then 39 empty columns.
        table.rows = vec![vec![TableCell {
            col: 0,
            colspan: 1,
            rowspan: 1,
            spans: vec![Span::plain("word ".repeat(60))],
        }]];
        let chunks = render_chunks(&doc(vec![block]), &opts(32));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.text.chars().any(|ch| !(ch.is_whitespace() || ch == '|')),
                "a chunk of separators only: {:?}",
                chunk.text
            );
        }
    }

    #[test]
    fn a_long_table_is_cut_between_rows_and_repeats_its_header() {
        let chunks = render_chunks(&doc(vec![table_block("Table 1: Prices", 1, 200)]), &opts(64));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.text.contains("Table 1: Prices\nYear | Price\n"), "{}", chunk.text);
            assert!(chunk.tokens <= 64, "{}", chunk.text);
            assert!(
                chunk
                    .text
                    .lines()
                    .filter(|line| line.contains(" | "))
                    .all(|line| line.split(" | ").count() == 2)
            );
        }
        let rows: usize = chunks
            .iter()
            .map(|chunk| chunk.text.lines().filter(|line| line.starts_with('2')).count())
            .sum();
        assert_eq!(rows, 200);
    }

    #[test]
    fn a_table_without_header_is_still_cut_between_rows() {
        let chunks = render_chunks(&doc(vec![table_block("", 0, 200)]), &opts(64));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(
                chunk.text.lines().all(|line| line.split(" | ").count() == 2),
                "{}",
                chunk.text
            );
            assert!(!chunk.text.contains("Year"));
        }
        let rows: usize = chunks.iter().map(|chunk| chunk.text.lines().count()).sum();
        assert_eq!(rows, 200);
    }

    #[test]
    fn a_row_longer_than_a_chunk_is_cut_between_words() {
        let mut block = table_block("", 1, 1);
        let long = vec!["word"; 300].join(" ");
        block.table.as_mut().expect("a table").rows[1][1].spans = vec![Span::plain(&long)];
        let chunks = render_chunks(&doc(vec![block]), &opts(64));
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(chunk.text.starts_with("Year | Price\n"), "{}", chunk.text);
            assert!(chunk.tokens <= 64, "{}", chunk.text);
        }
        let words: usize = chunks
            .iter()
            .map(|chunk| chunk.text.split_whitespace().filter(|word| *word == "word").count())
            .sum();
        assert_eq!(words, 300);
    }

    #[test]
    fn a_table_that_fits_stays_whole_beside_its_paragraphs() {
        let para = block(1, BlockKind::Paragraph { in_abstract: false }, "Before.");
        let chunks = render_chunks(&doc(vec![para, table_block("Table 1", 1, 3)]), &ChunkOptions::default());
        assert_eq!(chunks.len(), 1);
        assert!(
            chunks[0]
                .text
                .ends_with("Table 1\nYear | Price\n2000 | 0.5\n2001 | 1.5\n2002 | 2.5")
        );
    }
}
