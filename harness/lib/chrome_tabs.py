from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import socket
import struct
import time
from uuid import uuid4


class NativeHostGone(ConnectionError):
    pass


def process_token(pid):
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()
        return None if fields[0] == "Z" else fields[19]
    except (OSError, IndexError):
        return None


def live(record, owner_token, child_token):
    return owner_token == record["owner_start"] or (
        record.get("child_start") is not None and child_token == record["child_start"])


def close_candidates(record, owner_token, child_token, group_id, current_ids):
    if live(record, owner_token, child_token) or group_id != record.get("group_id"):
        return []
    return sorted(set(record["tabs"]) & set(current_ids))


def create(path, record):
    with os.fdopen(os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600), "w") as stream:
        stream.write(json.dumps(record) + "\n")


@contextmanager
def locked(path):
    with open(path, "r+") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        if os.fstat(stream.fileno()).st_nlink == 0:
            raise FileNotFoundError(f"Chrome session {path.stem} was already reaped")
        record = json.load(stream)
        yield record
        stream.seek(0)
        stream.truncate()
        stream.write(json.dumps(record) + "\n")


def receive(sock, size):
    data = bytearray()
    while len(data) < size:
        part = sock.recv(size - len(data))
        if not part:
            raise ConnectionError("Chrome native host disconnected")
        data.extend(part)
    return bytes(data)


def error_result(error):
    content = error.get("content")
    if isinstance(content, str):
        content = [{"type": "text", "text": content}]
    return {**error, "isError": True, "content": content or []}


def native_call(record, tool, args):
    path = Path(record["native_socket"])
    if not path.is_socket() or path.stat().st_ino != record["native_inode"]:
        raise NativeHostGone("Chrome native host changed; keeping recorded tabs")
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(10)
        sock.connect(str(path))
        request_id = str(uuid4())
        params = {"client_id": "claude-code", "tool": tool, "args": args,
                  "tool_use_id": request_id,
                  "session_scope": {"sessionId": record["group_key"]}}
        if record.get("group_id") is not None:
            params["session_scope"]["tabGroupId"] = record["group_id"]
        payload = json.dumps({"method": "execute_tool", "params": params}).encode()
        sock.sendall(struct.pack("<I", len(payload)) + payload)
        while True:
            length = struct.unpack("<I", receive(sock, 4))[0]
            if length > 32 * 1024 * 1024:
                raise ValueError("Chrome native host response too large")
            reply = json.loads(receive(sock, length))
            if reply.get("tool_use_id") == request_id:
                if "error" in reply:
                    return error_result(reply["error"])
                return reply["result"]


def context(result):
    for block in result.get("content", []):
        if block.get("type") == "text":
            try:
                value = json.loads(block["text"])
                if isinstance(value, dict) and isinstance(value.get("availableTabs"), list):
                    return value
            except (ValueError, KeyError):
                pass
    return {"availableTabs": []}


PAGE_TOOLS = {"computer", "file_upload", "find", "form_input", "get_page_text", "gif_creator",
              "javascript_tool", "read_console_messages", "read_network_requests", "read_page",
              "upload_image"}


def created_tab(result):
    for block in result.get("content", []):
        if block.get("type") == "text":
            match = re.search(r"Created new tab\. Tab ID: (\d+)", block["text"])
            if match:
                return int(match[1])
    return None


def tab_id(args):
    try:
        return int(args.get("tabId"))
    except (TypeError, ValueError):
        return None


def record_result(record, tool, args, result):
    if result.get("isError"):
        return
    blank = set(record.get("blank", []))
    if tool == "tabs_context_mcp":
        value = context(result)
        group_id = value.get("tabGroupId")
        if group_id is not None and group_id != record.get("group_id"):
            record["group_id"] = group_id
            record["tabs"].extend(tab["tabId"] for tab in value["availableTabs"])
        if "tabGroupId" in value:
            blank = {tab["tabId"] for tab in value["availableTabs"]
                     if not tab.get("url") or tab["url"].startswith("chrome://newtab")}
    elif tool == "tabs_create_mcp":
        tab = created_tab(result)
        if tab is not None:
            record["tabs"].append(tab)
            blank.add(tab)
    elif tool == "navigate":
        blank.discard(tab_id(args))
    record["tabs"] = sorted(set(record["tabs"]))
    record["blank"] = sorted(blank)


SAFETY_PENDING = "Could not verify this site's safety category"
SAFETY_BACKOFF = (1, 2, 4, 8, 15, 30, 30, 30)


def safety_pending(result):
    return result.get("isError") and any(
        SAFETY_PENDING in block.get("text", "") for block in result.get("content", []))


def call(path, tool, args, sleep=time.sleep):
    if tool == "browser_batch":
        raise ValueError("Use individual Chrome calls so tab creation is recorded")
    if tool == "tabs_create_mcp" and args.get("url"):
        created = call(path, tool, {}, sleep)
        tab = created_tab(created)
        if tab is None:
            return created
        result = call(path, "navigate", {"tabId": tab, "url": args["url"]}, sleep)
        return {**result, "content": [*created["content"][:1], *result.get("content", [])]}
    waited = 0
    for delay in (*SAFETY_BACKOFF, None):
        with locked(path) as record:
            if tool in PAGE_TOOLS and tab_id(args) in record.get("blank", []):
                text = (f"Tab {tab_id(args)} is still a blank New Tab page, which Chrome cannot "
                        "allow-list, so acting on it would prompt the user. Navigate it first, "
                        "or create tabs with tabs_create_mcp {\"url\": ...}.")
                return {"isError": True, "content": [{"type": "text", "text": text}]}
            result = native_call(record, tool, args)
            record_result(record, tool, args, result)
        if not safety_pending(result):
            return result
        if delay is None:
            break
        sleep(delay)
        waited += delay
    text = (f"Chrome's site safety service stayed unavailable for {waited}s, which happens for a few "
            "minutes after Chrome starts; navigation is blocked until it answers. Retry later.")
    return {"isError": True, "content": [{"type": "text", "text": text}, *result["content"]]}


def reap(path):
    failures = 0
    while failures < 60:
        try:
            with locked(path) as record:
                owner = process_token(record["owner_pid"])
                child = process_token(record["child_pid"]) if record.get("child_pid") else None
                if not live(record, owner, child):
                    result = native_call(record, "tabs_context_mcp", {"createIfEmpty": False})
                    if result.get("isError"):
                        raise ConnectionError("Chrome refused cleanup context")
                    value = context(result)
                    ids = [tab["tabId"] for tab in value["availableTabs"]]
                    candidates = close_candidates(record, owner, child, value.get("tabGroupId"), ids)
                    if candidates and set(candidates) >= set(ids):
                        if native_call(record, "tabs_create_mcp", {}).get("isError"):
                            raise ConnectionError("Chrome refused the placeholder tab")
                    for tab_id in candidates:
                        if native_call(record, "tabs_close_mcp", {"tabId": tab_id}).get("isError"):
                            raise ConnectionError("Chrome refused recorded tab cleanup")
                    path.unlink()
                    return
        except (FileNotFoundError, NativeHostGone):
            return
        except (OSError, ValueError):
            failures += 1
        time.sleep(1)
