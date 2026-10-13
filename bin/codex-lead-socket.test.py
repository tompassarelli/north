import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


if '--isolated' not in sys.argv:
    raise SystemExit(subprocess.call(['bwrap', '--dev-bind', '/', '/', '--tmpfs', '/tmp', '--unshare-net', '--', 'python3', '-I', __file__, '--isolated']))

repo = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix='codex-lead-socket-') as temporary:
    root = Path(temporary)
    home = root / 'home'
    (home / 'sqlite').mkdir(parents=True)
    stubs = root / 'bin'
    stubs.mkdir()
    key = hashlib.sha256(f'{home}\n{home}/sqlite'.encode()).hexdigest()[:16]
    endpoint = f"unix://{os.environ.get('XDG_RUNTIME_DIR', '/run/user/' + str(os.getuid()))}/codex-shared-{key}/app-server.sock"
    directory = Path(f'/tmp/codex-daemon-{os.getuid()}')
    directory.mkdir()
    shared_socket = directory / 'abc1'
    probe_socket = directory / 'abc2'
    server = root / 'server.py'
    server.write_text('''import json, socket, sys
with socket.socket(socket.AF_UNIX) as server:
    server.bind(sys.argv[1])
    server.listen()
    print('ready', flush=True)
    while True:
        conn, _ = server.accept()
        with conn:
            request = json.loads(conn.makefile().readline())
            assert request['method'] == 'thread/goal/get', request
            assert request['params'] == {'threadId': 'fixture-lead'}, request
            conn.sendall((json.dumps({'id': 2, 'result': {'goal': {'status': 'active', 'objective': sys.argv[2]}}}) + '\\n').encode())
''')
    client = stubs / 'websocat'
    client.write_text('''#!/usr/bin/env python3
import json, socket, sys
for line in sys.stdin:
    request = json.loads(line)
    if request.get('id') == 2:
        with socket.socket(socket.AF_UNIX) as client:
            client.connect(sys.argv[-1].removeprefix('ws-c:unix:'))
            client.sendall(line.encode())
            print(client.makefile().readline(), end='', flush=True)
        break
''')
    client.chmod(0o755)
    owners = []
    try:
        for path, objective, address in ((shared_socket, 'shared owner', endpoint), (probe_socket, 'probe owner', 'unix:///tmp/probe.sock')):
            owner = subprocess.Popen(['python3', str(server), str(path), objective, 'app-server', '--listen', address], stdout=subprocess.PIPE, text=True)
            owners.append(owner)
            assert owner.stdout.readline() == 'ready\n'
        rows = subprocess.check_output(['ss', '-xlpH'], text=True).splitlines()
        listeners = [row for row in rows if str(shared_socket) in row or str(probe_socket) in row]
        assert len(listeners) == 2, listeners
        env = dict(os.environ, PATH=str(stubs) + os.pathsep + os.environ['PATH'], NORTH_CODEX_POOLED_HOME=str(home), CODEX_LEAD_THREAD='fixture-lead')
        env.pop('NORTH_CODEX_CONVERSATION_HOME', None)
        env.pop('NORTH_CODEX_CONVERSATION_SQLITE_HOME', None)
        for first in (probe_socket, shared_socket):
            ordered = sorted(listeners, key=lambda row: str(first) not in row)
            (root / 'listeners').write_text('\n'.join(ordered) + '\n')
            stub = stubs / 'ss'
            stub.write_text('#!/bin/sh\nexec cat ' + str(root / 'listeners') + '\n')
            stub.chmod(0o755)
            result = subprocess.run([str(repo / 'bin/codex-lead'), 'goal-get'], env=env, capture_output=True, text=True, timeout=10)
            assert result.returncode == 0, result.stderr
            assert result.stdout.strip() == 'active: shared owner', result.stdout
        print('PASS shared ownership selects goal-get with a probe started second and either listener order')
    finally:
        for owner in owners:
            owner.terminate()
            owner.wait()
