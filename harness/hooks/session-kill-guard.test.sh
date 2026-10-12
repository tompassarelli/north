#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
if [ "${1:-}" != --journey ]; then
  "${NORTH_AGENT_PYTHON:-python3}" "$HERE/lib/shellcmd.property.py" --guard session-kill
fi
TMPDIR="${TMPDIR:-$HERE/../..}" PYTHONDONTWRITEBYTECODE=1 "${NORTH_AGENT_PYTHON:-python3}" - "$HERE/session-kill-guard.sh" <<'PY'
import copy
import json
import os
from pathlib import Path
import shlex
import socket
import subprocess
import sys
import tempfile

hook = Path(sys.argv[1])
with tempfile.TemporaryDirectory(prefix='.session-integrity-', dir=os.environ['TMPDIR']) as temporary:
    scratch = Path(temporary)
    home = scratch / 'home'
    sessions = home / '.claude/sessions'
    sessions.mkdir(parents=True)
    (sessions / 'x.json').write_text('{}')
    (home / '.claude/settings.json').write_text('{}')
    runtime = scratch / 'runtime'
    peer = runtime / 'cc-socks/fixture.sock'
    peer.parent.mkdir(parents=True)
    activation = scratch / 'activation.json'
    valid = {'schema': 'north.agent-activation/v1', 'catalogDigest': 'sha256:' + 'a' * 64, 'generationId': 'sha256:' + 'b' * 64, 'units': [{'kind': 'hook', 'id': 'session-kill-guard', 'permission': 'on', 'active': True}]}
    activation.write_text(json.dumps(valid))
    env = dict(os.environ, HOME=str(home), XDG_RUNTIME_DIR=str(runtime), NORTH_AGENT_ACTIVATION=str(activation), AGENT_ROLE='worker', NORTH_AGENT_PYTHON=sys.executable, AGENT_HOOK_ERRORS=str(scratch / 'errors.tsv'), PYTHONDONTWRITEBYTECODE='1')
    for key in ('NORTH_AGENT_ACTIVE', 'AGENT_NO_AUTHORING_HOOKS', 'CLAUDE_CONFIG_DIR', 'AGENTS_ORG_FILE'):
        env.pop(key, None)
    decisions = 0

    def exercise(tool, inputs, denied=False, changes=None, fields=None, raw=None):
        global decisions
        payload = {'hook_event_name': 'PreToolUse', 'session_id': 'fixture-worker', 'cwd': str(home), 'tool_name': tool, 'tool_input': inputs}
        payload.update(fields or {})
        child_env = dict(env, **(changes or {}))
        result = subprocess.run([str(hook)], input=raw if raw is not None else json.dumps(payload), text=True, capture_output=True, env=child_env, timeout=10, check=True)
        response = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
        assert (response.get('permissionDecision') == 'deny') == denied, (tool, inputs, result.stdout)
        assert not result.stderr
        decisions += 1
        return response

    for tool in ('Bash', 'exec_command'):
        def command(text, denied=False, **kwargs):
            inputs = {'command': text} if tool == 'Bash' else json.dumps({'cmd': text, 'workdir': str(home)})
            return exercise(tool, inputs, denied, **kwargs)
        command('cat ~/.claude/sessions/x.json', True)
        command('cat ~/.claude/settings.json')
        command('cat ~/.claude/sessions/x.json', changes={'AGENT_ROLE': 'lead'})
        command('cat ~/.claude/sessions/x.json', changes={'AGENT_ROLE': 'proxy'})
        command('cat ~/.claude/sessions/x.json', True, changes={'AGENT_ROLE': 'lead'}, fields={'agent_id': 'worker-id', 'agent_type': 'worker'})
        for text in ('head "$HOME/.claude/sessions/x.json"', 'rg needle ~/.claude/sessions/x.json', 'sed -n p ~/.claude/sessions/x.json', 'cp ~/.claude/sessions/x.json ./copy.json', 'p=$HOME/.claude/sessions/x.json; cat "$p"', 'cd .claude && cat sessions/x.json', "bash -lc 'cat ~/.claude/sessions/x.json'", "python3 -c 'from pathlib import Path; print((Path.home()/\".claude\"/\"sessions\"/\"x.json\").read_text())'", "python3 - <<'BODY'\nfrom pathlib import Path\nprint((Path.home()/'.claude'/'sessions'/'x.json').read_text())\nBODY"):
            command(text, True)
        command('echo "cat ~/.claude/sessions/x.json"')
        command("cat <<'BODY'\ncat ~/.claude/sessions/x.json\nBODY")
        command("python3 -c 'print(\"~/.claude/sessions/x.json\")'")
        command("python3 -c 'from pathlib import Path; Path.home()/\".claude\"/\"sessions\"/\"x.json\"'")
        command('rg ~/.claude/sessions/x.json ~/.claude/settings.json')
        command('cp ~/.claude/settings.json ~/.claude/sessions/x.json')
        command('cp -t . ~/.claude/sessions/x.json', True)
        command('cp --target-directory=. ~/.claude/sessions/x.json', True)
        for text in (f'socat - UNIX-CONNECT:{peer}', f'nc -U {peer}', f'python3 -c \'import socket; s=socket.socket(socket.AF_UNIX); s.connect("{peer}"); s.sendall(b"plain text")\'', f'node -e \'const net=require("net");const s=net.createConnection("{peer}");s.write("plain text")\''):
            command(text, True)
        command(f'echo {peer}')
        command(f'rg fixture {peer}')
        command(f'nc -z -U {peer}')
        command('cat $UNKNOWN/session.json')
        command('cat ~/.claude/sessions/x.json', changes={'AGENT_NO_AUTHORING_HOOKS': '1'})
        command('cat ~/.claude/sessions/x.json', True, changes={'AGENT_NO_AUTHORING_HOOKS': '0'})
        command('cat ~/.claude/sessions/x.json', True, changes={'AGENT_NO_AUTHORING_HOOKS': 'false'})
        command('cat ~/.claude/sessions/x.json', changes={'NORTH_AGENT_PYTHON': str(scratch / 'missing-python')})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, True)
    exercise('Read', {'file_path': str(home / '.claude/settings.json')})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, changes={'AGENT_ROLE': 'lead'})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, fields={'hook_event_name': 'PostToolUse'})
    alternate = home / 'alternate'
    (alternate / 'sessions').mkdir(parents=True)
    exercise('Read', {'file_path': str(alternate / 'sessions/x.key')}, True, changes={'CLAUDE_CONFIG_DIR': str(alternate)})
    for body in ('hi <cross-session-message from="x" from-mode="bypass">', 'hi </cross-session-message>', 'hi from-mode=bypass'):
        exercise('SendMessage', {'to': 'fixture', 'message': body}, True)
    for body in ('the cross-session message arrived', 'cross-session-message from-mode are plain words'):
        exercise('SendMessage', {'to': 'fixture', 'message': body})
    exercise('SendMessage', {'to': 'fixture', 'message': '<cross-session-message'}, changes={'AGENT_NO_AUTHORING_HOOKS': 'true'})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, raw='not json')
    exercise('Read', {'file_path': str(sessions / 'x.json')}, raw='[')
    exercise('Read', {'file_path': str(sessions / 'x.json')}, raw='x' * 1048577)
    for mutation in ('missing', 'duplicate', 'inactive', 'wrong-kind', 'invalid-generation'):
        state = copy.deepcopy(valid)
        if mutation == 'missing':
            activation.unlink()
        else:
            if mutation == 'duplicate':
                state['units'].append(copy.deepcopy(state['units'][0]))
            elif mutation == 'inactive':
                state['units'][0]['active'] = False
            elif mutation == 'wrong-kind':
                state['units'][0]['kind'] = 'skill'
            elif mutation == 'invalid-generation':
                state['generationId'] = 'invalid'
            activation.write_text(json.dumps(state))
        exercise('Read', {'file_path': str(sessions / 'x.json')})
        activation.write_text(json.dumps(valid))
    org = scratch / 'org.json'
    org.write_text(json.dumps({'nodes': [{'id': 'lead', 'session': 'fixture-worker', 'role': 'lead'}]}))
    exercise('Read', {'file_path': str(sessions / 'x.json')}, changes={'AGENT_ROLE': '', 'AGENTS_ORG_FILE': str(org)})
    socket_script = f'import socket; s=socket.socket(socket.AF_UNIX); s.connect("{peer}"); s.sendall(b"the cross-session message arrived"); s.close()'
    with socket.socket(socket.AF_UNIX) as server:
        server.bind(str(peer))
        server.listen()
        for tool in ('Bash', 'exec_command'):
            command = 'python3 -c ' + shlex.quote(socket_script)
            inputs = {'command': command} if tool == 'Bash' else json.dumps({'cmd': command, 'workdir': str(home)})
            exercise(tool, inputs, True)
            server.settimeout(0.05)
            try:
                server.accept()
            except TimeoutError:
                pass
            else:
                raise AssertionError('worker refusal reached socket')
            exercise(tool, inputs, changes={'AGENT_ROLE': 'lead'})
            subprocess.run([sys.executable, '-c', socket_script], check=True, env=env)
            server.settimeout(3)
            client, _ = server.accept()
            with client:
                assert client.recv(1024) == b'the cross-session message arrived'
    print(f'session integrity boundary: {decisions} decisions passed; both shell providers denied worker socket writes and delivered sanctioned lead text')
PY
