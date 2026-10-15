from __future__ import annotations

import json
import uuid

KINDS = ('KICKOFF', 'STUCK', 'LANDED', 'QUESTION', 'LAND-READY', 'RECYCLE-READY', 'ANSWER', 'LEARN')
FIELDS = ('to_node', 'repo', 'lane', 'sha', 'body', 'packet', 'key', 'root_event', 'answered_by')
STATE = frozenset(('attempted_at', 'delivered_at', 'last_error', 'resend_at', 'parent_escalated_at', 'proxy_escalated_at', 'successor'))


def normalize(row):
    data = json.loads(row['data'])
    return dict(id=row['seq'], time=row['ts'], item=row['item'], kind=row['kind'], from_node=row['actor'],
                **{field: data.get(field) for field in (*FIELDS, *sorted(STATE))})


def migrate(db, schema, transaction):
    with transaction(db):
        sql = db.execute("SELECT sql FROM sqlite_master WHERE name='events'").fetchone()[0]
        if all(f"'{kind}'" in sql for kind in KINDS):
            return
        indexes = [row[0] for row in db.execute("SELECT sql FROM sqlite_master WHERE type='index' AND tbl_name='events' AND sql IS NOT NULL")]
        declaration = next(stmt for stmt in schema.split(';') if 'CREATE TABLE IF NOT EXISTS events(' in stmt)
        db.execute(declaration.replace('CREATE TABLE IF NOT EXISTS events(', 'CREATE TABLE events_widened(', 1))
        db.execute('INSERT INTO events_widened(seq,ts,item,kind,actor,tier,data) SELECT seq,ts,item,kind,actor,tier,data FROM events')
        db.execute('DROP TABLE events')
        db.execute('ALTER TABLE events_widened RENAME TO events')
        for index in indexes:
            db.execute(index)


def addressed(db, event_id):
    row = db.execute('SELECT * FROM events WHERE seq=?', (event_id,)).fetchone()
    if row is None or row['kind'] not in KINDS:
        raise ValueError(f'no addressed event {event_id}')
    return row


def append(db, kind, item, sender, data, timestamp, transaction):
    if kind not in KINDS or not sender.strip() or not data.get('to_node', '').strip():
        raise ValueError('agent-event requires a supervisor kind and addressed sender/receiver')
    data = dict(data)
    data['key'] = str(uuid.uuid4()) if data.get('key') is None else data['key']
    if not isinstance(data['key'], str) or not data['key'].strip():
        raise ValueError('key must be nonempty text')
    data['answered_by'] = None
    answer = data.get('answer')
    if (kind == 'ANSWER') != (answer is not None):
        raise ValueError('ANSWER requires --answer; other kinds cannot use --answer')
    with transaction(db):
        existing = db.execute("SELECT * FROM events WHERE kind IN (" + ','.join('?' for _ in KINDS) + ") AND json_extract(data,'$.key')=?", (*KINDS, data['key'])).fetchone()
        if existing:
            original = json.loads(existing['data'])
            original['answered_by'] = None
            original = {key: value for key, value in original.items() if key not in STATE}
            if (existing['kind'], existing['item'], existing['actor'], original) != (kind, item, sender, data):
                raise ValueError('key already names a different payload')
            return normalize(existing)
        targets = []
        if data.get('root_event') is not None:
            root = addressed(db, data['root_event'])
            if root['kind'] == 'ANSWER':
                raise ValueError('root event cannot be an ANSWER')
        if answer is not None:
            target = addressed(db, answer)
            target_data = json.loads(target['data'])
            if target['kind'] == 'ANSWER' or target_data.get('answered_by') is not None:
                raise ValueError('event already answered or is an ANSWER')
            if sender not in (target_data.get('to_node'), 'proxy'):
                raise ValueError('only the recipient or proxy may answer')
            if data['to_node'] != target['actor']:
                raise ValueError('answer recipient must be the event sender')
            targets = [target]
            linked_root = target_data.get('root_event')
            if linked_root is not None:
                targets.append(addressed(db, linked_root))
            if data.get('root_event') is not None and data['root_event'] not in {row['seq'] for row in targets}:
                raise ValueError('answer root must match the referenced event root')
            targets = [row for row in targets if json.loads(row['data']).get('answered_by') is None]
        cursor = db.execute('INSERT INTO events(ts,item,kind,actor,data) VALUES (?,?,?,?,?)', (timestamp, item, kind, sender, json.dumps(data)))
        event_id = cursor.lastrowid
        for target in targets:
            payload = json.loads(target['data'])
            payload['answered_by'] = event_id
            db.execute('UPDATE events SET data=? WHERE seq=?', (json.dumps(payload), target['seq']))
        return normalize(addressed(db, event_id))


def pending(db, event_id=None):
    query = 'SELECT * FROM events WHERE kind IN (' + ','.join('?' for _ in KINDS) + ')'
    args = list(KINDS)
    if event_id is None:
        query += " AND kind != 'ANSWER' AND json_extract(data,'$.answered_by') IS NULL"
    else:
        query += ' AND seq=?'
        args.append(event_id)
    return [normalize(row) for row in db.execute(query + ' ORDER BY seq', args)]


def update_state(db, event_id, changes, transaction):
    if not isinstance(changes, dict) or changes.keys() - STATE:
        raise ValueError('update-json permits delivery state keys only')
    with transaction(db):
        row = addressed(db, event_id)
        data = json.loads(row['data'])
        data.update(changes)
        db.execute('UPDATE events SET data=? WHERE seq=?', (json.dumps(data), event_id))
        return normalize(addressed(db, event_id))
