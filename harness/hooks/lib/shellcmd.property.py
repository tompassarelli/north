#!/usr/bin/env python3
import argparse
import contextlib
import io
import json
import os
from pathlib import Path
import random
import re
import shlex
import subprocess
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import shellcmd

TRIGGERS = {
    'git-blind-stage': (['git add -A', 'git add --all', 'git add -u', 'git add --update', 'git add .', 'git commit -a', 'git commit --all', 'git commit -am msg'], ['git add path/to/file', 'git add .gitignore', 'git commit -m msg']),
    'git-stash': (['git stash', 'git stash push', 'git stash save', 'git stash pop', 'git -C /tmp stash pop'], ['git stash list', 'git stash apply abc', 'git stash push -u -m tag', 'git stash push --message=tag']),
    'clipboard': (['wl-copy hi', 'wl-copy --clear', '/run/current-system/sw/bin/wl-copy hi', 'niri msg action screenshot-window', 'niri msg -j action screenshot-screen', 'wl-paste', 'wl-paste --list-types', 'xclip -o -selection clipboard', 'xsel -o'], ['echo "wl-paste"', 'rg wl-paste socrates', 'agent-capture --screen /tmp/s.png', 'niri msg action focus-window --id 3']),
    'generated-policy': (['sed -i s/a/b/ {cwd}/src/host.nix', 'tee {home}/.claude/CLAUDE.md', 'sd old new {home}/.claude/CLAUDE.md', 'cp /tmp/x {home}/.codex/AGENTS.md', 'mv /tmp/x {home}/.codex/skills/a/SKILL.md'], ['cat {home}/.claude/CLAUDE.md', 'sed -n p {cwd}/src/host.nix', 'sed -i s/a/b/ {cwd}/src/plain.nix', 'cp {home}/.claude/CLAUDE.md /tmp/claude.md', 'agents sync']),
    'session-kill': (['kill -1', 'kill -9 -1', 'kill -- -1', 'pkill -u tom', 'pkill niri', 'killall -u tom', 'killall niri', 'loginctl terminate-user tom', 'systemctl --user exit', 'systemctl --user restart niri.service', 'nohup bun task', 'setsid bun task', 'disown', 'bun /tmp/task.mjs'], ['kill -1 1234', 'kill -9 1234', 'pkill -u tom scoped', 'systemctl --user restart wob.service', 'run-bounded 30m -- bun /tmp/task.mjs', 'bun task > /tmp/task.log 2>&1']),
    'modern-search': (['grep -rn foo src', "find . -name '*.ts'", 'rgrep foo src', 'grep --recursive -il foo src'], ['grep foo a.ts', 'git grep foo', "find . -maxdepth 1 -name '*.ts'", 'find . -mtime +3 -delete']),
    'resource-safe-search': (['rg TARGET /proc/[0-9]*/cwd', 'rg TARGET /sys', 'rg TARGET {home}/code/proj'], ['rg TARGET /proc/1234/status', 'rg TARGET {cwd}/src', 'find /proc -maxdepth 1 -type d']),
    'corpus-scan': (['rg needle {home}/code/north-data', 'rg needle {home}/code/north-data/accounts', 'grep -R needle {home}/code/north-data', 'find {home}/code/north-data -name rollout-*.jsonl'], ['rg needle {home}/code/north-data/accounts/openai/acct/sessions/2026/08/12', 'rg --max-depth 2 needle {home}/code/north-data', 'convo -x needle']),
    'native-launch': (['wine game.exe', 'wine64 game.exe', 'proton run game.exe', 'steam-run wine game.exe'], ['ls game.exe', 'pgrep -f game.exe', 'bun /opt/machine-capacity.mjs session --class native --owner x -- wine game.exe']),
    'native-item': (['gh issue comment 5 --body life#1', 'gh pr edit 3 --body travel#2'], ['gh issue view 1', 'gh issue comment 5 --body landed', 'threads note life#1 progress']),
    'heavy-command': (['cargo build --release', 'bun test', 'bun wisp map build', 'bun wisp headless --render out --frames 3', 'bun wisp view scene out'], ['cargo check', 'bun test test/a.test.ts', 'bun wisp map rebuild', 'bun /opt/machine-capacity.mjs run --owner x --timeout-seconds 60 -- cargo build']),
}


class Guard:
    def __init__(self, path):
        self.path = Path(path)
        match = re.search(r"<<'PYEOF'.*?\n(.*?)\nPYEOF", self.path.read_text(), re.S)
        self.code = compile(match.group(1), str(self.path), 'exec')

    def payload(self, payload):
        saved = sys.stdin, sys.argv
        output = io.StringIO()
        sys.stdin = io.StringIO(json.dumps(payload))
        sys.argv = [str(self.path), str(self.path.parent / 'lib')]
        try:
            with contextlib.redirect_stdout(output):
                try:
                    exec(self.code, {'__name__': '__main__', '__file__': str(self.path)})
                except SystemExit as error:
                    if error.code not in (None, 0):
                        raise
        finally:
            sys.stdin, sys.argv = saved
        result = json.loads(output.getvalue() or '{}')
        return (result.get('hookSpecificOutput') or {}).get('permissionDecision') == 'deny'

    def __call__(self, command, cwd):
        return self.payload({'tool_name': 'Bash', 'cwd': cwd, 'tool_input': {'command': command}})


def check(condition, name, command):
    if not condition:
        raise AssertionError(f'{name}: seed=20261011 input={command!r}')


def tripwire_commands(text):
    result = []
    tokens = shellcmd.tripwire_tokens(text)
    start = 0
    for end in range(len(tokens) + 1):
        if end < len(tokens) and tokens[end] not in (';', '&&', '||', '|'):
            continue
        index = start
        while index < end:
            name = shellcmd.tripwire_word(tokens[index])
            if name == '_skip_user':
                index += 2
            elif not name:
                index += 1
            else:
                result.append((name, tokens[index + 1:end]))
                break
        start = end + 1
    return result


def tripwire_property():
    count = 0
    for trigger in ('rm -rf /', 'git push --force', 'shutdown -h now', 'mkfs.ext4 /dev/sda1'):
        expected = tripwire_commands(trigger)
        for prefix in ('', 'CHECK=1 ', 'command ', 'sudo ', 'env CHECK=1 '):
            for separator in (';', '&&', '|', '\n'):
                command = 'printf ready ' + separator + ' ' + prefix + trigger
                check(expected[0] in tripwire_commands(command), 'tripwire prefix/chaining', command)
                count += 1
        for command in (shlex.quote(trigger), '"' + trigger + '"', 'echo ' + shlex.quote(trigger), 'git commit -m ' + shlex.quote(trigger), 'echo ' + shlex.quote('example; ' + trigger)):
            check(expected[0] not in tripwire_commands(command), 'tripwire argument data', command)
            count += 1
    return count


def launch_property(cwd):
    from launch_critical_decide import decide
    Path(cwd, '.git').mkdir(exist_ok=True)
    count = 0
    for trigger in (f'git -C {cwd} reset --hard HEAD', f'rm {cwd}/src/main.txt', f'sed -i s/a/b/ {cwd}/src/main.txt', f'cargo test --target-dir {Path(cwd).parent}/target-agent'):
        def denied(command):
            return bool(decide({'tool_name': 'Bash', 'cwd': str(Path(cwd).parent / 'worktrees/lane'), 'tool_input': {'command': command}}))
        for prefix in ('', 'CHECK=1 ', 'command ', 'sudo ', 'env CHECK=1 '):
            for command in [prefix + trigger] + ['printf ready ' + separator + ' ' + prefix + trigger for separator in (';', '&&', '|')]:
                check(denied(command), 'launch prefix/chaining', command)
                count += 1
        for command in ('echo ' + shlex.quote(trigger), 'git commit -m ' + shlex.quote(trigger), "cat <<'BODY'\n" + trigger + '\nBODY'):
            check(not denied(command), 'launch inert data', command)
            count += 1
    return count


def exercise(name, home, cwd, boundary):
    path = Path(__file__).resolve().parent.parent / (name + '-guard.sh')
    guard = Guard(path)
    triggers, near_misses = TRIGGERS[name]
    triggers = [t.format(home=home, cwd=cwd) for t in triggers]
    near_misses = [t.format(home=home, cwd=cwd) for t in near_misses]
    count = 0
    if name == 'generated-policy':
        for leaf in ('host.nix', 'host.bnix', 'plain.nix'):
            Path(cwd, 'src', leaf).touch()
    prefixes = ['', 'CHECK=1 ', 'CHECK="some value" ', 'command ', 'sudo ', 'env CHECK=1 ', 'CHECK=1 command sudo env OTHER=2 ']
    generator = random.Random(20261011)
    for index in range(12):
        wrappers = generator.sample(['command', 'sudo', 'env'], generator.randrange(1, 4))
        assignment = f'CHECK_{index}={generator.randrange(10000)}'
        prefixes.append(assignment + ' ' + ' '.join(wrappers) + ' ')
    prefixes += ['sudo -u tom ', 'env -u CHECK CHECK=1 ', 'CHECK=${HOME} ', 'CHECK=$HOME ']
    generator.shuffle(prefixes)
    for trigger in triggers:
        check(guard(trigger, cwd), name + ' trigger', trigger)
        count += 1
        substitution = 'CHECK=$(' + trigger + '); echo $CHECK'
        check(guard(substitution, cwd), name + ' live substitution', substitution)
        count += 1
        for prefix in prefixes:
            wrapped = prefix + trigger
            for command in [wrapped] + [f'printf ready {sep} {wrapped}' for sep in (';', '&&', '|', '\n')]:
                check(guard(command, cwd), name + ' prefix/chaining', command)
                count += 1
        single = "'" + trigger.replace("'", "'\"'\"'") + "'"
        double = '"' + trigger.replace('\\', '\\\\').replace('"', '\\"').replace('$', '\\$').replace('`', '\\`') + '"'
        for data in [single, double, 'echo ' + single, 'echo ' + double, 'git commit -m ' + single, "cat <<'BODY'\n" + trigger + '\nBODY', 'echo '+trigger]:
            expected = data in (single, double) and (name == 'native-launch' or (name == 'session-kill' and trigger == 'disown'))
            check(guard(data, cwd) == expected, name + ' inert data', data)
            count += 1
        for separator in (';', '&&', '|', '\n', '('):
            prose = 'example ' + separator + ' ' + trigger
            for quote in ("'", '"'):
                escaped = prose.replace('\\', '\\\\').replace(quote, '\\' + quote) if quote == '"' else prose.replace("'", "'\"'\"'")
                data = 'echo ' + quote + escaped + quote
                check(not guard(data, cwd), name + ' quoted separators', data)
                count += 1
    for command in near_misses:
        check(not guard(command, cwd), name + ' near miss', command)
        count += 1
    if name == 'native-item':
        import sqlite3
        database = Path(home) / '.local/state/threads/threads.db'
        database.parent.mkdir(parents=True, exist_ok=True)
        with sqlite3.connect(database) as connection:
            connection.executescript("CREATE TABLE items(title TEXT, body TEXT); CREATE TABLE events(kind TEXT, data TEXT); INSERT INTO items VALUES ('the orange bicycle', 'repaint the garden shed blue'); INSERT INTO events VALUES ('note', '{\"text\":\"the parcel went to the neighbour\"}');")
        body = Path(cwd) / 'body.md'
        body.write_text('repaint the garden shed blue')
        for command in ['gh issue comment 5 --body "the orange bicycle"', 'gh issue comment 5 --body "the parcel went to the neighbour"', 'gh issue comment 5 --body-file body.md', 'gh api repos/x/y/issues -F body=@body.md', "gh issue comment 5 --body-file - <<'BODY'\nlife#1\nBODY"]:
            check(guard(command, cwd), name + ' SQLite/body-file boundary', command)
            count += 1
        database.unlink()
        check(not guard('gh issue comment 5 --body "the orange bicycle"', cwd), name + ' missing database', 'missing database')
        count += 1
    if boundary:
        active = Path(home) / 'activation.active'
        active.write_text('hook ' + name + '-guard\n')
        if name == 'session-kill':
            active.with_suffix('.json').write_text(json.dumps({'schema': 'north.agent-activation/v1', 'catalogDigest': 'sha256:' + 'a' * 64, 'generationId': 'sha256:' + 'b' * 64, 'units': [{'kind': 'hook', 'id': 'session-kill-guard', 'permission': 'on', 'active': True}]}))
        env = dict(os.environ, NORTH_AGENT_ACTIVE=str(active), AGENT_NO_AUTHORING_HOOKS='0', NORTH_AGENT_PYTHON='/etc/codex/hooks/runtime/python3')
        payload = {'tool_name': 'Bash', 'cwd': cwd, 'tool_input': {'command': 'CHECK=1 ' + triggers[0]}}
        result = subprocess.run([str(path)], input=json.dumps(payload), text=True, capture_output=True, env=env, check=True, timeout=30)
        decoded = json.loads(result.stdout or '{}')
        check((decoded.get('hookSpecificOutput') or {}).get('permissionDecision') == 'deny', name + ' entry point', payload)
        count += 1
    return count


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--guard', choices=TRIGGERS)
    parser.add_argument('--tripwire', action='store_true')
    parser.add_argument('--launch', action='store_true')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='shellcmd-property-', dir='/tmp') as temporary:
        home = Path(temporary) / 'home'
        cwd = home / 'code/proj/main'
        for directory in [cwd / 'src', home / 'code/proj/worktrees/lane', home / 'code/north-data/accounts/openai/acct/sessions/2026/08/12']:
            directory.mkdir(parents=True)
        env = dict(os.environ)
        try:
            os.environ['HOME'] = str(home)
            os.environ.pop('AGENT_NO_AUTHORING_HOOKS', None)
            names = [] if args.tripwire or args.launch else [args.guard] if args.guard else TRIGGERS
            count = sum(exercise(name, str(home), str(cwd), bool(args.guard)) for name in names)
            if args.tripwire or (not args.guard and not args.launch):
                count += tripwire_property()
            if args.launch or (not args.guard and not args.tripwire):
                count += launch_property(str(cwd))
            print(f'shellcmd property: {count} decisions, seed=20261011, guards={len(names)}')
        finally:
            os.environ.clear()
            os.environ.update(env)


if __name__ == '__main__':
    main()
