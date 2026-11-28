"""Tests of kakemono.aio, the awaited functions, on the LIGO paper."""

import asyncio
import inspect
import time
import zipfile
from pathlib import Path

import pytest

import kakemono
from kakemono import aio

LIGO = Path(__file__).resolve().parents[2] / "tests" / "data" / "1602.03837v1.pdf"
WORD = Path(__file__).resolve().parents[2] / "tests" / "data" / "word" / "sample.docx"


async def _ticks_during(work):
    """The result of awaiting `work`, the seconds it took, and how many times
    a task sleeping 5 ms at a time woke up meanwhile."""
    ticks = 0
    done = asyncio.Event()

    async def heartbeat():
        nonlocal ticks
        while not done.is_set():
            await asyncio.sleep(0.005)
            ticks += 1

    beat = asyncio.create_task(heartbeat())
    start = time.perf_counter()
    try:
        result = await work
    finally:
        done.set()
        await beat
    return result, time.perf_counter() - start, ticks


def test_the_event_loop_keeps_running_during_a_conversion():
    md, seconds, ticks = asyncio.run(_ticks_during(aio.to_markdown(LIGO)))
    assert "Observation of Gravitational Waves" in md
    # A blocked loop would wake the heartbeat once, after the conversion.
    assert seconds > 0.05
    assert ticks >= seconds / 0.005 / 4


def test_a_synchronous_call_would_block_the_loop():
    # The control of the test above: the same work, not in a thread.
    async def blocking():
        return kakemono.to_markdown(LIGO)

    _, seconds, ticks = asyncio.run(_ticks_during(blocking()))
    assert seconds > 0.05
    assert ticks <= 2


def test_results_are_those_of_the_synchronous_functions(tmp_path):
    async def main():
        return await asyncio.gather(
            aio.to_markdown(LIGO),
            aio.convert(LIGO, "medium", embed_fonts=False, scale=1.0),
            aio.to_chunks(LIGO, max_tokens=256),
            aio.to_docx(LIGO, tmp_path / "ligo.docx", scale=1.0),
        )

    md, html, chunks, docx = asyncio.run(main())
    assert md == kakemono.to_markdown(LIGO)
    assert html == kakemono.convert(LIGO, "medium", embed_fonts=False, scale=1.0)
    assert chunks == kakemono.to_chunks(LIGO, max_tokens=256)
    assert (tmp_path / "ligo.docx").read_bytes() == docx
    with zipfile.ZipFile(tmp_path / "ligo.docx") as package:
        assert package.testzip() is None


def test_conversions_run_in_parallel():
    count = 4

    async def one():
        return await aio.to_markdown(LIGO)

    async def many():
        return await asyncio.gather(*(aio.to_markdown(LIGO, jobs=1) for _ in range(count)))

    async def timed(work):
        start = time.perf_counter()
        result = await work
        return result, time.perf_counter() - start

    _, single = asyncio.run(timed(aio.to_markdown(LIGO, jobs=1)))
    results, together = asyncio.run(timed(many()))
    assert len(set(results)) == 1
    # Run one after another they would take `count` times as long.
    assert together < single * count * 0.75


def test_a_document_is_opened_once_and_awaited():
    async def main():
        doc = await aio.Document.open(LIGO)
        md, content, book = await asyncio.gather(doc.to_markdown(), doc.to_json(), doc.to_epub())
        return doc, md, content, book

    doc, md, content, book = asyncio.run(main())
    assert doc.format == "pdf" and doc.page_count == 16
    assert repr(doc) == "<kakemono.aio.Document pdf, 16 pages>"
    assert md == doc.sync.to_markdown()
    assert content["blocks"] == kakemono.to_json(LIGO)["blocks"]
    assert book[:2] == b"PK"


def test_a_word_document_is_laid_out_awaited():
    pdf = asyncio.run(aio.to_pdf(WORD))
    assert pdf.startswith(b"%PDF-")


def test_errors_are_raised_where_awaited():
    async def main():
        await aio.to_markdown(b"not a document")

    with pytest.raises(ValueError):
        asyncio.run(main())
    with pytest.raises(FileNotFoundError):
        asyncio.run(aio.Document.open("no/such/file.pdf"))


def test_the_functions_keep_their_signature_and_docs():
    for name in aio.__all__:
        if name == "Document":
            continue
        function, sync = getattr(aio, name), getattr(kakemono, name)
        assert inspect.iscoroutinefunction(function), name
        assert inspect.signature(function) == inspect.signature(sync), name
        assert function.__doc__ == sync.__doc__, name
