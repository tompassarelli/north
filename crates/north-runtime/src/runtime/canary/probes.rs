use std::time::Duration;

const HUB: &str = "gex45";
const SHELL_LIMIT: Duration = Duration::from_secs(45);
const MINT_LIMIT: Duration = Duration::from_secs(90);
const LAUNCH_LIMIT: Duration = Duration::from_secs(150);
const VERIFY_NORTH_LIMIT: Duration = Duration::from_secs(600);
const SCRATCH_LANDING_LIMIT: Duration = Duration::from_secs(240);
const FETCH: &str = "for file in \"$HOME\"/code/*/main/.safe-push; do\n  [ -f \"$file\" ] || continue\n  git -C \"$(dirname \"$file\")\" ls-remote origin HEAD >/dev/null || { echo \"$file: fetch failed\" >&2; exit 1; }\ndone";
pub(super) const PROVIDERS: [&str; 3] = ["claude", "opencode", "pi"];

pub(in crate::runtime) struct Probe {
    pub(super) name: String,
    pub(super) script: String,
    pub(super) limit: Duration,
}

pub(super) struct CoreLoop {
    pub(super) probes: Vec<Probe>,
    pub(super) not_applicable: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::runtime) struct Landers {
    pub(super) served: Vec<String>,
    pub(super) unserved: Vec<String>,
}

const LANDER_SCAN: &str = "\
pids=$(pgrep -u \"$(id -u)\" -f 'land serve') || pids=\n\
served=()\n\
all=\n\
program=\n\
for pid in $pids; do\n\
mapfile -d '' -t args < \"/proc/$pid/cmdline\" 2>/dev/null || continue\n\
for ((at = 1; at < ${#args[@]}; at++)); do\n\
[ \"${args[at]}\" = land ] && [ \"${args[at + 1]}\" = serve ] || continue\n\
at=$((at + 2))\n\
found=\n\
while [ $at -lt ${#args[@]} ]; do\n\
case \"${args[at]}\" in\n\
--all) all=1; found=1 ;;\n\
--repo) at=$((at + 1)); repo=\"${args[at]}\"; if [ \"${repo#/}\" = \"$repo\" ]; then repo=\"$(readlink \"/proc/$pid/cwd\" 2>/dev/null)/$repo\"; fi; served+=(\"$repo\"); found=1 ;;\n\
--app-token) at=$((at + 1)); [ -z \"$program\" ] && program=\"${args[at]}\" ;;\n\
--checker) at=$((at + 1)) ;;\n\
esac\n\
at=$((at + 1))\n\
done\n\
if [ -z \"$found\" ]; then\n\
cwd=$(readlink \"/proc/$pid/cwd\" 2>/dev/null) && served+=(\"$cwd\")\n\
fi\n\
break\n\
done\n\
done\n";

fn probe(name: impl Into<String>, script: String, limit: Duration) -> Probe {
    Probe { name: name.into(), script, limit }
}

pub(super) fn sessions_reach_hub(config: &toml_edit::DocumentMut, name: &str) -> bool {
    let Some(table) = config.get("hosts").and_then(|hosts| hosts.get(name)) else { return true };
    let launches = table.get("launches").and_then(|launches| launches.as_bool()).unwrap_or(true);
    let reaches = table.get("reaches").and_then(|reaches| reaches.as_array()).is_none_or(|reached| reached.iter().any(|named| named.as_str() == Some(HUB)));
    launches && reaches
}

pub(super) fn core_loop(host: &str, sha: &str, launchable: &[String], reaches_hub: bool, settles: bool, landers: Option<&Landers>) -> CoreLoop {
    let runtime = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/runtimes/{sha}\"");
    let north = format!("{runtime}/north");
    let mut probes = Vec::new();
    match (is_hub(host), reaches_hub) {
        (true, _) => probes.push(probe(format!("{HUB} fetches each landing repository with its credential"), FETCH.to_owned(), MINT_LIMIT)),
        (false, true) => probes.push(probe(
            format!("a sandboxed worker shell runs ssh {HUB} true"),
            format!("NORTH_FLEET_ROLE=worker NORTH_WORKER_SANDBOX=1 {north} --worker-sandbox 'ssh -o BatchMode=yes -o ConnectTimeout=15 {HUB} true'"),
            SHELL_LIMIT,
        )),
        (false, false) => {}
    }
    let mut not_applicable = Vec::new();
    match landers {
        Some(landers) => {
            not_applicable.extend(landers.unserved.iter().map(|file| format!("no running lander serves {file}")));
            match landers.served.is_empty() {
                true => not_applicable.push("each repository's lander mints its push token".to_owned()),
                false => probes.push(probe("each repository's lander mints its push token", lander_tokens(&north), MINT_LIMIT)),
            }
        }
        None => probes.push(probe("each repository's lander mints its push token", lander_tokens(&north), MINT_LIMIT)),
    }
    if settles {
        probes.push(probe("verify-north walks the spine on the promoted runtime", verify_north(&north, &runtime), VERIFY_NORTH_LIMIT));
        probes.push(probe("one scratch landing lands on its origin", scratch_landing(&north), SCRATCH_LANDING_LIMIT));
    }
    if is_hub(host) {
        return CoreLoop { probes: judging(sha, probes), not_applicable };
    }
    not_applicable.extend(PROVIDERS.iter().copied().filter(|provider| !launchable.iter().any(|present| present.as_str() == *provider)).map(str::to_owned));
    for provider in PROVIDERS.iter().filter(|provider| launchable.iter().any(|present| present.as_str() == **provider)) {
        probes.push(probe(format!("a --test {provider} worker answers its first turn"), format!("{north} runtime probe launch {provider}"), LAUNCH_LIMIT));
    }
    CoreLoop { probes: judging(sha, probes), not_applicable }
}

/// Every process a probe starts follows a link naming the judged runtime, so a resident one such as a probe worker's
/// session host stays on the build being judged instead of moving onto whichever runtime `current` or the lander link
/// names while the canary runs.
fn judging(sha: &str, probes: Vec<Probe>) -> Vec<Probe> {
    let pinned = format!("link=\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/judged/{sha}\"\nmkdir -p \"${{link%/*}}\" && ln -sfn \"../runtimes/{sha}\" \"$link\"\nexport {}=\"$link\"\n", north_runtime_follow::LINK);
    probes.into_iter().map(|probe| Probe { script: format!("{pinned}{}", probe.script), ..probe }).collect()
}

/// The product verification of the judged runtime, run only when a Claude account can place its
/// worker: no account is a probe that cannot judge the runtime, not one the runtime failed.
fn verify_north(north: &str, runtime: &str) -> String {
    VERIFY_NORTH.replace("__NORTH__", north).replace("__RUNTIME__", runtime)
}

const VERIFY_NORTH: &str = r#"set -euo pipefail
north=__NORTH__
brief=$(mktemp "${TMPDIR:-/tmp}/north-canary-verify.XXXXXX")
trap 'rm -f "$brief"' EXIT
printf 'Surface: shell\nNeeds: laptop\n' > "$brief"
if ! "$north" config agents plan --place "$brief" | grep -q 'claude:'; then
  echo 'no Claude account can place the verify-north worker here' >&2
  exit 75
fi
__RUNTIME__/verify-north --north "$north"
"#;

/// A scratch repository whose lane queues one commit with the judged runtime and lands it through
/// that runtime's `land serve`, so a runtime that cannot walk the landing half of the spine fails
/// here instead of shipping to the other hosts.
fn scratch_landing(north: &str) -> String {
    SCRATCH_LANDING.replace("__NORTH__", north)
}

const SCRATCH_LANDING: &str = r#"set -euo pipefail
north=__NORTH__
root=$(mktemp -d "${TMPDIR:-/tmp}/north-canary-landing.XXXXXX")
trap 'rm -rf "$root"' EXIT
state="$root/state"
mkdir -p "$state"
export XDG_STATE_HOME="$state"
export NORTH_TEST_ROOT="$state"
export THREADS_DB="$state/threads.db"
export NORTH_PUSH_LOG="$root/pushes.tsv"
export NORTH_FLEET_TEST=1
export GIT_CONFIG_GLOBAL="$root/gitconfig"
export GIT_CONFIG_NOSYSTEM=1
: > "$root/gitconfig"
export PATH="$(dirname "$north"):$PATH"
origin="$root/origin.git"
main="$root/project/main"
lane="$root/project/worktrees/canary-landing"
mkdir -p "$main"
git -C "$main" init -q -b main
git -C "$main" config user.name canary
git -C "$main" config user.email canary@example.invalid
git -C "$main" config commit.gpgsign false
printf 'the scratch repository the runtime canary lands into\n' > "$main/README.md"
printf '[landing]\n\tqueue = lander\n\tlander-host = %s\n\tremote-hosts =\n[land]\n\tcheck = ** :: true\n' "$(hostname -s 2>/dev/null || hostname)" > "$main/.safe-push"
git -C "$main" add .
git -C "$main" commit -q -m 'Start the scratch repository'
git clone -q --bare "$main" "$origin"
git -C "$main" remote add origin "$origin"
git -C "$main" fetch -q origin
git -C "$main" worktree add -q -b canary-landing "$lane" origin/main
base=$(git -C "$origin" rev-parse refs/heads/main)
printf 'the canary change\n' > "$lane/canary.txt"
git -C "$lane" add canary.txt
git -C "$lane" commit -q -m 'Land the canary change'
checker="$root/checker"
printf '#!/bin/sh\nprintf "safe-push: verdict green\\n"\nexit 0\n' > "$checker"
chmod +x "$checker"
token="$root/token"
printf '#!/bin/sh\nprintf "ghs_canary_landing\\n"\n' > "$token"
chmod +x "$token"
(cd "$lane" && "$north" safe-push --to main >/dev/null)
(cd "$main" && "$north" land serve --once --repo "$main" --checker "$checker" --app-token "$token")
landed=$(git -C "$origin" rev-parse refs/heads/main)
if [ "$landed" = "$base" ]; then
  echo "the scratch landing did not move origin main past $base" >&2
  exit 1
fi
if ! git -C "$origin" show --format= --name-only "$landed" | grep -qx canary.txt; then
  echo "origin main $landed does not change canary.txt" >&2
  exit 1
fi
echo "landed $landed"
"#;

pub(super) fn launchable_script() -> String {
    let key = crate::providers::opencode::key_file(std::path::Path::new(".local/state/north"));
    format!(
        "for provider in {}; do\n  if [ \"$provider\" != claude ] && [ ! -f \"$HOME/{}\" ]; then continue; fi\n  if [ -x \"$HOME/.local/lib/$provider/current/bin/$provider\" ] || [ -f \"$HOME/.local/bin/$provider\" ] || command -v \"$provider\" >/dev/null 2>&1; then printf '%s\\n' \"$provider\"; fi\ndone\ntrue\n",
        PROVIDERS.join(" "),
        key.display()
    )
}

fn is_hub(destination: &str) -> bool {
    let host = destination.rsplit_once('@').map_or(destination, |(_, host)| host);
    host.split('.').next() == Some(HUB)
}

pub(super) fn repositories_script() -> String {
    let mut script = LANDER_SCAN.to_owned();
    script.push_str(
        "for file in \"$HOME\"/code/*/main/.safe-push; do\n\
         [ -f \"$file\" ] || continue\n\
         dir=$(cd \"$(dirname \"$file\")\" && pwd -P) || continue\n\
         here=\n\
         if [ -n \"$all\" ]; then\n\
         here=1\n\
         else\n\
         for repo in \"${served[@]}\"; do\n\
         [ \"$(cd \"$repo\" 2>/dev/null && pwd -P)\" = \"$dir\" ] && here=1 && break\n\
         done\n\
         fi\n\
         if [ -n \"$here\" ]; then printf 'served\\t%s\\n' \"$file\"; else printf 'unserved\\t%s\\n' \"$file\"; fi\n\
         done\n\
         exit 0\n",
    );
    script
}

pub(super) fn parse_repositories(listing: &str) -> Landers {
    let mut landers = Landers::default();
    for line in listing.lines() {
        let Some((kind, file)) = line.split_once('\t') else { continue };
        match kind {
            "served" => landers.served.push(file.to_owned()),
            "unserved" => landers.unserved.push(file.to_owned()),
            _ => {}
        }
    }
    landers
}

pub(super) fn lander_tokens(north: &str) -> String {
    let mut script = LANDER_SCAN.to_owned();
    script.push_str(&format!(
        "if [ -z \"$all\" ] && [ ${{#served[@]}} -eq 0 ]; then\n\
         printf 'no `land serve` process runs on this host\\n' >&2\n\
         exit {}\n\
         fi\n\
         judged=0\n\
         for file in \"$HOME\"/code/*/main/.safe-push; do\n\
         [ -f \"$file\" ] || continue\n\
         dir=$(cd \"$(dirname \"$file\")\" && pwd -P) || continue\n\
         if [ -z \"$all\" ]; then\n\
         here=\n\
         for repo in \"${{served[@]}}\"; do\n\
         [ \"$(cd \"$repo\" 2>/dev/null && pwd -P)\" = \"$dir\" ] && here=1 && break\n\
         done\n\
         [ -n \"$here\" ] || continue\n\
         fi\n\
         judged=$((judged + 1))\n\
         app=$(git config -f \"$file\" --get landing.push-app || echo agent)\n\
         if [ -n \"$program\" ] && [ -O \"$program\" ] && \"$program\" --app \"$app\" >/dev/null 2>&1; then continue; fi\n\
         {north} broker github-token --app \"$app\" >/dev/null 2>&1 && continue\n\
         printf 'this host cannot mint the %s push token for %s by the lander program or the broker\\n' \"$app\" \"$file\" >&2\n\
         exit {}\n\
         done\n\
         [ \"$judged\" -eq 0 ] && {{ printf 'no running lander serves any repository here\\n' >&2; exit {}; }}\n\
         exit 0",
        super::NO_VERDICT,
        super::HOST_FAULT,
        super::NO_VERDICT
    ));
    script
}
