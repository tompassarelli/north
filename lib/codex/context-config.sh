#!/usr/bin/env bash
CONTEXT_CONFIG=()
if [ -r /etc/north/agent-context.json ]; then
  context_config="$("${NORTH_AGENT_PYTHON:-python3}" -c '
import json
with open("/etc/north/agent-context.json") as source:
    context = json.load(source)
for key, field in (("model_context_window", "window"), ("model_auto_compact_token_limit", "autoCompactTokenLimit")):
    value = context[field]
    if type(value) is not int or value <= 0:
        raise ValueError("invalid declared context limit")
    print(f"{key}={value}")
')" || exit 1
  while IFS= read -r context_override; do
    CONTEXT_CONFIG+=(-c "$context_override")
  done <<<"$context_config"
fi
