//! The fixed parts of a `.docx` package (content types, relationships,
//! styles, settings, properties) and the page layout.

use super::xml::esc;
use crate::model::Meta;

/// Width of a US Letter page, in twentieths of a point (twips).
pub(crate) const PAGE_WIDTH_TWIPS: u32 = 12240;
/// Height of a US Letter page, in twips.
pub(crate) const PAGE_HEIGHT_TWIPS: u32 = 15840;
/// Page margin on every side (one inch), in twips.
pub(crate) const MARGIN_TWIPS: u32 = 1440;
/// Width of the text column, in twips.
pub(crate) const TEXT_WIDTH_TWIPS: u32 = PAGE_WIDTH_TWIPS - 2 * MARGIN_TWIPS;
/// Twips per PDF point.
pub(crate) const TWIPS_PER_PT: f32 = 20.0;

/// Namespace declarations of the main document part.
pub(crate) const DOCUMENT_NAMESPACES: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\" \
xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" \
xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"";

/// The XML declaration that starts every part.
pub(crate) const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Base of the relationship types of Office documents.
const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// `[Content_Types].xml`: the media type of every part.
pub(crate) const CONTENT_TYPES: &str = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Default Extension=\"png\" ContentType=\"image/png\"/>\
<Default Extension=\"jpeg\" ContentType=\"image/jpeg\"/>\
<Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
<Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>\
<Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/>\
<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>\
<Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>\
</Types>";

/// The content type of the footnotes part, added to `[Content_Types].xml`
/// when the document has translator's notes.
pub(crate) const FOOTNOTES_TYPE: &str = "<Override PartName=\"/word/footnotes.xml\" \
ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml\"/>";

/// The relationship of the main document to its footnotes part, added
/// when the document has translator's notes.
pub(crate) const FOOTNOTES_REL: &str = "<Relationship Id=\"rIdFootnotes\" \
Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes\" Target=\"footnotes.xml\"/>";

/// `word/footnotes.xml`: the separator and continuation separator that
/// Word expects (ids -1 and 0), then one footnote per item of `notes` (its
/// runs), with ids from 1, in the `FootnoteText` style with its raised
/// number first.
pub(crate) fn footnotes_xml(notes: &[String]) -> String {
    let mut out = format!(
        "{XML_DECL}<w:footnotes xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\">\
<w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
<w:r><w:separator/></w:r></w:p></w:footnote>\
<w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:pPr><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/></w:pPr>\
<w:r><w:continuationSeparator/></w:r></w:p></w:footnote>"
    );
    for (i, runs) in notes.iter().enumerate() {
        out.push_str(&format!(
            "<w:footnote w:id=\"{}\"><w:p><w:pPr><w:pStyle w:val=\"FootnoteText\"/></w:pPr>\
<w:r><w:rPr><w:vertAlign w:val=\"superscript\"/></w:rPr><w:footnoteRef/></w:r><w:r><w:t xml:space=\"preserve\"> </w:t></w:r>{runs}</w:p></w:footnote>",
            i + 1
        ));
    }
    out.push_str("</w:footnotes>");
    out
}

/// `_rels/.rels`: the main document and the property parts.
pub(crate) const PACKAGE_RELS: &str = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
<Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>\
<Relationship Id=\"rId3\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties\" Target=\"docProps/app.xml\"/>\
</Relationships>";

/// `docProps/app.xml`: the application that wrote the document.
pub(crate) const APP_PROPS: &str = "<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">\
<Application>kakemono</Application></Properties>";

/// `word/settings.xml`: Word 2013+ layout, Cambria Math for formulas.
pub(crate) const SETTINGS: &str = "<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">\
<w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat>\
<m:mathPr><m:mathFont m:val=\"Cambria Math\"/><m:dispDef/></m:mathPr>\
</w:settings>";

/// `word/styles.xml`: the paragraph and character styles the body uses.
/// Built-in names (`Title`, `Heading1`, `Caption`, ...) keep Word's
/// navigation pane, outline and table of contents working.
pub(crate) const STYLES: &str = concat!(
    "<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">",
    "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Cambria\" w:hAnsi=\"Cambria\" w:eastAsia=\"Cambria\" w:cs=\"Times New Roman\"/>",
    "<w:sz w:val=\"22\"/><w:szCs w:val=\"22\"/><w:lang w:val=\"en-US\"/></w:rPr></w:rPrDefault>",
    "<w:pPrDefault><w:pPr><w:spacing w:after=\"120\" w:line=\"264\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults>",
    "<w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:jc w:val=\"both\"/></w:pPr></w:style>",
    "<w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/>",
    "<w:uiPriority w:val=\"1\"/><w:semiHidden/></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Title\"><w:name w:val=\"Title\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:spacing w:after=\"240\" w:line=\"240\" w:lineRule=\"auto\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Author\"><w:name w:val=\"Author\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:spacing w:after=\"0\"/><w:jc w:val=\"center\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"AuthorName\"><w:name w:val=\"Author Name\"/><w:basedOn w:val=\"Author\"/>",
    "<w:pPr><w:spacing w:before=\"120\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"22\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Note\"><w:name w:val=\"Note\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:jc w:val=\"left\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"360\" w:after=\"120\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"0\"/></w:pPr>",
    "<w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading2\"><w:name w:val=\"heading 2\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"280\" w:after=\"80\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"1\"/></w:pPr>",
    "<w:rPr><w:b/><w:sz w:val=\"24\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading3\"><w:name w:val=\"heading 3\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"200\" w:after=\"60\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"2\"/></w:pPr>",
    "<w:rPr><w:b/><w:sz w:val=\"22\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Heading4\"><w:name w:val=\"heading 4\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before=\"160\" w:after=\"40\"/><w:jc w:val=\"left\"/><w:outlineLvl w:val=\"3\"/></w:pPr>",
    "<w:rPr><w:b/><w:i/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"AbstractHeading\"><w:name w:val=\"Abstract Heading\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:next w:val=\"Abstract\"/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"60\"/><w:jc w:val=\"center\"/></w:pPr>",
    "<w:rPr><w:b/><w:sz w:val=\"20\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Abstract\"><w:name w:val=\"Abstract\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:ind w:left=\"567\" w:right=\"567\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"SceneBreak\"><w:name w:val=\"Scene Break\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:jc w:val=\"center\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"ListItem\"><w:name w:val=\"List Item\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"425\" w:hanging=\"425\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"ListContinue\"><w:name w:val=\"List Continuation\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"60\"/><w:ind w:left=\"425\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Figure\"><w:name w:val=\"Figure\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:before=\"120\" w:after=\"60\" w:line=\"240\" w:lineRule=\"auto\"/><w:jc w:val=\"center\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Caption\"><w:name w:val=\"caption\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:spacing w:before=\"60\" w:after=\"200\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Equation\"><w:name w:val=\"Equation\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:before=\"60\" w:after=\"60\"/><w:jc w:val=\"center\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"EquationNumber\"><w:name w:val=\"Equation Number\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"0\"/><w:jc w:val=\"right\"/></w:pPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"Code\"><w:name w:val=\"Code\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>",
    "<w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F4F4F4\"/><w:spacing w:after=\"0\" w:line=\"240\" w:lineRule=\"auto\"/>",
    "<w:jc w:val=\"left\"/></w:pPr><w:rPr><w:rFonts w:ascii=\"Consolas\" w:hAnsi=\"Consolas\" w:cs=\"Consolas\"/><w:sz w:val=\"17\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:customStyle=\"1\" w:styleId=\"TableText\"><w:name w:val=\"Table Text\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:before=\"20\" w:after=\"20\" w:line=\"240\" w:lineRule=\"auto\"/><w:jc w:val=\"left\"/></w:pPr>",
    "<w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"FootnoteText\"><w:name w:val=\"footnote text\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"80\"/></w:pPr><w:rPr><w:sz w:val=\"18\"/></w:rPr></w:style>",
    "<w:style w:type=\"paragraph\" w:styleId=\"Bibliography\"><w:name w:val=\"Bibliography\"/><w:basedOn w:val=\"Normal\"/>",
    "<w:pPr><w:spacing w:after=\"80\"/><w:ind w:left=\"567\" w:hanging=\"567\"/><w:jc w:val=\"left\"/></w:pPr><w:rPr><w:sz w:val=\"20\"/></w:rPr></w:style>",
    "<w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/><w:basedOn w:val=\"DefaultParagraphFont\"/>",
    "<w:rPr><w:color w:val=\"1F4E99\"/><w:u w:val=\"single\"/></w:rPr></w:style>",
    "<w:style w:type=\"character\" w:customStyle=\"1\" w:styleId=\"CrossReference\"><w:name w:val=\"Cross Reference\"/>",
    "<w:basedOn w:val=\"DefaultParagraphFont\"/><w:rPr><w:color w:val=\"1F4E99\"/></w:rPr></w:style>",
    "</w:styles>",
);

/// `docProps/core.xml`: the title and, when known, the arXiv identifier
/// of the paper. No dates, so that the same paper gives the same file.
pub(crate) fn core_props(title: &str, meta: &Meta, source: &str) -> String {
    let identifier = meta
        .arxiv_id
        .as_ref()
        .map(|id| format!("<dc:identifier>arXiv:{}</dc:identifier>", esc(id)))
        .unwrap_or_default();
    format!(
        "{XML_DECL}<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" \
xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\
<dc:title>{}</dc:title>{identifier}<dc:description>Converted from {source} by kakemono</dc:description></cp:coreProperties>",
        esc(title)
    )
}

/// `word/_rels/document.xml.rels`: styles, settings, the pictures
/// (`(id, target)`) and the external links (`(id, URL)`).
pub(crate) fn document_rels(images: &[(String, String)], links: &[(String, String)]) -> String {
    let mut out = format!(
        "{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rIdStyles\" Type=\"{REL_BASE}/styles\" Target=\"styles.xml\"/>\
<Relationship Id=\"rIdSettings\" Type=\"{REL_BASE}/settings\" Target=\"settings.xml\"/>"
    );
    for (id, target) in images {
        out.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{REL_BASE}/image\" Target=\"{}\"/>",
            esc(target)
        ));
    }
    for (id, url) in links {
        out.push_str(&format!(
            "<Relationship Id=\"{id}\" Type=\"{REL_BASE}/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>",
            esc(url)
        ));
    }
    out.push_str("</Relationships>");
    out
}

/// The section properties that end the body: a US Letter page with
/// one-inch margins.
pub(crate) fn section_props() -> String {
    format!(
        "<w:sectPr><w:pgSz w:w=\"{PAGE_WIDTH_TWIPS}\" w:h=\"{PAGE_HEIGHT_TWIPS}\"/>\
<w:pgMar w:top=\"{MARGIN_TWIPS}\" w:right=\"{MARGIN_TWIPS}\" w:bottom=\"{MARGIN_TWIPS}\" w:left=\"{MARGIN_TWIPS}\" \
w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/></w:sectPr>"
    )
}
