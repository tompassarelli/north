use std::time::Duration;

const HUB: &str = "gex45";
const SHELL_LIMIT: Duration = Duration::from_secs(45);
const MINT_LIMIT: Duration = Duration::from_secs(90);
const LAUNCH_LIMIT: Duration = Duration::from_secs(150);
const FETCH: &str = "for file in \"$HOME\"/code/*/main/.safe-push; do\n  [ -f \"$file\" ] || continue\n  git -C \"$(dirname \"$file\")\" ls-remote origin HEAD >/dev/null || { echo \"$file: fetch failed\" >&2; exit 1; }\ndone";
const PROVIDERS: [&str; 2] = ["claude", "opencode"];

pub(in crate::runtime) struct Probe {
    pub(super) name: String,
    pub(super) script: String,
    pub(super) limit: Duration,
}

fn probe(name: impl Into<String>, script: String, limit: Duration) -> Probe {
    Probe { name: name.into(), script, limit }
}

pub(super) fn core_loop(host: &str, sha: &str) -> Vec<Probe> {
    let north = format!("\"${{XDG_STATE_HOME:-$HOME/.local/state}}/north/runtimes/{sha}/north\"");
    let mut probes = Vec::new();
    match is_hub(host) {
        true => probes.push(probe(format!("{HUB} fetches each landing repository with its credential"), FETCH.to_owned(), MINT_LIMIT)),
        false => probes.push(probe(
            format!("a sandboxed worker shell runs ssh {HUB} true"),
            format!("NORTH_FLEET_ROLE=worker NORTH_WORKER_SANDBOX=1 {north} --worker-sandbox 'ssh -o BatchMode=yes -o ConnectTimeout=15 {HUB} true'"),
            SHELL_LIMIT,
        )),
    }
    probes.push(probe("each repository's lander mints its push token", lander_tokens(&north), MINT_LIMIT));
    if is_hub(host) {
        return probes;
    }
    probes.extend(PROVIDERS.iter().map(|provider| probe(format!("a --test {provider} worker answers its first turn"), format!("{north} runtime probe launch {provider}"), LAUNCH_LIMIT)));
    probes
}

fn is_hub(destination: &str) -> bool {
    let host = destination.rsplit_once('@').map_or(destination, |(_, host)| host);
    host.split('.').next() == Some(HUB)
}

pub(super) fn lander_tokens(north: &str) -> String {
    format!(
        "pids=$(pgrep -f 'land serve') || exit 0\n\
         program=\n\
         for pid in $pids; do\n\
         program=$(tr '\\0' '\\n' < \"/proc/$pid/cmdline\" 2>/dev/null | awk 'wanted {{ print; exit }} $0 == \"--app-token\" {{ wanted = 1 }}')\n\
         [ -n \"$program\" ] && break\n\
         done\n\
         for file in \"$HOME\"/code/*/main/.safe-push; do\n\
         [ -f \"$file\" ] || continue\n\
         app=$(git config -f \"$file\" --get landing.push-app || echo agent)\n\
         if [ -n \"$program\" ] && \"$program\" --app \"$app\" >/dev/null 2>&1; then continue; fi\n\
         {north} broker github-token --app \"$app\" >/dev/null 2>&1 && continue\n\
         printf 'this host cannot mint the %s push token for %s by the lander program or the broker\\n' \"$app\" \"$file\" >&2\n\
         exit {}\n\
         done",
        super::HOST_FAULT
    )
}
