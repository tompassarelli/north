#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec python3 - "$HERE/concrete-model-identity-guard.sh" <<'PY'
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

hook = sys.argv[1]
with tempfile.TemporaryDirectory(prefix='model-boundary-') as temporary:
    root = Path(temporary)
    todo = root / 'home/code/todo'
    todo.mkdir(parents=True)
    path = str(todo / 'task.md')
    env = dict(os.environ, HOME=str(root / 'home'), TODO_ROOT=str(todo), AGENT_NO_AUTHORING_HOOKS='0')
    checks = 0
    for tool, field in [('Write', 'content'), ('Edit', 'new_string'), ('MultiEdit', 'edits')]:
        for model, denied in [('inherited', True), ('gpt-6-sol', False)]:
            text = f'model = "{model}"'
            value = [{'newString': text}] if tool == 'MultiEdit' else text
            tool_input = {'filePath' if tool == 'MultiEdit' else 'file_path': path, field: value}
            payload = {'tool_name': tool, 'tool_input': tool_input}
            result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
            output = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
            assert (output.get('permissionDecision') == 'deny') == denied, (payload, result.stdout)
            assert not result.stderr, result.stderr
            checks += 1
    patch = f'*** Begin Patch\n*** Add File: {path}\n+model = "self"\n*** End Patch'
    for payload in [
        {'tool_name': 'apply_patch', 'cwd': str(root), 'tool_input': {'input': patch}},
        {'tool_name': 'Bash', 'cwd': str(root), 'tool_input': {'command': f'printf \'model = "self"\' > {path}'}}]:
        result = subprocess.run([hook], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
        output = json.loads(result.stdout or '{}').get('hookSpecificOutput', {})
        assert output.get('permissionDecision') == 'deny', (payload, result.stdout)
        assert not result.stderr, result.stderr
        checks += 1
    print(f'model Write/Edit/MultiEdit/patch/Bash boundary: {checks} decisions passed')
PY
