#!/usr/bin/env python3
import json
import concurrent.futures
import importlib.machinery
import importlib.util
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
EVIDENCE = ROOT / 'target/durable-events-evidence'


class DurableEvents(unittest.TestCase):
    def setUp(self):
        EVIDENCE.mkdir(parents=True, exist_ok=True)

    def test_concurrent_opens_preserve_fresh_and_existing_ledgers(self):
        loader = importlib.machinery.SourceFileLoader('migration_threads', str(ROOT / 'bin/threads'))
        module = importlib.util.module_from_spec(importlib.util.spec_from_loader(loader.name, loader))
        loader.exec_module(module)
        for existing in (False, True):
            with self.subTest(existing=existing), tempfile.TemporaryDirectory(dir=EVIDENCE) as scratch:
                module.DB = str(Path(scratch) / 'threads.db')
                if existing:
                    with sqlite3.connect(module.DB) as db:
                        db.executescript(module.SCHEMA)
                        db.execute("INSERT INTO runs(agent,item,tier,category,ended,minutes,outcome) "
                                   "VALUES ('seed','ops#1','medium','tooling','2026-10-15T00:00:00Z',1,'done')")
                inspected = threading.Event()
                contending = threading.Event()
                finished = threading.Event()
                role = threading.local()
                connect = sqlite3.connect

                class ScheduledConnection(sqlite3.Connection):
                    def execute(self, sql, *args):
                        if role.name == 'second' and sql == 'BEGIN IMMEDIATE':
                            contending.set()
                        cursor = super().execute(sql, *args)
                        if sql == 'PRAGMA table_info(runs)':
                            rows = cursor.fetchall()
                            if role.name == 'first':
                                inspected.set()
                                if not contending.wait(5):
                                    raise TimeoutError('second opener never contended')
                            else:
                                contending.set()
                                if not self.in_transaction and not finished.wait(5):
                                    raise TimeoutError('first opener never finished')
                            return rows
                        return cursor

                def open_ledger(name):
                    role.name = name
                    try:
                        db = module.connect()
                        db.close()
                    finally:
                        if name == 'first':
                            finished.set()

                with patch.object(module.sqlite3, 'connect', side_effect=lambda *args, **kwargs:
                                  connect(*args, **kwargs, factory=ScheduledConnection)):
                    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
                        first = pool.submit(open_ledger, 'first')
                        self.assertTrue(inspected.wait(5))
                        second = pool.submit(open_ledger, 'second')
                        first.result(timeout=10)
                        second.result(timeout=10)
                with sqlite3.connect(module.DB) as db:
                    self.assertEqual(db.execute('PRAGMA integrity_check').fetchone()[0], 'ok')
                    self.assertIn('brief_sha256', {row[1] for row in db.execute('PRAGMA table_info(runs)')})
                    self.assertEqual(db.execute('SELECT agent,item,tier,outcome FROM runs').fetchall(),
                                     [('seed', 'ops#1', 'medium', 'done')] if existing else [])

    def test_recorded_store_journey(self):
        with tempfile.TemporaryDirectory(dir=EVIDENCE) as scratch:
            db = Path(scratch) / 'threads.db'
            env = dict(os.environ, THREADS_DB=str(db), PYTHONDONTWRITEBYTECODE='1', NORTH_BIN=os.environ.get('NORTH_PACE_BIN', 'north'))
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
            root = run('agent-event', 'STUCK', '--from-node', 'worker', '--to-node', 'lead', '--body', 'delegated')
            forwarded = run('agent-event', 'QUESTION', '--from-node', 'worker', '--to-node', 'specialist', '--body', 'delegate', '--root-event', str(root['id']))
            run('agent-event', 'ANSWER', '--from-node', 'lead', '--to-node', 'worker', '--body', 'wrong exact recipient', '--answer', str(forwarded['id']), ok=False)
            run('agent-event', 'ANSWER', '--from-node', 'specialist', '--to-node', 'stranger', '--body', 'wrong destination', '--answer', str(forwarded['id']), ok=False)
            original = run('agent-event', 'ANSWER', '--from-node', 'lead', '--to-node', 'worker', '--body', 'root solved', '--answer', str(root['id']))
            final = run('agent-event', 'ANSWER', '--from-node', 'specialist', '--to-node', 'worker', '--body', 'forward solved', '--answer', str(forwarded['id']))
            self.assertEqual(run('agent-events', '--id', str(root['id']))[0]['answered_by'], original['id'])
            self.assertEqual(run('agent-events', '--id', str(forwarded['id']))[0]['answered_by'], final['id'])
            delegated = run('agent-event', 'QUESTION', '--from-node', 'worker', '--to-node', 'lead', '--body', 'new delegated')
            child = run('agent-event', 'QUESTION', '--from-node', 'worker', '--to-node', 'specialist', '--body', 'delegate', '--root-event', str(delegated['id']))
            resolved = run('agent-event', 'ANSWER', '--from-node', 'specialist', '--to-node', 'worker', '--body', 'solved', '--answer', str(child['id']))
            self.assertEqual(run('agent-events', '--id', str(delegated['id']))[0]['answered_by'], resolved['id'])
            with sqlite3.connect(db) as conn:
                self.assertEqual(conn.execute('SELECT * FROM events WHERE seq <= ?', (old[-1][0],)).fetchall(), old)
                self.assertEqual(conn.execute("SELECT name FROM sqlite_master WHERE type='index' AND name='events_item'").fetchone()[0], 'events_item')

    def test_canonical_transport_boundary(self):
        with tempfile.TemporaryDirectory(dir=EVIDENCE) as scratch:
            home = Path(scratch)
            (home / '.config/threads').mkdir(parents=True)
            (home / '.config/threads/remote').write_text('canonical')
            (home / 'bin').mkdir()
            peer = home / 'bin/ssh'
            peer.write_text('#!/bin/sh\nexit 255\n')
            peer.chmod(0o700)
            env = dict(os.environ, HOME=str(home), PATH=f'{home}/bin:{os.environ["PATH"]}', PYTHONDONTWRITEBYTECODE='1', NORTH_BIN=os.environ.get('NORTH_PACE_BIN', 'north'))
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
