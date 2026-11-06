#!/usr/bin/env python3
"""Patch Core Git dependencies to this checkout for external guest checks."""
import json
from pathlib import Path
import sys
import tomllib

root = Path(__file__).resolve().parents[2]
manifest = tomllib.loads((root / "Cargo.toml").read_text())
patches = manifest["patch"]["https://github.com/xila-project/core"]
lines = ['[patch."https://github.com/xila-project/core"]']
for name, spec in sorted(patches.items()):
    lines.append(f'{json.dumps(name)} = {{ path = {json.dumps(str((root / spec["path"]).resolve()))} }}')
Path(sys.argv[1]).write_text("\n".join(lines) + "\n")
