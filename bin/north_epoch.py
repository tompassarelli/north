#!/usr/bin/env python3
"""Offline epoch/retention tool for North v0.3 EDN-line logs."""

from __future__ import annotations

import argparse
import collections
import dataclasses
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
from pathlib import Path
from typing import Iterable


TOOL_VERSION = "0.4.0"
STRING_FIELDS = {
    field: re.compile(rf":{field}\s+(\"(?:\\.|[^\"\\])*\")")
    for field in ("op", "l", "p", "r", "ts")
}
TX_FIELD = re.compile(r":tx\s+(\d+)")
SCHEMA_PREDICATES = {"cardinality", "value_kind", "acyclic"}
TEMPORAL_EXACT = {"committed", "updated", "created", "start", "end"}
TERMINAL_VALUES = {"landed", "abandoned", "released", "settled", "closed", "done"}
OPEN_VALUES = {"open", "active", "reserved", "held", "building", "likely-to-land"}


@dataclasses.dataclass(frozen=True, order=True)
class Triple:
    subject: str
    predicate: str
    obj: str


@dataclasses.dataclass
class Event:
    triple: Triple
    tx: int | None
    timestamp: str
    raw: bytes


@dataclasses.dataclass
class FoldedLog:
    path: Path
    source_bytes: int
    source_sha256: str
    boundary_tx: int | None
    boundary_timestamp: str | None
    live: dict[Triple, Event]
    raw_bytes_by_subject: collections.Counter[str]
    lines: int
    malformed_lines: int
    blank_lines: int
    asserts: int
    retracts: int
    duplicate_asserts: int
    matched_retracts: int
    unmatched_retracts: int


@dataclasses.dataclass
class Plan:
    folded: list[FoldedLog]
    values: dict[str, dict[str, set[str]]]
    kinds: dict[str, str]
    fully_retained_subjects: set[str]
    closure_facts: set[Triple]
    closure_subjects: set[str]
    closure_fallback_subjects: set[str]
    closure_unresolved_targets: set[str]
    selected_by_log: list[set[Triple]]
    report: dict


def _json_string_field(line: str, field: str) -> str | None:
    match = STRING_FIELDS[field].search(line)
    if not match:
        return None
    try:
        return json.loads(match.group(1), strict=False)
    except json.JSONDecodeError:
        return None


def parse_event(raw: bytes) -> tuple[str, Triple, str, int | None] | None:
    try:
        line = raw.decode("utf-8")
    except UnicodeDecodeError:
        return None
    op = _json_string_field(line, "op")
    subject = _json_string_field(line, "l")
    predicate = _json_string_field(line, "p")
    obj = _json_string_field(line, "r")
    timestamp = _json_string_field(line, "ts")
    if None in (op, subject, predicate, obj, timestamp):
        return None
    tx_match = TX_FIELD.search(line)
    return op, Triple(subject, predicate, obj), timestamp, (
        int(tx_match.group(1)) if tx_match else None
    )


def fold_log(path: Path) -> FoldedLog:
    path = path.resolve()
    limit = path.stat().st_size
    digest = hashlib.sha256()
    live: dict[Triple, Event] = {}
    raw_bytes_by_subject: collections.Counter[str] = collections.Counter()
    lines = malformed = blank = asserts = retracts = duplicates = 0
    matched = unmatched = 0
    boundary_tx = None
    boundary_timestamp = None
    with path.open("rb") as handle:
        while handle.tell() < limit:
            raw = handle.readline(limit - handle.tell())
            if not raw:
                break
            digest.update(raw)
            lines += 1
            if not raw.strip():
                blank += 1
                continue
            if not raw.endswith(b"\n") and handle.tell() == limit:
                malformed += 1
                continue
            parsed = parse_event(raw)
            if parsed is None:
                malformed += 1
                continue
            op, triple, timestamp, tx = parsed
            raw_bytes_by_subject[triple.subject] += len(raw)
            boundary_timestamp = timestamp
            if tx is not None:
                boundary_tx = tx
            if op == "assert":
                asserts += 1
                if triple in live:
                    duplicates += 1
                live[triple] = Event(triple, tx, timestamp, raw)
            elif op == "retract":
                retracts += 1
                if triple in live:
                    del live[triple]
                    matched += 1
                else:
                    unmatched += 1
            else:
                malformed += 1
    return FoldedLog(
        path=path,
        source_bytes=limit,
        source_sha256=digest.hexdigest(),
        boundary_tx=boundary_tx,
        boundary_timestamp=boundary_timestamp,
        live=live,
        raw_bytes_by_subject=raw_bytes_by_subject,
        lines=lines,
        malformed_lines=malformed,
        blank_lines=blank,
        asserts=asserts,
        retracts=retracts,
        duplicate_asserts=duplicates,
        matched_retracts=matched,
        unmatched_retracts=unmatched,
    )


def parse_as_of(value: str) -> dt.datetime:
    if re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
        return dt.datetime.combine(
            dt.date.fromisoformat(value), dt.time.max, tzinfo=dt.timezone.utc
        )
    normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
    parsed = dt.datetime.fromisoformat(normalized)
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt.timezone.utc)
    return parsed.astimezone(dt.timezone.utc)


def parse_timestamp(value: str) -> dt.datetime | None:
    try:
        if re.fullmatch(r"\d{4}-\d{2}-\d{2}", value):
            return dt.datetime.combine(
                dt.date.fromisoformat(value), dt.time.min, tzinfo=dt.timezone.utc
            )
        normalized = value[:-1] + "+00:00" if value.endswith("Z") else value
        parsed = dt.datetime.fromisoformat(normalized)
        if parsed.tzinfo is None:
            parsed = parsed.replace(tzinfo=dt.timezone.utc)
        return parsed.astimezone(dt.timezone.utc)
    except ValueError:
        return None


def load_policy(path: Path) -> dict:
    with path.open(encoding="utf-8") as handle:
        policy = json.load(handle)
    required = {
        "version",
        "ttl_days",
        "retain_kinds",
        "retain_kind_prefixes",
        "ephemeral_kinds",
        "ephemeral_kind_prefixes",
        "drop_subject_prefixes",
        "identity_predicates",
        "identity_fallback_predicates",
        "registry_predicates",
    }
    missing = sorted(required - policy.keys())
    if missing:
        raise ValueError(f"policy missing keys: {', '.join(missing)}")
    return policy


def cursor_repair_plan(paths: Iterable[Path], policy: dict) -> list[dict[str, str]]:
    """Enumerate every persisted cursor whose file identity a cut replaces."""
    by_name = {path.name: path.resolve() for path in paths}
    repairs = []
    for cursor in policy.get("log_identity_cursors", []):
        log = by_name.get(cursor["log"])
        if log is None:
            continue
        stat = log.stat()
        repairs.append({"cursor": cursor["cursor"], "log": str(log),
                        "file_identity_before": f"{stat.st_dev}:{stat.st_ino}",
                        "repair": cursor["repair"]})
    return repairs


def subject_values(folded: Iterable[FoldedLog]) -> dict[str, dict[str, set[str]]]:
    values: dict[str, dict[str, set[str]]] = collections.defaultdict(
        lambda: collections.defaultdict(set)
    )
    for log in folded:
        for triple in log.live:
            values[triple.subject][triple.predicate].add(triple.obj)
    return values


def prefix_kind(subject: str) -> str | None:
    token = subject[1:] if subject.startswith("@") else subject
    rules = (
        ("concern-", "concern"),
        ("agent:", "agent"),
        ("msg:", "message"),
        ("message:", "message"),
        ("topic-", "topic"),
        ("run-event:", "run_event"),
        ("run_event:", "run_event"),
        ("run:", "run"),
        ("run-", "run"),
        ("run/", "run"),
        ("session:", "session"),
        ("session-", "session"),
        ("sess:", "session"),
        ("sess-", "session"),
        ("lane:", "lane"),
        ("notification:", "notification"),
        ("worktree-allocation:", "worktree_allocation"),
        ("worktree-reservation:", "worktree_reservation"),
    )
    for prefix, kind in rules:
        if token.startswith(prefix):
            return kind
    return None


def classify_subjects(values: dict[str, dict[str, set[str]]]) -> dict[str, str]:
    kinds = {}
    for subject, predicates in values.items():
        explicit = sorted(predicates.get("kind", ()))
        if explicit:
            kind = explicit[-1]
        else:
            kind = prefix_kind(subject)
            if kind is None and predicates.get("title"):
                kind = "thread"
            elif kind is None and SCHEMA_PREDICATES.intersection(predicates):
                kind = "predicate"
            elif kind is None:
                kind = "other"
        kinds[subject] = kind
    return kinds


def is_terminal_concern(predicates: dict[str, set[str]]) -> bool:
    if predicates.get("outcome") or predicates.get("abandoned"):
        return True
    for predicate in ("reached", "state", "status"):
        if {value.lower() for value in predicates.get(predicate, ())} & TERMINAL_VALUES:
            return True
    return False


def is_nonterminal_thread(
    subject: str, values: dict[str, dict[str, set[str]]], kinds: dict[str, str]
) -> bool:
    predicates = values[subject]
    return (
        kinds.get(subject) == "thread"
        and not predicates.get("outcome")
        and not predicates.get("abandoned")
    )


def subject_activity(predicates: dict[str, set[str]]) -> dt.datetime | None:
    candidates = []
    for predicate, objects in predicates.items():
        temporal = predicate in TEMPORAL_EXACT or predicate.endswith(
            ("_at", "_on", "_time", "_date", "_until")
        )
        if not temporal:
            continue
        for obj in objects:
            parsed = parse_timestamp(obj)
            if parsed is not None:
                candidates.append(parsed)
    return max(candidates) if candidates else None


def has_open_reservation(
    subject: str,
    predicates: dict[str, set[str]],
    kind: str,
    as_of: dt.datetime,
) -> bool:
    if "reservation" not in kind and "reservation" not in subject:
        return False
    if predicates.get("released_at") or predicates.get("settled_at"):
        return False
    for predicate in ("reached", "state", "status"):
        lowered = {value.lower() for value in predicates.get(predicate, ())}
        if lowered & TERMINAL_VALUES:
            return False
        if lowered & OPEN_VALUES:
            return True
    for predicate, objects in predicates.items():
        if "lease" not in predicate:
            continue
        for obj in objects:
            try:
                payload = json.loads(obj)
            except (json.JSONDecodeError, TypeError):
                continue
            expires = payload.get("expiresAt") or payload.get("expires_at")
            parsed = parse_timestamp(str(expires)) if expires else None
            if parsed is not None and parsed > as_of:
                return True
    return False


def matches_kind(kind: str, exact: Iterable[str], prefixes: Iterable[str]) -> bool:
    return kind in set(exact) or any(kind.startswith(prefix) for prefix in prefixes)


def choose_fully_retained(
    values: dict[str, dict[str, set[str]]],
    kinds: dict[str, str],
    policy: dict,
    as_of: dt.datetime,
) -> tuple[set[str], dict[str, str]]:
    live_threads = {
        subject
        for subject in values
        if is_nonterminal_thread(subject, values, kinds)
    }
    retained = set()
    reasons = {}
    cutoff = as_of - dt.timedelta(days=int(policy["ttl_days"]))
    for subject, predicates in values.items():
        kind = kinds[subject]
        refs = {obj for objects in predicates.values() for obj in objects if obj.startswith("@")}
        override = bool(predicates.get("driver")) or has_open_reservation(
            subject, predicates, kind, as_of
        )
        if subject.startswith("@") and set(policy["registry_predicates"]).intersection(predicates):
            # The executable predicate registry prevents Fram from inferring refs.
            keep, reason = True, "predicate_registry"
        elif override:
            keep, reason = True, "driver_or_open_reservation"
        elif any(subject.startswith(prefix) for prefix in policy["drop_subject_prefixes"]):
            keep, reason = False, "operational_subject_prefix"
        elif kind == "concern":
            keep = not is_terminal_concern(predicates)
            reason = "live_concern" if keep else "terminal_concern"
        elif matches_kind(kind, policy["retain_kinds"], policy["retain_kind_prefixes"]):
            keep, reason = True, "durable_kind"
        elif matches_kind(
            kind, policy["ephemeral_kinds"], policy["ephemeral_kind_prefixes"]
        ):
            touches_thread = bool(refs & live_threads)
            activity = subject_activity(predicates)
            recent = activity is not None and activity > cutoff
            keep = touches_thread or recent
            if touches_thread:
                reason = "touches_live_thread"
            elif recent:
                reason = "newer_than_ttl"
            else:
                reason = "expired_or_undated_ephemeral"
        else:
            keep, reason = True, "unclassified_conservative"
        reasons[subject] = reason
        if keep:
            retained.add(subject)
    return retained, reasons


def compute_closure(
    all_live: set[Triple],
    all_subjects: set[str],
    fully_retained: set[str],
    identity_predicates: set[str],
    fallback_predicates: list[str],
) -> tuple[set[Triple], set[str], set[str], set[str]]:
    facts_by_subject: dict[str, list[Triple]] = collections.defaultdict(list)
    for fact in all_live:
        facts_by_subject[fact.subject].append(fact)
    retained_facts = {fact for fact in all_live if fact.subject in fully_retained}
    closure_facts = set()
    fallback_subjects = set()
    fallback_rank = {predicate: rank for rank, predicate in enumerate(fallback_predicates)}
    closure_subjects = set()
    while True:
        referenced = {
            fact.obj
            for fact in retained_facts | closure_facts
            if fact.obj.startswith("@")
            and fact.obj in all_subjects
            and fact.obj not in fully_retained
        }
        pending = referenced - closure_subjects
        if not pending:
            break
        for subject in sorted(pending):
            facts = facts_by_subject[subject]
            preferred = {fact for fact in facts if fact.predicate in identity_predicates}
            if preferred:
                closure_facts.update(preferred)
            else:
                fallback_subjects.add(subject)
                ranked = [fact for fact in facts if fact.predicate in fallback_rank]
                if ranked:
                    closure_facts.add(
                        min(ranked, key=lambda fact: (fallback_rank[fact.predicate], fact))
                    )
                elif facts:
                    closure_facts.add(min(facts))
            closure_subjects.add(subject)
    selected_subjects = fully_retained | {fact.subject for fact in closure_facts}
    unresolved = {
        fact.obj
        for fact in retained_facts | closure_facts
        if fact.obj.startswith("@")
        and fact.obj in all_subjects
        and fact.obj not in selected_subjects
    }
    return closure_facts, closure_subjects, fallback_subjects, unresolved


def _kind_counter_template() -> dict:
    return {
        "retained_subjects": set(),
        "dropped_subjects": set(),
        "closure_subjects": set(),
        "retained_triples": 0,
        "dropped_triples": 0,
        "retained_live_bytes": 0,
        "dropped_live_bytes": 0,
        "raw_event_bytes": 0,
    }


def _serialize_kind_counts(counts: dict[str, dict]) -> dict:
    result = {}
    for kind in sorted(counts):
        item = counts[kind]
        result[kind] = {
            key: len(value) if isinstance(value, set) else value
            for key, value in item.items()
        }
    return result


def build_plan(paths: list[Path], policy: dict, as_of: dt.datetime) -> Plan:
    folded = [fold_log(path) for path in paths]
    values = subject_values(folded)
    kinds = classify_subjects(values)
    fully_retained, reasons = choose_fully_retained(values, kinds, policy, as_of)
    all_live = {triple for log in folded for triple in log.live}
    all_subjects = set(values)
    (
        closure_facts,
        closure_subjects,
        closure_fallback_subjects,
        closure_unresolved_targets,
    ) = compute_closure(
        all_live,
        all_subjects,
        fully_retained,
        set(policy["identity_predicates"]),
        list(policy["identity_fallback_predicates"]),
    )
    selected_by_log = []
    input_reports = []
    physical_selected_triples = physical_selected_bytes = 0
    physical_dropped_triples = physical_dropped_bytes = 0
    for log in folded:
        selected = {
            triple
            for triple in log.live
            if triple.subject in fully_retained or triple in closure_facts
        }
        selected_by_log.append(selected)
        counts = collections.defaultdict(_kind_counter_template)
        for subject, byte_count in log.raw_bytes_by_subject.items():
            counts[kinds.get(subject, "other")]["raw_event_bytes"] += byte_count
        for triple, event in log.live.items():
            kind = kinds.get(triple.subject, "other")
            if triple in selected:
                counts[kind]["retained_triples"] += 1
                counts[kind]["retained_live_bytes"] += len(event.raw)
                if triple.subject in closure_subjects and triple.subject not in fully_retained:
                    counts[kind]["closure_subjects"].add(triple.subject)
                else:
                    counts[kind]["retained_subjects"].add(triple.subject)
                physical_selected_triples += 1
                physical_selected_bytes += len(event.raw)
            else:
                counts[kind]["dropped_triples"] += 1
                counts[kind]["dropped_live_bytes"] += len(event.raw)
                counts[kind]["dropped_subjects"].add(triple.subject)
                physical_dropped_triples += 1
                physical_dropped_bytes += len(event.raw)
        input_reports.append(
            {
                "source": str(log.path),
                "source_identity_sha256": log.source_sha256,
                "source_bytes": log.source_bytes,
                "boundary_tx": log.boundary_tx,
                "boundary_timestamp": log.boundary_timestamp,
                "lines": log.lines,
                "asserts": log.asserts,
                "retracts": log.retracts,
                "duplicate_asserts": log.duplicate_asserts,
                "matched_retracts": log.matched_retracts,
                "unmatched_retracts": log.unmatched_retracts,
                "malformed_lines": log.malformed_lines,
                "blank_lines": log.blank_lines,
                "by_kind": _serialize_kind_counts(counts),
            }
        )
    output_union = {
        triple
        for log, selected in zip(folded, selected_by_log)
        for triple in selected
        if triple in log.live
    }
    full_union = {triple for triple in all_live if triple.subject in fully_retained}
    work_graph_subjects = {
        subject
        for subject in fully_retained
        if not matches_kind(
            kinds[subject],
            policy["ephemeral_kinds"],
            policy["ephemeral_kind_prefixes"],
        )
        and not any(
            subject.startswith(prefix) for prefix in policy["drop_subject_prefixes"]
        )
    }
    work_graph_union = {
        triple for triple in all_live if triple.subject in work_graph_subjects
    }
    work_graph_physical_bytes = sum(
        len(event.raw)
        for log in folded
        for triple, event in log.live.items()
        if triple.subject in work_graph_subjects
    )
    dropped_counts = collections.Counter()
    dropped_bytes = collections.Counter()
    for log in folded:
        for triple, event in log.live.items():
            if triple.subject not in fully_retained:
                dropped_counts[triple.subject] += 1
                dropped_bytes[triple.subject] += len(event.raw)
    top_affected = [
        {
            "subject": subject,
            "kind": kinds.get(subject, "other"),
            "reason": reasons.get(subject),
            "dropped_live_triples": count,
            "dropped_live_bytes": dropped_bytes[subject],
            "closure_identity_pulled_back": subject in closure_subjects,
        }
        for subject, count in sorted(
            dropped_counts.items(), key=lambda item: (-item[1], item[0])
        )[:20]
    ]
    report = {
        "tool": "north-epoch",
        "tool_version": TOOL_VERSION,
        "mode": "dry-run",
        "policy_version": policy["version"],
        "as_of": as_of.isoformat(),
        "ttl_cutoff": (as_of - dt.timedelta(days=int(policy["ttl_days"]))).isoformat(),
        "inputs": input_reports,
        "totals": {
            "source_bytes": sum(log.source_bytes for log in folded),
            "source_live_triples_setwise": len(all_live),
            "source_live_subjects": len(all_subjects),
            "fully_retained_subjects": len(fully_retained),
            "fully_retained_triples_setwise": len(full_union),
            "closure_subjects_pulled_back": len(closure_subjects),
            "closure_identity_triples_pulled_back": len(closure_facts),
            "closure_fallback_subjects": len(closure_fallback_subjects),
            "closure_unresolved_targets": len(closure_unresolved_targets),
            "work_graph_subjects": len(work_graph_subjects),
            "work_graph_triples_setwise": len(work_graph_union),
            "work_graph_live_bytes_physical": work_graph_physical_bytes,
            "output_live_triples_setwise": len(output_union),
            "output_live_triples_physical": physical_selected_triples,
            "output_live_bytes_physical": physical_selected_bytes,
            "dropped_live_triples_physical": physical_dropped_triples,
            "dropped_live_bytes_physical": physical_dropped_bytes,
        },
        "closure_fallback_subjects": sorted(closure_fallback_subjects),
        "closure_unresolved_targets": sorted(closure_unresolved_targets),
        "cursor_repairs_required": cursor_repair_plan(paths, policy),
        "top_subjects_affected": top_affected,
    }
    return Plan(
        folded=folded,
        values=values,
        kinds=kinds,
        fully_retained_subjects=fully_retained,
        closure_facts=closure_facts,
        closure_subjects=closure_subjects,
        closure_fallback_subjects=closure_fallback_subjects,
        closure_unresolved_targets=closure_unresolved_targets,
        selected_by_log=selected_by_log,
        report=report,
    )


def epoch_header(log: FoldedLog, as_of: dt.datetime, policy: dict) -> bytes:
    identity = log.source_sha256[:20]
    provenance = {
        "boundary_tx": log.boundary_tx,
        "epoch_cut_time": as_of.isoformat(),
        "policy_version": policy["version"],
        "source_log_identity_sha256": log.source_sha256,
        "source_log_name": log.path.name,
        "tool_version": TOOL_VERSION,
    }
    return (
        "{:tx 0, :op \"assert\", :l "
        + json.dumps(f"@epoch:{identity}")
        + ", :p \"epoch_header\", :r "
        + json.dumps(json.dumps(provenance, sort_keys=True, separators=(",", ":")))
        + ", :frame \"epoch\", :ts "
        + json.dumps(as_of.isoformat().replace("+00:00", "Z"))
        + "}\n"
    ).encode("utf-8")


def atomic_write_bytes(path: Path, chunks: Iterable[bytes]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            for chunk in chunks:
                handle.write(chunk)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def atomic_write_json(path: Path, payload: dict) -> None:
    atomic_write_bytes(
        path, [json.dumps(payload, indent=2, sort_keys=True).encode("utf-8"), b"\n"]
    )


def execute_plan(
    plan: Plan,
    policy: dict,
    as_of: dt.datetime,
    output_dir: Path,
    archive_dir: Path,
) -> dict:
    output_dir = output_dir.resolve()
    archive_dir = archive_dir.resolve()
    if output_dir == archive_dir:
        raise ValueError("output and archive directories must differ")
    basenames = [log.path.name for log in plan.folded]
    if len(basenames) != len(set(basenames)):
        raise ValueError("input basenames must be unique")
    entries = []
    targets = []
    for log in plan.folded:
        epoch_path = output_dir / f"{log.path.stem}.epoch{log.path.suffix or '.log'}"
        cold_path = archive_dir / log.path.name
        targets.extend((epoch_path, cold_path))
        entries.append((log, epoch_path, cold_path))
    index_path = archive_dir / "epoch-index.json"
    targets.append(index_path)
    existing = [str(path) for path in targets if path.exists()]
    if existing:
        raise FileExistsError("refusing to overwrite: " + ", ".join(existing))
    if any(log.path in targets for log in plan.folded):
        raise ValueError("an input path collides with an output/archive target")
    output_dir.mkdir(parents=True, exist_ok=True)
    archive_dir.mkdir(parents=True, exist_ok=True)
    index_inputs = []
    for (log, epoch_path, cold_path), selected, input_report in zip(
        entries, plan.selected_by_log, plan.report["inputs"]
    ):
        ordered = sorted(selected)
        header = epoch_header(log, as_of, policy)
        atomic_write_bytes(epoch_path, [header, *(log.live[fact].raw for fact in ordered)])
        index_input = dict(input_report)
        index_input.update(
            {
                "epoch_path": str(epoch_path),
                "cold_path": str(cold_path),
                "epoch_data_triples": len(ordered),
                "epoch_data_bytes": sum(len(log.live[fact].raw) for fact in ordered),
                "epoch_header_bytes": len(header),
            }
        )
        index_inputs.append(index_input)
    moved = []
    try:
        for log, _epoch_path, cold_path in entries:
            shutil.move(str(log.path), str(cold_path))
            moved.append((cold_path, log.path))
    except BaseException:
        for cold_path, original_path in reversed(moved):
            if cold_path.exists() and not original_path.exists():
                shutil.move(str(cold_path), str(original_path))
        raise
    index = {
        "tool": "north-epoch",
        "tool_version": TOOL_VERSION,
        "created_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "as_of": as_of.isoformat(),
        "policy": policy,
        "inputs": index_inputs,
        "totals": plan.report["totals"],
        "top_subjects_affected": plan.report["top_subjects_affected"],
        "closure_fallback_subjects": plan.report[
            "closure_fallback_subjects"
        ],
        "closure_unresolved_targets": plan.report[
            "closure_unresolved_targets"
        ],
    }
    atomic_write_json(index_path, index)
    result = dict(plan.report)
    result["mode"] = "execute"
    result["index_path"] = str(index_path)
    result["inputs"] = index_inputs
    return result


def verify_index(index_path: Path) -> dict:
    with index_path.open(encoding="utf-8") as handle:
        index = json.load(handle)
    policy = index["policy"]
    as_of = parse_as_of(index["as_of"])
    cold_paths = [Path(item["cold_path"]) for item in index["inputs"]]
    rebuilt = build_plan(cold_paths, policy, as_of)
    input_results = []
    all_actual = set()
    for item, expected_log, selected in zip(
        index["inputs"], rebuilt.folded, rebuilt.selected_by_log
    ):
        epoch_path = Path(item["epoch_path"])
        epoch_fold = fold_log(epoch_path)
        epoch_data = {
            fact for fact in epoch_fold.live if fact.predicate != "epoch_header"
        }
        all_actual.update(epoch_data)
        expected = set(selected)
        current_hash = expected_log.source_sha256
        hash_ok = current_hash == item["source_identity_sha256"]
        missing = expected - epoch_data
        extra = epoch_data - expected
        input_results.append(
            {
                "cold_path": str(expected_log.path),
                "epoch_path": str(epoch_path),
                "cold_sha256": current_hash,
                "archive_hash_matches_source": hash_ok,
                "expected_data_triples": len(expected),
                "actual_data_triples": len(epoch_data),
                "missing_triples": len(missing),
                "extra_triples": len(extra),
                "pass": hash_ok and not missing and not extra,
            }
        )
    original_live = {fact for log in rebuilt.folded for fact in log.live}
    retained_original = {
        fact
        for fact in original_live
        if fact.subject in rebuilt.fully_retained_subjects
    }
    retained_epoch = {
        fact
        for fact in all_actual
        if fact.subject in rebuilt.fully_retained_subjects
    }
    retained_missing = retained_original - retained_epoch
    retained_extra = retained_epoch - retained_original
    passed = (
        all(item["pass"] for item in input_results)
        and not retained_missing
        and not retained_extra
        and not rebuilt.closure_unresolved_targets
    )
    return {
        "tool": "north-epoch",
        "tool_version": TOOL_VERSION,
        "verification": "exact retained-subject and closure set diff",
        "index_path": str(index_path.resolve()),
        "inputs": input_results,
        "fully_retained_subjects": len(rebuilt.fully_retained_subjects),
        "retained_original_triples": len(retained_original),
        "retained_epoch_triples": len(retained_epoch),
        "retained_missing_triples": len(retained_missing),
        "retained_extra_triples": len(retained_extra),
        "closure_subjects": len(rebuilt.closure_subjects),
        "closure_identity_triples": len(rebuilt.closure_facts),
        "closure_unresolved_targets": len(rebuilt.closure_unresolved_targets),
        "pass": passed,
    }


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(
        prog="north-epoch",
        description="Offline retention/epoch tool for North v0.3 EDN-line logs",
    )
    result.add_argument("logs", nargs="*", type=Path)
    result.add_argument("--as-of", help="ISO date/time; date-only means end of UTC day")
    result.add_argument(
        "--policy", type=Path, default=Path(__file__).parents[1] / "policy" / "epoch-retention-v2.json"
    )
    result.add_argument("--execute", action="store_true", help="write epochs and move input copies")
    result.add_argument("--output-dir", type=Path)
    result.add_argument("--archive-dir", type=Path)
    result.add_argument("--report", type=Path, help="write the JSON report (the only dry-run write)")
    result.add_argument("--verify-index", type=Path, help="mechanically verify an executed epoch index")
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.verify_index:
            if args.logs or args.execute or args.as_of:
                raise ValueError("--verify-index cannot be combined with logs, --execute, or --as-of")
            report = verify_index(args.verify_index.resolve())
            if args.report:
                atomic_write_json(args.report.resolve(), report)
            print(json.dumps(report, indent=2, sort_keys=True))
            return 0 if report["pass"] else 1
        if not args.logs:
            raise ValueError("at least one input log is required")
        if not args.as_of:
            raise ValueError("--as-of is required")
        if args.execute and (args.output_dir is None or args.archive_dir is None):
            raise ValueError("--execute requires --output-dir and --archive-dir")
        paths = [path.resolve() for path in args.logs]
        if len(paths) != len(set(paths)):
            raise ValueError("input paths must be unique")
        for path in paths:
            if not path.is_file():
                raise FileNotFoundError(path)
        policy = load_policy(args.policy.resolve())
        as_of = parse_as_of(args.as_of)
        plan = build_plan(paths, policy, as_of)
        report = (
            execute_plan(
                plan,
                policy,
                as_of,
                args.output_dir,
                args.archive_dir,
            )
            if args.execute
            else plan.report
        )
        if args.report:
            atomic_write_json(args.report.resolve(), report)
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0
    except (OSError, ValueError, KeyError, json.JSONDecodeError) as error:
        print(f"north-epoch: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
