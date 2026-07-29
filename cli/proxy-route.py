#!/usr/bin/env python3
"""Read the active North coordinator slot from HAProxy's runtime map."""

from __future__ import annotations

import fcntl
import os
import socket
import stat
import sys
from typing import NoReturn


MAX_RESPONSE_BYTES = 64 * 1024


def fail(message: str) -> NoReturn:
    raise SystemExit(f"north proxy route: {message}")


def parse_route(response: str) -> str:
    rows = [line.split() for line in response.splitlines() if line.strip()]
    if len(rows) != 1 or len(rows[0]) != 3:
        fail("HAProxy route map must contain exactly one canonical row")
    address, key, slot = rows[0]
    if not address.startswith("0x") or key != "active" or slot not in {"blue", "green"}:
        fail("HAProxy route map returned a malformed active selector")
    return slot


def read_regular_text(path: str, label: str) -> str:
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        fail(f"{label} is unreadable or unsafe: {path}: {error.strerror}")
    try:
        if not stat.S_ISREG(os.fstat(descriptor).st_mode):
            fail(f"{label} is not a regular file: {path}")
        data = os.read(descriptor, 4097)
        if len(data) > 4096 or os.read(descriptor, 1):
            fail(f"{label} exceeds 4096 bytes")
    finally:
        os.close(descriptor)
    try:
        return data.decode("utf-8", errors="strict")
    except UnicodeDecodeError:
        fail(f"{label} is not valid UTF-8")


def read_durable_route(map_path: str) -> str:
    text = read_regular_text(map_path, "durable route map")
    if not text.endswith("\n") or text.count("\n") != 1:
        fail("durable route map must be one LF-terminated canonical line")
    fields = text[:-1].split(" ")
    if len(fields) != 2 or fields[0] != "active" or fields[1] not in {"blue", "green"}:
        fail("durable route map is malformed")
    return fields[1]


def require_no_transaction(transaction_path: str) -> None:
    try:
        os.lstat(transaction_path)
    except FileNotFoundError:
        return
    except OSError as error:
        fail(f"cannot inspect selector transaction: {error.strerror}")
    fail(f"unfinished selector transaction exists: {transaction_path}")


def read_runtime_route(socket_path: str, map_path: str) -> str:
    try:
        socket_mode = os.lstat(socket_path).st_mode
    except OSError as error:
        fail(f"selector socket is missing or unsafe: {socket_path}: {error.strerror}")
    if not stat.S_ISSOCK(socket_mode):
        fail(f"selector socket is missing or unsafe: {socket_path}")

    request = f"show map {map_path}\n".encode("utf-8")
    chunks: list[bytes] = []
    total = 0
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(2.0)
        client.connect(socket_path)
        client.sendall(request)
        client.shutdown(socket.SHUT_WR)
        while True:
            chunk = client.recv(4096)
            if not chunk:
                break
            total += len(chunk)
            if total > MAX_RESPONSE_BYTES:
                fail("HAProxy route response exceeds 65536 bytes")
            chunks.append(chunk)
    try:
        response = b"".join(chunks).decode("utf-8", errors="strict")
    except UnicodeDecodeError:
        fail("HAProxy route response is not valid UTF-8")
    return parse_route(response)


def read_route(
    socket_path: str,
    map_path: str,
    transaction_path: str,
    lock_path: str,
) -> str:
    if not all(
        os.path.isabs(path)
        for path in (socket_path, map_path, transaction_path, lock_path)
    ):
        fail("selector socket, map, transaction, and lock paths must be absolute")
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        lock = os.open(lock_path, flags)
    except OSError as error:
        fail(f"selector lock is unreadable or unsafe: {lock_path}: {error.strerror}")
    try:
        if not stat.S_ISREG(os.fstat(lock).st_mode):
            fail(f"selector lock is not a regular file: {lock_path}")
        fcntl.flock(lock, fcntl.LOCK_SH)
        require_no_transaction(transaction_path)
        durable_before = read_durable_route(map_path)
        runtime = read_runtime_route(socket_path, map_path)
        require_no_transaction(transaction_path)
        durable_after = read_durable_route(map_path)
        if durable_before != durable_after:
            fail("durable route changed during attestation")
        if durable_after != runtime:
            fail(
                "durable/runtime route disagreement: "
                f"durable={durable_after} runtime={runtime}"
            )
        return runtime
    finally:
        os.close(lock)


def main(argv: list[str]) -> int:
    if argv == ["--parse"]:
        print(parse_route(sys.stdin.read()))
        return 0
    if len(argv) != 4:
        fail(
            "usage: proxy-route.py SOCKET MAP TRANSACTION LOCK "
            "| proxy-route.py --parse"
        )
    print(read_route(argv[0], argv[1], argv[2], argv[3]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
