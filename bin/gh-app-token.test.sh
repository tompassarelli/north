#!/usr/bin/env bash
set -euo pipefail

repo=$(cd "$(dirname "$0")/.." && pwd)
tool="$repo/bin/gh-app-token"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/gh-app-token-test.XXXXXX")
server_pid=
trap '[ -n "$server_pid" ] && kill "$server_pid" 2>/dev/null; rm -rf "${scratch:?}"' EXIT

mkdir -p "$scratch/bin" "$scratch/secrets" "$scratch/run"
: >"$scratch/age.txt"
: >"$scratch/secrets/github-app.yaml"

bun -e '
const { generateKeyPairSync } = require("crypto");
const { privateKey, publicKey } = generateKeyPairSync("rsa", { modulusLength: 2048, publicKeyEncoding: { type: "spki", format: "pem" }, privateKeyEncoding: { type: "pkcs1", format: "pem" } });
const dir = process.argv[1];
require("fs").writeFileSync(dir + "/creds.json", JSON.stringify({ app_id: 123, client_id: "Iv1.test", installation_id: 42, private_key: privateKey }));
require("fs").writeFileSync(dir + "/public.pem", publicKey);
' "$scratch"

cat >"$scratch/bin/sops" <<EOF
#!/usr/bin/env bash
cat "$scratch/creds.json"
EOF
chmod +x "$scratch/bin/sops"

cat >"$scratch/server.ts" <<'EOF'
import { createVerify } from "crypto";
import { appendFileSync, readFileSync, writeFileSync } from "fs";
const dir = process.argv[2];
const pub = readFileSync(`${dir}/public.pem`, "utf8");
let n = 0;
const server = Bun.serve({
  port: 0,
  async fetch(req) {
    const url = new URL(req.url);
    if (req.method !== "POST" || url.pathname !== "/app/installations/42/access_tokens") return new Response("{}", { status: 404 });
    n++;
    const jwt = (req.headers.get("authorization") ?? "").replace(/^Bearer /, "");
    const [h, b, s] = jwt.split(".");
    const verified = createVerify("RSA-SHA256").update(`${h}.${b}`).verify(pub, s ?? "", "base64url");
    const claims = JSON.parse(Buffer.from(b ?? "", "base64url").toString() || "{}");
    appendFileSync(`${dir}/requests`, JSON.stringify({ n, verified, now: Math.floor(Date.now() / 1000), body: JSON.parse((await req.text()) || "null"), ...claims }) + "\n");
    if (readFileSync(`${dir}/mode`, "utf8").trim() === "error") return Response.json({ message: "Bad credentials" }, { status: 401 });
    return Response.json({ token: `ghs_test_${n}`, expires_at: new Date(Date.now() + 3600_000).toISOString() }, { status: 201 });
  },
});
writeFileSync(`${dir}/port`, String(server.port));
EOF
echo ok >"$scratch/mode"
bun "$scratch/server.ts" "$scratch" &
server_pid=$!
for _ in $(seq 100); do [ -s "$scratch/port" ] && break; sleep 0.05; done
[ -s "$scratch/port" ]

run() {
  PATH="$scratch/bin:$PATH" SOPS_AGE_KEY_FILE="$scratch/age.txt" SOUTH_SECRETS="$scratch/secrets" \
    GH_APP_API="http://127.0.0.1:$(cat "$scratch/port")" XDG_RUNTIME_DIR="$scratch/run" "$tool" "$@"
}
requests() { wc -l <"$scratch/requests" | tr -d ' '; }

[ "$(run)" = ghs_test_1 ]
jq -e '.verified == true and .iss == "123" and (.exp - .iat) == 600 and (.now - .iat) >= 59 and (.now - .iat) <= 65' \
  <(tail -1 "$scratch/requests") >/dev/null
[ "$(stat -c %a "$scratch/run/gh-app-token")" = 600 ]

[ "$(run)" = ghs_test_1 ]
[ "$(requests)" = 1 ]

jq --arg e "$(date -u -d '+4 minutes' +%Y-%m-%dT%H:%M:%SZ)" '.expires_at = $e' "$scratch/run/gh-app-token" >"$scratch/expiring"
mv "$scratch/expiring" "$scratch/run/gh-app-token"
[ "$(run)" = ghs_test_2 ]
[ "$(requests)" = 2 ]

[ "$(run --repos north,south)" = ghs_test_3 ] && jq -e '.body == {repositories: ["north", "south"], permissions: {contents: "write", metadata: "read"}}' <(tail -1 "$scratch/requests") >/dev/null || exit 1
[ "$(jq -r .token "$scratch/run/gh-app-token")" = ghs_test_2 ]
[ "$(run --repos north)" = ghs_test_4 ] && [ "$(head -1 "$scratch/requests" | jq -c .body)" = null ] || exit 1

rm "$scratch/run/gh-app-token"
echo error >"$scratch/mode"
set +e
run >"$scratch/out" 2>"$scratch/err"
status=$?
set -e
[ "$status" -ne 0 ]
[ ! -s "$scratch/out" ]
grep -Fq 'HTTP 401' "$scratch/err"
key_line=$(jq -r .private_key "$scratch/creds.json" | sed -n 2p)
! grep -Fq -e 'PRIVATE KEY' -e "$key_line" "$scratch/err"
[ ! -e "$scratch/run/gh-app-token" ]

echo "gh-app-token: ok"
