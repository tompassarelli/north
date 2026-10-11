#!/usr/bin/env python3
"""Replay the 2026-07-29 primary breaches and exercise real layout/Git/patch boundaries."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent / 'lib'))
from launch_critical_decide import decide

NORTH = '/home/tom/code/north/main'
CLAUSE = '/home/tom/code/clause/main'


def bash(command, cwd='/home/tom'):
    return {'tool_name': 'Bash', 'tool_input': {'command': command}, 'cwd': cwd}


incidents = [
    bash("python3 - <<'PYEOF'\nopen('cli/x.clj','w')\nPYEOF", NORTH),
    bash('git add cli/x.clj', NORTH),
    bash("git commit -q -m 'x'", NORTH),
    bash('git -C /home/tom/code/clause/main reset --hard origin/main'),
    bash('FILES=$(cat list); git add $FILES', CLAUSE),
    bash('git push origin main', NORTH),
    bash('echo x > /home/tom/code/north/main/cli/x.clj'),
    bash('sed -i s/a/b/ /home/tom/code/clause/main/bin/clause'),
    bash('cp /tmp/x.clj /home/tom/code/north/main/cli/x.clj'),
    bash('rm /home/tom/code/clause/main/bin/clause'),
    bash('cd /home/tom/code/north/main && echo x > cli/x.clj'),
]
for payload in incidents:
    assert decide(payload), payload

checks = len(incidents)
with tempfile.TemporaryDirectory(prefix='launch-boundary-') as temporary:
    code = Path(temporary) / 'code'
    main = code / 'proj/main'
    pin_oid = '0123456789abcdef0123456789abcdef01234567'
    pin = code / 'proj/pins' / pin_oid
    lane = code / 'proj/worktrees/main'
    for directory in [main, pin, lane, code / 'client/acme/app/main/.git', code / 'resources/upstream/main/.git', code / 'runtime-data/.git']:
        directory.mkdir(parents=True)
    for directory in [main, pin]:
        subprocess.run(['git', 'init', '-q', str(directory)], check=True)
    (main / 'src').mkdir()
    (main / 'src/kept.txt').write_text('tracked\n')
    (main / 'build').mkdir()
    (main / 'build/out.js').write_text('generated\n')
    subprocess.run(['git', '-C', str(main), 'add', 'src/kept.txt'], check=True)
    subprocess.run(['git', '-C', str(main), '-c', 'user.name=t', '-c', 'user.email=t@t', 'commit', '-qm', 'base'], check=True)
    sidecar = pin.with_name(pin_oid + '.pin')
    sidecar.write_text('Consumers: docs-boundary-consumer.\n')
    saved = os.environ.get('LAUNCH_CRITICAL_CODE_ROOT')
    os.environ['LAUNCH_CRITICAL_CODE_ROOT'] = str(code)
    try:
        for target, denied in [
            (main / 'src/kept.txt', True),
            (code / 'client/acme/app/main/src/x.py', True),
            (lane / 'src/x.py', False),
            (code / 'proj/scratch.txt', False),
            (code / 'proj/worktrees', False),
            (code / 'proj/pins', False),
            (pin / 'index.html', True),
            (sidecar, False),
            (pin / 'nested.pin', True),
            (code / 'runtime-data/state.json', False),
            (code / 'resources/upstream/main/README.md', False),
        ]:
            payload = {'tool_name': 'Edit', 'tool_input': {'file_path': str(target)}}
            reason = decide(payload)
            assert bool(reason) == denied, (payload, reason)
            if target == pin / 'index.html':
                assert 'docs-boundary-consumer' in reason
            checks += 1
        for command, denied in [
            (f'rm -rf {main}/build', False),
            (f'rm {main}/src/kept.txt', True),
            (f'rm -rf {main}/src', True),
            (f'rm -rf {main}', True),
            (f'echo x > {main}/build/new.js', True),
        ]:
            reason = decide(bash(command, str(lane)))
            assert bool(reason) == denied, (command, reason)
            checks += 1
        for target, denied in [(main / 'src/kept.txt', True), (lane / 'x.txt', False), (pin / 'x.txt', True)]:
            envelope = f'*** Begin Patch\n*** Update File: {target}\n@@\n-a\n+b\n*** End Patch'
            payload = {'tool_name': 'apply_patch', 'cwd': str(lane), 'tool_input': {'input': envelope}}
            reason = decide(payload)
            assert bool(reason) == denied, (payload, reason)
            checks += 1
        assert decide({'tool_name': 'apply_patch', 'tool_input': {'input': 'not a patch'}})
        checks += 1
    finally:
        os.environ.pop('LAUNCH_CRITICAL_CODE_ROOT', None)
        if saved is not None:
            os.environ['LAUNCH_CRITICAL_CODE_ROOT'] = saved
print(f'launch recorded/layout/Git/patch boundaries: {checks} decisions passed')
