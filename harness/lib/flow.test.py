import hashlib
import json
import os
import sqlite3
from pathlib import Path
import tempfile
import unittest

import flow


class SmashcraftReference(unittest.TestCase):
    def test_spawn_recording_flow_boundary(self):
        with tempfile.TemporaryDirectory() as temporary:
            previous = {key: os.environ.get(key) for key in ('XDG_STATE_HOME', 'THREADS_DB')}
            os.environ['XDG_STATE_HOME'] = temporary
            os.environ['THREADS_DB'] = str(Path(temporary) / 'runs.db')
            try:
                flags = Path(temporary) / 'agents/worker-sweep/metadata-parent.json'
                flags.parent.mkdir(parents=True)
                flags.write_text(json.dumps(['spawn metadata missing spec: lead fixture-lead worker child']))
                self.assertEqual(flow.spawn_metadata_flags(), ['spawn metadata missing spec: lead fixture-lead worker child'])
                with sqlite3.connect(os.environ['THREADS_DB']) as db:
                    db.execute('CREATE TABLE runs(role TEXT,outcome TEXT,category TEXT,spec TEXT,scope TEXT,surface TEXT,verify TEXT,brief_text TEXT,brief_origin TEXT)')
                    values = ('worker','done','tooling','measured','module','shell','native','Category: tooling\nSpec: measured\nScope: module\nSurface: shell\nVerify: native','codex.PostToolUse.spawn_agent')
                    db.execute('INSERT INTO runs VALUES(?,?,?,?,?,?,?,?,?)', values)
                    db.execute('INSERT INTO runs VALUES(?,?,?,?,?,?,?,?,?)', ('worker','running', *values[2:]))
                    db.execute('INSERT INTO runs VALUES(?,?,?,?,?,?,?,?,?)', (*values[:7], None, values[8]))
                    db.execute('INSERT INTO runs VALUES(?,?,?,?,?,?,?,?,?)', (*values[:3], None, *values[4:]))
                    db.execute('INSERT INTO runs VALUES(?,?,?,?,?,?,?,?,?)', (*values[:8], 'claude.PostToolUse.Agent'))
                    db.execute('INSERT INTO runs VALUES(?,?,?,?,?,?,?,?,?)', ('worker','running', *values[2:8], 'claude.PostToolUse.Agent'))
                self.assertEqual(flow.brief_coverage(), 2)
            finally:
                for key, value in previous.items():
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value

    def test_recorded_11_oct_landing_window(self):
        fixture = json.loads(Path(__file__).with_name('flow-smashcraft-2026-10-11.json').read_text())
        runs = [dict(zip(fixture['run_fields'], values), workflowName='Autoland') for values in fixture['runs']]
        jobs = [dict(zip(fixture['job_fields'], values), workflow='autoland.yml') for values in fixture['jobs']]
        nodes = [{'id': 'smashcraft-lead', 'domain': 'smashcraft', 'role': 'lead', 'state': 'active'}]

        def row(now):
            landings = [at for at in fixture['landings'] if at <= now]
            active = [{**job, 'started_at': flow.timestamp(job['started_at']), 'runner': job['runner'] or 'waiting'}
                      for job in jobs if flow.timestamp(job['started_at']) <= now
                      and (not job['completed_at'] or now < flow.timestamp(job['completed_at']))]
            pending = sum(flow.timestamp(item['createdAt']) <= now
                          and (not item['conclusion'] or now < flow.timestamp(item['updatedAt']))
                          for item in runs if flow.landing_run(item))
            return {'repo': 'smashcraft', 'main_sha': str(max(landings)),
                    'stall': flow.stall_verdict(now, max(landings), landings, pending, active,
                                               {'autoland.yml': flow.workflow_median(runs, now)})}

        events = {}
        for window, bounds in fixture['windows'].items():
            state = {}
            events[window] = []
            for now in range(int(flow.timestamp(bounds[0])), int(flow.timestamp(bounds[1])) + 1, 300):
                state, actions = flow.alert_plan([row(now)], state, now, nodes)
                events[window].extend((now, action) for action in actions)
                for action in actions:
                    state[action['repo']]['sent'].append(action['key'])
        self.assertEqual(events['healthy'], [])
        first = min(at for at, action in events['stall'])
        self.assertGreaterEqual(first, flow.timestamp('2026-10-11T13:36:00Z'))
        self.assertLessEqual(first, flow.timestamp('2026-10-11T14:10:00Z'))
        recipients = {(action['channel'], action['node']) for at, action in events['stall'] if at == first}
        self.assertEqual(recipients, {('chat', 'smashcraft-lead'), ('chat', 'proxy'), ('inbox', 'smashcraft-lead')})
        stalled = row(first)
        self.assertEqual(stalled['stall']['runner'], 'vast-smashcraft-8')
        with tempfile.TemporaryDirectory(prefix='flow-replay-') as directory:
            old_state = os.environ.get('XDG_STATE_HOME')
            old_org = os.environ.get('AGENTS_ORG_FILE')
            os.environ['XDG_STATE_HOME'] = directory
            os.environ['AGENTS_ORG_FILE'] = str(Path(directory) / 'org.json')
            Path(os.environ['AGENTS_ORG_FILE']).write_text(json.dumps({'version': 1, 'nodes': nodes}))
            try:
                spool = Path(directory) / 'socrates-chat/outbox'
                state_path = Path(directory) / 'agents/flow/stalls.json'
                inbox = Path(directory) / 'agents/handoffs/smashcraft-lead-inbox.md'
                flow.stall_alert([stalled], first)
                flow.stall_alert([stalled], first + 300)
                messages = [json.loads(path.read_text()) for path in spool.glob('*.json')]
                self.assertEqual({message['node'] for message in messages}, {'proxy', 'smashcraft-lead'})
                self.assertEqual(len(messages), 2)
                self.assertTrue(all(message['kind'] == 'notice' for message in messages))
                self.assertEqual(len(inbox.read_text().splitlines()), 1)
                initial = state_path.read_text()
                flow.stall_alert([{'repo': 'smashcraft', 'stall': 'error'}], first + 600)
                self.assertEqual(state_path.read_text(), initial)
                prolonged = first + 3600
                job = next(job for job in jobs if job['run_id'] == 38143883634 and job['name'] == 'reports')
                job = {**job, 'started_at': flow.timestamp(job['started_at'])}
                fault = {'repo': 'smashcraft', 'main_sha': 'moved',
                         'stall': flow.stall_verdict(prolonged, prolonged, [prolonged], 0, [job],
                                                    {'autoland.yml': flow.workflow_median(runs, first)})}
                self.assertTrue(fault['stall']['stalled'])
                self.assertEqual(len(fault['stall']['overdue_jobs']), 1)
                flow.stall_alert([fault], prolonged)
                flow.stall_alert([fault], prolonged + 300)
                messages = [json.loads(path.read_text()) for path in spool.glob('*.json')]
                self.assertEqual(len(messages), 2)
                self.assertEqual(len(inbox.read_text().splitlines()), 1)
                healthy = {'repo': 'smashcraft', 'main_sha': stalled['main_sha'],
                           'stall': flow.stall_verdict(prolonged, prolonged, [prolonged], 0, [], {})}
                flow.stall_alert([healthy], prolonged)
                self.assertIn('smashcraft', json.loads(state_path.read_text()))
                healthy['main_sha'] = 'recovered'
                flow.stall_alert([healthy], prolonged)
                self.assertEqual(json.loads(state_path.read_text()), {})
                flow.stall_alert([stalled], prolonged + 300)
                self.assertEqual(len(list(spool.glob('*.json'))), 4)
            finally:
                for key, value in [('XDG_STATE_HOME', old_state), ('AGENTS_ORG_FILE', old_org)]:
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value
        print(f'first alert {flow.datetime.fromtimestamp(first, flow.timezone.utc).isoformat()} '
              'to smashcraft-lead + Plato + lead inbox; healthy 06:00–09:00Z: 0 alerts')


class LandRatio(unittest.TestCase):
    def test_landings_count_lanes_landed_not_main_commits(self):
        def tip(state, description):
            return {'oid': 'x', 'status': {'contexts': [
                {'context': 'autoland', 'state': state, 'description': description, 'createdAt': '2026-10-12T03:00:00Z'}]}}
        tips = {'tip0': tip('SUCCESS', 'landed as ' + 'a' * 40), 'tip1': tip('SUCCESS', 'landed as ' + 'a' * 40),
                'tip2': tip('PENDING', 'waiting to test alone'), 'tip3': tip('SUCCESS', 'superseded by a later push'),
                'tip4': None}
        self.assertEqual(flow.landed_times(tips), [flow.timestamp('2026-10-12T03:00:00Z')] * 2)


class DeadLanes(unittest.TestCase):
    def test_failed_autoland_lanes_are_dead_not_pending(self):
        now = flow.timestamp('2026-10-12T02:26:00Z')
        failed = {'state': 'FAILURE', 'createdAt': '2026-10-11T12:26:00Z'}
        states = {'a': failed, 'b': {'state': 'ERROR', 'createdAt': '2026-10-12T01:56:00Z'},
                  'c': {'state': 'PENDING', 'createdAt': '2026-10-12T02:20:00Z'}}
        self.assertEqual(flow.lane_counts(['a', 'b', 'c', 'd'], states, now), (2, 2, 840.0))
        self.assertEqual(flow.lane_counts(['c', 'd'], states, now), (2, 0, None))
        row = {'repo': 'smashcraft', 'pending': 2, 'dead': 2, 'dead_oldest_minutes': 840.0,
               'landed_per_hour': 1, 'push_to_land_minutes': 12, 'push_to_land_p90_minutes': 30}
        self.assertIn('smashcraft: pending 2, dead 2 (oldest 840m), landed 1/h', flow.format_row(row))


class CapacityGuards(unittest.TestCase):
    def test_recorded_11_oct_idle_capacity(self):
        now = flow.timestamp('2026-10-11T17:00:00Z')
        # Wisp lan status lines recorded on the vast pool on 11 October, with the driver fields wisp-lead adds.
        lines = [{'pair': 0, 'profile': 'checks', 'fps': 60, 'clients': [{'name': 'lan0a', 'pid': 2361799}, {'name': 'lan0b', 'pid': 2362260}],
                  'driver': {'pid': 2400100, 'since': '2026-10-11T16:10:00Z', 'goneAt': '2026-10-11T16:40:00Z'}},
                 {'pair': 1, 'profile': 'checks', 'fps': 60, 'clients': [{'name': 'lan1a', 'pid': 2363166}, {'name': 'lan1b', 'pid': 2363890}],
                  'driver': {'pid': 2400200, 'since': '2026-10-11T16:55:00Z'}}]
        busy_pool = [{**line, 'state': 'busy', 'idleSeconds': 0} for line in lines]
        landings = [{'context': 'autoland', 'state': 'SUCCESS', 'createdAt': f'2026-10-11T16:{minute:02d}:00Z',
                     'description': f'landed as {minute:040x} census-reused={reused}'}
                    for minute, reused in zip(range(20), ['yes'] * 9 + ['no'] * 11)]
        pushes = [{'headBranch': f'claude/land-{i}', 'headSha': f'{i:040x}', 'createdAt': '2026-10-11T15:00:00Z'}
                  for i in range(20)]
        tips = {f'tip{i}': {'oid': f'{i:040x}', 'status': {'contexts': [landings[i]]}} for i in range(20)}
        _, reuse = flow.autoland_samples(pushes, tips, now)
        quiet = [[now - 900 - 60 + 300 * i, 1] for i in range(5)]
        nodes = [{'id': 'smashcraft-lead', 'domain': 'smashcraft', 'role': 'lead', 'state': 'active'}]
        # smashcraft-client's pin and the wc3-controller v0.3.3 tag build recorded on 11 October (Windows e2e red, no Windows asset).
        manifest = 'wc3-controller-model = { git = "https://github.com/tompassarelli/wc3-controller", tag = "v0.3.3" }\n'
        pins = flow.cargo_pins(['[package]\nname = "smashcraft"\n', manifest])
        self.assertEqual(pins, [('tompassarelli/wc3-controller', 'v0.3.3')])
        red = [(*pins[0], 'completed', 'failure')]

        def row(pending, samples, pairs, waiting, flags, builds):
            return {'repo': 'smashcraft', 'main_sha': 'tip', 'pending': pending,
                    'stall': flow.stall_verdict(now, now, [now], pending, [], {}),
                    'actions': flow.actions_verdict(pending, samples, now),
                    'wisp': flow.wisp_verdict(pairs, waiting, now), 'reuse': flow.reuse_verdict(flags),
                    'pins': flow.pins_verdict(builds)}

        _, actions = flow.alert_plan([row(17, quiet, lines, 1, reuse, red)], {}, now, nodes)
        flagged = {action['key'].split(':')[1] for action in actions}
        self.assertEqual(flagged, {'actions', 'wisp', 'reuse', 'pins'})
        self.assertEqual(len(actions), 12)
        self.assertIn('tompassarelli/wc3-controller@v0.3.3 built failure',
                      next(action['text'] for action in actions if ':pins:' in action['key']))
        busy = [[at, 30] for at, _ in quiet]
        healthy = row(17, busy, busy_pool, 1, [True] * 15 + [False] * 5, [(*pins[0], 'completed', 'success')])
        state, actions = flow.alert_plan([healthy], {}, now, nodes)
        self.assertEqual((state, actions), ({}, []))
        # 18:56Z: pair 0's lan0b had no game process while pair 1 played the #409 soak.
        dead_pool = [{**lines[0], 'state': 'dead', 'idleSeconds': 0, 'down': ['lan0b']}, busy_pool[1]]
        verdict = flow.wisp_verdict(dead_pool, 1, now)
        self.assertTrue(verdict['text'].startswith('wisp 0/2 pairs idle'))
        self.assertIn('lan0b', verdict['reason'])
        self.assertNotIn('idle', verdict['reason'].split(':', 1)[1])
        print(flow.format_row({**row(17, quiet, lines, 1, reuse, red), 'landed_per_hour': 3, 'push_to_land_minutes': None,
                               'push_to_land_p90_minutes': None, 'main_age_minutes': 0, 'oldest_job': None}))


class GoalThroughput(unittest.TestCase):
    def test_recorded_11_12_oct_landings_without_closures(self):
        import tick_precheck
        now = flow.timestamp('2026-10-12T03:45:00Z')
        # Smashcraft 11-12 Oct: about 70 lanes landed in 24 h, each pushed 12 minutes earlier, while open issues fell only 87 to 78.
        landings = [now - 86400 + 1234 * i for i in range(1, 71)]
        pushes = [at - 720 for at in landings]
        main_log = [(hashlib.sha1(str(i).encode()).hexdigest(), at, f'Land lane {i}') for i, at in enumerate(landings)]
        closers = [3, 12, 21, 30, 39, 48, 57, 64, 69]
        closed = [{'number': 400 + i, 'closedAt': datetime_z(landings[i] + 600),
                   'comments': [f'Closed by auto-close after {main_log[i][0][:12]} landed'] if i % 2 else []}
                  for i in closers]
        main_log = [(sha, at, message + (f'\n\nFixes #{400 + i}' if i in closers and not i % 2 else ''))
                    for i, (sha, at, message) in enumerate(main_log)]
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parents[2]) as scratch:
            from unittest.mock import patch
            config = Path(scratch) / 'orchestration.toml'
            config.write_text('[projects.smashcraft]\ntarget = "2026-10-13T03:45:00Z"\n'
                              '[projects.north]\ntarget = "2026-10-12T15:45:00Z"\n[projects.wisp]\n')
            with patch.dict(os.environ, {'AGENTS_ORCHESTRATION': str(config)}):
                targets = flow.project_targets()
        self.assertEqual(set(targets), {'smashcraft', 'north'})
        row = flow.throughput('smashcraft', now, 78, closed, main_log, landings, pushes, targets, [], {}, [now - 900])
        self.assertAlmostEqual(row['closed_per_hour'], 1 / 3)
        self.assertAlmostEqual(row['needed_per_hour'], 78 / 24)
        north = flow.throughput('north', now, 24, [], [], [], [], targets, [], {}, [])
        self.assertEqual(north['needed_per_hour'], 2)
        self.assertEqual(row['land_to_close_minutes'], 10)
        self.assertEqual(row['land_ratio_3h'], 1.0)
        self.assertEqual((row['failed_unowned'], row['oldest_lane_wait_minutes']), (0, 15))
        formatted = {'repo': 'smashcraft', 'pending': 1, 'landed_per_hour': 3, 'push_to_land_minutes': 12,
                     'push_to_land_p90_minutes': 20, **row, 'goal_finish': row['target'] + 3600}
        with tempfile.TemporaryDirectory(dir=Path(__file__).resolve().parents[2]) as scratch:
            (Path(scratch) / 'costs.toml').write_text('[[service]]\nid = "build"\nkind = "cloud"\nusd_per_week = 12.5\n'
                                                   '[[service]]\nid = "host"\nkind = "cloud"\nbilling = "fixed"\nusd_per_week = 7\n')
            with patch.dict(os.environ, {'AGENTS_OPERATOR_DIR': scratch}):
                line = flow.format_row(formatted)
                adequate = flow.format_row({**formatted, 'goal_finish': row['target'] - 60})
                unknown = flow.format_row({**formatted, 'goal_finish': None})
                undated = flow.format_row({**formatted, 'target': None})
        decisions = [part for part in line.splitlines() if part.startswith('Blocked: decision for Tom:')]
        self.assertEqual(len(decisions), 1)
        self.assertIn('more budget (rent build/test compute: build current ledger cost $12.5/week, speedup unmeasured', decisions[0])
        self.assertIn('reconfigure/swap service: host current ledger cost $7/week, speedup unmeasured) / later date / smaller scope', decisions[0])
        for other in (adequate, unknown, undated):
            self.assertNotIn('Blocked: decision for Tom:', other)
        self.assertIn('closed 0.3/h needed 3.2/h, land-to-close 10m, land ratio 1.00 over 3h, failed unowned 0, '
                      'oldest lane wait 15m', line)
        nodes = [{'id': 'smashcraft-lead', 'role': 'lead', 'domain': 'smashcraft', 'session': 's1'},
                 {'id': 'north-lead', 'role': 'lead', 'domain': 'north', 'session': 'n1'}]
        seen = {who: tick_precheck.decide({'flow': line + '\n'}, [], 3, who, nodes)[2]
                for who in ('smashcraft-lead', 'north-lead')}
        self.assertEqual(seen, {'smashcraft-lead': ['flow smashcraft goal closures behind'], 'north-lead': []})
        failed = now - 3600
        stuck = flow.throughput('smashcraft', now, 78, closed, main_log, landings, pushes, None,
                                [(failed, {513}), (failed, {514}), (now - 300, {515})], {514: failed + 60}, [])
        self.assertEqual((stuck['needed_per_hour'], stuck['failed_unowned']), (None, 1))
        print(line)


class LeadBoxes(unittest.TestCase):
    def test_seeded_12_oct_galileo_codex_lead_boxes(self):
        import tick_precheck
        from unittest.mock import patch
        fixture = json.loads((Path(__file__).parent / 'fixtures/galileo-codex-lead-2026-10-12.json').read_text())
        flags = {}
        with tempfile.TemporaryDirectory(prefix='flow-boxes-') as directory:
            nodes = [{**node, 'status_file': str(Path(directory) / node['status_file'])} for node in fixture['nodes']]
            environ = {'XDG_STATE_HOME': directory,
                       'AGENTS_ORCHESTRATION': str(Path(__file__).parent / 'fixtures/orchestration.toml')}
            with patch.dict(os.environ, environ):
                targets = flow.project_targets()
                for at, texts in fixture['ticks']:
                    for node in nodes:
                        if node['id'] in texts:
                            Path(node['status_file']).write_text(fixture['texts'][texts[node['id']]])
                    flags[at[11:16]] = flow.box_alert(nodes, targets, flow.timestamp(at))
        self.assertEqual([at for at, found in flags.items() if found][0], '07:45')
        for at in ('07:45', '07:50', '07:55'):
            self.assertEqual(len(flags[at]), 1)
            self.assertRegex(flags[at][0], r'^box waiting over 10m with no running worker: galileo-codex-lead 8\.14 \(')
        for at in ('08:00', '08:05', '08:10'):
            self.assertEqual(flags[at], ['box staffed by 4 running workers: galileo-codex-lead 8.14 '
                                         '(/root/w814a, /root/w814b, /root/w814c, /root/w814d)'])
        self.assertEqual(len(flags['08:15']), 2)
        self.assertRegex(flags['08:15'][1], r'^box waiting over 10m with no running worker: galileo-codex-lead 8\.15 \(15m\)$')
        output = ''.join(f'agents flow: {flag}\n' for flag in flags['07:45'] + flags['08:00'])
        seen = {who: tick_precheck.decide({'flow': output}, [], 3, who, nodes)[2]
                for who in ('galileo-lead', 'north-lead', 'proxy')}
        self.assertEqual(seen, {'galileo-lead': ['flow box-staffed galileo-codex-lead 8.14', 'flow box-waiting galileo-codex-lead 8.14'],
                                'north-lead': [], 'proxy': seen['galileo-lead']})
        print(output, end='')

    def test_real_status_excerpts_flag_only_true_stalls(self):
        texts = json.loads((Path(__file__).parent / 'fixtures/status-excerpts-2026-10-12.json').read_text())['texts']
        observed = {lead: flow.lead_boxes(text) for lead, text in texts.items()}
        state, _ = flow.box_flags(0, observed, {})
        _, flags = flow.box_flags(11 * 60, observed, state)
        self.assertEqual(sorted(flag.split(' (')[0] for flag in flags), [
            f'box waiting over 10m with no running worker: galileo-codex-lead {box}' for box in ('4.38', '8.14', '8.15')])
        self.assertEqual(flow.lead_boxes('Running:\n- 371/386 ticked\n- 0.74 s against <2 s\n- /tmp/.git\n- 2000=5 within 10%\n'
                                         'Next:\n- 1.37 MB in 0.41 ms\n- 8.8b landed e7915f7\n- stacked behind 8.9 on main\n'),
                         {'8.8b': {'waiting': True, 'workers': []}})
        self.assertEqual(flow.lead_boxes('Running: 8.6 /root/w86\n8.7 note on a landed thing\n'),
                         {'8.6': {'waiting': False, 'workers': ['/root/w86']},
                          '8.7': {'waiting': False, 'workers': []}})


def datetime_z(at):
    return flow.datetime.fromtimestamp(at, flow.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')


if __name__ == '__main__':
    unittest.main()
