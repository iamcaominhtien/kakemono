"""Convert PDF papers and books into clean, readable HTML.

>>> import kakemono
>>> html = kakemono.convert("paper.pdf")                  # friendly mode
>>> pages = kakemono.convert("1706.03762", mode="hard")   # an arXiv id works too
>>> doc = kakemono.analyze("paper.pdf")                   # the document structure

The work is done by the kakemono Rust library; the GIL is released while it
runs, so conversions can run in parallel threads.
"""

from __future__ import annotations

import json
import os
from typing import Any, Dict, List, Literal, Sequence, Union

from . import _kakemono

__all__ = ["MODES", "Source", "analyze", "convert", "convert_all", "fetch_arxiv", "__version__"]

__version__: str = _kakemono.__version__

#: The four output modes, from the most faithful to the most readable.
MODES = ("hard", "hard-95", "medium", "friendly")

Mode = Literal["hard", "hard-95", "medium", "friendly"]
#: A PDF: its bytes, a path to a file, or an arXiv id or URL.
Source = Union[bytes, bytearray, memoryview, str, "os.PathLike[str]"]


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

    Raises ``ValueError`` when the data is not a readable PDF or an option is
    wrong, and ``FileNotFoundError`` when `source` names nothing.
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
    )
    return dict(zip(modes, pages))


def convert(source: Source, mode: Mode = "friendly", **options: Any) -> str:
    """Convert a PDF to one self-contained HTML document.

    `source` is the PDF's bytes, a path, or an arXiv id or URL. `mode` is one
    of :data:`MODES`; the other options are those of :func:`convert_all`.
    """
    return convert_all(source, [mode], **options)[mode]


def analyze(source: Source, *, tables: bool = True, jobs: int = 0) -> Dict[str, Any]:
    """The structure of a PDF: its blocks in reading order, as a dict.

    This is the document that ``kakemono convert --json`` writes: each block
    has a ``type`` (``heading``, ``paragraph``, ``equation``, ``figure``, ...),
    its ``page``, ``bbox`` and ``spans`` of text.
    """
    data, _ = _load(source)
    return json.loads(_kakemono.analyze(data, tables=tables, jobs=jobs))


def fetch_arxiv(arxiv_id: str) -> bytes:
    """Download the PDF of an arXiv paper (cached in ``~/.cache/kakemono``)."""
    return _kakemono.fetch_arxiv(arxiv_id)
