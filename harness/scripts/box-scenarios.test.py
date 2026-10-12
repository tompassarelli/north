#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
RESULT = "echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"done\",\"model\":\"m\"}'\n"
STUBS = {
    "fix": "printf 'def add(a, b):\\n    return a + b\\n' > calc.py\n",
    "noop": "",
}


def sh(*args, cwd):
    subprocess.run(args, cwd=cwd, check=True, capture_output=True, text=True)


def commit(repo, files, message):
    for rel, text in files.items():
        (repo / rel).write_text(text)
    sh("git", "add", "-A", cwd=repo)
    sh("git", "-c", "user.name=t", "-c", "user.email=t@localhost", "commit", "-q", "-m", message, cwd=repo)


with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    up = tmp / "up"
    up.mkdir()
    sh("git", "init", "-q", "-b", "main", cwd=up)
    commit(up, {"calc.py": "def add(a, b):\n    return a - b\n"}, "calc")
    commit(up, {"calc.py": "def add(a, b):\n    return a + b\n",
                "calc.test.py": "from calc import add\nassert add(2, 3) == 5\n"},
           "add sums its operands\n\nadd subtracted.\n\nTest-kind: calc.test.py 4 kills add-subtracts")
    commit(up, {"notes.md": "later\n"}, "notes without a test")
    landed = subprocess.run(["git", "rev-parse", "HEAD~1"], cwd=up, capture_output=True, text=True, check=True).stdout.strip()
    sh("git", "clone", "-q", str(up), str(tmp / "work"), cwd=tmp)

    made = subprocess.run([sys.executable, str(HERE / "box-scenarios.py"), tmp / "work", "--since", "1 year ago",
                           "--domain", "north", "--n", "5", "--seed", "1", "--out", tmp / "scen"],
                          capture_output=True, text=True)
    assert made.returncode == 0, made.stderr
    files = sorted((tmp / "scen").glob("*.json"))
    assert len(files) == 1, files
    box = json.loads(files[0].read_text())
    assert box["commit"] == landed and box["repo"] == str(up.resolve()), box
    assert box["detector"] == {"tests": ["calc.test.py"], "cmd": "python3 calc.test.py"}, box
    assert box["brief"] == "add sums its operands\n\nadd subtracted.", box["brief"]
    print("ok: box-scenarios writes one box scenario for the Test-kind commit, its test file and a trailer-free brief")

    (tmp / "AGENTS.md").write_text("# Policy\n")
    for name, body in STUBS.items():
        fake = tmp / f"bin-{name}"
        fake.mkdir()
        seen = tmp / f"seen-{name}"
        (fake / "claude").write_text(
            "#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9; exit 0; }\n"
            f"{{ ls; git log --all --format=%s; grep -rl 'add(2, 3)' . || true; }} > {seen}\n" + body + RESULT)
        (fake / "claude").chmod(0o755)
        out = tmp / f"{name}.jsonl"
        env = dict(os.environ, PATH=f"{fake}:{os.environ['PATH']}", HOME=str(tmp))
        replay = subprocess.run([sys.executable, str(HERE / "policy-replay.py"), files[0], "--arm", "full",
                                 "--policy", tmp / "AGENTS.md", "--out", out], env=env, capture_output=True, text=True)
        assert replay.returncode == 0, replay.stderr[-2000:]
        assert seen.read_text() == "calc.py\nbase\n", seen.read_text()
        row = json.loads(out.read_text())
        assert row["pass"] is (name == "fix"), row
    print("ok: the worker sees commit^ without the landed test or its history; the restored test scores the fix pass and a no-op fail")
