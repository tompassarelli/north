use std::time::Duration;

const HUB: &str = "gex45";
const SHELL_LIMIT: Duration = Duration::from_secs(45);
const MINT_LIMIT: Duration = Duration::from_secs(90);
const LAUNCH_LIMIT: Duration = Duration::from_secs(150);
const FETCH: &str = "for file in \"$HOME\"/code/*/main/.safe-push; do\n  [ -f \"$file\" ] || continue\n  git -C \"$(dirname \"$file\")\" ls-remote origin HEAD >/dev/null || { echo \"$file: fetch failed\" >&2; exit 1; }\ndone";
pub(super) const PROVIDERS: [&str; 2] = ["claude", "opencode"];

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

pub(super) fn core_loop(host: &str, sha: &str, installed: &[String], landers: Option<&Landers>) -> CoreLoop {
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
    if is_hub(host) {
        return CoreLoop { probes, not_applicable };
    }
    not_applicable.extend(PROVIDERS.iter().copied().filter(|provider| !installed.iter().any(|present| present.as_str() == *provider)).map(str::to_owned));
    for provider in PROVIDERS.iter().filter(|provider| installed.iter().any(|present| present.as_str() == **provider)) {
        probes.push(probe(format!("a --test {provider} worker answers its first turn"), format!("{north} runtime probe launch {provider}"), LAUNCH_LIMIT));
    }
    CoreLoop { probes, not_applicable }
}

pub(super) fn installed_script() -> String {
    format!(
        "for provider in {}; do\n  if [ -x \"$HOME/.local/lib/$provider/current/bin/$provider\" ] || [ -f \"$HOME/.local/bin/$provider\" ] || command -v \"$provider\" >/dev/null 2>&1; then printf '%s\\n' \"$provider\"; fi\ndone\ntrue\n",
        PROVIDERS.join(" ")
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
