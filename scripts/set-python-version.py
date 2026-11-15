"""Sets the version of the Python package (python/Cargo.toml) to the one given.

The release workflow bumps only the Rust crate's version; the Python wheels of a
release are built with the same version: `python scripts/set-python-version.py 0.4.0`.
"""

import pathlib
import re
import sys

manifest = pathlib.Path(__file__).resolve().parent.parent / "python" / "Cargo.toml"
text = manifest.read_text(encoding="utf-8")
new, count = re.subn(r'(?m)^version = ".*"$', f'version = "{sys.argv[1]}"', text, count=1)
if count != 1:
    sys.exit(f"no version line in {manifest}")
manifest.write_text(new, encoding="utf-8")
print(f"{manifest.name}: version {sys.argv[1]}")
