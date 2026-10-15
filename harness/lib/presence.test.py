from datetime import datetime, timedelta
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import presence


class PresenceScenarios(unittest.TestCase):
    def setUp(self):
        self.fixture = json.loads(Path(__file__).with_name('presence-nightly.fixture.json').read_text())
        turns = []
        for day in self.fixture['days']:
            at = datetime.fromisoformat(f"{day}T{self.fixture['awake_from']}:00Z")
            end = datetime.fromisoformat(f"{day}T{self.fixture['awake_until']}:00Z")
            while at <= end:
                turns.append({'at': at.isoformat(), 'label': 'exact'})
                at += timedelta(minutes=15)
        self.signals = {'turns': turns, 'timezone': self.fixture['timezone']}

    def test_nightly_gap_forecasts_sleep_and_live_activity_overrides_it(self):
        result = presence.presence('2026-10-14T17:30:00Z', self.signals)
        self.assertEqual(result['state'], 'present')
        self.assertEqual(result['next']['state'], 'asleep')
        self.assertEqual(result['next']['at'], '2026-10-14T18:00:00Z')
        now = '2026-10-14T19:00:00Z'
        self.assertEqual(presence.presence(now, self.signals)['state'], 'asleep')
        live = dict(self.signals, desktop={'active': True, 'at': now})
        self.assertEqual(presence.presence(now, live)['state'], 'present')
        live['desktop']['active'] = False
        self.assertEqual(presence.presence(now, live)['state'], 'asleep')
        self.assertEqual(presence.presence('2026-10-15T01:00:00Z', self.signals)['state'], 'away')

    def test_live_activity_overrides_stated_day_until_input_expires(self):
        now = '2026-10-15T05:00:00Z'
        signals = dict(self.signals, away=[self.fixture['travel']], desktop={'active': True, 'at': now})
        result = presence.presence(now, signals)
        self.assertEqual(result['state'], 'present')
        self.assertEqual(result['since'], '2026-10-15T05:00:00Z')
        self.assertEqual(result['next']['at'], '2026-10-15T05:15:00Z')
        self.assertNotEqual(presence.presence('2026-10-15T16:00:00Z', signals)['state'], 'travelling')

    def test_turn_presence_expires_and_games_never_establish_presence(self):
        now = presence.timestamp('2026-10-15T04:00:00Z')
        signals = {'turns': [{'at': (now + timedelta(minutes=1)).isoformat()}]}
        self.assertEqual(presence.presence(now, signals)['state'], 'away')
        signals['turns'].append({'at': (now - timedelta(minutes=15)).isoformat()})
        self.assertEqual(presence.presence(now, signals)['state'], 'present')
        self.assertEqual(presence.presence(now + timedelta(minutes=17), signals)['state'], 'away')
        self.assertEqual(presence.presence(now, {'gaming': {'active': True, 'at': now.isoformat()}})['state'], 'away')

    def test_typed_input_overrides_travel_only_for_the_presence_slot(self):
        now = presence.timestamp('2026-10-15T05:00:00Z')
        signals = {'away': [self.fixture['travel']], 'turns': [{'at': now.isoformat()}]}
        self.assertEqual(presence.presence(now, signals)['state'], 'present')
        self.assertEqual(presence.presence(now + timedelta(minutes=presence.SLOT_MIN), signals)['state'], 'present')
        self.assertEqual(presence.presence(now + timedelta(minutes=presence.SLOT_MIN, seconds=1), signals)['state'], 'travelling')

    def test_game_left_open_cannot_override_sleep_or_supply_evidence(self):
        now = '2026-10-14T19:00:00Z'
        signals = dict(self.signals, gaming={'active': True, 'at': now})
        result = presence.presence(now, signals)
        self.assertEqual(result['state'], 'asleep')
        self.assertNotIn('gaming', result['evidence'])

    def test_background_pool_waits_for_continuous_idle_and_input_resets_it(self):
        start = presence.timestamp('2026-10-15T04:00:00Z')
        signals = {'idle': [{'at': start.isoformat(), 'idle': True}]}
        edge = start + timedelta(seconds=presence.IDLE_REOPEN_SEC)
        self.assertEqual(presence.presence(edge - timedelta(seconds=1), signals)['pool_state'], 'unknown')
        self.assertEqual(presence.presence(edge, signals)['pool_state'], 'away')
        signals['turns'] = [{'at': (edge - timedelta(minutes=1)).isoformat()}]
        self.assertEqual(presence.presence(edge, signals)['pool_state'], 'present')
        self.assertEqual(presence.presence(edge + timedelta(minutes=16), signals)['pool_state'], 'unknown')
        self.assertEqual(presence.presence(edge + timedelta(minutes=29), signals)['pool_state'], 'away')
        signals['idle'] += [{'at': (edge + timedelta(minutes=29)).isoformat(), 'idle': False},
                            {'at': (edge + timedelta(minutes=30)).isoformat(), 'idle': True},
                            {'at': (edge + timedelta(minutes=31)).isoformat(), 'idle': True}]
        self.assertEqual(presence.presence(edge + timedelta(minutes=59), signals)['pool_state'], 'unknown')
        self.assertEqual(presence.presence(edge + timedelta(minutes=60), signals)['pool_state'], 'away')
        self.assertEqual(presence.presence(edge, {})['pool_state'], 'unknown')

    def test_idle_history_improves_the_forecast_without_claiming_live_presence(self):
        at = presence.timestamp('2026-10-15T04:00:00Z')
        idle = [{'at': (at - timedelta(days=1, hours=2)).isoformat(), 'idle': False},
                {'at': (at - timedelta(days=1) + timedelta(hours=2)).isoformat(), 'idle': True}]
        model = presence.forecast_model(at, [], idle, at.tzinfo)
        self.assertGreater(model(at), 0.5)
        self.assertEqual(presence.presence(at, {'idle': idle})['state'], 'away')

    def test_live_reader_and_cli_use_only_fixture_state_and_validate_the_seat_process(self):
        with tempfile.TemporaryDirectory(prefix='presence-') as temporary:
            root = Path(temporary)
            runtime, operator, index_dir = root / 'runtime', root / 'operator', root / 'convo'
            for path in (runtime, operator, index_dir):
                path.mkdir()
            index = index_dir / 'index-v4.db'
            at = '2026-10-15T05:00:00Z'
            transcript = root / 'session.jsonl'
            transcript.write_text(json.dumps({'origin': {'kind': 'human'}}) + '\n' +
                                  json.dumps({'origin': {'kind': 'agent'}}) + '\n')
            with closing(sqlite3.connect(index)) as db:
                db.executescript('CREATE TABLE files(id INTEGER,path TEXT,provider TEXT,agent TEXT,cwd TEXT,off INTEGER);'
                                 'CREATE TABLE msg(file_id INTEGER,line INTEGER,ts TEXT,role TEXT);'
                                 'CREATE INDEX msg_file ON msg(file_id,line);')
                db.execute('INSERT INTO files VALUES(1,?,\'anthropic\',NULL,?,?)',
                           (str(transcript), str(Path.home()), transcript.stat().st_size))
                db.executemany('INSERT INTO msg VALUES(1,?,?,\'user\')', [(1, at), (2, at)])
                db.commit()
            fresh = '2026-10-15T05:01:00Z'
            with transcript.open('a') as stream:
                stream.write(json.dumps({'origin': {'kind': 'human'}, 'timestamp': fresh}) + '\n')
            (operator / 'presence.toml').write_text('[[away]]\nfrom = "2026-10-14T16:00:00Z"\n'
                                                 'to = "2026-10-15T16:00:00Z"\nkind = "travel"\nsaid = "away"\n')
            desktop = runtime / 'desktop-idle.json'
            desktop.write_text(json.dumps({'pid': 123, 'started': '100', 'idle': False}))
            proc = root / 'proc/123'
            proc.mkdir(parents=True)
            stat = proc / 'stat'
            stat.write_text('123 (swayidle) ' + ' '.join(['S', '1'] + ['0'] * 17 + ['100']))
            env = dict(os.environ, XDG_STATE_HOME=str(root), XDG_RUNTIME_DIR=str(runtime),
                       AGENTS_OPERATOR_DIR=str(operator), CONVO_STATE=str(index_dir), LAPTOP_STATE=str(root / 'laptop.json'))
            (root / 'laptop.json').write_text(json.dumps({'reason': 'gaming', 'by': 'laptop-auto'}))
            (root / 'laptop-observations.json').write_text(json.dumps({'at': fresh}))
            with patch.dict(os.environ, env):
                signals = presence.live_signals(presence.timestamp(fresh), root / 'proc')
                self.assertTrue(signals['desktop']['active'])
                self.assertNotIn('gaming', signals)
                self.assertEqual(signals['turns'], [{'at': at, 'label': 'exact'}, {'at': fresh, 'label': 'exact'}])
                stat.write_text('123 (swayidle) ' + ' '.join(['S', '1'] + ['0'] * 17 + ['101']))
                self.assertNotIn('desktop', presence.live_signals(presence.timestamp(at), root / 'proc'))
            command = Path(__file__).resolve().parents[2] / 'bin/agents'
            result = subprocess.run([str(command), 'presence', '--json', '--at', at], env=env,
                                    text=True, capture_output=True, check=True)
            self.assertEqual(json.loads(result.stdout)['state'], 'present')


if __name__ == '__main__':
    unittest.main(verbosity=2)
