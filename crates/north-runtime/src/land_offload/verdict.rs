use north_base::foundation::clock::Timestamp;
use north_base::verdict_cache::{Pass, Store, remote_hits, remote_record};
use std::process::{Command, Stdio};

pub const USAGE: &str = "north capacity offload verdict hit KEY... (prints KEY HOST SECONDS SHA for each key a check host holds a pass for) | record --host NAME --key KEY (--pass SHA | --fail, which clears the key on every host)";
const LOCAL: &str = "local";
const SSH: [&str; 6] = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=2", "-oServerAliveInterval=2"];

fn ssh(destination: &str, script: &str) -> Option<String> {
    let output = Command::new("ssh").args(SSH).args([destination, "--", script]).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn hits(keys: &[String]) -> u8 {
    let local = Store::here();
    for key in keys {
        if let Some(pass) = local.hit(key) {
            println!("{key} {} {} {}", crate::platform::host::name(), pass.at.unix_seconds(), pass.sha);
        }
    }
    let hosts = super::build_hosts();
    let found: Vec<(String, String)> = std::thread::scope(|scope| {
        let reads: Vec<_> = hosts.iter().map(|(name, destination)| scope.spawn(move || ssh(destination, &remote_hits(keys)).map(|text| (name.clone(), text)))).collect();
        reads.into_iter().filter_map(|read| read.join().ok().flatten()).collect()
    });
    for (host, text) in found {
        for line in text.lines() {
            let Some((key, entry)) = line.split_once(' ') else { continue };
            if keys.iter().any(|wanted| wanted == key)
                && let Some(pass) = Pass::parse(entry)
            {
                println!("{key} {host} {} {}", pass.at.unix_seconds(), pass.sha);
            }
        }
    }
    0
}

fn record(host: &str, key: &str, pass: Option<&Pass>) -> u8 {
    if host == LOCAL || host == crate::platform::host::name() {
        return match Store::here().record(key, pass) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("north capacity offload verdict: {error}");
                1
            }
        };
    }
    let Some((_, destination)) = super::build_hosts().into_iter().find(|(name, _)| name == host) else {
        eprintln!("north capacity offload verdict: {host} is not a build host");
        return 1;
    };
    let Some(script) = remote_record(key, pass) else {
        eprintln!("north capacity offload verdict: the key and sha must be hex");
        return 2;
    };
    if ssh(&destination, &script).is_some() {
        0
    } else {
        eprintln!("north capacity offload verdict: could not write {host}'s verdict cache");
        1
    }
}

fn clear(key: &str) -> u8 {
    let mut code = record(LOCAL, key, None);
    for (host, _) in super::build_hosts() {
        code = code.max(record(&host, key, None));
    }
    code
}

pub fn run(words: &[&str]) -> u8 {
    match words {
        ["hit", keys @ ..] if !keys.is_empty() => hits(&keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>()),
        ["record", "--host", host, "--key", key, "--pass", sha] => record(host, key, Some(&Pass { at: Timestamp::now(), sha: (*sha).to_owned() })),
        ["record", "--host", _, "--key", key, "--fail"] => clear(key),
        _ => {
            eprintln!("usage: {USAGE}");
            2
        }
    }
}
