#!/usr/bin/env bash
set -euo pipefail

if [ "$#" -ne 3 ]; then
  echo "usage: $0 <codex-bin> <libredirect.so> <python3>" >&2
  exit 64
fi

CODEX_BIN="$1"
LIBREDIRECT="$2"
PYTHON="$3"

fail() {
  printf 'codex-managed-hook-failure-smoke: %s\n' "$*" >&2
  exit 1
}

[ -x "$CODEX_BIN" ] || fail "Codex executable is not executable: $CODEX_BIN"
[ -r "$LIBREDIRECT" ] || fail "libredirect is not readable: $LIBREDIRECT"
[ -x "$PYTHON" ] || fail "Python executable is not executable: $PYTHON"

EXPECTED_VERSION="0.144.4"
actual_version="$("$CODEX_BIN" --version)"
[ "$actual_version" = "codex-cli $EXPECTED_VERSION" ] || \
  fail "expected codex-cli $EXPECTED_VERSION, got $actual_version"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/north-codex-managed-hook-smoke.XXXXXX")"
SERVER_PID=""
cleanup() {
  if [ -n "$SERVER_PID" ]; then
    if kill -0 "$SERVER_PID" 2>/dev/null; then
      kill "$SERVER_PID" 2>/dev/null || true
    fi
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  rm -rf -- "${TMP:?}"
}
trap cleanup EXIT

HOME_DIR="$TMP/home"
CODEX_HOME_DIR="$HOME_DIR/.codex"
WORKSPACE="$TMP/workspace"
MANAGED_DIR="$TMP/managed-hooks"
HOOK="$MANAGED_DIR/failing-pre-tool-use.sh"
HOOK_INPUT="$TMP/hook-input.json"
HOOK_MARKER="$TMP/hook-ran"
SIDE_EFFECT_MARKER="$TMP/provider-command-side-effect"
REQUIREMENTS="$TMP/requirements.toml"
REQUEST_LOG="$TMP/provider-requests.jsonl"
PORT_FILE="$TMP/provider-port"
SERVER="$TMP/provider.py"
CODEX_STDOUT="$TMP/codex.stdout"
CODEX_STDERR="$TMP/codex.stderr"
SENTINEL="NORTH-CODEX-MANAGED-HOOK-SMOKE-SENTINEL"
SIDE_EFFECT_COMMAND="printf '%s' '$SENTINEL' > '$SIDE_EFFECT_MARKER'"

mkdir -p "$CODEX_HOME_DIR" "$WORKSPACE" "$MANAGED_DIR" \
  "$HOME_DIR/.cache" "$HOME_DIR/.config" "$HOME_DIR/.local/state"
: >"$REQUEST_LOG"

cat >"$HOOK" <<'HOOK'
#!/usr/bin/env bash
set -euo pipefail
cat >"${NORTH_SMOKE_HOOK_INPUT:?}"
printf 'ran\n' >"${NORTH_SMOKE_HOOK_MARKER:?}"
printf 'intentional managed hook failure\n' >&2
exit 17
HOOK
chmod 0755 "$HOOK"

cat >"$REQUIREMENTS" <<EOF
allow_managed_hooks_only = true
managed_hook_failure_mode = "block"

[features]
hooks = true

[hooks]
managed_dir = "$MANAGED_DIR"

[[hooks.PreToolUse]]
matcher = "^Bash$"

[[hooks.PreToolUse.hooks]]
type = "command"
command = "$BASH $HOOK"
timeout = 5
EOF

cat >"$SERVER" <<'PY'
import http.server
import json
import pathlib
import sys

port_path = pathlib.Path(sys.argv[1])
request_log = pathlib.Path(sys.argv[2])
side_effect_command = sys.argv[3]


def event_stream(events):
    chunks = []
    for event in events:
        kind = event["type"]
        chunks.append(f"event: {kind}\n")
        chunks.append("data: " + json.dumps(event, separators=(",", ":")) + "\n\n")
    return "".join(chunks).encode("utf-8")


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    requests_seen = 0

    def log_message(self, _format, *_args):
        return

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        raw = self.rfile.read(length)
        try:
            body = json.loads(raw)
        except json.JSONDecodeError:
            self.send_error(400, "request body was not JSON")
            return

        type(self).requests_seen += 1
        request_number = type(self).requests_seen
        record = {
            "path": self.path,
            "headers": {key.lower(): value for key, value in self.headers.items()},
            "body": body,
        }
        with request_log.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(record, separators=(",", ":")) + "\n")

        if self.path != "/v1/responses" or request_number > 2:
            self.send_error(500, "unexpected provider request")
            return

        if request_number == 1:
            arguments = json.dumps({"command": side_effect_command}, separators=(",", ":"))
            events = [
                {"type": "response.created", "response": {"id": "resp-1"}},
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "function_call",
                        "call_id": "managed-hook-smoke",
                        "name": "shell_command",
                        "arguments": arguments,
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "id": "resp-1",
                        "usage": {
                            "input_tokens": 0,
                            "input_tokens_details": None,
                            "output_tokens": 0,
                            "output_tokens_details": None,
                            "total_tokens": 0,
                        },
                    },
                },
            ]
        else:
            events = [
                {"type": "response.created", "response": {"id": "resp-2"}},
                {
                    "type": "response.output_item.done",
                    "item": {
                        "type": "message",
                        "role": "assistant",
                        "id": "msg-1",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "managed hook denial observed",
                            }
                        ],
                    },
                },
                {
                    "type": "response.completed",
                    "response": {
                        "id": "resp-2",
                        "usage": {
                            "input_tokens": 0,
                            "input_tokens_details": None,
                            "output_tokens": 0,
                            "output_tokens_details": None,
                            "total_tokens": 0,
                        },
                    },
                },
            ]

        payload = event_stream(events)
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(payload)))
        self.send_header("connection", "close")
        self.end_headers()
        self.wfile.write(payload)
        self.wfile.flush()
        self.close_connection = True


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
port_path.write_text(str(server.server_address[1]), encoding="utf-8")
server.serve_forever(poll_interval=0.05)
PY

"$PYTHON" "$SERVER" "$PORT_FILE" "$REQUEST_LOG" "$SIDE_EFFECT_COMMAND" &
SERVER_PID=$!
for ((attempt = 0; attempt < 100; attempt++)); do
  if [ -s "$PORT_FILE" ]; then
    break
  fi
  kill -0 "$SERVER_PID" 2>/dev/null || fail "local provider exited before publishing its port"
  sleep 0.05
done
[ -s "$PORT_FILE" ] || fail "local provider did not publish its port"
PORT="$(<"$PORT_FILE")"
[[ "$PORT" =~ ^[0-9]+$ ]] || fail "local provider published an invalid port"

provider_override="model_providers.smoke={ name = \"smoke\", base_url = \"http://127.0.0.1:$PORT/v1\", wire_api = \"responses\", requires_openai_auth = false, supports_websockets = false, request_max_retries = 0, stream_max_retries = 0, stream_idle_timeout_ms = 5000 }"

set +e
env -i \
  PATH="$PATH" \
  HOME="$HOME_DIR" \
  CODEX_HOME="$CODEX_HOME_DIR" \
  XDG_CACHE_HOME="$HOME_DIR/.cache" \
  XDG_CONFIG_HOME="$HOME_DIR/.config" \
  XDG_STATE_HOME="$HOME_DIR/.local/state" \
  NORTH_SMOKE_HOOK_INPUT="$HOOK_INPUT" \
  NORTH_SMOKE_HOOK_MARKER="$HOOK_MARKER" \
  LD_PRELOAD="$LIBREDIRECT" \
  NIX_REDIRECTS="/etc/codex/requirements.toml=$REQUIREMENTS" \
  NO_PROXY=127.0.0.1,localhost \
  timeout --foreground --signal=TERM --kill-after=2s 20s \
  "$CODEX_BIN" exec \
    --json \
    --color never \
    --skip-git-repo-check \
    --ephemeral \
    --dangerously-bypass-hook-trust \
    --dangerously-bypass-approvals-and-sandbox \
    -m gpt-5.5 \
    -c "$provider_override" \
    -c 'model_provider="smoke"' \
    -C "$WORKSPACE" \
    'Run the single shell command supplied by the provider.' \
    >"$CODEX_STDOUT" 2>"$CODEX_STDERR"
codex_status=$?
set -e

if [ "$codex_status" -ne 0 ]; then
  sed -n '1,160p' "$CODEX_STDERR" >&2
  fail "Codex exited $codex_status"
fi
[ -s "$HOOK_INPUT" ] || fail "managed PreToolUse hook did not receive input"
[ -s "$HOOK_MARKER" ] || fail "managed PreToolUse hook did not run"
[ ! -e "$SIDE_EFFECT_MARKER" ] || fail "guarded provider command reached its side effect"

for ((attempt = 0; attempt < 100; attempt++)); do
  if [ "$(wc -l <"$REQUEST_LOG")" -ge 2 ]; then
    break
  fi
  sleep 0.05
done

"$PYTHON" - "$REQUEST_LOG" "$HOOK_INPUT" "$CODEX_STDOUT" \
  "$SIDE_EFFECT_COMMAND" "$SENTINEL" <<'PY'
import json
import pathlib
import sys

request_log = pathlib.Path(sys.argv[1])
hook_input_path = pathlib.Path(sys.argv[2])
codex_stdout_path = pathlib.Path(sys.argv[3])
side_effect_command = sys.argv[4]
sentinel = sys.argv[5]
fixed_reason = (
    "managed PreToolUse hook failed; tool execution denied by administrator policy"
)

requests = [
    json.loads(line)
    for line in request_log.read_text(encoding="utf-8").splitlines()
    if line.strip()
]
assert len(requests) == 2, f"expected exactly two local provider requests, got {len(requests)}"
for request in requests:
    assert request["path"] == "/v1/responses", request["path"]
    assert "authorization" not in request["headers"], "smoke must not send an API credential"

items = requests[1]["body"].get("input", [])
outputs = [
    item
    for item in items
    if item.get("type") == "function_call_output"
    and item.get("call_id") == "managed-hook-smoke"
]
assert len(outputs) == 1, f"expected one exact function output, got {outputs!r}"
output_text = json.dumps(outputs[0].get("output"), sort_keys=True)
assert fixed_reason in output_text, output_text
assert sentinel not in output_text, output_text
assert side_effect_command not in output_text, output_text

hook_input = json.loads(hook_input_path.read_text(encoding="utf-8"))
assert hook_input.get("tool_name") == "Bash", hook_input
assert hook_input.get("tool_input", {}).get("command") == side_effect_command, hook_input

events = [
    json.loads(line)
    for line in codex_stdout_path.read_text(encoding="utf-8").splitlines()
    if line.strip()
]
assert any(event.get("type") == "turn.completed" for event in events), events
PY

printf 'codex-managed-hook-failure-smoke: PASS (%s)\n' "$actual_version"
