#!/usr/bin/env python3
"""Focused behavior fixture for the log compressor and PostToolUse hook."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time


HERE = Path(__file__).resolve().parent
HOOK = HERE / "logcompress-hook.py"
SPEC = importlib.util.spec_from_file_location("logcompress", HERE / "logcompress.py")
assert SPEC is not None and SPEC.loader is not None
LOGCOMPRESS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LOGCOMPRESS)

failures: list[str] = []
checks = 0


def check(label: str, condition: bool) -> None:
    global checks
    checks += 1
    if condition:
        print("  PASS ", label)
    else:
        failures.append(label)
        print("  FAIL ", label)


scratch = Path(tempfile.mkdtemp(prefix="logcompress-cache-"))
try:
    cache_root = scratch / "cache"
    inactive = scratch / "inactive.json"
    missing_activation = scratch / "missing-activation.json"
    base_environment = {
        "XDG_CACHE_HOME": str(cache_root),
        "NORTH_AGENT_ACTIVATION": str(missing_activation),
        "NORTH_AGENT_PYTHON": sys.executable,
    }

    def run(payload: object, environment: dict[str, str] | None = None) -> str:
        hook_environment = dict(os.environ)
        hook_environment.update(base_environment)
        if environment:
            hook_environment.update(environment)
        result = subprocess.run(
            [sys.executable, str(HOOK)],
            input=json.dumps(payload),
            capture_output=True,
            text=True,
            timeout=5,
            env=hook_environment,
            check=True,
        )
        return result.stdout

    storm = (
        "[12:00:00] INFO start\n"
        + "\n".join(
            f"[12:00:{index + 1:02d}] WARN db refused, retrying"
            for index in range(26)
        )
        + "\n[12:00:27] ERROR gave up: host=db-primary\n"
        + "[12:00:28] INFO ok\n"
    )

    view, saved, dropped = LOGCOMPRESS.compress(storm)
    check("collapses timestamped run", dropped == 25)
    check("view is smaller than original", len(view) < len(storm))
    check("count recoverable via expand", LOGCOMPRESS.expand(view).count("WARN") == 26)
    check("keeps non-collapsed lines", "ERROR gave up: host=db-primary" in view)
    check("strips ANSI", "\x1b[" not in LOGCOMPRESS.compress("\x1b[31m" + storm)[0])

    small = "\x1b[32mline\x1b[0m\na\na\na\n"
    small_view, _, small_dropped = LOGCOMPRESS.compress(small)
    check("small output not collapsed below gate", small_dropped == 0)
    check("small output ANSI stripped below gate", "\x1b[" not in small_view)

    trace = "\n".join(
        f'  File "app/x{index}.py", line {index}, in f{index}'
        for index in range(30)
    )
    check("distinct stack lines remain", LOGCOMPRESS.compress(trace)[2] == 0)

    def bash_result(stdout: str) -> dict[str, object]:
        return {
            "stdout": stdout,
            "stderr": "",
            "interrupted": False,
            "isImage": False,
            "noOutputExpected": False,
        }

    bash_storm = {"tool_name": "Bash", "tool_response": bash_result(storm)}
    output = json.loads(run(bash_storm))["hookSpecificOutput"]["updatedToolOutput"]
    check(
        "hook emits a shape-preserving replacement",
        isinstance(output, dict)
        and "×26" in output["stdout"]
        and "collapsed 25 repeated line(s)" in output["stdout"]
        and "saved " in output["stdout"]
        and "ANSI" not in output["stdout"]
        and output["stderr"] == ""
        and output["interrupted"] is False
        and output["isImage"] is False
        and output["noOutputExpected"] is False,
    )

    inactive.write_text(
        json.dumps(
            {
                "schema": "north.agent-activation/v1",
                "units": [
                    {
                        "id": "logcompress-hook",
                        "kind": "hook",
                        "category": "context",
                        "active": False,
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    check(
        "inactive generation preserves the original",
        run(bash_storm, {"NORTH_AGENT_ACTIVATION": str(inactive)}) == "",
    )

    note = output["stdout"]
    digest = note.rsplit("/", 1)[1].split(".json", 1)[0]
    stashed = (cache_root / "claude-logcompress" / f"{digest}.json").read_text()
    check("hook stashes the original", stashed == storm)
    check("retrieval note uses cat", "cat " in note and "eso retrieve" not in note)
    check(
        "non-Bash tool passes through",
        run({"tool_name": "Read", "tool_response": storm}) == "",
    )
    check(
        "non-repetitive Bash passes through",
        run({"tool_name": "Bash", "tool_response": bash_result(trace)}) == "",
    )
    check(
        "legacy string response fails open",
        run({"tool_name": "Bash", "tool_response": storm}) == "",
    )
    malformed = subprocess.run(
        [sys.executable, str(HOOK)],
        input="not json",
        capture_output=True,
        text=True,
        timeout=5,
        env={**os.environ, **base_environment},
        check=True,
    )
    check("malformed input fails open", malformed.stdout == "")

    held_open = subprocess.Popen(
        [sys.executable, str(HOOK)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env={**os.environ, **base_environment},
    )
    started = time.monotonic()
    status = held_open.wait(timeout=3)
    elapsed = time.monotonic() - started
    assert held_open.stdin is not None
    held_open.stdin.close()
    stdout = held_open.stdout.read() if held_open.stdout is not None else b""
    stderr = held_open.stderr.read() if held_open.stderr is not None else b""
    check(
        "held-open stdin is cut off before the provider deadline",
        status == 0 and stdout == b"" and stderr == b"" and 0.8 <= elapsed < 3,
    )

    isolated = scratch / "isolated"
    isolated.mkdir()
    isolated_hook = isolated / HOOK.name
    shutil.copyfile(HOOK, isolated_hook)
    missing_module = subprocess.run(
        [sys.executable, str(isolated_hook)],
        input=json.dumps(bash_storm),
        capture_output=True,
        text=True,
        timeout=5,
        env={**os.environ, **base_environment},
        check=True,
    )
    check("missing compressor module fails open", missing_module.stdout == "")

    pid_file = scratch / "hung-inner.pid"
    started = time.monotonic()
    hung = run(
        bash_storm,
        {
            "LOGCOMPRESS_TEST_HANG": "1",
            "LOGCOMPRESS_TEST_PID_FILE": str(pid_file),
        },
    )
    elapsed = time.monotonic() - started
    inner_pid = int(pid_file.read_text())
    try:
        os.kill(inner_pid, 0)
        inner_dead = False
    except ProcessLookupError:
        inner_dead = True
    check(
        "inner deadline turns a hung compressor into a clean no-op",
        hung == "" and 1.7 <= elapsed < 4 and inner_dead,
    )

    ansi_heavy = "\n".join(
        "\x1b[38;2;255;128;0m\x1b[1m\x1b[4m\x1b[48;2;0;0;128m"
        f"[bold-color-underline-bg] line {index}\x1b[0m"
        for index in range(10)
    )
    _, ansi_saved, ansi_dropped = LOGCOMPRESS.compress(ansi_heavy)
    ansi_output = run(
        {"tool_name": "Bash", "tool_response": bash_result(ansi_heavy)}
    )
    ansi_stdout = json.loads(ansi_output)["hookSpecificOutput"][
        "updatedToolOutput"
    ]["stdout"]
    check(
        "ANSI-only savings above 200 chars emit",
        ansi_saved > 200
        and ansi_dropped == 0
        and "stripped " in ansi_stdout
        and "chars total" not in ansi_stdout,
    )

    ansi_light = "\x1b[32mok\x1b[0m"
    _, light_saved, light_dropped = LOGCOMPRESS.compress(ansi_light)
    check(
        "ANSI-only savings at or below 200 chars pass through",
        light_saved <= 200
        and light_dropped == 0
        and run({"tool_name": "Bash", "tool_response": bash_result(ansi_light)}) == "",
    )
finally:
    shutil.rmtree(scratch)

print()
if failures:
    print(f"logcompress: {len(failures)} FAILED of {checks}")
    raise SystemExit(1)
print(f"logcompress: {checks} / {checks} PASS")
