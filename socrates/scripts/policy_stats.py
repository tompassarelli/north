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
