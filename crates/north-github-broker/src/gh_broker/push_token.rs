use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use north_base::platform::broker_client::{AGENT_APP, Client};
use north_base::platform::ssh_retry::{Link, Reply, Rerun, retried};

const USAGE: &str = "usage: north ops github push-token --host HOST...

Mint the North Fleet App installation token the broker holds with at least 45 minutes left, list the repositories the app covers,
and copy both over ssh to ~/.local/state/north/gh-write-token and gh-write-token.repos on each HOST (mode 0600), where git and gh
read them for 55 minutes. It also writes ~/.local/state/north/github-installation-token.yaml (token, expires_at, the repositories and
the permissions the broker grants it) beside them, for the broker to read as its scoped token. A host whose copy fails is tried again every 10 seconds, with a freshly asked token, until it succeeds or
27 minutes have passed since the start (the next scheduled run follows at 30); hosts already copied are not copied again. One line
on stderr per failed attempt names the host and the reason. Exits 1 when a host still has no fresh token.";

const PAUSE: Duration = Duration::from_secs(10);
const WINDOW: Duration = Duration::from_secs(27 * 60);
const VALID_FOR: u64 = 2700;
const CONNECT_TIMEOUT: &str = "ConnectTimeout=10";

const AGENT_PERMISSIONS: [(&str, &str); 5] = [("actions", "read"), ("contents", "read"), ("issues", "write"), ("metadata", "read"), ("pull_requests", "write")];

const INSTALL: &str = "set -eu
umask 077
dir=$HOME/.local/state/north
mkdir -p \"$dir\"
IFS= read -r token
IFS= read -r installation
IFS= read -r expires
IFS= read -r permissions
IFS= read -r scope
printf %s \"$token\" > \"$dir/gh-write-token.tmp\"
cat > \"$dir/gh-write-token.repos.tmp\"
if [ -n \"$installation\" ]; then
{
printf 'token: %s\\n' \"$installation\"
printf 'expires_at: %s\\n' \"$expires\"
printf 'repositories: %s\\n' \"$scope\"
printf 'permissions: %s\\n' \"$permissions\"
} > \"$dir/github-installation-token.yaml.tmp\"
mv \"$dir/github-installation-token.yaml.tmp\" \"$dir/github-installation-token.yaml\"
fi
mv \"$dir/gh-write-token.repos.tmp\" \"$dir/gh-write-token.repos\"
mv \"$dir/gh-write-token.tmp\" \"$dir/gh-write-token\"";

fn agent_permissions() -> BTreeMap<String, String> {
    AGENT_PERMISSIONS.iter().map(|(name, level)| ((*name).to_owned(), (*level).to_owned())).collect()
}

pub trait Hosts {
    fn copy(&mut self, host: &str) -> Result<(), String>;
    fn pause(&mut self, wait: Duration);
    fn elapsed(&self) -> Duration;
    fn say(&mut self, line: String);
}

pub fn deliver(hosts: &[String], window: Duration, pause: Duration, run: &mut impl Hosts) -> Vec<String> {
    let mut pending: Vec<String> = hosts.to_vec();
    let mut attempt = 0_u32;
    while !pending.is_empty() {
        attempt += 1;
        let mut failed = Vec::new();
        for host in pending {
            match run.copy(&host) {
                Ok(()) => {
                    if attempt > 1 {
                        run.say(format!("host-gh-token: copy to {host} succeeded on attempt {attempt}"));
                    }
                }
                Err(reason) => {
                    run.say(format!("host-gh-token: copy to {host} failed on attempt {attempt}: {reason}"));
                    failed.push(host);
                }
            }
        }
        pending = failed;
        if !pending.is_empty() {
            if run.elapsed() + pause >= window {
                return pending;
            }
            run.pause(pause);
        }
    }
    pending
}

struct Control {
    repositories: String,
    started: Instant,
}

impl Control {
    fn scoped(&self) -> Result<[String; 4], String> {
        let names: Vec<String> = self.repositories.lines().filter_map(|full| full.rsplit('/').next()).map(str::to_owned).collect();
        let granted = Client::discover().github_token(AGENT_APP, &names, &agent_permissions(), VALID_FOR).map_err(|why| format!("no agent-set token from the broker: {why}"))?;
        let repositories = granted.repositories.ok_or("the broker granted no repository list")?;
        let levels = granted.permissions.iter().map(|(name, level)| format!("{name}:{level}")).collect::<Vec<_>>().join(",");
        Ok([granted.token, granted.expires_at, repositories.join(","), levels])
    }
}

impl Hosts for Control {
    fn copy(&mut self, host: &str) -> Result<(), String> {
        let raw = Client::discover().github_token(AGENT_APP, &[], &Default::default(), VALID_FOR).map_err(|why| format!("no token from the broker: {why}"))?.token;
        let [token, expires, scope, levels] = self.scoped().unwrap_or_else(|why| {
            eprintln!("host-gh-token: {host} keeps its previous scoped token this round: {why}");
            Default::default()
        });
        let payload = format!("{raw}\n{token}\n{expires}\n{levels}\n{scope}\n{}\n", self.repositories);
        retried(host, Rerun::Anywhere, |link| install(host, &payload, link))
    }

    fn pause(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn say(&mut self, line: String) {
        eprintln!("{line}");
    }
}

fn install(host: &str, payload: &str, link: Link) -> Reply<Result<(), String>> {
    let spawned = Command::new("ssh")
        .args(link.options())
        .args(["-o", "BatchMode=yes", "-o", CONNECT_TIMEOUT, "-o", "ServerAliveInterval=5", "-o", "ServerAliveCountMax=3", host, INSTALL])
        .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped())
        .spawn();
    let output = spawned.and_then(|mut child| {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload.as_bytes());
        }
        child.wait_with_output()
    });
    match output {
        Err(error) => Reply { value: Err(format!("cannot run ssh: {error}")), exit: None, said: String::new() },
        Ok(output) => {
            let said = String::from_utf8_lossy(&output.stderr).into_owned();
            let first = said.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("no error text").to_owned();
            let value = if output.status.success() { Ok(()) } else { Err(format!("ssh exited {}: {first}", output.status.code().map_or("on a signal".to_owned(), |code| code.to_string()))) };
            Reply { value, exit: output.status.code(), said }
        }
    }
}

fn repositories() -> Result<String, String> {
    let token = Client::discover().github_token(AGENT_APP, &[], &Default::default(), VALID_FOR)?.token;
    let mut curl = Command::new("curl")
        .args(["-fsS", "-K", "-", "-H", "Accept: application/vnd.github+json", "https://api.github.com/installation/repositories?per_page=100"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|error| format!("cannot run curl: {error}"))?;
    if let Some(mut stdin) = curl.stdin.take() {
        let _ = writeln!(stdin, "header = \"Authorization: Bearer {token}\"");
    }
    let output = curl.wait_with_output().map_err(|error| format!("curl: {error}"))?;
    if !output.status.success() {
        return Err(format!("listing the app's repositories failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    let answer: serde_json::Value = serde_json::from_slice(&output.stdout).map_err(|error| format!("GitHub's repository list is not JSON: {error}"))?;
    let names: Vec<String> = answer["repositories"].as_array().into_iter().flatten().filter_map(|repository| repository["full_name"].as_str()).map(str::to_ascii_lowercase).collect();
    if names.is_empty() {
        return Err("GitHub lists no repository for the app".to_owned());
    }
    Ok(names.join("\n"))
}

pub fn run(arguments: &[&str]) -> u8 {
    let mut hosts = Vec::new();
    let mut words = arguments.iter().copied();
    while let Some(word) = words.next() {
        match (word, words.next()) {
            ("--host", Some(host)) => hosts.push(host.to_owned()),
            ("-h" | "--help", _) => {
                println!("{USAGE}");
                return 0;
            }
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    if hosts.is_empty() {
        eprintln!("{USAGE}");
        return 2;
    }
    let started = Instant::now();
    let repositories = match repositories() {
        Ok(repositories) => repositories,
        Err(why) => {
            eprintln!("host-gh-token: {why}");
            return 1;
        }
    };
    let left = deliver(&hosts, WINDOW, PAUSE, &mut Control { repositories, started });
    for host in &left {
        eprintln!("host-gh-token: {host} still has no fresh token after {} min", started.elapsed().as_secs() / 60);
    }
    u8::from(!left.is_empty())
}

#[cfg(test)]
#[path = "push_token_tests.rs"]
mod tests;
