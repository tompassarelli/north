#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import tempfile
from datetime import datetime, timedelta, timezone

repo = Path(__file__).resolve().parent.parent


def test_recorded_observation_journey(root):
    env = {**os.environ, "PATH": f"{repo / 'bin'}:{os.environ['PATH']}", "THREADS_DB": str(root / "threads.db"),
           "LAPTOP_STATE": str(root / "laptop.json"), "LAPTOP_WINDOW_FILE": str(root / "window.json"), "NORTH_BIN": str((repo / os.environ.get("CARGO_TARGET_DIR", "target")).resolve() / "debug/north")}
    (root / "window.json").write_text(json.dumps({"tz": "UTC", "open": "23:00", "close": "10:00"}))
    start = datetime(2026, 10, 14, 0, 0, tzinfo=timezone.utc)

    def run(minutes, *args):
        result = subprocess.run([str(repo / "bin/laptop"), *args], env={**env, "LAPTOP_NOW": (start + timedelta(minutes=minutes)).isoformat()},
                                text=True, capture_output=True, timeout=15)
        assert result.returncode == 0, result.stderr
        return json.loads((root / "laptop.json").read_text())

    def observe(minutes, gaming=False, idle=None, gpu_busy=None, agent_gpu=False, battery_low=False, sleeping=False):
        return run(minutes, "observe", json.dumps({"gaming": gaming, "idle": idle, "gpu_busy": gpu_busy, "agent_gpu": agent_gpu, "battery_low": battery_low, "sleeping": sleeping}))

    run(0, "open")
    state = observe(0, gaming=True)
    assert state["state"] == "unavailable" and state["reason"] == "gaming" and state["grace"] == 120
    original_since = state["since"]
    assert observe(1, gaming=True)["since"] == original_since
    for minute in range(2, 12):
        assert observe(minute)["state"] == "unavailable"
    assert observe(12)["state"] == "available"
    run(13, "close", "manual")
    run(13, "tick")
    for minute in range(14, 45):
        state = observe(minute, idle=True)
        assert state["state"] == "unavailable" and state["reason"] == "drained (manual)"
    run(45, "open")
    for minute in range(45, 50):
        assert observe(minute, idle=False)["state"] == "available"
    state = observe(50, idle=False)
    assert state["state"] == "draining" and state["reason"] == "active" and state["grace"] == 900
    run(50, "tick")
    for minute in range(51, 81):
        assert observe(minute, idle=True)["state"] == "unavailable"
    assert observe(81, idle=True)["state"] == "available"
    for minute in range(82, 88):
        assert observe(minute, gpu_busy=75, agent_gpu=True)["state"] == "available"
    for minute in range(88, 91):
        assert observe(minute, gpu_busy=75)["state"] == "available"
    assert observe(91, gpu_busy=75)["reason"] == "gaming"
    run(92, "open")
    observe(92, gpu_busy=75, agent_gpu=True)
    observe(93, gpu_busy=75)
    observe(94, gpu_busy=75)
    assert observe(100, gpu_busy=75)["state"] == "available"
    run(600, "tick")
    for minute in range(601, 631):
        assert observe(minute, idle=True)["state"] == "unavailable"
    run(1374, "close", "manual")
    for minute in range(1374, 1380):
        assert observe(minute, idle=False)["state"] == "unavailable"
    assert run(1380, "tick")["state"] == "available"
    for minute in range(1380, 1385):
        assert observe(minute, idle=False)["state"] == "available"
    state = observe(1385, idle=False)
    assert state["state"] == "draining" and state["reason"] == "active" and state["grace"] == 900


with tempfile.TemporaryDirectory(prefix="laptop-auto-test-") as temporary:
    test_recorded_observation_journey(Path(temporary))
print("PASS laptop-auto observation journey")
