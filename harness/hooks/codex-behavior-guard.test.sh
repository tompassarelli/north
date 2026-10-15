#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec python3 - "$HERE/codex-behavior-guard.sh" <<'PY'
import json
import hashlib
import time
import os
import shutil
import sqlite3
import runpy
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

    pooled = root / 'pooled'
    (pooled / 'sqlite').mkdir(parents=True)
    created = 0
    with sqlite3.connect(pooled / 'sqlite/state_5.sqlite') as db:
        db.execute("CREATE TABLE threads(id TEXT,created_at_ms INTEGER,model TEXT,reasoning_effort TEXT,agent_path TEXT,archived INTEGER)")
        db.execute("CREATE TABLE thread_spawn_edges(parent_thread_id TEXT,child_thread_id TEXT,status TEXT)")
        db.execute("INSERT INTO threads VALUES('native-lead',0,'gpt-6.1-sol','medium',NULL,0)")
        db.execute("INSERT INTO threads VALUES('native-child',?,'gpt-6.1-sol','medium','/root/creation',0)", (created,))
        db.execute("INSERT INTO thread_spawn_edges VALUES('native-lead','native-child','open')")
        db.execute("INSERT INTO threads VALUES('native-nonprimary',0,'gpt-6.1-sol','medium',NULL,0)")
        db.execute("INSERT INTO threads VALUES('native-archived',0,'gpt-6.1-sol','medium',NULL,1)")
        db.execute("INSERT INTO thread_spawn_edges VALUES('native-lead','native-nonprimary','closed')")
    env.update(NORTH_CODEX_POOLED_HOME=str(pooled), THREADS_DB=str(root / 'runs.db'))
    generated = root / 'generation/providers/codex/hooks'
    generated.mkdir(parents=True)
    shutil.copy2(hook, generated / Path(hook).name)
    shutil.copytree(Path(hook).parent / 'lib', generated / 'lib')
    for name in ('authoring-killswitch.sh', 'north-agent-activation.sh'):
        shutil.copy2(Path(hook).parent.parent / 'lib' / name, generated / 'lib' / name)
    runtime_bin = root / 'runtime-bin'
    runtime_bin.mkdir()
    installed_threads = Path(hook).resolve().parents[2] / 'bin/threads'
    (runtime_bin / 'threads').symlink_to(installed_threads)
    runtime_path = env['PATH']
    env['PATH'] = str(runtime_bin) + os.pathsep + runtime_path
    generated_hook = str(generated / Path(hook).name)
    brief = 'Item: north#35\nCategory: bug-known-cause\nSpec: measured\nScope: cross-module\nSurface: shell\nVerify: native\nExact decision text beyond metadata.'
    brief_path = root / 'original-brief.txt'
    brief_path.write_text(brief)
    state.mkdir(parents=True, exist_ok=True)
    for unresolved in ('native-nonprimary', 'native-archived', 'native-unknown'):
        refused = subprocess.run([str(installed_threads), 'register-spawn', '--parent', unresolved, '--task', 'creation', '--brief-file', str(brief_path)], env=env, text=True, capture_output=True)
        assert refused.returncode != 0 and 'spawn parent has no native task path' in refused.stderr, refused
    key = 'spawn-brief-' + hashlib.sha256(json.dumps(['native-lead', 'creation']).encode()).hexdigest()
    obstruction = state / (key + '.tmp')
    obstruction.mkdir()
    refused = subprocess.run([str(installed_threads), 'register-spawn', '--parent', 'native-lead', '--task', 'creation', '--brief-file', str(brief_path)], env=env, text=True, capture_output=True)
    assert refused.returncode != 0 and str(obstruction) in refused.stderr, refused
    assert not (state / (key + '.json')).exists()
    optional = state / 'native-lead.tmp'
    optional.mkdir()
    failed = event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': 'encrypted-opaque-spawn', 'task_name': 'creation'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    assert failed['additionalContext'] == 'spawn recording boundary miss for lead native-lead: IsADirectoryError', failed
    with sqlite3.connect(root / 'runs.db') as db:
        assert db.execute('SELECT count(*) FROM runs').fetchone()[0] == 0
    assert not (state / 'native-lead.json').exists()
    optional.rmdir()
    obstruction.rmdir()
    subprocess.run([str(installed_threads), 'register-spawn', '--parent', 'native-lead', '--task', 'creation', '--brief-file', str(brief_path)], env=env, check=True)
    created = time.time_ns() // 1000000
    with sqlite3.connect(pooled / 'sqlite/state_5.sqlite') as db:
        db.execute("UPDATE threads SET created_at_ms=? WHERE id='native-child'", (created,))
    obstruction.mkdir()
    failed = event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': 'encrypted-opaque-spawn', 'task_name': 'creation'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    assert failed['additionalContext'] == 'spawn recording boundary miss for lead native-lead: IsADirectoryError', failed
    assert 'child' not in json.loads((state / (key + '.json')).read_text())['registration']
    with sqlite3.connect(root / 'runs.db') as db:
        assert db.execute('SELECT count(*) FROM runs').fetchone()[0] == 0
    obstruction.rmdir()
    event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': 'encrypted-opaque-spawn', 'task_name': 'creation'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    with sqlite3.connect(root / 'runs.db') as db:
        row = db.execute("SELECT agent,session,category,spec,scope,surface,verify,outcome,brief_text,brief_origin,tier,model,effort,role,staffed,started,brief_path,brief_sha256 FROM runs").fetchone()
    from datetime import datetime, timezone
    staffed = datetime.fromtimestamp(created / 1000, timezone.utc).isoformat().replace('+00:00', 'Z')
    assert row[:-2] == ('native-child','native-lead','bug-known-cause','measured','cross-module','shell','native','running',brief,'codex.PostToolUse.spawn_agent','gpt-6.1-sol medium','gpt-6.1-sol','medium','worker',staffed,staffed), row
    assert row[-2:] == (str(brief_path), hashlib.sha256(brief.encode()).hexdigest()), row
    brief_path.write_text(brief + 'changed')
    changed = event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': 'encrypted-opaque-spawn', 'task_name': 'creation'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    assert changed['additionalContext'].endswith(': ValueError'), changed
    brief_path.write_text(brief)
    absent = event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': 'encrypted-opaque-spawn', 'task_name': 'unregistered'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    assert absent['additionalContext'].endswith(': ValueError'), absent
    with sqlite3.connect(pooled / 'sqlite/state_5.sqlite') as db:
        db.execute("UPDATE threads SET created_at_ms=0 WHERE id='native-child'")
    stale = event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': 'encrypted-opaque-spawn', 'task_name': 'creation'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    assert stale['additionalContext'].endswith(': ValueError'), stale
    with sqlite3.connect(pooled / 'sqlite/state_5.sqlite') as db:
        db.execute("UPDATE threads SET created_at_ms=? WHERE id='native-child'", (created,))
    contract = runpy.run_path(str(Path(hook).resolve().parents[2] / 'bin/threads'))
    assert contract['unjudged']({'outcome': 'running', 'item': 'north#35'})
    assert not contract['unjudged']({'outcome': 'done', 'item': 'north#35'})
    print('PASS generated hook native creation records exact decision brief, actual child and observed metadata before completion')
    missing_bin = root / 'missing-cli-bin'
    missing_bin.mkdir()
    for name in ('bash', 'head', 'dirname', 'python3', 'mkdir'):
        (missing_bin / name).symlink_to(shutil.which(name))
    env['PATH'] = str(missing_bin)
    assert shutil.which('threads', path=env['PATH']) is None
    missing = event('PostToolUse', '', session='native-lead', tool='spawn_agent', tool_input={'message': brief, 'task_name': 'creation'}, response=json.dumps({'task_name': '/root/creation'}), hook_path=generated_hook)
    assert missing['additionalContext'] == 'spawn recording boundary miss for lead native-lead: FileNotFoundError', missing
    with sqlite3.connect(root / 'runs.db') as db:
        assert db.execute('SELECT count(*) FROM runs').fetchone()[0] == 1
    env['PATH'] = runtime_path
    print('PASS missing installed threads CLI reports an honest boundary miss and fails open')

    control = subprocess.run([hook], input=json.dumps({'hook_event_name': 'PreToolUse', 'session_id': 'plain-codex', 'cwd': str(game), 'tool_name': 'apply_patch', 'tool_input': {}}), text=True, capture_output=True, env=env, check=True)
    assert (control.stdout, control.stderr, control.returncode) == ('', '', 0), control
    relay_session = 'relay-origin'
    event('UserPromptSubmit', '', session=relay_session, prompt='Continue the task.')
    relay_state = state / f'{relay_session}.json'
    relay_stop = 'From the proxy, STOP NOW: never run window actions'
    marked = event('UserPromptSubmit', '', session=relay_session, prompt='<north_machine_relay/> ' + relay_stop)
    assert 'Scoreboard:' in marked['additionalContext'] and 'Tom asked you to stop.' not in marked['additionalContext']
    assert 'stop_request' not in json.loads(relay_state.read_text())
    direct = event('UserPromptSubmit', '', session=relay_session, prompt=relay_stop)
    assert json.loads(relay_state.read_text())['stop_request'] and 'Tom asked you to stop.' in direct['additionalContext']
    relay_correction = 'Why are you adding process? Close this.'
    marked = event('UserPromptSubmit', '', session=relay_session, prompt='<north_machine_relay/> ' + relay_correction)
    assert 'Scoreboard:' in marked['additionalContext']
    assert 'Tom is correcting you.' not in marked['additionalContext'] and 'Tom wants this closed.' not in marked['additionalContext']
    assert json.loads(relay_state.read_text())['stop_request']
    direct = event('UserPromptSubmit', '', session=relay_session, prompt=relay_correction)
    assert 'Tom is correcting you.' in direct['additionalContext'] and 'Tom wants this closed.' in direct['additionalContext']
    assert 'stop_request' not in json.loads(relay_state.read_text())
    for ask in ('request_user_input_async', 'request_user_input'):
        assert event('PreToolUse', '', denied=True, tool=ask)['permissionDecisionReason'] == 'Use north fleet ask with your parent session when you need an answer.'
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
    assert stop('Done: tests pass on the arc fix; land the change with safe-push --to main.', 'land')
    event('UserPromptSubmit', '', session='ad-hoc', prompt='why did you open with fast mode? change it to the default tier')
    assert not stop('Both fixes are landed and future launches use the default tier.', 'ad-hoc')
    event('UserPromptSubmit', '', session='issue-work', prompt='finish north#52')
    assert stop('Both fixes are landed and future launches use the default tier.', 'issue-work')
    assert not stop('Landed; #52 closed with every box ticked.', 'issue-work')
    reminder = event('UserPromptSubmit', '', session='dependency-checkpoint', prompt='Finish the owned worker task; exclusive capacity is held by another worker.')
    assert all(word in reminder['additionalContext'] for word in ('Not done', 'checkpoint', 'dependency', 'parent'))
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
    with sqlite3.connect(pooled / 'sqlite/state_5.sqlite') as db:
        db.execute("INSERT INTO threads VALUES('gate-lead',0,'gpt-6.1-sol','medium','/root',0)")
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
    gate_ok_brief = root / 'gate-ok-brief.md'
    gate_ok_brief.write_text(tooling_box('shell'))
    ts_brief = root / 'gate-ts-brief.md'
    ts_brief.write_text(tooling_box('ts'))
    subprocess.run([str(installed_threads), 'register-spawn', '--parent', 'gate-lead', '--task', 'gate_registered', '--brief-file', str(ts_brief)], env=env, check=True)
    refused = spawn('gate_registered', 'opaque relay text', True)
    assert 'route to Claude: Surface ts' in refused['permissionDecisionReason'], refused
    subprocess.run([str(installed_threads), 'register-spawn', '--parent', 'gate-lead', '--task', 'gate_opaque_ts', '--brief-file', str(ts_brief)], env=env, check=True)
    refused = spawn('gate_opaque_ts', 'gAAAAABq0JPY_QUer0123456789abcdefABCDEF-_=', True)
    assert 'route to Claude: Surface ts' in refused['permissionDecisionReason'], refused
    subprocess.run([str(installed_threads), 'register-spawn', '--parent', 'gate-lead', '--task', 'gate_opaque_ok', '--brief-file', str(gate_ok_brief)], env=env, check=True)
    spawn('gate_opaque_ok', 'gAAAAABq0JPY_QUer0123456789abcdefABCDEF-_=', False)
    spawn('gate_delegate', 'Run the entire operator task: fix the failing build and report.', False)
    spawn('gate_probe', 'echo_probe', False)
    print('PASS Codex spawn gate routes unqualified boxes to Claude, admits qualified Codex sessions without a coach, admits a Category: review box only for a reviewer family other than its Reviews author, checks the registered brief behind an encrypted message, and leaves header-less spawns to the registration check')
    print(f'behavior Git/ledger/process/relay boundary: {decisions} decisions passed')
PY
