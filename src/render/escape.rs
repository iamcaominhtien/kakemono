//! HTML for text: escaping, styled spans, captions and small attributes.

use crate::model::*;

/// Extra bytes reserved when escaping a string, so that a few entities fit
/// without the output growing again.
const ESCAPE_SLACK_BYTES: usize = 8;

/// Maximum number of characters of the text inside a figure that are added
/// to its alt text (enough to identify it, short enough for screen readers).
const FIGURE_ALT_MAX_CHARS: usize = 240;

pub(crate) use crate::common::patterns::CAP_LABEL;

/// What the document was converted from, as the colophons say it: `PDF`,
/// `EPUB` for a book, or `Word` for another document without pages.
pub(crate) fn source_label(doc: &Document) -> &'static str {
    if doc.meta.source_format.as_deref() == Some("epub") {
        "EPUB"
    } else if doc.pages.is_empty() {
        "Word"
    } else {
        "PDF"
    }
}

/// Escapes `text` for use in HTML text and in double-quoted attribute
/// values (`&`, `<`, `>` and `"`).
pub(crate) fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + ESCAPE_SLACK_BYTES);
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            other => out.push(other),
        }
    }
    out
}

/// Styled spans as semantic HTML (`<code>`, `<strong>`, `<em>`, `<sup>`,
/// `<sub>`), with links: internal ones (`#anchor`) get the `xref` class,
/// external ones open in a new tab. The call of a numbered added
/// note is a raised link to the note.
pub(crate) fn spans_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for span in spans {
        if span.note.is_some() {
            out.push_str(&super::notes::call_html(span));
            continue;
        }
        let mut open = String::new();
        let mut close = String::new();
        let mut tag = |name: &str| {
            open.push_str(&format!("<{name}>"));
            close.insert_str(0, &format!("</{name}>"));
        };
        if span.style & MONO != 0 {
            tag("code");
        }
        if span.style & BOLD != 0 {
            tag("strong");
        }
        if span.style & ITALIC != 0 {
            tag("em");
        }
        if span.style & SUP != 0 {
            tag("sup");
        }
        if span.style & SUB != 0 {
            tag("sub");
        }
        // Spaces at the ends of inline code go outside its tags: they would
        // otherwise show inside its shaded box.
        let text = span.text.as_str();
        let body = if span.style & MONO == 0 {
            format!("{open}{}{close}", esc(text).replace('\n', "<br>"))
        } else {
            let inner = text.trim_matches(' ');
            let lead = &text[..text.len() - text.trim_start_matches(' ').len()];
            let trail = &text[text.trim_end_matches(' ').len()..];
            if inner.is_empty() {
                esc(text)
            } else {
                format!("{lead}{open}{}{close}{trail}", esc(inner))
            }
        };
        match &span.href {
            Some(href) if href.starts_with('#') => {
                out.push_str(&format!("<a class=\"xref\" href=\"{}\">{body}</a>", esc(href)))
            }
            // A link whose text is an address may break anywhere; one on
            // words breaks between them.
            Some(href) => out.push_str(&format!(
                "<a{} href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{body}</a>",
                if looks_like_address(text) { " class=\"url\"" } else { "" },
                esc(href)
            )),
            None => out.push_str(&body),
        }
    }
    out
}

/// Styled spans as HTML, like [`spans_html`], with the formulas inside the
/// text (see [`inline_math`](super::inline_math)) as MathML when `math`.
pub(crate) fn text_html(spans: &[Span], math: bool) -> String {
    if !math || !super::inline_math::has_math(spans) {
        return spans_html(spans);
    }
    super::inline_math::split(spans)
        .into_iter()
        .map(|piece| match piece {
            super::inline_math::Piece::Text(spans) => spans_html(&spans),
            super::inline_math::Piece::Math(node, alt, _) => super::mathml::inline_math_html(&node, &alt),
        })
        .collect()
}

/// Whether a link text is a web or mail address rather than words.
fn looks_like_address(text: &str) -> bool {
    let text = text.trim();
    !text.contains(' ') && (text.contains("://") || text.contains("www.") || text.contains('/') || text.contains('@'))
}

/// The text of `spans` without styling and without the calls of notes,
/// trimmed.
pub(crate) fn plain(spans: &[Span]) -> String {
    spans
        .iter()
        .filter(|span| span.note.is_none())
        .map(|span| span.text.as_str())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Caption HTML with the label ("Figure 3:") emphasised, and inline
/// formulas as MathML when `math`.
pub(crate) fn caption_html(spans: &[Span], math: bool) -> String {
    if let Some(first) = spans.first()
        && let Some(label_match) = CAP_LABEL.find(&first.text)
    {
        let mut rest = spans.to_vec();
        let label = first.text[..label_match.end()].trim_end().to_string();
        rest[0].text = first.text[label.len()..].to_string();
        return format!(
            "<span class=\"cap-label\">{}</span>{}",
            esc(&label),
            text_html(&rest, math)
        );
    }
    text_html(spans, math)
}

/// Caption HTML with the label ("Figure 3") set apart and its trailing
/// punctuation (":", ".", "|", a dash) dropped: the friendly mode marks
/// the label by colour and weight instead. Inline formulas are MathML when
/// `math`.
pub(crate) fn caption_html_bare(spans: &[Span], math: bool) -> String {
    if let Some(first) = spans.first()
        && let Some(label_match) = CAP_LABEL.find(&first.text)
    {
        let mut rest = spans.to_vec();
        let label = first.text[..label_match.end()]
            .trim_end()
            .trim_end_matches([':', '.', '|', '—', '–', '-'])
            .trim_end();
        rest[0].text = first.text[label_match.end()..].trim_start().to_string();
        return format!(
            "<span class=\"cap-label\">{}</span>{}",
            esc(label),
            text_html(&rest, math)
        );
    }
    text_html(spans, math)
}

/// The CSS class of a float of the given kind.
pub(crate) fn float_class(kind: crate::model::FloatKind) -> &'static str {
    use crate::model::FloatKind::*;
    match kind {
        Figure => "fig",
        Table => "tab",
        Algorithm => "alg",
        Graphic => "gfx",
    }
}

/// The ` id="..."` attribute of a block that is the target of a link, or an
/// empty string when nothing links to it.
pub(crate) fn id_attr(block: &Block) -> String {
    match &block.anchor {
        Some(anchor) => format!(" id=\"{}\"", esc(anchor)),
        None => String::new(),
    }
}

/// The title of the document, or "Untitled paper" when none was found.
pub(crate) fn doc_title(doc: &Document) -> String {
    doc.meta.title.clone().unwrap_or_else(|| "Untitled paper".to_string())
}

/// Alt text for a figure: its label plus any text found inside it (at most
/// [`FIGURE_ALT_MAX_CHARS`] characters of it).
pub(crate) fn figure_alt(block: &Block, label: &Option<String>) -> String {
    let mut alt = label.clone().unwrap_or_else(|| "Figure".into());
    if let Some(inner) = block.rows.first() {
        let text = plain(inner);
        if !text.is_empty() {
            alt.push_str(": ");
            alt.extend(text.chars().take(FIGURE_ALT_MAX_CHARS));
        }
    }
    alt
}

/// The `data-kind` value of a block in the text layers of the facsimile
/// modes (`paragraph`, `heading`, ...; a figure block is its `caption`).
pub(crate) fn kind_name(kind: &BlockKind) -> &'static str {
    match kind {
        BlockKind::Title => "title",
        BlockKind::Authors => "authors",
        BlockKind::Note => "note",
        BlockKind::Heading { .. } => "heading",
        BlockKind::Paragraph { .. } => "paragraph",
        BlockKind::ListItem { .. } => "list-item",
        BlockKind::ListContinuation => "list-continuation",
        BlockKind::Equation { .. } => "equation",
        BlockKind::Figure { .. } => "caption",
        BlockKind::Footnote { .. } => "footnote",
        BlockKind::Reference { .. } => "reference",
        BlockKind::Code => "code",
        BlockKind::ContentsEntry { .. } => "contents-entry",
    }
}

/// A link to the arXiv abstract page of the paper for the top bar, or an
/// empty string when the paper has no arXiv identifier.
pub(crate) fn arxiv_abs_link(doc: &Document) -> String {
    doc.meta
        .arxiv_id
        .as_ref()
        .map(|id| {
            format!(
                "<a href=\"https://arxiv.org/abs/{0}\" target=\"_blank\" rel=\"noopener noreferrer\">arXiv:{0}</a>",
                esc(id)
            )
        })
        .unwrap_or_default()
}
