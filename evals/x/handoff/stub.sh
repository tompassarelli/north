#!/bin/sh
trial=/eval-x

field() {
  value=${2#*\"$1\"}
  value=${value#*:}
  value=${value# }
  value=${value%%,*}
  printf '%s' "${value%%\}*}"
}

publish() {
  printf '%s\n' "$2" > "$trial/.$1.tmp"
  mv "$trial/.$1.tmp" "$trial/$1"
}

reply() {
  printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$1" "$2"
}

while IFS= read -r line; do
  case $line in *'"id"'*) ;; *) continue ;; esac
  id=$(field id "$line")
  method=$(field method "$line")
  method=${method#\"}
  method=${method%\"}
  case $method in
    initialize)
      reply "$id" "{\"protocolVersion\":$(field protocolVersion "$line"),\"agentCapabilities\":{}}"
      ;;
    session/new)
      publish session.json "$line"
      reply "$id" '{"sessionId":"eval-x-handoff"}'
      ;;
    session/prompt)
      IFS= read -r host < /etc/hostname || :
      publish container "$host"
      publish prompt.json "$line"
      while [ ! -e "$trial/done.json" ]; do
        sleep 1
      done
      reply "$id" '{"stopReason":"end_turn"}'
      ;;
    *)
      reply "$id" '{}'
      ;;
  esac
done
