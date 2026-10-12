import json
import os
from pathlib import Path

from org import health, org_file


def owner_ids(node):
    ids = {node.get("id"), node.get("session")}
    if node.get("provider") and node.get("session"):
        ids.add(f"{node['provider']}:{node['session']}")
    return {value for value in ids if isinstance(value, str) and value}


def local_alive_owners():
    try:
        nodes = json.loads(Path(org_file()).read_text()).get("nodes", [])
    except (OSError, ValueError):
        return set()
    return {owner for node in nodes if node.get("state") == "active" and health(node) == "active"
            for owner in owner_ids(node)}


def alive_owners():
    try:
        relayed = json.loads(os.environ.get("THREADS_ALIVE_OWNERS", "[]"))
    except ValueError:
        relayed = []
    return local_alive_owners() | {owner for owner in relayed if isinstance(owner, str)}


def held_claim(claim, live, elapsed):
    return (claim["owner"] in live or not claim["eta_min"] or elapsed <= 2 * claim["eta_min"])
