import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from defaults import DEFAULTS


def account_id(provider):
    from usage import local_account
    label = local_account(provider)
    return f"{provider}-{label}" if label else provider


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
