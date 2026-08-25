#!/usr/bin/env python3
# Adapted from https://github.com/Green-PT/honey-for-devs hooks/ (MIT).
# Copyright (c) 2026 Green-PT
"""Fail-open Codex PostToolUse compression hook."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import time


HERE = Path(__file__).resolve().parent
MAX_INPUT_BYTES = 64 * 1024 * 1024
READ_TIMEOUT_SECONDS = 1.0
INNER_TIMEOUT_SECONDS = 2.0


def emit(payload: object) -> None:
    sys.stdout.write(json.dumps(payload, ensure_ascii=False, separators=(",", ":")))


def read_bounded_input() -> bytes | None:
    """Drain stdin completely, or return None at the size/deadline boundary."""
    deadline = time.monotonic() + READ_TIMEOUT_SECONDS
    chunks: list[bytes] = []
    size = 0
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return None
        ready, _, _ = select.select([sys.stdin.fileno()], [], [], remaining)
        if not ready:
            return None
        chunk = os.read(sys.stdin.fileno(), 64 * 1024)
        if not chunk:
            return b"".join(chunks)
        size += len(chunk)
        if size > MAX_INPUT_BYTES:
            return None
        chunks.append(chunk)


def hook_enabled() -> bool:
    environment = dict(os.environ)
    environment["NORTH_AGENT_PYTHON"] = sys.executable
    try:
        result = subprocess.run(
            [
                "bash",
                "-c",
                'source "$1" && north_hook_enabled "$2"',
                "logcompress-dial",
                str(HERE / "lib" / "harness-dial.sh"),
                "logcompress-hook",
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=environment,
            timeout=1.0,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return True
    return result.returncode != 1


def supervise() -> None:
    payload = read_bounded_input()
    if payload is None or not hook_enabled():
        return
    environment = dict(os.environ)
    environment["LOGCOMPRESS_INNER"] = "1"
    try:
        child = subprocess.run(
            [sys.executable, str(Path(__file__).resolve())],
            input=payload,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=environment,
            timeout=INNER_TIMEOUT_SECONDS,
            check=False,
        )
        if child.returncode != 0 or not child.stdout:
            return
        result = json.loads(child.stdout)
        output = result.get("hookSpecificOutput")
        if isinstance(output, dict) and output.get("hookEventName") == "PostToolUse":
            sys.stdout.buffer.write(child.stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        return


def inner() -> None:
    try:
        if os.environ.get("LOGCOMPRESS_TEST_HANG") == "1":
            pid_file = os.environ.get("LOGCOMPRESS_TEST_PID_FILE")
            if pid_file:
                Path(pid_file).write_text(str(os.getpid()), encoding="utf-8")
            time.sleep(30)

        from logcompress import compress

        payload = json.load(sys.stdin)
        if (payload.get("tool_name") or payload.get("toolName")) != "Bash":
            return
        response = payload.get("tool_response")
        if response is None:
            response = payload.get("toolResponse")
        if response is None:
            response = payload.get("tool_result")
        if not isinstance(response, dict):
            return

        text_field = next(
            (
                field
                for field in ("stdout", "output", "content")
                if isinstance(response.get(field), str)
            ),
            None,
        )
        if text_field is None or not response[text_field]:
            return
        text = response[text_field]
        view, saved, dropped = compress(text)
        if dropped < 1 and saved <= 200:
            return

        cache_root = os.environ.get("XDG_CACHE_HOME")
        if not cache_root:
            cache_root = str(Path.home() / ".cache")
        cache_dir = Path(cache_root) / "claude-logcompress"
        digest = hashlib.sha256(text.encode()).hexdigest()[:16]
        cache_path = cache_dir / f"{digest}.json"
        cache_tmp = cache_dir / f"{digest}.json.{os.getpid()}.tmp"
        cache_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        try:
            descriptor = os.open(
                cache_tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600
            )
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                handle.write(text)
            os.replace(cache_tmp, cache_path)
        finally:
            try:
                cache_tmp.unlink()
            except FileNotFoundError:
                pass

        parts: list[str] = []
        if dropped > 0:
            parts.append(f"collapsed {dropped} repeated line(s)")
            if saved > 0:
                parts.append(f"saved {saved} chars total")
        elif saved > 0:
            parts.append(f"stripped {saved} ANSI chars")
        note = (
            f"\n[logcompress: {', '.join(parts)}. "
            f"Full output: cat {cache_dir}/{digest}.json]"
        )
        replacement = dict(response)
        replacement[text_field] = view + note
        emit(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PostToolUse",
                    "updatedToolOutput": replacement,
                }
            }
        )
    except Exception:
        return


if __name__ == "__main__":
    try:
        if os.environ.get("LOGCOMPRESS_INNER") == "1":
            inner()
        else:
            supervise()
    except Exception:
        pass
