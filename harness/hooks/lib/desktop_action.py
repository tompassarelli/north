import json
import os
from pathlib import Path
import re
import shlex
import stat
import subprocess
import sys

from shellcmd import command as command_index, rewrite_input, strip_heredocs, tokenize

DENIAL = 'No. Tom is using the desktop. Use the private desktop, wait until he is idle, or put the need in your brief and announce it in one line first.'
WRAPPERS = {'env', 'command', 'exec', 'sudo', 'doas', 'nohup', 'setsid', 'timeout', 'nice', 'stdbuf', 'xargs', '!', '{', 'if', 'then', 'elif', 'while', 'until', 'do'}
SANCTIONED = {'codex-relay', 'spawn-quiet', 'chrome-tabs', 'private-desktop.sh'}


def process(pid):
    fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
    return int(fields[1]), fields[19], Path(f'/proc/{pid}/exe').resolve().name


def state_path():
    return Path(os.environ.get('XDG_RUNTIME_DIR') or f'/run/user/{os.getuid()}') / 'desktop-idle.json'


def record_idle(idle):
    pid = os.getppid()
    while pid > 1:
        parent, started, name = process(pid)
        if name == 'swayidle':
            path = state_path()
            temporary = path.with_suffix(f'.{os.getpid()}.tmp')
            temporary.write_text(json.dumps({'pid': pid, 'started': started, 'idle': idle}))
            os.chmod(temporary, 0o600)
            temporary.replace(path)
            return
        pid = parent
    raise ValueError('not a swayidle callback')


def seat_idle():
    try:
        data = json.loads(state_path().read_text())
        _, started, name = process(int(data['pid']))
        return name == 'swayidle' and started == data['started'] and data['idle'] is True
    except (OSError, ValueError, KeyError, TypeError):
        return False


def private_target(assignments, name):
    runtime = assignments.get('XDG_RUNTIME_DIR', os.environ.get('XDG_RUNTIME_DIR', ''))
    display = assignments.get('WAYLAND_DISPLAY', os.environ.get('WAYLAND_DISPLAY', ''))
    root = Path(os.environ.get('NORTH_PRIVATE_DESKTOP_ROOT') or f'/run/user/{os.getuid()}')
    path = Path(runtime)
    run = path.parent
    try:
        valid = (path.name == 'runtime' and run.parent == root
                and run.name.startswith('private-desktop.') and run.stat().st_uid == os.getuid()
                and (run / 'active').is_file() and (run / 'lifecycle-owned').is_file()
                and display == (run / 'wayland-display').read_text().strip()
                and '/' not in display and stat.S_ISSOCK((path / display).stat().st_mode))
        if name == 'niri':
            ipc = Path(assignments.get('NIRI_SOCKET', os.environ.get('NIRI_SOCKET', '')))
            return valid and ipc.parent == path and stat.S_ISSOCK(ipc.stat().st_mode)
        return valid and not assignments.get('WAYLAND_SOCKET', os.environ.get('WAYLAND_SOCKET'))
    except OSError:
        return False


def wrapped(name, args):
    if name == 'spawn-quiet':
        return args
    if name == 'chrome-tabs' and args[:1] == ['run']:
        return args[2:] if args[1:2] == ['--'] else args[1:]
    if name == 'private-desktop.sh' and args[:1] == ['start'] and '--' in args:
        return args[args.index('--') + 1:]
    return []


def quiet_close(args, assignments):
    window = None
    for index, arg in enumerate(args):
        if arg == '--id' and index + 1 < len(args):
            window = args[index + 1]
        elif arg.startswith('--id='):
            window = arg[5:]
    if window is None or not window.isdigit():
        return False
    env = dict(os.environ)
    if 'NIRI_SOCKET' in assignments:
        env['NIRI_SOCKET'] = assignments['NIRI_SOCKET']
    try:
        result = subprocess.run(['niri', 'msg', '-j', 'windows'], capture_output=True, text=True, timeout=5, env=env, check=True)
        windows = json.loads(result.stdout)
        return any(isinstance(item, dict) and item.get('id') == int(window) and item.get('app_id') == 'quiet.spawn'
                   and item.get('is_focused') is False for item in windows)
    except (OSError, ValueError, TypeError, subprocess.SubprocessError):
        return False


def substitutions(text):
    for match in re.finditer(r'\$\(|`', text):
        start = match.end()
        if match.group() == '`':
            end = text.find('`', start)
            if end > start:
                yield text[start:end]
            continue
        depth = 1
        for index in range(start, len(text)):
            depth += {'(': 1, ')': -1}.get(text[index], 0)
            if not depth:
                yield text[start:index]
                break


def dangerous(text, inherited=None):
    if any(dangerous(body, inherited) for body in substitutions(strip_heredocs(text)) if body != text):
        return True
    parts = []
    segment = []
    for kind, word, _ in tokenize(strip_heredocs(text)) + [('sep', ';', False)]:
        if kind == 'sep':
            if segment:
                parts.append(segment)
            segment = []
        else:
            segment.append(word)
    for words in parts:
        index = command_index(words, WRAPPERS)
        if index >= len(words):
            continue
        assignments = dict(inherited or {})
        for word in words[:index]:
            if re.fullmatch(r'[A-Za-z_][A-Za-z0-9_]*=.*', word, re.S):
                key, value = word.split('=', 1)
                assignments[key] = value
        name = os.path.basename(words[index])
        args = words[index + 1:]
        if name in SANCTIONED:
            nested = wrapped(name, args)
            if nested and dangerous(shlex.join(nested), assignments):
                return True
            continue
        shell_flag = next((i for i, arg in enumerate(args) if arg.startswith('-') and 'c' in arg and not arg.startswith('--')), None)
        if name in {'bash', 'sh', 'zsh', 'dash'} and shell_flag is not None:
            nested = shell_flag + 1
            if nested < len(args) and dangerous(args[nested], assignments):
                return True
        if name == 'eval' and dangerous(' '.join(args), assignments):
            return True
        action = False
        if name in {'wtype', 'ydotool'}:
            action = True
        elif name == 'niri' and 'msg' in args and 'action' in args:
            target = args.index('action') + 1
            action = target < len(args) and (args[target].startswith(('focus-', 'fullscreen-', 'move-'))
                                             or (args[target] == 'close-window' and not quiet_close(args[target + 1:], assignments)))
        if action and not (name != 'ydotool' and private_target(assignments, name)):
            return True
    return False


def restore_command(text):
    return (
        '(\n'
        "_north_desktop_focus=$(niri msg -j focused-window | jq -r '.id // empty') || exit $?\n"
        'case "$_north_desktop_focus" in *[!0-9]*) exit 1;; esac\n'
        'readonly _north_desktop_focus\n'
        '[ -z "$_north_desktop_focus" ] || trap \'niri msg action focus-window --id "$_north_desktop_focus" >/dev/null 2>&1 || :\' EXIT\n'
        "trap 'exit 130' INT\n"
        "trap 'exit 143' TERM\n"
        '(\n' + text + '\n)\n)'
    )


def decide(data):
    if data.get('tool_name') != 'Bash' or not isinstance(data.get('tool_input'), dict):
        return None
    text = data['tool_input'].get('command')
    if not isinstance(text, str) or not dangerous(text):
        return None
    if not seat_idle():
        return {'hookSpecificOutput': {'hookEventName': 'PreToolUse', 'permissionDecision': 'deny', 'permissionDecisionReason': DENIAL}}
    return rewrite_input(data, 'command', restore_command(text),
                         'desktop-action-guard: wrapped the desktop command to read the focused window when it runs and refocus that window on exit.')


if __name__ == '__main__':
    try:
        result = decide(json.load(sys.stdin))
        if result:
            print(json.dumps(result))
    except (ValueError, TypeError, OSError):
        raise SystemExit(65)
