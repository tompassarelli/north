#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from datetime import datetime, timezone


class IdleAppServerBoundary(unittest.TestCase):
    def test_idle_read_and_rpc_failure_clean_up_owned_process(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as directory:
            scratch = Path(directory)
            runtime = scratch / "codex"
            runtime.write_text(r"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
Path(os.environ['PROBE_PID']).write_text(str(os.getpid()))
for line in sys.stdin:
    request = json.loads(line)
    method = request['method']
    with open(os.environ['PROBE_METHODS'], 'a') as log:
        log.write(method + '\n')
    if 'id' not in request:
        continue
    result = {}
    if method == 'account/read':
        result = {'account': {'accountId': 'fixture'}}
    if method == 'account/rateLimits/read':
        if os.environ.get('PROBE_FAIL'):
            print(json.dumps({'id': request['id'], 'error': {'code': -32000}}), flush=True)
            continue
        result = {'rateLimits': {'limitId': 'codex', 'primary': {'windowDurationMins': 10080,
                  'usedPercent': 77, 'resetsAt': 1800244689},
                  'credits': {'hasCredits': False, 'ordinaryUsageAllowed': True}}}
    print(json.dumps({'id': request['id'], 'result': result}), flush=True)
""")
            runtime.chmod(0o755)
            listener = scratch / "ss"
            listener.write_text("#!/bin/sh\nexit 0\n")
            listener.chmod(0o755)
            env = dict(os.environ, CODEX_RUNTIME=str(runtime), NORTH_CODEX_POOLED_HOME=str(scratch),
                       PATH=str(scratch) + os.pathsep + os.environ['PATH'],
                       PROBE_PID=str(scratch / 'pid'), PROBE_METHODS=str(scratch / 'methods'))
            adapter = Path(__file__).with_name('usage_codex.py')
            for failure in (False, True):
                with self.subTest(rpc_failure=failure):
                    (scratch / 'methods').unlink(missing_ok=True)
                    run = subprocess.run(['python3', '-I', str(adapter)], env=env | ({'PROBE_FAIL': '1'} if failure else {}),
                                         capture_output=True, text=True, timeout=10)
                    pid = int((scratch / 'pid').read_text())
                    with self.assertRaises(ProcessLookupError):
                        os.kill(pid, 0)
                    self.assertEqual((scratch / 'methods').read_text().splitlines(),
                                     ['initialize', 'initialized', 'account/read', 'account/rateLimits/read'])
                    if failure:
                        self.assertEqual(run.returncode, 1)
                        self.assertIn('RPC code -32000', run.stderr)
                        self.assertEqual(run.stdout, '')
                    else:
                        self.assertEqual(run.returncode, 0, run.stderr)
                        rows = [json.loads(line) for line in run.stdout.splitlines()]
                        week = next(row for row in rows if row['name'] == 'week')
                        self.assertEqual(week['used_pct'], 77)
                        age = datetime.now(timezone.utc) - datetime.fromisoformat(week['ts'].replace('Z', '+00:00'))
                        self.assertLess(age.total_seconds(), 20 * 60)
                        self.assertEqual(next(row for row in rows if row['name'] == 'ordinaryUsageAllowed')['amount'], 1)


if __name__ == '__main__':
    unittest.main()
