use north_base::foundation::clock::Timestamp;
use north_base::verdict_cache::{Pass, REMOTE_DIR, Replica, Store, exchange, remote_hits, remote_record, sync};
use std::process::Command;

pub const USAGE: &str = "north capacity offload verdict hit KEY... (prints KEY HOST SECONDS SHA for each key a check host holds a pass for) | record --host NAME --key KEY (--pass SHA | --fail, which clears the key on every host) | sync [--host NAME]... (copies each recorded path set, pass and clear this host or a build host lacks to the other, both ways, deletes a pass with a clear on either side from both, and prints one line per host: files sent, files received, passes cleared)";
const LOCAL: &str = "local";
const SSH: [&str; 6] = ["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=2", "-oServerAliveInterval=2"];

fn ssh(destination: &str, script: &str) -> Option<String> {
    remote(destination, script, b"").ok().map(|output| String::from_utf8_lossy(&output).into_owned())
}

fn remote(destination: &str, script: &str, input: &[u8]) -> std::io::Result<Vec<u8>> {
    exchange(Command::new("ssh").args(SSH).args([destination, "--", script]), input)
}

struct Remote<'a>(&'a str);

impl Replica for Remote<'_> {
    fn root(&self) -> String {
        REMOTE_DIR.to_owned()
    }

    fn run(&self, script: &str, input: &[u8]) -> std::io::Result<Vec<u8>> {
        remote(self.0, script, input)
    }
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

fn sync_hosts(names: &[&str]) -> u8 {
    let hosts: Vec<(String, String)> = super::build_hosts().into_iter().filter(|(name, _)| names.is_empty() || names.contains(&name.as_str())).collect();
    if let Some(missing) = names.iter().find(|name| !hosts.iter().any(|(host, _)| host == **name)) {
        eprintln!("north capacity offload verdict: {missing} is not a build host");
        return 1;
    }
    let local = Store::here();
    let mut code = 0;
    for (name, destination) in hosts {
        match sync(&local, &Remote(&destination)) {
            Ok(report) => println!("{name}: sent {}, received {}, cleared {}", report.sent, report.received, report.cleared),
            Err(error) => {
                eprintln!("north capacity offload verdict: could not sync with {name}: {error}");
                code = 1;
            }
        }
    }
    code
}

fn clear(key: &str) -> u8 {
    let mut code = record(LOCAL, key, None);
    for (host, _) in super::build_hosts() {
        code = code.max(record(&host, key, None));
    }
    code
}

fn sync_names<'a>(words: &[&'a str]) -> Option<Vec<&'a str>> {
    words.chunks(2).map(|pair| match pair {
        ["--host", name] => Some(*name),
        _ => None,
    }).collect()
}

pub fn run(words: &[&str]) -> u8 {
    match words {
        ["hit", keys @ ..] if !keys.is_empty() => hits(&keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>()),
        ["record", "--host", host, "--key", key, "--pass", sha] => record(host, key, Some(&Pass { at: Timestamp::now(), sha: (*sha).to_owned() })),
        ["record", "--host", _, "--key", key, "--fail"] => clear(key),
        ["sync", rest @ ..] => match sync_names(rest) {
            Some(names) => sync_hosts(&names),
            None => {
                eprintln!("usage: {USAGE}");
                2
            }
        },
        _ => {
            eprintln!("usage: {USAGE}");
            2
        }
    }
}
