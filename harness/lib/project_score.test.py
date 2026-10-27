import importlib.util
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
from datetime import datetime, timedelta, timezone
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import usage

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("project_score", HERE / "project_score.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def scenario():
    with tempfile.TemporaryDirectory(dir=HERE) as scratch:
        root = Path(scratch)
        operator = root / "operator"
        operator.mkdir()
        (operator / "orchestration.toml").write_text('[projects.north]\ntarget = "2020-01-08T00:00:00Z"\nshare = 50\ngoal = ["north#123"]\n')
        (operator / "costs.toml").write_text('[[service]]\nid = "build"\nkind = "cloud"\nusd_per_week = 12.5\n[[service]]\nid = "gemini"\nkind = "ai-plan"\nusd_per_week = 50\n')
        db = sqlite3.connect(root / "snapshot.db")
        db.execute("CREATE TABLE runs(item TEXT, ended TEXT, cost_usd REAL)")
        db.execute("INSERT INTO runs VALUES('north#123', '2020-01-07T00:00:00Z', 2.5)")
        db.commit()
        read_at = datetime.now(timezone.utc)
        usage.record(db, [{"ts": read_at.isoformat(), "provider": "gemini", "account": "gemini", "kind": "window",
                           "name": "week", "used_pct": 20, "window_min": 10080,
                           "resets_at": (read_at + timedelta(days=3)).isoformat()}])
        db.close()
        tools = root / "bin"
        tools.mkdir()
        gh = tools / "gh"
        gh.write_text('#!/bin/sh\nif [ "$2" = list ]; then printf \'%s\\n\' \'[{"number":9,"state":"OPEN","closedAt":null,"body":"- [ ] unrelated backlog"}]\'; exit; fi\nprintf \'%s\\n\' \'{"state":"CLOSED","closedAt":"2020-01-07T00:00:00Z","body":"- [x] verified"}\'\n')
        gh.chmod(0o755)
        env = dict(os.environ, AGENTS_OPERATOR_DIR=str(operator), THREADS_DB=str(root / "snapshot.db"),
                   XDG_STATE_HOME=str(root / "state"), PATH=f"{tools}:{os.environ['PATH']}", PYTHONDONTWRITEBYTECODE="1")
        result = subprocess.run([str(HERE.parents[1] / "bin/agents"), "plan", "--score", "north"],
                                env=env, capture_output=True, text=True, check=True)
        assert result.stdout.splitlines() == ['north on-date: pass', 'north in-budget: pass', 'north spec-pass: pass'], result.stdout
        lines = (root / "state/agents/goal-scores.jsonl").read_text().splitlines()
        assert len(lines) == 1
        assert json.loads(lines[0])["spent_usd"] == 2.5
        assert json.loads(lines[0])["budget_usd"] == 20
        rows = [{"cost_usd": 2.5}]
        issues = [{"state": "CLOSED", "closedAt": "2020-01-07T00:00:00Z", "body": "- [x] verified"}]
        now = datetime(2020, 1, 9, tzinfo=timezone.utc)
        passed = module.score("north", "2020-01-08T00:00:00Z", now, issues, rows, 3)
        assert [passed[key] for key in ("on-date", "in-budget", "spec-pass")] == ["pass"] * 3
        issues[0].update(closedAt="2020-01-09T00:00:00Z", body="- [ ] pending")
        failed = module.score("north", "2020-01-08T00:00:00Z", now, issues, rows, 2)
        assert [failed[key] for key in ("on-date", "in-budget", "spec-pass")] == ["fail"] * 3
        os.environ["AGENTS_OPERATOR_DIR"] = str(operator)
        import sys
        sys.path.insert(0, str(HERE))
        import routing
        ctx = json.loads((HERE / "routing-scenarios/deadline.json").read_text())
        ctx["boxes"].append(dict(ctx["boxes"][-1], id="m5"))
        result = routing.ask("plan", ctx=ctx)
        assert len(result["needs"]) == 1, result["needs"]
        decision = result["needs"][0]
        assert "more budget" in decision and "later date / smaller scope" in decision
        assert "rent build/test compute: build current ledger cost $12.5/week, speedup unmeasured" in decision
        assert "reconfigure/swap service: service current ledger cost $7/week, speedup unmeasured" in decision
        print("PASS score boundary: 3 verdicts, 1 row; failed evidence fails; 1 miss decision with recorded non-AI dollars")


if __name__ == "__main__":
    scenario()
