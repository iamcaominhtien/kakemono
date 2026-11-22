//! The pictures placed in cells (Excel 365's "Place in cell"): a cell's
//! value metadata (`vm`) leads, through the workbook's metadata part
//! (`xl/metadata.xml`), to a rich value (`xl/richData/rdrichvalue.xml`)
//! whose structure (`rdrichvaluestructure.xml`) says which of its values
//! is the index of a relationship (`richValueRel.xml` and its
//! relationships) to the picture's file.

use crate::word::docx::{parse, read_rels, rel_types_of};
use crate::word::zip::ZipArchive;
use roxmltree::Node;
use std::collections::HashMap;

/// The metadata type of rich values.
const RICH_VALUE_TYPE: &str = "XLRICHVALUE";
/// The key of a rich value's structure that holds the index of its
/// picture's relationship.
const IMAGE_KEY: &str = "_rvRel:LocalImageIdentifier";
/// The key of a rich value's structure that holds its alternative text.
const TEXT_KEY: &str = "Text";
/// The parts, by the last word of their relationship type from the
/// workbook, and where Excel puts them.
const PARTS: [(&str, &str); 4] = [
    ("sheetMetadata", "xl/metadata.xml"),
    ("rdRichValue", "xl/richData/rdrichvalue.xml"),
    ("rdRichValueStructure", "xl/richData/rdrichvaluestructure.xml"),
    ("richValueRel", "xl/richData/richValueRel.xml"),
];

/// A picture placed in cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CellPicture {
    /// The path of its file in the package.
    pub(crate) part: String,
    /// Its alternative text.
    pub(crate) alt: String,
}

/// The pictures of the cells of the workbook `zip` (main part
/// `workbook`), by the value metadata index (`vm`, from 1) of their cells.
/// A workbook without them, or with a damaged part, has none.
pub(crate) fn cell_pictures(zip: &ZipArchive<'_>, workbook: &str) -> HashMap<u32, CellPicture> {
    let related = rel_types_of(zip, workbook);
    let path = |(kind, usual): (&str, &str)| {
        related
            .iter()
            .find(|(name, _)| name == kind)
            .map_or_else(|| usual.to_string(), |(_, target)| target.clone())
    };
    let [metadata, values, structures, rels] = PARTS.map(path);
    let read = |part: &str| zip.read_text(part).ok().flatten();
    let (Some(metadata), Some(values), Some(structures), Some(rel_list)) =
        (read(&metadata), read(&values), read(&structures), read(&rels))
    else {
        return HashMap::new();
    };
    let (Ok(metadata), Ok(values), Ok(structures), Ok(rel_list)) =
        (parse(&metadata), parse(&values), parse(&structures), parse(&rel_list))
    else {
        return HashMap::new();
    };
    let files: HashMap<String, String> = read_rels(zip, &rels)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, _, rel)| !rel.external)
        .map(|(id, _, rel)| (id, rel.target))
        .collect();
    let pictures: Vec<Option<String>> = named(rel_list.root_element(), "rel")
        .map(|rel| {
            let id = rel.attributes().find(|attr| attr.name() == "id")?.value();
            files.get(id).cloned()
        })
        .collect();
    let keys: Vec<(Option<usize>, Option<usize>)> = named(structures.root_element(), "s")
        .map(|structure| {
            let names: Vec<&str> = named(structure, "k")
                .map(|key| key.attribute("n").unwrap_or_default())
                .collect();
            let position = |key: &str| names.iter().position(|name| *name == key);
            (position(IMAGE_KEY), position(TEXT_KEY))
        })
        .collect();
    let rich: Vec<Option<CellPicture>> = named(values.root_element(), "rv")
        .map(|value| {
            let structure = value.attribute("s")?.parse::<usize>().ok()?;
            let (image, text) = *keys.get(structure)?;
            let fields: Vec<&str> = named(value, "v")
                .map(|field| field.text().unwrap_or_default())
                .collect();
            let rel = fields.get(image?)?.trim().parse::<usize>().ok()?;
            Some(CellPicture {
                part: pictures.get(rel)?.clone()?,
                alt: text
                    .and_then(|text| fields.get(text))
                    .map(|text| text.trim().to_string())
                    .unwrap_or_default(),
            })
        })
        .collect();
    by_value_metadata(metadata.root_element(), &rich)
}

/// The pictures `rich` (by rich value index) by the value metadata index
/// (from 1) that names them in the metadata part `root`.
fn by_value_metadata(root: Node<'_, '_>, rich: &[Option<CellPicture>]) -> HashMap<u32, CellPicture> {
    let types: Vec<&str> = named(root, "metadataTypes")
        .flat_map(|types| named(types, "metadataType"))
        .map(|kind| kind.attribute("name").unwrap_or_default())
        .collect();
    // The rich value of each block of the rich values' future metadata.
    let futures: Vec<Option<usize>> = named(root, "futureMetadata")
        .filter(|future| future.attribute("name") == Some(RICH_VALUE_TYPE))
        .flat_map(|future| named(future, "bk"))
        .map(|block| {
            block
                .descendants()
                .find(|node| node.tag_name().name() == "rvb")?
                .attribute("i")?
                .parse()
                .ok()
        })
        .collect();
    let mut found = HashMap::new();
    let blocks = named(root, "valueMetadata").flat_map(|list| named(list, "bk"));
    for (index, block) in blocks.enumerate() {
        let Some(record) = named(block, "rc").next() else {
            continue;
        };
        let number = |name: &str| record.attribute(name)?.parse::<usize>().ok();
        let (Some(kind), Some(value)) = (number("t"), number("v")) else {
            continue;
        };
        if kind.checked_sub(1).and_then(|kind| types.get(kind)) != Some(&RICH_VALUE_TYPE) {
            continue;
        }
        let picture = futures
            .get(value)
            .copied()
            .flatten()
            .and_then(|rich_index| rich.get(rich_index)?.clone());
        if let (Some(picture), Ok(vm)) = (picture, u32::try_from(index + 1)) {
            found.insert(vm, picture);
        }
    }
    found
}

/// The child elements of `node` named `name`, whatever their namespace.
fn named<'a, 'i, 'n>(node: Node<'a, 'i>, name: &'n str) -> impl Iterator<Item = Node<'a, 'i>> + 'n
where
    'a: 'n,
    'i: 'n,
{
    node.children()
        .filter(move |child| child.is_element() && child.tag_name().name() == name)
}
