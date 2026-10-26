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
            "projects": {str(root): {"hasTrustDialogAccepted": directory == home,
                                    "allowedTools": ["source-tool" if directory == home else "profile-tool"]},
                         str(cwd): {"hasTrustDialogAccepted": False},
                         "profile-only": {"hasTrustDialogAccepted": directory == profile}},
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
                      + f'b = {{ "{socket.gethostname()}" = {{ path = "{home}/.claude" }} }}, c = {{ "{socket.gethostname()}" = {{ path = "{profile}" }} }}'
                      + ' }\n')
    env = dict(os.environ, HOME=str(home), AGENTS_ORCHESTRATION=str(policy), PATH=f"{fake}:{os.environ['PATH']}")
    env.pop("CLAUDE_CONFIG_DIR", None)
    env.pop("AGENT_ORG_NAME", None)
    before = json.loads((profile / ".claude.json").read_text())
    source_before = (home / ".claude.json").read_bytes()
    subprocess.run([sys.executable, "-I", str(HERE / "claude_profiles.py")], env=env, check=True)
    after = (profile / ".claude.json").read_bytes()
    subprocess.run([sys.executable, "-I", str(HERE / "claude_profiles.py")], env=env, check=True)
    assert (profile / ".claude.json").read_bytes() == after
    before["projects"][str(root)]["hasTrustDialogAccepted"] = True
    assert json.loads(after) == before
    assert (home / ".claude.json").read_bytes() == source_before
    assert (profile / ".credentials.json").read_text() == credentials
    assert (profile / "settings.json.before-harness-sync").read_text() == "profile onboarding"
    for name in ("CLAUDE.md", "settings.json", "hooks", "skills", "agents", "commands"):
        assert (profile / name).is_symlink() and (profile / name).resolve() == (source / name).resolve(), name
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
    routes = routing.ask("route", ctx=ctx, provider="claude")
    assert "claude:c" not in routes.values(), (routes, ctx)
    (profile / ".credentials.json").write_text(json.dumps({"claudeAiOauth": {
        "subscriptionType": "max", "rateLimitTier": "default_claude_max_5x"}}))
    result = subprocess.run([sys.executable, "-I", str(HERE / "usage_claude.py"), "--no-refresh"],
                            env=env, capture_output=True, text=True, check=True)
    readings = [json.loads(line) for line in result.stdout.splitlines()]
    ctx = plan_usage.live_ctx(cfg, catalog, readings, None, datetime.now(timezone.utc), {}, {"claude-b": 50, "claude-c": 25})
    assert ctx["accounts"]["claude:c"]["auto_route"] and routing.ask("route", ctx=ctx, provider="claude")["under_pace"] == "claude:c", ctx
    account = {"windows": {"week": {"headroom": 93, "window_h": 168, "reset_in": 150, "beta": 0,
                                   "resets_at": "2099-01-01T00:00:00Z", "as_of": "2098-12-28T00:00:00Z"}},
               "age_min": 1, "target_left": 5, "usd_per_week": 25, "extra_usage": False, "spend": False, "hasCredits": False}
    ctx = {"now": datetime(2098, 12, 28, tzinfo=timezone.utc),
           "accounts": {"claude:b": account | {"windows": {"week": account["windows"]["week"] | {"headroom": 2}}},
                        "claude:c": account}, "billing": {"claude": {"standard": "plan", "overflow": "blocked"}}}
    assert routing.ask("route", ctx=ctx, provider="claude")["under_pace"] == "claude:c"
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
