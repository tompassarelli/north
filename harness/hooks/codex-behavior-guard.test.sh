#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec python3 - "$HERE/codex-behavior-guard.sh" <<'PY'
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

hook = sys.argv[1]
with tempfile.TemporaryDirectory(prefix='behavior-boundary-') as temporary:
    root = Path(temporary)
    game = root / 'code/smashcraft/main'
    game.mkdir(parents=True)
    subprocess.run(['git', 'init', '-q', str(game)], check=True)
    source = game / 'arc.ts'
    source.write_text('a\n')
    subprocess.run(['git', '-C', str(game), 'add', 'arc.ts'], check=True)
    subprocess.run(['git', '-C', str(game), '-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-qm', 'init'], check=True)
    state = root / 'state'
    env = dict(os.environ, CODEX_BEHAVIOR_CODE_ROOT=str(root / 'code'), CODEX_BEHAVIOR_STATE=str(state), AGENT_NO_AUTHORING_HOOKS='0', AGENT_HOOK_ERRORS=str(root / 'errors.tsv'))
    decisions = 0

    def event(kind, command, denied=False, cwd=game, session='boundary', tool='Bash', response='Exit code: 0\nOutput:\nok'):
        global decisions
        payload = {'hook_event_name': kind, 'session_id': session, 'cwd': str(cwd), 'tool_name': tool, 'tool_input': {'command': command}, 'tool_response': response}
        result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
        decoded = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
        assert (decoded.get('permissionDecision') == 'deny') == denied, (kind, command, result.stdout)
        assert not result.stderr, result.stderr
        decisions += 1
        return decoded

    for ask in ('request_user_input_async', 'request_user_input'):
        assert event('PreToolUse', '', denied=True, tool=ask)['permissionDecisionReason'] == 'write QUESTION to your status file; your parent lead answers'
    event('PreToolUse', 'bun test')
    event('PostToolUse', 'bun test')
    event('PreToolUse', 'bun test', denied=True)
    source.write_text('b\n')
    event('PreToolUse', 'bun test')
    event('PostToolUse', 'bun test', response='Exit code: 1\nOutput:\nfailed')
    event('PreToolUse', 'bun test')

    farm = root / 'code/wisp/main'
    farm.mkdir(parents=True)
    subprocess.run(['git', 'init', '-q', str(farm)], check=True)
    (farm / 'a.ts').write_text('a\n')
    subprocess.run(['git', '-C', str(farm), 'add', 'a.ts'], check=True)
    subprocess.run(['git', '-C', str(farm), '-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-qm', 'init'], check=True)
    event('PreToolUse', 'bun wisp farm balance "wren expert 120" --wait', cwd=farm, session='first')
    event('PostToolUse', 'bun wisp farm balance "wren expert 120" --wait', cwd=farm, session='first')
    event('PreToolUse', 'cd ts && bun wisp farm balance "wren expert 120"', denied=True, cwd=farm, session='second')
    event('PreToolUse', 'bun wisp farm balance "wren expert 40" --wait', cwd=farm, session='second')

    suite = root / 'suite/ts'
    suite.mkdir(parents=True)
    other = root / 'other'
    other.mkdir()
    process = subprocess.Popen(['bash', '-c', 'printf "ready\\n"; read -r release', 'bun', 'scripts/lua-tests.ts'], cwd=suite, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    try:
        assert process.stdout.readline() == 'ready\n'
        event('PreToolUse', 'cd ts && bun scripts/lua-tests.ts', denied=True, cwd=suite.parent, session='process')
        event('PreToolUse', 'bun scripts/lua-tests.ts', cwd=other, session='process')
        event('PreToolUse', 'pkill -f scripts/lua-tests.ts', cwd=suite.parent, session='process')
    finally:
        process.communicate('release\n', timeout=5)
    assert process.returncode == 0

    relay = state / 'relay-relay.txt'
    relay.write_text('Workers blocked on native startup retry now.')
    delivered = event('PostToolUse', '', session='relay', tool='collaborationsend_message')
    assert delivered.get('additionalContext') and not relay.exists()
    again = event('PostToolUse', '', session='relay', tool='collaborationsend_message')
    assert not again.get('additionalContext')
    def stop(message, session):
        payload = {'hook_event_name': 'Stop', 'session_id': session, 'stop_hook_active': False, 'last_assistant_message': message}
        result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
        return json.loads(result.stdout or '{}').get('decision') == 'block'

    source.write_text('c\n')
    event('PostToolUse', 'bun test -t arc', session='land')
    assert stop('Tests pass on the arc fix.', 'land')
    marker = subprocess.run(['git', '-C', str(game), 'rev-parse', '--path-format=absolute', '--git-path', 'lead-lands'], text=True, capture_output=True, check=True).stdout.strip()
    Path(marker).touch()
    assert not stop('Done: tests pass on the arc fix; the lead lands it.', 'land')
    Path(marker).unlink()

    event('PostToolUse', '', session='orchestrator', tool='collaborationspawn_agent', response='{}')
    event('PreToolUse', 'bun scripts/machine-capacity.mjs estimate --json -- cargo build --release', session='orchestrator')
    event('PreToolUse', 'bun scripts/machine-capacity.mjs run --owner x --timeout-seconds 60 -- cargo build --release', denied=True, session='orchestrator')
    print(f'behavior Git/ledger/process/relay boundary: {decisions} decisions passed')
PY
