mod bounded;
mod deploy;
mod failure;
mod follow;
mod health;
pub mod layout;
mod peers;
mod pull;
mod probe;
mod remote;
mod reuse;
mod settle;
mod store;
mod supersede;
mod update;

use crate::ops::land_offload;
use std::path::Path;

const USAGE: &str = "usage: north runtime update [--sha SHA] | north runtime settle | north runtime deploy [HOST...] | north runtime health | north runtime promote | north runtime follow --repo-dir DIR

update [--sha SHA]     bring this host's current runtime to origin/main (or SHA): pull it from a peer, reuse the live binaries when no Rust build input changed, else build it under a runtime capacity lease; swap back when health or agents sync fails
settle                 on the host that settles the fleet: update here, deploy where current differs, probe every host and judge each landing once by landing_live_deadline_sec
deploy [HOST...]       build here, then ship and promote that runtime on each HOST (default: every other declared host with an ssh destination)
health                 check the live runtime
promote                update here, then deploy to every declared host
follow --repo-dir DIR  after a landing: start promote in the background when DIR's origin is north";

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let layout = layout::Layout::from_env();
    let Some(result) = dispatch(&layout, arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("north runtime {}: {}", arguments[0], failure.message);
            failure.code
        }
    }
}

fn update_command(layout: &layout::Layout, flags: &[String]) -> Option<Result<(), failure::Failure>> {
    match flags {
        [] if land_offload::settles_fleet_here() => Some(settle::settle(layout)),
        [] => Some(update::update(layout)),
        [flag, sha] if flag == "--sha" && layout::is_sha(sha) => Some(update::update_to(layout, sha)),
        _ => None,
    }
}

fn dispatch(layout: &layout::Layout, arguments: &[String]) -> Option<Result<(), failure::Failure>> {
    match arguments.split_first()? {
        (command, flags) if command == "update" => update_command(layout, flags),
        (command, []) if command == "settle" => Some(settle::settle(layout)),
        (command, []) if command == "health" => Some(health::health(layout)),
        (command, []) if command == "promote" => Some(follow::promote(layout)),
        (command, [flag, repo]) if command == "follow" && flag == "--repo-dir" => Some(follow::follow(layout, Path::new(repo))),
        (command, hosts) if command == "deploy" => Some(match hosts.iter().find(|host| host.starts_with('-')) {
            Some(flag) => Err(failure::failed(64, format!("unknown flag {flag}\n{USAGE}"))),
            None => deploy::deploy(layout, hosts),
        }),
        _ => None,
    }
}
