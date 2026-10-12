#!/usr/bin/env python3
"""Mine 7 days of transcript incident candidates through convo into classifier chunks."""

import argparse
import json
import re
import subprocess
import sys
from collections import Counter
from pathlib import Path

OUT = Path.home() / ".local/state/agents/policy-incidents"

RELAY = re.compile(
    r"^\s*(?:Read /home/|Delegation:|Item:|<task-notification|<command-|<local-command|<bash-|"
    r"<pasted_content|<system-reminder|Caveat:|This session is being continued|"
    r"From \S|Re \S|PEER\b|Claude\b|Codex\b|Plato\b|\S+-lead\b|\S+#\d+|#\d+|"
    r"[\w-]+ \d{1,2}:\d{2}Z|Answer on|Use the Agent tool|Two new|New #|Capacity:|QUIET|"
    r"Wind-down|Needs you|Stop hook|\[Request interrupted|Supervisor|CASE=|Overnight|Research only|"
    r"You own|Add (?:two|one|a) |Tom\b|❯|The \S+ (?:fix|work)\b)",
    re.I,
)
PUSHBACK = re.compile(
    r"(?:^\W*(?:no+|nope|stop|wait)\b|\b(?:wrong|why (?:are|is|did|do|does|would|not|isn'?t|aren'?t)|again\b|"
    r"should(?:n'?t)? have|i said|i told|told you|i asked|asked you|you (?:didn'?t|did not|missed|forgot|keep)|"
    r"broken|still (?:not|broken|failing|seeing|hanging|happening)|doesn'?t work|not working|isn'?t working|"
    r"hate|annoy\w*|frustrat\w*|confus\w*|fuck\w*|shit|wtf|ugh|supposed to|nonsense|waste\w*|"
    r"(?:that|this) is not|not what i|instead of|too (?:slow|long)|stuck|why the hell|stop \w+ing))",
    re.I,
)
TOM_IN_RELAY = re.compile(
    r"\bTom(?:'s)?\b[^.\n]{0,60}\b(?:complain\w*|frustrat\w*|pushback|pushed back|correct\w*|annoyed|unhappy|"
    r"angry|wrong|broken|hates?|not happy|reject\w*|objects?|is upset|said no)\b",
    re.I,
)
BRIEF = re.compile(r"^\s*(?:Item:|CASE=|Read /|Delegation:|Research only|You own|Add )", re.I)
LEAD_CORRECTION = re.compile(
    r"\b(?:wrong|did not stick|you did not|you didn'?t|not what|violat\w*|stop it|that is not|"
    r"undo|revert (?:it|this)|you (?:skipped|ignored|missed))\b",
    re.I,
)
ASSISTANT = [
    ("not-done", re.compile(r"(?m)^\W*Not done\b")),
    ("blocked", re.compile(r"(?m)^\W*Blocked:")),
    ("fix-chain", re.compile(r"(?i)\b(?:two|2) failed fix|second failed fix|Stop: reasoning")),
    ("revert", re.compile(r"(?i)\b(?:I |was |been |auto-)?revert(?:ed|ing)\b")),
    ("admission", re.compile(r"(?i)\b(?:you'?re right|my mistake|I was wrong|I misread|I missed|my fault|apologi[sz]e)\b")),
]


def convo(role, provider, days):
    cmd = ["convo", "-r", role, "-m", "--since", f"{days}d", "-n", "1000000",
           "--json", "--full", "--provider", provider]
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode not in (0, 1):
        sys.exit(f"convo failed ({out.returncode}): {' '.join(cmd)}\n{out.stderr}")
    return json.loads(out.stdout or "[]")


def excerpt(text, limit):
    text = re.sub(r"\s+", " ", text).strip()
    return text if len(text) <= limit else text[: limit - 1] + "…"


def user_candidate(m):
    text = m.get("text") or ""
    head = text.lstrip()[:400]
    relayed = bool(m.get("agent")) or bool(RELAY.match(head))
    if not relayed and m["provider"] == "codex" and len(text) > 600:
        relayed = True
    if not relayed:
        if len(text) > 4000 or not PUSHBACK.search(text):
            return None
        return "tom-typed", "pushback"
    if BRIEF.match(head) or len(text) > 2500:
        return None
    if TOM_IN_RELAY.search(text):
        return "relayed-tom", "pushback"
    if LEAD_CORRECTION.search(text) and not m.get("agent"):
        return "relayed-lead", "correction"
    return None


def assistant_candidate(m):
    text = m.get("text") or ""
    for flag, rx in ASSISTANT:
        if rx.search(text):
            return "assistant", flag
    return None


def row(m, source, flag, limit):
    return {
        "ref": m["id"], "provider": m["provider"], "session": m["session"],
        "project": m.get("project"), "ts": m["ts"],
        "role": m["role"], "source": source, "flag": flag,
        "text": excerpt(m.get("text") or "", limit),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--days", type=int, default=7)
    ap.add_argument("--chunks", type=int, default=6)
    ap.add_argument("--per-thread", type=int, default=3)
    ap.add_argument("--out", type=Path, default=OUT / "chunks")
    a = ap.parse_args()

    rows, seen, per_thread = [], set(), Counter()
    for provider in ("claude", "codex"):
        msgs = [(m, user_candidate(m)) for m in convo("user", provider, a.days)]
        msgs += [(m, assistant_candidate(m)) for m in convo("assistant", provider, a.days)]
        for m, hit in msgs:
            if not hit:
                continue
            source, flag = hit
            r = row(m, source, flag, 360 if source == "tom-typed" else 220)
            key = (source, r["text"][:200].lower())
            thread = (m["session"], m.get("agent"), flag)
            if key in seen or (source == "assistant" and per_thread[thread] >= a.per_thread):
                continue
            seen.add(key)
            per_thread[thread] += 1
            rows.append(r)
    rows.sort(key=lambda r: r["ts"])

    a.out.mkdir(parents=True, exist_ok=True)
    for old in a.out.glob("chunk-*.jsonl"):
        old.unlink()
    lines = [json.dumps(r, ensure_ascii=False) for r in rows]
    total = sum(len(l) for l in lines)
    per = total / a.chunks + 1
    paths, buf, size = [], [], 0
    for line in lines:
        if buf and size + len(line) > per and len(paths) < a.chunks - 1:
            paths.append(buf)
            buf, size = [], 0
        buf.append(line)
        size += len(line)
    if buf:
        paths.append(buf)
    written = []
    for i, chunk in enumerate(paths, 1):
        p = a.out / f"chunk-{i}.jsonl"
        p.write_text("\n".join(chunk) + "\n")
        written.append((p, len(chunk), sum(len(l) for l in chunk) // 4))

    print(f"candidates: {len(rows)}  (~{total // 4} tokens)")
    for k, v in sorted(Counter(r["source"] for r in rows).items()):
        print(f"  source {k}: {v}")
    for k, v in sorted(Counter(r["provider"] for r in rows).items()):
        print(f"  provider {k}: {v}")
    for k, v in sorted(Counter((r["provider"], r["source"], r["flag"]) for r in rows).items()):
        print(f"  {'/'.join(k)}: {v}")
    for p, n, tok in written:
        print(f"chunk {p}: {n} rows, ~{tok} tokens")


if __name__ == "__main__":
    main()
