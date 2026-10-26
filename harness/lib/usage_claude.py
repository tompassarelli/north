"""Claude Code usage rows: `claude -p /usage` (0 turns, $0) refreshes ~/.claude.json's cachedUsageUtilization,
which this prints as one JSON row per window, billing flag and unnamed bucket (contract: firn#12 plan §6)."""

import hashlib
import json
import os
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from usage import account_label

KNOWN = {"five_hour", "seven_day", "limits", "extra_usage", "spend", "cedar_ember"}
WINDOW_MIN = {"session": 300, "weekly": 10080}
# Claude Code sessions refresh this cache themselves; a /usage call is only needed when it is older.
FRESH_S = 60


def utc(ts):
    return datetime.fromisoformat(ts).astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ") if ts else None


def web_rows(account, now, labelled=False):
    path = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/usage/claude-web.json"
    try:
        entries = json.loads(path.read_text()).get("accounts", {})
    except (OSError, ValueError):
        return []
    entry = (next((e for e in entries.values() if e.get("account_label") == account), None)
             if labelled else entries.get(account))
    if not entry or (entry.get("valid_until") and utc(entry["valid_until"]) <= utc(now)):
        return []
    resets = entry.get("resets", {})
    if any(type(resets.get(kind, {}).get("count")) is not int for kind in ("full", "five_hour")):
        return []
    source = entry.get("source") or "claude.ai settings/usage via Chrome"
    if entry.get("valid_until"):
        source += "; until " + datetime.fromisoformat(entry["valid_until"].replace("Z", "+00:00")).strftime("%-d %b %H:%MZ")
    else:
        source += ", read " + datetime.fromisoformat(entry["read_at"].replace("Z", "+00:00")).strftime("%H:%MZ")
    return [{"provider": "claude", "account": account, "kind": "grant", "name": "cedar_ember/" + kind,
             "used_pct": None, "window_min": None, "resets_at": grant.get("expires"),
             "amount": grant["count"], "source": source, "ts": entry["read_at"]}
            for kind, grant in resets.items() if kind in ("full", "five_hour")]


def rows(cache, now):
    usage = cache.get("utilization") or {}
    account_hash = hashlib.sha256(cache.get("accountUuid", "").encode()).hexdigest()[:8]
    account = account_label("claude", account_hash) or account_hash
    base = {"provider": "claude", "account": account, "source": "claude -p /usage; ~/.claude.json cachedUsageUtilization",
            "ts": utc(datetime.fromtimestamp(cache.get("fetchedAtMs", 0) / 1000, timezone.utc).isoformat()) or now}
    for limit in usage.get("limits") or []:
        scope = ((limit.get("scope") or {}).get("model") or {}).get("display_name")
        yield {**base, "kind": "window", "name": limit["kind"] + (f"/{scope}" if scope else ""),
               "used_pct": limit.get("percent"), "window_min": WINDOW_MIN.get(limit.get("group")),
               "resets_at": utc(limit.get("resets_at")), "amount": None}
    extra = usage.get("extra_usage") or {}
    yield {**base, "kind": "billing", "name": "extra_usage", "used_pct": extra.get("utilization"), "window_min": None,
           "resets_at": None, "amount": 1 if extra.get("is_enabled") else 0}
    spend = usage.get("spend") or {}
    if spend:
        yield {**base, "kind": "billing", "name": "spend", "used_pct": spend.get("percent"), "window_min": None,
               "resets_at": None, "amount": 1 if spend.get("enabled") or spend.get("auto_reload") else 0}
        if spend.get("balance") is not None:
            yield {**base, "kind": "billing", "name": "credits", "used_pct": None, "window_min": None,
                   "resets_at": None, "amount": spend["balance"]}
    resets = usage.get("cedar_ember")
    grants = resets.get("grants") if isinstance(resets, dict) else None
    if isinstance(grants, list):
        yield {**base, "kind": "grant", "name": "cedar_ember", "used_pct": None, "window_min": None,
               "resets_at": None, "amount": sum(g["resets_left"] for g in grants),
               "source": "~/.claude.json cachedUsageUtilization.cedar_ember"}
    else:
        yield {**base, "kind": "grant", "name": "cedar_ember", "used_pct": None, "window_min": None,
               "resets_at": None, "amount": None}
        yield from ({**row, "account": account, "ts": base["ts"]} for row in web_rows(account_hash, now))
    for name, bucket in usage.items():
        if name not in KNOWN and isinstance(bucket, dict) and "utilization" in bucket:
            yield {**base, "kind": "grant", "name": name, "used_pct": bucket.get("utilization"), "window_min": None,
                   "resets_at": utc(bucket.get("resets_at")), "amount": None, "raw": bucket}


def main(argv):
    from claude_profiles import config_dir, profiles
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    directories = {None, *(config_dir(p) for p in profiles().values())}
    if os.environ.get("CLAUDE_CONFIG_DIR"):
        directories.add(Path(os.environ["CLAUDE_CONFIG_DIR"]))
    for directory in sorted(directories, key=lambda p: str(p or "")):
        if directory is not None and not (directory / ".credentials.json").exists():
            continue
        read_profile(directory, argv, now)
    return 0


def read_profile(directory, argv, now):
    path = (directory or Path.home()) / ".claude.json"
    try:
        fetched = (json.load(open(path)).get("cachedUsageUtilization") or {}).get("fetchedAtMs", 0) / 1000
    except (OSError, ValueError):
        fetched = 0
    if "--no-refresh" not in argv and datetime.now(timezone.utc).timestamp() - fetched > FRESH_S:
        try:
            env = dict(os.environ, DISABLE_AUTOUPDATER="1", DISABLE_TELEMETRY="1", DISABLE_ERROR_REPORTING="1")
            env.pop("CLAUDE_CONFIG_DIR", None)
            if directory is not None:
                env["CLAUDE_CONFIG_DIR"] = str(directory)
            result = subprocess.run(["claude", "-p", "/usage", "--output-format", "json", "--setting-sources", "", "--strict-mcp-config", "--no-session-persistence"], env=env, capture_output=True, timeout=30, check=False)
            if result.returncode:
                throttled = b"429" in result.stderr or b"rate limit" in result.stderr.lower()
                print(f"usage_claude: refresh failed code={result.returncode} throttled={throttled}", file=sys.stderr)
        except (OSError, subprocess.TimeoutExpired):
            pass
    try:
        data = json.load(open(path))
        cache = data.get("cachedUsageUtilization")
        if cache and not cache.get("accountUuid"):
            cache = cache | {"accountUuid": (data.get("oauthAccount") or {}).get("accountUuid", "")}
    except (OSError, ValueError):
        return 0
    for row in rows(cache or {}, now) if cache else []:
        print(json.dumps(row))
    from claude_profiles import observed_plan
    plan = observed_plan(directory)
    uuid = (data.get("oauthAccount") or {}).get("accountUuid") or (cache or {}).get("accountUuid")
    if uuid:
        account = account_label("claude", hashlib.sha256(uuid.encode()).hexdigest()[:8])
        if account:
            print(json.dumps({"provider": "claude", "account": account, "kind": "billing", "name": "plan",
                              "ts": now, "amount": 1, "source": plan}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
