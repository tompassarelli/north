mod broker;
mod calls;
mod http;
mod server;
mod upstream;

use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::platform::gh_broker_route::{Place, source_config};
use broker::{Broker, Settings};
use upstream::Upstream;

const USAGE: &str = "usage: north gh-broker serve [--resident] | north gh-broker release | north gh-broker quota

The one local GitHub broker every gh call on this host goes through (north#638). north gh-session,
which bin/gh runs in a fleet session, starts the broker when its socket does not answer and runs
the real gh with a private GH_CONFIG_DIR whose http_unix_socket points at the broker. The broker
forwards each request with the caller's own credentials and: answers identical reads from one
GitHub call (in flight, or fetched within 20 s); revalidates older reads with If-None-Match, so a
304 serves the stored body without spending rate limit; after a 403 or 429 with Retry-After, an
exhausted x-ratelimit-remaining or a secondary limit, makes no GitHub call until the wait passes,
serving stored reads and refusing the rest with 429; spaces writes 1 s apart. Socket:
${XDG_RUNTIME_DIR}/north-gh-broker/broker.sock. It exits after 30 idle minutes.

--resident    run as the host's user service: never exit idle, and once listening set
              http_unix_socket in the user's own gh config, so every gh on the host (timers,
              services, scripts, interactive shells) goes through the broker; tells systemd READY=1
release       remove that http_unix_socket line again (the service's ExecStopPost), so gh
              calls GitHub directly while the broker is down
quota         print the user's own login's core and graphql rate limit left (GET /rate_limit costs none),
              so its hourly drop can be set against the call log

Each call is logged to calls.log beside the socket as: unix time, caller (the fleet session, else
the caller's systemd unit), served from, status, method, target.";

const IDLE_EXIT: Duration = Duration::from_secs(30 * 60);
const TAKE_OVER_LIMIT: Duration = Duration::from_secs(30);

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["serve"] => serve(false),
        ["serve", "--resident"] => serve(true),
        ["release"] => release(),
        ["quota"] => quota(),
        ["-h" | "--help"] => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn serve(resident: bool) -> u8 {
    let place = Place::from_env();
    let lock = if resident { take_over(&place) } else { place.exclusive() };
    if !resident && (lock.is_none() || UnixStream::connect(place.socket()).is_ok()) {
        return 0;
    }
    let _ = std::fs::remove_file(place.socket());
    let listener = match UnixListener::bind(place.socket()) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("north gh-broker: cannot listen on {}: {error}", place.socket().display());
            return 1;
        }
    };
    let broker = Arc::new(Broker::new(Upstream::new(upstream::GITHUB_API), Settings::default()));
    let activity = Arc::new(server::Activity::default());
    if resident {
        if let Err(error) = place.claim_user_gh(&source_config()) {
            eprintln!("north gh-broker: cannot point the user's gh config at the broker: {error}");
            return 1;
        }
        notify_ready();
    }
    watch(&place, Arc::clone(&activity), resident);
    server::serve(&listener, &broker, &activity, &Arc::new(calls::CallLog::at(place.calls())));
    drop(lock);
    0
}

fn take_over(place: &Place) -> Option<std::fs::File> {
    let _ = std::fs::write(place.resident_wanted(), b"");
    let started = Instant::now();
    let lock = loop {
        if let Some(lock) = place.exclusive() {
            break Some(lock);
        }
        if started.elapsed() >= TAKE_OVER_LIMIT {
            eprintln!("north gh-broker: an on-demand broker kept the lock for {} s; serving without it", TAKE_OVER_LIMIT.as_secs());
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let _ = std::fs::remove_file(place.resident_wanted());
    lock
}

fn watch(place: &Place, watched: Arc<server::Activity>, resident: bool) {
    let (socket, wanted) = (place.socket(), place.resident_wanted());
    let bound = inode(&socket);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let superseded = inode(&socket) != bound;
            if resident {
                if superseded {
                    eprintln!("north gh-broker: {} is no longer this broker's socket; exiting so the service binds it again", socket.display());
                    std::process::exit(1);
                }
                continue;
            }
            let Some(idle) = watched.idle_for() else {
                continue;
            };
            if superseded || wanted.exists() || idle >= IDLE_EXIT {
                if !superseded {
                    let _ = std::fs::remove_file(&socket);
                }
                std::process::exit(0);
            }
        }
    });
}

fn inode(path: &Path) -> Option<(u64, u64)> {
    std::fs::symlink_metadata(path).ok().map(|meta| (meta.dev(), meta.ino()))
}

fn notify_ready() {
    let Some(socket) = std::env::var_os("NOTIFY_SOCKET") else {
        return;
    };
    if let Ok(datagram) = UnixDatagram::unbound() {
        let _ = datagram.send_to(b"READY=1", socket);
    }
}

fn release() -> u8 {
    match Place::from_env().release_user_gh(&source_config()) {
        Ok(true) => {
            println!("north gh-broker: gh calls GitHub directly until the broker runs again");
            0
        }
        Ok(false) => 0,
        Err(error) => {
            eprintln!("north gh-broker: cannot clear http_unix_socket from the user's gh config: {error}");
            1
        }
    }
}

fn quota() -> u8 {
    let Some(gh) = crate::platform::gh_shim::real_on_path(&std::env::var("PATH").unwrap_or_default()) else {
        eprintln!("north gh-broker: no gh on PATH apart from the north shim");
        return 1;
    };
    let mut command = std::process::Command::new(gh);
    command.args(["api", "rate_limit"]);
    for name in ["NORTH_FLEET_SESSION", "GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"] {
        command.env_remove(name);
    }
    command.env("GH_CONFIG_DIR", source_config());
    let output = match command.output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            eprintln!("north gh-broker: gh api rate_limit failed: {}", String::from_utf8_lossy(&output.stderr).trim());
            return 1;
        }
        Err(error) => {
            eprintln!("north gh-broker: cannot run gh: {error}");
            return 1;
        }
    };
    let Ok(answer) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        eprintln!("north gh-broker: gh api rate_limit printed no JSON");
        return 1;
    };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    for resource in ["core", "graphql"] {
        let field = |name: &str| answer["resources"][resource][name].as_u64().unwrap_or(0);
        println!("{resource}\t{} of {} left\tresets in {} min", field("remaining"), field("limit"), field("reset").saturating_sub(now).div_ceil(60));
    }
    0
}

#[cfg(test)]
fn spawn_server(listener: UnixListener, broker: Broker) {
    let (broker, activity) = (Arc::new(broker), Arc::new(server::Activity::default()));
    std::thread::spawn(move || server::serve(&listener, &broker, &activity, &Arc::new(calls::CallLog::off())));
}

#[cfg(test)]
mod tests;
