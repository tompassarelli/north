import argparse
from collections import defaultdict
from contextlib import closing
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import sqlite3
from statistics import median
import sys
from zoneinfo import ZoneInfo

sys.path.insert(0, str(Path(__file__).resolve().parent))
from defaults import DEFAULTS

SLOT_MIN = DEFAULTS["presence_slot_min"]
WINDOW_DAYS = DEFAULTS["presence_window_days"]
PREDEPARTURE_LEAD_MIN = DEFAULTS["predeparture_lead_min"]
IDLE_REOPEN_SEC = DEFAULTS["laptop_idle_reopen_sec"]


def timestamp(value):
    if isinstance(value, datetime):
        result = value
    else:
        result = datetime.fromisoformat(value.replace('Z', '+00:00'))
    if result.tzinfo is None:
        raise ValueError('presence timestamps need a timezone')
    return result


def iso(value):
    return value.astimezone(timezone.utc).isoformat().replace('+00:00', 'Z')


def sleep_block(turns, now, zone):
    recent = sorted({timestamp(row['at']).astimezone(zone) for row in turns
                     if now - timedelta(days=14) <= timestamp(row['at']) <= now})
    gaps = {}
    for start, end in zip(recent, recent[1:]):
        duration = (end - start).total_seconds() / 60
        if 120 <= duration <= 1440:
            key = start.date()
            if key not in gaps or duration > gaps[key][0]:
                minute = start.hour * 60 + start.minute
                unwrapped = minute if minute >= 720 else minute + 1440
                gaps[key] = (duration, unwrapped, unwrapped + duration)
    if not gaps:
        return None
    return median(row[1] for row in gaps.values()) % 1440, median(row[2] for row in gaps.values()) % 1440


def sleeping(at, block, zone):
    if block is None:
        return False
    local = at.astimezone(zone)
    minute = local.hour * 60 + local.minute
    start, end = block
    return start <= minute < end if start < end else minute >= start or minute < end


def forecast_model(now, turns, idle, zone):
    step = timedelta(minutes=SLOT_MIN)
    start = now - timedelta(days=WINDOW_DAYS)
    turns = sorted({timestamp(row['at']) for row in turns if start <= timestamp(row['at']) <= now})
    idle = sorted((timestamp(row['at']), row['idle']) for row in idle
                  if start <= timestamp(row['at']) <= now and type(row.get('idle')) is bool)
    if not turns and not idle:
        return lambda at: 0.0
    first = min(([turns[0]] if turns else []) + ([idle[0][0]] if idle else []))
    daily = defaultdict(lambda: [0, 0])
    weekly = defaultdict(lambda: [0, 0])
    slot = first.replace(hour=0, minute=0, second=0, microsecond=0)
    turn_index = idle_index = 0
    seat = None
    while slot < now:
        while turn_index < len(turns) and turns[turn_index] < slot - step:
            turn_index += 1
        while idle_index < len(idle) and idle[idle_index][0] <= slot:
            seat = not idle[idle_index][1]
            idle_index += 1
        active = seat is True or (turn_index < len(turns) and turns[turn_index] <= min(now, slot + step))
        local = slot.astimezone(zone)
        day_slot = (local.hour * 60 + local.minute) // SLOT_MIN
        for pair in (daily[day_slot], weekly[local.weekday(), day_slot]):
            pair[0] += int(active)
            pair[1] += 1
        slot += step

    def probability(at):
        local = at.astimezone(zone)
        day_slot = (local.hour * 60 + local.minute) // SLOT_MIN
        hits, n = daily[day_slot]
        day_p = hits / n if n else 0.0
        hits, weeks = weekly[local.weekday(), day_slot]
        weight = min(1.0, weeks / 3)
        return weight * (hits / weeks if weeks else day_p) + (1 - weight) * day_p

    return probability


def presence(now, signals):
    now = timestamp(now)
    zone = signals.get('timezone') or now.tzinfo
    if isinstance(zone, str):
        zone = ZoneInfo(zone)
    turns = [row for row in signals.get('turns', []) if timestamp(row['at']) <= now]
    idle = [row for row in signals.get('idle', []) if timestamp(row['at']) <= now]
    block = sleep_block(turns, now, zone)
    probability = forecast_model(now, turns, idle, zone)
    away = [(timestamp(row['from']), timestamp(row['to']), row) for row in signals.get('away', [])]

    def stated(at):
        return next(((start, row) for start, end, row in away if start <= at < end), None)

    def predicted(at):
        if stated(at):
            return 'travelling', 1.0
        if sleeping(at, block, zone):
            return 'asleep', 1 - probability(at)
        p = probability(at)
        return ('present', p) if p >= 0.5 else ('away', 1 - p)

    evidence = []
    stated_now = stated(now)
    live = []
    idle_machines = []
    for machine in ('desktop', 'laptop'):
        row = signals.get(machine, {})
        at = timestamp(row['at']) if row.get('at') else now
        fresh = row.get('at') and timedelta(0) <= now - at <= timedelta(minutes=SLOT_MIN)
        if row.get('active') is True and fresh:
            live.append(at)
            evidence.append(machine)
        if row.get('active') is False and fresh:
            idle_machines.append(machine)
    desktop = signals.get('desktop', {})
    session_start = timestamp(desktop['session_start']) if desktop.get('session_start') else now
    current_idle = [row for row in idle if timestamp(row['at']) >= session_start]
    idle_since = None
    for row in sorted(current_idle, key=lambda row: timestamp(row['at'])):
        if row['idle'] is True:
            if idle_since is None:
                idle_since = timestamp(row['at'])
        else:
            idle_since = None
    validated_idle = len(idle_machines) == 2 and idle_since is not None and bool(desktop.get('session_start'))
    recent = [timestamp(row['at']) for row in turns if now - timedelta(minutes=SLOT_MIN) <= timestamp(row['at'])]
    if recent:
        live.append(max(recent))
        evidence.append('human-turn')
    if live:
        state, since = 'present', min(live)
    elif stated_now:
        since, entry = stated_now
        state = 'travelling'
        evidence = [{'stated': entry.get('said', ''), 'kind': entry['kind']}]
    elif sleeping(now, block, zone):
        state = 'asleep'
        local = now.astimezone(zone)
        since = local.replace(hour=int(block[0] // 60), minute=int(block[0] % 60), second=0, microsecond=0)
        if since > now:
            since -= timedelta(days=1)
        evidence.append('learned-sleep')
    else:
        state = 'away' if validated_idle else 'unknown'
        since = idle_since if validated_idle else now
        evidence.append('live-idle' if validated_idle else 'incomplete-live-presence')
    step = timedelta(minutes=SLOT_MIN)
    slot = now.replace(second=0, microsecond=0) + timedelta(minutes=SLOT_MIN - now.minute % SLOT_MIN)
    transition = {'state': None, 'at': None, 'p': None}
    for _ in range(7 * 24 * 60 // SLOT_MIN):
        candidate, p = predicted(slot)
        if candidate != state and predicted(slot + step)[0] == candidate:
            transition = {'state': candidate, 'at': iso(slot), 'p': round(p, 4)}
            break
        slot += step
    if idle_since is not None and turns:
        idle_since = max(idle_since, max(timestamp(row['at']) for row in turns))
    if idle_since is not None and signals.get('laptop', {}).get('idle_since'):
        idle_since = max(idle_since, timestamp(signals['laptop']['idle_since']))
    sustained_idle = validated_idle and (now - idle_since).total_seconds() >= IDLE_REOPEN_SEC
    pool_state = state if state == 'present' or sustained_idle else 'unknown'
    return {'state': state, 'pool_state': pool_state, 'since': iso(since), 'next': transition, 'evidence': evidence}


def read_json(path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return {}


def human_turns(now, index, home):
    if not index.is_file():
        return []
    start = iso(now - timedelta(days=WINDOW_DAYS))
    grouped = defaultdict(dict)
    offsets = {}
    try:
        with closing(sqlite3.connect(index.resolve().as_uri() + '?mode=ro', uri=True)) as db:
            rows = db.execute("SELECT f.path,m.line,m.ts,f.off FROM files f JOIN msg m INDEXED BY msg_file ON m.file_id=f.id "
                              "WHERE f.provider='anthropic' AND f.agent IS NULL AND f.cwd=? "
                              "AND m.role='user' AND m.ts>=? AND m.ts<=?", (str(home), start, iso(now)))
            for path, line, at, offset in rows:
                grouped[path][line] = at
                offsets[path] = offset
    except sqlite3.Error:
        return []
    turns = []
    for name, wanted in grouped.items():
        try:
            last = max(wanted)
            with Path(name).open() as stream:
                for number, line in enumerate(stream, 1):
                    if number in wanted:
                        row = json.loads(line)
                        origin = row.get('origin')
                        exact = isinstance(origin, dict) and origin.get('kind') == 'human'
                        if exact or (origin is None and not row.get('isMeta') and not row.get('isCompactSummary')):
                            turns.append({'at': wanted[number], 'label': 'exact' if exact else 'heuristic'})
                    if number >= last:
                        break
                stream.seek(offsets[name])
                for line in stream:
                    try:
                        row = json.loads(line)
                        if (row.get('origin') or {}).get('kind') == 'human' and row.get('timestamp'):
                            at = timestamp(row['timestamp'])
                            if now - timedelta(days=WINDOW_DAYS) <= at <= now:
                                turns.append({'at': iso(at), 'label': 'exact'})
                    except (ValueError, TypeError, AttributeError):
                        continue
        except (OSError, ValueError):
            continue
    exact = [timestamp(row['at']) for row in turns if row['label'] == 'exact']
    cutoff = min(exact) if exact else now
    return sorted([row for row in turns if row['label'] == 'exact' or timestamp(row['at']) < cutoff],
                  key=lambda row: row['at'])


def live_signals(now, proc=Path('/proc')):
    home = Path.home()
    state = Path(os.environ.get('XDG_STATE_HOME') or home / '.local/state')
    operator = Path(os.environ.get('AGENTS_OPERATOR_DIR') or home / 'code/south/main/agents')
    signals = {'turns': human_turns(now, Path(os.environ.get('CONVO_STATE') or state / 'convo') / 'index-v4.db', home),
               'idle': [], 'timezone': os.environ.get('TZ') or datetime.now().astimezone().tzinfo}
    try:
        import tomllib
        signals['away'] = tomllib.loads((operator / 'presence.toml').read_text()).get('away', [])
    except (OSError, ValueError):
        signals['away'] = []
    try:
        with (state / 'agents/presence/idle.jsonl').open() as stream:
            for line in stream:
                try:
                    row = json.loads(line)
                    if now - timedelta(days=WINDOW_DAYS) <= timestamp(row['at']) <= now:
                        signals['idle'].append(row)
                except (ValueError, KeyError, TypeError):
                    continue
    except OSError:
        pass
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'hooks/lib'))
    from desktop_action import state_path
    desktop = read_json(state_path())
    try:
        stat = (proc / str(int(desktop['pid'])) / 'stat').read_text()
        name, fields = stat.split('(', 1)[1].rsplit(')', 1)
        fields = fields.split()
        started = fields[19]
        if name == 'swayidle' and fields[0] not in ('Z', 'X') and started == desktop['started']:
            signals['desktop'] = {'active': desktop['idle'] is False, 'at': iso(now)}
            boot = next(line.split()[1] for line in (proc / 'stat').read_text().splitlines() if line.startswith('btime '))
            signals['desktop']['session_start'] = iso(datetime.fromtimestamp(
                int(boot) + int(started) / os.sysconf('SC_CLK_TCK'), timezone.utc))
    except (OSError, ValueError, KeyError, TypeError, IndexError, StopIteration):
        pass
    laptop_path = Path(os.environ.get('LAPTOP_STATE') or home / '.local/state/placement/laptop.json')
    laptop = read_json(laptop_path.with_name(laptop_path.stem + '-observations.json'))
    if laptop.get('at') and (bool(laptop.get('active')) != bool(laptop.get('idle'))):
        signals['laptop'] = {'at': laptop['at'], 'active': bool(laptop.get('active'))}
        if laptop.get('idle'):
            signals['laptop']['idle_since'] = laptop['idle']
    return signals


def main():
    parser = argparse.ArgumentParser(prog='agents presence')
    parser.add_argument('--json', action='store_true')
    parser.add_argument('--at', type=timestamp)
    args = parser.parse_args()
    now = args.at or datetime.now().astimezone()
    result = presence(now, live_signals(now))
    if args.json:
        print(json.dumps(result))
    else:
        upcoming = result['next']
        print(f"{result['state']} since {result['since']}; next {upcoming['state']} at {upcoming['at']} (p={upcoming['p']})")


if __name__ == '__main__':
    main()
