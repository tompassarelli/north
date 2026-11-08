#!/usr/bin/env bash
set -euo pipefail
cache=${CI_TOOLS_CACHE:-$HOME/.cache/ci-tools}
mkdir -p "$cache"
fetch() {
  [ -s "$cache/$1" ] && return 0
  curl -fsSL --connect-timeout 20 --max-time 120 --retry 4 --retry-delay 10 -o "$cache/$1.part" "$2" && mv "$cache/$1.part" "$cache/$1"
}
if fetch betterleaks.tar.gz https://github.com/betterleaks/betterleaks/releases/download/v1.10.0/betterleaks_1.10.0_linux_x64.tar.gz; then
  echo "9d9ad503cd5d80d18a939352266c3fcca52f10da9b83cd5cfe646989b13940fe  $cache/betterleaks.tar.gz" | sha256sum -c -
  sudo tar -xz -C /usr/local/bin -f "$cache/betterleaks.tar.gz" betterleaks
else
  echo "::warning::the betterleaks release download failed; building v1.10.0 through the Go module proxy"
  GOBIN="$RUNNER_TEMP/go-bin" go install github.com/betterleaks/betterleaks@v1.10.0
  sudo install "$RUNNER_TEMP/go-bin/betterleaks" /usr/local/bin/betterleaks
fi
[ "${CI_TOOLS_SKIP_AST_GREP:-}" != true ] || exit 0
if fetch ast-grep.zip https://github.com/ast-grep/ast-grep/releases/download/0.42.1/app-x86_64-unknown-linux-gnu.zip; then
  echo "5de8b87cba67fc8dc3e239d54b6484802ad745a7ae3de76be4fe89661dc52657  $cache/ast-grep.zip" | sha256sum -c -
  sudo unzip -o -q "$cache/ast-grep.zip" ast-grep -d /usr/local/bin
else
  echo "::warning::the ast-grep release download failed; installing 0.42.1 from the npm registry"
  sudo npm install -g @ast-grep/cli@0.42.1
fi
