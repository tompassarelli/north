#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
scratch=$(mktemp -d "${TMPDIR:-/tmp}/gh-app-token-lander-test.XXXXXX")
server_pid=
trap '[ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null; rm -rf "${scratch:?}"' EXIT
mkdir -p "$scratch/bin" "$scratch/secrets/nexus" "$scratch/run"
: >"$scratch/age.txt"
: >"$scratch/secrets/nexus/north-lander.yaml"

cat >"$scratch/server.ts" <<'TS'
import { createVerify, generateKeyPairSync } from "crypto";
import { writeFileSync } from "fs";
const dir = process.argv[2];
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048, publicKeyEncoding: { type: "spki", format: "pem" }, privateKeyEncoding: { type: "pkcs1", format: "pem" } });
writeFileSync(`${dir}/creds.json`, JSON.stringify({ app_id: 5603348, installation_id: 178367738, pem: privateKey }));
let n = 0;
const server = Bun.serve({ port: 0, async fetch(req) {
  const [h, b, s] = (req.headers.get("authorization") ?? "").replace(/^Bearer /, "").split(".");
  const ok = createVerify("RSA-SHA256").update(`${h}.${b}`).verify(publicKey, s ?? "", "base64url");
  const iss = JSON.parse(Buffer.from(b ?? "", "base64url").toString() || "{}").iss;
  if (!ok || iss !== "5603348" || new URL(req.url).pathname !== "/app/installations/178367738/access_tokens") return Response.json({ message: "Bad credentials" }, { status: 401 });
  return Response.json({ token: `ghs_lander_${++n}`, expires_at: new Date(Date.now() + 3600_000).toISOString() }, { status: 201 });
} });
writeFileSync(`${dir}/port`, String(server.port));
TS
bun "$scratch/server.ts" "$scratch" &
server_pid=$!
for _ in $(seq 100); do [ -s "$scratch/port" ] && break; sleep 0.05; done
printf '#!/usr/bin/env bash\ncat "%s/creds.json"\n' "$scratch" >"$scratch/bin/sops"
chmod +x "$scratch/bin/sops"

run() {
  PATH="$scratch/bin:$PATH" SOPS_AGE_KEY_FILE="$scratch/age.txt" SOUTH_SECRETS="$scratch/secrets" \
    GH_APP_API="http://127.0.0.1:$(cat "$scratch/port")" XDG_RUNTIME_DIR="$scratch/run" "$repo/bin/gh-app-token" "$@"
}

[ "$(run --app lander)" = ghs_lander_1 ]
[ "$(stat -c %a "$scratch/run/gh-app-token-lander")" = 600 ]
[ ! -e "$scratch/run/gh-app-token" ]
[ "$(run --app lander)" = ghs_lander_1 ]
! run --app nosuch 2>"$scratch/err" && grep -Fq 'unknown app nosuch' "$scratch/err"
! run 2>/dev/null

echo "gh-app-token-lander: ok"
