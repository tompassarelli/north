import math
from datetime import datetime


def window_age(window, at):
    try:
        stamp = datetime.fromisoformat(window["as_of"].replace("Z", "+00:00"))
        return at - stamp.timestamp() if stamp.tzinfo is not None else math.inf
    except (KeyError, ValueError, TypeError, AttributeError):
        return math.inf


def fresh(control, at):
    return (0 <= at - control.get("at", 0) <= 120
            and bool(control.get("windows"))
            and all(0 <= window_age(w, at) <= 120 for w in control["windows"]))


def advance(previous, windows, workers, at, initial=2, margin=10):
    allowed = previous.get("allowed_workers", initial)
    elapsed = max(0, at - previous.get("at", at)) / 60
    old_workers = previous.get("workers")
    worker_minutes = ((old_workers + workers) * elapsed / 2
                      if old_workers is not None and workers is not None else None)
    old = {w["window"]: w for w in previous.get("windows", [])}
    measured = {}
    updated = []
    for raw in windows:
        w = dict(raw)
        baseline = old.get(w["window"])
        if baseline and baseline.get("resets_at") == w.get("resets_at"):
            anchor = baseline.get("anchor", {"used": baseline["used_pct"], "at": previous["at"], "minutes": 0})
            anchor = dict(anchor, minutes=anchor["minutes"] + (worker_minutes or 0))
            delta = w["used_pct"] - anchor["used"]
            duration = (at - anchor["at"]) / 60
            if duration > 0 and delta >= 0 and (delta > 0 or duration >= 10):
                w["burn_pct_h"] = delta * 60 / duration
                if anchor["minutes"] > 0:
                    measured[w["window"]] = delta / anchor["minutes"]
                if duration >= 10:
                    anchor = {"used": w["used_pct"], "at": at, "minutes": 0}
            elif delta == 0:
                w["burn_pct_h"] = baseline["burn_pct_h"]
            w["anchor"] = anchor
        else:
            w["anchor"] = {"used": w["used_pct"], "at": at, "minutes": 0}
        w["projected_pct"] = (w["used_pct"] + w["burn_pct_h"] * w["reset_h"]
                              if w["burn_pct_h"] is not None else None)
        updated.append(w)
    rates = {kind: rate for kind, rate in previous.get("burn_pct_worker_min", {}).items()
             if kind in old and any(w["window"] == kind and w["resets_at"] == old[kind]["resets_at"] for w in updated)}
    rates.update(measured)
    sample = [[w["window"], w["as_of"]] for w in updated]
    fresh = bool(updated) and all(0 <= w["age_s"] <= 120 for w in updated)
    known = all(w["projected_pct"] is not None for w in updated)
    over = any(w["projected_pct"] is not None and w["projected_pct"] > 100 for w in updated)
    held = not fresh or over or workers is None
    new_sample = sample != previous.get("sample")
    if new_sample and over:
        allowed = max(1, allowed // 2)
    elif new_sample and not held and known and workers and len(rates) == len(updated):
        if all(w["used_pct"] + rates[w["window"]] * 60 * (allowed + 1) * w["reset_h"] <= 100 - margin for w in updated):
            allowed += 1
    return {"allowed_workers": allowed, "held": held, "workers": workers,
            "worker_minutes": worker_minutes, "burn_pct_worker_min": rates,
            "sample": sample, "at": at, "windows": updated,
            "changed": held != previous.get("held") or allowed != previous.get("allowed_workers", initial)}


def interval(previous, elapsed, throttled=False, retry_after=None):
    if throttled:
        return max(60, previous * 2, retry_after or 0)
    return max(60, math.ceil(elapsed * 2)) if elapsed > 20 else 60


def refusal(account, control, now):
    if not 0 <= now - control.get("at", 0) <= 120:
        return f"Hold: {account} pace sample is not fresh (needs <=120s); refresh before spawning."
    for w in control["windows"]:
        age = window_age(w, now)
        if not 0 <= age <= 120:
            return f"Hold: {account} {w['window']} usage is {age:.0f}s old; needs <=120s after refresh."
    for w in control["windows"]:
        if w["projected_pct"] is not None and w["projected_pct"] > 100:
            return (f"Hold: {account} {w['window']} used {w['used_pct']:.1f}% + burn "
                    f"{w['burn_pct_h']:.2f}%/h x {w['reset_h']:.2f}h to reset = {w['projected_pct']:.1f}% > 100%.")
    if control["held"]:
        return f"Hold: {account} fresh usage or live worker attribution is unavailable; allowed {control['allowed_workers']}."
    if control["workers"] >= control["allowed_workers"]:
        return f"Hold: {account} workers {control['workers']}/{control['allowed_workers']} allowed by measured pace."
    return None
