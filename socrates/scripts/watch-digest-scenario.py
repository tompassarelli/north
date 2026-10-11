#!/usr/bin/env python3
"""Recorded public watch scenario; --live exercises Claude and the test topic."""

import argparse
from datetime import datetime, timezone
import http.server
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'socrates/lib'))
import usage
import watch_digest



def operator_dir():
    configured = os.environ.get('AGENTS_OPERATOR_DIR')
    if configured:
        return Path(configured)
    return Path.home() / 'code/south/main/agents'

def agreement(items, message, news):
    for item in items:
        block = f"[{item['id']}] {item['title']}\n{item['why']}\n{item['url']}"
        assert block in message and block in news, 'digest/news disagreement: ' + item['id']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--live', action='store_true')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='watch-digest-scenario-') as directory:
        work = Path(directory)
        state = {'hits': []}
        release = {'new': False}

        class Feed(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                if self.path == '/releases':
                    entry = '<entry><id>baseline</id><title>baseline release</title><link href="https://example.com/baseline"/></entry>'
                    if release['new']:
                        entry += '<entry><id>new</id><title>Introducing GPT-6.1</title><link href="https://example.com/release"/></entry>'
                    payload = ('<feed xmlns="http://www.w3.org/2005/Atom">' + entry + '</feed>').encode()
                else:
                    payload = json.dumps(state).encode()
                self.send_response(200)
                self.end_headers()
                self.wfile.write(payload)

            def log_message(self, *unused):
                pass

        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Feed)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            db_path = work / 'threads.db'
            env = dict(os.environ, THREADS_DB=str(db_path), PYTHONDONTWRITEBYTECODE='1',
                       AGENTS_ORCHESTRATION=str(operator_dir() / 'orchestration.toml'))
            fixture_bin = work / 'bin'
            fixture_bin.mkdir()
            if not args.live:
                (fixture_bin / 'claude').write_text('''#!/usr/bin/env python3
import json,os,sys
if sys.argv[1:]==['auth','status']:
 print(json.dumps(dict(loggedIn=True,authMethod='claude.ai',subscriptionType='max')))
else:
 assert os.environ['CLAUDE_CODE_DISABLE_FAST_MODE']=='1'
 prompt=sys.stdin.read(); data=json.loads(prompt.split('\\n',1)[1])
 print(json.dumps(dict(result=json.dumps([dict(id=r['id'],score=.9,why='Useful engine release for game development.') for r in data['candidates']]),session_id='recorded-ranking')))
''')
                (fixture_bin / 'ssh').write_text('''#!/usr/bin/env python3
import json,os,shlex,sys
parts=shlex.split(sys.argv[-1]); assert parts[:2]==['nexus-alert','socrates-test']
with open(os.environ['SCENARIO_INBOX'],'a') as stream: stream.write(json.dumps(dict(message=parts[2]))+'\\n')
''')
                for path in fixture_bin.iterdir():
                    path.chmod(0o755)
                env['PATH'] = str(fixture_bin) + ':' + os.environ['PATH']
            env['SCENARIO_INBOX'] = str(work / 'inbox.jsonl')
            with sqlite3.connect(db_path) as db:
                if args.live:
                    live_path = Path(usage.DB).resolve()
                    with sqlite3.connect(live_path.as_uri() + '?mode=ro', uri=True) as live:
                        rows = usage.latest(live)
                    usage.record(db, rows)
                else:
                    now = datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
                    rows = [dict(ts=now, provider='claude', account=usage.local_account('claude'),
                                 kind='window', name='week', used_pct=10, window_min=10080,
                                 resets_at=None, amount=None, source='recorded scenario')]
                    rows += [dict(rows[0], kind='billing', name=name, amount=0, used_pct=None) for name in ('extra_usage','spend')]
                    usage.record(db, rows)
            interests = work / 'interests.md'
            interests.write_text('- Likes: game engine releases and practical game development tools.\n- Dislikes: unrelated gossip.\n')
            feedback = work / 'feedback.jsonl'
            source = work / 'sources.toml'
            source.write_text('market_move_threshold=0.1\n[[sources]]\nname="recorded-release"\nparser="atom"\nendpoint="http://127.0.0.1:' + str(server.server_port) + '/releases"\n[[digest_sources]]\nname="recorded-hn"\nparser="hn-front-page"\nendpoint="http://127.0.0.1:' + str(server.server_port) + '/feed"\n')
            command = [str(ROOT / 'bin/model-watch'), '--sources', str(source), '--interests', str(interests),
                       '--feedback', str(feedback), '--topic', 'socrates-test', '--proposals', str(work / 'proposals')]

            def run():
                result = subprocess.run(command, env=env, text=True, capture_output=True, timeout=150)
                assert result.returncode == 0, result.stdout + result.stderr
                return json.loads(result.stdout)

            def notes():
                with sqlite3.connect(db_path) as db:
                    if not db.execute("SELECT 1 FROM sqlite_master WHERE name='events'").fetchone():
                        return 0
                    return db.execute("SELECT count(*) FROM events WHERE kind='note'").fetchone()[0]

            baseline = run()
            before = notes()
            quiet = run()
            assert quiet['digest']['invocations'] == 0 and notes() == before
            assert quiet['digest']['entries'] == 0 and not (work / 'inbox.jsonl').exists()
            print('quiet: new=0 rank_calls=0 note_delta=0 digests=0')
            state['hits'] = [dict(objectID='scenario-godot-45', title='Godot 4.5 released: new rendering, physics, and game development tools',
                                 url='https://godotengine.org/releases/4.5/')]
            if not args.live:
                release["new"] = True
            started = int(time.time())
            new = run()
            expected = 1 if args.live else 2
            assert new['digest']['invocations'] == 1 and new['digest']['entries'] == expected and new['digest']['sent'] == 1
            assert notes() == before + 1
            with sqlite3.connect(db_path) as db:
                items = watch_digest.kept(db)
                body = db.execute('SELECT body FROM watch_digests WHERE sent=1 ORDER BY id DESC LIMIT 1').fetchone()[0]
                if not args.live:
                    assert db.execute('SELECT count(*) FROM model_events').fetchone()[0] == 1
                    assert len(list((work / 'proposals').glob('*.toml'))) == 1
                    print('model/release store: model_events=1 proposal_rows=1')
            if args.live:
                remote = "python3 - <<'REMOTE'\nimport json,urllib.request\nfrom pathlib import Path\ntoken=Path('/run/secrets/ntfy-publisher-token').read_text().strip()\nrequest=urllib.request.Request('http://10.77.0.1:2586/socrates-test/json?poll=1',headers={'Authorization':'Bearer '+token})\nwith urllib.request.urlopen(request,timeout=20) as r: print(r.read().decode())\nREMOTE"
                received = subprocess.run(['ssh', '-o', 'BatchMode=yes', 'nexus', remote], text=True, capture_output=True, timeout=30)
                assert received.returncode == 0, 'test-topic readback failed'
                messages = [json.loads(line) for line in received.stdout.splitlines() if line.strip()]
                matches = [message for message in messages if message.get('message') == body and message.get('time', 0) >= started]
                assert len(matches) == 1, 'expected exactly one matching test-topic message'
                message = matches[0]
                print('ntfy_reply: ' + json.dumps(message, sort_keys=True))
            else:
                messages = [json.loads(line) for line in (work / 'inbox.jsonl').read_text().splitlines()]
                assert len(messages) == 1
                message = messages[0]
            news_command = [str(ROOT / 'bin/agents'), 'news', '--feedback', str(feedback)]
            news = subprocess.run(news_command, env=env, capture_output=True, text=True, check=True).stdout
            agreement(items, message['message'], news)
            try:
                agreement(items, message['message'].replace(items[0]['url'], 'https://example.invalid/corrupt'), news)
            except AssertionError:
                print('injected link fault: detected digest/news disagreement')
            else:
                raise AssertionError('agreement detector accepted a corrupt link')
            print(f'new: candidates={expected} rank_calls=1 note_delta=1 digests=1 links={expected} news_agreement=1')
            similar = dict(source='recorded-hn', title='Godot rendering and game development improvements')
            initial = watch_digest.pre_score(similar, [])
            feedback_item = next(item for item in items if item['source'] == 'recorded-hn')
            subprocess.run(news_command + ['--noise', feedback_item['id']], env=env, capture_output=True, text=True, check=True)
            demoted = watch_digest.pre_score(similar, watch_digest.feedback_rows(feedback))
            assert demoted < initial and notes() == before + 1
            print(f'noise: pre_score={initial:.2f}->{demoted:.2f} fixed_rank={0.9 + initial:.2f}->{0.9 + demoted:.2f} rank_calls=0 note_delta=0')
            repeated = run()
            assert repeated['digest']['invocations'] == 0 and notes() == before + 1
            print('repeat: rank_calls=0 note_delta=0 digests=0')
        finally:
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    main()
