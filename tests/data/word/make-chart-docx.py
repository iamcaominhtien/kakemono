#!/usr/bin/env python3
"""Writes `chart.docx` next to this script: a Word document with charts.

    python3 -I tests/data/word/make-chart-docx.py

The document holds, in this order:

1. a column chart in line with the text (two series, values shown on the
   bars), with a Word caption below it;
2. a doughnut chart anchored to its paragraph, with a title of its own and
   the shares of its slices shown, coloured by the document's theme;
3. an extended chart (a waterfall, `cx:`) in an `mc:AlternateContent` whose
   fallback is a picture: the picture is shown;
4. an extended chart with no fallback: its title and its data only.

Only the standard library is used, and every byte is fixed (no dates, no
random names), so the file is the same each time it is written.
"""

import struct
import zipfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "chart.docx"

# A ZIP entry date (the oldest ZIP can hold): the file does not change.
DATE = (1980, 1, 1, 0, 0, 0)
# English Metric Units in an inch.
EMU_PER_INCH = 914400

C_NS = "http://schemas.openxmlformats.org/drawingml/2006/chart"
CX_NS = "http://schemas.microsoft.com/office/drawing/2014/chartex"
A_NS = "http://schemas.openxmlformats.org/drawingml/2006/main"
R_NS = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def png(width, height, rgb):
    """A PNG of one colour."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(rgb) * width
    raw = zlib.compress(row * height, 9)
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")


def cache(kind, values, code=None):
    """A string (`str`) or number (`num`) reference with its cache."""
    points = "".join(f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>' for i, v in enumerate(values))
    fmt = f"<c:formatCode>{code}</c:formatCode>" if code else ""
    return (
        f"<c:{kind}Ref><c:f>Sheet1!$A$1</c:f><c:{kind}Cache>{fmt}"
        f'<c:ptCount val="{len(values)}"/>{points}</c:{kind}Cache></c:{kind}Ref>'
    )


def series(index, name, categories, values, more=""):
    """A `c:ser` of cached text categories and numbers."""
    return (
        f'<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx>{cache("str", [name])}</c:tx>{more}'
        f'<c:cat>{cache("str", categories)}</c:cat><c:val>{cache("num", values, "#,##0")}</c:val></c:ser>'
    )


def title(text):
    return f"<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>{text}</a:t></a:r></a:p></c:rich></c:tx><c:overlay val=\"0\"/></c:title>"


def chart_space(inside, plot, legend="b"):
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        f'<c:chartSpace xmlns:c="{C_NS}" xmlns:a="{A_NS}" xmlns:r="{R_NS}"><c:chart>{inside}'
        f'<c:plotArea><c:layout/>{plot}</c:plotArea><c:legend><c:legendPos val="{legend}"/></c:legend>'
        '<c:plotVisOnly val="1"/></c:chart></c:chartSpace>'
    )


QUARTERS = ["Q1", "Q2", "Q3", "Q4"]
VALUES_ON = '<c:dLbls><c:showLegendKey val="0"/><c:showVal val="1"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="0"/><c:showBubbleSize val="0"/></c:dLbls>'
SHARES_ON = '<c:dLbls><c:showLegendKey val="0"/><c:showVal val="0"/><c:showCatName val="0"/><c:showSerName val="0"/><c:showPercent val="1"/><c:showBubbleSize val="0"/></c:dLbls>'
AXES = (
    '<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>'
    '<c:axPos val="b"/><c:crossAx val="2"/></c:catAx>'
    '<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>'
    '<c:axPos val="l"/><c:majorGridlines/><c:numFmt formatCode="#,##0" sourceLinked="0"/><c:crossAx val="1"/></c:valAx>'
)

COLUMNS = chart_space(
    '<c:autoTitleDeleted val="1"/>',
    '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>'
    + series(0, "North", QUARTERS, [120, 135, 150, 170], VALUES_ON)
    + series(1, "South", QUARTERS, [90, 110, 105, 140], VALUES_ON)
    + '<c:gapWidth val="80"/><c:axId val="1"/><c:axId val="2"/></c:barChart>'
    + AXES,
)

DOUGHNUT = chart_space(
    title("Share of sales"),
    '<c:doughnutChart><c:varyColors val="1"/>'
    + series(0, "Sales", ["Online", "Shops", "Partners"], [45, 35, 20], SHARES_ON)
    + '<c:firstSliceAng val="0"/><c:holeSize val="55"/></c:doughnutChart>',
    legend="r",
)


def chartex(text):
    """An extended chart part: a waterfall with its data."""
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        f'<cx:chartSpace xmlns:cx="{CX_NS}" xmlns:a="{A_NS}" xmlns:r="{R_NS}"><cx:chartData>'
        '<cx:data id="0"><cx:strDim type="cat"><cx:f>Sheet1!$A$2:$A$4</cx:f><cx:lvl ptCount="3">'
        '<cx:pt idx="0">Start</cx:pt><cx:pt idx="1">Gain</cx:pt><cx:pt idx="2">Loss</cx:pt></cx:lvl></cx:strDim>'
        '<cx:numDim type="val"><cx:f>Sheet1!$B$2:$B$4</cx:f><cx:lvl ptCount="3" formatCode="General">'
        '<cx:pt idx="0">100</cx:pt><cx:pt idx="1">40</cx:pt><cx:pt idx="2">-25</cx:pt></cx:lvl></cx:numDim></cx:data>'
        f'</cx:chartData><cx:chart><cx:title><cx:tx><cx:txData><cx:v>{text}</cx:v></cx:txData></cx:tx></cx:title>'
        '<cx:plotArea><cx:plotAreaRegion><cx:series layoutId="waterfall"><cx:tx><cx:txData><cx:v>Cash</cx:v></cx:txData></cx:tx>'
        '<cx:dataId val="0"/></cx:series></cx:plotAreaRegion></cx:plotArea></cx:chart></cx:chartSpace>'
    )


THEME = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    f'<a:theme xmlns:a="{A_NS}" name="Sample"><a:themeElements><a:clrScheme name="Sample">'
    '<a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1>'
    '<a:dk2><a:srgbClr val="1F2937"/></a:dk2><a:lt2><a:srgbClr val="F3F4F6"/></a:lt2>'
    '<a:accent1><a:srgbClr val="2A6F97"/></a:accent1><a:accent2><a:srgbClr val="E07A5F"/></a:accent2>'
    '<a:accent3><a:srgbClr val="81B29A"/></a:accent3><a:accent4><a:srgbClr val="F2CC8F"/></a:accent4>'
    '<a:accent5><a:srgbClr val="3D405B"/></a:accent5><a:accent6><a:srgbClr val="9C6644"/></a:accent6>'
    '<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>'
    '</a:clrScheme><a:fontScheme name="Sample"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
    '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme>'
    '<a:fmtScheme name="Sample"/></a:themeElements></a:theme>'
)


def graphic(number, name, uri, inner):
    return (
        f'<wp:docPr id="{number}" name="{name}"/>'
        f'<a:graphic><a:graphicData uri="{uri}">{inner}</a:graphicData></a:graphic>'
    )


def chart_graphic(number, rel):
    return graphic(number, f"Chart {number}", C_NS, f'<c:chart xmlns:c="{C_NS}" r:id="{rel}"/>')


def chartex_graphic(number, rel):
    return graphic(number, f"Chart {number}", CX_NS, f'<cx:chart xmlns:cx="{CX_NS}" r:id="{rel}"/>')


def picture_graphic(number, rel, cx, cy):
    pic_ns = "http://schemas.openxmlformats.org/drawingml/2006/picture"
    return graphic(
        number,
        f"Picture {number}",
        pic_ns,
        f'<pic:pic xmlns:pic="{pic_ns}"><pic:nvPicPr><pic:cNvPr id="{number}" name="Picture {number}"/><pic:cNvPicPr/></pic:nvPicPr>'
        f'<pic:blipFill><a:blip r:embed="{rel}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
        f'<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>',
    )


def inline(width_in, height_in, inner):
    """A `w:drawing` in line with the text."""
    cx, cy = int(width_in * EMU_PER_INCH), int(height_in * EMU_PER_INCH)
    return f'<w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/>{inner}</wp:inline></w:drawing>'


def anchored(width_in, height_in, inner):
    """A `w:drawing` anchored to its paragraph, text above and below it."""
    cx, cy = int(width_in * EMU_PER_INCH), int(height_in * EMU_PER_INCH)
    return (
        '<w:drawing><wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="1" '
        'behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/>'
        '<wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH>'
        '<wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>'
        f'<wp:extent cx="{cx}" cy="{cy}"/><wp:wrapTopAndBottom/>{inner}</wp:anchor></w:drawing>'
    )


def alternate(choice, fallback):
    alt = (
        '<mc:AlternateContent><mc:Choice Requires="cx1">'
        f"{choice}</mc:Choice>"
    )
    if fallback:
        alt += f"<mc:Fallback>{fallback}</mc:Fallback>"
    return alt + "</mc:AlternateContent>"


def para(inner, style=None):
    props = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f"<w:p>{props}{inner}</w:p>"


def run(inner):
    return f"<w:r>{inner}</w:r>"


def text(value):
    return run(f'<w:t xml:space="preserve">{value}</w:t>')


DOCUMENT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    f'xmlns:r="{R_NS}" '
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    f'xmlns:a="{A_NS}" '
    'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="">'
    "<w:body>"
    + para(text("Charts"), "Title")
    + para(text("Sales by quarter, in units, for the two regions."))
    + para(run(inline(5.5, 3, chart_graphic(1, "rIdChart1"))))
    + para(text("Figure 1: Sales by quarter."), "Caption")
    + para(text("The share of each channel."))
    + para(run(anchored(4, 2.5, chart_graphic(2, "rIdChart2"))))
    + para(text("Cash over the year, as a waterfall with a picture for older readers."))
    + para(
        run(
            alternate(
                inline(4, 2.5, chartex_graphic(3, "rIdChartEx1")),
                inline(4, 2.5, picture_graphic(4, "rIdFallback", int(4 * EMU_PER_INCH), int(2.5 * EMU_PER_INCH))),
            )
        )
    )
    + para(text("The same waterfall with no picture beside it."))
    + para(run(alternate(inline(4, 2.5, chartex_graphic(5, "rIdChartEx2")), None)))
    + para(text("The end."))
    + '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
    '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/>'
    "</w:sectPr></w:body></w:document>"
)

STYLES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">'
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
    '<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/>'
    '<w:rPr><w:b/><w:sz w:val="40"/></w:rPr></w:style>'
    '<w:style w:type="paragraph" w:styleId="Caption"><w:name w:val="caption"/><w:basedOn w:val="Normal"/>'
    '<w:rPr><w:i/></w:rPr></w:style>'
    "</w:styles>"
)

CONTENT_TYPES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Default Extension="png" ContentType="image/png"/>'
    '<Override PartName="/word/document.xml" '
    'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    '<Override PartName="/word/styles.xml" '
    'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    '<Override PartName="/word/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>'
    '<Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>'
    '<Override PartName="/word/charts/chart2.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/>'
    '<Override PartName="/word/charts/chartEx1.xml" ContentType="application/vnd.ms-office.chartex+xml"/>'
    '<Override PartName="/word/charts/chartEx2.xml" ContentType="application/vnd.ms-office.chartex+xml"/>'
    "</Types>"
)

PACKAGE_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" '
    'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
    'Target="word/document.xml"/></Relationships>'
)

REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
DOCUMENT_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    f'<Relationship Id="rIdStyles" Type="{REL}/styles" Target="styles.xml"/>'
    f'<Relationship Id="rIdTheme" Type="{REL}/theme" Target="theme/theme1.xml"/>'
    f'<Relationship Id="rIdChart1" Type="{REL}/chart" Target="charts/chart1.xml"/>'
    f'<Relationship Id="rIdChart2" Type="{REL}/chart" Target="charts/chart2.xml"/>'
    '<Relationship Id="rIdChartEx1" Type="http://schemas.microsoft.com/office/2014/relationships/chartEx" Target="charts/chartEx1.xml"/>'
    '<Relationship Id="rIdChartEx2" Type="http://schemas.microsoft.com/office/2014/relationships/chartEx" Target="charts/chartEx2.xml"/>'
    f'<Relationship Id="rIdFallback" Type="{REL}/image" Target="media/fallback.png"/>'
    "</Relationships>"
)


def main():
    parts = [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", PACKAGE_RELS),
        ("word/document.xml", DOCUMENT),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS),
        ("word/styles.xml", STYLES),
        ("word/theme/theme1.xml", THEME),
        ("word/charts/chart1.xml", COLUMNS),
        ("word/charts/chart2.xml", DOUGHNUT),
        ("word/charts/chartEx1.xml", chartex("Cash")),
        ("word/charts/chartEx2.xml", chartex("Cash again")),
        ("word/media/fallback.png", png(40, 25, (0x9C, 0x66, 0x44))),
    ]
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in parts:
            info = zipfile.ZipInfo(name, DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, content.encode() if isinstance(content, str) else content)


if __name__ == "__main__":
    main()
