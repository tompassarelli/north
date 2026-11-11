#!/usr/bin/env bash
set -uo pipefail
missing=()
for package in "$@"; do dpkg -s "$package" >/dev/null 2>&1 || missing+=("$package"); done
[ "${#missing[@]}" -gt 0 ] || exit 0
for attempt in 1 2; do
  if [ "${CI_APT_FAIL_FIRST_ATTEMPT:-}" = true ] && [ "$attempt" = 1 ]; then
    echo "::warning::apt attempt 1 failed on purpose (fail-first-apt-attempt)"
    continue
  fi
  sudo timeout 60 apt-get -o Acquire::http::Timeout=15 -o Acquire::Retries=1 update -qq || true
  sudo timeout 90 apt-get -o Acquire::http::Timeout=15 -o Acquire::Retries=1 install -y --no-install-recommends "${missing[@]}" && exit 0
  echo "::warning::apt could not install ${missing[*]} (attempt $attempt of 2)"
done
exit 1
