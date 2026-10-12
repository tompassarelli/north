"""Write $XDG_STATE_HOME/agents/costs/digitalocean.json: the current billing month's usage, balance and credit, read
from the DigitalOcean balance endpoint (read-only) with the token from south:secrets/digitalocean.yaml."""

import json
import os
import subprocess
import sys
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

API = "https://api.digitalocean.com/v2/customers/my/balance"
SECRET = Path(os.environ.get("SOUTH_SECRETS") or Path.home() / "code/south/main/secrets") / "digitalocean.yaml"
KEY = os.environ.get("SOPS_AGE_KEY_FILE") or "/var/lib/sops-nix/key.txt"
OUT = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/costs/digitalocean.json"


def token():
    sops = ["sops", "-d", "--extract", '["token"]', str(SECRET)]
    cmd = sops if os.access(KEY, os.R_OK) else ["sudo", "-n", "env", f"SOPS_AGE_KEY_FILE={KEY}", *sops]
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=20,
                            env=os.environ | {"SOPS_AGE_KEY_FILE": KEY})
    if result.returncode != 0 or not result.stdout.strip():
        raise RuntimeError(f"could not decrypt {SECRET.name}")
    return result.stdout.strip()


def snapshot(balance, now):
    start = now.replace(day=1, hour=0, minute=0, second=0, microsecond=0)
    end = start.replace(year=start.year + 1, month=1) if start.month == 12 else start.replace(month=start.month + 1)
    account = float(balance["account_balance"])
    return {"service": "digitalocean", "period_start": start.isoformat(), "period_end": end.isoformat(),
            "usd_spent_period": float(balance["month_to_date_usage"]),
            "usd_balance": float(balance["month_to_date_balance"]),
            "credits_left": max(0.0, -account), "read_at": now.isoformat(), "source": API}


def main():
    try:
        request = urllib.request.Request(API, headers={"Authorization": f"Bearer {token()}"})
        with urllib.request.urlopen(request, timeout=20) as response:
            balance = json.load(response)
    except Exception as error:
        print(f"billing_digitalocean: {type(error).__name__}: {error}", file=sys.stderr)
        return 1
    OUT.parent.mkdir(parents=True, exist_ok=True)
    tmp = OUT.with_suffix(".tmp")
    tmp.write_text(json.dumps(snapshot(balance, datetime.now(timezone.utc))) + "\n")
    tmp.replace(OUT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
