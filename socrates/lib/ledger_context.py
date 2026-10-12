import json
import os
import re
from datetime import datetime


ROLES = ("worker", "lead", "sub-lead", "proxy")
HANDOFF = re.compile(r"Your context is \d+k tokens, past the \d+k handoff point\.|You have run \d+ minutes\. Land anything that passes now")
ROLE = re.compile(r"^\s*(?:Delegation:\s*role=|Your role is |AGENT_ROLE[=\s]+)(worker|sub-lead|lead|proxy)\b", re.M)


def record_text(r):
    body = r.get("message") or r.get("payload") or r.get("attachment") or r
    content = body.get("content") or body.get("prompt") or body.get("text") or ""
    if isinstance(content, str):
        return content
    return "\n".join(c if isinstance(c, str) else c.get("text") or (c.get("content") if isinstance(c.get("content"), str) else "") or ""
                     for c in content or [] if isinstance(c, (dict, str)))


def session_role(records, session, fallback=None, brief=""):
    path = os.environ.get("AGENTS_ORG_FILE", os.path.expanduser("~/.local/state/agents/org.json"))
    try:
        nodes = json.load(open(path)).get("nodes", [])
        role = next((n.get("role") for n in nodes if n.get("session") == session), None)
        if role in ROLES:
            return role
    except (OSError, ValueError):
        pass
    texts = [brief]
    for r in records:
        p = r.get("payload") or {}
        content = (r.get("message") or {}).get("content")
        if r.get("isCompactSummary") or (isinstance(content, list) and any(c.get("type") == "tool_result" for c in content)):
            continue
        if r.get("type") in ("user", "system", "attachment") or (p.get("type") == "message" and p.get("role") in ("user", "developer")):
            texts.append(record_text(r))
    for text in texts:
        match = ROLE.search(text)
        if match:
            return match[1]
        if "You are Plato, Tom's proxy and primary assistant" in text:
            return "proxy"
    return fallback or next(("worker" for r in records if str(r.get("attributionAgent", "")).startswith("worker")), None)


def hook_fire(r):
    text = record_text(r).strip()
    try:
        result = json.loads(text)
        text = result.get("hookSpecificOutput", {}).get("additionalContext", "") if isinstance(result, dict) else ""
    except ValueError:
        content = (r.get("message") or {}).get("content")
        if isinstance(content, list) and any(c.get("type") == "tool_result" for c in content):
            return False
    text = re.sub(r"^<system-reminder>\s*", "", text)
    return bool(HANDOFF.match(text))


def context_metrics(records, provider, agent=None):
    requests = {}
    boundaries = summaries = handoffs = 0
    fires = []
    for index, r in enumerate(records):
        kind, p = r.get("type"), r.get("payload") or {}
        if provider == "claude":
            boundaries += kind == "system" and r.get("subtype") == "compact_boundary"
            summaries += bool(r.get("isCompactSummary"))
            if kind in ("user", "system", "attachment") and not r.get("isCompactSummary"):
                if hook_fire(r):
                    handoffs += 1
                    fires.append(index)
            usage = (r.get("message") or {}).get("usage") if kind == "assistant" else None
            key = r.get("requestId") or (r.get("message") or {}).get("id") or index
            if usage is None:
                continue
            cache = usage.get("cache_read_input_tokens") or 0
            total = (usage.get("input_tokens") or 0) + cache + (usage.get("cache_creation_input_tokens") or 0)
        else:
            boundaries += kind == "compacted"
            summaries += kind == "event_msg" and p.get("type") == "context_compacted"
            if kind == "response_item" and p.get("type") == "message" and p.get("role") in ("user", "developer"):
                handoffs += hook_fire(r)
            if kind == "event_msg" and p.get("type") == "user_message":
                handoffs += hook_fire({"content": p.get("message") or ""})
            info = p.get("info") if kind == "event_msg" and p.get("type") == "token_count" else None
            if not info:
                continue
            usage = info.get("last_token_usage") or {}
            key = p.get("request_id") or json.dumps([info.get("total_token_usage"), usage], sort_keys=True)
            cache = usage.get("cached_input_tokens") or 0
            # Codex input_tokens already includes cached_input_tokens in its rollout format.
            total = usage.get("input_tokens") or 0
        timestamp = r.get("timestamp")
        previous = requests.get(key)
        if previous is None:
            requests[key] = (timestamp, total, cache)
        elif total > previous[1]:
            requests[key] = (previous[0], total, cache)
    values = sorted(requests.values(), key=lambda v: v[0] or "")
    misses, previous = 0, None
    for timestamp, total, cache in values:
        if timestamp:
            at = datetime.fromisoformat(timestamp.replace("Z", "+00:00")).timestamp()
            if previous is not None and at - previous > 3600 and total > 0 and cache / total <= .05:
                misses += 1
            previous = at
    ignored = 0 if provider == "claude" else None
    for fire in fires:
        turns = {}
        for index, r in enumerate(records[fire + 1:], fire + 1):
            if r.get("type") != "assistant":
                continue
            key = r.get("requestId") or (r.get("message") or {}).get("id") or index
            if key not in turns and len(turns) == 3:
                break
            turns[key] = turns.get(key, "") + record_text(r)
        ignored += not any(re.match(r"^\s*HANDOFF\s+\S+", text) for text in turns.values())
    if provider == "claude" and agent and not handoffs:
        directory = os.environ.get("LEDGER_HANDOFFS", os.path.expanduser("~/.local/state/agents/handoffs"))
        if any(os.path.isfile(os.path.join(directory, f".{agent}.{suffix}")) for suffix in ("reminded", "leash")):
            handoffs = 1
            ignored = None
    return dict(peak_ctx=max((v[1] for v in values), default=0), compactions=max(boundaries, summaries),
                handoff_fires=handoffs, handoff_ignored=ignored, input_tokens=sum(v[1] for v in values),
                cache_read_tokens=sum(v[2] for v in values), idle_gap_misses=misses)


def account_id(provider, host, catalog):
    label = catalog.get("accounts", {}).get(provider, {}).get("machines", {}).get(host)
    return f"{provider}-{label}" if label else provider


def run_usage(records, provider, host=None, catalog=None):
    import socket
    import tomllib
    from pathlib import Path
    catalog = catalog if catalog is not None else tomllib.load(open(Path(os.environ.get("LEDGER_CONTEXT_PY", __file__)).resolve().parent.parent / "model-catalog.toml", "rb"))
    account = account_id(provider, host or socket.gethostname(), catalog)
    model, effort, fast = None, None, 0
    requests, previous = {}, {}
    for index, r in enumerate(records):
        p = r.get("payload") or {}
        if r.get("type") == "session_meta" and "service_tier" in p:
            fast = int(p["service_tier"] in ("fast", "priority"))
        if r.get("type") == "turn_context":
            model, effort = p.get("model") or model, p.get("effort") or effort
            if "service_tier" in p:
                fast = int(p["service_tier"] in ("fast", "priority"))
        message = r.get("message") or {}
        if provider == "claude":
            usage = message.get("usage") if r.get("type") == "assistant" else None
            if usage is None:
                continue
            model = message.get("model") or model
            effort = message.get("effort") or r.get("effort") or effort
            fast = int(usage.get("speed") == "fast")
            key = str(r.get("requestId") or message.get("id") or index)
            inp = usage.get("input_tokens") or 0
            write = usage.get("cache_creation_input_tokens") or 0
            read = usage.get("cache_read_input_tokens") or 0
            out = usage.get("output_tokens") or 0
        elif provider == "codex":
            if r.get("type") != "event_msg" or p.get("type") != "token_count" or not p.get("info"):
                continue
            info = p["info"]
            cumulative = info.get("total_token_usage") or {}
            last = info.get("last_token_usage") or {}
            key = str(p.get("request_id") or json.dumps([cumulative, last], sort_keys=True))
            if key in requests:
                continue
            usage = {k: max(0, (v or 0) - (previous.get(k) or 0)) for k, v in cumulative.items()}
            usage.update({k: v for k, v in last.items() if k not in cumulative})
            previous = cumulative
            read = usage.get("cached_input_tokens") or 0
            inp = max(0, (usage.get("input_tokens") or 0) - read)
            write, out = 0, usage.get("output_tokens") or 0
        else:
            continue
        if not r.get("timestamp"):
            continue
        weight = inp + write * 1.25 + read * (.05 if "opus" in (model or "").lower() else .1) + out * 5
        sample = dict(request=key, ts=r["timestamp"], weight=weight)
        if key not in requests or weight > requests[key]["weight"]:
            requests[key] = sample
    if effort is None:
        effort = next((row.get("effort") for row in catalog.get("models", {}).values() if row.get("model") == model), None)
    return dict(account=account, model=model, effort=effort, fast=fast, usage_samples=list(requests.values()))


def allocation_intervals(readouts, samples, prices):
    from collections import defaultdict
    windows, result = defaultdict(dict), []
    for row in readouts:
        if row.get("kind") != "window" or row.get("name") not in ("week", "weekly_all", "Gemini Models/weekly") or row.get("used_pct") is None:
            continue
        account = row["provider"] + ("-" + row["account"] if row["provider"] == "claude" else "")
        windows[account][row["ts"]] = row
    for account, readings in windows.items():
        ordered = sorted(readings.values(), key=lambda r: datetime.fromisoformat(r["ts"].replace("Z", "+00:00")))
        for before, after in zip(ordered, ordered[1:]):
            if before.get("resets_at") != after.get("resets_at") or after["used_pct"] < before["used_pct"]:
                continue
            start, end = (datetime.fromisoformat(r["ts"].replace("Z", "+00:00")) for r in (before, after))
            weights = defaultdict(float)
            for sample in samples:
                if sample["account"] == account and start < datetime.fromisoformat(sample["ts"].replace("Z", "+00:00")) <= end:
                    weights[sample["agent"]] += sample["weight"]
            total = sum(weights.values())
            if total and account in prices:
                cost = (after["used_pct"] - before["used_pct"]) * prices[account] / 100
                for agent, weight in weights.items():
                    result.append(dict(account=account, started=before["ts"], ended=after["ts"], agent=agent, cost_usd=cost * weight / total))
    return result


def allocate_costs(readouts, samples, prices):
    from collections import defaultdict
    costs = defaultdict(float)
    for row in allocation_intervals(readouts, samples, prices):
        costs[row["agent"]] += row["cost_usd"]
    return dict(costs)


def delivery_metrics(rows, project=None):
    import statistics
    rows = [dict(r) for r in rows]
    if project is not None:
        rows = [r for r in rows if (r.get("item") or "").split("#")[0] == project]
    rows = [r for r in rows if r.get("role", "worker") == "worker" and r.get("outcome") == "done" and r.get("landed") == 1]
    events = []
    for r in rows:
        if (r.get("staffed") or r.get("started")) and r.get("ended"):
            events.extend(((datetime.fromisoformat((r.get("staffed") or r["started"]).replace("Z", "+00:00")), 1),
                           (datetime.fromisoformat(r["ended"].replace("Z", "+00:00")), -1)))
    active, concurrency = 0, 1
    for _, delta in sorted(events):
        active += delta
        concurrency = max(concurrency, active)
    followed = {r.get("follows") for r in rows}
    rows = [r for r in rows if r.get("agent") not in followed]
    durations = [(datetime.fromisoformat(r["landed_at"].replace("Z", "+00:00")) - datetime.fromisoformat((r.get("staffed") or r["started"]).replace("Z", "+00:00"))).total_seconds() / 60
                 for r in rows if r.get("landed_at") and (r.get("staffed") or r.get("started"))]
    costs = [r["cost_usd"] for r in rows if r.get("cost_usd") is not None]
    return dict(done_boxes=len(rows), concurrency=concurrency, median_brief_to_landed_min=statistics.median(durations) if durations else None,
                median_cost_per_done_usd=statistics.median(costs) if costs else None)


if __name__ == "__main__":
    import sys
    rows = []
    for line in open(sys.argv[1]):
        try:
            rows.append(json.loads(line))
        except ValueError:
            continue
    role = session_role(rows, sys.argv[2], sys.argv[3] or None)
    telemetry = run_usage(rows, "claude")
    import tomllib
    from pathlib import Path
    try:
        metadata = json.load(open(Path(sys.argv[1]).with_suffix(".meta.json")))
        operator = Path(os.environ.get("AGENTS_OPERATOR_DIR") or Path.home() / "code/south/main/agents")
        cfg = tomllib.load(open(os.environ.get("AGENTS_ORCHESTRATION") or operator / "orchestration.toml", "rb"))
        telemetry["effort"] = metadata.get("effort") or next((r["effort"] for r in [*cfg.get("tiers", {}).values(), *cfg.get("on_request", {}).values()]
                                                           if r.get("agent") == metadata.get("agentType") and r.get("model") == telemetry["model"]), telemetry["effort"])
    except (OSError, ValueError):
        pass
    print(json.dumps(dict(context_metrics(rows, "claude", sys.argv[2] if role == "worker" else None), **telemetry, role=role)))
