import json
import os
from pathlib import Path
import subprocess
import sys
from datetime import datetime, timedelta, timezone

VASTAI = Path(os.environ.get("VAST_JOB_VASTAI", Path.home() / ".local/bin/vastai"))
SECRET = Path("/run/secrets/vastai-api-key")
OUT = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/costs/vast.json"


def iso(t):
    return t.isoformat().replace("+00:00", "Z")


def vast(env, *cmd):
    out = subprocess.run(["timeout", "50", str(VASTAI), *cmd, "--raw"], env=env, capture_output=True, text=True,
                         check=True).stdout
    return json.loads(out)


def snapshot(user, charges, start, end, now):
    credit = user.get("credit")
    return {"service": "vast", "period_start": iso(start), "period_end": iso(end),
            "usd_spent_period": round(sum(c.get("amount") or 0 for c in charges), 4),
            "usd_balance": None if credit is None else round(credit, 4),
            "credits_left": None if credit is None else round(credit, 4),
            "read_at": iso(now),
            "source": "vastai show user (credit) + vastai show invoices-v1 --charges (month to date)"}


def main():
    if not SECRET.exists() or not VASTAI.exists():
        return 0
    env = {k: v for k, v in os.environ.items() if k != "VAST_API_KEY"} | {"VAST_API_KEY": SECRET.read_text().strip()}
    now = datetime.now(timezone.utc)
    start = now.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
    end = (start + timedelta(days=32)).replace(day=1)
    try:
        user = vast(env, "show", "user")
        charges, token = [], None
        while True:
            page = vast(env, "show", "invoices-v1", "--charges", "-s", start.date().isoformat(),
                        "-e", (end - timedelta(days=1)).date().isoformat(), "-l", "100",
                        *(["-t", token] if token else []))
            charges += page.get("results", [])
            token = page.get("next_token")
            if not token:
                break
    except (subprocess.CalledProcessError, ValueError) as error:
        print(f"billing_vast: {type(error).__name__}", file=sys.stderr)
        return 1
    OUT.parent.mkdir(parents=True, exist_ok=True)
    tmp = OUT.with_suffix(".tmp")
    tmp.write_text(json.dumps(snapshot(user, charges, start, end, now)) + "\n")
    tmp.replace(OUT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
