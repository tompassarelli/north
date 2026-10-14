#!/usr/bin/env python3
import json
import importlib.util
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
        (repo / rel).parent.mkdir(parents=True, exist_ok=True)
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

    commit(up, {"crates/boundary/Cargo.toml": '[package]\nname = "boundary-calc"\nversion = "0.1.0"\nautotests = false\n\n[[test]]\nname = "agreement"\npath = "tests/boundary.rs"\n',
                "crates/boundary/src/lib.rs": "pub fn add(a: i32, b: i32) -> i32 { a - b }\n"}, "Cargo boundary parent")
    rust_test = "crates/boundary/tests/boundary.rs"
    companion = "crates/boundary/tests/expected.txt"
    implementation = "crates/boundary/src/lib.rs"
    commit(up, {implementation: "pub fn add(a: i32, b: i32) -> i32 { a + b }\n",
                "crates/boundary/src/references.rs": "#[test]\nfn internal_reference() {}\n",
                rust_test: '#[test]\nfn agreement() { assert_eq!(boundary_calc::add(2, 3).to_string(), include_str!("expected.txt").trim()); }\n',
                companion: "5\n"}, "Cargo agreement\n\nTest-kind: crates/boundary/tests/boundary.rs 5 kills rust-integration-unsupported")
    rust_commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=up, capture_output=True, text=True, check=True).stdout.strip()
    made = subprocess.run([sys.executable, str(HERE / "box-scenarios.py"), up, "--since", "1 year ago",
                           "--domain", "north", "--ref", "main", "--n", "5", "--seed", "51", "--out", tmp / "rust-scen"],
                          capture_output=True, text=True)
    assert made.returncode == 0, made.stderr
    boxes = [json.loads(path.read_text()) for path in (tmp / "rust-scen").glob("*.json")]
    rust_box = next(box for box in boxes if box["commit"] == rust_commit)
    assert rust_box["detector"] == {"tests": [rust_test, companion],
                                    "cmd": "nix develop -c cargo test --locked --offline -p boundary-calc --test agreement"}, rust_box
    spec = importlib.util.spec_from_file_location("policy_replay", HERE / "policy-replay.py")
    replay_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(replay_module)
    rust_work = tmp / "rust-work"
    rust_cwd = rust_work / "checkout"
    rust_cwd.mkdir(parents=True)
    hidden = replay_module.box_checkout(rust_box, rust_work, rust_cwd)
    assert set(hidden) == {rust_test, companion}, hidden
    assert all(not (rust_cwd / path).exists() for path in hidden), hidden
    assert (rust_cwd / implementation).read_text() == "pub fn add(a: i32, b: i32) -> i32 { a - b }\n"
    assert replay_module.git("log", "--all", "--format=%s", cwd=rust_cwd) == b"base\n"
    print("ok: North Cargo integration command follows its manifest; replay hides test companions and preserves parent implementation")
