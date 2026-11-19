"""Convert PDF papers and books into clean, readable HTML, Word documents, Markdown, LaTeX, EPUB books or editable JSON.

>>> import kakemono
>>> html = kakemono.convert("paper.pdf")                  # friendly mode
>>> pages = kakemono.convert("1706.03762", mode="hard")   # an arXiv id works too
>>> data = kakemono.to_docx("paper.pdf")                  # a Word document (bytes)
>>> text = kakemono.to_markdown("paper.pdf", "out/paper.md")  # Markdown + out/paper_assets/
>>> tex = kakemono.to_latex("paper.pdf", "out/paper.tex")     # LaTeX + out/paper_assets/
>>> book = kakemono.to_epub("paper.pdf", "out/paper.epub")    # an EPUB book (also returns its bytes)
>>> content = kakemono.to_json("paper.pdf", "out/paper.json")  # editable JSON + out/paper_assets/
>>> html = kakemono.convert("paper.pdf", "hard-95", content="out/paper.vi.json")  # with the edited text
>>> doc = kakemono.analyze("paper.pdf")                   # the document structure

The work is done by the kakemono Rust library; the GIL is released while it
runs, so conversions can run in parallel threads.
"""

from __future__ import annotations

import datetime
import json
import os
import re
from typing import Any, Dict, List, Literal, Optional, Sequence, Union

from . import _kakemono

__all__ = [
    "MODES",
    "Source",
    "analyze",
    "convert",
    "convert_all",
    "fetch_arxiv",
    "to_docx",
    "to_epub",
    "to_json",
    "to_latex",
    "to_markdown",
    "__version__",
]

__version__: str = _kakemono.__version__

#: The four output modes, from the most faithful to the most readable.
MODES = ("hard", "hard-95", "medium", "friendly")

Mode = Literal["hard", "hard-95", "medium", "friendly"]
#: How hard PNG pictures are compressed (the pixels are the same).
PngCompression = Literal["fast", "balanced", "high"]
#: A PDF: its bytes, a path to a file, or an arXiv id or URL.
Source = Union[bytes, bytearray, memoryview, str, "os.PathLike[str]"]
#: An edited JSON file (from :func:`to_json`): the parsed dict, its text, or a path to it.
Content = Union[Dict[str, Any], str, "os.PathLike[str]"]


def _load(source: Source) -> tuple[bytes, str | None]:
    """The PDF bytes of `source`, and the name recorded in the output."""
    if isinstance(source, (bytes, bytearray, memoryview)):
        return bytes(source), None
    path = os.fspath(source)
    if os.path.exists(path):
        with open(path, "rb") as file:
            return file.read(), os.path.basename(path)
    arxiv_id = _kakemono.parse_arxiv_id(path)
    if arxiv_id is not None:
        return _kakemono.fetch_arxiv(arxiv_id), f"arXiv:{arxiv_id}"
    raise FileNotFoundError(f"{path!r} is neither a file nor an arXiv id or URL")


def _content(content: Optional[Content]) -> Optional[str]:
    """The text of an edited JSON file given as a dict, as JSON text or as a path."""
    if content is None:
        return None
    if isinstance(content, dict):
        return json.dumps(content, ensure_ascii=False)
    if isinstance(content, str) and content.lstrip().startswith("{"):
        return content
    with open(os.fspath(content), encoding="utf-8") as file:
        return file.read()


def convert_all(
    source: Source,
    modes: Sequence[Mode] = MODES,
    *,
    tables: bool = True,
    equations: Literal["mathml", "image"] = "mathml",
    page_image: Literal["svg", "svg-only", "png"] = "svg",
    embed_fonts: bool = True,
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    content: Optional[Content] = None,
) -> Dict[str, str]:
    """Convert a PDF to HTML in several modes, analysing it only once.

    Returns a dict from mode name to a self-contained HTML document (images
    and fonts embedded). The options are those of the command line:

    - ``tables``: rebuild tables as ``<table>`` when their grid is clear.
    - ``equations``: display formulas as MathML where possible, or pictures.
    - ``page_image``: page background of the ``hard`` and ``hard-95`` modes.
    - ``embed_fonts``: embed the reading and math fonts, or use the reader's.
    - ``scale``: image resolution in pixels per PDF point (3 is about 216 dpi).
    - ``jobs``: worker threads, 0 for one per core.
    - ``png_compression``: ``"fast"`` draws the pictures several times faster
      for PNG files about a quarter larger, ``"high"`` makes them about a
      tenth smaller for ten times the time; the pixels are the same.
    - ``content``: an edited copy of the JSON of :func:`to_json` for the same
      PDF (a dict, its text or a path): the output shows its text. The
      ``hard`` mode cannot. A block whose edited text cannot be read keeps
      its own, with a ``UserWarning``.

    Raises ``ValueError`` when the data is not a readable PDF, an option is
    wrong or `content` does not fit the PDF, and ``FileNotFoundError`` when
    `source` names nothing.
    """
    modes = list(modes)
    data, name = _load(source)
    pages = _kakemono.convert(
        data,
        modes,
        source=name,
        tables=tables,
        equations=equations,
        page_image=page_image,
        embed_fonts=embed_fonts,
        scale=scale,
        jobs=jobs,
        png_compression=png_compression,
        content=_content(content),
    )
    return dict(zip(modes, pages))


def convert(source: Source, mode: Mode = "friendly", **options: Any) -> str:
    """Convert a PDF to one self-contained HTML document.

    `source` is the PDF's bytes, a path, or an arXiv id or URL. `mode` is one
    of :data:`MODES`; the other options are those of :func:`convert_all`.
    """
    return convert_all(source, [mode], **options)[mode]


PathArg = Union[str, "os.PathLike[str]"]


def _write(path: PathArg, data: Union[bytes, str], files: Sequence[tuple[str, bytes]] = ()) -> None:
    """Write `data` to `path`, and each of `files` relative to its folder."""
    path = os.fspath(path)
    base = os.path.dirname(path)
    if base:
        os.makedirs(base, exist_ok=True)
    mode, kwargs = ("w", {"encoding": "utf-8"}) if isinstance(data, str) else ("wb", {})
    with open(path, mode, **kwargs) as file:
        file.write(data)
    for name, content in files:
        target = os.path.join(base, name)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "wb") as file:
            file.write(content)


def _image_dir(path: PathArg, image_dir: Optional[str]) -> str:
    """The image folder of the file at `path`: `image_dir` or ``<name>_assets``."""
    if image_dir is None:
        return os.path.splitext(os.path.basename(os.fspath(path)))[0] + "_assets"
    if os.path.isabs(image_dir) or ".." in image_dir.replace("\\", "/").split("/"):
        raise ValueError("image_dir must be a relative folder inside the output folder")
    return image_dir.rstrip("/\\")


def to_docx(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Literal["mathml", "image"] = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    save_images: bool = False,
    content: Optional[Content] = None,
) -> bytes:
    """Convert a PDF to a Word document (``.docx``) and return its bytes.

    When `path` is given, the document is also written there; with
    ``save_images=True`` its pictures are also written as files to
    `image_dir` (default ``<name>_assets``, relative to the document), and
    stay embedded in it. Headings, lists, captions and references get Word
    styles, rebuilt tables become Word tables, and formulas whose structure
    is clear become Word equations (with ``equations="image"``, every
    formula is a picture). The other options are those of
    :func:`convert_all`.
    """
    if save_images and path is None:
        raise ValueError("save_images needs a path to write the pictures next to")
    folder = _image_dir(path, image_dir) if save_images and path is not None else None
    data, name = _load(source)
    docx, files = _kakemono.to_docx(
        data, source=name, tables=tables, equations=equations, scale=scale, jobs=jobs, image_dir=folder,
        png_compression=png_compression, content=_content(content),
    )
    if path is not None:
        _write(path, docx, files)
    return docx


def to_markdown(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Literal["mathml", "image"] = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    inline_images: bool = False,
    content: Optional[Content] = None,
) -> str:
    """Convert a PDF to Markdown and return the text.

    Headings, lists, captions and references get their Markdown syntax,
    rebuilt tables become pipe tables (HTML tables when cells span), and
    formulas whose structure is clear become LaTeX between ``$$`` (with
    ``equations="image"``, every formula is a picture).

    When `path` is given, the Markdown is written there and its pictures to
    `image_dir` (default ``<name>_assets``, relative to the file). With
    ``inline_images=True``, or without `path`, the pictures are embedded as
    data URIs instead. The other options are those of :func:`convert_all`.
    """
    inline = inline_images or path is None
    folder = _image_dir(path, image_dir) if path is not None else "images"
    data, name = _load(source)
    text, files = _kakemono.to_markdown(
        data,
        source=name,
        tables=tables,
        equations=equations,
        scale=scale,
        jobs=jobs,
        png_compression=png_compression,
        image_dir=folder,
        inline_images=inline,
        content=_content(content),
    )
    if path is not None:
        _write(path, text, files)
    return text


def to_json(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Literal["mathml", "image"] = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    inline_images: bool = False,
    content: Optional[Content] = None,
) -> Dict[str, Any]:
    """Convert a PDF to the editable JSON and return it as a dict.

    The blocks come in reading order, each with its ``type``, ``section``,
    ``layout`` and ``text``: one string whose tags (``<i>``, ``<b>``,
    ``<a href>``, ...) keep the styles and links, and where ``<m id="n">``
    stands for a formula. Edit the text (translate it, say), then give the
    edited dict, its text or its path as ``content`` to any converter of this
    module to get that output with the edited text.

    Each block also has its ``plain`` text (no tags, formulas as
    ``$LaTeX$``), each reference its ``citation`` (authors, title, venue,
    year, DOI, arXiv id), and ``meta`` holds the authors with their
    affiliations and e-mail addresses, the date, the DOI, the keywords and
    the abstract: these are read-only, only ``text`` (and the ``rows`` of
    authors and the cells of tables) is read back.

    Formulas whose structure is clear also come as LaTeX and MathML; figures
    and the other formulas are pictures. When `path` is given, the JSON is
    written there and its pictures to `image_dir` (default
    ``<name>_assets``, relative to the file). With ``inline_images=True``, or
    without `path`, the pictures are data URIs instead. The other options are
    those of :func:`convert_all`.
    """
    inline = inline_images or path is None
    folder = _image_dir(path, image_dir) if path is not None else "images"
    data, name = _load(source)
    text, files = _kakemono.to_json(
        data,
        source=name,
        tables=tables,
        equations=equations,
        scale=scale,
        jobs=jobs,
        png_compression=png_compression,
        image_dir=folder,
        inline_images=inline,
        content=_content(content),
    )
    if path is not None:
        _write(path, text, files)
    return json.loads(text)


def to_latex(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Literal["mathml", "image"] = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    content: Optional[Content] = None,
) -> str:
    """Convert a PDF to a LaTeX document (``.tex``) and return its source.

    The document compiles with pdfLaTeX, XeLaTeX or LuaLaTeX. Headings
    become sections, the abstract and lists their environments, rebuilt
    tables ``tabular``s, and formulas whose structure is clear LaTeX math
    (with ``equations="image"``, every displayed formula is a picture).

    LaTeX cannot embed pictures: when `path` is given, the document is
    written there and its pictures to `image_dir` (default
    ``<name>_assets``, relative to the file); without `path`, only the
    source is returned, and it refers to pictures in ``images/``. The other
    options are those of :func:`convert_all`.
    """
    folder = _image_dir(path, image_dir) if path is not None else "images"
    data, name = _load(source)
    text, files = _kakemono.to_latex(
        data, source=name, tables=tables, equations=equations, scale=scale, jobs=jobs, image_dir=folder,
        png_compression=png_compression, content=_content(content),
    )
    if path is not None:
        _write(path, text, files)
    return text


def to_epub(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Literal["mathml", "image"] = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    max_math_width_em: float = 20.0,
    cover: bool = True,
    modified: Optional[str] = None,
    content: Optional[Content] = None,
) -> bytes:
    """Convert a PDF to an EPUB 3 book (``.epub``) and return its bytes.

    When `path` is given, the book is also written there. It has one
    chapter per top-level section and a table of contents; formulas whose
    structure is clear become MathML (with ``equations="image"``, every
    displayed formula is a picture), rebuilt tables become tables, and the
    pictures are stored in the book. A displayed formula wider than
    `max_math_width_em` (in multiples of the font size of the text) is a
    picture, which shrinks to the page, instead of MathML, which would be
    cut at the edge of a narrow screen. With ``cover=True`` the first page is
    its cover. `modified` is the date recorded in the book, as
    ``YYYY-MM-DDThh:mm:ssZ`` (UTC); by default, now. The other options are
    those of :func:`convert_all`.
    """
    if modified is None:
        modified = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    elif not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", modified):
        raise ValueError(f"modified must look like 2026-01-31T12:00:00Z, not {modified!r}")
    data, name = _load(source)
    book = _kakemono.to_epub(
        data,
        source=name,
        tables=tables,
        equations=equations,
        scale=scale,
        jobs=jobs,
        png_compression=png_compression,
        max_math_width_em=max_math_width_em,
        cover=cover,
        modified=modified,
        content=_content(content),
    )
    if path is not None:
        _write(path, book)
    return book


def analyze(source: Source, *, tables: bool = True, jobs: int = 0) -> Dict[str, Any]:
    """The structure of a PDF: its blocks in reading order, as a dict.

    This is the document that ``kakemono convert --json`` writes, with every
    field of the analysis (for the editable JSON, see :func:`to_json`): each block
    has a ``type`` (``heading``, ``paragraph``, ``equation``, ``figure``, ...),
    its ``page``, ``bbox`` and ``spans`` of text.
    """
    data, _ = _load(source)
    return json.loads(_kakemono.analyze(data, tables=tables, jobs=jobs))


def fetch_arxiv(arxiv_id: str) -> bytes:
    """Download the PDF of an arXiv paper (cached in ``~/.cache/kakemono``)."""
    return _kakemono.fetch_arxiv(arxiv_id)
