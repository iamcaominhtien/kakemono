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
from typing import Optional

import kakemono
from kakemono import aio

STUB = Path(__file__).resolve().parents[1] / "python" / "kakemono" / "aio.pyi"


EMPTY = inspect.Parameter.empty

#: The line that makes a property of a stub's class.
PROPERTY = "    @property"

#: The `isolate` keyword the functions of kakemono.aio add.
ISOLATE = inspect.Parameter("isolate", inspect.Parameter.KEYWORD_ONLY, default=False, annotation="bool")


def param_source(param: inspect.Parameter) -> str:
    """One parameter as source: name, annotation, default, `*` or `**`."""
    text = param.name
    if param.annotation is not EMPTY:
        text += f": {param.annotation}"
    if param.default is not EMPTY:
        text += f" = {param.default!r}" if param.annotation is not EMPTY else f"={param.default!r}"
    stars = {inspect.Parameter.VAR_POSITIONAL: "*", inspect.Parameter.VAR_KEYWORD: "**"}
    return stars.get(param.kind, "") + text


def params(signature: inspect.Signature, extra: Optional[inspect.Parameter] = None) -> str:
    """The parameters of `signature` as source, with the keyword parameter
    `extra` added (before ``**kwargs``)."""
    values = list(signature.parameters.values())
    if extra is not None:
        last = values and values[-1].kind is inspect.Parameter.VAR_KEYWORD
        values.insert(len(values) - 1 if last else len(values), extra)
    out = []
    star = any(param.kind is inspect.Parameter.VAR_POSITIONAL for param in values)
    for param in values:
        if param.kind is inspect.Parameter.KEYWORD_ONLY and not star:
            out.append("*")
            star = True
        out.append(param_source(param))
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
        "__all__: List[str]",
        "",
        "class ProcessError(RuntimeError): ...",
        "",
        "class ConvertResult:",
        *(f"    {field.name}: {field.type}" for field in dataclasses.fields(aio.ConvertResult)),
        PROPERTY,
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
        lines.append(f"async def {name}({params(sync, ISOLATE)}) -> {ret}: ...")
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
        PROPERTY,
        '    def format(self) -> Literal["pdf", "docx", "doc", "xlsx", "epub", "html"]: ...',
        PROPERTY,
        "    def page_count(self) -> int: ...",
    ]
    for name in ("convert_all", "convert", "to_docx", "to_markdown", "to_json", "to_latex", "to_typst", "to_epub", "to_chunks", "to_pdf", "analyze"):
        sync = inspect.signature(getattr(kakemono.Document, name))
        lines.append(f"    async def {name}({params(sync)}) -> {sync.return_annotation}: ...")
    return "\n".join(lines) + "\n"


def main() -> None:
    """Writes the stub, or with ``--check`` exits with an error when it is out of date."""
    text = stub()
    if "--check" in sys.argv:
        if STUB.read_text() != text:
            sys.exit("python/kakemono/aio.pyi is out of date: run scripts/make-aio-stub.py")
        return
    STUB.write_text(text)


if __name__ == "__main__":
    main()
