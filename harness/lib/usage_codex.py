import hashlib
import json
import math
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
from datetime import datetime, timezone


SOURCE = "account/rateLimits/read"


def utc(value):
    if value is None:
        return None
    if isinstance(value, str):
        value = datetime.fromisoformat(value.replace("Z", "+00:00"))
    else:
        value = datetime.fromtimestamp(value, timezone.utc)
    return value.astimezone(timezone.utc).isoformat(timespec="seconds").replace("+00:00", "Z")


def number(value):
    if value is None:
        return None
    value = float(value)
    if not math.isfinite(value):
        raise ValueError("non-finite usage value")
    return int(value) if value.is_integer() else value


def usage_rows(usage, account, ts):
    rows = []

    def add(kind, name, used_pct=None, window_min=None, resets_at=None, amount=None):
        rows.append({"provider": "codex", "account": account, "kind": kind,
                     "name": name, "used_pct": used_pct, "window_min": window_min,
                     "resets_at": utc(resets_at), "amount": number(amount),
                     "source": SOURCE, "ts": ts})

    buckets = dict(usage.get("rateLimitsByLimitId") or {})
    default = usage.get("rateLimits")
    if default:
        buckets.setdefault(default.get("limitId") or "codex", default)
    for limit_id, bucket in buckets.items():
        prefix = "" if limit_id == "codex" else limit_id + ":"
        for window in bucket.values():
            if not isinstance(window, dict) or "windowDurationMins" not in window:
                continue
            minutes = window["windowDurationMins"]
            if minutes is not None:
                minutes = int(minutes)
            name = {300: "5h", 10080: "week"}.get(minutes, f"{minutes}m" if minutes is not None else "unknown")
            used = number(window.get("usedPercent"))
            if used is not None:
                used = max(0, min(100, used))
            add("window", prefix + name, used, minutes, window.get("resetsAt"))
        credits = bucket.get("credits")
        if credits is not None:
            add("billing", prefix + "credits", amount=credits.get("balance"))
            if "hasCredits" in credits:
                add("billing", prefix + "hasCredits", amount=credits["hasCredits"])
            for name in ("spendControlReached", "ordinaryUsageAllowed"):
                if name in credits:
                    add("billing", prefix + name, amount=credits[name])
        for name in ("spendControlReached", "ordinaryUsageAllowed"):
            if name in bucket:
                add("billing", prefix + name, amount=bucket[name])
    for name in ("spendControlReached", "ordinaryUsageAllowed"):
        if name in usage:
            add("billing", name, amount=usage[name])
    credits = usage.get("credits")
    if credits is not None:
        add("billing", "credits", amount=credits.get("balance"))
        if "hasCredits" in credits:
            add("billing", "hasCredits", amount=credits["hasCredits"])
        for name in ("spendControlReached", "ordinaryUsageAllowed"):
            if name in credits:
                add("billing", name, amount=credits[name])
    resets = usage.get("rateLimitResetCredits")
    if resets is not None:
        add("billing", "rateLimitResetCredits", amount=resets.get("availableCount"))
        for credit in resets.get("credits") or []:
            status = credit.get("status")
            amount = None if status is None else int(status == "available")
            add("grant", credit.get("resetType") or "rateLimitResetCredit",
                resets_at=credit.get("expiresAt"), amount=amount)
    return rows


def read_usage(call, notify):
    call(1, "initialize", {"clientInfo": {"name": "usage-codex", "version": "1"},
                            "capabilities": {"experimentalApi": True}})
    notify({"method": "initialized", "params": {}})
    identity = call(2, "account/read", {"refreshToken": False})
    if identity.get("account") is None:
        raise ValueError("Codex app-server has no signed-in account")
    usage = call(3, SOURCE, {})
    ts = utc(datetime.now(timezone.utc).timestamp())
    account_id = ((identity.get("workspaceRouting") or {}).get("chatgptAccountId")
                  or identity["account"].get("accountId") or usage.get("accountId"))
    if not account_id:
        raise ValueError("Codex app-server did not report an account id")
    return usage_rows(usage, hashlib.sha256(account_id.encode()).hexdigest()[:8], ts)


def idle_usage(pooled):
    runtime = os.environ.get("CODEX_RUNTIME", str(Path.home() / ".local/lib/codex/current/bin/codex"))
    with tempfile.TemporaryDirectory(prefix="usage-codex-") as directory:
        home = Path(directory)
        (home / "auth.json").symlink_to(pooled / "auth.json")
        env = dict(os.environ, CODEX_HOME=str(home), CODEX_SQLITE_HOME=str(home / "sqlite"),
                   DBUS_SESSION_BUS_ADDRESS=f"unix:abstract=usage-codex-{os.getpid()}")
        proc = subprocess.Popen([runtime, "app-server", "--listen", "stdio://"],
                                stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, text=True, env=env)
        try:
            def notify(message):
                proc.stdin.write(json.dumps(message) + "\n")
                proc.stdin.flush()

            def call(request_id, method, params):
                notify({"id": request_id, "method": method, "params": params})
                for line in proc.stdout:
                    message = json.loads(line)
                    if message.get("id") != request_id:
                        continue
                    if "error" in message:
                        raise RuntimeError(f"{method} failed (RPC code {message['error'].get('code')})")
                    return message["result"]
                raise ConnectionError("idle Codex app-server closed the connection")

            return read_usage(call, notify)
        finally:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait()
            proc.stdin.close()
            proc.stdout.close()


def main():
    pooled = Path(os.environ.get("NORTH_CODEX_POOLED_HOME", Path.home() / ".local/state/north/codex-pooled"))
    for row in idle_usage(pooled):
        print(json.dumps(row, separators=(",", ":")))


def timed_out(*_):
    raise TimeoutError("Codex app-server read exceeded 25 seconds")


if __name__ == "__main__":
    signal.signal(signal.SIGALRM, timed_out)
    signal.alarm(25)
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print(f"usage_codex: {error}", file=sys.stderr)
        sys.exit(1)
    finally:
        signal.alarm(0)
