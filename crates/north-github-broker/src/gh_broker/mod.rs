mod app_reads;
mod batch;
mod broker;
mod calls;
mod budget;
mod http;
mod lifetime;
mod push_token;
mod quota;
mod server;
mod upstream;

use std::os::fd::AsFd;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::platform::gh_broker_route::{Place, source_config};
use broker::{Broker, Settings};
use upstream::Upstream;

pub use budget::github;

const USAGE: &str = "usage: north gh-broker serve [--resident] | north gh-broker release | north gh-broker quota

The one local GitHub broker every gh call on this host goes through (north#638). north gh-session,
which bin/gh runs in a fleet session, starts the broker when its socket does not answer and runs
the real gh with a private GH_CONFIG_DIR whose http_unix_socket points at the broker. The broker
forwards each request with the caller's own credentials, except that a systemd service's REST GET or HEAD
with a personal login is sent with the North Fleet App installation token (read again with the
service's own login when the app is answered 401, 403 or 404), and: answers identical reads from one
GitHub call (in flight, or fetched within 20 s); revalidates older reads with If-None-Match, so a
304 serves the stored body without spending rate limit; after a 403 or 429 with Retry-After, an
exhausted x-ratelimit-remaining or a secondary limit, makes no GitHub call until the wait passes,
serving stored reads and refusing the rest with 429; merges GraphQL query reads from different callers
with the same token that arrive within 25 ms into one aliased query (one call, one answer each; queries
with fragments, directives or several operations go alone); spaces writes 1 s apart. Socket:
${XDG_RUNTIME_DIR}/north-gh-broker/broker.sock. It exits after 30 idle minutes.

--resident    run as the host's user service: never exit idle, and once listening set
              http_unix_socket in the user's own gh config, so every gh on the host (timers,
              services, scripts, interactive shells) goes through the broker; tells systemd READY=1
              once the live runtime moves it takes no new call, finishes the open ones, then re-execs
              the new runtime with `--adopt`, which serves the listening socket it is handed on standard
              input, so calls queued meanwhile are answered rather than reset
release       remove that http_unix_socket line again (the service's ExecStopPost), so gh
              calls GitHub directly while the broker is down
quota         print the user's own login's core and graphql rate limit left (GET /rate_limit costs none),
              so its hourly drop can be set against the call log

Each call is logged to calls.log beside the socket as: unix time, caller (the fleet session, else
the caller's systemd unit), served from, status, method, target, token (app for a GitHub App
installation token, personal for any other, none when the call carries none).";

const TAKE_OVER_LIMIT: Duration = Duration::from_secs(30);
const DRAIN_LIMIT: Duration = Duration::from_secs(60);

pub fn run(arguments: &[String]) -> u8 {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["serve"] => serve(false, false),
        ["serve", "--resident"] => serve(true, false),
        ["serve", "--resident", "--adopt"] => serve(true, true),
        ["release"] => release(),
        ["quota"] => quota::print(),
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

fn serve(resident: bool, adopt: bool) -> u8 {
    let place = Place::from_env();
    let lock = if resident { take_over(&place) } else { place.exclusive() };
    if !resident && (lock.is_none() || UnixStream::connect(place.socket()).is_ok()) {
        return 0;
    }
    let Some(listener) = adopt.then(adopted).flatten().or_else(|| bound(&place)) else {
        return 1;
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
    lifetime::watch(&place, Arc::clone(&activity), resident);
    drain_on_sigterm(place.socket(), Arc::clone(&activity));
    let ended = server::serve(&listener, DRAIN_LIMIT, &broker, &activity, &Arc::new(calls::CallLog::at(place.calls(), place.quota())), &Arc::new(app_reads::AppReads::from_host()));
    drop(lock);
    end(&ended, &listener);
    0
}

fn end(ended: &server::Ended, listener: &UnixListener) {
    if !ended.finished() {
        eprintln!("north gh-broker: {} s passed with calls still open; leaving them", DRAIN_LIMIT.as_secs());
    }
    if let server::Ended::HandingOver { .. } = ended {
        eprintln!("north gh-broker: cannot hand the socket to the new runtime: {}; exiting so the service restarts", hand_over(listener));
    }
}

fn bound(place: &Place) -> Option<UnixListener> {
    let _ = std::fs::remove_file(place.socket());
    UnixListener::bind(place.socket()).inspect_err(|error| eprintln!("north gh-broker: cannot listen on {}: {error}", place.socket().display())).ok()
}

fn hand_over(listener: &UnixListener) -> std::io::Error {
    if let Err(error) = rustix::stdio::dup2_stdin(listener) {
        return error.into();
    }
    let mut arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if !arguments.iter().any(|argument| argument == "--adopt") {
        arguments.push("--adopt".into());
    }
    north_runtime_follow::Follow::current().reexec_with(arguments)
}

fn adopted() -> Option<UnixListener> {
    let held = std::io::stdin().as_fd().try_clone_to_owned().ok()?;
    let null = std::fs::File::open("/dev/null").ok()?;
    rustix::stdio::dup2_stdin(&null).ok()?;
    let listens = rustix::net::sockopt::get_socket_acceptconn(&held).unwrap_or(false);
    if !listens {
        eprintln!("north gh-broker: standard input holds no listening socket to adopt; binding the socket afresh");
    }
    listens.then(|| UnixListener::from(held))
}

fn drain_on_sigterm(socket: std::path::PathBuf, activity: Arc<server::Activity>) {
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread().enable_all().build() else {
            return;
        };
        runtime.block_on(async {
            if let Ok(mut term) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                term.recv().await;
            }
        });
        activity.drain(&socket);
    });
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

#[cfg(test)]
fn spawn_server(listener: UnixListener, broker: Broker) -> (Arc<server::Activity>, std::thread::JoinHandle<server::Ended>) {
    let (broker, activity) = (Arc::new(broker), Arc::new(server::Activity::default()));
    let served = Arc::clone(&activity);
    (activity, std::thread::spawn(move || {
        server::serve(&listener, DRAIN_LIMIT, &broker, &served, &Arc::new(calls::CallLog::off()), &Arc::new(app_reads::AppReads::with(Box::new(|| None))))
    }))
}

#[cfg(test)]
mod tests;
