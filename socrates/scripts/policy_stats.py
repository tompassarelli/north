"""Wilson score interval shared by policy-replay.py --sweep and the agent-config-check policy gate."""

from __future__ import annotations

import math

Z95 = 1.959964


def wilson(conformance: float, n: int, z: float = Z95) -> tuple[float, float]:
    if n <= 0:
        return 0.0, 1.0
    p = round(conformance * n) / n
    centre = (p + z * z / (2 * n)) / (1 + z * z / n)
    half = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / (1 + z * z / n)
    return max(0.0, centre - half), min(1.0, centre + half)


def compare(point: tuple[float, int], candidate: tuple[float, int]) -> str:
    lo, hi = wilson(*point)
    c_lo, c_hi = wilson(*candidate)
    if c_hi < lo:
        return "drop"
    if c_lo > hi:
        return "gain"
    return "same"


def flips(point: dict, candidate: dict, trials: int = 3) -> list[str]:
    return sorted(s for s, (passed, n) in candidate.items()
                  if n >= trials and passed == 0 and (top := point.get(s)) and top[1] >= trials and top[0] == top[1])


def operating_point(rows: list[dict]) -> dict:
    scored = [r for r in rows if r["conformance"] is not None and r["n"]]
    if not scored:
        raise SystemExit("policy-replay: no variant produced a scored run")
    best = max(scored, key=lambda r: (r["conformance"], r["n"]))

    def regressed(row: dict) -> bool:
        for scenario, (passed, n) in row.get("scenarios", {}).items():
            top = best.get("scenarios", {}).get(scenario)
            if top and top[1] and n and compare((top[0] / top[1], top[1]), (passed / n, n)) == "drop":
                return True
        return False

    return min((r for r in scored if compare((best["conformance"], best["n"]), (r["conformance"], r["n"])) != "drop"
                and not regressed(r)), key=lambda r: (r["startup_tokens"], -r["conformance"]))
