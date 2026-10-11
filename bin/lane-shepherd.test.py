#!/usr/bin/env python3
import csv
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


TARGET = Path(os.environ.get("LANE_SHEPHERD_TARGET", Path(__file__).with_name("lane-shepherd"))).resolve()
GC = Path(__file__).with_name("lane-gc").resolve()
REAL_GIT = shutil.which("git")


def boundary():
    with tempfile.TemporaryDirectory(prefix="lane-shepherd-test-") as temporary:
        root = Path(temporary)
        main = root / "code/demo/main"
        lanes = root / "code/demo/worktrees"
        main.mkdir(parents=True)
        lanes.mkdir()
        commands = root / "bin"
        commands.mkdir()
        (commands / "lane-gc").symlink_to(GC)
        claude = commands / "claude"
        claude.write_text("#!/usr/bin/env python3\nimport json, sys\ne = json.loads(sys.stdin.read().split('No tools.\\n', 1)[1])\n"
                          "v = 'fold' if e['branch'] == 'landed' else 'land' if e['branch'] == 'ready' else 'archive'\n"
                          "print(json.dumps({'result': json.dumps({'verdict': v, 'reason': 'cherry=' + str(e['cherry']) + '; rebase=' + e['rebase']})}))\n")
        claude.chmod(0o755)
        wrapper = commands / "git"
        wrapper.write_text("#!/usr/bin/env python3\nimport os, sys\n"
                           "if os.environ.get('BREAK_ARCHIVE') and 'update-ref' in sys.argv and any(a.startswith('refs/archive/') for a in sys.argv): sys.exit(0)\n"
                           f"os.execv({REAL_GIT!r}, [{REAL_GIT!r}, *sys.argv[1:]])\n")
        wrapper.chmod(0o755)
        env = dict(os.environ, PATH=str(commands) + ":" + os.environ["PATH"],
                   LANE_SHEPHERD_ROOT=str(root / "code"), LANE_SHEPHERD_STATE=str(root / "state"),
                   LANE_SHEPHERD_REPORT=str(root / "report"),
                   GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL="/dev/null",
                   GIT_AUTHOR_DATE="2001-01-01T00:00:00Z", GIT_COMMITTER_DATE="2001-01-01T00:00:00Z")

        def git(where, *args):
            return subprocess.check_output([REAL_GIT, "-C", str(where), *args], env=env, stderr=subprocess.DEVNULL).decode().strip()

        def shepherd(*args, success=True, extra=None):
            result = subprocess.run([sys.executable, str(TARGET), *args], cwd=root,
                                    env=dict(env, **(extra or {})), capture_output=True, text=True, timeout=90)
            assert (result.returncode == 0) == success, (args, result.stdout, result.stderr)
            return result

        git(main, "init", "-b", "main")
        git(main, "config", "user.name", "Fixture")
        git(main, "config", "user.email", "fixture@example.invalid")
        (main / "source").write_text("base\n")
        git(main, "add", "source")
        git(main, "commit", "-m", "base")
        base = git(main, "rev-parse", "HEAD")
        dirty = lanes / "dirty"
        git(main, "worktree", "add", "-b", "dirty", str(dirty))
        (dirty / "source").write_text("lane\n")
        git(dirty, "commit", "-am", "unique lane")
        tip = git(dirty, "rev-parse", "HEAD")
        (dirty / "source").write_text("staged\n")
        git(dirty, "add", "source")
        (dirty / "source").write_text("working\n")
        (dirty / "untracked").write_text("rescue me\n")
        git(main, "branch", "landed", base)
        git(main, "branch", "ready", tip)
        live = lanes / "live"
        locked = lanes / "locked"
        for name, path in (("live", live), ("locked", locked)):
            git(main, "worktree", "add", "-b", name, str(path), tip)
        git(main, "worktree", "lock", str(locked))
        git(main, "update-ref", "refs/remotes/origin/main", base)
        rows = [(str(dirty), "dirty"), ("(branch)", "landed"), ("(branch)", "ready"),
                (str(live), "live"), (str(locked), "locked"), (str(main), "main")]
        (root / "report").write_text("# lane-gc fixture\n" + "".join(
            f"100h\tdemo\t{path}\t{branch}\t+1\tdirty\tfixture\t-\n" for path, branch in rows))
        holder = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.read()"], cwd=live, stdin=subprocess.PIPE)
        try:
            before_refs = git(main, "show-ref")
            before_status = git(dirty, "status", "--porcelain")
            before_worktrees = git(main, "worktree", "list", "--porcelain")
            shepherd("propose", "--repo", "demo")
            proposal = next((root / "state").glob("proposals-*.tsv"))
            with proposal.open() as file:
                proposals = list(csv.DictReader(file, delimiter="\t"))
            assert {r["branch"] for r in proposals} == {"dirty", "landed", "ready"}, proposals
            assert git(main, "show-ref") == before_refs
            assert git(dirty, "status", "--porcelain") == before_status
            assert git(main, "worktree", "list", "--porcelain") == before_worktrees
            assert (dirty / "source").read_text() == "working\n"
            approved = next(r for r in proposals if r["branch"] == "dirty")
            landed = next(r for r in proposals if r["branch"] == "landed")
            ready = next(r for r in proposals if r["branch"] == "ready")
            evidence = json.loads(approved["evidence"])
            assert evidence["subjects"] == ["unique lane"] and evidence["cherry"][0].startswith("+")
            assert evidence["rebase"] == "clean" and evidence["dirty_paths"]
            output = shepherd("apply", str(proposal), "--only", ready["row"])
            assert "safe-push --to main" in output.stdout and git(main, "show-ref") == before_refs
            (dirty / "untracked").write_text("changed after approval\n")
            shepherd("apply", str(proposal), "--only", approved["row"], success=False)
            (dirty / "untracked").write_text("rescue me\n")
            git(main, "worktree", "lock", str(dirty))
            shepherd("apply", str(proposal), "--only", approved["row"], success=False)
            git(main, "worktree", "unlock", str(dirty))
            reader = subprocess.Popen([sys.executable, "-c", "import sys; sys.stdin.read()"], cwd=dirty, stdin=subprocess.PIPE)
            try:
                shepherd("apply", str(proposal), "--only", approved["row"], success=False)
            finally:
                reader.communicate(b"")
            git(dirty, "config", "core.excludesFile", str(root / "ignore"))
            (root / "ignore").write_text("ignored\n")
            (dirty / "ignored").write_text("valuable runtime\n")
            shepherd("apply", str(proposal), "--only", approved["row"], success=False)
            assert (dirty / "ignored").read_text() == "valuable runtime\n"
            (dirty / "ignored").unlink()
            shepherd("apply", str(proposal), "--only", approved["row"], success=False, extra={"BREAK_ARCHIVE": "1"})
            assert dirty.is_dir() and git(dirty, "rev-parse", "HEAD") == tip
            shepherd("apply", str(proposal), "--only", approved["row"])
            ref = "refs/archive/lanes/dirty"
            assert git(main, "show", ref + ":source") == "working"
            assert git(main, "show", ref + ":untracked") == "rescue me"
            assert git(main, "show", ref + "^:source") == "staged"
            git(main, "merge-base", "--is-ancestor", tip, ref)
            assert not dirty.exists()
            assert "refs/heads/dirty" not in git(main, "show-ref")
            assert git(main, "rev-parse", "landed") == base
            assert git(main, "rev-parse", "ready") == tip
            assert live.exists() and locked.exists() and git(main, "rev-parse", "main") == base
            shepherd("apply", str(proposal), "--only", landed["row"])
            assert git(main, "rev-parse", "refs/archive/lanes/landed") == base
            assert "refs/heads/landed" not in git(main, "show-ref")
            (main / "source").write_text("main moved\n")
            git(main, "commit", "-am", "main moved")
            git(main, "update-ref", "refs/remotes/origin/main", "HEAD")
            safe_lanes = {name: lanes / name for name in ("merged", "stale", "conflict", "queued-land", "excluded")}
            for name, path in safe_lanes.items():
                git(main, "worktree", "add", "-b", name, str(path), base if name == "conflict" else "main")
            (safe_lanes["stale"] / "source").write_text("stale commit\n")
            git(safe_lanes["stale"], "commit", "-am", "stale lane")
            stale_tip = git(safe_lanes["stale"], "rev-parse", "HEAD")
            (safe_lanes["stale"] / "source").write_text("stale working\n")
            (safe_lanes["stale"] / "untracked").write_text("keep me\n")
            (safe_lanes["conflict"] / "source").write_text("conflict\n")
            git(safe_lanes["conflict"], "commit", "-am", "conflicting lane")
            for name in ("merged", "stale", "queued-land", "excluded"):
                admin = Path(git(safe_lanes[name], "rev-parse", "--path-format=absolute", "--git-dir"))
                for directory, _, files in os.walk(safe_lanes[name]):
                    for item in [directory, *(os.path.join(directory, f) for f in files)]:
                        os.utime(item, (978307200, 978307200), follow_symlinks=False)
                for item in ("HEAD", "index", "logs/HEAD"):
                    os.utime(admin / item, (978393600, 978393600))
            (root / "exclude").write_text("excluded\n")
            shepherd("apply", "--safe", "--repo", "demo", "--exclude", str(root / "exclude"))
            moved = git(main, "rev-parse", "main")
            assert not safe_lanes["merged"].exists() and git(main, "rev-parse", "refs/archive/lanes/merged") == moved
            assert not safe_lanes["stale"].exists() and git(main, "show", "refs/archive/lanes/stale:source") == "stale working"
            assert git(main, "show", "refs/archive/lanes/stale:untracked") == "keep me"
            git(main, "merge-base", "--is-ancestor", stale_tip, "refs/archive/lanes/stale")
            assert not {"refs/heads/merged", "refs/heads/stale"} & set(line.split()[1] for line in git(main, "show-ref").splitlines())
            for path in (live, locked, safe_lanes["conflict"], safe_lanes["queued-land"], safe_lanes["excluded"]):
                assert path.is_dir() and "refs/archive/lanes/" + path.name not in git(main, "show-ref"), path
            with next((root / "state").glob("safe-*.tsv")).open() as file:
                verdicts = {r["branch"]: r["verdict"] for r in csv.DictReader(file, delimiter="\t")}
            assert verdicts["conflict"] == "conflict", verdicts
            print("lane-shepherd Git boundary: PASS (proposal unchanged; approved archive; live/locked protected; failed archive kept; --safe retired merged and dead lanes only)")
        finally:
            holder.communicate(b"")


if __name__ == "__main__":
    boundary()
