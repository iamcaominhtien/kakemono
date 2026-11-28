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

To convert many documents into several formats, ``limit`` at a time, each
with its own time limit, use :func:`convert_many`.

Cancelling the task stops the conversion: it stops at its next stopping
point, normally within a page, and the cancellation is raised once it has
stopped, so it writes no file afterwards (a download it was waiting for may
hold its connection until the download's own time limit, writing nothing).
A conversion still waiting for a thread of the executor is dropped at once.
``timeout`` (seconds, counted from the call) raises ``TimeoutError`` in the
same way; ``asyncio.wait_for`` (and ``asyncio.timeout`` from Python 3.11)
work too, and raise ``asyncio.TimeoutError``.

The conversions run in the loop's default executor, which has at most
``min(32, os.cpu_count() + 4)`` threads: more conversions at once wait for a
thread. To run more, give the loop a larger executor
(``loop.set_default_executor(ThreadPoolExecutor(max_workers=50))``) and each
conversion fewer threads of its own (``jobs=1``); :func:`convert_many` sets
up its threads itself.

With ``isolate=True``, a function runs in a process of its own, which a
cancellation or ``timeout`` kills at once: for documents that are not
trusted, or a hard time limit. It costs the start of a Python process
(about a tenth of a second), and its arguments and result are copied
between the processes; a killed conversion may leave a file it was writing
half written. A process that ends without a result (it crashed) raises
:class:`ProcessError`. Isolated conversions do not wait for the executor:
each starts its process at once, and uses every core (``jobs=0``), so bound
how many run at once with an ``asyncio.Semaphore``.
"""

from __future__ import annotations

import asyncio
import contextvars
import dataclasses
import functools
import inspect
import os
import pickle
import sys
import threading
import warnings
import time
from concurrent.futures import Executor, ThreadPoolExecutor
from typing import Any, AsyncIterator, Callable, Dict, Iterable, List, Literal, Optional, Sequence

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
    "ConvertResult",
    "Document",
    "ProcessError",
    "analyze",
    "convert",
    "convert_all",
    "convert_many",
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


async def _stopped(
    function: Callable[..., Any],
    *args: Any,
    cancel: Optional[CancelToken] = None,
    timeout: Optional[float] = None,
    _executor: Optional[Executor] = None,
    **kwargs: Any,
) -> Any:
    """`function(*args, **kwargs)` run in a worker thread (of `_executor`, by
    default the loop's) with a token of its own (a child of `cancel`, timing
    out `timeout` seconds from now), which a cancellation of the awaiting
    task cancels; the cancellation is raised once the work has stopped, or at
    once when it has not started."""
    token = cancel.child(timeout=timeout) if cancel is not None else CancelToken(timeout=timeout)
    started = threading.Event()
    context = contextvars.copy_context()  # the caller's, as asyncio.to_thread gives it

    def call() -> Any:
        started.set()
        return context.run(function, *args, cancel=token, **kwargs)

    work = asyncio.get_running_loop().run_in_executor(_executor, call)
    try:
        return await asyncio.shield(work)
    except asyncio.CancelledError:
        token.cancel()
        if not started.is_set():
            # Still waiting for a thread: when it gets one, it stops at
            # once on the cancelled token, before any work.
            work.add_done_callback(_retrieve)
            raise
        # The work stops at its next stopping point: wait for it, even if
        # cancelled again meanwhile, so that nothing of it runs afterwards.
        while not work.done():
            try:
                await asyncio.shield(work)
            except asyncio.CancelledError:
                continue
            except Exception:
                break
        _retrieve(work)
        raise


def _retrieve(work: "asyncio.Future[Any]") -> None:
    """Marks the outcome of `work` as seen: it is the cancellation's doing."""
    if work.done() and not work.cancelled():
        work.exception()


async def _isolated(name: str, args: Any, kwargs: Dict[str, Any]) -> Any:
    """`kakemono.<name>(*args, **kwargs)` run in a Python process of its own,
    killed when the awaiting task is cancelled, its ``timeout`` passes or its
    ``cancel`` token is cancelled."""
    cancel: Optional[CancelToken] = kwargs.pop("cancel", None)
    timeout: Optional[float] = kwargs.pop("timeout", None)
    if timeout is not None and not timeout > 0:
        raise ValueError("timeout must be a positive number of seconds")
    # A memoryview cannot be pickled: its bytes are sent instead.
    plain = lambda value: bytes(value) if isinstance(value, (memoryview, bytearray)) else value  # noqa: E731
    args = tuple(plain(value) for value in args)
    kwargs = {key: plain(value) for key, value in kwargs.items()}
    request = pickle.dumps((name, args, kwargs), protocol=pickle.HIGHEST_PROTOCOL)
    package_parent = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    # Isolated mode (-I): no module of the current folder or of PYTHONPATH
    # is imported in place of the standard library's.
    process = await asyncio.create_subprocess_exec(
        sys.executable,
        "-I",
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
            if await _kill(process, exchange):
                raise asyncio.CancelledError
            if watch is not None and watch in done and not cancel.timed_out:
                raise _cancelled_error()
            raise TimeoutError("the conversion took longer than its time limit")
        output, _ = exchange.result()
    except asyncio.CancelledError:
        await _kill(process, exchange)
        raise
    finally:
        if watch is not None:
            watch.cancel()
            await asyncio.gather(watch, return_exceptions=True)
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


async def _kill(process: Any, exchange: Any) -> bool:
    """Kills `process` and waits for it to end, even when cancelled again
    meanwhile, so that no process is left behind; returns whether a
    cancellation came meanwhile (the caller raises it)."""
    if process.returncode is None:
        try:
            process.kill()
        except ProcessLookupError:
            pass
    ended = asyncio.gather(exchange, return_exceptions=True)
    cancelled = False
    while not ended.done():
        try:
            await asyncio.shield(ended)
        except asyncio.CancelledError:
            cancelled = True
    return cancelled


def _cancelled_error() -> BaseException:
    """The exception of a conversion stopped by its token's ``cancel()``."""
    from . import Cancelled

    return Cancelled("the conversion was cancelled")


def _awaited(function: Callable[..., Any]) -> Callable[..., Any]:
    """`function`, awaited: run in a worker thread with the same arguments
    (and ``isolate``), signature and docstring."""
    # `convert` hands its options (`cancel` among them) on to `convert_all`.
    stoppable = any(
        param.name == "cancel" or param.kind is inspect.Parameter.VAR_KEYWORD
        for param in inspect.signature(function).parameters.values()
    )

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


@dataclasses.dataclass
class ConvertResult:
    """What :func:`convert_many` did with one document."""

    #: Its place in the `sources` given (from 0).
    index: int
    #: The source, as given.
    source: Any
    #: The file written for each format, by format.
    outputs: Dict[str, str]
    #: The error of each format that could not be written (a PDF has no
    #: ``pdf`` output, for example): the other formats are written.
    errors: Dict[str, BaseException]
    #: The error that stopped the whole document: it could not be read, or
    #: it ran out of time (``TimeoutError``) or was cancelled
    #: (``kakemono.Cancelled``); the formats written before it are kept
    #: (with ``isolate=True``, they are not known: `outputs` is empty).
    error: Optional[BaseException]
    #: How long the document took, in seconds.
    seconds: float

    @property
    def ok(self) -> bool:
        """Whether every format was written."""
        return self.error is None and not self.errors


async def convert_many(
    sources: Iterable[Any],
    out_dir: PathArg,
    formats: Sequence[str] = ("md",),
    *,
    limit: Optional[int] = None,
    isolate: bool = False,
    timeout: Optional[float] = None,
    jobs: Optional[int] = None,
    options: Optional[Dict[str, Dict[str, Any]]] = None,
    cancel: Optional[CancelToken] = None,
) -> AsyncIterator[ConvertResult]:
    """Convert many documents, `limit` at a time, each into every one of
    `formats`, and yield a :class:`ConvertResult` for each one as soon as it
    is done (not in the order given: sort them by ``index`` for that).

    >>> async for result in aio.convert_many(paths, "out", ["docx", "md", "pdf"], limit=50, timeout=120):
    ...     print(result.source, "ok" if result.ok else result.error or result.errors)

    - `sources`: paths, arXiv ids, web addresses or bytes, as for
      :class:`kakemono.Document`; a ``(name, source)`` pair names its files.
    - `out_dir`: where the files go, ``<name>.<ext>`` as ``kakemono convert``
      writes them, with the pictures in ``<name>_assets``
      (``<name>_<format>_assets`` when several formats write pictures). A
      name is the file's name without its extension, an arXiv id, the last
      part of a web address, or ``document-<n>`` for bytes; a name already
      taken gets ``-2``, ``-3``, ...
    - `formats`: any of ``html`` (the friendly mode), ``friendly``,
      ``medium``, ``hard``, ``hard-95`` (``<name>.<mode>.html``), ``docx``,
      ``md``, ``tex``, ``typ``, ``epub``, ``json``, ``jsonl`` (chunks) and
      ``pdf`` (a Word document, workbook, book or web page laid out in
      pages). Each document is read and analysed once for all of them.
    - `limit`: how many documents at once; by default one per core. The
      threads are set up for it: there is no need to change the loop's
      executor.
    - `isolate`: each document in a Python process of its own, killed when
      it runs out of time or is cancelled (see the module's documentation).
      `limit` processes run at once: mind the memory.
    - `timeout`: the time limit of each document, in seconds, from when it
      starts (waiting for its turn does not count; with `isolate`, the start
      of its process does).
    - `jobs`: the threads of each document; by default the cores shared out
      among the `limit` documents (one each when there are as many
      documents as cores).
    - `options`: the options of each format by name, and those of
      :class:`kakemono.Document` under ``"document"``, for example
      ``{"document": {"tables": False}, "md": {"mode": "medium"}}``.
    - `cancel`: a :data:`kakemono.CancelToken` that stops them all.

    A document that fails does not stop the others: its error is in its
    result. Cancelling the task that iterates stops every document; so does
    leaving the loop early, once the iterator is closed (at the end of
    ``asyncio.run``, or at once with ``contextlib.aclosing`` on Python 3.10+).
    """
    from . import _batch

    formats = _batch.check_formats(formats)
    named = _batch.output_names(list(sources))
    cores = os.cpu_count() or 1
    limit = limit if limit is not None else cores
    if limit < 1:
        raise ValueError("limit must be at least 1")
    jobs = jobs if jobs is not None else max(1, cores // limit)
    folder = os.fspath(out_dir)
    executor = None if isolate else ThreadPoolExecutor(max_workers=limit, thread_name_prefix="kakemono")
    gate = asyncio.Semaphore(limit)

    async def one(index: int) -> ConvertResult:
        name, source = named[index]
        call = dict(source=source, out_dir=folder, name=name, formats=formats, jobs=jobs, options=options)
        async with gate:
            start = time.perf_counter()
            try:
                if isolate:
                    outcome = await _isolated(
                        "kakemono._batch:convert_one", (), {**call, "cancel": cancel, "timeout": timeout}
                    )
                else:
                    outcome = await _stopped(
                        _batch.convert_one, cancel=cancel, timeout=timeout, _executor=executor, **call
                    )
                written, failed, error = outcome
            except asyncio.CancelledError:
                raise
            except Exception as stopped:  # killed by its time limit, crashed, ...
                written, failed, error = {}, {}, stopped
            return ConvertResult(index, source, written, failed, error, time.perf_counter() - start)

    tasks = [asyncio.ensure_future(one(index)) for index in range(len(named))]
    try:
        for done in asyncio.as_completed(tasks):
            yield await done
    finally:
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        if executor is not None:
            executor.shutdown(wait=False)
