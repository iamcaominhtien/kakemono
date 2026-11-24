//! The workbook part (`xl/workbook.xml`): its sheets in order, with their
//! state and part, the date system, the print titles, and the title of the
//! document properties.

use crate::word::docx::{Rel, document_title, parse, read_rels};
use crate::word::xml::REL_NS;
use crate::word::zip::ZipArchive;
use anyhow::{Context, Result};
use roxmltree::Node;
use std::collections::HashMap;

use super::grid::Merge;
use super::sheet::range;

/// Namespace of SpreadsheetML.
pub(crate) const SML_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
/// Namespace of SpreadsheetML in a Strict Office Open XML file.
pub(crate) const SML_STRICT_NS: &str = "http://purl.oclc.org/ooxml/spreadsheetml/main";
/// Namespace of relationship ids in a Strict Office Open XML file.
pub(crate) const REL_STRICT_NS: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";
/// Relationship type of the main part of a package.
const OFFICE_DOCUMENT_REL: &str = "officeDocument";
/// Relationship type of the shared strings part.
const SHARED_STRINGS_REL: &str = "sharedStrings";
/// The usual shared strings part, when the workbook does not relate one.
const DEFAULT_SHARED_STRINGS_PART: &str = "xl/sharedStrings.xml";
/// The defined name of the rows (and columns) printed on every page.
const PRINT_TITLES_NAME: &str = "_xlnm.Print_Titles";
/// The defined name of a sheet's print area.
const PRINT_AREA_NAME: &str = "_xlnm.Print_Area";

/// Whether `node` is the SpreadsheetML element `name` (transitional or
/// strict namespace).
pub(crate) fn is_sml(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.tag_name().name() == name
        && matches!(node.tag_name().namespace(), Some(SML_NS | SML_STRICT_NS))
}

/// The first SpreadsheetML child element `name` of `node`.
fn sml_child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|item| is_sml(*item, name))
}

/// A workbook: what its main part says about the sheets.
#[derive(Debug, Clone, Default)]
pub(crate) struct Workbook {
    /// The path of the workbook part (`xl/workbook.xml`).
    pub(crate) part: String,
    /// Every sheet, in the order of the tabs, hidden ones included.
    pub(crate) sheets: Vec<SheetRef>,
    /// Dates count from 1904-01-01 rather than 1900-01-01 (`date1904`).
    pub(crate) date1904: bool,
    /// The title of the document properties (`docProps/core.xml`).
    pub(crate) title: Option<String>,
    /// The rows printed at the top of every page (`_xlnm.Print_Titles`),
    /// by index of the sheet in [`Workbook::sheets`]: the first and last
    /// row, zero-based, inclusive.
    pub(crate) print_titles: HashMap<usize, (u32, u32)>,
    /// The print area of each sheet that has one (`_xlnm.Print_Area`),
    /// by index in [`Workbook::sheets`]: the box of its ranges.
    pub(crate) print_areas: HashMap<usize, Merge>,
}

/// One sheet of a workbook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SheetRef {
    /// The name on its tab.
    pub(crate) name: String,
    /// Shown: neither hidden nor very hidden.
    pub(crate) visible: bool,
    /// The path of its part in the package (`xl/worksheets/sheet1.xml`);
    /// empty when the relationship is missing.
    pub(crate) part: String,
}

/// The path of the workbook part, from the package relationships: `None`
/// when the package's main part is not a workbook that is there.
pub(crate) fn workbook_part(zip: &ZipArchive<'_>) -> Option<String> {
    read_rels(zip, "")
        .ok()?
        .into_iter()
        .find(|(_, kind, rel)| !rel.external && kind == OFFICE_DOCUMENT_REL)
        .map(|(_, _, rel)| rel.target)
        .filter(|target| target.ends_with("workbook.xml") && zip.contains(target))
}

impl Workbook {
    /// Reads the workbook part of `zip`.
    ///
    /// # Errors
    /// When the package has no workbook part, or it is damaged.
    pub(crate) fn read(zip: &ZipArchive<'_>) -> Result<Self> {
        let part = workbook_part(zip).context("no workbook in the package")?;
        let xml = zip.read_text(&part)?.context("no workbook in the package")?;
        let doc = parse(&xml).with_context(|| format!("reading {part}"))?;
        let root = doc.root_element();
        let rels: HashMap<String, Rel> = read_rels(zip, &part)?
            .into_iter()
            .map(|(id, _, rel)| (id, rel))
            .collect();
        let date1904 = sml_child(root, "workbookPr")
            .and_then(|props| props.attribute("date1904"))
            .is_some_and(|value| matches!(value, "1" | "true"));
        let sheets: Vec<SheetRef> = sml_child(root, "sheets")
            .into_iter()
            .flat_map(|sheets| sheets.children().filter(|node| is_sml(*node, "sheet")))
            .map(|sheet| SheetRef {
                name: sheet.attribute("name").unwrap_or_default().to_string(),
                visible: matches!(sheet.attribute("state"), None | Some("visible")),
                part: rel_id(sheet)
                    .and_then(|id| rels.get(id))
                    .filter(|rel| !rel.external)
                    .map(|rel| rel.target.clone())
                    .unwrap_or_default(),
            })
            .collect();
        let defined = |wanted: &'static str| {
            sml_child(root, "definedNames")
                .into_iter()
                .flat_map(|names| names.children().filter(|node| is_sml(*node, "definedName")))
                .filter(move |name| name.attribute("name") == Some(wanted))
                .filter_map(|name| {
                    let sheet = name.attribute("localSheetId")?.parse::<usize>().ok()?;
                    (sheet < sheets.len()).then_some((sheet, name.text()?))
                })
        };
        let print_titles = defined(PRINT_TITLES_NAME)
            .filter_map(|(sheet, text)| Some((sheet, title_rows(text)?)))
            .collect();
        let print_areas = defined(PRINT_AREA_NAME)
            .filter_map(|(sheet, text)| Some((sheet, print_area(text)?)))
            .collect();
        Ok(Self {
            part,
            sheets,
            date1904,
            title: document_title(zip)?,
            print_titles,
            print_areas,
        })
    }

    /// The path of the shared strings part, related to the workbook (or at
    /// its usual place).
    pub(crate) fn shared_strings_part(&self, zip: &ZipArchive<'_>) -> String {
        read_rels(zip, &self.part)
            .unwrap_or_default()
            .into_iter()
            .find(|(_, kind, rel)| !rel.external && kind == SHARED_STRINGS_REL)
            .map_or_else(|| DEFAULT_SHARED_STRINGS_PART.to_string(), |(_, _, rel)| rel.target)
    }
}

/// The relationship id (`r:id`) of a `sheet` element.
fn rel_id<'a>(sheet: Node<'a, '_>) -> Option<&'a str> {
    sheet
        .attribute((REL_NS, "id"))
        .or_else(|| sheet.attribute((REL_STRICT_NS, "id")))
}

/// The rows of a print titles reference (`'My sheet'!$1:$2`, possibly with
/// columns too: `Sheet1!$A:$B,Sheet1!$1:$3`), zero-based, inclusive.
fn title_rows(reference: &str) -> Option<(u32, u32)> {
    reference.split(',').find_map(|area| {
        let range = area.rsplit_once('!').map_or(area, |(_, range)| range);
        let (first, last) = range.split_once(':')?;
        let row = |cell: &str| cell.trim().trim_start_matches('$').parse::<u32>().ok()?.checked_sub(1);
        Some((row(first)?, row(last)?))
    })
}

/// The box of the ranges of a print area (`'Sheet 1'!$A$1:$H$40`, or
/// several separated by commas); `None` when none is a range of cells.
fn print_area(reference: &str) -> Option<Merge> {
    reference
        .split(',')
        .filter_map(|area| range(area.rsplit_once('!').map_or(area, |(_, range)| range)))
        .reduce(|one, other| Merge {
            top: one.top.min(other.top),
            left: one.left.min(other.left),
            bottom: one.bottom.max(other.bottom),
            right: one.right.max(other.right),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_areas_are_the_box_of_their_ranges() {
        let area = |top, left, bottom, right| Merge {
            top,
            left,
            bottom,
            right,
        };
        assert_eq!(print_area("'My sheet'!$B$2:$M$29"), Some(area(1, 1, 28, 12)));
        assert_eq!(print_area("S!$A$1:$C$3,S!$E$10:$F$12"), Some(area(0, 0, 11, 5)));
        assert_eq!(print_area("S!#REF!"), None);
    }

    #[test]
    fn print_titles_name_their_rows() {
        assert_eq!(title_rows("'My sheet'!$1:$2"), Some((0, 1)));
        assert_eq!(title_rows("Sheet1!$A:$B,Sheet1!$3:$4"), Some((2, 3)));
        assert_eq!(title_rows("Sheet1!$A:$B"), None);
    }
}
