"""Writes `sample.pptx` next to this script: a presentation with what the
PowerPoint reader must handle (a title slide, sections, bullets on three
levels, numbered lists, a hidden slide, a slide without a title, a table with
merged cells, pictures, a chart, a formula, SmartArt, a group, speaker notes,
footers and slide numbers, an empty placeholder).

Run it with `python3 -I tests/data/powerpoint/make-sample-pptx.py`: only the
standard library is used, every part is written as XML, and the archive is
the same on every run.
"""

import struct
import zipfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "sample.pptx"
# A fixed date for the entries, so that the archive is the same on every run.
DATE = (1980, 1, 1, 0, 0, 0)
EMU_PER_INCH = 914400
# A 16:9 slide, 13.333 × 7.5 inches.
SLIDE_CX = 12192000
SLIDE_CY = 6858000

P = "http://schemas.openxmlformats.org/presentationml/2006/main"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
C = "http://schemas.openxmlformats.org/drawingml/2006/chart"
DGM = "http://schemas.openxmlformats.org/drawingml/2006/diagram"
DSP = "http://schemas.microsoft.com/office/drawing/2008/diagram"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
A14 = "http://schemas.microsoft.com/office/drawing/2010/main"
M = "http://schemas.openxmlformats.org/officeDocument/2006/math"
P14 = "http://schemas.microsoft.com/office/powerpoint/2010/main"
ASVG = "http://schemas.microsoft.com/office/drawing/2016/SVG/main"
A16 = "http://schemas.microsoft.com/office/drawing/2014/main"
ADEC = "http://schemas.microsoft.com/office/drawing/2017/decorative"
XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
NS = f'xmlns:a="{A}" xmlns:r="{R}" xmlns:p="{P}"'
REL_TYPES = {
    "officeDocument": f"{R}/officeDocument",
    "slide": f"{R}/slide",
    "slideLayout": f"{R}/slideLayout",
    "slideMaster": f"{R}/slideMaster",
    "theme": f"{R}/theme",
    "notesSlide": f"{R}/notesSlide",
    "notesMaster": f"{R}/notesMaster",
    "image": f"{R}/image",
    "chart": f"{R}/chart",
    "hyperlink": f"{R}/hyperlink",
    "diagramData": f"{R}/diagramData",
    "diagramLayout": f"{R}/diagramLayout",
    "diagramQuickStyle": f"{R}/diagramQuickStyle",
    "diagramColors": f"{R}/diagramColors",
    "diagramDrawing": "http://schemas.microsoft.com/office/2007/relationships/diagramDrawing",
    "core": "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties",
}


def png(width, height, rgb):
    """A PNG of one colour."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(rgb) * width
    raw = zlib.compress(row * height, 9)
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")


SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">'
    '<circle cx="32" cy="32" r="28" fill="#2A6F97"/></svg>'
)


def rels(*items):
    """A relationships part of (id, type name, target[, external])."""
    body = ""
    for item in items:
        rid, kind, target = item[:3]
        mode = ' TargetMode="External"' if len(item) > 3 and item[3] else ""
        body += f'<Relationship Id="{rid}" Type="{REL_TYPES[kind]}" Target="{target}"{mode}/>'
    return f'{XML}<Relationships xmlns="{PKG}">{body}</Relationships>'


def emu(inches):
    return round(inches * EMU_PER_INCH)


def xfrm(x, y, cx, cy, tag="a:xfrm"):
    """A position and size, in inches."""
    return f'<{tag}><a:off x="{emu(x)}" y="{emu(y)}"/><a:ext cx="{emu(cx)}" cy="{emu(cy)}"/></{tag}>'


# The theme, master and layouts.

THEME = (
    f'{XML}<a:theme xmlns:a="{A}" name="Sample"><a:themeElements>'
    '<a:clrScheme name="Sample">'
    '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>'
    '<a:dk2><a:srgbClr val="1F3B4D"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>'
    '<a:accent1><a:srgbClr val="2A6F97"/></a:accent1><a:accent2><a:srgbClr val="E07A5F"/></a:accent2>'
    '<a:accent3><a:srgbClr val="81B29A"/></a:accent3><a:accent4><a:srgbClr val="F2CC8F"/></a:accent4>'
    '<a:accent5><a:srgbClr val="3D405B"/></a:accent5><a:accent6><a:srgbClr val="6D597A"/></a:accent6>'
    '<a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>'
    "</a:clrScheme>"
    '<a:fontScheme name="Sample">'
    '<a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
    '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>'
    "</a:fontScheme>"
    '<a:fmtScheme name="Sample">'
    '<a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst>'
    '<a:lnStyleLst><a:ln w="6350"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="12700"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln><a:ln w="19050"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln></a:lnStyleLst>'
    "<a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst>"
    '<a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst>'
    "</a:fmtScheme></a:themeElements></a:theme>"
)


def nv(pid, name, ph=None, kind="sp", extra=""):
    """The non-visual properties of a shape, with its placeholder."""
    ph_xml = f"<p:ph {ph}/>" if ph else ""
    lock = '<a:spLocks noGrp="1"/>' if ph else ""
    tag = {"sp": "p:nvSpPr", "pic": "p:nvPicPr", "frame": "p:nvGraphicFramePr", "grp": "p:nvGrpSpPr"}[kind]
    inner = {"sp": "p:cNvSpPr", "pic": "p:cNvPicPr", "frame": "p:cNvGraphicFramePr", "grp": "p:cNvGrpSpPr"}[kind]
    return f'<{tag}><p:cNvPr id="{pid}" name="{name}"{extra}/><{inner}>{lock}</{inner}><p:nvPr>{ph_xml}</p:nvPr></{tag}>'


def placeholder(pid, name, ph, box=None, body="<a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr/></a:p>"):
    """A placeholder shape (`box`: x, y, width, height in inches, or None to inherit)."""
    sp_pr = f"<p:spPr>{xfrm(*box)}</p:spPr>" if box else "<p:spPr/>"
    return f"<p:sp>{nv(pid, name, ph)}{sp_pr}<p:txBody>{body}</p:txBody></p:sp>"


def prompt(text):
    """The body of a layout or master placeholder: its prompt text."""
    return f'<a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r></a:p>'


def tree(shapes):
    return (
        '<p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
        '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
        f"{shapes}</p:spTree></p:cSld>"
    )


def level(n, size, bullet="", indent=None):
    mar = f' marL="{emu(0.25 * n)}" indent="{-emu(0.25)}"' if indent is None else indent
    return (
        f'<a:lvl{n}pPr{mar} algn="l"><a:spcBef><a:spcPts val="1000"/></a:spcBef>{bullet}'
        f'<a:defRPr sz="{size}"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl{n}pPr>'
    )


BULLET = '<a:buFont typeface="Arial"/><a:buChar char="&#8226;"/>'
TITLE_STYLE = (
    '<p:titleStyle><a:lvl1pPr algn="l"><a:lnSpc><a:spcPct val="90000"/></a:lnSpc><a:buNone/>'
    '<a:defRPr sz="4400"><a:solidFill><a:schemeClr val="tx1"/></a:solidFill><a:latin typeface="+mj-lt"/></a:defRPr></a:lvl1pPr></p:titleStyle>'
)
BODY_STYLE = "<p:bodyStyle>" + "".join(level(n, size, BULLET) for n, size in [(1, 2800), (2, 2400), (3, 2000)]) + "</p:bodyStyle>"
OTHER_STYLE = '<p:otherStyle><a:lvl1pPr><a:defRPr sz="1800"/></a:lvl1pPr></p:otherStyle>'

MASTER = (
    f'{XML}<p:sldMaster {NS}>'
    '<p:cSld><p:bg><p:bgRef idx="1001"><a:schemeClr val="bg1"/></p:bgRef></p:bg><p:spTree>'
    '<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>'
    + placeholder(2, "Title Placeholder 1", 'type="title"', (0.92, 0.4, 11.5, 1.45), prompt("Click to edit Master title style"))
    + placeholder(3, "Text Placeholder 2", 'type="body" idx="1"', (0.92, 2.0, 11.5, 4.75), prompt("Click to edit Master text styles"))
    + placeholder(4, "Date Placeholder 3", 'type="dt" sz="half" idx="2"', (0.92, 6.95, 3.0, 0.4), '<a:bodyPr/><a:lstStyle/><a:p><a:fld id="{B5C0F1E2-0000-4000-8000-000000000001}" type="datetimeFigureOut"><a:rPr lang="en-US"/><a:t>11/28/2026</a:t></a:fld></a:p>')
    + placeholder(5, "Footer Placeholder 4", 'type="ftr" sz="quarter" idx="3"', (4.42, 6.95, 4.5, 0.4))
    + placeholder(6, "Slide Number Placeholder 5", 'type="sldNum" sz="quarter" idx="4"', (9.42, 6.95, 3.0, 0.4), '<a:bodyPr/><a:lstStyle/><a:p><a:fld id="{B5C0F1E2-0000-4000-8000-000000000002}" type="slidenum"><a:rPr lang="en-US"/><a:t>&#8249;#&#8250;</a:t></a:fld></a:p>')
    # A band of colour on every slide whose layout shows the master's shapes.
    + f'<p:sp>{nv(7, "Band")}<p:spPr>{xfrm(0, 7.3, 13.333, 0.2)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent1"/></a:solidFill><a:ln><a:noFill/></a:ln></p:spPr></p:sp>'
    + "</p:spTree></p:cSld>"
    '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
    '<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/><p:sldLayoutId id="2147483650" r:id="rId2"/>'
    '<p:sldLayoutId id="2147483651" r:id="rId3"/><p:sldLayoutId id="2147483652" r:id="rId4"/></p:sldLayoutIdLst>'
    f"<p:txStyles>{TITLE_STYLE}{BODY_STYLE}{OTHER_STYLE}</p:txStyles></p:sldMaster>"
)


def layout(kind, name, shapes, show_master=True):
    show = "" if show_master else ' showMasterSp="0"'
    return f'{XML}<p:sldLayout {NS} type="{kind}" preserve="1"{show}><p:cSld name="{name}">' + tree(shapes)[len("<p:cSld>"):] + "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"


LAYOUT_TITLE = layout(
    "title",
    "Title Slide",
    placeholder(2, "Title 1", 'type="ctrTitle"', (1.67, 1.23, 10.0, 2.6), '<a:bodyPr anchor="b"/><a:lstStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="6000"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master title style</a:t></a:r></a:p>')
    + placeholder(3, "Subtitle 2", 'type="subTitle" idx="1"', (1.67, 3.92, 10.0, 1.8), '<a:bodyPr/><a:lstStyle><a:lvl1pPr marL="0" indent="0" algn="ctr"><a:buNone/><a:defRPr sz="2400"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master subtitle style</a:t></a:r></a:p>')
    + placeholder(4, "Date Placeholder 3", 'type="dt" sz="half" idx="10"')
    + placeholder(5, "Footer Placeholder 4", 'type="ftr" sz="quarter" idx="11"')
    + placeholder(6, "Slide Number Placeholder 5", 'type="sldNum" sz="quarter" idx="12"'),
    show_master=False,
)
LAYOUT_CONTENT = layout(
    "obj",
    "Title and Content",
    placeholder(2, "Title 1", 'type="title"', None, prompt("Click to edit Master title style"))
    + placeholder(3, "Content Placeholder 2", 'idx="1"', None, prompt("Click to edit Master text styles"))
    + placeholder(4, "Date Placeholder 3", 'type="dt" sz="half" idx="10"')
    + placeholder(5, "Footer Placeholder 4", 'type="ftr" sz="quarter" idx="11"')
    + placeholder(6, "Slide Number Placeholder 5", 'type="sldNum" sz="quarter" idx="12"'),
)
LAYOUT_SECTION = layout(
    "secHead",
    "Section Header",
    placeholder(2, "Title 1", 'type="title"', (0.92, 1.7, 11.5, 2.85), prompt("Click to edit Master title style"))
    + placeholder(
        3,
        "Text Placeholder 2",
        'type="body" idx="1"',
        (0.92, 4.59, 11.5, 1.5),
        '<a:bodyPr/><a:lstStyle><a:lvl1pPr marL="0" indent="0"><a:buNone/><a:defRPr sz="2400"/></a:lvl1pPr></a:lstStyle><a:p><a:r><a:rPr lang="en-US"/><a:t>Click to edit Master text styles</a:t></a:r></a:p>',
    ),
)
LAYOUT_BLANK = layout("blank", "Blank", placeholder(2, "Slide Number Placeholder 1", 'type="sldNum" sz="quarter" idx="12"'))


# The slides.


def run(text, props=""):
    return f'<a:r><a:rPr lang="en-US" dirty="0"{props}/><a:t>{text}</a:t></a:r>'


def para(content, lvl=0, ppr_extra=""):
    lvl_attr = f' lvl="{lvl}"' if lvl else ""
    ppr = f"<a:pPr{lvl_attr}>{ppr_extra}</a:pPr>" if lvl or ppr_extra else ""
    return f"<a:p>{ppr}{content}</a:p>"


def body(*paras, props="<a:bodyPr/>"):
    return f"{props}<a:lstStyle/>" + "".join(paras)


def title_shape(text, pid=2):
    return placeholder(pid, "Title 1", 'type="title"', None, body(para(run(text))))


def text_box(pid, name, box, *paras, extra=""):
    return (
        f'<p:sp>{nv(pid, name)}<p:spPr>{xfrm(*box)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:noFill/></p:spPr>'
        f'<p:txBody>{body(*paras, props=f"<a:bodyPr wrap=\"square\">{extra}</a:bodyPr>")}</p:txBody></p:sp>'
    )


def slide(shapes, show=True):
    hidden = "" if show else ' show="0"'
    return f"{XML}<p:sld {NS}{hidden}>" + tree(shapes) + "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"


SLIDE_TITLE = slide(
    placeholder(2, "Title 1", 'type="ctrTitle"', None, body(para(run("Coral Sea Survey 2026"))))
    + placeholder(3, "Subtitle 2", 'type="subTitle" idx="1"', None, body(para(run("Marine Science Team")), para(run("Quarterly review"))))
)

SLIDE_GOALS = slide(
    title_shape("Goals of the survey")
    + placeholder(
        3,
        "Content Placeholder 2",
        'idx="1"',
        None,
        body(
            para(run("Measure coral cover at ") + run("twelve", ' b="1"') + run(" reefs")),
            para(run("Compare with the 2024 baseline"), lvl=1),
            para(run("Use the same transects"), lvl=2),
            para(run("Publish the data on the ") + '<a:r><a:rPr lang="en-US"><a:hlinkClick r:id="rId3"/></a:rPr><a:t>open data portal</a:t></a:r>'),
            para(run("H") + run("2", ' baseline="-25000"') + run("O temperature at 10") + run("2", ' baseline="30000"') + run(" m depth")),
        ),
    )
    + placeholder(4, "Footer Placeholder 3", 'type="ftr" sz="quarter" idx="11"', None, body(para(run("Coral Sea Survey · Confidential"))))
    + placeholder(5, "Slide Number Placeholder 4", 'type="sldNum" sz="quarter" idx="12"', None, body(para('<a:fld id="{B5C0F1E2-0000-4000-8000-000000000003}" type="slidenum"><a:rPr lang="en-US"/><a:t>2</a:t></a:fld>')))
)

AUTONUM = '<a:buFont typeface="+mj-lt"/><a:buAutoNum type="{kind}"{start}/>'


def numbered(text, kind, lvl=0, start=None):
    start_attr = f' startAt="{start}"' if start else ""
    mar = f' marL="{emu(0.4 + 0.4 * lvl)}" indent="{-emu(0.4)}"'
    ppr = f"<a:pPr{mar}" + (f' lvl="{lvl}"' if lvl else "") + ">" + AUTONUM.format(kind=kind, start=start_attr) + "</a:pPr>"
    return f"<a:p>{ppr}{run(text)}</a:p>"


SLIDE_STEPS = slide(
    title_shape("Steps of a dive")
    + placeholder(
        3,
        "Content Placeholder 2",
        'idx="1"',
        None,
        body(
            numbered("Check the equipment", "arabicPeriod", start=3),
            numbered("Tanks", "alphaLcParenR", lvl=1),
            numbered("Cameras", "alphaLcParenR", lvl=1),
            numbered("Lay the transect line", "arabicPeriod", start=3),
            numbered("Photograph every metre", "arabicPeriod", start=3),
            para(run("No bullet on this closing line"), ppr_extra="<a:buNone/>"),
        ),
    )
    # A footer known only by its index: its type is its layout's.
    + placeholder(4, "Footer Placeholder 3", 'idx="11"', None, body(para(run("Dive manual, second edition"))))
)

SLIDE_HIDDEN = slide(title_shape("Draft ideas, hidden") + text_box(3, "TextBox 2", (1, 2, 8, 1), para(run("This slide is hidden and left out."))), show=False)

SLIDE_SECTION = slide(
    title_shape("Results")
    + placeholder(3, "Text Placeholder 2", 'type="body" idx="1"', None, body(para(run("What the dives found"))))
)


def pic(pid, name, rid, box, descr, svg_rid=None, decorative=False):
    ext = ""
    if svg_rid:
        ext = f'<a:extLst><a:ext uri="{{96DAC541-7B7A-43D3-8B79-37D633B846F1}}"><asvg:svgBlip xmlns:asvg="{ASVG}" r:embed="{svg_rid}"/></a:ext></a:extLst>'
    deco = ""
    if decorative:
        deco = f'><a:extLst><a:ext uri="{{C183D7F6-B498-43B3-948B-1728B52AA6E4}}"><adec:decorative xmlns:adec="{ADEC}" val="1"/></a:ext></a:extLst></p:cNvPr'
        nv_xml = f'<p:nvPicPr><p:cNvPr id="{pid}" name="{name}" descr="{descr}"{deco}><p:cNvPicPr/><p:nvPr/></p:nvPicPr>'
    else:
        nv_xml = nv(pid, name, kind="pic", extra=f' descr="{descr}"')
    return (
        f'<p:pic>{nv_xml}<p:blipFill><a:blip r:embed="{rid}">{ext}</a:blip><a:stretch><a:fillRect/></a:stretch></p:blipFill>'
        f'<p:spPr>{xfrm(*box)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr></p:pic>'
    )


SLIDE_NO_TITLE = slide(
    text_box(2, "TextBox 1", (0.8, 0.6, 7, 1.2), para(run("A slide without a title, with a photograph of a reef and a logo.")))
    + pic(3, "Picture 2", "rId2", (0.8, 2.0, 5.0, 3.0), "A reef seen from above")
    + pic(4, "Picture 3", "rId3", (7.0, 2.0, 2.0, 2.0), "The team's logo", svg_rid="rId4")
    + pic(5, "Picture 4", "rId2", (11.5, 0.3, 1.0, 0.5), "", decorative=True)
    # An empty placeholder of the layout is not on this blank slide, but an
    # empty text box with no text is: neither gives anything.
    + text_box(6, "TextBox 5", (1, 6, 4, 0.5), "<a:p><a:endParaRPr lang=\"en-US\"/></a:p>")
)


def cell(text, extra="", props=""):
    content = para(run(text, props)) if text is not None else "<a:p><a:endParaRPr/></a:p>"
    return f"<a:tc{extra}><a:txBody><a:bodyPr/><a:lstStyle/>{content}</a:txBody><a:tcPr/></a:tc>"


TABLE = (
    '<a:tbl><a:tblPr firstRow="1" bandRow="1"><a:tableStyleId>{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}</a:tableStyleId></a:tblPr>'
    '<a:tblGrid><a:gridCol w="2743200"/><a:gridCol w="2743200"/><a:gridCol w="2743200"/></a:tblGrid>'
    '<a:tr h="457200">' + cell("Site") + cell("Coral cover", ' gridSpan="2"') + cell(None, ' hMerge="1"') + "</a:tr>"
    '<a:tr h="457200">' + cell("") + cell("2024") + cell("2026") + "</a:tr>"
    '<a:tr h="457200">' + cell("North reef", ' rowSpan="2"') + cell("31%") + cell("34%") + "</a:tr>"
    '<a:tr h="457200">' + cell(None, ' vMerge="1"') + cell("29%") + cell("33%") + "</a:tr>"
    '<a:tr h="457200">' + cell("South reef") + cell("22%") + cell("19%") + "</a:tr>"
    "</a:tbl>"
)

SLIDE_TABLE = slide(
    title_shape("Results by site")
    + f'<p:graphicFrame>{nv(3, "Table 2", kind="frame")}{xfrm(1.5, 2.0, 9.0, 2.5, "p:xfrm")}'
    f'<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table">{TABLE}</a:graphicData></a:graphic></p:graphicFrame>'
)

YEARS = ["2022", "2024", "2026"]


def cache(kind, values, code=None):
    points = "".join(f'<c:pt idx="{i}"><c:v>{v}</c:v></c:pt>' for i, v in enumerate(values))
    fmt = f"<c:formatCode>{code}</c:formatCode>" if code else ""
    return f'<c:{kind}Ref><c:f>Sheet1!$A$2:$A$4</c:f><c:{kind}Cache>{fmt}<c:ptCount val="{len(values)}"/>{points}</c:{kind}Cache></c:{kind}Ref>'


def series(index, name, values):
    return (
        f'<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx>{cache("str", [name])}</c:tx>'
        f'<c:cat>{cache("str", YEARS)}</c:cat><c:val>{cache("num", values, "0")}</c:val></c:ser>'
    )


CHART = (
    f'{XML}<c:chartSpace xmlns:c="{C}" xmlns:a="{A}" xmlns:r="{R}"><c:chart>'
    '<c:title><c:tx><c:rich><a:bodyPr/><a:p><a:r><a:t>Coral cover (%)</a:t></a:r></a:p></c:rich></c:tx><c:overlay val="0"/></c:title>'
    '<c:autoTitleDeleted val="0"/><c:plotArea><c:layout/>'
    '<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:varyColors val="0"/>'
    + series(0, "North reef", [28, 31, 34])
    + series(1, "South reef", [25, 22, 19])
    + '<c:gapWidth val="80"/><c:axId val="1"/><c:axId val="2"/></c:barChart>'
    '<c:catAx><c:axId val="1"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:crossAx val="2"/></c:catAx>'
    '<c:valAx><c:axId val="2"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="l"/><c:majorGridlines/><c:numFmt formatCode="0" sourceLinked="1"/><c:crossAx val="1"/></c:valAx>'
    '</c:plotArea><c:legend><c:legendPos val="b"/></c:legend><c:plotVisOnly val="1"/></c:chart></c:chartSpace>'
)

SLIDE_CHART = slide(
    title_shape("Coral cover by year")
    + f'<p:graphicFrame>{nv(3, "Chart 2", kind="frame")}{xfrm(1.5, 1.8, 10.0, 5.0, "p:xfrm")}'
    f'<a:graphic><a:graphicData uri="{C}"><c:chart xmlns:c="{C}" r:id="rId2"/></a:graphicData></a:graphic></p:graphicFrame>'
)

OMML = (
    f'<m:oMathPara xmlns:m="{M}"><m:oMath>'
    "<m:r><m:t>C</m:t></m:r><m:r><m:t>=</m:t></m:r>"
    "<m:f><m:num><m:r><m:t>A</m:t></m:r></m:num><m:den><m:r><m:t>T</m:t></m:r></m:den></m:f>"
    "</m:oMath></m:oMathPara>"
)
SLIDE_FORMULA = slide(
    title_shape("How cover is computed")
    + f'<mc:AlternateContent xmlns:mc="{MC}" xmlns:a14="{A14}"><mc:Choice Requires="a14">'
    f'<p:sp>{nv(3, "TextBox 2")}<p:spPr>{xfrm(1.5, 2.2, 6.0, 1.2)}</p:spPr><p:txBody><a:bodyPr/><a:lstStyle/>'
    f"<a:p><a14:m>{OMML}</a14:m></a:p></p:txBody></p:sp></mc:Choice>"
    f'<mc:Fallback><p:sp>{nv(3, "TextBox 2")}<p:spPr>{xfrm(1.5, 2.2, 6.0, 1.2)}'
    '<a:blipFill><a:blip r:embed="rId2"/><a:stretch><a:fillRect/></a:stretch></a:blipFill></p:spPr>'
    "<p:txBody><a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr/></a:p></p:txBody></p:sp></mc:Fallback></mc:AlternateContent>"
    + text_box(4, "TextBox 3", (1.5, 3.8, 9.0, 1.0), para(run("where A is the area covered and T the area of the transect.")))
)

SMART_ART_FRAME = (
    f'<p:graphicFrame>{nv(3, "Diagram 2", kind="frame")}{xfrm(1.5, 2.0, 10.0, 3.0, "p:xfrm")}'
    f'<a:graphic><a:graphicData uri="{DGM}"><dgm:relIds xmlns:dgm="{DGM}" r:dm="rId2" r:lo="rId3" r:qs="rId4" r:cs="rId5"/></a:graphicData></a:graphic></p:graphicFrame>'
)


def dsp_shape(model_id, x, text):
    return (
        f'<dsp:sp modelId="{{{model_id}}}"><dsp:nvSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvSpPr/></dsp:nvSpPr>'
        f'<dsp:spPr><a:xfrm><a:off x="{emu(x)}" y="{emu(0.5)}"/><a:ext cx="{emu(2.6)}" cy="{emu(1.5)}"/></a:xfrm>'
        '<a:prstGeom prst="roundRect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></dsp:spPr>'
        f'<dsp:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/><a:p><a:pPr algn="ctr"/><a:r><a:rPr lang="en-US" sz="2000"/><a:t>{text}</a:t></a:r></a:p></dsp:txBody></dsp:sp>'
    )


DIAGRAM_DRAWING = (
    f'{XML}<dsp:drawing xmlns:dsp="{DSP}" xmlns:a="{A}"><dsp:spTree><dsp:nvGrpSpPr><dsp:cNvPr id="0" name=""/><dsp:cNvGrpSpPr/></dsp:nvGrpSpPr><dsp:grpSpPr/>'
    + dsp_shape("A0000000-0000-4000-8000-000000000001", 0.1, "Plan")
    + dsp_shape("A0000000-0000-4000-8000-000000000002", 3.6, "Dive")
    + dsp_shape("A0000000-0000-4000-8000-000000000003", 7.1, "Report")
    + "</dsp:spTree></dsp:drawing>"
)


def dgm_point(model_id, kind, text=None):
    if text is None:
        return f'<dgm:pt modelId="{{{model_id}}}" type="{kind}"><dgm:prSet/><dgm:spPr/></dgm:pt>'
    return f'<dgm:pt modelId="{{{model_id}}}"><dgm:prSet/><dgm:spPr/><dgm:t><a:bodyPr/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US"/><a:t>{text}</a:t></a:r></a:p></dgm:t></dgm:pt>'


DIAGRAM_DATA = (
    f'{XML}<dgm:dataModel xmlns:dgm="{DGM}" xmlns:a="{A}"><dgm:ptLst>'
    + dgm_point("B0000000-0000-4000-8000-000000000000", "doc")
    + dgm_point("A0000000-0000-4000-8000-000000000001", "node", "Plan")
    + dgm_point("A0000000-0000-4000-8000-000000000002", "node", "Dive")
    + dgm_point("A0000000-0000-4000-8000-000000000003", "node", "Report")
    + "</dgm:ptLst><dgm:cxnLst>"
    + "".join(
        f'<dgm:cxn modelId="{{C0000000-0000-4000-8000-00000000000{i}}}" srcId="{{B0000000-0000-4000-8000-000000000000}}" destId="{{A0000000-0000-4000-8000-00000000000{i}}}" srcOrd="{i - 1}" destOrd="0"/>'
        for i in (1, 2, 3)
    )
    + "</dgm:cxnLst><dgm:bg/><dgm:whole/>"
    + f'<dgm:extLst><a:ext uri="http://schemas.microsoft.com/office/drawing/2008/diagram"><dsp:dataModelExt xmlns:dsp="{DSP}" relId="rId6" minVer="http://schemas.openxmlformats.org/drawingml/2006/diagram"/></a:ext></dgm:extLst>'
    + "</dgm:dataModel>"
)
DIAGRAM_LAYOUT = f'{XML}<dgm:layoutDef xmlns:dgm="{DGM}" xmlns:a="{A}" uniqueId="urn:microsoft.com/office/officeart/2005/8/layout/process1"><dgm:layoutNode name="Name0"/></dgm:layoutDef>'
DIAGRAM_STYLE = f'{XML}<dgm:styleDef xmlns:dgm="{DGM}" xmlns:a="{A}" uniqueId="urn:microsoft.com/office/officeart/2005/8/quickstyle/simple1"><dgm:scene3d><a:camera prst="orthographicFront"/><a:lightRig rig="threePt" dir="t"/></dgm:scene3d></dgm:styleDef>'
DIAGRAM_COLORS = f'{XML}<dgm:colorsDef xmlns:dgm="{DGM}" xmlns:a="{A}" uniqueId="urn:microsoft.com/office/officeart/2005/8/colors/accent1_2"/>'

SLIDE_SMART_ART = slide(title_shape("How a survey runs") + SMART_ART_FRAME)

GROUP = (
    f'<p:grpSp>{nv(3, "Group 2", kind="grp")}<p:grpSpPr><a:xfrm><a:off x="{emu(1.5)}" y="{emu(2)}"/><a:ext cx="{emu(9)}" cy="{emu(2)}"/>'
    # A child extent of 0, as some exports write: the children are not scaled.
    f'<a:chOff x="{emu(1.5)}" y="{emu(2)}"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>'
    f'<p:sp>{nv(4, "Rectangle 3")}<p:spPr>{xfrm(1.5, 2, 4, 2)}<a:prstGeom prst="rect"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent2"/></a:solidFill></p:spPr>'
    f'<p:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/>{para(run("Before the dive"), ppr_extra="")}</p:txBody></p:sp>'
    f'<p:sp>{nv(5, "Rectangle 4")}<p:spPr>{xfrm(6.5, 2, 4, 2)}<a:prstGeom prst="ellipse"><a:avLst/></a:prstGeom><a:solidFill><a:schemeClr val="accent3"/></a:solidFill></p:spPr>'
    f'<p:txBody><a:bodyPr anchor="ctr"/><a:lstStyle/>{para(run("After the dive"))}</p:txBody></p:sp>'
    "</p:grpSp>"
)
SLIDE_GROUP = slide(
    title_shape("Before and after")
    + GROUP
    # An empty content placeholder: its layout's prompt text must not show.
    + placeholder(6, "Content Placeholder 5", 'idx="1"', (1.5, 4.5, 9, 2), "<a:bodyPr/><a:lstStyle/><a:p><a:endParaRPr lang=\"en-US\"/></a:p>")
    # Shrunk text (autofit), as PowerPoint writes it.
    + text_box(
        7,
        "TextBox 6",
        (1.5, 5.0, 9, 0.8),
        para(run("A long line of text that PowerPoint shrank to fit its box, at sixty-two and a half percent of its size.", ' sz="2800"')),
        extra='<a:normAutofit fontScale="62500" lnSpcReduction="20000"/>',
    )
)

SLIDES = [
    # (xml, layout number, rels beyond the layout, notes text or None)
    (SLIDE_TITLE, 1, [], None),
    (SLIDE_GOALS, 2, [("rId3", "hyperlink", "https://example.org/coral-data", True)], ["Mention the baseline of 2024 first.", "Then the twelve reefs."]),
    (SLIDE_STEPS, 2, [], None),
    (SLIDE_HIDDEN, 2, [], None),
    (SLIDE_NO_TITLE, 4, [("rId2", "image", "../media/reef.png"), ("rId3", "image", "../media/logo.png"), ("rId4", "image", "../media/logo.svg")], None),
    (SLIDE_SECTION, 3, [], None),
    (SLIDE_TABLE, 2, [], None),
    (SLIDE_CHART, 2, [("rId2", "chart", "../charts/chart1.xml")], ["The south reef lost cover after the 2025 heat wave."]),
    (SLIDE_FORMULA, 2, [("rId2", "image", "../media/formula.png")], None),
    (
        SLIDE_SMART_ART,
        2,
        [
            ("rId2", "diagramData", "../diagrams/data1.xml"),
            ("rId3", "diagramLayout", "../diagrams/layout1.xml"),
            ("rId4", "diagramQuickStyle", "../diagrams/quickStyle1.xml"),
            ("rId5", "diagramColors", "../diagrams/colors1.xml"),
            ("rId6", "diagramDrawing", "../diagrams/drawing1.xml"),
        ],
        None,
    ),
    (SLIDE_GROUP, 2, [], None),
]

# The slides of each section, by their position in SLIDES (from 0).
SECTIONS = [("Introduction", [0, 1, 2, 3, 4]), ("Results", [5, 6, 7, 8, 9, 10])]


def section_list():
    body = ""
    for index, (name, slides) in enumerate(SECTIONS):
        ids = "".join(f'<p14:sldId id="{256 + i}"/>' for i in slides)
        body += f'<p14:section name="{name}" id="{{D0000000-0000-4000-8000-00000000000{index}}}"><p14:sldIdLst>{ids}</p14:sldIdLst></p14:section>'
    return f'<p:extLst><p:ext uri="{{521415D9-36F7-43E2-AB2F-B90AF26B5E84}}"><p14:sectionLst xmlns:p14="{P14}">{body}</p14:sectionLst></p:ext></p:extLst>'


def presentation():
    ids = "".join(f'<p:sldId id="{256 + i}" r:id="rId{10 + i}"/>' for i in range(len(SLIDES)))
    default_style = '<p:defaultTextStyle><a:lvl1pPr><a:defRPr sz="1800"><a:latin typeface="+mn-lt"/></a:defRPr></a:lvl1pPr></p:defaultTextStyle>'
    return (
        f'{XML}<p:presentation {NS} saveSubsetFonts="1">'
        '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>'
        '<p:notesMasterIdLst><p:notesMasterId r:id="rId3"/></p:notesMasterIdLst>'
        f'<p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx="{SLIDE_CX}" cy="{SLIDE_CY}"/><p:notesSz cx="6858000" cy="9144000"/>'
        f"{default_style}{section_list()}</p:presentation>"
    )


NOTES_MASTER = (
    f'{XML}<p:notesMaster {NS}>'
    + tree(
        placeholder(2, "Slide Image Placeholder 1", 'type="sldImg" idx="2"', (1.0, 0.75, 5.5, 3.1))
        + placeholder(3, "Notes Placeholder 2", 'type="body" sz="quarter" idx="3"', (0.75, 4.35, 6.0, 3.6), prompt("Click to edit Master text styles"))
    )
    + '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
    + "</p:notesMaster>"
)


def notes_slide(lines, number):
    return (
        f"{XML}<p:notes {NS}>"
        + tree(
            placeholder(2, "Slide Image Placeholder 1", 'type="sldImg"', None, None).replace("<p:txBody>None</p:txBody>", "")
            + placeholder(3, "Notes Placeholder 2", 'type="body" idx="1"', None, body(*(para(run(line)) for line in lines)))
            + placeholder(4, "Slide Number Placeholder 3", 'type="sldNum" sz="quarter" idx="5"', None, body(para(f'<a:fld id="{{B5C0F1E2-0000-4000-8000-000000000009}}" type="slidenum"><a:rPr lang="en-US"/><a:t>{number}</a:t></a:fld>')))
        )
        + "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>"
    )


CORE = (
    f'{XML}<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
    'xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Coral Sea Survey 2026</dc:title><dc:creator>Marine Science Team</dc:creator></cp:coreProperties>'
)


def content_types(notes):
    ov = [
        ("/ppt/presentation.xml", "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"),
        ("/ppt/slideMasters/slideMaster1.xml", "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"),
        ("/ppt/notesMasters/notesMaster1.xml", "application/vnd.openxmlformats-officedocument.presentationml.notesMaster+xml"),
        ("/ppt/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"),
        ("/ppt/theme/theme2.xml", "application/vnd.openxmlformats-officedocument.theme+xml"),
        ("/ppt/charts/chart1.xml", "application/vnd.openxmlformats-officedocument.drawingml.chart+xml"),
        ("/ppt/diagrams/data1.xml", "application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml"),
        ("/ppt/diagrams/layout1.xml", "application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml"),
        ("/ppt/diagrams/quickStyle1.xml", "application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml"),
        ("/ppt/diagrams/colors1.xml", "application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml"),
        ("/ppt/diagrams/drawing1.xml", "application/vnd.ms-office.drawingml.diagramDrawing+xml"),
        ("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml"),
    ]
    ov += [(f"/ppt/slideLayouts/slideLayout{i}.xml", "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml") for i in range(1, 5)]
    ov += [(f"/ppt/slides/slide{i}.xml", "application/vnd.openxmlformats-officedocument.presentationml.slide+xml") for i in range(1, len(SLIDES) + 1)]
    ov += [(f"/ppt/notesSlides/notesSlide{i}.xml", "application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml") for i in notes]
    defaults = (
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/>'
        '<Default Extension="svg" ContentType="image/svg+xml"/>'
    )
    body = "".join(f'<Override PartName="{name}" ContentType="{kind}"/>' for name, kind in ov)
    return f'{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">{defaults}{body}</Types>'


def main():
    parts = [
        ("_rels/.rels", rels(("rId1", "officeDocument", "ppt/presentation.xml"), ("rId2", "core", "docProps/core.xml"))),
        ("docProps/core.xml", CORE),
        ("ppt/presentation.xml", presentation()),
        (
            "ppt/_rels/presentation.xml.rels",
            rels(
                ("rId1", "slideMaster", "slideMasters/slideMaster1.xml"),
                ("rId2", "theme", "theme/theme1.xml"),
                ("rId3", "notesMaster", "notesMasters/notesMaster1.xml"),
                *[(f"rId{10 + i}", "slide", f"slides/slide{i + 1}.xml") for i in range(len(SLIDES))],
            ),
        ),
        ("ppt/theme/theme1.xml", THEME),
        ("ppt/theme/theme2.xml", THEME),
        ("ppt/slideMasters/slideMaster1.xml", MASTER),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels",
            rels(*[(f"rId{i}", "slideLayout", f"../slideLayouts/slideLayout{i}.xml") for i in range(1, 5)], ("rId5", "theme", "../theme/theme1.xml")),
        ),
        ("ppt/notesMasters/notesMaster1.xml", NOTES_MASTER),
        ("ppt/notesMasters/_rels/notesMaster1.xml.rels", rels(("rId1", "theme", "../theme/theme2.xml"))),
        ("ppt/charts/chart1.xml", CHART),
        ("ppt/diagrams/data1.xml", DIAGRAM_DATA),
        ("ppt/diagrams/_rels/data1.xml.rels", rels(("rId6", "diagramDrawing", "drawing1.xml"))),
        ("ppt/diagrams/layout1.xml", DIAGRAM_LAYOUT),
        ("ppt/diagrams/quickStyle1.xml", DIAGRAM_STYLE),
        ("ppt/diagrams/colors1.xml", DIAGRAM_COLORS),
        ("ppt/diagrams/drawing1.xml", DIAGRAM_DRAWING),
        ("ppt/media/reef.png", png(48, 32, (0x2A, 0x9D, 0x8F))),
        ("ppt/media/logo.png", png(16, 16, (0x2A, 0x6F, 0x97))),
        ("ppt/media/logo.svg", SVG),
        ("ppt/media/formula.png", png(40, 12, (0x00, 0x00, 0x00))),
    ]
    for number, content in enumerate([LAYOUT_TITLE, LAYOUT_CONTENT, LAYOUT_SECTION, LAYOUT_BLANK], start=1):
        parts.append((f"ppt/slideLayouts/slideLayout{number}.xml", content))
        parts.append((f"ppt/slideLayouts/_rels/slideLayout{number}.xml.rels", rels(("rId1", "slideMaster", "../slideMasters/slideMaster1.xml"))))
    notes = []
    for number, (xml, layout_number, more, lines) in enumerate(SLIDES, start=1):
        items = [("rId1", "slideLayout", f"../slideLayouts/slideLayout{layout_number}.xml"), *more]
        if lines:
            items.append(("rId9", "notesSlide", f"../notesSlides/notesSlide{number}.xml"))
            notes.append(number)
            parts.append((f"ppt/notesSlides/notesSlide{number}.xml", notes_slide(lines, number)))
            parts.append(
                (
                    f"ppt/notesSlides/_rels/notesSlide{number}.xml.rels",
                    rels(("rId1", "notesMaster", "../notesMasters/notesMaster1.xml"), ("rId2", "slide", f"../slides/slide{number}.xml")),
                )
            )
        parts.append((f"ppt/slides/slide{number}.xml", xml))
        parts.append((f"ppt/slides/_rels/slide{number}.xml.rels", rels(*items)))
    parts.insert(0, ("[Content_Types].xml", content_types(notes)))
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as archive:
        for name, content in parts:
            info = zipfile.ZipInfo(name, DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, content.encode() if isinstance(content, str) else content)


if __name__ == "__main__":
    main()
