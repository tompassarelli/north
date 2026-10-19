#!/usr/bin/env bash
set -uo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/laptop-ntfy-test.XXXXXX")
pids=()
cleanup() {
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  rm -rf "${scratch:?}"
}
trap cleanup EXIT
export THREADS_DB=$scratch/threads.db LAPTOP_STATE=$scratch/placement/laptop.json LAPTOP_THREADS=true LAPTOP_NOTE_ITEM=x
export LAPTOP_WINDOW_FILE=$scratch/window.json LAPTOP_NTFY_STATE=$scratch/ntfy-state PATH=$repo/bin:$PATH
printf '{"tz": "Etc/GMT-8", "open": "23:00", "close": "10:00"}\n' >"$LAPTOP_WINDOW_FILE"
printf 'fixture-publisher\n' >"$scratch/publisher"
printf 'fixture-action\n' >"$scratch/action"
export LAPTOP_NTFY_TOKEN_FILE=$scratch/publisher LAPTOP_NTFY_ACTION_TOKEN_FILE=$scratch/action
export LAPTOP_NTFY_TOPIC=socrates-laptop LAPTOP_NTFY_ACTION_URL=http://10.77.0.1:2588

cat >"$scratch/ntfy.py" <<'PY'
import json, sys
from http.server import BaseHTTPRequestHandler, HTTPServer
class H(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        body["auth"] = self.headers.get("Authorization") == "Bearer fixture-publisher"
        open(sys.argv[2], "a").write(json.dumps(body) + "\n")
        self.send_response(200); self.end_headers(); self.wfile.write(b'{"id":"x"}')
    def log_message(self, *a): pass
HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
PY
free_port() { python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])'; }
ntfy_port=$(free_port)
serve_port=$(free_port)
python3 "$scratch/ntfy.py" "$ntfy_port" "$scratch/published.jsonl" &
pids+=($!)
export LAPTOP_NTFY_URL=http://127.0.0.1:$ntfy_port LAPTOP_NTFY_LISTEN=127.0.0.1:$serve_port
laptop-ntfy serve >"$scratch/serve.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do curl -s -o /dev/null "http://127.0.0.1:$serve_port/" && break; sleep 0.1; done

pass=0 fail=0 out=""
check() {
  if eval "$2"; then pass=$((pass + 1)); printf 'PASS  %s\n' "$1"
  else fail=$((fail + 1)); printf 'FAIL  %s\n%s\n' "$1" "$out"; fi
}
sent() { if [[ -f $scratch/published.jsonl ]]; then wc -l <"$scratch/published.jsonl"; else echo 0; fi; }
last_title() { tail -1 "$scratch/published.jsonl" | jq -r .title; }
state() { laptop status --json | jq -r .state; }
by() { laptop status --json | jq -r .by; }
press() { curl -s -o /dev/null -w '%{http_code}' -X POST -H "Authorization: Bearer ${2-fixture-action}" "http://127.0.0.1:$serve_port/laptop/$1"; }
notify_at() { LAPTOP_NOW=$1 laptop-ntfy notify >/dev/null 2>&1; }

LAPTOP_NOW=2026-10-13T08:00:00Z laptop open >/dev/null
notify_at 2026-10-13T08:00:30Z
check "a state change publishes nothing" '[[ $(sent) == 0 ]]'

out=$(press close)
check "Close button flips the state to draining attributed to ntfy" '[[ $out == 200 && $(state) == draining && $(by) == ntfy ]]'
check "a button press publishes nothing" 'sleep 0.3; [[ $(sent) == 0 ]]'
out=$(press open)
check "Open button flips the state back to available" '[[ $out == 200 && $(state) == available && $(by) == ntfy ]]'
check "bad token is refused and changes nothing" 'before=$(sent); r=$(press close wrong); [[ $r == 401 && $(state) == available && $(sent) == "$before" ]]'
check "unknown action and GET are refused" '[[ $(press toggle) == 404 && $(curl -s -o /dev/null -w "%{http_code}" "http://127.0.0.1:$serve_port/laptop/close") == 405 && $(state) == available ]]'
check "the token never reaches the endpoint log" '! grep -q fixture-action "$scratch/serve.log"'

LAPTOP_NOW=2026-10-13T08:00:00Z laptop close until 10:00 >/dev/null
notify_at 2026-10-13T14:30:00Z
check "outside the window edge nothing is published" '[[ $(sent) == 0 ]]'
notify_at 2026-10-13T15:05:00Z
check "the window open edge sends one reminder with the publisher token" '[[ $(sent) == 1 && $(last_title) == "Laptop window opens 23:00" ]] && jq -e ".auth and .topic==\"socrates-laptop\"" "$scratch/published.jsonl" >/dev/null'
check "the reminder carries Open and Close http buttons to the endpoint with the action token" 'jq -e "(.actions|map(.label)==[\"Open\",\"Close\"]) and (.actions|map(.url)==[\"http://10.77.0.1:2588/laptop/open\",\"http://10.77.0.1:2588/laptop/close\"]) and (.actions|all(.action==\"http\" and .method==\"POST\" and .headers.Authorization==\"Bearer fixture-action\"))" "$scratch/published.jsonl" >/dev/null'
notify_at 2026-10-13T15:20:00Z
check "the reminder is once a day" '[[ $(sent) == 1 ]]'
notify_at 2026-10-14T15:05:00Z
check "the next day's edge reminds again" '[[ $(sent) == 2 ]]'

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[[ $fail == 0 ]]
