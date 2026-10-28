#!/usr/bin/env python3
import json
import os
import subprocess


def ask(op, **request):
    result = subprocess.run([os.environ.get("NORTH_PACE_BIN", "north"), "pace"], input=json.dumps({"op": op, **request}, default=str), capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise ValueError(result.stderr.strip())
    return json.loads(result.stdout)
