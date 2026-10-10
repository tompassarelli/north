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
    return "\n".join(c.get("text") or (c.get("content") if isinstance(c.get("content"), str) else "") or ""
                     for c in content or [] if isinstance(c, dict))


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
    for index, r in enumerate(records):
        kind, p = r.get("type"), r.get("payload") or {}
        if provider == "claude":
            boundaries += kind == "system" and r.get("subtype") == "compact_boundary"
            summaries += bool(r.get("isCompactSummary"))
            if kind in ("user", "system", "attachment") and not r.get("isCompactSummary"):
                handoffs += hook_fire(r)
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
    if provider == "claude" and agent:
        directory = os.environ.get("LEDGER_HANDOFFS", os.path.expanduser("~/.local/state/agents/handoffs"))
        if os.path.isfile(os.path.join(directory, f".{agent}.reminded")):
            handoffs = 1
    return dict(peak_ctx=max((v[1] for v in values), default=0), compactions=max(boundaries, summaries),
                handoff_fires=handoffs, input_tokens=sum(v[1] for v in values),
                cache_read_tokens=sum(v[2] for v in values), idle_gap_misses=misses)


if __name__ == "__main__":
    import sys
    rows = []
    for line in open(sys.argv[1]):
        try:
            rows.append(json.loads(line))
        except ValueError:
            continue
    role = session_role(rows, sys.argv[2], sys.argv[3] or None)
    print(json.dumps(dict(context_metrics(rows, "claude", sys.argv[2] if role == "worker" else None), role=role)))
