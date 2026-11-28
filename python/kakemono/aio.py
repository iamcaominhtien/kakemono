"""Asynchronous versions of the kakemono functions, for asyncio programs.

Each function here takes the arguments of the one of the same name in
:mod:`kakemono` and returns the same result, but is awaited: the work runs in
a worker thread (the event loop's default executor), so the event loop keeps
serving other tasks while a document is fetched, read, analysed and written.
The GIL is released while the Rust library works, so several conversions run
in parallel:

>>> import asyncio
>>> from kakemono import aio
>>> async def main():
...     md = await aio.to_markdown("paper.pdf", "out/paper.md")
...     books = await asyncio.gather(*(aio.to_epub(p) for p in paths))

To write several outputs of one document, open it once:

>>> doc = await aio.Document.open("paper.pdf")
>>> await doc.to_docx("out/paper.docx"); await doc.to_epub("out/paper.epub")

Cancelling the task stops the conversion: it stops at its next stopping
point, normally within a page, and the cancellation is raised once it has
stopped, so nothing of it is still running or writing files. ``timeout``
(seconds) raises ``TimeoutError`` in the same way; ``asyncio.timeout`` and
``asyncio.wait_for`` work too. The number of conversions at once is that of
the loop's default executor (``loop.set_default_executor``), or of an
``asyncio.Semaphore`` of your own.

With ``isolate=True``, a function runs in a process of its own, which a
cancellation or ``timeout`` kills at once: for documents that are not
trusted, or a hard time limit. It costs the start of a Python process
(about a tenth of a second), and its arguments and result are copied
between the processes; a killed conversion may leave a file it was writing
half written. A process that ends without a result (it crashed) raises
:class:`ProcessError`.
"""

from __future__ import annotations

import asyncio
import contextvars
import functools
import inspect
import os
import pickle
import sys
import warnings
from typing import Any, Callable, Dict, List, Literal, Optional

from . import (
    CancelToken,
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

__all__ = [
    "Document",
    "ProcessError",
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


class ProcessError(RuntimeError):
    """The process of an ``isolate=True`` conversion ended without a result
    (it crashed, or was killed from outside)."""


async def _stopped(function: Callable[..., Any], *args: Any, cancel: Optional[CancelToken] = None, **kwargs: Any) -> Any:
    """`function(*args, **kwargs)` run in a worker thread with a token of its
    own (a child of `cancel`), which a cancellation of the awaiting task
    cancels; the cancellation is raised once the work has stopped."""
    token = cancel.child() if cancel is not None else CancelToken()
    loop = asyncio.get_running_loop()
    call = functools.partial(function, *args, cancel=token, **kwargs)
    # The context of the caller goes with the work, as with asyncio.to_thread.
    work = loop.run_in_executor(None, functools.partial(contextvars.copy_context().run, call))
    try:
        return await asyncio.shield(work)
    except asyncio.CancelledError:
        token.cancel()
        # The work stops at its next stopping point: wait for it, even if
        # cancelled again meanwhile, so that nothing of it runs afterwards.
        while not work.done():
            try:
                await asyncio.shield(work)
            except asyncio.CancelledError:
                continue
            except BaseException:
                break
        if not work.cancelled():
            work.exception()  # retrieved: it is the cancellation's doing
        raise


async def _isolated(name: str, args: Any, kwargs: Dict[str, Any]) -> Any:
    """`kakemono.<name>(*args, **kwargs)` run in a Python process of its own,
    killed when the awaiting task is cancelled, its ``timeout`` passes or its
    ``cancel`` token is cancelled."""
    cancel: Optional[CancelToken] = kwargs.pop("cancel", None)
    timeout: Optional[float] = kwargs.pop("timeout", None)
    if timeout is not None and not timeout > 0:
        raise ValueError("timeout must be a positive number of seconds")
    request = pickle.dumps((name, args, kwargs), protocol=pickle.HIGHEST_PROTOCOL)
    package_parent = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    process = await asyncio.create_subprocess_exec(
        sys.executable,
        "-c",
        f"import sys; sys.path.insert(0, {package_parent!r}); from kakemono._isolated import main; main()",
        stdin=asyncio.subprocess.PIPE,
        stdout=asyncio.subprocess.PIPE,
    )
    exchange = asyncio.ensure_future(process.communicate(request))
    watch = asyncio.ensure_future(_watch(cancel)) if cancel is not None else None
    try:
        waiting = {exchange} if watch is None else {exchange, watch}
        done, _ = await asyncio.wait(waiting, timeout=timeout, return_when=asyncio.FIRST_COMPLETED)
        if exchange not in done:
            await _kill(process, exchange)
            if watch is not None and watch in done:
                raise _cancelled_error()
            raise TimeoutError("the conversion took longer than its time limit")
        output, _ = exchange.result()
    except asyncio.CancelledError:
        await _kill(process, exchange)
        raise
    finally:
        if watch is not None:
            watch.cancel()
    if process.returncode != 0 or not output:
        raise ProcessError(f"the conversion process ended without a result (exit code {process.returncode})")
    status, value, caught = pickle.loads(output)
    for message, category in caught:
        warnings.warn(message, category, stacklevel=3)
    if status == "error":
        raise value
    return value


async def _watch(cancel: CancelToken) -> None:
    """Returns once `cancel` is cancelled (or past its time limit)."""
    while not cancel.cancelled:
        await asyncio.sleep(_WATCH_INTERVAL_S)


#: How often an isolated conversion looks at its ``cancel`` token.
_WATCH_INTERVAL_S = 0.02


async def _kill(process: Any, exchange: Any) -> None:
    """Kills `process` and waits for it to end (shielded from a second
    cancellation, so that no process is left behind)."""
    if process.returncode is None:
        try:
            process.kill()
        except ProcessLookupError:
            pass
    ended = asyncio.gather(exchange, return_exceptions=True)
    while not ended.done():
        try:
            await asyncio.shield(ended)
        except asyncio.CancelledError:
            continue


def _cancelled_error() -> BaseException:
    """The exception of a conversion stopped by its token's ``cancel()``."""
    from . import Cancelled

    return Cancelled("the conversion was cancelled")


def _awaited(function: Callable[..., Any]) -> Callable[..., Any]:
    """`function`, awaited: run in a worker thread with the same arguments
    (and ``isolate``), signature and docstring."""
    stoppable = "cancel" in inspect.signature(function).parameters

    @functools.wraps(function)
    async def run(*args: Any, isolate: bool = False, **kwargs: Any) -> Any:
        if isolate:
            return await _isolated(function.__name__, args, kwargs)
        if stoppable:
            return await _stopped(function, *args, **kwargs)
        return await asyncio.to_thread(function, *args, **kwargs)

    signature = inspect.signature(function)
    isolate = inspect.Parameter("isolate", inspect.Parameter.KEYWORD_ONLY, default=False, annotation="bool")
    params = list(signature.parameters.values())
    if params and params[-1].kind is inspect.Parameter.VAR_KEYWORD:
        params.insert(len(params) - 1, isolate)
    else:
        params.append(isolate)
    run.__signature__ = signature.replace(parameters=params)  # type: ignore[attr-defined]
    del run.__wrapped__  # the signature is this one, with `isolate`
    return run


analyze = _awaited(_analyze)
convert = _awaited(_convert)
convert_all = _awaited(_convert_all)
fetch_arxiv = _awaited(_fetch_arxiv)
read_formula = _awaited(_read_formula)
render_svg = _awaited(_render_svg)
to_chunks = _awaited(_to_chunks)
to_docx = _awaited(_to_docx)
to_epub = _awaited(_to_epub)
to_json = _awaited(_to_json)
to_latex = _awaited(_to_latex)
to_markdown = _awaited(_to_markdown)
to_pdf = _awaited(_to_pdf)
to_typst = _awaited(_to_typst)


class Document:
    """A :class:`kakemono.Document` whose methods are awaited.

    Open one with :meth:`open`, which reads and analyses the document in a
    worker thread; its methods take the arguments of those of
    :class:`kakemono.Document` (``cancel`` and ``timeout`` included).
    Several of them can be awaited at once, and cancelling one stops its
    work only: the document stays open.
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
        cancel: Optional[CancelToken] = None,
        timeout: Optional[float] = None,
    ) -> "Document":
        """Read and analyse `source`, with the options of :class:`kakemono.Document`."""
        document = await _stopped(
            _Document,
            source,
            tables=tables,
            jobs=jobs,
            png_compression=png_compression,
            content=content,
            header_rows=header_rows,
            cancel=cancel,
            timeout=timeout,
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
        return await _stopped(self.sync.convert_all, *args, **kwargs)

    async def convert(self, *args: Any, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.convert`, awaited."""
        return await _stopped(self.sync.convert, *args, **kwargs)

    async def to_docx(self, path: Optional[PathArg] = None, **kwargs: Any) -> bytes:
        """:meth:`kakemono.Document.to_docx`, awaited."""
        return await _stopped(self.sync.to_docx, path, **kwargs)

    async def to_markdown(self, path: Optional[PathArg] = None, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.to_markdown`, awaited."""
        return await _stopped(self.sync.to_markdown, path, **kwargs)

    async def to_json(self, path: Optional[PathArg] = None, **kwargs: Any) -> Dict[str, Any]:
        """:meth:`kakemono.Document.to_json`, awaited."""
        return await _stopped(self.sync.to_json, path, **kwargs)

    async def to_latex(self, path: Optional[PathArg] = None, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.to_latex`, awaited."""
        return await _stopped(self.sync.to_latex, path, **kwargs)

    async def to_typst(self, path: Optional[PathArg] = None, **kwargs: Any) -> str:
        """:meth:`kakemono.Document.to_typst`, awaited."""
        return await _stopped(self.sync.to_typst, path, **kwargs)

    async def to_epub(self, path: Optional[PathArg] = None, **kwargs: Any) -> bytes:
        """:meth:`kakemono.Document.to_epub`, awaited."""
        return await _stopped(self.sync.to_epub, path, **kwargs)

    async def to_chunks(self, path: Optional[PathArg] = None, **kwargs: Any) -> List[Dict[str, Any]]:
        """:meth:`kakemono.Document.to_chunks`, awaited."""
        return await _stopped(self.sync.to_chunks, path, **kwargs)

    async def to_pdf(self, path: Optional[PathArg] = None, **kwargs: Any) -> bytes:
        """:meth:`kakemono.Document.to_pdf`, awaited."""
        return await _stopped(self.sync.to_pdf, path, **kwargs)

    async def analyze(self, **kwargs: Any) -> Dict[str, Any]:
        """:meth:`kakemono.Document.analyze`, awaited."""
        return await _stopped(self.sync.analyze, **kwargs)
