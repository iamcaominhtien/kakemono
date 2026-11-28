"""Tests of kakemono.aio, the awaited functions, on the LIGO paper."""

import asyncio
import os
import inspect
import sys
import threading
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
    assert doc.format == "pdf"
    assert doc.page_count == 16
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

    converting = main()
    with pytest.raises(ValueError):
        asyncio.run(converting)
    opening = aio.Document.open("no/such/file.pdf")
    with pytest.raises(FileNotFoundError):
        asyncio.run(opening)


def test_the_functions_keep_their_signature_and_docs():
    for name in aio.__all__:
        if name in ("Document", "ProcessError", "ConvertResult", "convert_many"):
            continue
        function, sync = getattr(aio, name), getattr(kakemono, name)
        assert inspect.iscoroutinefunction(function), name
        params = inspect.signature(function).parameters
        assert list(params) == [p for p in inspect.signature(sync).parameters if p != "options"] + ["isolate"] + (
            ["options"] if "options" in inspect.signature(sync).parameters else []
        ), name
        assert function.__doc__ == sync.__doc__, name


# --- stopping a conversion ---------------------------------------------------


def _full_seconds():
    start = time.perf_counter()
    kakemono.to_markdown(LIGO)
    return time.perf_counter() - start


def test_a_token_cancelled_from_another_thread_stops_the_conversion(tmp_path):
    full = _full_seconds()
    token = kakemono.CancelToken()
    timer = threading.Timer(full / 3, token.cancel)
    timer.start()
    start = time.perf_counter()
    with pytest.raises(kakemono.Cancelled):
        kakemono.to_markdown(LIGO, tmp_path / "ligo.md", cancel=token)
    took = time.perf_counter() - start
    timer.join()
    assert took < full / 3 + 0.5
    assert token.cancelled
    assert list(tmp_path.iterdir()) == [], "a cancelled conversion writes no file"


def test_a_cancelled_token_does_not_start_the_work():
    token = kakemono.CancelToken()
    token.cancel()
    start = time.perf_counter()
    with pytest.raises(kakemono.Cancelled):
        kakemono.Document(LIGO, cancel=token)
    assert time.perf_counter() - start < 0.05


def test_a_time_limit_raises_timeout_error():
    with pytest.raises(TimeoutError):
        kakemono.to_markdown(LIGO, timeout=0.02)
    with pytest.raises(TimeoutError):
        kakemono.to_markdown(LIGO, cancel=kakemono.CancelToken(timeout=0.02))
    with pytest.raises(ValueError):
        kakemono.to_markdown(LIGO, timeout=0)


def test_a_child_token_is_stopped_by_its_parent_only_one_way():
    parent = kakemono.CancelToken()
    child = parent.child(timeout=60)
    child.cancel()
    assert not parent.cancelled
    assert kakemono.to_markdown(LIGO, cancel=parent)  # the parent goes on
    parent.cancel()
    assert parent.child().cancelled


def test_a_document_stays_usable_after_a_cancelled_method():
    doc = kakemono.Document(LIGO)
    token = kakemono.CancelToken()
    token.cancel()
    with pytest.raises(kakemono.Cancelled):
        doc.to_epub(cancel=token)
    assert doc.to_markdown() == kakemono.to_markdown(LIGO)


def test_cancelling_the_task_stops_the_conversion_before_it_is_raised(tmp_path):
    full = _full_seconds()
    out = tmp_path / "ligo.md"

    async def main():
        task = asyncio.create_task(aio.to_markdown(LIGO, out))
        await asyncio.sleep(full / 3)
        start = time.perf_counter()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        return time.perf_counter() - start

    stopped = asyncio.run(main())
    assert stopped < 0.5
    time.sleep(full)  # had it gone on in its thread, it would have written by now
    assert not out.exists()


def test_asyncio_timeouts_stop_the_conversion():
    async def main():
        start = time.perf_counter()
        with pytest.raises(asyncio.TimeoutError):
            await asyncio.wait_for(aio.to_markdown(LIGO), 0.03)
        waited = time.perf_counter() - start
        with pytest.raises(TimeoutError):
            await aio.to_markdown(LIGO, timeout=0.03)
        return waited

    assert asyncio.run(main()) < 0.5


def test_cancelling_one_method_of_a_document_leaves_the_others():
    async def main():
        doc = await aio.Document.open(LIGO)
        stopped = asyncio.create_task(doc.to_epub())
        kept = asyncio.create_task(doc.to_markdown())
        await asyncio.sleep(0.01)
        stopped.cancel()
        with pytest.raises(asyncio.CancelledError):
            await stopped
        return doc, await kept

    doc, md = asyncio.run(main())
    assert md == doc.sync.to_markdown()


def test_an_isolated_conversion_gives_the_same_result(tmp_path):
    (tmp_path / "a").mkdir()
    (tmp_path / "b").mkdir()

    async def main():
        return await aio.to_markdown(LIGO, tmp_path / "a" / "ligo.md", isolate=True)

    md = asyncio.run(main())
    assert md == kakemono.to_markdown(LIGO, tmp_path / "b" / "ligo.md")
    assert (tmp_path / "a" / "ligo.md").read_text() == md
    assert sorted(p.name for p in (tmp_path / "a" / "ligo_assets").iterdir()) == sorted(
        p.name for p in (tmp_path / "b" / "ligo_assets").iterdir()
    )
    assert asyncio.run(aio.to_markdown(LIGO, isolate=True)) == kakemono.to_markdown(LIGO)


def test_an_isolated_conversion_raises_its_errors():
    broken = aio.to_markdown(b"not a document", isolate=True)
    with pytest.raises(ValueError):
        asyncio.run(broken)
    missing = aio.to_markdown("no/such/file.pdf", isolate=True)
    with pytest.raises(FileNotFoundError):
        asyncio.run(missing)


def test_an_isolated_conversion_is_killed_when_cancelled_or_late():
    async def main():
        task = asyncio.create_task(aio.convert_all(LIGO, isolate=True))
        await asyncio.sleep(0.3)  # started, past the start of Python
        start = time.perf_counter()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        killed = time.perf_counter() - start
        start = time.perf_counter()
        with pytest.raises(TimeoutError):
            await aio.convert_all(LIGO, isolate=True, timeout=0.3)
        late = time.perf_counter() - start
        token = kakemono.CancelToken()
        asyncio.get_running_loop().call_later(0.3, token.cancel)
        with pytest.raises(kakemono.Cancelled):
            await aio.convert_all(LIGO, isolate=True, cancel=token)
        return killed, late

    killed, late = asyncio.run(main())
    assert killed < 0.2
    assert late < 0.5


def test_an_isolated_conversion_gives_its_warnings():
    content = kakemono.to_json(LIGO)
    content["blocks"][1]["text"] = "<unknown>tag</unknown>"
    with pytest.warns(UserWarning):
        asyncio.run(aio.to_markdown(LIGO, content=content, isolate=True))


def _stub_script():
    """scripts/make-aio-stub.py, imported (its name is not a module name)."""
    import importlib.util

    path = Path(__file__).resolve().parents[2] / "scripts" / "make-aio-stub.py"
    spec = importlib.util.spec_from_file_location("make_aio_stub", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_the_typed_stub_is_current():
    script = _stub_script()
    assert script.STUB.read_text() == script.stub(), "run scripts/make-aio-stub.py"


def test_the_stub_script_writes_and_checks(tmp_path, monkeypatch):
    script = _stub_script()
    monkeypatch.setattr(script, "STUB", tmp_path / "aio.pyi")
    monkeypatch.setattr(sys, "argv", ["make-aio-stub.py"])
    script.main()
    assert (tmp_path / "aio.pyi").read_text() == script.stub()
    monkeypatch.setattr(sys, "argv", ["make-aio-stub.py", "--check"])
    script.main()  # current: no error
    (tmp_path / "aio.pyi").write_text("old")
    with pytest.raises(SystemExit):
        script.main()


def test_the_isolated_process_hands_back_results_errors_and_warnings():
    import pickle

    from kakemono import _isolated

    def call(name, *args, **kwargs):
        return _isolated.outcome(_isolated.run(pickle.dumps((name, args, kwargs))))

    assert call("os.path:basename", "/a/b.txt") == "b.txt"
    with pytest.raises(ValueError):
        call("to_markdown", b"not a document")
    content = kakemono.to_json(LIGO)
    content["blocks"][1]["text"] = "<unknown>tag</unknown>"
    with pytest.warns(UserWarning):
        call("to_markdown", LIGO, content=content)
    # An outcome that cannot be pickled comes back as a RuntimeError.
    with pytest.raises(RuntimeError):
        call("threading:Lock")


def test_aio_convert_is_stopped_too():
    async def main():
        start = time.perf_counter()
        with pytest.raises(asyncio.TimeoutError):
            await asyncio.wait_for(aio.convert(LIGO, "medium"), 0.03)
        return time.perf_counter() - start

    assert asyncio.run(main()) < 0.5


def test_a_conversion_waiting_for_a_thread_is_dropped_at_once():
    from concurrent.futures import ThreadPoolExecutor

    async def main():
        asyncio.get_running_loop().set_default_executor(ThreadPoolExecutor(max_workers=1))
        busy = asyncio.create_task(aio.to_epub(LIGO))
        waiting = asyncio.create_task(aio.to_markdown(LIGO))
        await asyncio.sleep(0.01)
        start = time.perf_counter()
        waiting.cancel()
        with pytest.raises(asyncio.CancelledError):
            await waiting
        dropped = time.perf_counter() - start
        await busy
        return dropped

    assert asyncio.run(main()) < 0.05


def test_a_time_limit_counts_from_the_call():
    from concurrent.futures import ThreadPoolExecutor

    async def main():
        asyncio.get_running_loop().set_default_executor(ThreadPoolExecutor(max_workers=1))
        busy = asyncio.create_task(aio.to_markdown(LIGO))
        await asyncio.sleep(0.01)
        with pytest.raises(TimeoutError):  # its limit passed while it waited for the thread
            await aio.to_markdown(LIGO, timeout=0.05)
        await busy

    asyncio.run(main())


def test_an_isolated_conversion_takes_a_memoryview():
    data = memoryview(LIGO.read_bytes())
    assert asyncio.run(aio.to_markdown(data, isolate=True)) == kakemono.to_markdown(LIGO)


def test_an_isolated_conversion_times_out_with_its_token_as_in_a_thread():
    late = aio.convert_all(LIGO, isolate=True, cancel=kakemono.CancelToken(timeout=0.3))
    with pytest.raises(TimeoutError):
        asyncio.run(late)


def test_cancelled_is_a_kakemono_exception_that_pickles():
    import pickle

    assert kakemono.Cancelled.__module__ == "kakemono"
    error = pickle.loads(pickle.dumps(kakemono.Cancelled("stopped")))
    assert isinstance(error, kakemono.Cancelled)
    token = kakemono.CancelToken(timeout=0.001)
    time.sleep(0.01)
    assert token.cancelled
    assert token.timed_out
    stopped = kakemono.CancelToken()
    stopped.cancel()
    assert stopped.cancelled
    assert not stopped.timed_out


# --- convert_many -------------------------------------------------------------

DOCX = Path(__file__).resolve().parents[2] / "tests" / "data" / "word" / "sample.docx"
XLSX = Path(__file__).resolve().parents[2] / "tests" / "data" / "excel" / "sample.xlsx"


def _many(*args, **kwargs):
    async def main():
        return [result async for result in aio.convert_many(*args, **kwargs)]

    return sorted(asyncio.run(main()), key=lambda result: result.index)


@pytest.mark.parametrize("isolate", [False, True])
def test_convert_many_writes_every_format_of_every_document(tmp_path, isolate):
    sources = [LIGO, DOCX, XLSX, b"not a document", ("named", DOCX.read_bytes())]
    results = _many(sources, tmp_path, ["md", "docx", "pdf", "medium"], limit=2, isolate=isolate)
    assert [result.index for result in results] == [0, 1, 2, 3, 4]
    ligo, docx, xlsx, broken, named = results
    # The PDF has no pdf output: only that format fails.
    assert set(ligo.outputs) == {"md", "docx", "medium"}
    assert set(ligo.errors) == {"pdf"}
    assert Path(ligo.outputs["md"]).read_text() == kakemono.to_markdown(LIGO, tmp_path / "again" / "1602.03837v1.md")
    assert Path(ligo.outputs["medium"]).name == "1602.03837v1.medium.html"
    # A Word document has no pages: no medium mode, but a PDF.
    assert set(docx.errors) == {"medium"}
    assert Path(docx.outputs["pdf"]).read_bytes().startswith(b"%PDF-")
    assert set(xlsx.outputs) == {"md", "docx", "pdf"}
    assert set(xlsx.errors) == {"medium"}  # no pages
    assert isinstance(broken.error, ValueError)
    assert broken.outputs == {}
    assert Path(named.outputs["md"]).name == "named.md"
    assert (tmp_path / "1602.03837v1_assets").is_dir()


def test_convert_many_gives_names_once():
    from kakemono import _batch

    names = [name for name, _ in _batch.output_names(["a/x.pdf", "b/x.pdf", b"..", "https://example.org/wiki/Rust", "1706.03762"])]
    assert names == ["x", "x-2", "document-3", "Rust", "1706.03762"]
    with pytest.raises(ValueError):
        _batch.check_formats(["md", "doc"])


@pytest.mark.parametrize("isolate", [False, True])
def test_convert_many_times_each_document_out(tmp_path, isolate):
    results = _many([LIGO] * 4 + [DOCX], tmp_path, ["epub", "md"], limit=5, timeout=0.05 if not isolate else 0.3, isolate=isolate)
    assert all(isinstance(result.error, TimeoutError) for result in results[:4])


def test_convert_many_runs_limit_documents_at_once(tmp_path):
    def running_most(limit):
        start = time.perf_counter()
        _many([LIGO] * 8, tmp_path / str(limit), ["md"], limit=limit, jobs=1)
        return time.perf_counter() - start

    # Eight documents one at a time take about four times as long as four
    # at a time (on at least four cores).
    if (os.cpu_count() or 1) >= 4:
        assert running_most(4) < running_most(1) * 0.6


def test_convert_many_stops_every_document_when_cancelled(tmp_path):
    async def main():
        task = asyncio.create_task(_drain(aio.convert_many([LIGO] * 6, tmp_path, ["epub"], limit=3)))
        await asyncio.sleep(0.05)
        start = time.perf_counter()
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        return time.perf_counter() - start

    assert asyncio.run(main()) < 0.5
    time.sleep(0.5)
    assert not list(tmp_path.glob("*.epub")), "no document goes on after the cancellation"


async def _drain(results):
    return [result async for result in results]
