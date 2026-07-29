#!/usr/bin/env python3

from __future__ import annotations

import pathlib
import socket
import subprocess
import sys
import tempfile
import threading
import unittest


HELPER = pathlib.Path(__file__).resolve().parent.parent / "proxy-route.py"


class OneResponseServer:
    def __init__(self, path: pathlib.Path, response: bytes):
        self.path = path
        self.response = response
        self.ready = threading.Event()
        self.thread = threading.Thread(target=self.run, daemon=True)

    def run(self) -> None:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
            server.bind(str(self.path))
            server.listen(1)
            server.settimeout(2)
            self.ready.set()
            try:
                connection, _ = server.accept()
            except TimeoutError:
                return
            with connection:
                connection.recv(4096)
                connection.sendall(self.response)

    def start(self) -> None:
        self.thread.start()
        if not self.ready.wait(2):
            raise AssertionError("test HAProxy server did not start")

    def join(self) -> None:
        self.thread.join(3)
        if self.thread.is_alive():
            raise AssertionError("test HAProxy server did not stop")


class ProxyRouteTest(unittest.TestCase):
    def run_helper(
        self,
        *,
        durable: str = "blue",
        runtime: bytes = b"0xabc active blue\n",
        transaction: bool = False,
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            socket_path = root / "admin.sock"
            map_path = root / "route.map"
            transaction_path = root / "selector.transaction"
            lock_path = root / "selector.lock"
            map_path.write_text(f"active {durable}\n", encoding="utf-8")
            lock_path.touch()
            if transaction:
                transaction_path.write_text("holding blue green\n", encoding="utf-8")
            server = OneResponseServer(socket_path, runtime)
            server.start()
            result = subprocess.run(
                [
                    sys.executable,
                    str(HELPER),
                    str(socket_path),
                    str(map_path),
                    str(transaction_path),
                    str(lock_path),
                ],
                text=True,
                capture_output=True,
                timeout=4,
                check=False,
            )
            server.join()
            return result

    def test_blue(self) -> None:
        result = self.run_helper()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "blue\n")

    def test_green(self) -> None:
        result = self.run_helper(
            durable="green", runtime=b"0xdef active green\n"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "green\n")

    def test_durable_runtime_disagreement(self) -> None:
        result = self.run_helper(
            durable="blue", runtime=b"0xdef active green\n"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("durable/runtime route disagreement", result.stderr)

    def test_unfinished_transaction(self) -> None:
        result = self.run_helper(transaction=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unfinished selector transaction exists", result.stderr)

    def test_malformed_runtime(self) -> None:
        result = self.run_helper(runtime=b"0xabc active purple\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("malformed active selector", result.stderr)

    def test_duplicate_runtime_rows(self) -> None:
        result = self.run_helper(
            runtime=b"0xabc active blue\n0xdef active blue\n"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("exactly one canonical row", result.stderr)

    def test_malformed_durable_route(self) -> None:
        result = self.run_helper(durable="purple")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("durable route map is malformed", result.stderr)


if __name__ == "__main__":
    unittest.main()
