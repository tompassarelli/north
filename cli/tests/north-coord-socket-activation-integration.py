#!/usr/bin/env python3
"""Exercise the North LISTEN_FDS adapter and restart-time accept queue."""

from __future__ import annotations

import os
import socket
import subprocess
import sys
import threading
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ADAPTER = ROOT / "bin" / "north-coord-sd-listen"


def check(condition: bool, message: str) -> None:
    if not condition:
        raise AssertionError(message)


def child() -> None:
    fd_text = os.environ.get("FRAM_LISTEN_FD")
    check(fd_text == "3", f"child received FRAM_LISTEN_FD={fd_text!r}")
    listener = socket.socket(fileno=int(fd_text))
    print("ready", flush=True)
    connection, _ = listener.accept()
    with connection:
        request = connection.recv(64)
        connection.sendall(b"completed:" + request)


def activated_child(listener_fd: int) -> subprocess.Popen[str]:
    command = [
        "bash",
        "-c",
        'export LISTEN_PID=$$ LISTEN_FDS=1 LISTEN_FDNAMES=north-coord; exec "$@"',
        "north-coord-activation-test",
        str(ADAPTER),
        sys.executable,
        str(Path(__file__).resolve()),
        "--child",
    ]
    return subprocess.Popen(
        command,
        pass_fds=(listener_fd,),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )


def expect_activation_error(env: dict[str, str], expected: str) -> None:
    result = subprocess.run(
        [str(ADAPTER), "/bin/true"],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    check(result.returncode == 2, f"expected rc 2, got {result.returncode}")
    check(expected in result.stderr, f"missing named error {expected!r}: {result.stderr}")


def unit_checks() -> None:
    no_activation = subprocess.run(
        [str(ADAPTER), "/usr/bin/env"],
        env={"PATH": os.environ["PATH"]},
        capture_output=True,
        text=True,
        check=True,
    )
    check("FRAM_LISTEN_FD=" not in no_activation.stdout, "cold bind path gained an fd")

    bad_pid = dict(os.environ, LISTEN_PID="1", LISTEN_FDS="1")
    expect_activation_error(bad_pid, "LISTEN_PID does not name this process")
    bad_count = subprocess.run(
        [
            "bash",
            "-c",
            'export LISTEN_PID=$$ LISTEN_FDS=2; exec "$@"',
            "north-coord-bad-count-test",
            str(ADAPTER),
            "/bin/true",
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    check(bad_count.returncode == 2, f"expected bad-count rc 2, got {bad_count.returncode}")
    check(
        "expected exactly one inherited descriptor, got 2" in bad_count.stderr,
        f"missing descriptor-count error: {bad_count.stderr}",
    )

    inherited, peer = socket.socketpair()
    peer.close()
    if inherited.fileno() != 3:
        os.dup2(inherited.fileno(), 3)
        inherited.close()
        inherited = socket.socket(fileno=3)
    inherited.set_inheritable(True)
    activated_env = subprocess.run(
        [
            "bash",
            "-c",
            'export LISTEN_PID=$$ LISTEN_FDS=1 LISTEN_FDNAMES=north-coord; exec "$@"',
            "north-coord-unit-test",
            str(ADAPTER),
            "/usr/bin/env",
        ],
        pass_fds=(inherited.fileno(),),
        capture_output=True,
        text=True,
        check=True,
    )
    inherited.close()
    check("FRAM_LISTEN_FD=3" in activated_env.stdout, "activation fd was not translated")
    check("LISTEN_FDS=" not in activated_env.stdout, "systemd fd count leaked to Fram")
    check("LISTEN_PID=" not in activated_env.stdout, "systemd pid leaked to Fram")
    check("LISTEN_FDNAMES=" not in activated_env.stdout, "systemd fd name leaked to Fram")


def main() -> None:
    unit_checks()
    listener = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    listener.bind(("127.0.0.1", 0))
    listener.listen(4096)
    if listener.fileno() != 3:
        os.dup2(listener.fileno(), 3)
        listener.close()
        listener = socket.socket(fileno=3)
    listener.set_inheritable(True)

    first = activated_child(listener.fileno())
    check(first.stdout is not None, "first child stdout unavailable")
    check(first.stdout.readline().strip() == "ready", "first child did not inherit listener")
    first.terminate()
    first.wait(timeout=5)

    client = socket.create_connection(listener.getsockname(), timeout=2)
    client.sendall(b"queued-during-restart")

    response: list[bytes] = []
    failure: list[BaseException] = []

    def receive() -> None:
        try:
            response.append(client.recv(128))
        except BaseException as error:
            failure.append(error)

    receiver = threading.Thread(target=receive)
    receiver.start()
    receiver.join(timeout=0.1)
    check(receiver.is_alive(), "request completed before replacement daemon started")

    second = activated_child(listener.fileno())
    check(second.stdout is not None, "second child stdout unavailable")
    check(second.stdout.readline().strip() == "ready", "replacement did not inherit listener")
    receiver.join(timeout=5)
    second.wait(timeout=5)
    client.close()
    listener.close()

    check(not receiver.is_alive(), "queued client did not complete")
    check(not failure, f"queued client failed: {failure}")
    check(response == [b"completed:queued-during-restart"], f"unexpected response: {response}")
    print("ok: client connected during restart, queued on inherited :7977-style socket, and completed")


if __name__ == "__main__":
    if sys.argv[1:] == ["--child"]:
        child()
    elif sys.argv[1:] == ["--unit"]:
        unit_checks()
        print("ok: LISTEN_FDS translated to engine-neutral FRAM_LISTEN_FD=3")
    elif sys.argv[1:]:
        raise SystemExit(
            "usage: north-coord-socket-activation-integration.py [--child|--unit]"
        )
    else:
        main()
