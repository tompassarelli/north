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

    def event(kind, command, denied=False, cwd=game, session='boundary', tool='Bash', response='Exit code: 0\nOutput:\nok', prompt=None, tool_input=None, hook_cwd=None, hook_path=hook, tool_use_id=None):
        global decisions
        payload = {'hook_event_name': kind, 'session_id': session, 'cwd': str(cwd), 'tool_name': tool, 'tool_input': {'command': command} if tool_input is None else tool_input, 'tool_response': response}
        if prompt is not None:
            payload['prompt'] = prompt
        if tool_use_id is not None:
            payload['tool_use_id'] = tool_use_id
        result = subprocess.run([hook_path], input=json.dumps(payload), text=True, capture_output=True, env=env, cwd=hook_cwd, check=True, timeout=30)
        decoded = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
        assert (decoded.get('permissionDecision') == 'deny') == denied, (kind, command, result.stdout)
        assert not result.stderr, result.stderr
        decisions += 1
        return decoded


    control = subprocess.run([hook], input=json.dumps({'hook_event_name': 'PreToolUse', 'session_id': 'plain-codex', 'cwd': str(game), 'tool_name': 'apply_patch', 'tool_input': {}}), text=True, capture_output=True, env=env, check=True)
    assert (control.stdout, control.stderr, control.returncode) == ('', '', 0), control
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

    def stop(message, session):
        payload = {'hook_event_name': 'Stop', 'session_id': session, 'stop_hook_active': False, 'last_assistant_message': message}
        result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
        return json.loads(result.stdout or '{}').get('decision') == 'block'

    source.write_text('c\n')
    event('PostToolUse', 'bun test -t arc', session='land')
    assert stop('Tests pass on the arc fix.', 'land')
    assert stop('Done: tests pass on the arc fix; land the change with safe-push --to main.', 'land')
    def test_mid_box_codex_turn_gets_no_stop_nudge():
        event('UserPromptSubmit', '', session='mid-box', prompt='Read and do the box brief at brief-desktop-userns.md')
        assert not stop("I've created the private-desktop-userns worktree from origin/main. The fix is still outstanding; I haven't yet tested or landed it.", 'mid-box')
    test_mid_box_codex_turn_gets_no_stop_nudge()
    assert not stop('Not done: Stop: capacity. Checkpoint saved; the parent owns the exclusive-capacity dependency.', 'dependency-checkpoint')

    event('PostToolUse', '', session='orchestrator', tool='collaborationspawn_agent', response='{}')
    for command in (
        "python3 - <<'PY'\nfrom pathlib import Path\nPath('status.md').write_text('passed; bun test; cat worker.log')\nPY",
        "cat <<'REPORT' > status.md\npassed; bun test; cat worker.log\nREPORT",
    ):
        event('PreToolUse', command, session='orchestrator')
        event('PostToolUse', command, session='orchestrator')
        event('PreToolUse', command, session='orchestrator')
        event('PreToolUse', command + '\nbun test', denied=True, session='orchestrator')
    event('PreToolUse', 'bun test', denied=True, session='orchestrator')
    event('PreToolUse', 'cat worker.log', denied=True, session='orchestrator')
    event('PreToolUse', 'bun scripts/machine-capacity.mjs estimate --json -- cargo build --release', session='orchestrator')
    event('PreToolUse', 'bun scripts/machine-capacity.mjs run --owner x --timeout-seconds 60 -- cargo build --release', denied=True, session='orchestrator')
    gate_hooks = root / 'gate-hooks'
    (gate_hooks / 'lib').mkdir(parents=True)
    shutil.copy2(hook, gate_hooks / 'codex-behavior-guard.sh')
    for name in ('codex_behavior.py', 'shellcmd.py'):
        shutil.copy2(Path(hook).parent / 'lib' / name, gate_hooks / 'lib' / name)
    (gate_hooks / 'lib/authoring-killswitch.sh').symlink_to(Path(hook).parent.parent / 'lib/authoring-killswitch.sh')
    (gate_hooks / 'lib/spawn_capacity.py').write_text(
        'import sys\n'
        'def evaluate(event):\n    return None, None\n'
        f'def north_lib():\n    sys.path.insert(0, {str(Path(hook).resolve().parents[2] / "harness/lib")!r})\n'
    )
    gate_hook = str(gate_hooks / 'codex-behavior-guard.sh')
    env['XDG_STATE_HOME'] = str(root / 'gate-xdg')
    (root / 'gate-xdg/agents').mkdir(parents=True)

    def tooling_box(surface):
        return f'Item: north#51\nCategory: tooling\nSpec: exact\nScope: module\nSurface: {surface}\nVerify: local-test\nFiles: harness/hooks/lib/codex_behavior.py\nETA: 20 min\nDone when: `bash harness/hooks/codex-behavior-guard.test.sh` passes\n\nThe goal in full sentences.'

    def spawn(task, message, denied):
        return event('PreToolUse', '', denied=denied, session='gate-lead', tool='spawn_agent', tool_input={'message': message, 'task_name': task}, hook_path=gate_hook, tool_use_id='call-' + task)

    refused = spawn('gate_ts', tooling_box('ts'), True)
    assert 'route to Claude: Surface ts' in refused['permissionDecisionReason'], refused
    spawn('gate_eligible', tooling_box('shell'), False)
    held = {'claude': {'paces': {'claude:a': {'state': 'at or over pace'}, 'claude:b': {'state': 'at or over pace'}}}}
    (root / 'gate-xdg/agents/usage-gate.json').write_text(json.dumps(held))
    cross = tooling_box('ts').replace('Scope: module', 'Scope: cross-module')
    refused = spawn('gate_overflow_unqualified', cross, True)
    assert 'route to Claude: Scope cross-module' in refused['permissionDecisionReason'], refused
    held['claude']['paces']['claude:b'] = {'state': 'under pace'}
    (root / 'gate-xdg/agents/usage-gate.json').write_text(json.dumps(held))
    refused = spawn('gate_overflow_with_claude', cross, True)
    assert 'route to Claude: Scope cross-module' in refused['permissionDecisionReason'], refused
    (root / 'gate-xdg/agents/usage-gate.json').unlink()
    refused = spawn('gate_ts_again', tooling_box('ts'), True)
    assert 'route to Claude: Surface ts' in refused['permissionDecisionReason'], refused
    balance = tooling_box('assets').replace('Category: tooling', 'Category: balance-tuning').replace('Verify: local-test', 'Verify: visual')
    refused = spawn('gate_balance', balance, True)
    assert 'route to Claude: Category balance-tuning' in refused['permissionDecisionReason'], refused
    explored = balance.replace('Item: north#51\n', 'Item: north#51\nExplore: codex\n')
    spawn('gate_explore', explored, False)
    plan_review = 'Item: north#51\nCategory: review\nReviews: claude (opus-high netcode plan)\nSpec: judged\nScope: cross-module\nSurface: lua\nVerify: none\n\nBreak this netcode plan.\n'
    spawn('gate_review_claude_plan', plan_review, False)
    refused = spawn('gate_review_own_family', plan_review.replace('Reviews: claude (opus-high netcode plan)', 'Reviews: codex'), True)
    assert 'route to Claude: Reviews codex' in refused['permissionDecisionReason'], refused
    refused = spawn('gate_review_unnamed', plan_review.replace('Reviews: claude (opus-high netcode plan)\n', ''), True)
    assert 'route to Claude: Reviews missing' in refused['permissionDecisionReason'], refused
    refused = spawn('gate_explore_lua', explored.replace('Surface: assets', 'Surface: lua'), True)
    assert 'route to Claude: Surface lua' in refused['permissionDecisionReason'], refused
    spawn('gate_delegate', 'Run the entire operator task: fix the failing build and report.', False)
    spawn('gate_probe', 'echo_probe', False)
    print('PASS Codex spawn gate routes unqualified boxes to Claude, admits qualified Codex sessions without a coach, admits a Category: review box only for a reviewer family other than its Reviews author, and leaves header-less spawns alone')
    print(f'behavior Git/process boundary: {decisions} decisions passed')
PY
