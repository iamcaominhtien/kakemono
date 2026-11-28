"""Writes python/kakemono/aio.pyi, the typed signatures of kakemono.aio: those
of the functions and Document methods of kakemono, awaited, with `isolate`
on the functions. Run it (with the package installed) after changing a
signature of kakemono; python/tests/test_aio.py checks that it is current.

Usage: python scripts/make-aio-stub.py [--check]
"""

from __future__ import annotations

import dataclasses
import inspect
import sys
from pathlib import Path

import kakemono
from kakemono import aio

STUB = Path(__file__).resolve().parents[1] / "python" / "kakemono" / "aio.pyi"


def params(signature: inspect.Signature, extra: str = "", skip_self: bool = False) -> str:
    """The parameters of `signature` as source, with `extra` keyword
    parameters added."""
    out = []
    star = False
    for param in signature.parameters.values():
        if skip_self and param.name == "self":
            out.append("self")
            continue
        text = param.name
        if param.annotation is not inspect.Parameter.empty:
            text += f": {param.annotation}"
        if param.default is not inspect.Parameter.empty:
            text += f" = {param.default!r}" if param.annotation is not inspect.Parameter.empty else f"={param.default!r}"
        if param.kind is inspect.Parameter.KEYWORD_ONLY and not star:
            out.append("*")
            star = True
        if param.kind is inspect.Parameter.VAR_KEYWORD:
            text = "**" + text
            if extra:
                if not star:
                    out.append("*")
                    star = True
                out.append(extra)
                extra = ""
        out.append(text)
    if extra:
        if not star:
            out.append("*")
        out.append(extra)
    return ", ".join(out)


def stub() -> str:
    lines = [
        "# Written by scripts/make-aio-stub.py from the signatures of kakemono: do not edit.",
        "from typing import Any, AsyncIterator, Dict, Iterable, List, Literal, Optional, Sequence, Union",
        "",
        "from . import (",
        "    CancelToken,",
        "    Content,",
        "    Document as _Document,",
        "    Equations,",
        "    Mode,",
        "    PathArg,",
        "    PngCompression,",
        "    Source,",
        ")",
        "",
        f"__all__: List[str]",
        "",
        "class ProcessError(RuntimeError): ...",
        "",
        "class ConvertResult:",
        *(f"    {field.name}: {field.type}" for field in dataclasses.fields(aio.ConvertResult)),
        "    @property",
        "    def ok(self) -> bool: ...",
        "",
        f"def convert_many({params(inspect.signature(aio.convert_many))}) -> "
        f"{inspect.signature(aio.convert_many).return_annotation}: ...",
        "",
    ]
    for name in aio.__all__:
        if name in ("Document", "ProcessError", "ConvertResult", "convert_many"):
            continue
        sync = inspect.signature(getattr(kakemono, name))
        ret = sync.return_annotation
        lines.append(f"async def {name}({params(sync, 'isolate: bool = False')}) -> {ret}: ...")
    lines += [
        "",
        "class Document:",
        "    sync: _Document",
        "    def __init__(self, document: _Document) -> None: ...",
    ]
    opened = inspect.signature(kakemono.Document.__init__)
    opened = opened.replace(parameters=[p for p in opened.parameters.values() if p.name != "self"])
    lines.append("    @classmethod")
    lines.append(f"    async def open(cls, {params(opened)}) -> Document: ...")
    lines += [
        "    @property",
        '    def format(self) -> Literal["pdf", "docx", "doc", "xlsx", "epub", "html"]: ...',
        "    @property",
        "    def page_count(self) -> int: ...",
    ]
    for name in ("convert_all", "convert", "to_docx", "to_markdown", "to_json", "to_latex", "to_typst", "to_epub", "to_chunks", "to_pdf", "analyze"):
        sync = inspect.signature(getattr(kakemono.Document, name))
        lines.append(f"    async def {name}({params(sync, skip_self=True)}) -> {sync.return_annotation}: ...")
    return "\n".join(lines) + "\n"


if __name__ == "__main__":
    text = stub()
    if "--check" in sys.argv:
        sys.exit(0 if STUB.read_text() == text else "python/kakemono/aio.pyi is out of date: run scripts/make-aio-stub.py")
    STUB.write_text(text)
