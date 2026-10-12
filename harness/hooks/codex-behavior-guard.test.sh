#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec python3 - "$HERE/codex-behavior-guard.sh" <<'PY'
import json
import os
import shutil
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

    def event(kind, command, denied=False, cwd=game, session='boundary', tool='Bash', response='Exit code: 0\nOutput:\nok', prompt=None):
        global decisions
        payload = {'hook_event_name': kind, 'session_id': session, 'cwd': str(cwd), 'tool_name': tool, 'tool_input': {'command': command}, 'tool_response': response}
        if prompt is not None:
            payload['prompt'] = prompt
        result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
        decoded = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
        assert (decoded.get('permissionDecision') == 'deny') == denied, (kind, command, result.stdout)
        assert not result.stderr, result.stderr
        decisions += 1
        return decoded

    control = subprocess.run([hook], input=json.dumps({'hook_event_name': 'PreToolUse', 'session_id': 'coach-disabled', 'cwd': str(game), 'tool_name': 'apply_patch', 'tool_input': {}}), text=True, capture_output=True, env=env, check=True)
    assert (control.stdout, control.stderr, control.returncode) == ('', '', 0), control
    coach_root = root / 'coach-state'
    brief = root / 'brief.md'
    brief.write_text(f'Files:\n- {source}\n')
    marker = {'brief': str(brief), 'state': str(coach_root), 'cwd': str(game)}
    event('UserPromptSubmit', '', session='coach-enabled', prompt='COACH BOX: ' + json.dumps(marker))
    held = event('PreToolUse', '', denied=True, session='coach-enabled', tool='apply_patch')
    assert held['permissionDecisionReason'] == 'coach: first edit waits for OK for thread coach-enabled', held
    event('PreToolUse', f'cat {source}', session='coach-enabled')
    event('PreToolUse', f'rg --pre touch pattern {source}', denied=True, session='coach-enabled')
    event('PreToolUse', f'rg --pre=touch pattern {source}', denied=True, session='coach-enabled')
    admission = coach_root / 'coach-enabled.json'
    data = json.loads(admission.read_text())
    data['ok'] = 'a-sibling-thread'
    admission.write_text(json.dumps(data))
    event('PreToolUse', 'printf edit > arc.ts', denied=True, session='coach-enabled')
    data['ok'] = 'coach-enabled'
    admission.write_text(json.dumps(data))
    event('PreToolUse', '', session='coach-enabled', tool='apply_patch')

    refusal = 'Codex cannot land durable harness changes; a Claude Opus reviewer gates them and the lead lands them.'
    for repository, path, denied in (
        ('north', 'harness/skills/example/SKILL.md', True),
        ('North', 'harness/skills/example/SKILL.md', True),
        ('north', 'agent-machinery/example.md', True),
        ('north', 'bin/example', True),
        ('north', 'lib/example.py', True),
        ('north', 'src/example.rs', False),
        ('south', 'agents/routines/tick.md', True),
        ('south', 'hosts/example.nix', False),
        ('firn', 'modules/north-profile/example.nix', True),
        ('firn', 'modules/codex/default.nix', True),
        ('nixos-config', 'modules/job-run/default.nix', True),
        ('firn', 'modules/airplane/default.nix', False),
    ):
        lane = root / 'landing' / repository / path.replace('/', '_')
        lane.mkdir(parents=True)
        subprocess.run(['git', 'init', '-q', '-b', 'main', str(lane)], check=True)
        subprocess.run(['git', '-C', str(lane), 'remote', 'add', 'origin', str(root / f'{repository}.git') + ('/' if repository == 'North' else '')], check=True)
        subprocess.run(['git', '-C', str(lane), '-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '--allow-empty', '-qm', 'init'], check=True)
        subprocess.run(['git', '-C', str(lane), 'update-ref', 'refs/remotes/origin/main', 'HEAD'], check=True)
        target = lane / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text('durable harness\n')
        subprocess.run(['git', '-C', str(lane), 'add', path], check=True)
        subprocess.run(['git', '-C', str(lane), '-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-qm', 'edit'], check=True)
        marker = subprocess.run(['git', '-C', str(lane), 'rev-parse', '--path-format=absolute', '--git-path', 'lead-lands'], capture_output=True, text=True, check=True).stdout.strip()
        Path(marker).touch()
        decoded = event('PreToolUse', 'git push origin HEAD:main', denied=denied, cwd=lane, session='landing')
        if denied:
            assert decoded == {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': refusal}, decoded
        else:
            assert decoded == {}, decoded
        event('PreToolUse', 'git commit -m "edit"', cwd=lane, session='landing')
        event('PreToolUse', 'printf "git push origin HEAD:main"', cwd=lane, session='landing')
        event('PreToolUse', 'env FOO=bar git push origin HEAD:main', denied=denied, cwd=lane, session='landing')
        event('PreToolUse', f'git -C "{lane}" push origin HEAD:main', denied=denied, cwd=root, session='landing')
        event('PreToolUse', f'cd "{lane}" && git push origin HEAD:main', denied=denied, cwd=root, session='landing')
        if repository == 'north' and path == 'harness/skills/example/SKILL.md':
            projection = root / 'projection'
            (projection / 'lib').mkdir(parents=True)
            shutil.copy2(hook, projection / 'codex-behavior-guard.sh')
            shutil.copy2(Path(hook).parent / 'lib/codex_behavior.py', projection / 'lib/codex_behavior.py')
            (projection / 'lib/authoring-killswitch.sh').symlink_to(Path(hook).parent.parent / 'lib/authoring-killswitch.sh')
            (root / 'bin').mkdir()
            (root / 'bin/safe-push').symlink_to(Path(hook).resolve().parents[2] / 'bin/safe-push')
            original_hook, original_path = hook, env['PATH']
            hook = str(projection / 'codex-behavior-guard.sh')
            env['PATH'] = str(root / 'bin') + os.pathsep + original_path
            event('PreToolUse', 'git push origin HEAD:main', denied=True, cwd=lane, session='projection')
            hook, env['PATH'] = original_hook, original_path
            subprocess.run(['git', '-C', str(lane), 'branch', 'durable', 'HEAD'], check=True)
            subprocess.run(['git', '-C', str(lane), 'reset', '--hard', '-q', 'origin/main'], check=True)
            event('PreToolUse', 'git push origin durable:main', denied=True, cwd=lane, session='alternate-source')

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
