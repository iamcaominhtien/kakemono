"""The process of an isolated conversion (``isolate=True`` in
:mod:`kakemono.aio`): reads ``(name, args, kwargs)`` pickled on its standard
input, runs ``kakemono.<name>(*args, **kwargs)`` (or, for a name
``module:function``, that function) and writes ``(status, value,
warnings)`` pickled on its standard output, where status is ``"ok"`` (value
is the result) or ``"error"`` (value is the exception)."""

from __future__ import annotations

import importlib
import os
import pickle
import sys
import warnings
from typing import Any, Callable, List, Tuple


def main() -> None:
    """Runs the conversion read on the standard input."""
    request = sys.stdin.buffer.read()
    # The result goes to a copy of the standard output, and anything else
    # written there (by the library or a warning) to the standard error.
    result = os.fdopen(os.dup(sys.stdout.fileno()), "wb")
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    sys.stdout = sys.stderr
    result.write(run(request))
    result.flush()


def run(request: bytes) -> bytes:
    """The pickled outcome of the pickled call `request`: its result or its
    exception, with the warnings it gave."""
    name, args, kwargs = pickle.loads(request)
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        try:
            outcome: Tuple[str, Any] = ("ok", _function(name)(*args, **kwargs))
        except Exception as error:  # handed to the parent, which raises it
            outcome = ("error", error)
    shown: List[Tuple[str, Any]] = [(str(warning.message), warning.category) for warning in caught]
    try:
        return pickle.dumps((*outcome, shown), protocol=pickle.HIGHEST_PROTOCOL)
    except Exception as error:  # an exception or a result that cannot be pickled
        status, value = outcome
        failure = RuntimeError(f"{type(value).__name__}: {value}" if status == "error" else str(error))
        return pickle.dumps(("error", failure, shown), protocol=pickle.HIGHEST_PROTOCOL)


def outcome(output: bytes, stacklevel: int = 2) -> Any:
    """The result of the pickled outcome `output` of :func:`run`, its
    warnings given again; its exception is raised."""
    status, value, shown = pickle.loads(output)
    for message, category in shown:
        warnings.warn(message, category, stacklevel=stacklevel)
    if status == "error":
        raise value
    return value


def _function(name: str) -> Callable[..., Any]:
    """The function named `name`: one of kakemono, or ``module:function``."""
    module, _, function = name.rpartition(":")
    return getattr(importlib.import_module(module or "kakemono"), function)


if __name__ == "__main__":
    main()
