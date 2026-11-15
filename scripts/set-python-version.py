"""Sets the version of the Python package (python/Cargo.toml) to the one given.

The release workflow bumps only the Rust crate's version; the Python wheels of a
release are built with the same version: `python scripts/set-python-version.py 0.4.0`.
"""

import pathlib
import re
import sys

MANIFEST = pathlib.Path(__file__).resolve().parent.parent / "python" / "Cargo.toml"


def set_version(manifest: pathlib.Path, version: str) -> None:
    """Replaces the first `version = "..."` line of `manifest` (the package's)."""
    text = manifest.read_text(encoding="utf-8")
    new, count = re.subn(r'(?m)^version = ".*"$', f'version = "{version}"', text, count=1)
    if count != 1:
        raise ValueError(f"no version line in {manifest}")
    manifest.write_text(new, encoding="utf-8")


if __name__ == "__main__":
    set_version(MANIFEST, sys.argv[1])
    print(f"{MANIFEST.name}: version {sys.argv[1]}")
