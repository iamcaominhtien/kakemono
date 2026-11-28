"""Asynchronous versions of the kakemono functions, for asyncio programs.

Each function here takes the same arguments as the one of the same name in
:mod:`kakemono` and returns the same result, but is awaited: the work runs in
a worker thread (:func:`asyncio.to_thread`, the event loop's default
executor), so the event loop keeps serving other tasks while a document is
fetched, read, analysed and written. The GIL is released while the Rust
library works, so several conversions run in parallel:

>>> import asyncio
>>> from kakemono import aio
>>> async def main():
...     md = await aio.to_markdown("paper.pdf", "out/paper.md")
...     books = await asyncio.gather(*(aio.to_epub(p) for p in paths))

To write several outputs of one document, open it once:

>>> doc = await aio.Document.open("paper.pdf")
>>> await doc.to_docx("out/paper.docx"); await doc.to_epub("out/paper.epub")

A conversion cannot be stopped once started: cancelling the task stops the
wait, not the worker thread, which finishes and drops its result. The number
of conversions at once is that of the loop's default executor
(``loop.set_default_executor``), or of an ``asyncio.Semaphore`` of your own.
"""

from __future__ import annotations

import asyncio
import functools
import sys
from typing import TYPE_CHECKING, Any, Awaitable, Callable, Dict, List, Literal, Optional, TypeVar

from . import (
    Content,
    Document as _Document,
    PathArg,
    PngCompression,
    Source,
    analyze as _analyze,
    convert as _convert,
    convert_all as _convert_all,
    fetch_arxiv as _fetch_arxiv,
    read_formula as _read_formula,
    render_svg as _render_svg,
    to_chunks as _to_chunks,
    to_docx as _to_docx,
    to_epub as _to_epub,
    to_json as _to_json,
    to_latex as _to_latex,
    to_markdown as _to_markdown,
    to_pdf as _to_pdf,
    to_typst as _to_typst,
)

if TYPE_CHECKING:
    if sys.version_info >= (3, 10):
        from typing import ParamSpec
    else:
        from typing_extensions import ParamSpec

    P = ParamSpec("P")

R = TypeVar("R")

__all__ = [
    "Document",
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
]


def _in_thread(function: Callable[P, R]) -> Callable[P, Awaitable[R]]:
    """`function`, awaited: run in a worker thread with the same arguments,
    signature and docstring."""

    @functools.wraps(function)
    async def run(*args: P.args, **kwargs: P.kwargs) -> R:
        return await asyncio.to_thread(function, *args, **kwargs)

    return run


analyze = _in_thread(_analyze)
convert = _in_thread(_convert)
convert_all = _in_thread(_convert_all)
fetch_arxiv = _in_thread(_fetch_arxiv)
read_formula = _in_thread(_read_formula)
render_svg = _in_thread(_render_svg)
to_chunks = _in_thread(_to_chunks)
to_docx = _in_thread(_to_docx)
to_epub = _in_thread(_to_epub)
to_json = _in_thread(_to_json)
to_latex = _in_thread(_to_latex)
to_markdown = _in_thread(_to_markdown)
to_pdf = _in_thread(_to_pdf)
to_typst = _in_thread(_to_typst)


class Document:
    """A :class:`kakemono.Document` whose methods are awaited.

    Open one with :meth:`open`, which reads and analyses the document in a
    worker thread; its methods take the arguments of those of
    :class:`kakemono.Document`. Several of them can be awaited at once.
    """

    def __init__(self, document: _Document) -> None:
        """Wrap an open :class:`kakemono.Document` (see :meth:`open` to open one)."""
        self.sync = document
        """The :class:`kakemono.Document` underneath, for calls that need not be awaited."""

    @classmethod
    async def open(
        cls,
        source: Source,
        *,
        tables: bool = True,
        jobs: int = 0,
        png_compression: PngCompression = "balanced",
        content: Optional[Content] = None,
        header_rows: Optional[int] = None,
    ) -> "Document":
        """Read and analyse `source`, with the options of :class:`kakemono.Document`."""
        document = await asyncio.to_thread(
            _Document,
            source,
            tables=tables,
            jobs=jobs,
            png_compression=png_compression,
            content=content,
            header_rows=header_rows,
        )
        return cls(document)

    @property
    def format(self) -> Literal["pdf", "docx", "doc", "xlsx", "epub", "html"]:
        """The input format, as :attr:`kakemono.Document.format`."""
        return self.sync.format

    @property
    def page_count(self) -> int:
        """The number of pages of a PDF, as :attr:`kakemono.Document.page_count`."""
        return self.sync.page_count

    def __repr__(self) -> str:
        return f"<kakemono.aio.{repr(self.sync)[len('<kakemono.') :]}"

    async def convert_all(self, *args: Any, **kwargs: Any) -> Dict[str, str]:
        """:meth:`kakemono.Document.convert_all`, awaited."""
        return await asyncio.to_thread(self.sync.convert_all, *args, **kwargs)

    async def convert(self, *args: Any, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.convert`, awaited."""
        return await asyncio.to_thread(self.sync.convert, *args, **kwargs)

    async def to_docx(self, path: Optional[PathArg] = None, **kwargs: Any) -> bytes:
        """:meth:`kakemono.Document.to_docx`, awaited."""
        return await asyncio.to_thread(self.sync.to_docx, path, **kwargs)

    async def to_markdown(self, path: Optional[PathArg] = None, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.to_markdown`, awaited."""
        return await asyncio.to_thread(self.sync.to_markdown, path, **kwargs)

    async def to_json(self, path: Optional[PathArg] = None, **kwargs: Any) -> Dict[str, Any]:
        """:meth:`kakemono.Document.to_json`, awaited."""
        return await asyncio.to_thread(self.sync.to_json, path, **kwargs)

    async def to_latex(self, path: Optional[PathArg] = None, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.to_latex`, awaited."""
        return await asyncio.to_thread(self.sync.to_latex, path, **kwargs)

    async def to_typst(self, path: Optional[PathArg] = None, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.to_typst`, awaited."""
        return await asyncio.to_thread(self.sync.to_typst, path, **kwargs)

    async def to_epub(self, path: Optional[PathArg] = None, **kwargs: Any) -> bytes:
        """:meth:`kakemono.Document.to_epub`, awaited."""
        return await asyncio.to_thread(self.sync.to_epub, path, **kwargs)

    async def to_chunks(self, path: Optional[PathArg] = None, **kwargs: Any) -> List[Dict[str, Any]]:
        """:meth:`kakemono.Document.to_chunks`, awaited."""
        return await asyncio.to_thread(self.sync.to_chunks, path, **kwargs)

    async def to_pdf(self, path: Optional[PathArg] = None) -> bytes:
        """:meth:`kakemono.Document.to_pdf`, awaited."""
        return await asyncio.to_thread(self.sync.to_pdf, path)

    async def analyze(self) -> Dict[str, Any]:
        """:meth:`kakemono.Document.analyze`, awaited."""
        return await asyncio.to_thread(self.sync.analyze)
