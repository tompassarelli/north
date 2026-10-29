#!/usr/bin/env python3
"""Offline learned-routing backtest (north#35): walk-forward logistic regression and a RouteLLM-style
matrix factorisation against recommend's tier pick and the 2-failed-fixes retry rule, on a read-only
copy of the NorthThreads ledger. Stdlib only.

usage: harness/scripts/routing-backtest.py <copy of snapshot.db> [--boot 2000] [--seed 7] [--retrieval-only]

--retrieval-only requires >=300 flow-qualified finished runs and skips the legacy model/retry arms.
Neighbor price/success is total known allocated USD divided by successful priced neighbors;
unpriced neighbors cannot provide free successes. Allocation window ends proxy price availability.
Matched-tier comparisons are observational and may select different populations."""

import argparse
import bisect
from collections import Counter
import json
import re
import sqlite3, subprocess
import math
import random
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "lib"))
from calibration import threads_call
from defaults import DEFAULTS
from policy_stats import wilson
from run_outcome import succeeded, unjudged

FACETS = ["spec", "scope", "surface", "verify"]
GROUPS = ["category", *FACETS]
BACKOFF = ["surface", "verify", "scope", "spec"]
MIN_RUNS = USD_MIN = DEFAULTS["min_runs"]
SUCCESS_THRESHOLD = DEFAULTS["success_threshold"]
DEFAULT_TIER = DEFAULTS["default_tier"]
SPAWN_ORIGINS = ("codex.PostToolUse.spawn_agent", "claude.PostToolUse.Agent")
REFIT_EVERY = 20
MIN_TRAIN = 30


def sigmoid(z):
    return 1 / (1 + math.exp(-max(-30.0, min(30.0, z))))


class Logistic:
    def __init__(self, rows, seed, epochs=25, lr=0.2, l2=1e-3):
        self.w = {}
        g2 = {}
        rng = random.Random(seed)
        order = list(range(len(rows)))
        for _ in range(epochs):
            rng.shuffle(order)
            for i in order:
                x, y = rows[i]
                err = self.prob(x) - y
                for k, v in x.items():
                    g = err * v + l2 * self.w.get(k, 0.0)
                    g2[k] = g2.get(k, 1e-8) + g * g
                    self.w[k] = self.w.get(k, 0.0) - lr * g / math.sqrt(g2[k])

    def prob(self, x):
        return sigmoid(sum(self.w.get(k, 0.0) * v for k, v in x.items()))


class Factorisation:
    def __init__(self, rows, seed, dim=3, epochs=40, lr=0.05, l2=0.02):
        rng = random.Random(seed)
        self.dim, self.b, self.br, self.bc, self.u, self.v = dim, 0.0, {}, {}, {}, {}
        vec = lambda: [rng.gauss(0, 0.1) for _ in range(dim)]
        for r, c, _ in rows:
            self.u.setdefault(r, vec())
            self.v.setdefault(c, vec())
        order = list(range(len(rows)))
        for _ in range(epochs):
            rng.shuffle(order)
            for i in order:
                r, c, y = rows[i]
                err = self.prob(r, c) - y
                self.b -= lr * err
                self.br[r] = self.br.get(r, 0.0) - lr * (err + l2 * self.br.get(r, 0.0))
                self.bc[c] = self.bc.get(c, 0.0) - lr * (err + l2 * self.bc.get(c, 0.0))
                u, v = self.u[r], self.v[c]
                for k in range(dim):
                    u[k], v[k] = u[k] - lr * (err * v[k] + l2 * u[k]), v[k] - lr * (err * u[k] + l2 * v[k])

    def prob(self, r, c):
        u, v = self.u.get(r, [0.0] * self.dim), self.v.get(c, [0.0] * self.dim)
        return sigmoid(self.b + self.br.get(r, 0.0) + self.bc.get(c, 0.0) + sum(a * b for a, b in zip(u, v)))


def percentile_of(sorted_xs, x):
    if x is None or not sorted_xs:
        return None
    return bisect.bisect_left(sorted_xs, x) / len(sorted_xs)


def chain_depth(r, by_agent):
    d, seen = 1, {r["agent"]}
    while r["follows"]:
        d += 1
        r = by_agent.get(r["follows"])
        if r is None or r["agent"] in seen:
            break
        seen.add(r["agent"])
    return d


def annotated(rows):
    """The rows with the cells, tier key and rank, and brief coverage `north threads annotate-runs` reads off each, under `note`."""
    pending = [r for r in rows if "note" not in r]
    notes = iter(threads_call("annotate-runs", pending)["rows"] if pending else [])
    return [r if "note" in r else dict(r, note=next(notes)) for r in rows]


def recommend_run(r, ok):
    return {"cells": r["note"]["cells"], "tier": r["note"]["tier"], "ok": ok, "minutes": r["minutes"],
            "cost_usd": r["cost_usd"], "outcome": r["outcome"]}


def recommend_cases(cases, carry=False):
    """`north threads recommend-runs`: one pick per case of (runs, asked cells), each pick's stats listing tier, rank and n."""
    request = {"carry": carry, "cases": [{"runs": runs, "asked": asked} for runs, asked in cases]}
    return threads_call("recommend-runs", request)["picks"] if cases else []


def load(db):
    worker = [dict(r) for r in db.execute("SELECT * FROM runs WHERE role = 'worker' ORDER BY ended")]
    escalated = {r["follows"] for r in worker if r["follows"]}
    rows = annotated([r for r in worker if not unjudged(r)])
    by_agent = {r["agent"]: r for r in rows}
    children = {}
    for r in rows:
        if r["follows"] in by_agent:
            children.setdefault(r["follows"], []).append(r)
    cat_minutes = {}
    for r in rows:
        cat = r["note"]["cells"][0]
        prior = cat_minutes.setdefault(cat, [])
        r["cat"] = cat
        r["tierk"] = r["note"]["tier"]
        r["difficulty"] = percentile_of(prior, r["minutes"])
        r["depth"] = chain_depth(r, by_agent)
        r["ok_final"] = succeeded(r, r["agent"] in escalated)
        r["escalation"] = r["outcome"] == "escalated" or r["agent"] in children
        bisect.insort(prior, r["minutes"])
    return rows, by_agent, children


def dataset_report(rows):
    n = len(rows)
    ok = sum(r["ok_final"] for r in rows)
    print(f"dataset: {n} finished worker runs {rows[0]['ended'][:10]}..{rows[-1]['ended'][:10]}; ok {ok} ({ok / n:.0%}), "
          f"not ok {n - ok}; outcomes " + ", ".join(f"{o} {sum(r['outcome'] == o for r in rows)}"
                                                   for o in sorted({r['outcome'] for r in rows})))
    known = [r for r in rows if r["cat"] != "unknown"]
    print(f"  category known {len(known)}, tiers {len({r['tierk'] for r in rows})}, categories {len({r['cat'] for r in known})}, "
          f"priced (cost_usd) {sum(r['cost_usd'] is not None for r in rows)}, eta_min {sum(r['eta_min'] is not None for r in rows)}, "
          f"facets {sum(r['spec'] is not None for r in rows)}, arm {sum(bool(r['arm']) for r in rows)}")
    covered = sum(brief_qualified(r) for r in rows)
    print(f"  spawn brief coverage {covered}; retrieval trigger >=300, rerun backtest >500")
    depths = {}
    for r in rows:
        depths[min(r["depth"], 4)] = depths.get(min(r["depth"], 4), 0) + 1
    print("  Follows: depth " + ", ".join(f"{'4+' if d == 4 else d}: {c}" for d, c in sorted(depths.items())))
    cells = {}
    for r in known:
        cells.setdefault((r["cat"], r["tierk"]), []).append(r["ok_final"])
    big = sum(len(v) >= MIN_RUNS for v in cells.values())
    print(f"  (category x tier) cells {len(cells)}, with >= {MIN_RUNS} runs {big}")


def tier_features(cat, facets, tier, diff):
    x = {"bias": 1.0, "t=" + tier: 1.0, "c=" + cat: 1.0, "ct=" + cat + "|" + tier: 1.0}
    for f in FACETS:
        if facets.get(f, "-") != "-":
            x[f"{f}={facets[f]}"] = 1.0
            x[f"{f}={facets[f]}|{tier}"] = 1.0
    if diff is None:
        x["eta=none"] = 1.0
    else:
        x["eta"] = diff - 0.5
        x["eta|" + tier] = diff - 0.5
    return x


def backtest_tier(db_path, rows, seed):
    firsts = [r for r in rows if r["follows"] is None and r["outcome"] != "split"]
    picks = subprocess.run(["north", "defaults-measure", db_path, "--picks"], capture_output=True, text=True, check=True)
    by_agent = {p["agent"]: (p["pick"], p["ok"]) for p in map(json.loads, picks.stdout.splitlines())}
    followed = {}
    for r in rows:
        if r["follows"]:
            followed[r["follows"]] = min(r["ended"], followed.get(r["follows"], r["ended"]))
    eta_hist = {}
    pending = []
    for x in firsts:
        cat = x["cat"]
        at = x["started"] or x["ended"]
        prior_eta = eta_hist.setdefault(cat, [])
        diff = percentile_of(prior_eta, x["eta_min"])
        if x["eta_min"] is not None:
            bisect.insort(prior_eta, x["eta_min"])
        x["eta_pct"] = diff
        if x["agent"] not in by_agent:
            continue
        hist = [r for r in firsts if r["ended"] < at]
        if len(hist) < MIN_TRAIN:
            continue

        def ok_then(r, at=at):
            return succeeded(r, r["agent"] in followed and followed[r["agent"]] < at)

        cat_runs = [recommend_run(r, ok_then(r)) for r in hist if r["cat"] == cat]
        pending.append((x, diff, hist, ok_then, cat_runs))
    picks = recommend_cases([(cat_runs, [x["cat"]] + [None] * len(FACETS)) for x, _, _, _, cat_runs in pending])
    out, lr_model, mf_model, fitted_at = [], None, None, -1
    for (x, diff, hist, ok_then, cat_runs), found in zip(pending, picks):
        cat = x["cat"]
        if lr_model is None or len(hist) - fitted_at >= REFIT_EVERY:
            labelled = [(r, ok_then(r)) for r in hist]
            lr_model = Logistic([(tier_features(r["cat"], r, r["tierk"], r.get("eta_pct")), y) for r, y in labelled], seed)
            mf_model = Factorisation([(r["cat"], r["tierk"], y) for r, y in labelled], seed)
            fitted_at = len(hist)
            train_acc = sum((lr_model.prob(tier_features(r["cat"], r, r["tierk"], r.get("eta_pct"))) >= .5) == y
                            for r, y in labelled) / len(labelled)
        sampled = {st["tier"]: st for st in found["stats"] if st["n"] >= MIN_RUNS}
        cost = {}
        for r in cat_runs:
            if r["cost_usd"] is not None:
                cost.setdefault(r["tier"], []).append(r["cost_usd"])
        asked = dict(zip(GROUPS, x["note"]["cells"]))
        rule_rate = cell_rate(cat_runs, asked, x["tierk"])
        row = {"run": x, "ok": by_agent[x["agent"]][1], "rule_pick": by_agent[x["agent"]][0],
               "rule_pred": rule_rate is not None and rule_rate >= 0.5, "train_acc": train_acc}
        for name, prob in (("lr", lambda t: lr_model.prob(tier_features(cat, x, t, diff))),
                           ("mf", lambda t: mf_model.prob(cat, t))):
            row[name + "_pick"] = model_pick(sampled, cost, prob)
            row[name + "_pred"] = prob(x["tierk"]) >= 0.5
        out.append(row)
    return out


def cell_rate(runs, asked, tier):
    keys = ["category", *(k for k in reversed(BACKOFF) if asked.get(k, "-") != "-")]
    while keys:
        rs = [r for r in runs if r["tier"] == tier and all(r["cells"][GROUPS.index(k)] == asked[k] for k in keys)]
        if len(rs) >= MIN_RUNS or len(keys) == 1:
            return sum(r["ok"] for r in rs) / len(rs) if rs else None
        keys.pop()


def model_pick(sampled, cost, prob):
    if not sampled:
        return DEFAULT_TIER
    p = {t: prob(t) for t in sampled}
    rank = lambda t: sampled[t]["rank"]
    passing = [t for t in sampled if p[t] * 100 >= SUCCESS_THRESHOLD]
    if not passing:
        return max(sampled, key=lambda t: (p[t], -rank(t)))
    if len(passing) > 1 and all(len(cost.get(t, [])) >= USD_MIN for t in passing):
        return min(passing, key=lambda t: (sum(cost[t]) / len(cost[t]) / p[t], rank(t)))
    return min(passing, key=rank)


def retry_features(r):
    x = {"bias": 1.0, "t=" + r["tierk"]: 1.0, "c=" + r["cat"]: 1.0, "o=" + r["outcome"]: 1.0,
         "d=" + str(min(r["depth"], 3)): 1.0, "depth": r["depth"] - 1.5, "arm=" + (r["arm"] or "-"): 1.0}
    for f in FACETS:
        if r[f]:
            x[f"{f}={r[f]}"] = 1.0
    if r["difficulty"] is not None:
        x["difficulty"] = r["difficulty"] - 0.5
    return x


def backtest_retry(rows, children, seed):
    cases = []
    for r in rows:
        kids = sorted(children.get(r["agent"], []), key=lambda k: k["started"] or k["ended"])
        if not r["ok_final"] and kids and kids[0]["tierk"] == r["tierk"]:
            nxt = kids[0]
            cases.append({"run": r, "next": nxt, "label": nxt["outcome"] == "done", "known_at": nxt["ended"]})
    out = []
    lr_model = mf_model = None
    fitted = -1
    for c in cases:
        at = c["run"]["ended"]
        hist = [h for h in cases if h["known_at"] < at]
        rule_retry = c["run"]["depth"] < 2
        row = {"label": c["label"], "rule": rule_retry, "lr": rule_retry, "mf": rule_retry, "trained": len(hist) >= MIN_TRAIN}
        if row["trained"]:
            if lr_model is None or len(hist) - fitted >= 5:
                lr_model = Logistic([(retry_features(h["run"]), h["label"]) for h in hist], seed)
                mf_model = Factorisation([(h["run"]["cat"], f"{h['run']['tierk']}|d{min(h['run']['depth'], 3)}", h["label"])
                                          for h in hist], seed)
                fitted = len(hist)
            r = c["run"]
            row["lr"] = lr_model.prob(retry_features(r)) >= 0.5
            row["mf"] = mf_model.prob(r["cat"], f"{r['tierk']}|d{min(r['depth'], 3)}") >= 0.5
        out.append(row)
    return cases, out


def boot_ci(values_fn, n, boot, rng):
    stats = sorted(values_fn([rng.randrange(n) for _ in range(n)]) for _ in range(boot))
    return stats[int(0.025 * boot)], stats[int(0.975 * boot) - 1]


def tier_metrics(rows, arm):
    def rates(idx, who):
        m = [rows[i] for i in idx if rows[i][who + "_pick"] == rows[i]["run"]["tierk"]]
        if not m:
            return 0.0, 0.0, 0
        return (sum(not r["ok"] for r in m) / len(m), sum(r["run"]["escalation"] for r in m) / len(m), len(m))

    def diff(idx, k):
        return (rates(idx, "rule")[k] - rates(idx, arm)[k]) * 100

    def acc_diff(idx):
        return sum((rows[i][arm + "_pred"] == rows[i]["ok"]) - (rows[i]["rule_pred"] == rows[i]["ok"]) for i in idx) / len(idx) * 100

    return diff, acc_diff, rates


def fmt_ci(v, lo, hi, unit=""):
    return f"{v:+.1f}{unit} [{lo:+.1f}, {hi:+.1f}]"


def brief_qualified(r):
    return r["note"]["brief_covered"] and r.get("brief_origin") in SPAWN_ORIGINS


def brief_vector(text):
    counts = Counter(re.findall(r"[a-z0-9]+", text.lower()))
    norm = math.sqrt(sum(n * n for n in counts.values()))
    return {word: n / norm for word, n in counts.items()} if norm else {}


def neighbor_pick(target, history, k):
    query = brief_vector(target["brief_text"])
    scored = []
    for r in history:
        vector = brief_vector(r["brief_text"])
        similarity = sum(value * vector.get(word, 0) for word, value in query.items())
        if similarity > 0:
            scored.append((similarity, r))
    neighbors = [r for _, r in sorted(scored, key=lambda pair: (-pair[0], pair[1]["ended"], pair[1]["agent"]))[:k]]
    priced = {}
    for r in neighbors:
        if r["cost_usd"] is not None:
            priced.setdefault(r["tier"], []).append(r)
    candidates = {}
    for tier, rs in priced.items():
        wins = sum(r["ok"] for r in rs)
        if wins:
            candidates[tier] = sum(r["cost_usd"] for r in rs) / wins
    rank = {tier: rs[0]["note"]["rank"] for tier, rs in priced.items()}
    return min(candidates, key=lambda t: (candidates[t], rank[t], t)) if candidates else None


def retrieval_decisions(rows, costs):
    rows = annotated(rows)
    finished = [r for r in rows if not unjudged(r) and r.get("ended")]
    firsts = [r for r in finished if r["follows"] is None and r["outcome"] != "split"]
    targets = sorted((r for r in firsts if brief_qualified(r) and r.get("started") and r["note"]["known_tier"]),
                     key=lambda r: (r["started"], r["agent"]))
    histories = []
    for target in targets:
        at = target["started"]
        followed = {r["follows"] for r in rows if r.get("follows") and r.get("ended") and r["ended"] < at}
        available = {}
        for cost in costs:
            if cost.get("ended") and cost["ended"] < at and cost["cost_usd"] > 0:
                available[cost["agent"]] = available.get(cost["agent"], 0) + cost["cost_usd"]
        history = []
        for r in finished:
            if r["ended"] >= at or not r["note"]["known_tier"]:
                continue
            known = dict(r, cost_usd=available.get(r["agent"]))
            if known["expects_landing"] and (not known.get("landed_at") or known["landed_at"] >= at):
                known["landed"] = 0
            known["ok"] = succeeded(known, r["agent"] in followed)
            known["tier"] = r["note"]["tier"]
            history.append(known)
        histories.append(history)
    baselines = [[recommend_run(r, r["ok"]) for r in history if r["follows"] is None and r["outcome"] != "split"]
                 for history in histories]
    picks = recommend_cases([(baseline, target["note"]["cells"]) for baseline, target in zip(baselines, targets)], carry=True)
    out = []
    for target, history, found in zip(targets, histories, picks):
        final_followed = any(r.get("follows") == target["agent"] for r in rows)
        row = {"actual": target["note"]["tier"], "rule": found["tier"],
               "waste": not succeeded(target, final_followed),
               "escalation": target["outcome"] == "escalated" or final_followed}
        neighbors = [r for r in history if brief_qualified(r)]
        for k in (10, 20):
            row[f"k{k}"] = neighbor_pick(target, neighbors, k)
        out.append(row)
    return out


def retrieval_metrics(rows, arm, boot, seed):
    def rates(sample, who):
        matched = [r for r in sample if r[who] is not None and r[who] == r["actual"]]
        return {"matched": len(matched), "abstentions": sum(r[who] is None for r in sample),
                **{metric: sum(r[metric] for r in matched) / len(matched) if matched else None
                   for metric in ("waste", "escalation")}}

    def saving(sample, metric):
        rule, candidate = rates(sample, "rule"), rates(sample, arm)
        return (rule[metric] - candidate[metric]) * 100 if rule[metric] is not None and candidate[metric] is not None else None

    result = {"rule": rates(rows, "rule"), "arm": rates(rows, arm)}
    rng = random.Random(seed)
    draws = {metric: [] for metric in ("waste", "escalation")}
    for _ in range(boot):
        sample = [rows[rng.randrange(len(rows))] for _ in rows]
        for metric in draws:
            draws[metric].append(saving(sample, metric))
    for metric, values in draws.items():
        supported = sorted(v for v in values if v is not None)
        result[metric + "_saving_pp"] = saving(rows, metric)
        result[metric + "_bootstrap_supported"] = len(supported)
        result[metric + "_ci95"] = ([supported[int(.025 * boot)], supported[min(boot - 1, int(.975 * boot))]]
                                      if len(supported) == boot and boot else None)
    ci = result["waste_ci95"]
    result["candidate_for_review"] = ci is not None and ci[0] > 0
    return result


def retrieval_only(db, boot, seed):
    rows = annotated([dict(r) for r in db.execute("SELECT * FROM runs WHERE role='worker'")])
    covered = sum(r["outcome"] != "running" and brief_qualified(r) for r in rows)
    if covered < 300:
        raise SystemExit(f"retrieval requires >=300 flow-qualified finished runs; found {covered}")
    costs = ([dict(r) for r in db.execute("SELECT agent,ended,cost_usd FROM run_costs")]
             if db.execute("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'run_costs'").fetchone() else [])
    decisions = retrieval_decisions(rows, costs)
    result = {"flow_qualified": covered, "finished_workers": sum(r["outcome"] != "running" for r in rows),
              "outcomes": dict(Counter(r["outcome"] for r in rows)),
              "runs_with_aggregate_cost": sum(r["cost_usd"] is not None for r in rows),
              "timestamped_cost_agents": len({r["agent"] for r in costs}),
              "targets": len(decisions), "seed": seed, "bootstrap": boot,
              "arms": {f"k{k}": retrieval_metrics(decisions, f"k{k}", boot, seed) for k in (10, 20)}}
    print(json.dumps(result, indent=2, sort_keys=True))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("db")
    ap.add_argument("--boot", type=int, default=2000)
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--retrieval-only", action="store_true")
    a = ap.parse_args()
    if a.boot < 1:
        ap.error("--boot must be positive")
    db = sqlite3.connect(f"file:{a.db}?mode=ro", uri=True)
    db.row_factory = sqlite3.Row
    if a.retrieval_only:
        retrieval_only(db, a.boot, a.seed)
        db.close()
        return
    rows, _, children = load(db)
    dataset_report(rows)
    rng = random.Random(a.seed)

    tier_rows = backtest_tier(a.db, rows, a.seed)
    n = len(tier_rows)
    idx = list(range(n))
    ok_n = sum(r["ok"] for r in tier_rows)
    print(f"\n(a) tier pick: {n} first-assignment decisions with >= {MIN_TRAIN} earlier runs, ok {ok_n} ({ok_n / n:.0%}); "
          f"refit every {REFIT_EVERY} new runs; last LR train accuracy {tier_rows[-1]['train_acc']:.0%}")
    table = []
    rule_acc = sum(r["rule_pred"] == r["ok"] for r in tier_rows)
    for arm, name in (("lr", "logistic regression"), ("mf", "matrix factorisation")):
        diff, acc_diff, rates = tier_metrics(tier_rows, arm)
        acc = sum(r[arm + "_pred"] == r["ok"] for r in tier_rows)
        agree = sum(r[arm + "_pick"] == r["rule_pick"] for r in tier_rows)
        rw, re_, rm = rates(idx, "rule")
        aw, ae, am = rates(idx, arm)
        print(f"  {name}: pick agrees with recommend {agree}/{n}; matched runs rule {rm}, arm {am}; "
              f"waste rule {rw:.0%} arm {aw:.0%}; escalation rule {re_:.0%} arm {ae:.0%}")
        table.append(("(a) tier", name, n, (acc, n), (rule_acc, n), acc_diff(idx), boot_ci(acc_diff, n, a.boot, rng),
                      diff(idx, 1), boot_ci(lambda s: diff(s, 1), n, a.boot, rng),
                      diff(idx, 0), boot_ci(lambda s: diff(s, 0), n, a.boot, rng), "/100 decisions"))

    cases, retry = backtest_retry(rows, children, a.seed)
    m = len(retry)
    trained = sum(r["trained"] for r in retry)
    pos = sum(r["label"] for r in retry)
    print(f"\n(b) retry vs abort: {m} failed runs with a resolved Follows: run, next attempt ok {pos} ({pos / m:.0%}); "
          f"{trained} with >= {MIN_TRAIN} earlier labelled cases (the rest fall back to the rule); "
          f"rule retries {sum(r['rule'] for r in retry)}")
    rule_correct = sum(r["rule"] == r["label"] for r in retry)
    for arm, name in (("lr", "logistic regression"), ("mf", "matrix factorisation")):
        def esc(s, who):
            return sum((not retry[i][who]) and retry[i]["label"] for i in s)

        def waste(s, who):
            return sum(retry[i][who] and not retry[i]["label"] for i in s)

        def acc_diff(s):
            return sum((retry[i][arm] == retry[i]["label"]) - (retry[i]["rule"] == retry[i]["label"]) for i in s) / len(s) * 100

        ridx = list(range(m))
        acc = sum(r[arm] == r["label"] for r in retry)
        e_saved = lambda s: esc(s, "rule") - esc(s, arm)
        w_saved = lambda s: waste(s, "rule") - waste(s, arm)
        print(f"  {name}: retries {sum(r[arm] for r in retry)}, needless escalations rule {esc(ridx, 'rule')} arm {esc(ridx, arm)}, "
              f"futile retries rule {waste(ridx, 'rule')} arm {waste(ridx, arm)}")
        table.append(("(b) retry", name, m, (acc, m), (rule_correct, m), acc_diff(ridx), boot_ci(acc_diff, m, a.boot, rng),
                      e_saved(ridx), boot_ci(e_saved, m, a.boot, rng), w_saved(ridx), boot_ci(w_saved, m, a.boot, rng), "runs"))

    print("\n| task | arm | n | arm accuracy (Wilson 95%) | rule accuracy (Wilson 95%) | accuracy diff, pts (bootstrap 95%) "
          "| escalations saved (bootstrap 95%) | wasted runs saved (bootstrap 95%) | unit |")
    print("|---|---|---|---|---|---|---|---|---|")
    wins = []
    for task, name, nn, (acc, an), (racc, rn), ad, adci, es, esci, ws, wsci, unit in table:
        alo, ahi = wilson(acc / an, an)
        rlo, rhi = wilson(racc / rn, rn)
        print(f"| {task} | {name} | {nn} | {acc / an:.0%} [{alo:.0%}, {ahi:.0%}] | {racc / rn:.0%} [{rlo:.0%}, {rhi:.0%}] "
              f"| {fmt_ci(ad, *adci)} | {fmt_ci(es, *esci)} | {fmt_ci(ws, *wsci)} | {unit} |")
        if (esci[0] > 0 or wsci[0] > 0) and esci[1] >= 0 and wsci[1] >= 0 and ad >= 0:
            wins.append(f"{task} {name}")
    print("\nverdict: " + ("clearly wins: " + ", ".join(wins) if wins else "tried, no durable gain"))


if __name__ == "__main__":
    main()
