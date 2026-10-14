#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class DurableEvents(unittest.TestCase):
    def test_recorded_store_journey(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'target/durable-events-evidence') as scratch:
            db = Path(scratch) / 'threads.db'
            env = dict(os.environ, THREADS_DB=str(db), PYTHONDONTWRITEBYTECODE='1')
            def run(*args, ok=True, restricted=False):
                result = subprocess.run([str(ROOT / 'bin/threads'), *args], env=dict(env, THREADS_NO_NATIVE='1') if restricted else env, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0 if ok else 1, result.stderr)
                return result.stdout.strip() if args[0] == 'new' else json.loads(result.stdout) if result.stdout.strip() else None
            run('new', 'ops', 'fixture')
            with sqlite3.connect(db) as conn:
                old = conn.execute('SELECT * FROM events').fetchall()
                conn.execute('ALTER TABLE events RENAME TO events_seed')
                conn.execute("CREATE TABLE events(seq INTEGER PRIMARY KEY,ts TEXT NOT NULL,item TEXT,kind TEXT NOT NULL CHECK(kind IN ('claim','release','handoff','need','unneed','run','note','capture','close','reopen')),actor TEXT NOT NULL DEFAULT '',tier TEXT NOT NULL DEFAULT '',data TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(data))) STRICT")
                conn.execute('INSERT INTO events SELECT * FROM events_seed')
                conn.execute('DROP TABLE events_seed')
                conn.execute('CREATE INDEX events_item ON events(item,seq)')
            event = run('agent-event', 'STUCK', 'ops#1', '--from-node', 'worker', '--to-node', 'lead', '--body', 'blocked', '--key', 'seed')
            run('agent-event', 'ANSWER', '--from-node', 'lead', '--to-node', 'worker', '--body', 'restricted', '--answer', str(event['id']), ok=False, restricted=True)
            self.assertEqual(run('agent-events', restricted=True), [])
            replay = run('agent-event', 'STUCK', 'ops#1', '--from-node', 'worker', '--to-node', 'lead', '--body', 'blocked', '--key', 'seed')
            self.assertEqual(event, replay)
            run('agent-event', 'STUCK', '--from-node', 'worker', '--to-node', 'lead', '--body', 'changed', '--key', 'seed', ok=False)
            state = run('agent-event-state', str(event['id']), '--update-json', '{"delivered_at":"now"}')
            self.assertEqual(state['delivered_at'], 'now')
            run('agent-event-state', str(event['id']), '--update-json', '{"body":"changed"}', ok=False)
            other = run('agent-event', 'QUESTION', '--from-node', 'worker', '--to-node', 'lead', '--body', 'unanswered')
            linked = run('agent-event', 'QUESTION', '--from-node', 'worker', '--to-node', 'proxy', '--body', 'escalate', '--root-event', str(event['id']))
            run('agent-event', 'ANSWER', '--from-node', 'intruder', '--to-node', 'worker', '--body', 'no', '--answer', str(linked['id']), ok=False)
            answer = run('agent-event', 'ANSWER', '--from-node', 'proxy', '--to-node', 'worker', '--body', 'yes', '--answer', str(linked['id']), '--key', 'answer')
            self.assertEqual(run('agent-events', '--id', str(event['id']))[0]['answered_by'], answer['id'])
            self.assertEqual([e['id'] for e in run('agent-events')], [other['id']])
            self.assertEqual(run('agent-event', 'ANSWER', '--from-node', 'proxy', '--to-node', 'worker', '--body', 'yes', '--answer', str(linked['id']), '--key', 'answer')['id'], answer['id'])
            with sqlite3.connect(db) as conn:
                self.assertEqual(conn.execute('SELECT * FROM events WHERE seq <= ?', (old[-1][0],)).fetchall(), old)
                self.assertEqual(conn.execute("SELECT name FROM sqlite_master WHERE type='index' AND name='events_item'").fetchone()[0], 'events_item')

    def test_canonical_transport_boundary(self):
        with tempfile.TemporaryDirectory(dir=ROOT / 'target/durable-events-evidence') as scratch:
            home = Path(scratch)
            (home / '.config/threads').mkdir(parents=True)
            (home / '.config/threads/remote').write_text('canonical')
            (home / 'bin').mkdir()
            peer = home / 'bin/ssh'
            peer.write_text('#!/bin/sh\nexit 255\n')
            peer.chmod(0o700)
            env = dict(os.environ, HOME=str(home), PATH=f'{home}/bin:{os.environ["PATH"]}', PYTHONDONTWRITEBYTECODE='1')
            env.pop('THREADS_DB', None)
            snapshot = home / '.local/state/threads/snapshot.db'
            snapshot.parent.mkdir(parents=True)
            snapshot.write_bytes(b'retained snapshot')
            def run(*args):
                return subprocess.run([str(ROOT / 'bin/threads'), *args], env=env, capture_output=True, text=True)
            self.assertEqual(run('agent-events').returncode, 255)
            self.assertEqual(run('agent-event-state', '1', '--update-json', '{}').returncode, 255)
            self.assertEqual(snapshot.read_bytes(), b'retained snapshot')
            self.assertEqual(run('agent-event', 'STUCK', '--from-node', 'worker', '--to-node', 'lead', '--body', 'offline').returncode, 0)
            spool = home / '.local/state/threads/spool'
            packet = next(spool.glob('*.argv'))
            argv = json.loads(packet.read_text())
            self.assertIn('--key', argv)
            self.assertTrue(argv[argv.index('--key') + 1])
            self.assertEqual(packet.stat().st_mode & 0o777, 0o600)
            self.assertEqual(spool.stat().st_mode & 0o777, 0o700)
            peer.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$HOME/replay"\ncase "$*" in *"threads agent-event "*) printf \'{"id":1}\\n\';; *"threads agent-events"*) printf \'[]\\n\';; esac\nexit 0\n')
            online = run('agent-events')
            self.assertEqual(online.returncode, 0)
            self.assertEqual(online.stdout, '[]\n')
            self.assertFalse(packet.exists())
            self.assertIn(argv[argv.index('--key') + 1], (home / 'replay').read_text())


if __name__ == '__main__':
    unittest.main()
