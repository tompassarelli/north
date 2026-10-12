"""Shared ranked digest rows and private feedback."""

from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import xml.etree.ElementTree as ET

INTERESTS = Path.home() / 'code/south/main/config/interests.md'
FEEDBACK = Path.home() / 'code/south/main/config/news-feedback.jsonl'
SCHEMA = '''
CREATE TABLE IF NOT EXISTS watch_candidates(
 id TEXT PRIMARY KEY, source TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL,
 detail TEXT NOT NULL CHECK(json_valid(detail)), ranked INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS watch_digests(
 id INTEGER PRIMARY KEY, ts TEXT NOT NULL, body TEXT NOT NULL, sent INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS watch_digest_items(
 digest INTEGER NOT NULL REFERENCES watch_digests(id), id TEXT NOT NULL,
 source TEXT NOT NULL, title TEXT NOT NULL, url TEXT NOT NULL, why TEXT NOT NULL,
 rank REAL NOT NULL, PRIMARY KEY(digest,id)
) STRICT;
'''


def feed_rows(source, payload):
    rows = {}
    if source['parser'] == 'hn-front-page':
        for item in json.loads(payload)['hits'][:100]:
            key = str(item['objectID'])
            rows[key] = dict(title=item['title'], url=item.get('url') or 'https://news.ycombinator.com/item?id=' + key)
    elif source['parser'] == 'feed':
        root = ET.fromstring(payload)
        atom = '{http://www.w3.org/2005/Atom}'
        for item in root.findall('.//item') or root.findall(atom + 'entry'):
            title = item.findtext('title') or item.findtext(atom + 'title') or ''
            link = item.find(atom + 'link')
            url = item.findtext('link') or (link.get('href') if link is not None else '')
            key = item.findtext('guid') or item.findtext(atom + 'id') or url
            if title and url and key:
                rows[key] = dict(title=title, url=url)
    else:
        raise ValueError('unknown digest feed parser')
    return {key: dict(title=' '.join(row['title'].split())[:300], url=row['url']) for key, row in rows.items()
            if isinstance(row['url'], str) and re.match(r'^https?://', row['url'])}


def observe(db, source, rows, ts):
    name = 'digest:' + source
    previous = {key for (key,) in db.execute('SELECT key FROM watch_snapshot WHERE source=?', (name,))}
    added = 0
    with db:
        for key, row in rows.items():
            if previous and key not in previous:
                identity = 'n' + hashlib.sha256((source + ':' + key).encode()).hexdigest()[:16]
                added += db.execute('INSERT OR IGNORE INTO watch_candidates(id,source,title,url,detail) VALUES(?,?,?,?,?)',
                                    (identity, source, row['title'], row['url'], json.dumps(row))).rowcount
        db.execute('DELETE FROM watch_snapshot WHERE source=?', (name,))
        db.executemany('INSERT INTO watch_snapshot VALUES(?,?,?,?)',
                       [(name, key, json.dumps(row), ts) for key, row in {'@baseline': {}, **rows}.items()])
    return added


def feedback_rows(path):
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def pre_score(item, feedback):
    terms = set(re.findall(r'[a-z0-9]{3,}', item['title'].lower()))
    score = 0.0
    latest = {row['id']: row for row in feedback}
    for row in latest.values():
        shared = terms & set(re.findall(r'[a-z0-9]{3,}', row['title'].lower()))
        if item['source'] == row['source'] and shared:
            score += (-0.25 if row['feedback'] == 'noise' else 0.1) * min(1, len(shared) / 2)
    return max(-0.75, min(0.3, score))


def kept(db, days=7):
    row = db.execute('SELECT id,ts FROM watch_digests WHERE sent=1 ORDER BY id DESC LIMIT 1').fetchone()
    if not row or (datetime.now(timezone.utc) - datetime.fromisoformat(row[1].replace('Z', '+00:00'))).days >= days:
        return []
    return [dict(zip(('id', 'source', 'title', 'url', 'why', 'rank'), item)) for item in db.execute(
        'SELECT id,source,title,url,why,rank FROM watch_digest_items WHERE digest=? ORDER BY rank DESC,id', (row[0],))]


def record_feedback(db, identity, label, path):
    item = db.execute('SELECT source,title FROM watch_digest_items WHERE id=? ORDER BY digest DESC LIMIT 1', (identity,)).fetchone()
    if not item:
        raise ValueError('unknown digest item ID')
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a') as stream:
        stream.write(json.dumps(dict(id=identity, source=item[0], title=item[1], feedback=label,
                                    ts=datetime.now(timezone.utc).isoformat())) + '\n')
    path.chmod(0o600)
