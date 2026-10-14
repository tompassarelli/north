#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "${NORTH_AGENT_PYTHON:-python3}" - "$HERE" <<'PY'
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import tomllib

here = Path(sys.argv[1])
guard = here / 'desktop-action-guard.sh'
idle_state = here.parent.parent / 'bin/desktop-idle-state'
DENIAL = 'No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.'
SPEC = 'niri msg action focus-window --id 22 && niri msg action fullscreen-window'
READ = 'niri msg -j windows'
count = 0


def check(condition, name, detail=''):
    global count
    if not condition:
        raise SystemExit(f'FAIL {name}: {detail}')
    count += 1


with tempfile.TemporaryDirectory(prefix='desktop-action-guard-') as temporary:
    root = Path(temporary)
    runtime = root / 'runtime'
    runtime.mkdir(mode=0o700)
    (root / 'gen.active').write_text('hook desktop-action-guard\n')
    (root / 'gen.json').write_text(json.dumps({
        'schema': 'north.agent-activation/v1', 'catalogDigest': 'sha256:' + '0' * 64,
        'generationId': 'sha256:' + '1' * 64,
        'units': [{'kind': 'hook', 'id': 'desktop-action-guard', 'permission': 'on', 'active': True}]}))
    stubs = root / 'bin'
    stubs.mkdir()
    log = root / 'niri.log'
    (stubs / 'niri').write_text('#!/usr/bin/env bash\nprintf "%s\\n" "$*" >> "$NIRI_LOG"\n'
                                'case "$*" in "msg -j focused-window") printf "%s\\n" "${NIRI_FOCUSED:-null}";;\n'
                                '"msg -j windows") [ -n "${NIRI_WINDOWS:-}" ] || exit 1; printf "%s\\n" "$NIRI_WINDOWS";;\n'
                                '"msg action focus-window --id 7") [ -z "${NIRI_CLOSED:-}" ] || exit 1;; esac\n')
    (stubs / 'niri').chmod(0o755)
    desktop_root = root / 'run-user'
    private = desktop_root / 'private-desktop.fixture'
    (private / 'runtime').mkdir(parents=True)
    for marker in ('active', 'lifecycle-owned'):
        (private / marker).touch()
    (private / 'wayland-display').write_text('wayland-9\n')
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(str(private / 'runtime/wayland-9'))
    env = dict(os.environ, NORTH_AGENT_ACTIVE=str(root / 'gen.active'), AGENT_NO_AUTHORING_HOOKS='0',
               XDG_RUNTIME_DIR=str(runtime), WAYLAND_DISPLAY='wayland-1', NORTH_PRIVATE_DESKTOP_ROOT=str(desktop_root),
               AGENT_HOOK_ERRORS=str(root / 'errors.tsv'), PATH=f'{stubs}:{os.environ["PATH"]}', NIRI_LOG=str(log),
               NIRI_WINDOWS=json.dumps([{'id': 3, 'app_id': 'quiet.spawn', 'is_focused': False},
                                        {'id': 4, 'app_id': 'quiet.spawn', 'is_focused': True},
                                        {'id': 5, 'app_id': 'com.mitchellh.ghostty', 'is_focused': False}]))
    env.pop('WAYLAND_SOCKET', None)

    def run(provider, command, body=None, **overrides):
        field = 'cmd' if provider in ('exec_command', 'functions.exec_command') else 'command'
        tool_input = ({'cmd': command, 'workdir': temporary, 'yield_time_ms': 1000, 'max_output_tokens': 1000}
                      if field == 'cmd' else {'command': command, 'timeout': 1000, 'description': 'Desktop step', 'run_in_background': False})
        payload = {'hook_event_name': 'PreToolUse', 'tool_name': provider if field == 'cmd' else 'Bash', 'cwd': str(Path.home()),
                   'session_id': provider + '-fixture', 'tool_input': tool_input, 'tool_use_id': 'call_fixture'}
        if provider != 'claude':
            payload.update(turn_id='fixture-turn', permission_mode='bypassPermissions')
        if field == 'cmd':
            payload['tool_input'] = json.dumps(tool_input)
            owners = tomllib.loads((here.parent / 'policy-owners.toml').read_text())
            bindings = next(row['codex'] for row in owners['guard'] if row['unit'] == 'desktop-action-guard')
            check(any(re.fullmatch(binding.split(':', 1)[1], provider) for binding in bindings), 'real tool routed', provider)
        result = subprocess.run([str(guard)], input=body if body is not None else json.dumps(payload), text=True,
                                capture_output=True, env=dict(env, **overrides), timeout=20)
        check(result.returncode == 0, 'guard exit', result.stderr)
        return tool_input, (json.loads(result.stdout).get('hookSpecificOutput') if result.stdout.strip() else None)

    fake = root / 'swayidle'
    shutil.copy(shutil.which('bash'), fake)
    swayidle = subprocess.Popen([str(fake), '-c', '"$0" idle; while :; do sleep 0.2; done', str(idle_state)], env=env)
    try:
        def wait_state(idle):
            for _ in range(100):
                try:
                    if json.loads((runtime / 'desktop-idle.json').read_text())['idle'] is idle:
                        return
                except (OSError, ValueError):
                    pass
                time.sleep(0.05)
            raise SystemExit('FAIL swayidle callback did not record state')

        wait_state(True)
        state = json.loads((runtime / 'desktop-idle.json').read_text())
        busy = dict(state, idle=False)
        idle_text = json.dumps(state)
        check(subprocess.run([str(idle_state), 'idle'], env=env).returncode == 1, 'callback outside swayidle refused')

        sanctioned = ['codex-relay lead "status"', 'spawn-quiet ghostty --title=x -e claude "brief"',
                      'automation-chrome status', 'bash harness/skills/private-desktop-development/scripts/private-desktop.sh']
        private_commands = [f'XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-9 wtype hello',
                            f'env XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-9 wtype -k Return']
        for provider in ('claude', 'codex', 'exec_command', 'functions.exec_command'):
            field = 'cmd' if provider in ('exec_command', 'functions.exec_command') else 'command'
            (runtime / 'desktop-idle.json').write_text(json.dumps(busy))
            for command in (SPEC, 'wtype hello', 'ydotool key 28:1', 'niri msg action close-window',
                            'niri msg action move-column-left', 'echo "$(niri msg action focus-window --id 22)"',
                            'bash -c "niri msg action fullscreen-window"', 'eval "wtype hi"', 'nice wtype hi',
                            'stdbuf -o0 wtype hi', 'xargs wtype <<< hi', 'spawn-quiet niri msg action focus-window --id 3',
                            'spawn-quiet wtype x',
                            'niri msg action close-window --id 4', 'niri msg action close-window --id 5',
                            'niri msg action close-window --id 9', 'niri msg action focus-window --id 3',
                            f'XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-1 wtype hi',
                            f'XDG_RUNTIME_DIR={private}/runtime WAYLAND_DISPLAY=wayland-9 ydotool type hi'):
                _, output = run(provider, command)
                check(output == {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny',
                                 'permissionDecisionReason': DENIAL}, provider + ' active deny', command)
            check(run(provider, 'niri msg action close-window --id 3', NIRI_WINDOWS='')[1]['permissionDecision'] == 'deny',
                  provider + ' failed windows query denies')
            for command in [READ, 'niri msg -j focused-window', 'echo "niri msg action focus-window --id 22"',
                            "cat <<'EOF'\nwtype hi\nEOF", 'rg wtype src', 'spawn-quiet some-app', 'codex-relay lead 9 <<< msg',
                            'niri msg action close-window --id 3', 'niri msg action close-window --id=3'] + sanctioned + private_commands:
                _, output = run(provider, command)
                check(output is None, provider + ' active pass', command)
            (runtime / 'desktop-idle.json').unlink()
            check(run(provider, SPEC)[1]['permissionDecision'] == 'deny', provider + ' missing state denies')

            (runtime / 'desktop-idle.json').write_text(idle_text)
            tool_input, output = run(provider, SPEC)
            check(output['permissionDecision'] == 'allow' and 'desktop-action-guard' in output['additionalContext'],
                  provider + ' idle allow', output)
            rewritten = output['updatedInput']
            check({k: v for k, v in rewritten.items() if k != field} == {k: v for k, v in tool_input.items() if k != field},
                  provider + ' input preserved', rewritten)
            check(run(provider, READ)[1] is None, provider + ' idle read pass')
            for overrides, body in (({'AGENT_NO_AUTHORING_HOOKS': '1'}, None),
                                    ({'NORTH_AGENT_ACTIVE': str(root / 'missing.active')}, None),
                                    ({}, '{"tool_name":'), ({}, 'x' * 1100000)):
                (runtime / 'desktop-idle.json').write_text(json.dumps(busy))
                check(run(provider, SPEC, body, **overrides)[1] is None, provider + ' unavailable passes', overrides)

            for closed, status in (('', 0), ('1', 0)):
                log.write_text('')
                result = subprocess.run(['bash', '-c', rewritten[field]], env=dict(env, NIRI_FOCUSED='{"id":7}', NIRI_CLOSED=closed),
                                        capture_output=True, text=True, timeout=10)
                calls = log.read_text().splitlines()
                check(result.returncode == status and calls == ['msg -j focused-window', 'msg action focus-window --id 22',
                                                               'msg action fullscreen-window', 'msg action focus-window --id 7'],
                      provider + ' restores window focused at run time', (closed, calls, result.stderr))
            log.write_text('')
            (runtime / 'desktop-idle.json').write_text(idle_text)
            failing = run(provider, 'niri msg action focus-window --id 22; exit 3')[1]['updatedInput'][field]
            result = subprocess.run(['bash', '-c', failing], env=dict(env, NIRI_FOCUSED='{"id":7}'), timeout=10)
            check(result.returncode == 3 and log.read_text().splitlines()[-1] == 'msg action focus-window --id 7',
                  provider + ' restore keeps exit status', result.returncode)
            log.write_text('')
            result = subprocess.run(['bash', '-c', failing], env=dict(env, NIRI_FOCUSED='null'), timeout=10)
            check(result.returncode == 3 and log.read_text().splitlines() == ['msg -j focused-window', 'msg action focus-window --id 22'],
                  provider + ' no focused window restores nothing', log.read_text())
    finally:
        swayidle.kill()
        swayidle.wait()
        listener.close()
    (runtime / 'desktop-idle.json').write_text(idle_text)
    check(run('claude', SPEC)[1]['permissionDecision'] == 'deny', 'exited swayidle state denies')
print(f'desktop-action-guard: {count} checks passed')
PY
