import math
import os
from datetime import datetime
from pathlib import Path
import tomllib
from defaults import DEFAULTS


def target_for(account, at):
    path = Path(os.environ.get("AGENTS_ORCHESTRATION") or
                Path(os.environ.get("AGENTS_OPERATOR_DIR", Path.home() / "code/south/main/agents")) / "orchestration.toml")
    try:
        with path.open("rb") as source:
            entry = tomllib.load(source).get("pace", {}).get(account)
        if entry is None:
            return None
        by = entry["by"]
        stamp = by if isinstance(by, datetime) else datetime.fromisoformat(by.replace("Z", "+00:00"))
        goal = float(entry["spend_to_pct"])
        if stamp.tzinfo is None or stamp.utcoffset().total_seconds() != 0 or not 0 < goal <= 100:
            raise ValueError
        return {"by": stamp.isoformat(), "spend_to_pct": goal} if stamp.timestamp() > at else None
    except (OSError, ValueError, TypeError, KeyError, AttributeError):
        raise ValueError(f"invalid or unreadable pace target policy for {account}") from None


def horizon(window, target, at):
    if target is None:
        return window["reset_h"]
    return min(window["reset_h"], max(0, (datetime.fromisoformat(target["by"]).timestamp() - at) / 3600))


def projected(window, target, at):
    return window["used_pct"] + window["burn_pct_h"] * horizon(window, target, at) if window["burn_pct_h"] is not None else None


def below_deadline_line(window, target, at):
    if target is None or window["window"] != "week":
        return False
    try:
        start = datetime.fromisoformat(window["resets_at"].replace("Z", "+00:00")).timestamp() - 7 * 86400
        end = datetime.fromisoformat(target["by"]).timestamp()
        line = target["spend_to_pct"] * min(1, max(0, (at - start) / (end - start)))
        return window["used_pct"] < line
    except (KeyError, TypeError, ValueError, ZeroDivisionError):
        return False


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


def advance(previous, windows, workers, at, initial=2, margin=10, target=None):
    if target and datetime.fromisoformat(target["by"]).timestamp() <= at:
        target = None
    allowed = previous.get("allowed_workers", initial) if previous.get("target") == target else initial
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
            if duration >= 10 and delta >= 0:
                w["burn_pct_h"] = delta * 60 / duration
                if anchor["minutes"] > 0:
                    measured[w["window"]] = delta / anchor["minutes"]
                if duration >= 10:
                    anchor = {"used": w["used_pct"], "at": at, "minutes": 0}
            elif delta >= 0:
                w["burn_pct_h"] = baseline["burn_pct_h"]
            w["anchor"] = anchor
        else:
            w["anchor"] = {"used": w["used_pct"], "at": at, "minutes": 0}
        w["horizon_h"] = horizon(w, target, at)
        w["projected_pct"] = (w["used_pct"] + w["burn_pct_h"] * w["horizon_h"]
                              if w["burn_pct_h"] is not None else None)
        updated.append(w)
    rates = {kind: rate for kind, rate in previous.get("burn_pct_worker_min", {}).items()
             if kind in old and any(w["window"] == kind and w["resets_at"] == old[kind]["resets_at"] for w in updated)}
    rates.update(measured)
    sample = [[w["window"], w["as_of"]] for w in updated]
    fresh = bool(updated) and all(0 <= w["age_s"] <= 120 for w in updated)
    known = all(w["projected_pct"] is not None for w in updated)
    exhausted = any(w["window"] == "week" and w["used_pct"] >= DEFAULTS["money_ceiling"] for w in updated)
    over = exhausted or any(w["projected_pct"] is not None and w["projected_pct"] > 100 for w in updated)
    held = not fresh or over or workers is None
    state_since = previous.get("state_since", previous.get("at", at))
    if "held" in previous and previous.get("target") == target and held != previous["held"] and not over and fresh and at - state_since < DEFAULTS["pace_state_dwell_min"] * 60:
        held = previous["held"]
    if held != previous.get("held"):
        state_since = at
    new_sample = sample != previous.get("sample") or target != previous.get("target")
    if new_sample and over:
        allowed = max(1, allowed // 2)
    elif new_sample and not held and known and workers:
        if target and all(w["projected_pct"] < target["spend_to_pct"] or below_deadline_line(w, target, at) for w in updated):
            allowed += 1
        elif not target and len(rates) == len(updated) and all(w["used_pct"] + rates[w["window"]] * 60 * (allowed + 1) * w["reset_h"] <= 100 - margin for w in updated):
            allowed += 1
    return {"allowed_workers": allowed, "held": held, "workers": workers,
            "worker_minutes": worker_minutes, "burn_pct_worker_min": rates,
            "sample": sample, "at": at, "windows": updated,
            "target": target, "state_since": state_since,
            "changed": held != previous.get("held") or allowed != previous.get("allowed_workers", initial) or target != previous.get("target")}


def interval(previous, elapsed, throttled=False, retry_after=None):
    if throttled:
        return max(60, previous * 2, retry_after or 0)
    return max(60, math.ceil(elapsed * 2)) if elapsed > 20 else 60


def refusal(account, control, now):
    try:
        target = target_for(account, now)
    except ValueError as error:
        return f"Hold: {error}."
    if not 0 <= now - control.get("at", 0) <= 120:
        return f"Hold: {account} pace sample is not fresh (needs <=120s); refresh before spawning."
    for w in control["windows"]:
        age = window_age(w, now)
        if not 0 <= age <= 120:
            return f"Hold: {account} {w['window']} usage is {age:.0f}s old; needs <=120s after refresh."
    for w in control["windows"]:
        if w["window"] == "week" and w["used_pct"] >= DEFAULTS["money_ceiling"]:
            return f"Hold: {account} week used {w['used_pct']:.1f}% reaches the {DEFAULTS['money_ceiling']}% admission ceiling; wait for reset."
        projection = projected(w, target, now)
        if projection is not None and projection > 100:
            return (f"Hold: {account} {w['window']} used {w['used_pct']:.1f}% + burn "
                    f"{w['burn_pct_h']:.2f}%/h x {horizon(w, target, now):.2f}h to "
                    f"{target['by'] if target else 'reset'} = {projection:.1f}% > 100%.")
    if control["held"] or control.get("workers") is None:
        return f"Hold: {account} fresh usage or live worker attribution is unavailable; allowed {control['allowed_workers']}."
    if control["workers"] >= control["allowed_workers"]:
        return f"Hold: {account} workers {control['workers']}/{control['allowed_workers']} allowed by measured pace."
    return None
