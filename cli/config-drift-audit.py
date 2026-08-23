#!/usr/bin/env python3
"""Read-only effective shared/Codex skill and MCP configuration audit."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from typing import Any


HOME = Path(os.environ.get("HOME", "")).expanduser()
SHARED_SKILLS = Path(
    os.environ.get("NORTH_AGENT_SKILLS", HOME / ".local/state/north/agents/current/skills/shared")
)
CODEX_HOME = Path(os.environ.get("CODEX_HOME", HOME / ".codex"))
CODEX_CONFIG = Path(
    os.environ.get("NORTH_CODEX_CONFIG", CODEX_HOME / "config.toml")
)
CODEX_SYSTEM_SKILLS = Path(
    os.environ.get("NORTH_CODEX_SYSTEM_SKILLS", CODEX_HOME / "skills/.system")
)


def source_record(provider: str, path: Path, state: str) -> dict[str, str]:
    return {"provider": provider, "path": str(path), "state": state}


def read_json(path: Path, provider: str, diagnostics: list[dict[str, str]]) -> tuple[str, Any]:
    try:
        with path.open(encoding="utf-8") as stream:
            return "ok", json.load(stream)
    except FileNotFoundError:
        diagnostics.append(
            {"provider": provider, "source": str(path), "kind": "absent", "message": "source is absent"}
        )
        return "absent", None
    except (OSError, UnicodeError, json.JSONDecodeError):
        diagnostics.append(
            {"provider": provider, "source": str(path), "kind": "malformed", "message": "source is unreadable or malformed JSON"}
        )
        return "malformed", None


def read_toml(path: Path, provider: str, diagnostics: list[dict[str, str]]) -> tuple[str, Any]:
    try:
        with path.open("rb") as stream:
            return "ok", tomllib.load(stream)
    except FileNotFoundError:
        diagnostics.append(
            {"provider": provider, "source": str(path), "kind": "absent", "message": "source is absent"}
        )
        return "absent", None
    except (OSError, tomllib.TOMLDecodeError):
        diagnostics.append(
            {"provider": provider, "source": str(path), "kind": "malformed", "message": "source is unreadable or malformed TOML"}
        )
        return "malformed", None


def canonical_directory(
    path: Path,
    provider: str,
    source: str,
    diagnostics: list[dict[str, str]],
    *,
    within: Path | None = None,
) -> Path | None:
    try:
        canonical = path.resolve(strict=True)
        if not canonical.is_dir():
            raise NotADirectoryError
        if within is not None:
            root = within.resolve(strict=True)
            canonical.relative_to(root)
        return canonical
    except (OSError, RuntimeError, ValueError):
        diagnostics.append(
            {
                "provider": provider,
                "source": source,
                "kind": "containment",
                "message": f"skill root is missing, unreadable, or escapes its plugin root: {path}",
            }
        )
        return None


def manifest_at(root: Path, provider: str, diagnostics: list[dict[str, str]]) -> dict[str, Any] | None:
    names = (".codex-plugin/plugin.json", "plugin.json")
    manifest_path = next((root / name for name in names if (root / name).is_file()), root / names[0])
    try:
        manifest_path.resolve(strict=True).relative_to(root.resolve(strict=True))
    except (OSError, RuntimeError, ValueError):
        diagnostics.append(
            {"provider": provider, "source": str(manifest_path), "kind": "containment", "message": "plugin manifest is unreadable or escapes its plugin root"}
        )
        return None
    state, manifest = read_json(manifest_path, provider, diagnostics)
    if state != "ok" or not isinstance(manifest, dict):
        if state == "ok":
            diagnostics.append(
                {"provider": provider, "source": str(manifest_path), "kind": "malformed", "message": "plugin manifest must be a JSON object"}
            )
        return None
    if not isinstance(manifest.get("name"), str) or not manifest["name"].strip():
        diagnostics.append(
            {"provider": provider, "source": str(manifest_path), "kind": "malformed", "message": "plugin manifest must declare a nonempty name"}
        )
        return None
    return manifest


def skill_directories(
    root: Path,
    provider: str,
    plugin: str,
    scope: str,
    diagnostics: list[dict[str, str]],
    *,
    contain_in: Path | None,
    include_self: bool = False,
) -> list[dict[str, Any]]:
    canonical_root = canonical_directory(
        root, provider, plugin, diagnostics, within=contain_in
    )
    if canonical_root is None:
        return []
    rows: list[dict[str, Any]] = []
    try:
        children = sorted(canonical_root.iterdir(), key=lambda path: path.name)
    except OSError:
        diagnostics.append(
            {"provider": provider, "source": str(root), "kind": "unreadable", "message": "cannot enumerate skill root"}
        )
        return []
    candidates = ([canonical_root] if include_self else []) + children
    for child in candidates:
        try:
            canonical = child.resolve(strict=True)
            if contain_in is not None:
                containment_root = contain_in.resolve(strict=True)
                canonical.relative_to(containment_root)
            if not canonical.is_dir():
                continue
            skill_candidate = canonical / "SKILL.md"
            if not skill_candidate.exists() and not skill_candidate.is_symlink():
                continue
            skill_file = skill_candidate.resolve(strict=True)
            if contain_in is not None:
                skill_file.relative_to(containment_root)
            if not skill_file.is_file():
                continue
        except (OSError, RuntimeError, ValueError):
            diagnostics.append(
                {"provider": provider, "source": str(child), "kind": "containment", "message": "skill entry is unreadable or escapes its plugin root"}
            )
            continue
        rows.append(
            {
                "name": frontmatter_skill_name(skill_file, canonical.name),
                "provider": provider,
                "plugin": plugin,
                "scope": scope,
                "path": str(canonical),
            }
        )
    return rows


def frontmatter_skill_name(skill_file: Path, fallback: str) -> str:
    """Return the invocation name Codex derives from SKILL.md."""
    try:
        lines = skill_file.read_text(encoding="utf-8").splitlines()
    except (OSError, UnicodeError):
        return fallback
    if not lines or lines[0] != "---":
        return fallback
    try:
        end = lines.index("---", 1)
    except ValueError:
        return fallback
    for line in lines[1:end]:
        if line[:1].isspace():
            continue
        match = re.fullmatch(r"name:\s*(.*)", line)
        if match is None:
            continue
        raw = match.group(1).strip()
        if len(raw) >= 2 and raw[0] == raw[-1] == '"':
            try:
                value = json.loads(raw)
            except json.JSONDecodeError:
                return fallback
            return value.strip() if isinstance(value, str) and value.strip() else fallback
        if len(raw) >= 2 and raw[0] == raw[-1] == "'":
            value = raw[1:-1].replace("''", "'").strip()
            return value or fallback
        raw = re.sub(r"\s+#.*$", "", raw).strip()
        if not raw or raw.lower() in {"null", "true", "false", "~"}:
            return fallback
        return raw
    return fallback


def codex_plugin_inventory(diagnostics: list[dict[str, str]]) -> tuple[str, Any, str]:
    fixture = os.environ.get("NORTH_CODEX_PLUGIN_INVENTORY")
    if fixture:
        state, value = read_json(Path(fixture), "codex", diagnostics)
        return state, value, fixture
    command = os.environ.get("NORTH_CODEX_BIN", "codex")
    try:
        result = subprocess.run(
            [command, "plugin", "list", "--json"],
            check=False,
            capture_output=True,
            text=True,
            timeout=5,
        )
    except (OSError, subprocess.TimeoutExpired):
        diagnostics.append(
            {"provider": "codex", "source": command, "kind": "unavailable", "message": "plugin inventory command failed or timed out"}
        )
        return "unavailable", None, command
    if result.returncode != 0:
        diagnostics.append(
            {"provider": "codex", "source": command, "kind": "unavailable", "message": "plugin inventory command failed"}
        )
        return "unavailable", None, command
    try:
        return "ok", json.loads(result.stdout), command
    except json.JSONDecodeError:
        diagnostics.append(
            {"provider": "codex", "source": command, "kind": "malformed", "message": "plugin inventory command returned malformed JSON"}
        )
        return "malformed", None, command


def codex_plugin_root(entry: dict[str, Any]) -> Path | None:
    source = entry.get("source")
    if isinstance(source, dict) and source.get("source") == "local" and isinstance(source.get("path"), str):
        path = Path(source["path"])
        return path if path.is_absolute() else None
    plugin_id = entry.get("pluginId")
    marketplace = entry.get("marketplaceName")
    version = entry.get("version")
    if not all(isinstance(value, str) and value for value in (plugin_id, marketplace, version)):
        return None
    name = plugin_id.split("@", 1)[0]
    return CODEX_HOME / "plugins/cache" / marketplace / name / version


def codex_plugin_skills(diagnostics: list[dict[str, str]]) -> tuple[list[dict[str, Any]], list[dict[str, str]]]:
    state, inventory, source = codex_plugin_inventory(diagnostics)
    sources = [source_record("codex-plugin-inventory", Path(source), state)]
    if state != "ok" or not isinstance(inventory, dict) or not isinstance(inventory.get("installed"), list):
        if state == "ok":
            diagnostics.append(
                {"provider": "codex", "source": source, "kind": "malformed", "message": "plugin inventory must contain an installed array"}
            )
        return [], sources
    rows: list[dict[str, Any]] = []
    for entry in inventory["installed"]:
        if not isinstance(entry, dict) or entry.get("installed") is not True or entry.get("enabled") is not True:
            continue
        plugin = entry.get("pluginId")
        root_path = codex_plugin_root(entry)
        if not isinstance(plugin, str) or root_path is None:
            diagnostics.append(
                {"provider": "codex", "source": source, "kind": "unresolved", "message": "enabled plugin has no provable local root"}
            )
            continue
        local_source = (
            isinstance(entry.get("source"), dict)
            and entry["source"].get("source") == "local"
        )
        root = canonical_directory(
            root_path,
            "codex",
            plugin,
            diagnostics,
            within=None if local_source else CODEX_HOME / "plugins/cache",
        )
        if root is None:
            continue
        skills = root / "skills"
        if skills.exists() or skills.is_symlink():
            if manifest_at(root, "codex", diagnostics) is None:
                continue
            rows.extend(
                skill_directories(
                    skills, "codex", plugin, "user", diagnostics, contain_in=root
                )
            )
    return rows, sources


def skill_audit() -> dict[str, Any]:
    diagnostics: list[dict[str, str]] = []
    entries: list[dict[str, Any]] = []
    sources: list[dict[str, str]] = []

    farm = canonical_directory(SHARED_SKILLS, "north", "shared", diagnostics)
    sources.append(source_record("north-shared", SHARED_SKILLS, "ok" if farm else "unavailable"))
    if farm is not None:
        entries.extend(
            skill_directories(
                SHARED_SKILLS, "north", "shared", "shared", diagnostics, contain_in=None
            )
        )

    system_root = canonical_directory(CODEX_SYSTEM_SKILLS, "codex", "builtin", diagnostics)
    sources.append(source_record("codex-system", CODEX_SYSTEM_SKILLS, "ok" if system_root else "unavailable"))
    if system_root is not None:
        entries.extend(
            skill_directories(
                CODEX_SYSTEM_SKILLS,
                "codex",
                "builtin",
                "system",
                diagnostics,
                contain_in=system_root,
            )
        )

    codex_rows, codex_sources = codex_plugin_skills(diagnostics)
    entries.extend(codex_rows)
    sources.extend(codex_sources)

    entries = list(
        {
            (entry["provider"], entry["plugin"], entry["scope"], entry["path"], entry["name"]): entry
            for entry in entries
        }.values()
    )

    by_name: dict[str, list[dict[str, Any]]] = {}
    for entry in entries:
        by_name.setdefault(entry["name"], []).append(entry)
    collisions = []
    for name, members in sorted(by_name.items()):
        if len(members) < 2:
            continue
        collisions.append(
            {
                "name": name,
                "precedence": "uncertain",
                "reason": "provider/plugin precedence is not declared across discovery roots",
                "entries": [
                    {key: member[key] for key in ("provider", "plugin", "scope", "path")}
                    for member in sorted(members, key=lambda row: (row["provider"], row["plugin"], row["path"]))
                ],
            }
        )
    for entry in entries:
        peers = [
            f'{peer["provider"]}:{peer["plugin"]}:{peer["path"]}'
            for peer in by_name[entry["name"]]
            if peer is not entry
        ]
        entry["collision"] = bool(peers)
        entry["collidesWith"] = sorted(peers)
        entry["precedence"] = "uncertain" if peers else "not-applicable"
    entries.sort(key=lambda row: (row["name"], row["provider"], row["plugin"], row["path"]))
    return {
        "state": "partial" if diagnostics else "complete",
        "sources": sources,
        "entries": entries,
        "collisions": collisions,
        "diagnostics": diagnostics,
    }


def digest_value(value: Any) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def protected_map(value: Any, field: str) -> dict[str, Any]:
    if value is None:
        value = {}
    if not isinstance(value, dict) or not all(isinstance(key, str) for key in value):
        raise ValueError(f"{field} must be an object")
    return {"keys": sorted(value), "digest": digest_value(value)}


def normalized_mcp(provider: str, name: str, value: Any, source: Path) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError("server declaration must be an object")
    enabled = value.get("enabled", True) is not False
    declared_type = str(value.get("type", "")).lower()
    http = bool(value.get("url")) or declared_type in {"http", "sse", "streamable-http"}
    if http:
        headers = value.get("http_headers", {})
        env_headers = value.get("env_http_headers", {})
        identity = {
            "enabled": enabled,
            "transport": "sse" if declared_type == "sse" else "http",
            "endpoint": value.get("url"),
            "headers": protected_map(headers, "headers"),
            "environmentHeaders": protected_map(env_headers, "environment headers"),
            "bearerTokenEnvironmentVariable": value.get("bearer_token_env_var"),
        }
    else:
        arguments = value.get("args", [])
        if not isinstance(arguments, list):
            raise ValueError("args must be an array")
        forwarded_environment = value.get("env_vars", [])
        if not isinstance(forwarded_environment, list) or not all(
            isinstance(item, str) for item in forwarded_environment
        ):
            raise ValueError("env_vars must be an array of names")
        identity = {
            "enabled": enabled,
            "transport": "stdio",
            "command": value.get("command"),
            "arguments": arguments,
            "workingDirectory": value.get("cwd"),
            "environment": protected_map(value.get("env", {}), "env"),
            "forwardedEnvironmentVariables": sorted(set(forwarded_environment)),
        }
    return {
        "name": name,
        "provider": provider,
        "source": str(source),
        "identityDigest": digest_value(identity),
        "normalized": identity,
    }


def mcp_source(
    provider: str,
    path: Path,
    loader: Any,
    diagnostics: list[dict[str, str]],
) -> tuple[list[dict[str, Any]], dict[str, str]]:
    state, document = loader(path, provider, diagnostics)
    source = source_record(provider, path, state)
    if state != "ok":
        return [], source
    servers = document.get("mcp_servers", {}) if isinstance(document, dict) else None
    if not isinstance(servers, dict):
        diagnostics.append(
            {"provider": provider, "source": str(path), "kind": "malformed", "message": "MCP server table is missing or not an object"}
        )
        source["state"] = "malformed"
        return [], source
    rows = []
    for name, value in sorted(servers.items()):
        try:
            rows.append(normalized_mcp(provider, str(name), value, path))
        except (TypeError, ValueError):
            diagnostics.append(
                {"provider": provider, "source": str(path), "kind": "malformed-server", "message": f"MCP declaration is malformed: {name}"}
            )
    return rows, source


def mcp_audit() -> dict[str, Any]:
    diagnostics: list[dict[str, str]] = []
    codex, codex_source = mcp_source("codex", CODEX_CONFIG, read_toml, diagnostics)
    return {
        "state": "partial" if diagnostics else "complete",
        "sources": [codex_source],
        "servers": sorted(codex, key=lambda row: row["name"]),
        "diagnostics": diagnostics,
    }


def short_path(value: str) -> str:
    home = str(HOME)
    return "~" + value[len(home) :] if home and (value == home or value.startswith(home + os.sep)) else value


def render_skills(report: dict[str, Any]) -> None:
    print("EFFECTIVE SKILL PROVENANCE")
    if not report["entries"]:
        print("  (no readable effective skills)")
    for row in report["entries"]:
        collision = "COLLISION" if row["collision"] else "distinct"
        print(
            f'  {row["name"]}  {row["provider"]}/{row["plugin"]}  {row["scope"]}  '
            f'{collision}  precedence={row["precedence"]}  {short_path(row["path"])}'
        )
    for diagnostic in report["diagnostics"]:
        print(
            f'  warning [{diagnostic["provider"]}/{diagnostic["kind"]}] '
            f'{short_path(diagnostic["source"])}: {diagnostic["message"]}'
        )


def render_mcp(report: dict[str, Any]) -> None:
    print("CODEX MCP DECLARATIONS")
    if not report["servers"]:
        print("  (no readable declarations)")
    for server in report["servers"]:
        identity = server["normalized"]
        protected = []
        for field in ("environment", "headers", "environmentHeaders"):
            if field in identity:
                protected.append(
                    f'{field}Keys={",".join(identity[field]["keys"])} {field}Digest={identity[field]["digest"]}'
                )
        print(
            f'    {server["provider"]}/{server["name"]} {identity["transport"]} '
            f'{server["identityDigest"]} {" ".join(protected)} source={short_path(server["source"])}'
        )
    for diagnostic in report["diagnostics"]:
        print(
            f'  warning [{diagnostic["provider"]}/{diagnostic["kind"]}] '
            f'{short_path(diagnostic["source"])}: {diagnostic["message"]}'
        )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", dest="as_json")
    parser.add_argument("--section", choices=("all", "skills", "mcp"), default="all")
    args = parser.parse_args()
    report: dict[str, Any] = {"version": 1}
    if args.section in {"all", "skills"}:
        report["skills"] = skill_audit()
    if args.section in {"all", "mcp"}:
        report["mcp"] = mcp_audit()
    if args.as_json:
        json.dump(report, sys.stdout, indent=2, sort_keys=True)
        print()
    else:
        if "skills" in report:
            render_skills(report["skills"])
        if "mcp" in report:
            if "skills" in report:
                print()
            render_mcp(report["mcp"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
