import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import org
import placement
import resident_cap
import pace_sampler
import spawn_queue
from defaults import DEFAULTS
from usage import local_account


def field(text, key):
    match = re.search(rf"(?mi)^\s*{key}:\s*(.+)$", text)
    return match.group(1).strip() if match else ""


def main(argv):
    if argv == ["release"]:
        return release()
    parser = argparse.ArgumentParser(prog="agents spawn")
    parser.add_argument("brief", metavar="BRIEF_FILE|ID")
    parser.add_argument("--tier")
    args = parser.parse_args(argv)
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents"
    parent = os.environ.get("AGENT_ORG_NAME") or "proxy"
    with org.Org() as registry:
        up = registry.find(parent)
        if up is None:
            org.die(f"unknown parent {parent!r}")
        up = dict(up)
    session = os.environ.get("CLAUDE_CODE_SESSION_ID") or os.environ.get("CODEX_THREAD_ID") or up.get("session")
    if not session:
        org.die("spawn needs a parent session id")
    saved = state / "spawn"
    by_id = bool(re.fullmatch(r"[0-9a-f]{12}", args.brief))
    source = saved / f"{args.brief}.md" if by_id else Path(args.brief).expanduser()
    contents = source.read_bytes()
    ident = args.brief if by_id else hashlib.sha256(session.encode() + contents).hexdigest()[:12]
    text = contents.decode("utf-8")
    saved.mkdir(parents=True, exist_ok=True)
    if not (saved / f"{ident}.md").exists():
        (saved / f"{ident}.md").write_bytes(contents)
    provider = up.get("provider", "claude")
    job = {"spawn_id": ident, "parent": parent, "session": session,
           "provider": provider, "account": local_account(provider) or up.get("account") or provider,
           "tier": args.tier or field(text, "Tier") or DEFAULTS["default_tier"], "cwd": os.getcwd()}
    job = next((entry for entry in spawn_queue.read(state, placement.now().timestamp())
                if entry.get("spawn_id") == ident), job)
    result = start(state, job)
    if result == 0:
        for entry in spawn_queue.read(state, placement.now().timestamp()):
            if entry.get("spawn_id") == ident:
                spawn_queue.admit(state, entry["lane"], ident)
    return result


def start(state, job, decision=None, unattended=False):
    ident, parent, session = job["spawn_id"], job["parent"], job["session"]
    name = f"spawn-{ident}"
    saved = state / "spawn"
    brief = saved / f"{ident}.md"
    text = brief.read_text()
    with org.Org() as registry:
        existing = registry.find(name)
        if existing and org.health(existing) in (("active", "finished") if unattended else ("active",)):
            print(f"{name} session={existing.get('session') or 'starting'} (already active)")
            return 0
        cfg = placement.config()
        tier = job["tier"]
        if decision is None:
            gate_path = state / "usage-gate.json"
            gate = json.loads(gate_path.read_text()) if gate_path.exists() else {}
            decision = placement.place_spawn(text, job, gate, cfg, placement.now())
        kind = decision["kind"]
        if not unattended and kind in ("native", "launch"):
            lane = decision.get("account") or job["account"]
            if spawn_queue.ahead(state, lane, ident):
                decision = dict(decision, kind="queued", reason="older spawns wait for this account")
                kind = "queued"
        if kind in ("native", "launch") and decision.get("candidates"):
            decision = placement.admit(decision["candidates"], state / "usage-gate.json",
                                       eligible=decision.get("eligible"), session=session)
            kind = decision["kind"]
        if unattended and kind == "native":
            kind = "launch"
        if kind == "native":
            tier = decision.get("tier", tier)
            tiers = cfg.get("tiers", {})
            key = next((k for k, t in tiers.items() if tier in (k, k.split(":", 1)[-1], t.get("agent"))), tier)
            agent = tiers.get(key, {}).get("agent") or key.split(":", 1)[-1]
            print("Agent(" + ", ".join(f"{k}={json.dumps(v)}" for k, v in
                  {"subagent_type": agent, "prompt": text}.items()) + ")")
            return 0
        if kind == "launch" and decision.get("host") not in (None, resident_cap.host()):
            decision = dict(decision, kind="queued", reason=f"host {decision['host']} cannot launch on this machine")
            kind = "queued"
        if kind in ("queued", "refused"):
            if kind == "queued":
                position = spawn_queue.enqueue_spawn(state, job, decision)
                if position:
                    print(spawn_queue.notice(*position, automatic=True), file=sys.stderr)
            print(f"{kind}: {decision['reason']}")
            return 3
        if kind != "launch":
            org.die(f"unknown placement kind {kind!r}")
        item = field(text, "Item") or name
        footer = (f"\n\nWhen finished, report with:\n"
                  f"agents event LAND-READY|STUCK {shlex.quote(item)} --from-node {name} "
                  f"--to-node {shlex.quote(parent)} --body <report>\n"
                  f"Choose LAND-READY or STUCK and supply the report, then run:\n"
                  f"agents org finish {name}\n")
        if not text.endswith(footer):
            with brief.open("a") as target:
                target.write(footer)
        org.resident_hold(str(brief))
        node = org.add(registry, name, {"role": "worker", "budget": 0, "parent": parent,
                       "domain": item.split("#", 1)[0].split()[0], "brief": str(brief),
                       "provider": decision["provider"], "account": decision["account"].split(":", 1)[-1]})
        extra = (["-m", decision["model"], "-c", f"model_reasoning_effort={decision['effort']}"]
                 if node["provider"] == "codex" else [])
        node.update(model=decision["model"], effort=decision["effort"], tier=decision["tier"],
                    cwd=job["cwd"], args=extra, parent_session=session)
        registry.save()
    try:
        session = org.launch(node, extra, node["cwd"], dry=os.environ.get("AGENTS_SPAWN_DRY") == "1")
    except SystemExit:
        org.started(name, None)
    org.started(name, session)
    return 0


def release(wait=5):
    import signal
    import time

    state = pace_sampler.state_dir()
    if not (state / "spawn-queue.json").exists():
        return 0

    def timeout(*_):
        raise TimeoutError("spawn release deadline reached")

    previous = signal.signal(signal.SIGALRM, timeout)
    timer = signal.setitimer(signal.ITIMER_REAL, wait)
    began = time.monotonic()
    try:
        with (state / "spawn-release.lock").open("a") as lock:
            if not pace_sampler.locked(lock, 0):
                return 0
            for job in sorted(spawn_queue.read(state, time.time()), key=lambda e: e["id"]):
                if not job.get("spawn_id"):
                    continue
                with org.Org() as registry:
                    node = registry.find(f"spawn-{job['spawn_id']}")
                    existing = node and org.health(node) in ("active", "finished")
                if existing:
                    spawn_queue.admit(state, job["lane"], job["spawn_id"])
                    continue
                brief = (state / "spawn" / f"{job['spawn_id']}.md").read_text()
                decision = placement.place_spawn(brief, job, pace_sampler.load(state / "usage-gate.json", {}),
                                                 placement.config(), placement.now())
                if decision["kind"] not in ("launch", "native") or decision.get("host") not in (None, resident_cap.host()):
                    continue
                if start(state, job, decision, unattended=True) == 0:
                    spawn_queue.admit(state, job["lane"], job["spawn_id"])
                    return 0
        return 0
    except (TimeoutError, OSError, ValueError, AttributeError) as error:
        print(f"agents spawn release: {error}", file=sys.stderr)
        return 2
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        if timer[0]:
            signal.setitimer(signal.ITIMER_REAL, max(0.001, timer[0] - (time.monotonic() - began)), timer[1])


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (OSError, ValueError, AttributeError) as error:
        print(f"agents spawn: {error}", file=sys.stderr)
        sys.exit(2)
