"""Private sccache log -> allowlisted counters only; never echo raw errors."""

import json
import pathlib
import sys

from metrics import error_categories

log = pathlib.Path(sys.argv[1])
print(json.dumps(error_categories(log.read_text(errors="replace") if log.exists() else "")))
