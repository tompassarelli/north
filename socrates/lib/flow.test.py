import json
import os
from pathlib import Path
import tempfile
import unittest

import flow


class SmashcraftReference(unittest.TestCase):
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
                self.assertEqual(len(messages), 4)
                escalations = [message for message in messages if 'Needs Tom:' in message['text']]
                self.assertEqual({message['node'] for message in escalations}, {'proxy', 'smashcraft-lead'})
                self.assertEqual(len(escalations), 2)
                self.assertEqual(len(inbox.read_text().splitlines()), 2)
                healthy = {'repo': 'smashcraft', 'main_sha': stalled['main_sha'],
                           'stall': flow.stall_verdict(prolonged, prolonged, [prolonged], 0, [], {})}
                flow.stall_alert([healthy], prolonged)
                self.assertIn('smashcraft', json.loads(state_path.read_text()))
                healthy['main_sha'] = 'recovered'
                flow.stall_alert([healthy], prolonged)
                self.assertEqual(json.loads(state_path.read_text()), {})
                flow.stall_alert([stalled], prolonged + 300)
                self.assertEqual(len(list(spool.glob('*.json'))), 6)
            finally:
                for key, value in [('XDG_STATE_HOME', old_state), ('AGENTS_ORG_FILE', old_org)]:
                    if value is None:
                        os.environ.pop(key, None)
                    else:
                        os.environ[key] = value
        print(f'first alert {flow.datetime.fromtimestamp(first, flow.timezone.utc).isoformat()} '
              'to smashcraft-lead + Plato + lead inbox; healthy 06:00–09:00Z: 0 alerts')


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
        print(flow.format_row({**row(17, quiet, lines, 1, reuse, red), 'landed_per_hour': 3, 'push_to_land_minutes': None,
                               'push_to_land_p90_minutes': None, 'main_age_minutes': 0, 'oldest_job': None}))


if __name__ == '__main__':
    unittest.main()
