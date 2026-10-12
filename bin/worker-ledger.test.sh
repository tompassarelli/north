#!/usr/bin/env bash
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/worker-ledger-test.XXXXXX")
trap 'rm -rf "${scratch:?}"' EXIT
export HOME=$scratch/home XDG_STATE_HOME=$scratch/home/.local/state THREADS_DB=$scratch/threads.db CLAUDE_CONFIG_DIR=$scratch/claude CODEX_CONFIG_DIR=$scratch/codex
export AGY_CONFIG_DIR=$scratch/agy LEDGER_HANDOFFS=$scratch/handoffs
export AGENTS_ORCHESTRATION=$repo/socrates/lib/fixtures/orchestration.toml AGENTS_OPERATOR_DIR=$repo/socrates/lib/fixtures
mkdir "$scratch/bin"
cat >"$scratch/bin/gh" <<'GH'
#!/usr/bin/env bash
if [ "$1" = issue ]; then printf '%s\n' '[{"number":446,"labels":[{"name":"theme:balance"}]}]'; exit; fi
printf '%s\n' '{"data":{"repository":{"issues":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[]}}}}'
GH
chmod +x "$scratch/bin/gh"
export PATH="$scratch/bin:$PATH"
mkdir -p "$LEDGER_HANDOFFS" "$AGY_CONFIG_DIR/log" "$AGY_CONFIG_DIR/brain/0a0a0a0a-0000-4000-8000-000000000001/.system_generated/logs"
printf '%s\n' 'Running: #446 hinted Sol high, ETA 00:20Z, a status line the Smashcraft Codex lead already writes.' >"$LEDGER_HANDOFFS/smashcraft-codex-lead-status.md"
printf '%s\n' 'I1008 printmode.go:202] Print mode: starting (promptLength=9, model="gemini-3.1-pro-high", conversationID="")' \
  'I1008 server.go:1278] Created conversation 0a0a0a0a-0000-4000-8000-000000000001' >"$AGY_CONFIG_DIR/log/cli-20261008_000000.log"
printf '%s\n' \
  '{"type":"USER_INPUT","status":"DONE","created_at":"2026-10-08T00:00:00Z","content":"<USER_REQUEST>\nItem: wisp#71\nCategory: research\nLens: break it\n</USER_REQUEST>"}' \
  '{"type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-08T00:00:00Z","content":"Accepted flaws: 2","input_tokens":800,"output_tokens":120,"cache_read_tokens":100}' \
  >"$AGY_CONFIG_DIR/brain/0a0a0a0a-0000-4000-8000-000000000001/.system_generated/logs/transcript_full.jsonl"
sqlite3 "$AGY_CONFIG_DIR/conversation_summaries.db" "create table conversation_summaries(conversation_id text, workspace_uris text, last_modified_time datetime);
  insert into conversation_summaries values('0a0a0a0a-0000-4000-8000-000000000001', '[\"file:///tmp/claude-1000/p/5e55e55e-0000-4000-8000-000000000002/scratchpad\"]', '2026-10-08 00:07:00.123456789+00:00')"
python3 - "$CODEX_CONFIG_DIR" <<'PY'
import json,sys
from pathlib import Path
root=Path(sys.argv[1]); directory=root/'sessions/2026/10/08';directory.mkdir(parents=True)
def write(name, records):
    (directory/f'rollout-{name}.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in records))
def row(kind,payload,minute=0):
    return dict(type=kind,timestamp=f'2026-10-08T00:{minute:02}:00Z',payload=payload)
write('parent',[row('session_meta',dict(id='parent',timestamp='2026-10-08T00:00:00Z',source='cli')),
 row('response_item',dict(type='function_call',name='spawn_agent',call_id='spawn',arguments=json.dumps(dict(message='Item: firn#5\nFollows: prior\nCategory: tooling\nETA: 10 minutes',model='gpt-6.1-sol',reasoning_effort='medium')))),
 row('response_item',dict(type='function_call_output',call_id='spawn',output=json.dumps(dict(task_name='/root/finished'))))])
def worker(name,brief='',finished=False):
    rs=[row('session_meta',dict(id=name,timestamp='2026-10-08T00:00:00Z',source=dict(subagent=dict(thread_spawn=dict(parent_thread_id='parent',agent_path='/root/'+name))))),
        row('turn_context',dict(model='gpt-6.1-sol',effort='medium'))]
    if brief:rs.append(row('response_item',dict(type='message',role='assistant',phase='commentary',content=[dict(type='output_text',text=brief)])))
    rs.append(row('event_msg',dict(type='token_count',info=dict(total_token_usage=dict(output_tokens=321),last_token_usage=dict(input_tokens=900))),5))
    if finished:rs.append(row('response_item',dict(type='message',role='assistant',phase='final',content=[dict(type='output_text',text='Done: landed.')]),5))
    write(name,rs)
worker('finished',finished=True)
worker('running','Item: wisp#70\nCategory: native-check\nETA: 20 minutes',finished=True)
worker('untracked',finished=True)
worker('hinted',finished=True)
rs=[json.loads(l) for l in (directory/'rollout-untracked.jsonl').read_text().splitlines()]
commit='text(await tools.exec_command({cmd:"git commit -m \\"Derive spawn (firn#9)\\"",workdir:"/w"}))'
rs[2:2]=[row('response_item',dict(type='custom_tool_call',name='exec',call_id='u1',input=commit),1),
         row('response_item',dict(type='custom_tool_call_output',call_id='u1',output=[dict(type='input_text',text='{"exit_code":0,"output":"ok"}')]),2)]
write('untracked',rs)
farm='text(await tools.exec_command({cmd:"bun wisp farm balance \\"wren 120\\" --wait",workdir:"/w"}))'
rs=[row('session_meta',dict(id='repeater',timestamp='2026-10-08T00:00:00Z',source=dict(subagent=dict(thread_spawn=dict(parent_thread_id='parent',agent_path='/root/repeater'))))),
    row('turn_context',dict(model='gpt-6.1-sol',effort='high')),
    row('response_item',dict(type='message',role='assistant',phase='commentary',content=[dict(type='output_text',text='Item: smashcraft#250\nCategory: balance-tuning\nETA: 20 minutes')]))]
for n in (1,2,3):
    rs.append(row('response_item',dict(type='custom_tool_call',name='exec',call_id=f'c{n}',input=farm),n))
    rs.append(row('response_item',dict(type='custom_tool_call_output',call_id=f'c{n}',output=[dict(type='input_text',text='{"exit_code":0,"output":"pass"}')]),n))
rs.append(row('response_item',dict(type='message',role='assistant',phase='final',content=[dict(type='output_text',text='Done: measured.')]),5))
write('repeater',rs)
PY
subagents=$CLAUDE_CONFIG_DIR/projects/-home-tom/session/subagents
mkdir -p "$subagents"
printf '%s\n' '{"agentType":"worker"}' >"$subagents/agent-haiku1.meta.json"
printf '%s\n' \
  '{"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"session","message":{"content":"Item: smashcraft#255\nCategory: mechanical\nETA: 15 minutes"}}' \
  '{"type":"assistant","timestamp":"2026-10-08T00:04:00Z","requestId":"r1","message":{"model":"claude-haiku-5-5","usage":{"output_tokens":50,"input_tokens":700},"content":[{"type":"text","text":"Done: landed."}]}}' \
  >"$subagents/agent-haiku1.jsonl"
printf '%s\n' \
  '{"type":"user","timestamp":"2026-10-08T00:00:00Z","sessionId":"session","message":{"content":"Item: smashcraft#256\nCategory: mechanical"}}' \
  '{"type":"assistant","timestamp":"2026-10-08T00:01:00Z","requestId":"r1","message":{"model":"claude-haiku-5-5","usage":{"output_tokens":5},"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cd /w && git -c gc.auto=0 commit -qm fix"}}]}}' \
  '{"type":"user","timestamp":"2026-10-08T00:02:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}' \
  '{"type":"assistant","timestamp":"2026-10-08T00:03:00Z","requestId":"r2","message":{"model":"claude-haiku-5-5","usage":{"output_tokens":5},"content":[{"type":"text","text":"Done: committed."}]}}' \
  >"$subagents/agent-commit1.jsonl"
printf '%s\n' '{"agentType":"worker"}' >"$subagents/agent-commit1.meta.json"
sed -e 's/commit1/commit2/; s/commit -qm fix/commit -m \\"Unlanded change\\"/' "$subagents/agent-commit1.jsonl" >"$subagents/agent-commit2.jsonl"
cp "$subagents/agent-commit1.meta.json" "$subagents/agent-commit2.meta.json"
sed -e 's/commit1/commit3/; s/commit -qm fix/commit -q -F - <<EOF\\nfix\\nEOF/' "$subagents/agent-commit1.jsonl" >"$subagents/agent-commit3.jsonl"
cp "$subagents/agent-commit1.meta.json" "$subagents/agent-commit3.meta.json"
export LEDGER_REPOS=$scratch/repo
git init -q "$LEDGER_REPOS" && git -C "$LEDGER_REPOS" -c user.name=t -c user.email=t@t -c commit.gpgsign=false commit -q --allow-empty -m fix &&
  git -C "$LEDGER_REPOS" update-ref refs/remotes/origin/main HEAD
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null
python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
e=dict(c.execute("select agent,expects_landing from runs").fetchall())
assert (e['commit1'],e['commit2'],e['haiku1'],e['repeater'])==(1,1,0,0),e
print('PASS a run expects a landing exactly when its transcript ran a git commit or push')
l=dict(c.execute("select agent,landed from runs where agent like 'commit%'").fetchall())
assert l=={'commit1':1,'commit2':0,'commit3':1},l
print('PASS a commit whose subject is on origin/main counts as landed though the run never pushed')
r=c.execute("select agent,item,follows,tier,category,minutes,eta_min,tokens,peak_ctx,outcome from runs where agent not in ('haiku1','commit1','commit2','hinted') and tier like 'gpt%'").fetchall()
want=[('finished','firn#5','prior','gpt-6.1-sol medium','tooling',5,10,321,900,'done'),
      ('running','wisp#70',None,'gpt-6.1-sol medium','native-check',5,20,321,900,'done'),
      ('untracked','firn#9',None,'gpt-6.1-sol medium','unknown',5,None,321,900,'done'),
      ('repeater','smashcraft#250',None,'gpt-6.1-sol high','balance-tuning',5,20,0,0,'done')]
assert sorted(r)==sorted(want),(r,want)
print('PASS [spec #5; firn#11] Codex spawn brief records fields, model/effort, timing and tokens once; an unreadable brief derives its Item from the commit')
h=c.execute("select tier,category,minutes from runs where agent='haiku1'").fetchone()
assert h==('haiku','mechanical',4),h
print('PASS a Claude worker running a Haiku model is recorded as tier haiku')
rep=c.execute("select repeats,landed from runs where agent='repeater'").fetchone()
assert rep==(None,0),rep
print('PASS a Codex worker that reran a passing farm check twice and pushed nothing stores no repeats, no landing')
assert c.execute('select count(*) from claims').fetchone()[0]==0,'finished workers must release holds'
h=c.execute("select item,eta_min from runs where agent='hinted'").fetchone()
assert h==('smashcraft#446',20),h
assert c.execute("select category from runs where agent='hinted'").fetchone()==('balance-tuning',)
print('PASS [firn#11] a Codex worker named in its lead status line takes that line Item and ETA')
cols="item,session,tier,category,minutes,tokens,peak_ctx,flaws,lens,staffed,review_requested"
g=c.execute(f"select {cols} from runs where agent='0a0a0a0a-0000-4000-8000-000000000001'").fetchone()
assert g==('wisp#71','5e55e55e-0000-4000-8000-000000000002','gemini-3.1-pro high','research',7,120,900,2,'break it',
           '2026-10-08T00:00:00Z','2026-10-08T00:07:00Z'),g
k=c.execute(f"select {cols} from runs where agent='commit1'").fetchone()
assert all(v is not None for v in k[2:6]+k[9:]),k
print('PASS [firn#11] an agy run is recorded with the same fields as a Claude run: brief fields, parent session, model and effort, timing, tokens, stages')
st=c.execute("select staffed,first_commit,review_requested,landed_at is not null,turns from runs where agent in ('commit1','untracked') order by agent").fetchall()
assert st==[('2026-10-08T00:00:00Z',None,'2026-10-08T00:03:00Z',1,None),
            ('2026-10-08T00:00:00Z',None,'2026-10-08T00:05:00Z',0,None)],st
print('PASS [firn#11] stage times come from the transcript and origin/main: staffed, first report, landed; first commit and turns are not stored')
PY

python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
assert c.execute('select count(*) from run_events').fetchone()[0]==0
assert c.execute("select count(*) from events where kind='run'").fetchone()[0]==0
print('PASS ledger ingestion stores no per-turn events and no run events')
PY

out=$(PATH="$scratch/bin:$PATH" "$repo/bin/worker-ledger" --summary)
[[ "$out" =~ tooling[[:space:]]+gpt-6.1-sol[[:space:]]+medium[[:space:]]+1[[:space:]]+1 ]]
[[ "$out" =~ native-check[[:space:]]+gpt-6.1-sol[[:space:]]+medium[[:space:]]+1[[:space:]]+1 ]]
[[ "$out" =~ tokens_med[[:space:]]+no_landing ]]
[[ "$out" =~ balance-tuning[[:space:]]+gpt-6.1-sol[[:space:]]+high[[:space:]].*[[:space:]]0k[[:space:]]+0/0 ]]
printf '%s\n' 'PASS [spec #5] encrypted brief repeated in commentary counts once by category and model/effort in summary'

export THREADS_DB=$scratch/context.db CLAUDE_CONFIG_DIR=$scratch/context-claude CODEX_CONFIG_DIR=$scratch/context-codex
export AGY_CONFIG_DIR=$scratch/context-agy AGENTS_ORG_FILE=$scratch/context-org.json
python3 - "$repo/bin" "$CLAUDE_CONFIG_DIR" "$CODEX_CONFIG_DIR" <<'PY'
import json,sys
from pathlib import Path
fixtures,claude,codex=map(Path,sys.argv[1:])
project=claude/'projects/recorded';workers=project/'recorded-context/subagents';workers.mkdir(parents=True)
rows=[json.loads(l) for l in (fixtures/'worker-ledger.context.jsonl').open()]
(workers/'agent-context.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
(workers/'agent-context.meta.json').write_text('{"agentType":"worker"}')
for role in ('lead','sub-lead','proxy'):
    altered=json.loads(json.dumps(rows))
    for r in altered:r['sessionId']='recorded-'+role
    altered[0]['message']['content']=f'Delegation: role={role} budget=0\nCategory: tooling\nItem: firn#19\nFollows: context\n[role evidence seeded for replay]'
    (project/f'recorded-{role}.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in altered))
directory=codex/'sessions/2026/10/10';directory.mkdir(parents=True)
tokens=[json.loads(l) for l in (fixtures/'worker-ledger.codex-context.jsonl').open()]
timestamp=tokens[0]['timestamp']
def record(kind,payload):return dict(type=kind,timestamp=timestamp,payload=payload)
rollout=[record('session_meta',dict(id='recorded-codex',timestamp=timestamp,source=dict(subagent=dict(thread_spawn=dict(parent_thread_id='recorded-parent',agent_path='/root/recorded-codex'))))),
         record('turn_context',dict(model='gpt-6.1-sol',effort='high')),
         record('response_item',dict(type='message',role='user',content=[dict(type='input_text',text='Delegation: role=worker budget=0\nCategory: tooling\n[recorded prompt omitted]')])),
         *tokens,record('response_item',dict(type='message',role='assistant',phase='final',content=[dict(type='output_text',text='[recorded response omitted]')]))]
(directory/'rollout-recorded.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rollout))
PY
"$repo/bin/worker-ledger" --backfill --since 2026-10-03 >/dev/null
python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
row=c.execute("select compactions,handoff_fires,idle_gap_misses,peak_ctx from runs where agent='context'").fetchone()
assert row==(1,1,1,664423),('first divergence: recorded usage/compaction with seeded hook',row)
assert c.execute("select input_tokens,cache_read_tokens,peak_ctx from runs where agent='recorded-codex'").fetchone()==(18495,12160,18495)
assert dict(c.execute('select role,count(*) from runs group by role'))=={'worker':2,'lead':1,'sub-lead':1,'proxy':1}
assert c.execute('select count(*) from claims').fetchone()[0]==0
assert c.execute("select count(*) from runs where role!='worker' and item is not null").fetchone()[0]==0
print('PASS [firn#19] recorded usage/compaction with explicitly seeded hook yields 1/1/1; streamed duplicates count once')
print('PASS [firn#19] recorded Codex token_count includes cached input, duplicate events count once')
PY
out=$("$repo/bin/worker-ledger" --summary)
[[ "$out" =~ peak_ctx_p50.*peak_ctx_p90.*compactions/run.*handoff_fires.*cache_read_share.*idle_gap_misses ]]
[[ "$out" =~ tooling[[:space:]]+medium[[:space:]]+-[[:space:]]+1 ]]
[[ "$out" =~ sub-lead[[:space:]]+tooling ]]
printf '%s\n' 'PASS [firn#19] leadership appears in context cells and leaves worker success/ETA cells unchanged'
python3 - "$CLAUDE_CONFIG_DIR" <<'PY'
import json,sys
from pathlib import Path
p=Path(sys.argv[1])/'projects/recorded/recorded-context/subagents/agent-context.jsonl'
rows=[json.loads(l) for l in p.open()]
for r in rows:
    if r.get('fixtureOrigin')=='seeded worker-handoff emission':
        r['message']['content']='Quoted source: Your context is 400k tokens, past the 350k handoff point.'
p.write_text(''.join(json.dumps(r)+'\n' for r in rows))
PY
"$repo/bin/worker-ledger" --backfill --since 2026-10-03 >/dev/null
python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
observed=c.execute("select compactions,handoff_fires,idle_gap_misses from runs where agent='context'").fetchone()
assert c.execute("select count(*) from runs where role!='worker' and item is not null").fetchone()[0]==0
try:
    assert observed==(1,1,1),('first divergence: hook emission replaced by quoted source',observed)
except AssertionError:
    assert observed==(1,0,1),observed
    print('PASS [firn#19] injected missing hook trips the recorded scenario detector; quoted source counts zero')
else:
    raise AssertionError('injected fault escaped the detector')
PY
printf '%s\n' 17970 >"$LEDGER_HANDOFFS/.context.reminded"
printf '%s\n' 17970 >"$LEDGER_HANDOFFS/.recorded-codex.reminded"
"$repo/bin/worker-ledger" --backfill --since 2026-10-03 >/dev/null
python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
assert c.execute("select handoff_fires,handoff_ignored from runs where agent='context'").fetchone()==(1,None)
assert c.execute("select handoff_fires from runs where agent='recorded-codex'").fetchone()==(0,)
print('PASS recorded missing transcript hook counts the Claude marker as one fire; Codex ignores markers')
PY
mv "$LEDGER_HANDOFFS/.context.reminded" "$LEDGER_HANDOFFS/.context.leash"
"$repo/bin/worker-ledger" --backfill --since 2026-10-03 >/dev/null
python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
assert c.execute("select handoff_fires,handoff_ignored from runs where agent='context'").fetchone()==(1,None)
print('PASS recorded missing transcript hook also falls back to the Claude leash marker without inventing a turn position')
PY
python3 - "$repo/bin" "$CLAUDE_CONFIG_DIR" <<'PY'
import json,sys
from pathlib import Path
rows=[json.loads(l) for l in (Path(sys.argv[1])/'worker-ledger.context.jsonl').open()]
for r in rows:
    if r.get('fixtureOrigin')=='seeded worker-handoff emission':
        r.pop('message')
        r.update(type='attachment',attachment=dict(type='hook_additional_context',content=['Your context is 400k tokens, past the 350k handoff point. [seeded instruction omitted]']))
def assistant(identity,content):
    return dict(type='assistant',timestamp=rows[-1]['timestamp'],sessionId='recorded-context',
                requestId=identity,message=dict(id=identity,content=content))
rows += [assistant('second-turn',[dict(type='thinking',thinking='[omitted]')]),
         assistant('second-turn',[dict(type='text',text='[seeded second turn]')]),
         assistant('third-turn',[dict(type='text',text='HANDOFF /recorded/handoff.md')])]
p=Path(sys.argv[2])/'projects/recorded/recorded-context/subagents/agent-context.jsonl'
p.write_text(''.join(json.dumps(r)+'\n' for r in rows))
PY
"$repo/bin/worker-ledger" --backfill --since 2026-10-03 >/dev/null
python3 - "$THREADS_DB" "$CLAUDE_CONFIG_DIR" <<'PY'
import json,sqlite3,sys
from pathlib import Path
c=sqlite3.connect(sys.argv[1])
assert c.execute("select handoff_fires,handoff_ignored from runs where agent='context'").fetchone()==(1,0)
print('PASS sanitized native attachment fire followed by HANDOFF on third assistant turn is effective; streamed blocks count once')
p=Path(sys.argv[2])/'projects/recorded/recorded-context/subagents/agent-context.jsonl'
rows=[json.loads(l) for l in p.open()]
rows[-1]['message']['content'][0]['text']='[seeded third turn ignores the reminder]'
fourth=json.loads(json.dumps(rows[-1]))
fourth.update(requestId='fourth-turn')
fourth['message'].update(id='fourth-turn',content=[dict(type='text',text='HANDOFF /recorded/handoff.md')])
rows.append(fourth)
p.write_text(''.join(json.dumps(r)+'\n' for r in rows))
PY
"$repo/bin/worker-ledger" --backfill --since 2026-10-03 >/dev/null
python3 - "$THREADS_DB" <<'PY'
import sqlite3,sys
c=sqlite3.connect(sys.argv[1])
assert c.execute("select handoff_fires,handoff_ignored from runs where agent='context'").fetchone()==(1,1)
print('PASS injected ignored reminder trips native scenario even with HANDOFF on fourth assistant turn')
PY

python3 - "$repo/socrates/lib" "$scratch/costs.db" "$repo/bin/threads" <<'PYMEASURE'
import importlib.machinery,json,os,sqlite3,sys
sys.path.insert(0,sys.argv[1])
from ledger_context import account_id,allocate_costs,delivery_metrics,run_usage
catalog={"accounts":{"claude":{"machines":{"whiterabbit":"a","nexus":"b"}}}}
assert account_id("claude","nexus",catalog)=="claude-b"
assert account_id("claude","whiterabbit",catalog)=="claude-a"
claude=[dict(type="assistant",timestamp="2026-10-11T00:05:00Z",requestId="r",message=dict(model="claude-opus-4-6",effort="high",usage=dict(input_tokens=100,cache_creation_input_tokens=100,cache_read_input_tokens=100,output_tokens=100,speed="fast")))]
telemetry=run_usage(claude+claude,"claude","nexus",catalog)
assert telemetry["fast"]==1 and len(telemetry["usage_samples"])==1
assert telemetry["usage_samples"][0]["weight"]==730,telemetry
codex=[dict(type="session_meta",payload=dict(service_tier="priority")),dict(type="turn_context",payload=dict(model="gpt-6.1-sol",effort="medium")),dict(type="event_msg",timestamp="2026-10-11T00:05:00Z",payload=dict(type="token_count",info=dict(total_token_usage=dict(input_tokens=1000,cached_input_tokens=800,output_tokens=100))))]
ct=run_usage(codex+codex[2:],"codex","whiterabbit",catalog)
assert ct["fast"]==1 and len(ct["usage_samples"])==1 and ct["usage_samples"][0]["weight"]==780,ct
rows=[dict(ts=f"2026-10-11T00:{minute:02d}:00Z",provider="claude",account="b",kind="window",name="weekly_all",used_pct=used,resets_at="2026-10-15T00:00:00Z") for minute,used in ((0,10),(10,14),(20,20))]
samples=[dict(agent=agent,account="claude-b",ts=ts,weight=weight) for agent,ts,weight in (("a","2026-10-11T00:05:00Z",730),("b","2026-10-11T00:08:00Z",270),("b","2026-10-11T00:15:00Z",100))]
costs=allocate_costs(rows,samples,{"claude-b":50})
assert abs(sum(costs.values())-5)<1e-10 and costs=={"a":1.46,"b":3.54},costs
flat=[dict(rows[0],ts="2026-10-10T23:50:00Z"),*rows]
idle=samples+[dict(agent="c",account="claude-b",ts="2026-10-10T23:55:00Z",weight=500)]
spread=allocate_costs(flat,idle,{"claude-b":50})
assert "(untracked)" not in spread and abs(sum(spread.values())-5)<1e-10,spread
jitter=[row|{"resets_at":"2026-10-15T00:00:01Z"} if row["used_pct"]==14 else row for row in rows]
assert allocate_costs(jitter,samples,{"claude-b":50})==costs
fault=allocate_costs(rows,[s|{"account":"claude-a"} for s in samples],{"claude-b":50})
assert sum(fault.values())!=5
priced=catalog|{"models":{"claude:opus-medium":{"model":"claude-opus-5-5","value":{"api":{"input_per_million":4.0,"output_per_million":20.0}}},"claude:haiku":{"model":"claude-haiku-5-5","value":{"api":{"input_per_million":0.1,"output_per_million":0.5}}}}}
turn=lambda m,t:[dict(type="assistant",timestamp=t,requestId=m,message=dict(model=m,usage=dict(input_tokens=1000,cache_creation_input_tokens=0,cache_read_input_tokens=1000,output_tokens=100)))]
mixed=[s|dict(agent=m,account="claude-b") for m,t in (("claude-opus-5-5","2026-10-11T00:03:00Z"),("claude-haiku-5-5","2026-10-11T00:04:00Z"),("claude-sonnet-9","2026-10-11T00:13:00Z")) for s in run_usage(turn(m,t),"claude","nexus",priced)["usage_samples"]]
share=allocate_costs(rows[:2],mixed[:2],{"claude-b":50})
assert abs(share["claude-opus-5-5"]/share["claude-haiku-5-5"]-40)<1e-9 and abs(sum(share.values())-2)<1e-10,share
assert run_usage(turn("claude-sonnet-9","2026-10-11T00:13:00Z"),"claude","nexus",priced)["unpriced_models"]==["claude-sonnet-9"] and mixed[2]["weight"]==1600
print(f"PASS [north#35] Opus and Haiku sharing one interval split plan $ 40:1 by list price (${share['claude-opus-5-5']:.4f} vs ${share['claude-haiku-5-5']:.4f}); an uncatalogued model takes token weight and is flagged")
os.environ["THREADS_DB"]=sys.argv[2]
threads=importlib.machinery.SourceFileLoader("ledger_threads_measure",sys.argv[3]).load_module()
threads.DB=sys.argv[2]
db=threads.connect()
import io
sys.stdin=io.StringIO("".join(json.dumps(row|dict(type="usage"))+"\n" for row in rows))
threads.cmd_ingest(db,None)
assert db.execute("SELECT COUNT(*) FROM usage").fetchone()[0]==3
records=[]
for agent in ("a","b"):
 records.append(dict(id=agent,account="claude-b",model="claude-opus-4-6",effort="high",fast=int(agent=="a"),usage_samples=[{k:v for k,v in sample.items() if k not in ("agent","account")} | {"request":str(n)} for n,sample in enumerate(samples) if sample["agent"]==agent],tier="high",actual_min=20,started="2026-10-11T00:00:00Z",ended="2026-10-11T00:20:00Z",outcome="done",landed=1,staffed="2026-10-11T00:00:00Z",landed_at="2026-10-11T00:20:00Z",brief="Item: north#2" if agent=="a" else "Item: wisp#3"))
sys.stdin=io.StringIO("".join(json.dumps(r)+"\n" for r in records))
running=records[1]|dict(status="running")
sys.stdin=io.StringIO(json.dumps(records[0])+"\n"+json.dumps(running)+"\n")
threads.cmd_ingest(db,None)
assert db.execute("SELECT COUNT(*) FROM runs").fetchone()[0]==1
assert abs(db.execute("SELECT SUM(cost_usd) FROM run_costs").fetchone()[0]-5)<1e-10
sys.stdin=io.StringIO(json.dumps(records[1])+"\n")
threads.cmd_ingest(db,None)
assert abs(db.execute("SELECT SUM(cost_usd) FROM runs").fetchone()[0]-5)<1e-10
assert db.execute("SELECT fast,COUNT(*) FROM runs GROUP BY fast").fetchall()[0][0]==0
metrics=delivery_metrics(db.execute("SELECT * FROM runs"),"north")
assert metrics==dict(done_boxes=1,concurrency=1,median_brief_to_landed_min=20,median_cost_per_done_usd=1.46),metrics
retried=[dict(dict(r),agent="a0",outcome="not done",landed=0,cost_usd=0.54,staffed="2026-10-10T23:40:00Z",started="2026-10-10T23:40:00Z",ended="2026-10-10T23:55:00Z",landed_at=None) for r in db.execute("SELECT * FROM runs WHERE agent='a'")]
assert delivery_metrics([*map(dict,db.execute("SELECT * FROM runs")),*retried],"north")["median_cost_per_done_usd"]==2.0
sys.stdin=io.StringIO("".join(json.dumps(r)+"\n" for r in records))
threads.cmd_ingest(db,None)
assert db.execute("SELECT COUNT(*) FROM run_usage").fetchone()[0]==3
prior=dict(db.execute("SELECT * FROM runs WHERE agent='a'").fetchone())
sys.stdin=io.StringIO(json.dumps(records[0]|dict(telemetry_only=True, role=None, actual_min=999, outcome="unclear", fast=0))+"\n")
threads.cmd_ingest(db,None)
after=dict(db.execute("SELECT * FROM runs WHERE agent='a'").fetchone())
assert after["fast"]==0 and all(after[k]==prior[k] for k in ("role","outcome","minutes","landed_at"))
print("PASS measured interval cost $5.00: all allocated to tracked runs, a zero-cost interval does not shrink later shares; a box's $ includes its failed attempt; Nexus account claude-b; fast 0/1; replay idempotent; project delivery 20 min/$1.46")
PYMEASURE
cal() { sqlite3 "$THREADS_DB" "select count(distinct at) from calibration where kind $1 'prior'"; }
monday=$(python3 -c 'from datetime import datetime,timedelta,timezone as z;n=datetime.now(z.utc);print((n-timedelta(days=n.weekday())).strftime("%Y-%m-%dT00:00:00Z"))')
sqlite3 "$THREADS_DB" "update calibration set at='2000-01-01T00:00:00Z' where kind!='prior'"
before=$(cal '!=')
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '!=')" -eq $((before + 1)) ] && [ "$(sqlite3 "$THREADS_DB" "select max(at) > datetime('now','-5 minutes') from calibration where kind!='prior'")" = 1 ]
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '!=')" -eq $((before + 1)) ]
sqlite3 "$THREADS_DB" "update calibration set at='2000-01-01T00:00:00Z' where kind!='prior'"
mkdir -p "$HOME/.config/threads" && printf 'nexus\n' >"$HOME/.config/threads/remote"
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '!=')" -eq "$before" ]
rm "$HOME/.config/threads/remote"
printf '%s\n' 'PASS [north#35] stale calibration rewritten on the ledger host, fresh skipped, remote-configured host skipped'
priors=$(python3 -c 'import sys,tomllib;print(sum(str(e.get("source","")).startswith("prior: ") for e in tomllib.load(open(sys.argv[1],"rb")).values()))' "$repo/socrates/defaults.toml")
sqlite3 "$THREADS_DB" "create table if not exists usage(ts, provider, kind, name, used_pct, resets_at);
  create table if not exists model_events(model, kind, at)"
before=$(cal '=')
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '=')" -eq $((before + 1)) ]
[ "$(sqlite3 "$THREADS_DB" "select count(*) from calibration where kind='prior' and at=(select max(at) from calibration where kind='prior') and value > 0")" -eq "$priors" ]
sqlite3 "$THREADS_DB" "update calibration set at='$monday' where kind='prior'"
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '=')" -eq $((before + 1)) ]
sqlite3 "$THREADS_DB" "update calibration set at=datetime('$monday','-1 minute') where kind='prior'"
CODEX_CONFIG_DIR=$scratch/no-codex "$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '=')" -eq $((before + 1)) ]
"$repo/bin/worker-ledger" --since 2026-10-08 >/dev/null 2>&1
[ "$(cal '=')" -eq $((before + 2)) ]
printf '%s\n' "PASS [north#35] $priors prior rows on the first run from Monday 00:00Z, none again that week, none without Codex rollouts"
