#!/usr/bin/env python3
"""Writes `vector.docx` next to this script: a Word document with vector pictures.

    python3 -I tests/data/word/make-vector-docx.py

The document holds, in this order:

1. an SVG drawing as Word stores it: an `a:blip` of a PNG fallback (plain grey)
   with an `asvg:svgBlip` extension naming the SVG. The SVG also holds what a
   reader must never copy out: a script, event handlers, a `foreignObject`, a
   style sheet and links to files and to the web;
2. an EMF metafile (two rectangles and an ellipse);
3. an SVG named straight by the `a:blip`, with no PNG.

Only the standard library is used, and every byte is fixed (no dates, no
random names), so the file is the same each time it is written.
"""

import struct
import zipfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "vector.docx"

# A ZIP entry date (the oldest ZIP can hold): the file does not change.
DATE = (1980, 1, 1, 0, 0, 0)
# English Metric Units in an inch.
EMU_PER_INCH = 914400

SVG = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"
     width="192" height="96" viewBox="0 0 192 96" onload="alert('onload')">
  <style>@import url(https://example.com/evil.css); circle { stroke: #000; stroke-width: 2 }</style>
  <script>alert('script')</script>
  <rect x="4" y="4" width="184" height="88" rx="8" fill="#1f77b4" onclick="alert('click')"/>
  <circle cx="48" cy="48" r="28" fill="#ff7f0e"/>
  <path d="M96 76 L128 20 L160 76 Z" fill="#2ca02c" stroke="#000" stroke-width="2"/>
  <foreignObject x="0" y="0" width="192" height="96">
    <div xmlns="http://www.w3.org/1999/xhtml">foreign HTML</div>
  </foreignObject>
  <image x="0" y="0" width="10" height="10" xlink:href="file:///etc/passwd"/>
  <image x="0" y="0" width="10" height="10" href="https://example.com/tracker.png"/>
  <a xlink:href="javascript:alert('link')"><rect x="170" y="70" width="10" height="10" fill="#d62728"/></a>
</svg>
"""

SVG_PLAIN = """<svg xmlns="http://www.w3.org/2000/svg" width="96" height="96" viewBox="0 0 96 96">
  <rect x="8" y="8" width="80" height="80" fill="#9467bd"/>
  <circle cx="48" cy="48" r="24" fill="#ffffff"/>
</svg>
"""


def png(width, height, rgb):
    """A PNG of one colour."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    row = b"\x00" + bytes(rgb) * width
    raw = zlib.compress(row * height, 9)
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", raw) + chunk(b"IEND", b"")


def emf():
    """An EMF of 2 by 1 inches: a framed blue rectangle, a red one, a green ellipse."""
    # Device: 1024 x 768 pixels on 320 x 240 mm (3.2 pixels per mm).
    frame = (0, 0, 5080, 2540)  # hundredths of a millimetre: 2 x 1 inches
    records = []

    def record(kind, *values):
        data = b"".join(struct.pack("<i", value) for value in values)
        records.append(struct.pack("<II", kind, 8 + len(data)) + data)

    # A black pen 2 pixels wide (EMR_CREATEPEN: index, style, width x, y, colour).
    record(38, 1, 0, 2, 0, 0x000000)
    record(37, 1)
    # Blue brush (EMR_CREATEBRUSHINDIRECT: index, style, COLORREF 0x00BBGGRR, hatch).
    record(39, 2, 0, 0xB4771F, 0)
    record(37, 2)
    record(43, 4, 4, 158, 77)  # EMR_RECTANGLE
    record(39, 3, 0, 0x2827D6, 0)
    record(37, 3)
    record(43, 100, 16, 150, 66)
    record(39, 4, 0, 0x2CA02C, 0)
    record(37, 4)
    record(42, 16, 16, 86, 66)  # EMR_ELLIPSE
    record(14, 0, 16, 20)  # EMR_EOF
    body = b"".join(records)
    header_size = 88
    total = header_size + len(body)
    header = struct.pack(
        "<II4i4iIIIIHHIIIiiii",
        1,
        header_size,
        0, 0, 162, 81,  # bounds, device pixels
        *frame,
        0x464D4520,  # " EMF"
        0x10000,
        total,
        len(records) + 1,
        5,  # handles
        0,
        0,
        0,
        0,
        1024, 768,
        320, 240,
    )
    assert len(header) == header_size
    return header + body


def inline(number, rel, width_in, height_in, name, svg_rel=None):
    """A `w:drawing` of a picture in line with the text."""
    cx, cy = int(width_in * EMU_PER_INCH), int(height_in * EMU_PER_INCH)
    ext = ""
    if svg_rel:
        ext = (
            '<a:extLst><a:ext uri="{96DAC541-7B7A-43D3-8B79-37D633B846F1}">'
            '<asvg:svgBlip xmlns:asvg="http://schemas.microsoft.com/office/drawing/2016/SVG/main" '
            f'r:embed="{svg_rel}"/></a:ext></a:extLst>'
        )
    return (
        f'<w:r><w:drawing><wp:inline><wp:extent cx="{cx}" cy="{cy}"/>'
        f'<wp:docPr id="{number}" name="{name}" descr="{name}"/>'
        '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        f'<pic:pic><pic:nvPicPr><pic:cNvPr id="{number}" name="{name}"/><pic:cNvPicPr/></pic:nvPicPr>'
        f'<pic:blipFill><a:blip r:embed="{rel}">{ext}</a:blip><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
        f'<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>'
        "</a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"
    )


def para(inner, style=None):
    props = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
    return f"<w:p>{props}{inner}</w:p>"


def text(value):
    return f'<w:r><w:t xml:space="preserve">{value}</w:t></w:r>'


DOCUMENT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
    'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body>'
    + para(text("Vector pictures"), "Title")
    + para(text("An SVG drawing, an EMF metafile and an SVG with no PNG beside it."))
    + para(inline(1, "rIdPng", 2, 1, "SVG drawing", svg_rel="rIdSvg"))
    + para(text("Figure 1: An SVG drawing with a PNG fallback."), "Caption")
    + para(inline(2, "rIdEmf", 2, 1, "EMF drawing"))
    + para(text("Figure 2: An EMF drawing."), "Caption")
    + para(inline(3, "rIdPlain", 1, 1, "Plain SVG"))
    + para(text("Figure 3: An SVG with no fallback."), "Caption")
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
    '<Default Extension="svg" ContentType="image/svg+xml"/>'
    '<Default Extension="emf" ContentType="image/x-emf"/>'
    '<Override PartName="/word/document.xml" '
    'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    '<Override PartName="/word/styles.xml" '
    'ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    "</Types>"
)

PACKAGE_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" '
    'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" '
    'Target="word/document.xml"/></Relationships>'
)

IMAGE = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"
DOCUMENT_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rIdStyles" '
    'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    f'<Relationship Id="rIdPng" Type="{IMAGE}" Target="media/image1.png"/>'
    f'<Relationship Id="rIdSvg" Type="{IMAGE}" Target="media/image2.svg"/>'
    f'<Relationship Id="rIdEmf" Type="{IMAGE}" Target="media/image3.emf"/>'
    f'<Relationship Id="rIdPlain" Type="{IMAGE}" Target="media/image4.svg"/>'
    "</Relationships>"
)


def main():
    parts = [
        ("[Content_Types].xml", CONTENT_TYPES.encode()),
        ("_rels/.rels", PACKAGE_RELS.encode()),
        ("word/document.xml", DOCUMENT.encode()),
        ("word/styles.xml", STYLES.encode()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS.encode()),
        ("word/media/image1.png", png(192, 96, (128, 128, 128))),
        ("word/media/image2.svg", SVG.encode()),
        ("word/media/image3.emf", emf()),
        ("word/media/image4.svg", SVG_PLAIN.encode()),
    ]
    with zipfile.ZipFile(OUT, "w", zipfile.ZIP_DEFLATED) as package:
        for name, data in parts:
            info = zipfile.ZipInfo(name, DATE)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            package.writestr(info, data)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
