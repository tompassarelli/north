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
        "attachment_identities",
        "menu_direction",
        "ConversationSnapshot",
        "find_container",
        "parse_message",
        "markdown_table_columns",
        "paired_diff",
        "final_text",
        "ok_then",
        "north_explore_missing_symbol_38",
    ],
    "galileo": [
        "parse_type",
        "lower_program_contract",
        "program_identity",
        "minimal_supports",
        "render_all",
        "run_with_budget",
        "print_idiomatic",
        "normalized_claim",
        "parse_record_file",
        "galileo_explore_missing_symbol_38",
    ],
}


def sha(root):
    return subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()


def scenario(project, root, symbol):
    commit = sha(root)
    assert commit == {
        "north": "20c3990798e61248c94bb10e61e87b6f677c4efb",
        "galileo": "411004388c9f53ff14c07c6f12bf66f9997859f4",
    }[project], f"{project}: repository HEAD differs from the immutable pin"
    argv = ["rg", "--no-heading", "--color", "never", "-n", "-H", "-w", "-F", "--", symbol, "."]
    found = subprocess.run(argv, cwd=root, capture_output=True, text=True)
    if found.returncode not in (0, 1):
        raise SystemExit(found.stderr)
    matches = sorted({":".join(line.removeprefix("./").split(":", 2)[:2]) for line in found.stdout.splitlines()})
    files = {line.rsplit(":", 1)[0] for line in matches}
    if symbol == f"{project}_explore_missing_symbol_38":
        assert found.returncode == 1 and not matches, f"{project}: absent symbol {symbol} was found"
    else:
        assert found.returncode == 0 and 3 <= len(matches) <= 15, f"{project}: {symbol} needs 3-15 matching lines"
        assert len(files) >= 2, f"{project}: {symbol} needs matches in at least two files"
    answer = "\n".join(matches) if matches else "NOT FOUND"
    return {
        "id": f"explore-{project}-{symbol}",
        "prompt": (
            f"Read-only Explore search, breadth medium. In {root} at git HEAD {commit}, "
            f"where is the exact word {symbol!r} defined or used anywhere in the repository? "
            "Search every matching line, including comments and strings. Do not modify files or run state-changing commands. "
            "Reply only with the complete unique repo-relative file:line set, one per line, sorted lexicographically. "
            "If there are no matches reply exactly NOT FOUND. No prose or fences."
        ),
        "detector": {"kind": "regex", "pattern": r"\A\s*" + re.escape(answer) + r"\s*\Z"},
        "tools": ["Bash", "Read", "Glob", "Grep"],
        "model": "claude-opus-5-5",
        "effort": "low",
        "max_turns": 12,
        "timeout_s": 300,
        "max_budget_usd": 0.06,
        "ground_truth": {"project": project, "repo": str(root), "commit": commit,
                         "argv": argv, "file_lines": matches, "rg_exit": found.returncode},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--north-root", type=Path, default=Path("/home/tom/code/north/pins/20c3990798e61248c94bb10e61e87b6f677c4efb"))
    parser.add_argument("--galileo-root", type=Path, default=Path("/home/tom/code/galileo/pins/411004388c9f53ff14c07c6f12bf66f9997859f4"))
    parser.add_argument("--out", type=Path, default=HERE / "explore-scenarios")
    parser.add_argument("--check", action="store_true", help="verify corpus and pins without rewriting")
    args = parser.parse_args()
    expected = {}
    table = []
    for project, root in (("north", args.north_root), ("galileo", args.galileo_root)):
        root = root.resolve()
        assert root.is_dir(), f"repository pin missing: {root}"
        assert len(QUERIES[project]) == len(set(QUERIES[project])) == 10
        assert QUERIES[project][-1] == f"{project}_explore_missing_symbol_38"
        project_files = set()
        for symbol in QUERIES[project]:
            row = scenario(project, root, symbol)
            assert (row["max_turns"], row["timeout_s"], row["max_budget_usd"]) == (12, 300, 0.06)
            assert str(root) + "/" not in row["prompt"]
            expected[row["id"] + ".json"] = json.dumps(row, indent=2) + "\n"
            matches = row["ground_truth"]["file_lines"]
            files = {line.rsplit(":", 1)[0] for line in matches}
            project_files.update(files)
            table.append((project, symbol, len(matches), len(files)))
        if project == "north":
            assert all(any(path.startswith(prefix) for path in project_files)
                       for prefix in ("src/", "bin/", "harness/scripts/"))
        else:
            assert len({path.split("/")[1] for path in project_files if path.startswith("crates/")}) >= 3
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
    for project, symbol, lines, files in table:
        print(f"{project:7} {symbol:40} {lines:2} lines {files:2} files")
    print(f"{'verified' if args.check else 'generated'} {len(expected)} scenarios: north=10 galileo=10; "
          "40 paired runs at $0.06/run = $2.40 list ceiling")


if __name__ == "__main__":
    main()
