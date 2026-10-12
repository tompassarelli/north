#!/usr/bin/env python3
"""Generate or verify twenty pinned, read-only Explore replay scenarios."""

import argparse
import json
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
QUERIES = {
    "north": [
        ("policy_for_arm", ["harness/scripts/policy-replay.py"]),
        ("run_one", ["harness/scripts/policy-replay.py"]),
        ("advisor_usage", ["harness/scripts/policy-replay.py"]),
        ("reap", ["harness/scripts/policy-replay.py"]),
        ("paired_diff", ["harness/scripts/policy_stats.py", "harness/scripts/advisor-report.py"]),
        ("wilson", ["harness/scripts/policy_stats.py", "harness/scripts/advisor-report.py"]),
        ("plan_upper", ["harness/scripts/advisor-report.py"]),
        ("parse_command", ["src/main.rs"]),
        ("navigate_view", ["src/main.rs"]),
        ("north_explore_missing_symbol_38", ["src/main.rs"]),
    ],
    "galileo": [
        ("Session", ["crates/galileo-runtime/src"]),
        ("Step", ["crates/galileo-runtime/src"]),
        ("Snapshot", ["crates/galileo-runtime/src"]),
        ("LARGE_TEXT", ["crates/galileo-runtime/src/save.rs"]),
        ("Writer", ["crates/galileo-runtime/src/save.rs"]),
        ("lex", ["crates/galileo-syntax/src"]),
        ("lower_program", ["crates/galileo-syntax/src"]),
        ("read_program", ["crates/galileo-syntax/src"]),
        ("QuestionKind", ["crates/galileo-syntax/src"]),
        ("galileo_explore_missing_symbol_38", ["crates/galileo-syntax/src"]),
    ],
}


def sha(root):
    return subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()


def scenario(project, root, symbol, paths):
    commit = sha(root)
    argv = ["rg", "--no-heading", "--color", "never", "-n", "-H", "-w", "-F", "--", symbol, *paths]
    found = subprocess.run(argv, cwd=root, capture_output=True, text=True)
    if found.returncode not in (0, 1):
        raise SystemExit(found.stderr)
    matches = sorted({":".join(line.split(":", 2)[:2]) for line in found.stdout.splitlines()})
    answer = "\n".join(matches) if matches else "NOT FOUND"
    return {
        "id": f"explore-{project}-{symbol}",
        "prompt": (
            f"Read-only Explore search, breadth medium. In {root} at git HEAD {commit}, "
            f"where is the exact word {symbol!r} defined or used within {', '.join(paths)}? "
            "Search every matching line, including comments and strings. Do not modify files or run state-changing commands. "
            "Reply only with the complete unique repo-relative file:line set, one per line, sorted lexicographically. "
            "If there are no matches reply exactly NOT FOUND. No prose or fences."
        ),
        "detector": {"kind": "regex", "pattern": r"\A\s*" + re.escape(answer) + r"\s*\Z"},
        "tools": ["Bash", "Read", "Glob", "Grep"],
        "model": "claude-opus-5-5",
        "effort": "low",
        "max_turns": 4,
        "timeout_s": 120,
        "max_budget_usd": 0.0375,
        "ground_truth": {"project": project, "repo": str(root), "commit": commit,
                         "argv": argv, "file_lines": matches, "rg_exit": found.returncode},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--north-root", type=Path, default=Path.home() / "code/north/main")
    parser.add_argument("--galileo-root", type=Path, default=Path.home() / "code/galileo/main")
    parser.add_argument("--out", type=Path, default=HERE / "explore-scenarios")
    parser.add_argument("--check", action="store_true", help="verify corpus and pins without rewriting")
    args = parser.parse_args()
    expected = {}
    for project, root in (("north", args.north_root), ("galileo", args.galileo_root)):
        root = root.resolve()
        for symbol, paths in QUERIES[project]:
            row = scenario(project, root, symbol, paths)
            expected[row["id"] + ".json"] = json.dumps(row, indent=2) + "\n"
    if args.check:
        actual = {p.name: p.read_text() for p in args.out.glob("*.json")}
        if actual != expected:
            raise SystemExit("Explore corpus differs from current rg ground truth or repository pins")
    else:
        args.out.mkdir(parents=True, exist_ok=True)
        extra = {p.name for p in args.out.glob("*.json")} - expected.keys()
        if extra:
            raise SystemExit(f"refuse to overwrite corpus with unexpected scenarios: {sorted(extra)}")
        for name, content in expected.items():
            (args.out / name).write_text(content)
    print(f"{'verified' if args.check else 'generated'} {len(expected)} scenarios: north=10 galileo=10; "
          "40 paired runs at $0.0375/run = $1.50 list ceiling")


if __name__ == "__main__":
    main()
