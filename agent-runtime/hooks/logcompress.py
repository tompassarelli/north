# Adapted from https://github.com/Green-PT/honey-for-devs hooks/ (MIT).
# Copyright (c) 2026 Green-PT
"""Conservative compression for repetitive command output."""

from __future__ import annotations

import re


ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
VOLATILE = (
    (
        re.compile(
            r"\b\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}"
            r"(?:\.\d+)?(?:Z|[+-]\d{2}:?\d{2})?\b"
        ),
        "⟨ts⟩",
    ),
    (re.compile(r"\b\d{1,2}:\d{2}:\d{2}(?:\.\d+)?\b"), "⟨t⟩"),
    (re.compile(r"\b0x[0-9a-fA-F]+\b"), "⟨hex⟩"),
    (
        re.compile(
            r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-"
            r"[0-9a-f]{4}-[0-9a-f]{12}\b"
        ),
        "⟨uuid⟩",
    ),
)
MIN_RUN = 3
GATE_LINES = 25
EXPANSION = re.compile(r"^(.*)  ⟨×(\d+)⟩$", re.MULTILINE)


def template(line: str) -> str:
    """Mask fields whose changing values should not defeat adjacent deduplication."""
    result = line
    for pattern, token in VOLATILE:
        result = pattern.sub(token, result)
    return result


def compress(text: str) -> tuple[str, int, int]:
    """Return the compact view, characters saved, and lines dropped."""
    if not isinstance(text, str) or not text:
        return text or "", 0, 0
    stripped = ANSI.sub("", text)
    lines = stripped.split("\n")
    if len(lines) < GATE_LINES:
        return stripped, len(text) - len(stripped), 0

    output: list[str] = []
    dropped = 0
    index = 0
    while index < len(lines):
        run_template = template(lines[index])
        end = index + 1
        while end < len(lines) and template(lines[end]) == run_template:
            end += 1
        count = end - index
        if count >= MIN_RUN:
            output.append(f"{lines[index]}  ⟨×{count}⟩")
            dropped += count - 1
        else:
            output.extend(lines[index:end])
        index = end

    view = "\n".join(output)
    return view, len(text) - len(view), dropped


def expand(view: str) -> str:
    """Restore the count represented by each collapsed-run marker."""
    return EXPANSION.sub(
        lambda match: "\n".join([match.group(1)] * int(match.group(2))), view
    )
