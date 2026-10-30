#!/usr/bin/env bash
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT
sha=$(printf 'b%.0s' {1..40})
body=$'CI on `main` is failing.\n\n- First failing commit: '"$sha"$' (https://github.com/o/r/actions/runs/77)\n- Culprit lane: claude/land-final-image-0123456789ab (bbbbbbbbbb)\n- Culprit lane: claude/land-other-fedcba987654 (cccccccccc)\n\n## Failing tests\n\n- cargo: render::gpu_tests::blend\n- cargo: render::gpu_tests::modulate\n'
issue=$(jq -nc --arg body "$body" '{number: 135, title: "main is red", url: "https://github.com/o/r/issues/135", body: $body}')
cat > "$work/gh" <<SH
#!/usr/bin/env bash
case "\$2" in
  list) printf '%s\n' '$(base64 -w0 <<< "$issue")' ;;
  view) echo 2026-10-30T23:05:28Z ;;
esac
SH
cat > "$work/north" <<SH
#!/usr/bin/env bash
if [ "\$2" = list ]; then echo '[{"name":"final-image","status":"idle","cwd":"/home/tom/code/wisp/worktrees/final-image-0"},{"name":"author","status":"idle","cwd":"/home/tom/code/wisp/worktrees/final-image"}]'; else printf '%s\n' "\$*" >> "$work/sent"; fi
SH
chmod +x "$work/gh" "$work/north"
run() { RED_MAIN_REPOS=o/r RED_MAIN_GH="$work/gh" RED_MAIN_NORTH="$work/north" RED_MAIN_STATE="$work/state" "$here/red-main-notify"; }
run
grep -q '^fleet send author main is red in o/r from bbbbbbbbbb: your lane final-image landed in the range that broke it (cargo: render::gpu_tests::blend;cargo: render::gpu_tests::modulate)' "$work/sent"
grep -q "^fleet send proxy (sent to author) " "$work/sent"
grep -q "^fleet send proxy (no live session for lane other) " "$work/sent"
run
[ "$(wc -l < "$work/sent")" = 3 ]
echo "red-main-notify: ok"
