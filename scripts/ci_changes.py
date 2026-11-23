"""Whether a pull request changes anything that the build and the tests read.

A pull request that only changes documentation does not need the tests, the
Word fidelity check, SonarQube or the Python wheels. Every pull request carries
its release (CONTRIBUTING.md, "Releases"), so even one that only changes the
documentation changes `Cargo.toml`, `Cargo.lock` and `CHANGELOG.md`; it is
still documentation only when:

- every other file it changes is a Markdown file at the root, under `docs/`, or
  `python/README.md`, or `LICENSE`;
- `Cargo.toml` and `python/pyproject.toml` differ only by package metadata (the
  version, the description, the keywords, ...);
- `Cargo.lock` differs only by the version of the kakemono package.

    python3 scripts/ci_changes.py BASE HEAD    # prints code=true or code=false

The line is also appended to `$GITHUB_OUTPUT` when it is set. Anything that
cannot be read (a file that is not valid TOML) counts as code: it runs
everything.
"""

import os
import subprocess
import sys
import tomllib

# Files that nothing builds or tests.
DOC_DIRS = ("docs/",)
DOC_FILES = {"LICENSE", "python/README.md"}

# Keys of `[package]` (Cargo.toml) and `[project]` (pyproject.toml) that only
# describe the package: they change what crates.io and PyPI show, not the build.
CARGO_METADATA = {
    "version",
    "description",
    "keywords",
    "categories",
    "readme",
    "homepage",
    "documentation",
    "repository",
    "authors",
}
PYPROJECT_METADATA = {
    "description",
    "keywords",
    "classifiers",
    "readme",
    "authors",
    "maintainers",
    "urls",
}


def is_doc(path: str) -> bool:
    """A file that nothing builds or tests."""
    if path in DOC_FILES or path.startswith(DOC_DIRS):
        return True
    return "/" not in path and path.endswith(".md")


def without(data: dict, table: str, keys: set) -> dict:
    """`data` without the `keys` of its `table`."""
    data = dict(data)
    data[table] = {k: v for k, v in data.get(table, {}).items() if k not in keys}
    return data


def same_cargo_toml(old: dict, new: dict) -> bool:
    return without(old, "package", CARGO_METADATA) == without(new, "package", CARGO_METADATA)


def same_pyproject(old: dict, new: dict) -> bool:
    return without(old, "project", PYPROJECT_METADATA) == without(
        new, "project", PYPROJECT_METADATA
    )


def same_cargo_lock(old: dict, new: dict) -> bool:
    """The same lock file, but for the version of the kakemono package."""

    def without_version(lock: dict) -> dict:
        lock = dict(lock)
        lock["package"] = [
            {k: v for k, v in p.items() if k != "version"} if p.get("name") == "kakemono" else p
            for p in lock.get("package", [])
        ]
        return lock

    return without_version(old) == without_version(new)


SAME = {
    "Cargo.toml": same_cargo_toml,
    "Cargo.lock": same_cargo_lock,
    "python/pyproject.toml": same_pyproject,
}


def git(*args: str) -> str:
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def toml_at(rev: str, path: str) -> dict:
    """The file `path` at `rev`, read as TOML; empty when it does not exist."""
    try:
        return tomllib.loads(git("show", f"{rev}:{path}"))
    except subprocess.CalledProcessError:
        return {}


def code_changes(changed: list[str], read) -> list[str]:
    """The files of `changed` that the build or the tests read. `read(path)`
    gives the file before and after the change, as TOML."""
    code = []
    for path in changed:
        if is_doc(path):
            continue
        same = SAME.get(path)
        if same is None:
            code.append(path)
            continue
        try:
            old, new = read(path)
        except tomllib.TOMLDecodeError:
            code.append(path)
            continue
        if not same(old, new):
            code.append(path)
    return code


def main() -> int:
    base, head = sys.argv[1], sys.argv[2]
    changed = git("diff", "--name-only", f"{base}...{head}").split()
    merge_base = git("merge-base", base, head).strip()
    code = code_changes(changed, lambda p: (toml_at(merge_base, p), toml_at(head, p)))
    for path in code:
        print(f"code: {path}")
    line = f"code={'true' if code or not changed else 'false'}"
    print(line)
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as out:
            out.write(line + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
