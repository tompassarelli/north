#!/usr/bin/env python3
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LINK = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")
CODE = re.compile(r"`([^`\s]+)`")
ROT = [
    ("version", re.compile(r"\bv?\d+(?:\.\d+)+\b")),
    ("date", re.compile(r"\b\d{4}-\d{2}(?:-\d{2})?\b")),
    ("percentage", re.compile(r"\d+(?:\.\d+)?\s*%")),
    ("count", re.compile(r"\b\d+\b")),
    ("model id", re.compile(r"claude-|gpt-|\b(?:opus|sonnet|haiku)\b", re.I)),
]


def problems(base: Path, text: str, rot: bool = True) -> list[str]:
    out = []
    targets = [t for t in LINK.findall(text) if not re.match(r"[a-z][a-z0-9+.-]*:|#", t)]
    if rot:
        targets += [t for t in CODE.findall(text) if "/" in t or "." in t]
    for target in targets:
        path = target.split("#", 1)[0]
        if path and not (base / path).exists():
            out.append(f"missing path: {target}")
    for kind, pattern in ROT if rot else []:
        for m in pattern.finditer(text):
            out.append(f"{kind}: {m.group(0)!r}")
    return out


bad = "[x](docs/gone.md) `bin/gone` v1.2 2026-10-12 80% 3 claude-x Opus"
injected = problems(ROOT, bad)
for want in ("docs/gone.md", "bin/gone", "version", "date", "percentage", "count", "model id"):
    assert any(want in p for p in injected), (want, injected)

assert problems(ROOT / "docs", "[x](../bin/gone) 2026-10-12", rot=False) == ["missing path: ../bin/gone"]

found = [f"README.md: {p}" for p in problems(ROOT, (ROOT / "README.md").read_text())]
found += [f"docs/references.md: {p}" for p in problems(ROOT / "docs", (ROOT / "docs/references.md").read_text(), rot=False)]
for p in found:
    print(p)
if found:
    sys.exit(1)
print("README.md and docs/references.md: links and paths resolve; README.md has no rot-prone tokens")
