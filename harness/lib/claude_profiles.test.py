import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone

HERE = Path(__file__).resolve().parent

with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    home, profile, fake = root / "home", root / "profile", root / "bin"
    source = home / ".claude"
    source.mkdir(parents=True)
    profile.mkdir()
    fake.mkdir()
    for name in ("CLAUDE.md", "settings.json", "hooks", "skills", "agents", "commands"):
        if name in ("CLAUDE.md", "settings.json"):
            (source / name).write_text("shared harness")
        else:
            (source / name).mkdir()
            (source / name / "fixture").write_text("shared harness")
    cwd = root / "project"
    cwd.mkdir()
    reset = (datetime.now(timezone.utc) + timedelta(days=4)).isoformat()
    for directory, uuid in ((home, "fixture-default"), (profile, "fixture-c")):
        (directory / ".claude.json").write_text(json.dumps({"oauthAccount": {"accountUuid": uuid},
            "projects": {str(root): {"hasTrustDialogAccepted": True}} if directory == home else {},
            "cachedUsageUtilization": {"accountUuid": uuid, "fetchedAtMs": time.time() * 1000,
                "utilization": {"limits": [{"kind": "weekly_all", "group": "weekly", "percent": 98 if directory == home else 7,
                                             "resets_at": reset}]}}}))
    credentials = json.dumps({"claudeAiOauth": {"subscriptionType": "free", "rateLimitTier": "free"}})
    (profile / ".credentials.json").write_text(credentials)
    (profile / "settings.json").write_text("profile onboarding")
    hashes = {hashlib.sha256(uuid.encode()).hexdigest()[:8]: label
              for uuid, label in (("fixture-default", "b"), ("fixture-c", "c"))}
    policy = root / "orchestration.toml"
    policy.write_text('[accounts.claude]\nlabels = { ' + ', '.join(f'"{key}" = "{label}"' for key, label in hashes.items())
                      + ' }\nplans = { c = "max-5x" }\nauto_route_requires = { c = "max-5x" }\nprofiles = { '
                      + f'b = {{ "{socket.gethostname()}" = {{}} }}, c = {{ "{socket.gethostname()}" = {{ path = "{profile}" }} }}'
                      + ' }\n')
    capture = root / "launch.json"
    stub = fake / "spawn-quiet"
    stub.write_text('#!/usr/bin/env python3\nimport json,os,sys\n'
                    'open(os.environ["CAPTURE"],"w").write(json.dumps({"argv":sys.argv[1:],"profile":os.environ.get("CLAUDE_CONFIG_DIR")}))\n')
    stub.chmod(0o755)
    brief = root / "brief.md"
    brief.write_text("Read this fixture brief.")
    env = dict(os.environ, HOME=str(home), AGENTS_ORCHESTRATION=str(policy), AGENTS_ORG_FILE=str(root / "org.json"),
               AGENTS_SESSION_HOST="ghostty", CAPTURE=str(capture), PATH=f"{fake}:{os.environ['PATH']}")
    env.pop("CLAUDE_CONFIG_DIR", None)
    before = (profile / ".claude.json").read_bytes()
    subprocess.run([sys.executable, "-I", str(HERE / "claude_profiles.py")], env=env, check=True)
    subprocess.run([sys.executable, "-I", str(HERE / "claude_profiles.py")], env=env, check=True)
    assert (profile / ".claude.json").read_bytes() == before
    assert (profile / ".credentials.json").read_text() == credentials
    assert (profile / "settings.json.before-harness-sync").read_text() == "profile onboarding"
    for name in ("CLAUDE.md", "settings.json", "hooks", "skills", "agents", "commands"):
        assert (profile / name).is_symlink() and (profile / name).resolve() == (source / name).resolve(), name
    for label in ("c", "b", None):
        name = f"fixture-{label}"
        args = [sys.executable, "-I", str(HERE / "org.py"), "lead", "start", "--provider", "claude", "--domain", "north",
                "--brief", str(brief), "--cwd", str(cwd), "--name", name]
        if label:
            args += ["--account", label]
        selected_env = env | {"CLAUDE_CONFIG_DIR": str(profile)} if label == "b" else env
        result = subprocess.run(args, env=selected_env, capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        launched = json.loads(capture.read_text())
        assert launched["profile"] == (str(profile) if label == "c" else None), launched
        node = next(n for n in json.loads((root / "org.json").read_text())["nodes"] if n["id"] == name)
        assert node.get("account") == label and node["session"], node
        assert launched["argv"][0] == "ghostty" and node["session"] in launched["argv"][-1], launched
        if label == "c":
            assert json.loads((profile / ".claude.json").read_text())["projects"][str(cwd)]["hasTrustDialogAccepted"]
            assert str(cwd) not in json.loads((home / ".claude.json").read_text())["projects"]
    result = subprocess.run([sys.executable, "-I", str(HERE / "usage_claude.py"), "--no-refresh"],
                            env=env, capture_output=True, text=True, check=True)
    readings = [json.loads(line) for line in result.stdout.splitlines()]
    assert {r["account"] for r in readings} == {"b", "c"}, readings
    sys.path.insert(0, str(HERE))
    import routing, plan_usage, tomllib
    catalog = tomllib.loads((HERE.parent / "model-catalog.toml").read_text())
    cfg = tomllib.loads(policy.read_text())
    ctx = plan_usage.live_ctx(cfg, catalog, readings, None, datetime.now(timezone.utc), {}, {"claude-b": 50, "claude-c": 25})
    assert not ctx["accounts"]["claude:c"]["auto_route"], ctx
    for route in (routing.route_account, routing.route, routing.provider_account):
        assert route(ctx, "claude") != "claude:c", (route.__name__, ctx)
    (profile / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {
        "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x"}}))
    result = subprocess.run([sys.executable, "-I", str(HERE / "usage_claude.py"), "--no-refresh"],
                            env=env, capture_output=True, text=True, check=True)
    readings = [json.loads(line) for line in result.stdout.splitlines()]
    ctx = plan_usage.live_ctx(cfg, catalog, readings, None, datetime.now(timezone.utc), {}, {"claude-b": 50, "claude-c": 25})
    assert ctx["accounts"]["claude:c"]["auto_route"] and routing.route_account(ctx, "claude") == "claude:c", ctx
    result = subprocess.run([sys.executable, "-I", str(HERE / "org.py"), "lead", "start", "--provider", "claude",
                             "--account", "missing", "--domain", "north", "--brief", str(brief)], env=env, capture_output=True, text=True)
    assert result.returncode != 0 and "has no profile" in result.stderr, result
    account = {"windows": {"week": {"headroom": 93, "window_h": 168, "reset_in": 150, "beta": 0,
                                   "resets_at": "2099-01-01T00:00:00Z", "as_of": "2098-12-28T00:00:00Z"}},
               "age_min": 1, "target_left": 5, "usd_per_week": 25, "extra_usage": False, "spend": False, "hasCredits": False}
    ctx = {"now": datetime(2098, 12, 28, tzinfo=timezone.utc),
           "accounts": {"claude:b": account | {"windows": {"week": account["windows"]["week"] | {"headroom": 2}}},
                        "claude:c": account}, "billing": {"claude": {"standard": "plan", "overflow": "blocked"}}}
    assert routing.route_account(ctx, "claude") == "claude:c"
    import sqlite3
    from usage import record
    db_path = root / "usage.db"
    with sqlite3.connect(db_path) as db:
        record(db, readings)
    result = subprocess.run([sys.executable, "-I", str(HERE / "usage.py")], env=env | {"THREADS_DB": str(db_path)},
                            capture_output=True, text=True, check=True)
    c_line = next(line for line in result.stdout.splitlines() if line.startswith("claude c "))
    assert "weekly_all 7%" in c_line and "plan Claude Max 5x $100/month" in c_line, c_line
    print("PASS Claude profile projection, isolated launch selection, local usage and routing through public boundaries")
