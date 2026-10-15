import json
import os
import re
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(os.environ.get("LEDGER_CONTEXT_PY", __file__)).resolve().parent))
from defaults import DEFAULTS


ROLES = ("worker", "lead", "sub-lead", "proxy")
HANDOFF = re.compile(r"Your context is \d+k tokens, past the \d+k handoff point\.|You have run \d+ minutes\. Land anything that passes now")
ROLE = re.compile(r"^\s*(?:Delegation:\s*role=|Your role is |AGENT_ROLE[=\s]+)(worker|sub-lead|lead|proxy)\b", re.M)


def current_role(session=None, agent=None):
    if agent:
        return "worker"
    session = session or os.environ.get("CODEX_THREAD_ID") or os.environ.get("CLAUDE_CODE_SESSION_ID")
    name = os.environ.get("AGENT_ORG_NAME")
    path = os.environ.get("AGENTS_ORG_FILE", str(Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents/org.json"))
    try:
        nodes = json.loads(Path(path).read_text()).get("nodes", [])
        node = next((n for n in nodes if session and n.get("session") == session), None)
        if node is None and name:
            node = next((n for n in nodes if n.get("id") == name), None)
        if node is not None:
            return node.get("role", "unknown") if node.get("state", "active") == "active" else "unknown"
    except (OSError, ValueError, TypeError, AttributeError):
        pass
    return os.environ.get("AGENT_ROLE") or ("unknown" if name else "proxy")


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
        ignored += not any(re.match(r"^\s*(?:HANDOFF\s+\S+|(?:Not done|Done):)", text) for text in turns.values())
    if provider == "claude" and agent and not handoffs:
        directory = os.environ.get("LEDGER_HANDOFFS", os.path.expanduser("~/.local/state/agents/handoffs"))
        if any(os.path.isfile(os.path.join(directory, f".{agent}.{suffix}")) for suffix in ("reminded", "leash")):
            handoffs = 1
            ignored = None
    return dict(peak_ctx=max((v[1] for v in values), default=0), compactions=max(boundaries, summaries),
                handoff_fires=handoffs, handoff_ignored=ignored, input_tokens=sum(v[1] for v in values),
                cache_read_tokens=sum(v[2] for v in values), idle_gap_misses=misses)


def account_id(provider):
    from usage import local_account
    label = local_account(provider)
    return f"{provider}-{label}" if label else provider


def list_prices(catalog):
    prices = {}
    for row in catalog.get("models", {}).values():
        api = (row.get("value") or {}).get("api") if isinstance(row.get("value"), dict) else None
        if row.get("model") and api and api.get("input_per_million") is not None and api.get("output_per_million") is not None:
            prices[row["model"]] = (api["input_per_million"], api["output_per_million"])
    return prices


def usage_weight(model, inp, write, read, out, prices):
    rate = prices.get(model) if DEFAULTS["plan_meter_weighting"] == "price" else None
    if rate is None:
        return inp + write * 1.25 + read * (.05 if "opus" in (model or "").lower() else .1) + out * 5, False
    return inp * rate[0] + write * rate[0] * 1.25 + read * rate[0] * .1 + out * rate[1], True


def run_usage(records, provider, catalog=None):
    import tomllib
    from pathlib import Path
    catalog = catalog if catalog is not None else tomllib.load(open(Path(os.environ.get("LEDGER_CONTEXT_PY", __file__)).resolve().parent.parent / "model-catalog.toml", "rb"))
    account = account_id(provider)
    prices, unpriced = list_prices(catalog), set()
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
        weight, priced = usage_weight(model, inp, write, read, out, prices)
        if weight and not priced and DEFAULTS["plan_meter_weighting"] == "price":
            unpriced.add(model or "unknown")
        sample = dict(request=key, ts=r["timestamp"], weight=weight)
        if key not in requests or weight > requests[key]["weight"]:
            requests[key] = sample
    if effort is None:
        effort = next((row.get("effort") for row in catalog.get("models", {}).values() if row.get("model") == model), None)
    return dict(account=account, model=model, effort=effort, fast=fast, usage_samples=list(requests.values()),
                unpriced_models=sorted(unpriced))


def same_window(a, b):
    if a == b:
        return True
    if not a or not b:
        return False
    return abs((datetime.fromisoformat(a.replace("Z", "+00:00")) - datetime.fromisoformat(b.replace("Z", "+00:00"))).total_seconds()) <= 60


UNTRACKED = "(untracked)"


def allocation_intervals(readouts, samples, prices):
    from collections import defaultdict
    from bisect import bisect_right
    windows, result = defaultdict(dict), []
    by_account = defaultdict(list)
    for sample in samples:
        by_account[sample["account"]].append((datetime.fromisoformat(sample["ts"].replace("Z", "+00:00")), sample))
    for account in by_account:
        by_account[account].sort(key=lambda pair: pair[0])
    for row in readouts:
        if row.get("kind") != "window" or row.get("name") not in ("week", "weekly_all", "Gemini Models/weekly") or row.get("used_pct") is None:
            continue
        account = row["provider"] + ("-" + row["account"] if row["provider"] == "claude" else "")
        windows[account][row["ts"]] = row
    for account, readings in windows.items():
        ordered = sorted(readings.values(), key=lambda r: datetime.fromisoformat(r["ts"].replace("Z", "+00:00")))
        events = by_account[account]
        timestamps = [timestamp for timestamp, _ in events]
        plateau = ordered[0]
        for before, after in zip(ordered, ordered[1:]):
            if not same_window(before.get("resets_at"), after.get("resets_at")) or after["used_pct"] < before["used_pct"]:
                plateau = after
                continue
            change = after["used_pct"] - before["used_pct"]
            if not change:
                continue
            initial = plateau
            start, end = (datetime.fromisoformat(r["ts"].replace("Z", "+00:00")) for r in (initial, after))
            weights = defaultdict(float)
            for _, sample in events[bisect_right(timestamps, start):bisect_right(timestamps, end)]:
                weights[sample["agent"]] += sample["weight"]
            total = sum(weights.values())
            if account in prices:
                cost = change * prices[account] / 100
                if total:
                    for agent, weight in weights.items():
                        result.append(dict(account=account, started=initial["ts"], ended=after["ts"], agent=agent, cost_usd=cost * weight / total))
                elif account == "codex":
                    result.append(dict(account=account, started=initial["ts"], ended=after["ts"], agent=UNTRACKED, cost_usd=cost))
            plateau = after
    return result


def allocate_costs(readouts, samples, prices):
    from collections import defaultdict
    costs = defaultdict(float)
    for row in allocation_intervals(readouts, samples, prices):
        costs[row["agent"]] += row["cost_usd"]
    return dict(costs)


def moment(text):
    from datetime import timezone
    value = datetime.fromisoformat(text.replace("Z", "+00:00"))
    return value if value.tzinfo else value.replace(tzinfo=timezone.utc)


def box_groups(rows, gap=None):
    """Landed boxes: worker runs on one Item: in staffed order, chained while each is staffed within gap minutes of the
    chain's latest end; each chain with a landed run is one box, retries and follow-ups included."""
    gap = DEFAULTS["box_chain_gap_min"] if gap is None else gap
    by_item: dict = {}
    for r in rows:
        if r.get("item") and r.get("role", "worker") == "worker" and r["outcome"] != "split" and r.get("ended"):
            by_item.setdefault(r["item"], []).append({**r, "begin": moment(r.get("staffed") or r.get("started") or r["ended"])})
    groups = []
    for rs in by_item.values():
        rs.sort(key=lambda r: r["begin"])
        chain, end = [], None
        for r in rs:
            if end is None or (r["begin"] - end).total_seconds() / 60 > gap:
                chain = []
                groups.append(chain)
                end = None
            chain.append(r)
            end = max(filter(None, (end, moment(r["ended"]))))
    return [g for g in groups if any(r.get("landed") for r in g)]


def delivery_metrics(rows, project=None):
    import statistics
    rows = [dict(r) for r in rows]
    if project is not None:
        rows = [r for r in rows if (r.get("item") or "").split("#")[0] == project]
    boxes = [g for g in box_groups(rows) if all(r.get("cost_usd") is not None for r in g)]
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
    costs = [sum(r["cost_usd"] for r in g) for g in boxes]
    return dict(done_boxes=len(rows), concurrency=concurrency, median_brief_to_landed_min=statistics.median(durations) if durations else None,
                median_cost_per_done_usd=statistics.median(costs) if costs else None)


if __name__ == "__main__":
    import functools
    import tomllib
    account_id = functools.cache(account_id)
    catalog = tomllib.load(open(Path(os.environ.get("LEDGER_CONTEXT_PY", __file__)).resolve().parent.parent / "model-catalog.toml", "rb"))
    for line in sys.stdin:
        path, agent = line.rstrip("\n").split("\t")
        rows = []
        for record in open(path):
            try:
                rows.append(json.loads(record))
            except ValueError:
                continue
        try:
            metadata = json.load(open(Path(path).with_suffix(".meta.json")))
        except (OSError, ValueError):
            metadata = None
        tier = metadata.get("agentType") if isinstance(metadata, dict) else None
        tier = "" if tier is None or tier is False else tier if isinstance(tier, str) else json.dumps(tier)
        role = session_role(rows, agent, "worker" if tier.startswith("worker") else None)
        if not role:
            continue
        telemetry = run_usage(rows, "claude", catalog)
        if metadata is not None:
            try:
                operator = Path(os.environ.get("AGENTS_OPERATOR_DIR") or Path.home() / "code/south/main/agents")
                cfg = tomllib.load(open(os.environ.get("AGENTS_ORCHESTRATION") or operator / "orchestration.toml", "rb"))
                telemetry["effort"] = metadata.get("effort") or next((r["effort"] for r in [*cfg.get("tiers", {}).values(), *cfg.get("on_request", {}).values()]
                                                                   if r.get("agent") == metadata.get("agentType") and r.get("model") == telemetry["model"]), telemetry["effort"])
            except (OSError, ValueError):
                pass
        telemetry = dict(context_metrics(rows, "claude", agent if role == "worker" else None), **telemetry, role=role)
        print(path, agent, role, json.dumps(telemetry), tier, sep="\t")
