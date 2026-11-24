//! The notes of a web page, and the markup of the tools that write the
//! pages most read, made into the semantics the reader of `epub` knows
//! (a link with `role="doc-noteref"` to the note it calls, an author in a
//! block of class `author`):
//!
//! - a link in a superscript to an item of a list of notes (Wikipedia's
//!   `[1]`, the notes of most blogs and of pandoc) calls a note;
//! - LaTeXML (arXiv's HTML papers, ar5iv) writes a note where it is
//!   called, the authors as spans and the title of the abstract as `<h6>`;
//! - MediaWiki (Wikipedia) adds links back from its notes and markers of
//!   its own.

use super::dom::{Dom, Id};
use std::collections::HashMap;

/// Longest text of a link that calls a note (`[note 12]`).
const NOTE_MARK_MAX_CHARS: usize = 12;
/// Prefix of the `id`s given to the notes LaTeXML writes in place.
const NOTE_ID_PREFIX: &str = "kakemono-note-";
/// Prefix of the `id`s given to the links that call them.
const NOTE_REF_ID_PREFIX: &str = "kakemono-noteref-";
/// Words of a class that mark a link calling a note.
const NOTE_REF_WORDS: [&str; 5] = ["footnote", "noteref", "fnref", "footnote-ref", "footnote-reference"];
/// Classes of MediaWiki that are no text of the article: the links back
/// from a note, and the markers of a missing citation.
const MEDIAWIKI_REMOVED: [&str; 3] = ["mw-cite-backlink", "mw-ref-follow", "mw-empty-elt"];
/// Classes of LaTeXML that are no text: the separators of the authors,
/// the names of macros it could not convert, the type of a note
/// (`thanks:`) and the labels of an author's addresses (`Email:`).
const LATEXML_REMOVED: [&str; 4] = ["ltx_author_before", "ltx_ERROR", "ltx_note_type", "ltx_contact_name"];

/// Applies the rules of the module to the article in `root`.
pub(crate) fn mark(dom: &mut Dom, root: Id) {
    let latexml = dom.descendants(root).into_iter().any(|node| {
        dom.classes(node)
            .any(|class| class == "ltx_document" || class == "ltx_para")
    });
    if latexml {
        latexml_markup(dom, root);
    }
    if dom.descendants(root).into_iter().any(|node| {
        dom.classes(node)
            .any(|class| class == "mw-parser-output" || class == "mw-references")
    }) {
        remove_classes(dom, root, &MEDIAWIKI_REMOVED);
    }
    note_links(dom, root);
}

/// Removes the elements in `root` with one of the classes `classes`.
fn remove_classes(dom: &mut Dom, root: Id, classes: &[&str]) {
    for node in dom.descendants(root) {
        if dom.classes(node).any(|class| classes.contains(&class)) {
            dom.detach(node);
        }
    }
}

/// The markup of LaTeXML: notes written in place (`ltx_note`) move to the
/// end with a link calling them; each author (`ltx_creator`) becomes a
/// block of class `author`; the title of the abstract becomes a heading of
/// the level of the sections.
fn latexml_markup(dom: &mut Dom, root: Id) {
    remove_classes(dom, root, &LATEXML_REMOVED);
    front_after_title(dom, root);
    let mut count = 0;
    // The notes moved so far, by mark: a `\footnotemark` calls one again.
    let mut by_mark: HashMap<String, String> = HashMap::new();
    for node in dom.descendants(root) {
        if !dom.attached(node) {
            continue;
        }
        let is_note = dom.has_class(node, "ltx_note");
        if is_note && (dom.has_class(node, "ltx_role_footnote") || dom.has_class(node, "ltx_role_thanks")) {
            count += 1;
            let (mark, id) = latexml_note(dom, root, node, count);
            by_mark.entry(mark).or_insert(id);
        } else if is_note && dom.has_class(node, "ltx_role_footnotemark") {
            count += 1;
            let mark = note_mark(dom, node).unwrap_or_default();
            let call = match by_mark.get(&mark) {
                Some(id) => {
                    let ref_id = format!("{NOTE_REF_ID_PREFIX}{count}");
                    dom.element(
                        "a",
                        &[("href", &format!("#{id}")), ("id", &ref_id), ("role", "doc-noteref")],
                    )
                }
                None => dom.element("sup", &[]),
            };
            let text = dom.text_node(&mark);
            dom.append(call, text);
            dom.insert_after(node, call);
            dom.detach(node);
        } else if dom.is(node, "a") && dom.attr(node, "href") == Some("mailto:") {
            let address = dom.clean_text(node);
            dom.set_attr(node, "href", &format!("mailto:{address}"));
        } else if dom.has_class(node, "ltx_creator") {
            dom.rename(node, "div");
            dom.add_class(node, "author");
        } else if dom.has_class(node, "ltx_title_abstract") && dom.is_any(node, &["h1", "h2", "h3", "h4", "h5", "h6"]) {
            dom.rename(node, "h2");
        }
    }
}

/// The mark of the note `node` of LaTeXML.
fn note_mark(dom: &Dom, node: Id) -> Option<String> {
    dom.descendants(node)
        .into_iter()
        .find(|&id| dom.has_class(id, "ltx_note_mark"))
        .map(|id| dom.clean_text(id))
        .filter(|mark| !mark.is_empty())
}

/// Moves what LaTeXML writes before the title of the paper (a note on its
/// license, set above the title on the first page) after its authors: the
/// title comes first.
fn front_after_title(dom: &mut Dom, root: Id) {
    let Some(title) = dom
        .descendants(root)
        .into_iter()
        .find(|&id| dom.has_class(id, "ltx_title_document"))
    else {
        return;
    };
    let Some(parent) = dom.nodes[title].parent else {
        return;
    };
    let siblings = dom.element_children(parent);
    let before: Vec<Id> = siblings.iter().copied().take_while(|&id| id != title).collect();
    let after = siblings
        .iter()
        .copied()
        .find(|&id| dom.has_class(id, "ltx_authors"))
        .unwrap_or(title);
    for node in before.into_iter().rev() {
        dom.insert_after(after, node);
    }
}

/// Moves the note `node` of LaTeXML (its mark, then its content) to the
/// end of `root`, a link calling it in its place. Returns its mark and the
/// `id` it is given.
fn latexml_note(dom: &mut Dom, root: Id, node: Id, number: usize) -> (String, String) {
    let mark = note_mark(dom, node).unwrap_or_else(|| number.to_string());
    let content = dom
        .descendants(node)
        .into_iter()
        .find(|&id| dom.has_class(id, "ltx_note_content"));
    let note_id = format!("{NOTE_ID_PREFIX}{number}");
    let ref_id = format!("{NOTE_REF_ID_PREFIX}{number}");
    let link = dom.element(
        "a",
        &[
            ("href", &format!("#{note_id}")),
            ("id", &ref_id),
            ("role", "doc-noteref"),
        ],
    );
    let text = dom.text_node(&mark);
    dom.append(link, text);
    dom.insert_after(node, link);
    dom.detach(node);
    let Some(content) = content else {
        return (mark, note_id);
    };
    for id in dom.descendants(content) {
        if dom.has_class(id, "ltx_note_mark") || dom.has_class(id, "ltx_tag_note") {
            dom.detach(id);
        }
    }
    let note = dom.element("aside", &[("id", &note_id), ("role", "doc-footnote")]);
    let para = dom.element("p", &[]);
    for child in dom.nodes[content].children.clone() {
        dom.append(para, child);
    }
    dom.append(note, para);
    dom.append(root, note);
    (mark, note_id)
}

/// Marks the links in `root` that call a note: a link to a part of the
/// page in a superscript (or holding one, or of a class of note links),
/// with a short text, whose target is an item of a list or a note. The
/// link takes the `id` of its superscript, which the note links back to.
fn note_links(dom: &mut Dom, root: Id) {
    let mut ids: HashMap<String, Id> = HashMap::new();
    for node in dom.descendants(root) {
        if let Some(id) = dom.attr(node, "id") {
            ids.entry(id.to_string()).or_insert(node);
        }
    }
    let mut count = 0;
    for link in dom.find_all(root, "a") {
        let Some(target) = dom
            .attr(link, "href")
            .and_then(|href| href.strip_prefix('#'))
            .and_then(|fragment| ids.get(fragment).copied())
        else {
            continue;
        };
        if dom.attr(link, "role").is_some_and(|role| role.contains("doc-noteref")) {
            continue;
        }
        let sup = dom.ancestor(link, |id| dom.is(id, "sup"));
        let holds_sup = dom.element_children(link).iter().any(|&child| dom.is(child, "sup"));
        let classed = dom
            .classes(link)
            .any(|class| NOTE_REF_WORDS.iter().any(|word| class.eq_ignore_ascii_case(word)));
        let short = dom.clean_text(link).chars().count() <= NOTE_MARK_MAX_CHARS;
        let is_note = (dom.is_any(target, &["li", "aside", "div", "p", "section", "dd"]))
            && !dom.classes(target).any(|class| class.contains("bib"))
            && !dom.attr(target, "id").is_some_and(|id| id.contains("bib"));
        if !((sup.is_some() || holds_sup || classed) && short && is_note) {
            continue;
        }
        dom.set_attr(link, "role", "doc-noteref");
        if dom.attr(link, "id").is_none() {
            let id = sup
                .and_then(|sup| dom.attr(sup, "id").map(str::to_string))
                .unwrap_or_else(|| {
                    count += 1;
                    format!("{NOTE_REF_ID_PREFIX}link-{count}")
                });
            if let Some(sup) = sup {
                dom.remove_attr(sup, "id");
            }
            dom.set_attr(link, "id", &id);
        }
        // The superscript is the note's mark: the reader writes its own.
        if let Some(sup) = sup
            && dom.clean_text(sup) == dom.clean_text(link)
        {
            dom.unwrap(sup);
        }
    }
}
