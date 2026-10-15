#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "${NORTH_AGENT_PYTHON:-python3}" - "$HERE/code-health-edit.sh" <<'PY'
import json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path

hook = sys.argv[1]
bash = shutil.which("bash")
with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    tools = root / "tools"
    stub = root / "stub"
    tools.mkdir()
    stub.mkdir()
    for name in ("git", "dirname", "mkdir", "python3", "cat", "sleep"):
        (tools / name).symlink_to(shutil.which(name))
    (stub / "code-health").write_text(
        "#!" + bash + "\n"
        "[ \"$1\" = check ] || exit 2\n"
        "case \"$(cat \"$2\")\" in\n"
        "  *slow*) exec sleep 5 ;;\n"
        "  *broken*) echo 'internal error'; exit 2 ;;\n"
        "  *clean*) exit 0 ;;\n"
        "esac\n"
        "echo \"file-size ${2#$PWD/}: 612 lines > 500; split the file\"\n"
        "exit 1\n")
    (stub / "code-health").chmod(0o755)

    repo = root / "repo"
    (repo / "src").mkdir(parents=True)
    subprocess.run(["git", "init", "-q", str(repo)], check=True)
    (repo / "structure.toml").write_text("[thresholds]\nfile_lines = 500\n")
    bare = root / "bare"
    bare.mkdir()
    subprocess.run(["git", "init", "-q", str(bare)], check=True)
    files = {
        "big": repo / "src/big.ts",
        "clean": repo / "src/clean.rs",
        "slow": repo / "src/slow.c",
        "broken": repo / "src/broken.tsx",
        "doc": repo / "notes.md",
        "bare": bare / "big.ts",
    }
    for key, path in files.items():
        path.write_text(key + "\n")

    active = root / "activation.active"
    active.write_text("hook code-health-edit\n")
    base_env = dict(os.environ, HOME=str(root), NORTH_AGENT_ACTIVE=str(active),
                    AGENT_NO_AUTHORING_HOOKS="", AGENT_HOOK_ERRORS=str(root / "errors.tsv"),
                    PATH=f"{stub}:{tools}")
    base_env.pop("NORTH_HOOK_ID", None)

    def run(stdin, env=None):
        started = time.monotonic()
        result = subprocess.run([bash, hook], input=stdin, text=True, capture_output=True,
                                env=dict(base_env, **(env or {})), timeout=30)
        return result, time.monotonic() - started

    def payload(tool, tool_input, cwd=repo):
        return json.dumps({"hook_event_name": "PostToolUse", "tool_name": tool, "cwd": str(cwd),
                           "tool_input": tool_input, "tool_response": {"success": True}})

    patch = ("*** Begin Patch\n*** Update File: src/big.ts\n@@\n-a\n+b\n"
             "*** Add File: src/clean.rs\n+clean\n*** End Patch")
    reported = [
        ("Edit", payload("Edit", {"file_path": str(files["big"]), "old_string": "a", "new_string": "b"})),
        ("Write", payload("Write", {"file_path": str(files["big"]), "content": "big"})),
        ("MultiEdit", payload("MultiEdit", {"file_path": str(files["big"]),
                                            "edits": [{"old_string": "a", "new_string": "b"}]})),
        ("relative Write", payload("Write", {"file_path": "src/big.ts", "content": "big"})),
        ("apply_patch command", payload("apply_patch", {"command": patch})),
        ("apply_patch input", payload("apply_patch", {"input": patch})),
    ]
    silent = [
        ("clean edit", payload("Edit", {"file_path": str(files["clean"]), "old_string": "a", "new_string": "b"}), None),
        ("no structure.toml", payload("Write", {"file_path": str(files["bare"]), "content": "x"}, bare), None),
        ("non-source extension", payload("Write", {"file_path": str(files["doc"]), "content": "x"}), None),
        ("missing file", payload("Write", {"file_path": str(repo / "src/gone.ts"), "content": "x"}), None),
        ("tool error", payload("Edit", {"file_path": str(files["broken"])}), None),
        ("other tool", payload("Read", {"file_path": str(files["big"])}), None),
        ("malformed stdin", '{"tool_name":"Edit","tool_input":', None),
        ("non-object stdin", '["Edit"]', None),
        ("oversize stdin", payload("Write", {"file_path": str(files["big"]), "content": "x" * 1100000}), None),
        ("missing code-health", reported[0][1], {"PATH": str(tools)}),
        ("override", reported[0][1], {"AGENT_NO_AUTHORING_HOOKS": "1"}),
        ("inactive generation", reported[0][1], {"NORTH_AGENT_ACTIVE": str(root / "none")}),
    ]

    failures = []
    for name, stdin in reported:
        result, _ = run(stdin)
        try:
            out = json.loads(result.stdout)["hookSpecificOutput"]
        except Exception:
            failures.append(f"{name}: no context: {result.stdout!r} {result.stderr!r}")
            continue
        text = out.get("additionalContext", "")
        if (result.returncode != 0 or out.get("hookEventName") != "PostToolUse"
                or "program-craftsmanship" not in text
                or "file-size src/big.ts: 612 lines > 500; split the file" not in text
                or "permissionDecision" in out):
            failures.append(f"{name}: wrong report: {result.returncode} {result.stdout!r}")
    for name, stdin, env in silent:
        result, _ = run(stdin, env)
        if result.returncode != 0 or result.stdout or result.stderr:
            failures.append(f"{name}: not silent: {result.returncode} {result.stdout!r} {result.stderr!r}")
    result, elapsed = run(payload("Edit", {"file_path": str(files["slow"])}))
    if result.returncode != 0 or result.stdout or elapsed > 3:
        failures.append(f"timeout: {result.returncode} {result.stdout!r} {elapsed:.1f}s")

    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print(f"code-health-edit: {len(reported) + len(silent) + 1} cases passed")
PY
