//! The pictures, charts and SmartArt of a deck's slides: pictures read
//! once each and made ready (`word::pictures`), charts read and drawn by
//! `chart`, and the text of a SmartArt diagram read from its data model, as
//! a tree of nodes.

use crate::chart::Charts;
use crate::model::Picture;
use crate::word::docx::{Rel, parse};
use crate::word::ir::Inline;
use crate::word::pictures::{MAX_PICTURE_BYTES, store, store_chart};
use crate::word::xml::{DRAWING_NS, child, children};
use crate::word::zip::ZipArchive;
use roxmltree::Node;
use std::collections::{HashMap, HashSet};

/// Namespace of the SmartArt data model (`dgm:`).
pub(crate) const DIAGRAM_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/diagram";
/// The deepest node of a SmartArt diagram read (a guard against a cycle of
/// connections or a hostile file).
const MAX_DIAGRAM_DEPTH: usize = 16;
/// The most nodes of a SmartArt diagram read.
const MAX_DIAGRAM_NODES: usize = 10_000;

/// The pictures and charts of a deck, as they are read.
pub(crate) struct Media<'z> {
    /// The package.
    zip: &'z ZipArchive<'z>,
    /// The pictures stored so far (charts drawn too).
    pub(crate) pictures: Vec<Picture>,
    /// The index of each picture part read, by path; `None` for one that
    /// cannot be shown.
    picture_index: HashMap<String, Option<usize>>,
    /// The charts, with the deck's theme.
    charts: Charts,
}

impl<'z> Media<'z> {
    /// No picture read yet, in `zip`, whose charts take their colours from
    /// the theme part `theme`.
    pub(crate) fn new(zip: &'z ZipArchive<'z>, theme: Option<&[u8]>) -> Self {
        Self {
            zip,
            pictures: Vec::new(),
            picture_index: HashMap::new(),
            charts: Charts::new(theme),
        }
    }

    /// The index of the picture of the relationship `id` of `rels`, read
    /// and made ready once; `None` when it cannot be shown.
    pub(crate) fn picture(&mut self, rels: &HashMap<String, Rel>, id: &str) -> Option<usize> {
        let rel = rels.get(id).filter(|rel| !rel.external)?;
        if let Some(index) = self.picture_index.get(&rel.target) {
            return *index;
        }
        let index = self
            .zip
            .read(&rel.target)
            .ok()
            .flatten()
            .and_then(|data| store(&mut self.pictures, data, MAX_PICTURE_BYTES));
        self.picture_index.insert(rel.target.clone(), index);
        index
    }

    /// The chart of the relationship `id`, drawn `size_pt` large; `None`
    /// when its part is missing or damaged.
    pub(crate) fn chart(&mut self, rels: &HashMap<String, Rel>, id: &str, size_pt: (f32, f32)) -> Option<Inline> {
        let rel = rels.get(id).filter(|rel| !rel.external)?;
        let xml = self.zip.read(&rel.target).ok()??;
        let mut placed = self.charts.place(&xml, size_pt)?;
        let picture = placed
            .svg
            .take()
            .and_then(|svg| store_chart(&mut self.pictures, svg, placed.size_pt, MAX_PICTURE_BYTES));
        Some(Inline::Chart(Box::new((placed, picture))))
    }

    /// The text of the SmartArt diagram whose data model is the part of
    /// the relationship `id`: its nodes in order, each with its depth (0
    /// for the outermost) and text; empty when the part is missing.
    pub(crate) fn diagram(&self, rels: &HashMap<String, Rel>, id: &str) -> Vec<(u8, String)> {
        let Some(rel) = rels.get(id).filter(|rel| !rel.external) else {
            return Vec::new();
        };
        let Some(xml) = self.zip.read_text(&rel.target).ok().flatten() else {
            return Vec::new();
        };
        parse(&xml).map_or_else(|_| Vec::new(), |doc| diagram_nodes(doc.root_element()))
    }
}

/// The nodes of a SmartArt data model (`dgm:dataModel`), depth first from
/// the document point, children in their order, each with its depth and
/// text. Points that are not nodes (transitions, presentation points) give
/// nothing; a node without text gives nothing but its children do.
fn diagram_nodes(model: Node<'_, '_>) -> Vec<(u8, String)> {
    let points: HashMap<&str, Node<'_, '_>> = child(model, DIAGRAM_NS, "ptLst")
        .into_iter()
        .flat_map(|list| children(list, DIAGRAM_NS, "pt"))
        .filter_map(|point| point.attribute("modelId").map(|id| (id, point)))
        .collect();
    let mut kids: HashMap<&str, Vec<(usize, &str)>> = HashMap::new();
    for cxn in child(model, DIAGRAM_NS, "cxnLst")
        .into_iter()
        .flat_map(|list| children(list, DIAGRAM_NS, "cxn"))
    {
        if cxn.attribute("type").unwrap_or("parOf") != "parOf" {
            continue;
        }
        let (Some(parent), Some(kid)) = (cxn.attribute("srcId"), cxn.attribute("destId")) else {
            continue;
        };
        let order = cxn
            .attribute("srcOrd")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        kids.entry(parent).or_default().push((order, kid));
    }
    for list in kids.values_mut() {
        list.sort_by_key(|(order, _)| *order);
    }
    let Some(root) = points
        .iter()
        .find(|(_, point)| point.attribute("type") == Some("doc"))
        .map(|(id, _)| *id)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    // Depth first, with an explicit stack of (point, its depth); a point is
    // read once, whatever its connections say.
    let mut stack: Vec<(&str, usize)> = vec![(root, 0)];
    let mut seen: HashSet<&str> = HashSet::new();
    while let Some((id, depth)) = stack.pop() {
        if !seen.insert(id) || seen.len() > MAX_DIAGRAM_NODES {
            continue;
        }
        if depth < MAX_DIAGRAM_DEPTH
            && let Some(list) = kids.get(id)
        {
            stack.extend(list.iter().rev().map(|(_, kid)| (*kid, depth + 1)));
        }
        if id == root {
            continue;
        }
        let Some(point) = points.get(id) else { continue };
        if !matches!(point.attribute("type").unwrap_or("node"), "node" | "asst") {
            continue;
        }
        let text = point_text(*point);
        if !text.is_empty() {
            out.push((u8::try_from(depth - 1).unwrap_or(u8::MAX), text));
        }
    }
    out
}

/// The text of a SmartArt point (`dgm:t`), its paragraphs joined by spaces.
fn point_text(point: Node<'_, '_>) -> String {
    let Some(body) = child(point, DIAGRAM_NS, "t") else {
        return String::new();
    };
    children(body, DRAWING_NS, "p")
        .map(|para| {
            para.descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "t")
                .filter_map(|node| node.text())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagram_nodes_follow_their_connections() {
        let xml = format!(
            "<dgm:dataModel xmlns:dgm=\"{DIAGRAM_NS}\" xmlns:a=\"{DRAWING_NS}\"><dgm:ptLst>\
             <dgm:pt modelId=\"0\" type=\"doc\"/>\
             <dgm:pt modelId=\"b\"><dgm:t><a:p><a:r><a:t>Second</a:t></a:r></a:p></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"a\"><dgm:t><a:p><a:r><a:t>First</a:t></a:r></a:p></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"a1\"><dgm:t><a:p><a:r><a:t>Under first</a:t></a:r></a:p></dgm:t></dgm:pt>\
             <dgm:pt modelId=\"x\" type=\"sibTrans\"/>\
             </dgm:ptLst><dgm:cxnLst>\
             <dgm:cxn modelId=\"c1\" srcId=\"0\" destId=\"b\" srcOrd=\"1\"/>\
             <dgm:cxn modelId=\"c2\" srcId=\"0\" destId=\"a\" srcOrd=\"0\"/>\
             <dgm:cxn modelId=\"c3\" srcId=\"a\" destId=\"a1\" srcOrd=\"0\"/>\
             <dgm:cxn modelId=\"c4\" type=\"presOf\" srcId=\"a\" destId=\"x\"/>\
             <dgm:cxn modelId=\"c5\" srcId=\"a1\" destId=\"0\" srcOrd=\"0\"/>\
             </dgm:cxnLst></dgm:dataModel>"
        );
        let doc = parse(&xml).unwrap();
        // The connection back to the document point is not followed again.
        assert_eq!(
            diagram_nodes(doc.root_element()),
            [
                (0, "First".to_string()),
                (1, "Under first".to_string()),
                (0, "Second".to_string())
            ]
        );
    }
}
