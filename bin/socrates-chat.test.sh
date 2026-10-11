#!/usr/bin/env bash
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/socrates-chat-test.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
export AGENTS_ORG_FILE="$scratch/org.json"
export AGENTS_ORCHESTRATION="$scratch/orchestration.toml"
printf '[projects]\n' >"$AGENTS_ORCHESTRATION"
org() { "$repo/bin/agents" org "$@" >/dev/null; }
replay() { local live="${3:-}"; [[ -n "$live" ]] || live='{}'; jq -n --slurpfile org "$1" --argjson rooms "$2" --argjson live "$live" '{org:$org[0],rooms:$rooms,live:$live}' | bun --no-install "$repo/bin/socrates-chat" replay; }
check() { if [[ "$1" != "$2" ]]; then printf 'FAIL %s\ngot: %s\nwant: %s\n' "$3" "$1" "$2" >&2; exit 1; fi; printf 'PASS %s\n' "$3"; }

org add chattest-room --role lead --domain chattest
org add chattest-child --role sub-lead --domain chattest --parent chattest-room
org add chattest-worker --role worker --domain chattest --parent chattest-child
cp "$AGENTS_ORG_FILE" "$scratch/new.json"
active='[{"node":"chattest-room","host":"fixture","room_id":"!room:example.test","state":"active","tom":"join"},{"node":"chattest-child","host":"fixture","room_id":"!child:example.test","state":"active","tom":"join"}]'
check "$(replay "$scratch/new.json" '[]' | jq -c '[.[]|[.kind,.node.id]]')" '[["open","chattest-room"],["open","chattest-child"]]' 'new leads open; worker has no room'
check "$(replay "$scratch/new.json" "$active")" '[]' 'unchanged snapshot is idempotent'

org finish chattest-room
cp "$AGENTS_ORG_FILE" "$scratch/finished.json"
check "$(replay "$scratch/finished.json" "$active" | jq -c '[.[]|[.kind,.node]]')" '[["close","chattest-room"]]' 'finished room closes'
closed="$(jq -c 'map(if .node=="chattest-room" then .state="finished"|.tom="leave" else . end)' <<<"$active")"
check "$(replay "$scratch/finished.json" "$closed")" '[]' 'finished snapshot is idempotent'

org remove chattest-room
cp "$AGENTS_ORG_FILE" "$scratch/gone.json"
check "$(replay "$scratch/gone.json" "$active" | jq -c '[.[]|[.kind,.node]]')" '[["close","chattest-room"],["close","chattest-child"]]' 'gone nodes close'

org add chattest-room --role lead --domain chattest
org exit chattest-room
cp "$AGENTS_ORG_FILE" "$scratch/died.json"
one="$(jq -c '.[0:1]' <<<"$active")"
check "$(replay "$scratch/died.json" "$one" | jq -c '[.[]|[.kind,.node]]')" '[["died","chattest-room"]]' 'dead session emits one notice'
dead="$(jq -c 'map(.state="died")' <<<"$one")"
check "$(replay "$scratch/died.json" "$dead")" '[]' 'death notice is not repeated'
check "$(replay "$scratch/died.json" "$(jq -c 'map(.tom="leave")' <<<"$dead")" | jq -c '[.[].kind]')" '["open"]' 'pending force-join does not repeat the death notice'

org set chattest-room --state active
cp "$AGENTS_ORG_FILE" "$scratch/returning.json"
check "$(replay "$scratch/returning.json" "$dead" | jq -c '[.[]|[.kind,.node]]')" '[["revived","chattest-room"]]' 'returning session retains its room'
check "$(replay "$scratch/returning.json" "$(jq -c '.[0:1]' <<<"$closed")" | jq -c '[.[]|[.kind,.node.id]]')" '[["open","chattest-room"]]' 'returning name reopens its finished room'
check "$(replay "$scratch/returning.json" "$one" '{"chattest-room":false}' | jq -c '[.[].kind]')" '["died"]' 'pid liveness is supplied to the pure core'

export AGENTS_ORG_FILE="$scratch/domains-org.json"
org add smashcraft-lead --role lead --domain smashcraft
org add galileo-sub-lead --role sub-lead --domain galileo --parent smashcraft-lead
org add chattest-lead --role lead --domain chattest
bun --no-install -e 'const {bootstrapNodes}=await import(process.argv[1]); console.log(JSON.stringify(bootstrapNodes));' "$repo/socrates/lib/chat-core.ts" >"$scratch/bootstrap.json"
jq -n --slurpfile org "$AGENTS_ORG_FILE" --slurpfile bootstrap "$scratch/bootstrap.json" '($org[0]|.nodes+=$bootstrap[0]) as $snapshot|{org:$snapshot,server:"example.test",
  spaces:{socrates:"!root:example.test",smashcraft:"!smashcraft:example.test",galileo:"!galileo:example.test",wisp:"!wisp:example.test",nexus:"!nexus:example.test",north:"!north:example.test",tooling:"!tooling:example.test",mail:"!mail:example.test"},
  rooms:[$snapshot.nodes[]|{node:.id,host:"fixture",room_id:("!node-"+.id+":example.test"),state:"active",tom:"join"}],
  children:[$snapshot.nodes[]|{parent:"socrates",room_id:("!node-"+.id+":example.test"),content:{via:["example.test"],suggested:true}}],
  parents:[$snapshot.nodes[]|{parent:"!root:example.test",room_id:("!node-"+.id+":example.test"),content:{canonical:true,via:["example.test"]}}]}' >"$scratch/layout.json"
bun --no-install "$repo/bin/socrates-chat" replay "$scratch/layout.json" >"$scratch/layout-actions.json"
check "$(jq -c '[.[]|select(.kind=="space" and (.room_id|startswith("!node-")))|[.parent,.room_id,.content.order//null]]' "$scratch/layout-actions.json")" \
  '[["socrates","!node-smashcraft-lead:example.test",null],["socrates","!node-galileo-sub-lead:example.test",null],["smashcraft","!node-smashcraft-lead:example.test","smashcraft-lead"],["galileo","!node-galileo-sub-lead:example.test","galileo-sub-lead"],["socrates","!node-chattest-lead:example.test","99-chattest-lead"],["socrates","!node-proxy:example.test","00-plato"],["socrates","!node-alerts:example.test","01-alerts"]]' 'domain rooms move; unknown domain stays in Socrates'
check "$(jq -c '[.[]|select(.kind=="parent" and .content=={})|[.room_id,.parent]]' "$scratch/layout-actions.json")" \
  '[["!node-smashcraft-lead:example.test","!root:example.test"],["!node-galileo-sub-lead:example.test","!root:example.test"]]' 'moving rooms clear their old parent state'
jq --slurpfile actions "$scratch/layout-actions.json" '
  reduce $actions[0][] as $action (. ;
    if $action.kind=="space" then .children=(.children|map(select(.parent!=$action.parent or .room_id!=$action.room_id)))+[$action|del(.kind)]
    elif $action.kind=="parent" then .parents=(.parents|map(select(.parent!=$action.parent or .room_id!=$action.room_id)))+[$action|del(.kind)]
    else . end)' "$scratch/layout.json" >"$scratch/layout-settled.json"
check "$(jq -c '[.parents[]|select(.content.via)|{key:.room_id,value:.parent}]|from_entries' "$scratch/layout-settled.json")" \
  '{"!node-chattest-lead:example.test":"!root:example.test","!node-proxy:example.test":"!root:example.test","!node-alerts:example.test":"!root:example.test","!smashcraft:example.test":"!root:example.test","!galileo:example.test":"!root:example.test","!wisp:example.test":"!root:example.test","!nexus:example.test":"!root:example.test","!north:example.test":"!root:example.test","!tooling:example.test":"!root:example.test","!mail:example.test":"!root:example.test","!node-smashcraft-lead:example.test":"!smashcraft:example.test","!node-galileo-sub-lead:example.test":"!galileo:example.test"}' 'each sub-space and room has its canonical parent'
check "$(jq '[.parents[]|select(.content.via)]|all(.content=={canonical:true,via:["example.test"]})' "$scratch/layout-settled.json")" 'true' 'parent content follows the Matrix space relationship'
check "$(bun --no-install "$repo/bin/socrates-chat" replay "$scratch/layout-settled.json")" '[]' 'unchanged domain snapshot has no actions'

cat >"$scratch/inbound.json" <<'JSON'
{"nodes": [{"id": "chattest-lead", "role": "lead", "domain": "chattest", "parent": "proxy", "provider": "claude", "session": "fixture", "state": "active"}], "rooms": [{"node": "chattest-lead", "host": "fixture", "room_id": "!room:example.test", "state": "active", "tom": "join"}], "tom": "@tom:example.test", "live": {"chattest-lead": true}, "inbound": [{"room_id": "!room:example.test", "event": {"event_id": "$agent", "type": "m.room.message", "sender": "@agent-probe:example.test", "origin_server_ts": 1791717455730, "content": {"body": "unauthorized inbound probe", "msgtype": "m.text"}}}, {"room_id": "!room:example.test", "event": {"event_id": "$socrates", "type": "m.room.message", "sender": "@socrates:example.test", "origin_server_ts": 1791717455812, "content": {"body": "unauthorized inbound probe", "msgtype": "m.text"}}}, {"room_id": "!room:example.test", "event": {"event_id": "$other", "type": "m.room.message", "sender": "@other:example.test", "origin_server_ts": 1791717410597, "content": {"body": "safe\u001b[201~ words\u001b clean", "msgtype": "m.text"}}}, {"room_id": "!room:example.test", "event": {"event_id": "$controls", "type": "m.room.message", "sender": "@tom:example.test", "origin_server_ts": 1791717481322, "content": {"body": "safe\u001b[201~ words\u001b clean", "msgtype": "m.text"}}}]}
JSON
bun --no-install "$repo/bin/socrates-chat" replay "$scratch/inbound.json" >"$scratch/inbound-actions.json"
check "$(jq -c 'map(.kind)' "$scratch/inbound-actions.json")" '["drop","drop","drop","deliver"]' 'recorded agent and bot events plus seeded third party never deliver; exact Tom does'
check "$(jq -r '.[3].text' "$scratch/inbound-actions.json")" 'Tom (chat): safe words clean' 'Tom bracketed-paste terminator and raw ESC are removed'
jq '.live={}' "$scratch/inbound.json" >"$scratch/queued.json"
check "$(bun --no-install "$repo/bin/socrates-chat" replay "$scratch/queued.json" | jq -c '.[3]|[.kind,.wake.node]')" '["queue","proxy"]' 'dead recorded target queues and wakes through Plato notice'
jq '.dialogs={"chattest-lead":true}' "$scratch/inbound.json" >"$scratch/dialog.json"
check "$(bun --no-install "$repo/bin/socrates-chat" replay "$scratch/dialog.json" | jq -c '.[3]|[.kind,.wake]')" '["queue",null]' 'dialog blocks delivery without waking a live target'
bun --no-install - "$repo/socrates/lib/chat-core.ts" <<'TS'
const {chatText,routeInbound,dialogOpen,inputReady}=await import(process.argv[2]);
const seed=JSON.parse(require("fs").readFileSync(process.env.AGENTS_ORG_FILE.replace("domains-org.json","inbound.json"),"utf8"));
for(let control=0;control<=0x9f;control++) {
  if(control>0x1f && control<0x7f) continue;
  const text=chatText("first"+String.fromCharCode(control)+"last");
  if(/[\x00-\x1f\x7f-\x9f]/.test(text)||!text.startsWith("first")||!text.endsWith("last")) throw Error("control escaped sanitizer");
}
const event={...seed.inbound[3].event,content:{msgtype:"m.text",body:"/exit\nsecond\nthird"}};
const action=routeInbound(event,"!room:example.test",seed.nodes,seed.rooms,seed.tom,seed.live,{},false,"reply\x1b[201~\nsecond");
if(action.kind!=="deliver"||action.text!=="Tom (chat): Re: reply — /exit\u2028second\u2028third") throw Error("reply or multiline routing failed");
const near=routeInbound({...event,sender:seed.tom+".evil"},"!room:example.test",seed.nodes,seed.rooms,seed.tom,seed.live,{},false);
if(near.kind!=="drop") throw Error("sender match was not exact");
if(!dialogOpen("❯ 1. Yes\n  2. No\nEnter to select · Esc to cancel")||dialogOpen("❯ Tom (chat): ping")) throw Error("dialog gate failed");
if(inputReady("loading")||!inputReady("❯ \n────\nstatus")) throw Error("startup gate failed");
console.log("PASS seeded control property, reply, multiline, slash, sender exactness and dialog gate");
TS
bun --no-install - "$repo/socrates/lib/chat-core.ts" <<'TS'
const {decideWatches,formatOutbound,parseWatch,routeInbound}=await import(process.argv[2]);
const tom="@tom:example.test", room="!room:example.test", start=1791717455730;
const nodes=[{id:"north-lead",role:"lead",domain:"north",parent:"proxy",provider:"codex",session:"fixture",state:"active"}];
const rooms=[{node:"north-lead",host:"fixture",room_id:room,state:"active",tom:"join"}];
const event=(id,sender,body,time=start)=>({event_id:id,type:"m.room.message",sender,origin_server_ts:time,content:{msgtype:"m.text",body}});
const timeline=(e,node="north-lead",room_id=room)=>({room_id,node,event:e});
let state={watches:[],etas:[]};
const step=(events,now,issues={})=>{const result=decideWatches(state.watches,state.etas,events,issues,now,tom);state=result;return result.posts;};
const expect=(ok,rule)=>{if(!ok)throw Error("B9 watch spec: "+rule);};
const ping=(post,signal)=>post.content.msgtype==="m.text"&&post.content.body.startsWith(tom+" "+signal+":")&&JSON.stringify(post.content["m.mentions"])===JSON.stringify({user_ids:[tom]});
const command=event("$watch",tom,"WaTcH this done if over 2MIN");
const ack=step([timeline(command)],start);
expect(ack.length===1&&ack[0].content.msgtype==="m.notice"&&ack[0].content.body.includes(new Date(start+120000).toISOString()),"one silent acknowledgement with the deadline");
expect(routeInbound(command,room,nodes,rooms,tom,{"north-lead":true},{},false).kind==="watch","watch never enters the pane");
expect(step([],start+119999).length===0,"deadline has not passed");
const overdue=step([],start+120000);
expect(overdue.length===1&&ping(overdue[0],"overdue"),"2-minute deadline pings once");
expect(step([],start+180000).length===0,"later ticks never repeat overdue");
const blocked=timeline(event("$blocked","@agent-north-lead:example.test","Blocked: waiting",start+180001));
expect(step([blocked],start+180001).filter(post=>ping(post,"blocked")).length===1,"blocked pings once");
expect(step([blocked],start+180002).length===0,"replayed blocked never repeats");
const done=timeline(event("$done","@agent-north-lead:example.test","Done: shipped",start+180003));
expect(step([done],start+180003).filter(post=>ping(post,"done")).length===1&&state.watches[0].ended,"done pings once and ends the watch");
expect(step([done,blocked],start+240000).length===0,"ended watch never pings again");
const entry={v:1,node:"north-lead",kind:"reply",tag:null,text:"Normal post",file:null,source:"cli",session:null,ts:new Date(start).toISOString()};
const quiet=formatOutbound(entry,tom);
expect(quiet.msgtype==="m.notice"&&!("m.mentions"in quiet),"unwatched agent post stays quiet");
const needs=formatOutbound({...entry,text:"Status\nNeeds Tom: decide"},tom);
expect(needs.msgtype==="m.text"&&needs.body.startsWith(tom)&&JSON.stringify(needs["m.mentions"])===JSON.stringify({user_ids:[tom]}),"Needs Tom pings");
expect(formatOutbound({...entry,node:"proxy",kind:"notice"},tom).msgtype==="m.text","Plato stays text");
const attachment=formatOutbound({...entry,text:"Needs Tom: decide\n"+"x".repeat(4001)},tom);
expect(attachment.msgtype==="m.file"&&attachment.ping.body.startsWith(tom)&&attachment.ping["m.mentions"].user_ids[0]===tom,"attachment stays file with a separate Needs Tom ping");
const stranger=event("$stranger","@agent-probe:example.test","watch this if over 2m",start+240001);
expect(routeInbound(stranger,room,nodes,rooms,tom,{"north-lead":true},{},false).kind==="drop"&&step([timeline(stranger)],start+240001).length===0,"non-Tom watch is ignored");
step([timeline(event("$eta","@agent-other-host:example.test","ETA: 3 minutes B9\nT7 mentioned elsewhere",start+240002),"other-host","!other:example.test")],start+240002);
expect(state.etas.find(eta=>eta.target==="other-host").deadline===start+420002&&state.etas.find(eta=>eta.target==="b9").deadline===start+420002&&!state.etas.some(eta=>eta.target==="t7"),"timeline ETA tracks its node and boxes on its own line");
const etaAck=step([timeline(event("$box",tom,"watch B9",start+240003))],start+240003);
expect(etaAck.length===1&&state.watches.find(watch=>watch.target==="b9").deadline===start+420002,"watch defaults to latest ETA");
step([timeline(event("$eta-new","@agent-other-host:example.test","ETA 4m B9",start+240004),"other-host","!other:example.test")],start+240004);
expect(state.etas.find(eta=>eta.target==="b9").deadline===start+480004,"later ETA replaces earlier ETA");
expect(state.watches.find(watch=>watch.target==="b9").deadline===start+480004,"a later ETA moves an unstated watch deadline");
expect(step([timeline(event("$other-done","@agent-other-host:example.test","Done: T5 landed\nB9 still running",start+240005),"other-host","!other:example.test")],start+240005).length===0,"a Done line about another box leaves the watch open");
const boxDone=step([timeline(event("$box-done","@agent-other-host:example.test","Done: B9 landed",start+240005),"other-host","!other:example.test")],start+240005);
expect(boxDone.length===1&&ping(boxDone[0],"done"),"remote box post completes the watch");
step([timeline(event("$issue",tom,"watch north#42 done",start+240006))],start+240006);
expect(step([],start+240007,{"north#42":"OPEN"}).length===0,"open issue remains watched");
expect(step([],start+240008,{"north#42":"CLOSED"}).filter(post=>ping(post,"done")).length===1,"closed issue completes the watch");
step([timeline(event("$again",tom,"watch this if over 1h",start+240009))],start+240009);
const unwatch=step([timeline(event("$unwatch",tom,"unwatch this",start+240010))],start+240010);
expect(unwatch.length===1&&unwatch[0].content.msgtype==="m.notice"&&!state.watches.some(watch=>watch.target==="north-lead")&&step([],start+4000000).length===0,"unwatch removes the watch with one silent acknowledgement");
expect(parseWatch("watch B9 if over 1d").duration===86400000&&parseWatch("watch north#42 if over 2m").duration===120000&&parseWatch("watch this if over nope").kind==="invalid","documented durations and malformed command handling");
console.log("PASS B9 watch spec recorded scenario: quiet posts, mentions, one-shot watches, timeline ETAs and Tom authority");
TS
