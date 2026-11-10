use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use north_base::platform::broker_client::{self, Client, SYSTEM_SOCKET};

use crate::credentials::Source;
use crate::policy::Policy;
use crate::server::Broker;

const PRINTED_VALIDITY: u64 = 2700;
const USAGE: &str = "usage: north broker serve [--user | --socket PATH --policy FILE] | north broker github-token --app agent|lander [--repo NAME]... [--permission NAME:LEVEL]... [--valid-for SECONDS] | north broker inspect [--json]

The credential broker (south#94): the one process that holds the GitHub App keys and the cloud API tokens.
serve         run it. As a system service (the default) it listens on /run/north-broker/broker.sock, loads each secret
              as a systemd credential and answers each caller uid by the ceilings of its policy file
              (default /etc/north-broker/policy.toml); --user serves the calling user alone from its own sops-decrypted secrets
              at ${XDG_RUNTIME_DIR}/north-broker/broker.sock and exits after 30 idle minutes, which is what the clients start
              when no system broker answers
github-token  print an installation token for the app, limited to the repositories and permissions named (metadata read is
              always added); without --repo or --permission the caller's whole ceiling is granted; --valid-for SECONDS (up to 3300, default 2700)
              returns only a token with at least that long to live, minting a new one when the held one has less, so a token
              printed for another process to keep outlives a 30-minute refresh
inspect       read a token on stdin and print the repositories and permissions the broker granted it
Every request is logged to stderr with the caller's uid, pid and systemd unit and what was granted; the user broker's stderr is broker.log beside its socket. Clients take the socket
from NORTH_BROKER_SOCKET, else /run/north-broker/broker.sock, else their own ${XDG_RUNTIME_DIR}/north-broker/broker.sock.
A cloud call goes to the same socket as plain HTTP at /hcloud/..., /digitalocean/... or /cloudflare/... with any placeholder
token; the broker forwards it only if the caller's allowlist names the method and path, and never returns the real token.";

const USER_IDLE: Duration = Duration::from_secs(30 * 60);
const DEFAULT_POLICY: &str = "/etc/north-broker/policy.toml";

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["-h" | "--help"] => {
            println!("{USAGE}");
            0
        }
        ["serve", rest @ ..] => serve(rest),
        ["github-token", rest @ ..] => finish(github_token(rest)),
        ["inspect", rest @ ..] => finish(inspect(rest)),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn finish(result: Result<(), String>) -> u8 {
    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("north broker: {message}");
            1
        }
    }
}

fn github_token(arguments: &[&str]) -> Result<(), String> {
    let (mut app, mut repositories, mut permissions, mut valid_for) = (None, Vec::new(), BTreeMap::new(), PRINTED_VALIDITY);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match (*word, words.next()) {
            ("--app", Some(value)) => app = Some((*value).to_owned()),
            ("--repo", Some(value)) => repositories.push((*value).to_owned()),
            ("--valid-for", Some(value)) => valid_for = value.parse().map_err(|_| format!("--valid-for {value} is not a number of seconds"))?,
            ("--permission", Some(value)) => {
                let (name, level) = value.split_once(':').ok_or_else(|| format!("--permission {value} is not NAME:LEVEL"))?;
                permissions.insert(name.to_owned(), level.to_owned());
            }
            _ => return Err(USAGE.to_owned()),
        }
    }
    let app = app.ok_or("--app agent|lander is required")?;
    println!("{}", Client::discover().github_token(&app, &repositories, &permissions, valid_for)?.token);
    Ok(())
}

fn inspect(arguments: &[&str]) -> Result<(), String> {
    let json = match arguments {
        [] => false,
        ["--json"] => true,
        _ => return Err(USAGE.to_owned()),
    };
    let mut token = String::new();
    std::io::stdin().read_to_string(&mut token).map_err(|error| error.to_string())?;
    let answer = Client::discover().inspect(token.trim())?;
    if json {
        println!("{answer}");
    } else {
        println!("app: {}\nrepositories: {}\npermissions: {}\nexpires_at: {}", answer["app"], answer["repositories"], answer["permissions"], answer["expires_at"]);
    }
    Ok(())
}

fn serve(arguments: &[&str]) -> u8 {
    let (mut user, mut socket, mut policy) = (false, None, None);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match (*word, words.clone().next()) {
            ("--user", _) => user = true,
            ("--socket", Some(value)) => {
                socket = Some(PathBuf::from(value));
                words.next();
            }
            ("--policy", Some(value)) => {
                policy = Some(PathBuf::from(value));
                words.next();
            }
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    finish(start(user, socket, policy))
}

fn start(user: bool, socket: Option<PathBuf>, policy: Option<PathBuf>) -> Result<(), String> {
    let (socket, broker, idle) = if user {
        let socket = socket.or_else(broker_client::user_socket).ok_or("XDG_RUNTIME_DIR is unset")?;
        let uid = rustix::process::getuid().as_raw();
        let source = Source::south();
        if let Source::Sops(secrets) = &source {
            use std::os::unix::fs::MetadataExt;
            let owner = std::fs::metadata(secrets).map(|meta| meta.uid()).map_err(|error| format!("cannot read {}: {error}", secrets.display()))?;
            if owner != uid {
                return Err(format!("{} belongs to uid {owner}, not uid {uid}; the user broker serves only the operator that owns the secrets", secrets.display()));
            }
        }
        (socket, Broker::new(Policy::for_user(uid), source), Some(USER_IDLE))
    } else {
        let policy = Policy::load(&policy.unwrap_or_else(|| PathBuf::from(DEFAULT_POLICY)))?;
        (socket.unwrap_or_else(|| PathBuf::from(SYSTEM_SOCKET)), Broker::new(policy, Source::systemd()?), None)
    };
    let _ = rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable);
    let Some(listener) = bind(&socket, user)? else { return Ok(()) };
    let broker = Arc::new(broker);
    if let Some(limit) = idle {
        crate::retire::when_idle(&broker, socket.clone(), limit);
    }
    eprintln!("north-broker: serving {}", socket.display());
    broker.serve(&listener);
    Ok(())
}

fn bind(socket: &Path, user: bool) -> Result<Option<UnixListener>, String> {
    if UnixStream::connect(socket).is_ok() {
        return Ok(None);
    }
    let parent = socket.parent().ok_or_else(|| format!("{} has no directory", socket.display()))?;
    std::fs::DirBuilder::new().recursive(true).mode(if user { 0o700 } else { 0o750 }).create(parent).map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket).map_err(|error| format!("cannot listen on {}: {error}", socket.display()))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(if user { 0o600 } else { 0o660 })).map_err(|error| error.to_string())?;
    Ok(Some(listener))
}
