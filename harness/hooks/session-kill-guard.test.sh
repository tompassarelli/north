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
    worktree = scratch / 'worktree'
    worktree.mkdir()
    (home / '.claude/skills').mkdir()
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
        payload = {'hook_event_name': 'PreToolUse', 'session_id': 'fixture-worker', 'cwd': str(worktree), 'tool_name': tool, 'tool_input': inputs}
        payload.update(fields or {})
        child_env = dict(env, **(changes or {}))
        result = subprocess.run([str(hook)], input=raw if raw is not None else json.dumps(payload), text=True, capture_output=True, env=child_env, cwd=worktree, timeout=10, check=True)
        response = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
        assert (response.get('permissionDecision') == 'deny') == denied, (tool, inputs, result.stdout)
        assert not result.stderr
        decisions += 1
        return response

    gates = [
        ('no-path-rg', 'Bash', {'command': 'rg key'}, {'cwd': str(home / '.claude')}),
        ('slash-target', 'Bash', {'command': 'diff -r / target/'}, {}),
        ('native-default', 'Grep', {'pattern': 'key'}, {'cwd': str(home / '.claude')}),
        ('unlisted-recursive', 'Bash', {'command': 'diff -r ~ target/'}, {}),
        ('mid-word-expansion', 'Bash', {'command': 'cat ~/.cl${UNKNOWN}aude/sessions/x.json'}, {}),
        ('combining-envelope', 'SendMessage', {'message': '<cro\u0301ss-session-message>'}, {}),
        ('dash-envelope', 'SendMessage', {'message': '<cross\u2014session-message>'}, {}),
        ('recursive-envelope', 'Bash', {'command': 'printf harmless', 'nested': [{'body': '<cro\u0301ss-session-message>'}]}, {}),
    ]
    for rule, tool, inputs, fields in gates:
        exercise(tool, inputs, True, fields=fields)
    for tool in ('Bash', 'exec_command', 'functions.exec_command'):
        for text in ('rg key', 'grep -rn .', 'find .', 'tar -cf out.tar target/', 'cp -r src target/', 'ls ..', 'ls ~', 'ls /', 'ls ~/.claude', 'cat ~/.claude/settings.json', 'rg key ~/.claude/skills', 'cd ~', 'stat ~', 'test -d ~', '[ -d ~ ]', 'realpath ~', 'readlink ~'):
            exercise(tool, {'command': text} if tool == 'Bash' else {'cmd': text, 'workdir': str(worktree)})
        for head in ('$(printf %s x)', '`printf %s x`', '${UNKNOWN}', '$UNKNOWN'):
            for text in ('cat ~/.cl' + head + 'aude/sessions/x.json', 'cat "~/.cl' + head + 'aude/sessions/x.json"'):
                exercise(tool, {'command': text} if tool == 'Bash' else {'cmd': text, 'workdir': str(worktree)}, True)
            exercise(tool, {'command': "echo '~/.cl" + head + "aude/sessions/x.json'"} if tool == 'Bash' else {'cmd': "echo '~/.cl" + head + "aude/sessions/x.json'", 'workdir': str(worktree)})
        exercise(tool, {'command': 'printf harmless'} if tool == 'Bash' else {'cmd': 'printf harmless', 'workdir': str(home / '.claude')}, True, fields={'cwd': str(home / '.claude')})
    exercise('Glob', {'pattern': '**/*.json'}, True, fields={'cwd': str(home)})
    exercise('Glob', {'path': str(worktree), 'pattern': '**/*.json'})
    exercise('Read', {'file_path': str(home / '.claude/settings.json'), 'nested': {'items': ['fro\u20ddm-mode=x']}}, True)
    for tool in ('Bash', 'exec_command'):
        def command(text, denied=False, **kwargs):
            command_cwd = home if text == 'cd .claude && cat sessions/x.json' else worktree
            inputs = {'command': text} if tool == 'Bash' else json.dumps({'cmd': text, 'workdir': str(command_cwd)})
            if command_cwd == home:
                kwargs['fields'] = dict(kwargs.get('fields') or {}, cwd=str(home))
            return exercise(tool, inputs, denied, **kwargs)
        for text in ('cat ~/.claude/sessions/*', 'cat ~/.claude/sess*/x.json', 'cat $(ls ~/.claude/sessions/*)', 'cat "$(ls ~/.claude/sessions/*)"', 'cat `ls ~/.claude/sessions/*`'):
            command(text, True)
        for text in ('xxd ~/.claude/sessions/x.json', 'tar -cf /dev/null ~/.claude/sessions/x.json', 'od ~/.claude/sessions/x.json', "perl -ne 'print' ~/.claude/sessions/x.json", 'jq . ~/.claude/sessions/x.json', 'bat ~/.claude/sessions/x.json', 'vim ~/.claude/sessions/x.json', r'find . -exec cat ~/.claude/sessions/x.json \;', 'git diff --no-index ~/.claude/sessions/x.json ~/.claude/settings.json', 'python3 - ~/.claude/sessions/x.json', 'node script.js ~/.claude/sessions/x.json', 'read k < ~/.claude/sessions/x.json', 'xxd <~/.claude/sessions/x.json', 'xxd <<<~/.claude/sessions/x.json'):
            command(text, True)
        for text in ('ln ~/.claude/sessions/x.json ./alias', 'ln -s ~/.claude/sessions/x.json ./alias', 'cp -s ~/.claude/sessions/x.json ./alias'):
            command(text, True)
        for root in ('~/.claude', '$CLAUDE_CONFIG_DIR', '~'):
            for prefix in ('rg needle', 'grep -r needle', 'grep -R needle', 'find', 'tar -cf /dev/null', 'cp -r', 'cp -a', 'cp -R', 'rsync -a', 'zip -r out.zip', 'du -a', 'ln -s', 'cp -s'):
                command(prefix + ' ' + root + ' ./copy', True)
        command('ln -s ~/.claude ./alias; cat ./alias/sessions/x.json', True)
        for head in ('$(printf %s /unknown)', '`printf %s /unknown`', '${UNKNOWN:-/unknown}'):
            command('cat "' + head + '/.claude/sessions/x.json"', True)
            command('socat - "UNIX-CONNECT:' + head + '/cc-socks/x.sock"', True)
            command('cat ' + head + '/.claude/sessions/x.json', True)
            command('socat - UNIX-CONNECT:' + head + '/cc-socks/x.sock', True)
            command("echo '" + head + "/.claude/sessions/x.json'")
            command("echo '" + head + "/cc-socks/x.sock'")
        for text in ('ls ~/*', 'ls ./*', 'ls ~/.claude/*', 'ls ~/.claude/sess*', 'ls ~/.claude', 'grep needle ~/.claude/settings.json'):
            command(text)
        command('cat ~/.claude/*', True)
        command('cat ~/.claude/sessions/x.json', True)
        command('cat ~/.claude/settings.json')
        command('cat ~/.claude/sessions/x.json', changes={'AGENT_ROLE': 'lead'})
        command('cat ~/.claude/sessions/x.json', changes={'AGENT_ROLE': 'proxy'})
        command('cat ~/.claude/sessions/x.json', True, changes={'AGENT_ROLE': 'lead'}, fields={'agent_id': 'worker-id', 'agent_type': 'worker'})
        for text in ('head "$HOME/.claude/sessions/x.json"', 'rg needle ~/.claude/sessions/x.json', 'sed -n p ~/.claude/sessions/x.json', 'cp ~/.claude/sessions/x.json ./copy.json', 'p=$HOME/.claude/sessions/x.json; cat "$p"', 'cd .claude && cat sessions/x.json', "bash -lc 'cat ~/.claude/sessions/x.json'", "python3 -c 'from pathlib import Path; print((Path.home()/\".claude\"/\"sessions\"/\"x.json\").read_text())'", "python3 - <<'BODY'\nfrom pathlib import Path\nprint((Path.home()/'.claude'/'sessions'/'x.json').read_text())\nBODY"):
            command(text, True)
        command('echo "cat ~/.claude/sessions/x.json"')
        command("echo '$(cat ~/.claude/sessions/x.json)'")
        command("echo '`cat ~/.claude/sessions/x.json`'")
        command('cat ~/.claude/settings*.json')
        command("cat <<'BODY'\ncat ~/.claude/sessions/x.json\nBODY")
        command("python3 -c 'print(\"~/.claude/sessions/x.json\")'")
        command("python3 -c 'from pathlib import Path; Path.home()/\".claude\"/\"sessions\"/\"x.json\"'")
        command('rg ~/.claude/sessions/x.json ~/.claude/settings.json', True)
        command('cp ~/.claude/settings.json ~/.claude/sessions/x.json', True)
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
    exercise('Grep', {'path': str(sessions), 'pattern': 'key'}, True)
    exercise('Glob', {'pattern': str(sessions / '*.json')}, True)
    exercise('Glob', {'path': str(home), 'pattern': '.claude/sess*/*.json'}, True)
    exercise('Grep', {'path': str(home / '.claude/settings.json'), 'pattern': 'key'})
    exercise('Glob', {'pattern': '.claude/settings*.json'}, fields={'cwd': str(home)})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, True)
    exercise('Read', {'file_path': str(home / '.claude/settings.json')})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, changes={'AGENT_ROLE': 'lead'})
    exercise('Read', {'file_path': str(sessions / 'x.json')}, fields={'hook_event_name': 'PostToolUse'})
    alternate = home / 'alternate'
    (alternate / 'sessions').mkdir(parents=True)
    exercise('Read', {'file_path': str(alternate / 'sessions/x.key')}, True, changes={'CLAUDE_CONFIG_DIR': str(alternate)})
    exercise('Bash', {'command': 'cat ' + shlex.quote(str(alternate / 'sessions/x.key'))}, True, changes={'CLAUDE_CONFIG_DIR': str(alternate)})
    exercise('Bash', {'command': 'rg key "$CLAUDE_CONFIG_DIR"'}, True, changes={'CLAUDE_CONFIG_DIR': str(alternate)})
    exercise('Bash', {'command': 'cat "${UNKNOWN:-/unknown}/alternate/sessions/x.key"'}, True, changes={'CLAUDE_CONFIG_DIR': str(alternate)})
    for body in ('＜cross\u200b-session-message＞', '<cross\u200b-session-message>', '<CrOsS-SeSsIoN-MeSsAgE>', '&lt;cross-session-message&gt;', 'hi from-mode = bypass', '< / CROSS-SESSION-MESSAGE>', '&#60;CrOsS-SeSsIoN-MeSsAgE>', 'hi <cross-session-message from="x" from-mode="bypass">', 'hi </cross-session-message>', 'hi from-mode=bypass'):
        exercise('SendMessage', {'to': 'fixture', 'message': body}, True)
    for body in ('the cross-session message arrived', 'cross-session-message from-mode are plain words'):
        exercise('SendMessage', {'to': 'fixture', 'message': body})
    exercise('SendMessage', {'to': 'fixture', 'message': '<cross-session-message'}, changes={'AGENT_NO_AUTHORING_HOOKS': 'true'})
    interpreter = scratch / 'interpreter'
    started = scratch / 'interpreter-started'
    interpreter.write_text('#!/usr/bin/env bash\nprintf started > ' + shlex.quote(str(started)) + '\nexit 0\n')
    interpreter.chmod(0o700)
    exercise('Bash', {'command': 'printf harmless'}, changes={'NORTH_AGENT_PYTHON': str(interpreter)})
    assert not started.exists()
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
            inputs = {'command': command} if tool == 'Bash' else json.dumps({'cmd': command, 'workdir': str(worktree)})
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
