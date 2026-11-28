"""The process of an isolated conversion (``isolate=True`` in
:mod:`kakemono.aio`): reads ``(name, args, kwargs)`` pickled on its standard
input, runs ``kakemono.<name>(*args, **kwargs)`` (or, for a name
``module:function``, that function) and writes ``(status, value,
warnings)`` pickled on its standard output, where status is ``"ok"`` (value
is the result) or ``"error"`` (value is the exception)."""

from __future__ import annotations

import os
import pickle
import sys
import warnings


def main() -> None:
    """Runs the conversion read on the standard input."""
    name, args, kwargs = pickle.load(sys.stdin.buffer)
    # The result goes to a copy of the standard output, and anything else
    # written there (by the library or a warning) to the standard error.
    result = os.fdopen(os.dup(sys.stdout.fileno()), "wb")
    os.dup2(sys.stderr.fileno(), sys.stdout.fileno())
    sys.stdout = sys.stderr
    import importlib

    import kakemono

    # `name` is a function of kakemono, or `module:function`.
    module, _, function = name.rpartition(":")
    target = getattr(importlib.import_module(module) if module else kakemono, function)

    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        try:
            outcome = ("ok", target(*args, **kwargs))
        except Exception as error:  # handed to the parent, which raises it
            outcome = ("error", error)
    shown = [(str(warning.message), warning.category) for warning in caught]
    try:
        data = pickle.dumps((*outcome, shown), protocol=pickle.HIGHEST_PROTOCOL)
    except Exception as error:  # an exception that cannot be pickled
        failure = RuntimeError(f"{type(outcome[1]).__name__}: {outcome[1]}" if outcome[0] == "error" else str(error))
        data = pickle.dumps(("error", failure, shown), protocol=pickle.HIGHEST_PROTOCOL)
    result.write(data)
    result.flush()


if __name__ == "__main__":
    main()
