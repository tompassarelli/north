import json, os, subprocess


def owner_ids(node):
    ids = {node.get("name"), node.get("conversation")}
    if node.get("provider") and node.get("conversation"):
        ids.add(f"{node['provider']}:{node['conversation']}")
    return {value for value in ids if isinstance(value, str) and value}


def local_alive_owners():
    try:
        nodes = json.loads(subprocess.run([os.environ.get("NORTH_BIN", "north"), "fleet", "list", "--json"],
                                          capture_output=True, text=True, timeout=5, check=True).stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        return set()
    return {owner for node in nodes if node.get("status") != "exited"
            for owner in owner_ids(node)}


def alive_owners():
    try:
        relayed = json.loads(os.environ.get("THREADS_ALIVE_OWNERS", "[]"))
    except ValueError:
        relayed = []
    return local_alive_owners() | {owner for owner in relayed if isinstance(owner, str)}


def held_claims(listing, live):
    return {claim["item"]: claim for claim in listing if claim["standing"] == "live" or claim["owner"] in live}
