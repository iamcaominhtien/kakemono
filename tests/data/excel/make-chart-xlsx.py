#!/usr/bin/env python3
"""Writes `charts.xlsx` next to this script: a workbook with charts and pictures.

    python3 -I tests/data/excel/make-chart-xlsx.py

The sheet "Sales" holds a table of sales by quarter and, in its drawing, in
anchor order:

1. a column chart over cells E2:L16 (values shown on the bars);
2. a line chart with markers, given its size in EMU (a one-cell anchor),
   with no title: it is "Chart 2" of the sheet;
3. a group of a text box and a picture;
4. a doughnut of the year's channels (shares shown);
5. a chart whose data is not stored in the file (formulas, no caches).

The sheet "Photos" lists items with a picture placed in a cell of each row
(Excel 365's "Place in cell": `xl/metadata.xml` and `xl/richData/`).

Only the standard library is used, and every byte is fixed (no dates, no
random names), so the file is the same each time it is written.
"""

import struct
import zipfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "charts.xlsx"

# A ZIP entry date (the oldest ZIP can hold): the file does not change.
DATE = (1980, 1, 1, 0, 0, 0)
# English Metric Units in an inch.
EMU_PER_INCH = 914400

SML = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
XDR = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
C = "http://schemas.openxmlformats.org/drawingml/2006/chart"
XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'

QUARTERS = ["Q1", "Q2", "Q3", "Q4"]
NORTH = [120, 135, 150, 170]
SOUTH = [90, 110, 105, 140]


def png(width, height, rgb):
    """A PNG of one colour."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(rgb) * width
    raw = zlib.compress(row * height, 9)
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")


def rels(*items):
    """A relationships part of (id, type, target)."""
    body = "".join(f'<Relationship Id="{i}" Type="{t}" Target="{target}"/>' for i, t, target in items)
    return f'{XML}<Relationships xmlns="{PKG}">{body}</Relationships>'


# The charts.


def cache(kind, values, code=None, ref="Sales!$A$2:$A$5", cached=True):
    """A string (`str`) or number (`num`) reference with its cache."""
    if not cached:
        return f"<c:{kind}Ref><c:f>{ref}</c:f></c:{kind}Ref>"
    points = "".join(f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>' for i, v in enumerate(values))
    fmt = f"<c:formatCode>{code}</c:formatCode>" if code else ""
    return (
        f"<c:{kind}Ref><c:f>{ref}</c:f><c:{kind}Cache>{fmt}"
        f'<c:ptCount val="{len(values)}"/>{points}</c:{kind}Cache></c:{kind}Ref>'
    )


def series(index, name, categories, values, more="", cached=True):
    """A `c:ser` of text categories and numbers."""
    return (
        f'<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx>{cache("str", [name], cached=cached)}</c:tx>{more}'
        f'<c:cat>{cache("str", categories, cached=cached)}</c:cat>'
        f'<c:val>{cache("num", values, "#,##0", cached=cached)}</c:val></c:ser>'
    )


def title(text):
    return f'<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title>'


def chart_space(inside, plot, legend="b"):
    return (
        f'{XML}<c:chartSpace xmlns:c="{C}" xmlns:a="{A}" xmlns:r="{REL}"><c:chart>{inside}'
        f'<c:plotArea><c:layout/>{plot}</c:plotArea><c:legend><c:legendPos val="{legend}"/></c:legend>'
        '<c:plotVisOnly val="1"/></c:chart></c:chartSpace>'
    )


VALUES_ON = '<c:dLbls><c:showLegendKey val="0"/><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="0"/><c:showBubbleSize val="0"/></c:dLbls>'
SHARES_ON = '<c:dLbls><c:showLegendKey val="0"/><c:showVal val="0"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="1"/><c:showBubbleSize val="0"/></c:dLbls>'
AXES = (
    '<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>'
    '<c:axPos val="b"/><c:crossAx val="2"/></c:catAx>'
    '<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>'
    '<c:axPos val="l"/><c:majorGridlines/><c:numFmt formatCode="#,##0" sourceLinked="1"/><c:crossAx val="1"/></c:valAx>'
)
BOTH = (
    series(0, "North", QUARTERS, NORTH, VALUES_ON, cached=True)
    + series(1, "South", QUARTERS, SOUTH, VALUES_ON, cached=True)
)

COLUMNS = chart_space(
    title("Sales by quarter"),
    '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>'
    + BOTH
    + '<c:gapWidth val="80"/><c:axId val="1"/><c:axId val="2"/></c:barChart>'
    + AXES,
)
LINES = chart_space(
    '<c:autoTitleDeleted val="1"/>',
    '<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>'
    + series(0, "North", QUARTERS, NORTH)
    + series(1, "South", QUARTERS, SOUTH)
    + '<c:marker val="1"/><c:axId val="1"/><c:axId val="2"/></c:lineChart>'
    + AXES,
)
DOUGHNUT = chart_space(
    title("Channels"),
    '<c:doughnutChart><c:varyColors val="1"/>'
    + series(0, "Sales", ["Online", "Shops", "Partners"], [45, 35, 20], SHARES_ON)
    + '<c:firstSliceAng val="0"/><c:holeSize val="55"/></c:doughnutChart>',
    legend="r",
)
NO_CACHE = chart_space(
    title("Forecast"),
    '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>'
    + series(0, "Forecast", QUARTERS, NORTH, cached=False)
    + '<c:axId val="1"/><c:axId val="2"/></c:barChart>'
    + AXES,
)

# The drawing of the sheet "Sales".


def marker(name, col, row, col_off=0, row_off=0):
    return (
        f"<xdr:{name}><xdr:col>{col}</xdr:col><xdr:colOff>{col_off}</xdr:colOff>"
        f"<xdr:row>{row}</xdr:row><xdr:rowOff>{row_off}</xdr:rowOff></xdr:{name}>"
    )


def frame(number, rel):
    return (
        f'<xdr:graphicFrame macro=""><xdr:nvGraphicFramePr><xdr:cNvPr id="{number}" name="Chart {number}"/>'
        "<xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>"
        '<xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm>'
        f'<a:graphic><a:graphicData uri="{C}"><c:chart xmlns:c="{C}" r:id="{rel}"/></a:graphicData></a:graphic>'
        "</xdr:graphicFrame>"
    )


def two_cell(start, end, inner):
    return f'<xdr:twoCellAnchor>{marker("from", *start)}{marker("to", *end)}{inner}<xdr:clientData/></xdr:twoCellAnchor>'


def one_cell(start, width_in, height_in, inner):
    cx, cy = int(width_in * EMU_PER_INCH), int(height_in * EMU_PER_INCH)
    return f'<xdr:oneCellAnchor>{marker("from", *start)}<xdr:ext cx="{cx}" cy="{cy}"/>{inner}<xdr:clientData/></xdr:oneCellAnchor>'


GROUP = (
    '<xdr:grpSp><xdr:nvGrpSpPr><xdr:cNvPr id="10" name="Group 10"/><xdr:cNvGrpSpPr/></xdr:nvGrpSpPr><xdr:grpSpPr/>'
    '<xdr:sp macro="" textlink=""><xdr:nvSpPr><xdr:cNvPr id="11" name="TextBox 11"/><xdr:cNvSpPr txBox="1"/></xdr:nvSpPr>'
    '<xdr:spPr/><xdr:txBody><a:bodyPr/><a:p><a:r><a:t>Our logo</a:t></a:r></a:p></xdr:txBody></xdr:sp>'
    '<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="12" name="Picture 12" descr="Logo"/><xdr:cNvPicPr/></xdr:nvPicPr>'
    '<xdr:blipFill><a:blip r:embed="rIdLogo"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill>'
    '<xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/></a:xfrm>'
    '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic></xdr:grpSp>'
)

DRAWING = (
    f'{XML}<xdr:wsDr xmlns:xdr="{XDR}" xmlns:a="{A}" xmlns:r="{REL}">'
    + two_cell((4, 1, 0, 0), (11, 16, 0, 0), frame(2, "rIdChart1"))
    + one_cell((4, 17), 5, 3, frame(3, "rIdChart2"))
    + two_cell((4, 34), (6, 37), GROUP)
    + two_cell((0, 38), (6, 52), frame(4, "rIdChart3"))
    + two_cell((0, 53), (6, 66), frame(5, "rIdChart4"))
    + "</xdr:wsDr>"
)

# The sheets.


def cell(ref, value, style=None):
    s = f' s="{style}"' if style is not None else ""
    if isinstance(value, str):
        return f'<c r="{ref}" t="inlineStr"{s}><is><t>{value}</t></is></c>'
    return f'<c r="{ref}"{s}><v>{value}</v></c>'


def row(number, cells, height=None):
    ht = f' ht="{height}" customHeight="1"' if height else ""
    return f'<row r="{number}"{ht}>{"".join(cells)}</row>'


SALES = (
    f'{XML}<worksheet xmlns="{SML}" xmlns:r="{REL}"><sheetFormatPr defaultRowHeight="15"/>'
    '<cols><col min="1" max="1" width="12.7109375" customWidth="1"/><col min="5" max="12" width="10.7109375" customWidth="1"/></cols>'
    "<sheetData>"
    + row(1, [cell("A1", "Quarter"), cell("B1", "North"), cell("C1", "South")])
    + "".join(
        row(index + 2, [cell(f"A{index + 2}", quarter), cell(f"B{index + 2}", north), cell(f"C{index + 2}", south)])
        for index, (quarter, north, south) in enumerate(zip(QUARTERS, NORTH, SOUTH))
    )
    + "</sheetData>"
    '<drawing r:id="rIdDrawing"/></worksheet>'
)

PHOTOS = (
    f'{XML}<worksheet xmlns="{SML}"><sheetData>'
    + row(1, [cell("A1", "Item"), cell("B1", "Photo"), cell("C1", "Price")])
    + row(2, ['<c r="A2" t="inlineStr"><is><t>Cup</t></is></c>', '<c r="B2" t="e" vm="1"><v>#VALUE!</v></c>', cell("C2", 4.5)], 40)
    + row(3, ['<c r="A3" t="inlineStr"><is><t>Pen</t></is></c>', '<c r="B3" t="e" vm="2"><v>#VALUE!</v></c>', cell("C3", 1.2)], 40)
    + "</sheetData></worksheet>"
)

WORKBOOK = (
    f'{XML}<workbook xmlns="{SML}" xmlns:r="{REL}"><sheets>'
    '<sheet name="Sales" sheetId="1" r:id="rId1"/><sheet name="Photos" sheetId="2" r:id="rId2"/>'
    "</sheets></workbook>"
)

# The pictures placed in cells.

METADATA = (
    f'{XML}<metadata xmlns="{SML}" xmlns:xlrd="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata">'
    '<metadataTypes count="1"><metadataType name="XLRICHVALUE" minSupportedVersion="120000" copy="1" pasteAll="1" '
    'pasteValues="1" merge="1" splitFirst="1" rowColShift="1" clearFormats="1" clearComments="1" assign="1" coerce="1"/>'
    '</metadataTypes><futureMetadata name="XLRICHVALUE" count="2">'
    '<bk><extLst><ext uri="{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}"><xlrd:rvb i="0"/></ext></extLst></bk>'
    '<bk><extLst><ext uri="{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}"><xlrd:rvb i="1"/></ext></extLst></bk>'
    '</futureMetadata><valueMetadata count="2"><bk><rc t="1" v="0"/></bk><bk><rc t="1" v="1"/></bk></valueMetadata>'
    "</metadata>"
)
RICH_VALUES = (
    f'{XML}<rvData xmlns="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata" count="2">'
    '<rv s="0"><v>0</v><v>5</v><v>A blue cup</v></rv><rv s="0"><v>1</v><v>5</v><v>A red pen</v></rv></rvData>'
)
RICH_STRUCTURES = (
    f'{XML}<rvStructures xmlns="http://schemas.microsoft.com/office/spreadsheetml/2017/richdata" count="1">'
    '<s t="_localImage"><k n="_rvRel:LocalImageIdentifier" t="i"/><k n="CalcOrigin" t="i"/><k n="Text" t="s"/></s>'
    "</rvStructures>"
)
RICH_RELS = (
    f'{XML}<richValueRels xmlns="http://schemas.microsoft.com/office/spreadsheetml/2022/richvaluerel" '
    f'xmlns:r="{REL}"><rel r:id="rId1"/><rel r:id="rId2"/></richValueRels>'
)

CONTENT_TYPES = (
    f'{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Default Extension="png" ContentType="image/png"/>'
    '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
    '<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
    '<Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
    '<Override PartName="/xl/drawings/drawing1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>'
    + "".join(
        f'<Override PartName="/xl/charts/chart{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>'
        for n in range(1, 5)
    )
    + '<Override PartName="/xl/metadata.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml"/>'
    '<Override PartName="/xl/richData/rdrichvalue.xml" ContentType="application/vnd.ms-excel.rdrichvalue+xml"/>'
    '<Override PartName="/xl/richData/rdrichvaluestructure.xml" ContentType="application/vnd.ms-excel.rdrichvaluestructure+xml"/>'
    '<Override PartName="/xl/richData/richValueRel.xml" ContentType="application/vnd.ms-excel.richvaluerel+xml"/>'
    "</Types>"
)


def main():
    rich = "http://schemas.microsoft.com/office/2017/06/relationships"
    parts = [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", rels(("rId1", f"{REL}/officeDocument", "xl/workbook.xml"))),
        ("xl/workbook.xml", WORKBOOK),
        (
            "xl/_rels/workbook.xml.rels",
            rels(
                ("rId1", f"{REL}/worksheet", "worksheets/sheet1.xml"),
                ("rId2", f"{REL}/worksheet", "worksheets/sheet2.xml"),
                ("rId3", f"{REL}/sheetMetadata", "metadata.xml"),
                ("rId4", f"{rich}/rdRichValue", "richData/rdrichvalue.xml"),
                ("rId5", f"{rich}/rdRichValueStructure", "richData/rdrichvaluestructure.xml"),
                ("rId6", "http://schemas.microsoft.com/office/2022/10/relationships/richValueRel", "richData/richValueRel.xml"),
            ),
        ),
        ("xl/worksheets/sheet1.xml", SALES),
        ("xl/worksheets/_rels/sheet1.xml.rels", rels(("rIdDrawing", f"{REL}/drawing", "../drawings/drawing1.xml"))),
        ("xl/worksheets/sheet2.xml", PHOTOS),
        ("xl/drawings/drawing1.xml", DRAWING),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            rels(
                ("rIdChart1", f"{REL}/chart", "../charts/chart1.xml"),
                ("rIdChart2", f"{REL}/chart", "../charts/chart2.xml"),
                ("rIdChart3", f"{REL}/chart", "../charts/chart3.xml"),
                ("rIdChart4", f"{REL}/chart", "../charts/chart4.xml"),
                ("rIdLogo", f"{REL}/image", "../media/logo.png"),
            ),
        ),
        ("xl/charts/chart1.xml", COLUMNS),
        ("xl/charts/chart2.xml", LINES),
        ("xl/charts/chart3.xml", DOUGHNUT),
        ("xl/charts/chart4.xml", NO_CACHE),
        ("xl/metadata.xml", METADATA),
        ("xl/richData/rdrichvalue.xml", RICH_VALUES),
        ("xl/richData/rdrichvaluestructure.xml", RICH_STRUCTURES),
        ("xl/richData/richValueRel.xml", RICH_RELS),
        (
            "xl/richData/_rels/richValueRel.xml.rels",
            rels(
                ("rId1", f"{REL}/image", "../media/cup.png"),
                ("rId2", f"{REL}/image", "../media/pen.png"),
            ),
        ),
        ("xl/media/logo.png", png(32, 16, (0x2A, 0x6F, 0x97))),
        ("xl/media/cup.png", png(24, 24, (0x1F, 0x77, 0xB4))),
        ("xl/media/pen.png", png(24, 24, (0xD6, 0x27, 0x28))),
    ]
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in parts:
            info = zipfile.ZipInfo(name, DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, content.encode() if isinstance(content, str) else content)


if __name__ == "__main__":
    main()
