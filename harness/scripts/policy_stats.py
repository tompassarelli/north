"""Wilson interval, one-sided Fisher exact comparison and the task-cluster paired bootstrap shared by policy-replay.py --sweep, north advisor-report and the agent-config-check policy gate."""

from __future__ import annotations

import math
import random

Z95 = 1.959964
TRIALS = 3


def wilson(conformance: float, n: int, z: float = Z95) -> tuple[float, float]:
    if n <= 0:
        return 0.0, 1.0
    p = round(conformance * n) / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return max(0.0, centre - half), min(1.0, centre + half)


def fisher_tail(a: int, n1: int, c: int, n2: int, upper: bool) -> float:
    k, total = a + c, n1 + n2
    lo, hi = max(0, k - n2), min(k, n1)
    weight = lambda x: math.comb(k, x) * math.comb(total - k, n1 - x)
    tail = range(a, hi + 1) if upper else range(lo, a + 1)
    return sum(weight(x) for x in tail) / math.comb(total, n1)


def compare(point: tuple[float, int], candidate: tuple[float, int], alpha: float = 0.05) -> str:
    (p, n1), (q, n2) = point, candidate
    if n1 <= 0 or n2 <= 0:
        return "same"
    a, c = round(p * n1), round(q * n2)
    if fisher_tail(a, n1, c, n2, upper=True) < alpha:
        return "drop"
    if fisher_tail(a, n1, c, n2, upper=False) < alpha:
        return "gain"
    return "same"


def covered(scenarios: dict, wanted, trials: int = TRIALS) -> int:
    return sum(1 for s in wanted if (scenarios.get(s) or [0, 0])[1] >= trials)


def flips(point: dict, candidate: dict, trials: int = TRIALS) -> list[str]:
    return sorted(s for s, (passed, n) in candidate.items()
                  if n >= trials and passed == 0 and (top := point.get(s)) and top[1] >= trials and top[0] == top[1])


def operating_point(rows: list[dict]) -> dict:
    scored = [r for r in rows if r["conformance"] is not None and r["n"]]
    if not scored:
        raise SystemExit("policy-replay: no variant produced a scored run")
    best = max(scored, key=lambda r: (r["conformance"], r["n"]))

    def regressed(row: dict) -> bool:
        if flips(best.get("scenarios", {}), row.get("scenarios", {})):
            return True
        for scenario, (passed, n) in row.get("scenarios", {}).items():
            top = best.get("scenarios", {}).get(scenario)
            if top and top[1] and n and compare((top[0] / top[1], top[1]), (passed / n, n)) == "drop":
                return True
        return False

    return min((r for r in scored if compare((best["conformance"], best["n"]), (r["conformance"], r["n"])) != "drop"
                and not regressed(r)), key=lambda r: (r["startup_tokens"], -r["conformance"]))


def paired_diff(rows_a: list[dict], rows_b: list[dict], seed: int = 0, b: int = 4000) -> tuple[float, float, float]:
    def means(rows: list[dict]) -> dict[str, float]:
        cells: dict[str, list[bool]] = {}
        for r in rows:
            if r.get("pass") is not None:
                cells.setdefault(r["scenario"], []).append(bool(r["pass"]))
        return {s: sum(v) / len(v) for s, v in cells.items()}

    ma, mb = means(rows_a), means(rows_b)
    diffs = [ma[s] - mb[s] for s in sorted(ma.keys() & mb.keys())]
    if not diffs:
        return 0.0, -1.0, 1.0
    rng = random.Random(seed)
    k = len(diffs)
    boot = sorted(sum(rng.choice(diffs) for _ in range(k)) / k for _ in range(b))
    return sum(diffs) / k, boot[int(0.025 * b)], boot[min(b - 1, int(0.975 * b))]
