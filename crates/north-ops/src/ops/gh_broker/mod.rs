mod broker;
mod calls;
mod http;
mod server;
mod upstream;

use std::os::unix::net::UnixListener;
use std::sync::Arc;
use std::time::Duration;

use crate::platform::gh_broker_route::Place;
use broker::{Broker, Settings};
use upstream::Upstream;

const USAGE: &str = "usage: north gh-broker serve

The one local GitHub broker every fleet session's gh calls go through (north#638). north gh-session,
which bin/gh runs in a fleet session, starts the broker when its socket does not answer and runs
the real gh with a private GH_CONFIG_DIR whose http_unix_socket points at the broker. The broker
forwards each request with the caller's own credentials and: answers identical reads from one
GitHub call (in flight, or fetched within 20 s); revalidates older reads with If-None-Match, so a
304 serves the stored body without spending rate limit; after a 403 or 429 with Retry-After, an
exhausted x-ratelimit-remaining or a secondary limit, makes no GitHub call until the wait passes,
serving stored reads and refusing the rest with 429; spaces writes 1 s apart. Socket:
${XDG_RUNTIME_DIR}/north-gh-broker/broker.sock. It exits after 30 idle minutes.";

const IDLE_EXIT: Duration = Duration::from_secs(30 * 60);

pub fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("serve") if arguments.len() == 1 => serve(),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn serve() -> u8 {
    let place = Place::from_env();
    let Some(_lock) = place.exclusive() else {
        return 0;
    };
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
    let watched = Arc::clone(&activity);
    let socket = place.socket();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(60));
            if watched.idle_for().is_some_and(|idle| idle >= IDLE_EXIT) {
                let _ = std::fs::remove_file(&socket);
                std::process::exit(0);
            }
        }
    });
    server::serve(&listener, &broker, &activity, &Arc::new(calls::CallLog::at(place.calls())));
    0
}

#[cfg(test)]
fn spawn_server(listener: UnixListener, broker: Broker) {
    let (broker, activity) = (Arc::new(broker), Arc::new(server::Activity::default()));
    std::thread::spawn(move || server::serve(&listener, &broker, &activity, &Arc::new(calls::CallLog::off())));
}

#[cfg(test)]
mod tests;
