"""Convert PDF papers and books, Word documents (.docx, .doc), EPUB books (.epub) and web pages (.html files or http(s) addresses) into clean, readable HTML, Word documents, Markdown, LaTeX, EPUB books, editable JSON or chunks for retrieval.

>>> import kakemono
>>> html = kakemono.convert("paper.pdf")                  # friendly mode
>>> pages = kakemono.convert("1706.03762", mode="hard")   # an arXiv id works too
>>> data = kakemono.to_docx("paper.pdf")                  # a Word document (bytes)
>>> text = kakemono.to_markdown("paper.pdf", "out/paper.md")  # Markdown + out/paper_assets/
>>> tex = kakemono.to_latex("paper.pdf", "out/paper.tex")     # LaTeX + out/paper_assets/
>>> typ = kakemono.to_typst("book.pdf", "out/book.typ")       # Typst + out/book_assets/
>>> book = kakemono.to_epub("paper.pdf", "out/paper.epub")    # an EPUB book (also returns its bytes)
>>> content = kakemono.to_json("paper.pdf", "out/paper.json")  # editable JSON + out/paper_assets/
>>> html = kakemono.convert("paper.pdf", "hard-95", content="out/paper.vi.json")  # with the edited text
>>> chunks = kakemono.to_chunks("paper.pdf")             # chunks for retrieval (RAG), a list of dicts
>>> doc = kakemono.analyze("paper.pdf")                   # the document structure
>>> md = kakemono.to_markdown("report.docx", "out/report.md")  # a Word document as input
>>> md = kakemono.to_markdown("https://en.wikipedia.org/wiki/Fourier_transform", "out/ft.md")  # a web page
>>> pdf = kakemono.to_pdf("report.docx", "out/report.pdf")     # a Word document laid out in pages
>>> png = kakemono.render_svg("figure.svg", "figure.png", scale=2)  # draw an SVG

Each function reads and analyses its input again. To write several outputs of
one document, open it once as a :class:`Document`:

>>> doc = kakemono.Document("paper.pdf", content="out/paper.vi.json")
>>> doc.to_docx("out/paper.vi.docx"); doc.to_epub("out/paper.vi.epub")

The work is done by the kakemono Rust library; the GIL is released while it
runs, so conversions can run in parallel threads. :mod:`kakemono.aio` has the
same functions to be awaited, run in worker threads, for asyncio programs.
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
    "CancelToken",
    "Cancelled",
    "Document",
    "Source",
    "analyze",
    "convert",
    "convert_all",
    "fetch_arxiv",
    "read_formula",
    "render_svg",
    "to_chunks",
    "to_docx",
    "to_epub",
    "to_json",
    "to_latex",
    "to_markdown",
    "to_pdf",
    "to_typst",
    "__version__",
]

__version__: str = _kakemono.__version__

# CancelToken: a request to stop a conversion, given as ``cancel=``; its
# ``cancel()`` is called from another thread. ``CancelToken(timeout=s)`` also
# stops the work after `s` seconds; ``token.child(timeout=s)`` is stopped by
# `token` and by its own limit. Cancelled: raised by a conversion stopped
# with ``cancel()``.
from ._kakemono import CancelToken, Cancelled  # noqa: E402

#: The four output modes, from the most faithful to the most readable.
MODES = ("hard", "hard-95", "medium", "friendly")

Mode = Literal["hard", "hard-95", "medium", "friendly"]
#: How hard PNG pictures are compressed (the pixels are the same).
PngCompression = Literal["fast", "balanced", "high"]
#: How displayed formulas are written.
Equations = Literal["mathml", "image"]
#: A path to a file, or a string.
PathArg = Union[str, "os.PathLike[str]"]
#: A PDF, a Word document (.docx, .doc), an EPUB book (.epub) or a web page
#: (.html): its bytes, a path to a file, an arXiv id or URL (the PDF), or the
#: http(s) address of a web page or document (fetched with its pictures).
Source = Union[bytes, bytearray, memoryview, PathArg]
#: An edited JSON file (from :func:`to_json`): the parsed dict, its text, or a path to it.
Content = Union[Dict[str, Any], PathArg]


def _token(cancel: Optional[CancelToken], timeout: Optional[float]) -> Optional[CancelToken]:
    """The token of a call: `cancel`, with the time limit `timeout` (seconds)
    from now."""
    if timeout is None:
        return cancel
    if cancel is None:
        return CancelToken(timeout=timeout)
    return cancel.child(timeout=timeout)


def _load(source: Source, cancel: Optional[CancelToken] = None) -> tuple[Dict[str, Any], str | None]:
    """Where `source` is read from (``data``, ``path`` or ``url``, the
    arguments of the native document), and the name recorded in the output."""
    if isinstance(source, (bytes, bytearray, memoryview)):
        return {"data": bytes(source)}, None
    path = os.fspath(source)
    if os.path.exists(path):
        # Read by the library: a web page has its pictures next to it.
        return {"path": path}, os.path.basename(path)
    arxiv_id = _kakemono.parse_arxiv_id(path)
    if arxiv_id is not None:
        return {"data": _kakemono.fetch_arxiv(arxiv_id, cancel=cancel)}, f"arXiv:{arxiv_id}"
    if re.match(r"(?i)https?://", path.strip()):
        return {"url": path.strip()}, path.strip()
    raise FileNotFoundError(f"{path!r} is neither a file, an arXiv id or URL, nor an http(s) address")


def _content(content: Optional[Content]) -> tuple[Optional[str], Optional[str]]:
    """The text of an edited JSON file given as a dict, as JSON text or as a
    path, and the folder its relative ``replace_image`` paths are read from:
    the file's own, or the current folder for a dict or text."""
    if content is None:
        return None, None
    if isinstance(content, dict):
        return json.dumps(content, ensure_ascii=False), os.getcwd()
    if isinstance(content, str) and content.lstrip().startswith("{"):
        return content, os.getcwd()
    path = os.fspath(content)
    with open(path, encoding="utf-8") as file:
        return file.read(), os.path.dirname(os.path.abspath(path))


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


def _modified(modified: Optional[str]) -> str:
    """The date recorded in an EPUB book: `modified`, checked, or now."""
    if modified is None:
        return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z", modified):
        raise ValueError(f"modified must look like 2026-01-31T12:00:00Z, not {modified!r}")
    return modified


class Document:
    """A PDF, Word document, Excel workbook, EPUB book or web page, read and analysed once, from which any number
    of outputs are written.

    The module functions (:func:`convert`, :func:`to_docx`, ...) each read and
    analyse their input again; a ``Document`` does it once, which saves most
    of the time when one document is written in several formats:

    >>> doc = kakemono.Document("paper.pdf")
    >>> content = doc.to_json("out/paper.json")
    >>> # ... translate content["blocks"][i]["text"] ...
    >>> vi = kakemono.Document("paper.pdf", content=content, header_rows=header_rows)
    >>> vi.to_docx("out/paper.vi.docx"); vi.to_epub("out/paper.vi.epub")

    The options are those of the analysis, kept for every output:

    - ``tables``: rebuild tables as real tables when their grid is clear.
    - ``jobs``: worker threads, 0 for one per core.
    - ``png_compression``: ``"fast"`` draws the pictures several times faster
      for PNG files about a quarter larger, ``"high"`` makes them about a
      tenth smaller for ten times the time; the pixels are the same.
    - ``content``: an edited copy of the JSON of :meth:`to_json` for the same
      document (a dict, its text or a path): every output shows its text. A
      block whose edited text cannot be read keeps its own, with a
      ``UserWarning``. A relative ``replace_image`` is read from the folder
      of the JSON file (the current folder for a dict or text).
    - ``header_rows``: the header rows of every table of an Excel workbook,
      given instead of found.
    - ``cancel`` and ``timeout``: a :data:`CancelToken` whose ``cancel()``,
      called from another thread, stops the reading (``Cancelled``), and a
      time limit in seconds (``TimeoutError``). Every method takes them too,
      for its own work; a module function's limit covers all of its work.
      The work stops at its next stopping point, normally within a page,
      and writes no file.

    Raises ``ValueError`` when the data is not a readable PDF, Word
    document, EPUB book or web page, a page cannot be fetched, an option is
    wrong or `content` does not fit the document, and ``FileNotFoundError``
    when `source` names nothing.

    A ``Document`` can be used from several threads at once.
    """

    def __init__(
        self,
        source: Source,
        *,
        tables: bool = True,
        jobs: int = 0,
        png_compression: PngCompression = "balanced",
        content: Optional[Content] = None,
        header_rows: Optional[int] = None,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> None:
        token = _token(cancel, timeout)
        where, name = _load(source, token)
        text, folder = _content(content)
        self._doc = _kakemono.Document(
            **where,
            source=name,
            tables=tables,
            jobs=jobs,
            png_compression=png_compression,
            content=text,
            content_dir=folder,
            header_rows=header_rows,
            cancel=token,
        )

    @property
    def format(self) -> Literal["pdf", "docx", "doc", "xlsx", "epub", "html"]:
        """The input format: ``"pdf"``, ``"docx"``, ``"doc"``, ``"xlsx"``, ``"epub"`` or ``"html"``."""
        return self._doc.format

    @property
    def page_count(self) -> int:
        """The number of pages of a PDF (0 for a Word document, which has none)."""
        return self._doc.page_count

    def __repr__(self) -> str:
        pages = f", {self.page_count} pages" if self.page_count else ""
        return f"<kakemono.Document {self.format}{pages}>"

    def convert_all(
        self,
        modes: Sequence[Mode] = MODES,
        *,
        equations: Equations = "mathml",
        page_image: Literal["svg", "svg-only", "png"] = "svg",
        embed_fonts: bool = True,
        scale: float = 3.0,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> Dict[str, str]:
        """The HTML of each of `modes`, as a dict from mode name to a
        self-contained HTML document (images and fonts embedded).

        - ``equations``: display formulas as MathML where possible, or pictures.
        - ``page_image``: page background of the ``hard`` and ``hard-95`` modes.
        - ``embed_fonts``: embed the reading and math fonts, or use the reader's.
        - ``scale``: image resolution in pixels per PDF point (3 is about 216 dpi).

        A Word document has only the ``friendly`` mode, and the ``hard`` mode
        cannot show edited text.
        """
        modes = list(modes)
        pages = self._doc.convert(
            modes, equations=equations, page_image=page_image, embed_fonts=embed_fonts, scale=scale, cancel=cancel, timeout=timeout
        )
        return dict(zip(modes, pages))

    def convert(self, mode: Mode = "friendly", **options: Any) -> str:
        """One self-contained HTML document in `mode`; the options are those
        of :meth:`convert_all`."""
        return self.convert_all([mode], **options)[mode]

    def to_docx(
        self,
        path: Optional[PathArg] = None,
        *,
        equations: Equations = "mathml",
        scale: float = 3.0,
        image_dir: Optional[str] = None,
        save_images: bool = False,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> bytes:
        """A Word document (``.docx``): its bytes.

        When `path` is given, the document is also written there; with
        ``save_images=True`` its pictures are also written as files to
        `image_dir` (default ``<name>_assets``, relative to the document), and
        stay embedded in it. Headings, lists, captions and references get Word
        styles, rebuilt tables become Word tables, and formulas whose
        structure is clear become Word equations (with ``equations="image"``,
        every formula is a picture).
        """
        if save_images and path is None:
            raise ValueError("save_images needs a path to write the pictures next to")
        folder = _image_dir(path, image_dir) if save_images and path is not None else None
        docx, files = self._doc.to_docx(equations=equations, scale=scale, image_dir=folder, cancel=cancel, timeout=timeout)
        if path is not None:
            _write(path, docx, files)
        return docx

    def to_markdown(
        self,
        path: Optional[PathArg] = None,
        *,
        equations: Equations = "mathml",
        scale: float = 3.0,
        image_dir: Optional[str] = None,
        inline_images: bool = False,
        mode: Literal["friendly", "medium"] = "friendly",
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> str:
        """Markdown: its text.

        Headings, lists, captions and references get their Markdown syntax,
        rebuilt tables become pipe tables, and formulas whose structure is
        clear become LaTeX between ``$$`` (with ``equations="image"``, every
        formula is a picture).

        With ``mode="friendly"`` every table is adapted to a pipe table: its
        header rows become one and a cell spanning rows or columns is
        repeated in each. With ``mode="medium"`` the layout of the tables is
        kept: a table with spanning cells stays an HTML table.

        When `path` is given, the Markdown is written there and its pictures
        to `image_dir` (default ``<name>_assets``, relative to the file). With
        ``inline_images=True``, or without `path`, the pictures are embedded
        as data URIs instead.
        """
        inline = inline_images or path is None
        folder = _image_dir(path, image_dir) if path is not None else "images"
        text, files = self._doc.to_markdown(
            equations=equations, scale=scale, image_dir=folder, inline_images=inline, mode=mode, cancel=cancel, timeout=timeout
        )
        if path is not None:
            _write(path, text, files)
        return text

    def to_json(
        self,
        path: Optional[PathArg] = None,
        *,
        equations: Equations = "mathml",
        scale: float = 3.0,
        image_dir: Optional[str] = None,
        inline_images: bool = False,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> Dict[str, Any]:
        """The editable JSON, as a dict.

        The blocks come in reading order, each with its ``type``,
        ``section``, ``layout`` and ``text``: one string whose tags (``<i>``,
        ``<b>``, ``<a href>``, ...) keep the styles and links, and where
        ``<m id="n">`` stands for a formula. Edit the text (translate it,
        say), then give the edited dict, its text or its path as ``content``
        to a :class:`Document` or to any converter of this module to get that
        output with the edited text.

        Each block also has its ``plain`` text (no tags, formulas as
        ``$LaTeX$``), each reference its ``citation`` (authors, title, venue,
        year, DOI, arXiv id), and ``meta`` holds the authors with their
        affiliations and e-mail addresses, the date, the DOI, the keywords
        and the abstract: these are read-only, only ``text`` (and the
        ``rows`` of authors and the cells of tables) is read back.

        Formulas whose structure is clear also come as LaTeX and MathML;
        figures and the other formulas are pictures. When `path` is given,
        the JSON is written there and its pictures to `image_dir` (default
        ``<name>_assets``, relative to the file). With ``inline_images=True``,
        or without `path`, the pictures are data URIs instead.
        """
        inline = inline_images or path is None
        folder = _image_dir(path, image_dir) if path is not None else "images"
        text, files = self._doc.to_json(equations=equations, scale=scale, image_dir=folder, inline_images=inline, cancel=cancel, timeout=timeout)
        if path is not None:
            _write(path, text, files)
        return json.loads(text)

    def to_latex(
        self,
        path: Optional[PathArg] = None,
        *,
        equations: Equations = "mathml",
        scale: float = 3.0,
        image_dir: Optional[str] = None,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> str:
        """A LaTeX document (``.tex``): its source.

        The document compiles with pdfLaTeX, XeLaTeX or LuaLaTeX. Headings
        become sections, the abstract and lists their environments, rebuilt
        tables ``tabular``s, and formulas whose structure is clear LaTeX math
        (with ``equations="image"``, every displayed formula is a picture).

        LaTeX cannot embed pictures: when `path` is given, the document is
        written there and its pictures to `image_dir` (default
        ``<name>_assets``, relative to the file); without `path`, only the
        source is returned, and it refers to pictures in ``images/``.
        """
        folder = _image_dir(path, image_dir) if path is not None else "images"
        text, files = self._doc.to_latex(equations=equations, scale=scale, image_dir=folder, cancel=cancel, timeout=timeout)
        if path is not None:
            _write(path, text, files)
        return text

    def to_typst(
        self,
        path: Optional[PathArg] = None,
        *,
        equations: Equations = "mathml",
        scale: float = 3.0,
        image_dir: Optional[str] = None,
        layout: str = "auto",
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> str:
        """A Typst document (``.typ``): its source.

        The document compiles with ``typst compile``. `layout` is
        ``"book"`` (a printed book: a title page, chapters on right-hand
        pages, running heads, footnotes), ``"article"``, or ``"auto"`` (a
        book when the document has chapters and nothing of a paper); the
        settings at the top of the file (page size, fonts, language) can be
        changed there. Formulas whose structure is clear are Typst math.

        Typst cannot embed pictures in its source: when `path` is given, the
        document is written there and its pictures to `image_dir` (default
        ``<name>_assets``, relative to the file); without `path`, only the
        source is returned, and it refers to pictures in ``images/``.
        """
        folder = _image_dir(path, image_dir) if path is not None else "images"
        text, files = self._doc.to_typst(equations=equations, scale=scale, image_dir=folder, layout=layout, cancel=cancel, timeout=timeout)
        if path is not None:
            _write(path, text, files)
        return text

    def to_epub(
        self,
        path: Optional[PathArg] = None,
        *,
        equations: Equations = "mathml",
        scale: float = 3.0,
        max_math_width_em: float = 20.0,
        cover: bool = True,
        modified: Optional[str] = None,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> bytes:
        """An EPUB 3 book (``.epub``): its bytes.

        When `path` is given, the book is also written there. It has one
        chapter per top-level section and a table of contents; formulas whose
        structure is clear become MathML (with ``equations="image"``, every
        displayed formula is a picture), rebuilt tables become tables, and
        the pictures are stored in the book. A displayed formula wider than
        `max_math_width_em` (in multiples of the font size of the text) is a
        picture, which shrinks to the page, instead of MathML, which would be
        cut at the edge of a narrow screen. With ``cover=True`` the first
        page is its cover. `modified` is the date recorded in the book, as
        ``YYYY-MM-DDThh:mm:ssZ`` (UTC); by default, now.
        """
        book = self._doc.to_epub(
            equations=equations,
            scale=scale,
            max_math_width_em=max_math_width_em,
            cover=cover,
            modified=_modified(modified), cancel=cancel, timeout=timeout
        )
        if path is not None:
            _write(path, book)
        return book

    def to_chunks(
        self,
        path: Optional[PathArg] = None,
        *,
        max_tokens: int = 512,
        tokenizer: Optional[PathArg] = None,
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> List[Dict[str, Any]]:
        """Chunks for retrieval (RAG), as dicts.

        Chunks follow the sections: each holds the blocks of one section, up
        to `max_tokens` tokens, and its ``text`` starts with the section path
        (``3 Model > 3.2 Attention``). Each also has its ``tokens``, the
        paper's ``title``, its ``section`` headings, and the ``blocks`` (ids
        of :meth:`to_json`) and ``pages`` it comes from.

        Tokens are estimated unless `tokenizer` names a real one: the path of
        a Hugging Face ``tokenizer.json`` (your embedding model's), or
        ``"cl100k_base"`` / ``"o200k_base"`` for OpenAI's models. When `path`
        is given, the chunks are also written there as JSON Lines.
        """
        text = self._doc.to_chunks(
            max_tokens=max_tokens, tokenizer=os.fspath(tokenizer) if tokenizer is not None else None, cancel=cancel, timeout=timeout
        )
        if path is not None:
            _write(path, text)
        return [json.loads(line) for line in text.split("\n") if line]

    def to_pdf(
        self, path: Optional[PathArg] = None, *, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None
    ) -> bytes:
        """A Word document laid out in pages as Word lays it out, as a PDF:
        its bytes. With ``content``, the edited document is laid out.

        When `path` is given, the PDF is also written there. Raises
        ``ValueError`` for a PDF, which already has its pages.
        """
        pdf = self._doc.to_pdf(cancel=cancel, timeout=timeout)
        if path is not None:
            _write(path, pdf)
        return pdf

    def analyze(self, *, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None) -> Dict[str, Any]:
        """The structure of the document: its blocks in reading order, as a dict.

        This is the document that ``kakemono convert --json`` writes, with
        every field of the analysis (for the editable JSON, see
        :meth:`to_json`): each block has a ``type`` (``heading``,
        ``paragraph``, ``equation``, ``figure``, ...), its ``page``, ``bbox``
        and ``spans`` of text.
        """
        return json.loads(self._doc.analyze(cancel=cancel, timeout=timeout))


def convert_all(
    source: Source,
    modes: Sequence[Mode] = MODES,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    page_image: Literal["svg", "svg-only", "png"] = "svg",
    embed_fonts: bool = True,
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> Dict[str, str]:
    """Convert a PDF or Word document to HTML in several modes, analysing it only once.

    Returns a dict from mode name to a self-contained HTML document (images
    and fonts embedded). The options are those of the command line:
    ``tables``, ``jobs``, ``png_compression`` and ``content`` as for
    :class:`Document`, the others as for :meth:`Document.convert_all`.

    Raises ``ValueError`` when the data is not a readable PDF, an option is
    wrong or `content` does not fit the PDF, and ``FileNotFoundError`` when
    `source` names nothing.
    """
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.convert_all(modes, equations=equations, page_image=page_image, embed_fonts=embed_fonts, scale=scale, cancel=token)


def convert(source: Source, mode: Mode = "friendly", **options: Any) -> str:
    """Convert a PDF or Word document to one self-contained HTML document.

    `source` is the PDF's bytes, a path, or an arXiv id or URL. `mode` is one
    of :data:`MODES`; the other options are those of :func:`convert_all`.
    """
    return convert_all(source, [mode], **options)[mode]


def to_docx(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    save_images: bool = False,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> bytes:
    """Convert a PDF or Word document to a Word document (``.docx``) and return its bytes.

    See :meth:`Document.to_docx`; ``tables``, ``jobs``, ``png_compression``
    and ``content`` are those of :class:`Document`.
    """
    if save_images and path is None:
        raise ValueError("save_images needs a path to write the pictures next to")
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.to_docx(path, equations=equations, scale=scale, image_dir=image_dir, save_images=save_images, cancel=token)


def to_markdown(  # NOSONAR python:S107: keyword options, as on the command line
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    inline_images: bool = False,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    mode: Literal["friendly", "medium"] = "friendly",
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> str:
    """Convert a PDF or Word document to Markdown and return the text.

    See :meth:`Document.to_markdown`; ``tables``, ``jobs``,
    ``png_compression`` and ``content`` are those of :class:`Document`.
    """
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.to_markdown(
        path, equations=equations, scale=scale, image_dir=image_dir, inline_images=inline_images, mode=mode, cancel=token
    )


def to_json(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    inline_images: bool = False,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> Dict[str, Any]:
    """Convert a PDF or Word document to the editable JSON and return it as a dict.

    See :meth:`Document.to_json`; ``tables``, ``jobs``, ``png_compression``
    and ``content`` are those of :class:`Document`.
    """
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.to_json(path, equations=equations, scale=scale, image_dir=image_dir, inline_images=inline_images, cancel=token)


def to_latex(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> str:
    """Convert a PDF or Word document to a LaTeX document (``.tex``) and return its source.

    See :meth:`Document.to_latex`; ``tables``, ``jobs``, ``png_compression``
    and ``content`` are those of :class:`Document`.
    """
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.to_latex(path, equations=equations, scale=scale, image_dir=image_dir, cancel=token)


def to_typst(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    image_dir: Optional[str] = None,
    layout: str = "auto",
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> str:
    """Convert a PDF or Word document to a Typst document (``.typ``) and return its source.

    See :meth:`Document.to_typst`; ``tables``, ``jobs``, ``png_compression``
    and ``content`` are those of :class:`Document`.
    """
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.to_typst(path, equations=equations, scale=scale, image_dir=image_dir, layout=layout, cancel=token)


def to_epub(  # NOSONAR python:S107: keyword options, as on the command line
    source: Source,
    path: Optional[PathArg] = None,
    *,
    tables: bool = True,
    equations: Equations = "mathml",
    scale: float = 3.0,
    jobs: int = 0,
    png_compression: PngCompression = "balanced",
    max_math_width_em: float = 20.0,
    cover: bool = True,
    modified: Optional[str] = None,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> bytes:
    """Convert a PDF or Word document to an EPUB 3 book (``.epub``) and return its bytes.

    See :meth:`Document.to_epub`; ``tables``, ``jobs``, ``png_compression``
    and ``content`` are those of :class:`Document`.
    """
    modified = _modified(modified)
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, png_compression=png_compression, content=content, header_rows=header_rows, cancel=token)
    return doc.to_epub(
        path, equations=equations, scale=scale, max_math_width_em=max_math_width_em, cover=cover, modified=modified, cancel=token
    )


def to_chunks(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    max_tokens: int = 512,
    tokenizer: Optional[PathArg] = None,
    tables: bool = True,
    jobs: int = 0,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> List[Dict[str, Any]]:
    """Cut a PDF or Word document into chunks for retrieval (RAG) and return them as dicts.

    See :meth:`Document.to_chunks`; ``tables``, ``jobs`` and ``content`` are
    those of :class:`Document`.
    """
    token = _token(cancel, timeout)
    doc = Document(source, tables=tables, jobs=jobs, content=content, header_rows=header_rows, cancel=token)
    return doc.to_chunks(path, max_tokens=max_tokens, tokenizer=tokenizer, cancel=token)


def to_pdf(
    source: Source,
    path: Optional[PathArg] = None,
    *,
    content: Optional[Content] = None,
    header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> bytes:
    """Lay out a Word document (``.docx``, ``.doc``) in pages as Word does, as a PDF, and return its bytes.

    When `path` is given, the PDF is also written there. With `content` (an
    edited JSON of the document, see :class:`Document`), the edited document
    is laid out. An Excel workbook is printed as Excel prints it. Raises
    ``ValueError`` for a PDF.
    """
    token = _token(cancel, timeout)
    return Document(source, content=content, header_rows=header_rows, cancel=token).to_pdf(path, cancel=token)


def analyze(
    source: Source, *, tables: bool = True, jobs: int = 0, header_rows: Optional[int] = None,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
) -> Dict[str, Any]:
    """The structure of a PDF or Word document: its blocks in reading order, as a dict.

    See :meth:`Document.analyze`.
    """
    token = _token(cancel, timeout)
    return Document(source, tables=tables, jobs=jobs, header_rows=header_rows, cancel=token).analyze(cancel=token)


def render_svg(
    svg: Union[bytes, bytearray, memoryview, PathArg],
    path: Optional[PathArg] = None,
    *,
    scale: float = 1.0,
    resources_dir: Optional[PathArg] = None,
) -> bytes:
    """Draw an SVG as a PNG, as ``kakemono render-svg`` does, and return the PNG's bytes.

    `svg` is a path to an SVG file, or the SVG itself as text (starting with
    ``<``) or bytes. The fonts are those kakemono draws SVG text with: the
    system's and those of the folders in ``KAKEMONO_FONT_DIRS``, loaded once
    per process. Pictures the SVG names by relative paths are read from
    `resources_dir`: by default the SVG file's folder, or the current folder
    for SVG text. `scale` is pixels per SVG unit. When `path` is given, the
    PNG is also written there.

    Raises ``ValueError`` saying what is wrong when the SVG cannot be drawn.
    """
    if isinstance(svg, (bytes, bytearray, memoryview)):
        text = bytes(svg).decode("utf-8")
        folder = os.getcwd()
    elif isinstance(svg, str) and svg.lstrip().startswith("<"):
        text = svg
        folder = os.getcwd()
    else:
        file = os.fspath(svg)
        with open(file, encoding="utf-8") as handle:
            text = handle.read()
        folder = os.path.dirname(os.path.abspath(file))
    if resources_dir is not None:
        folder = os.fspath(resources_dir)
    png = _kakemono.render_svg(text, scale=scale, resources_dir=folder)
    if path is not None:
        _write(path, png)
    return png


def read_formula(image: Union[bytes, bytearray, memoryview, PathArg]) -> Optional[str]:
    """Read the formula in a picture (PNG or JPEG, black on white, as LaTeX
    renders it), as ``kakemono formula`` does, and return its MathML.

    `image` is the picture's bytes or a path to it. Returns ``None`` when the
    formula cannot be read with confidence, and raises ``ValueError`` when
    the data is not a picture.
    """
    if not isinstance(image, (bytes, bytearray, memoryview)):
        with open(os.fspath(image), "rb") as file:
            image = file.read()
    return _kakemono.read_formula(bytes(image))


def fetch_arxiv(arxiv_id: str, *, cancel: Optional[CancelToken] = None, timeout: Optional[float] = None) -> bytes:
    """Download the PDF of an arXiv paper (cached in ``~/.cache/kakemono``).

    ``cancel`` and ``timeout`` stop the download, as for :class:`Document`.
    """
    return _kakemono.fetch_arxiv(arxiv_id, cancel=cancel, timeout=timeout)
