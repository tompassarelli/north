mod bounded;
mod cargo_build;
mod canary;
mod deploy;
mod entry_links;
mod failure;
mod follow;
mod lifecycle;
mod health;
mod history;
mod host_facts;
pub mod layout;
pub mod live;
mod peers;
mod pull;
mod probe;
mod remote;
pub mod mirror;
mod reuse;
mod settle;
mod store;
mod update;
#[cfg(test)]
pub(crate) use update::collect;

use crate::land_offload;
use std::path::Path;

const USAGE: &str = "usage: north runtime update [--sha SHA] | north runtime settle | north runtime deploy [HOST...] | north runtime rollback --from SHA | north runtime health | north runtime promote | north runtime mirror | north runtime follow --repo-dir DIR | north runtime agents|claude|agy [VERSION] | north runtime covers [--head REV] [--code PATH,...]

update [--sha SHA]     bring this host's current runtime to origin/main (or SHA): pull it from a peer, reuse the live binaries when no Rust build input changed, else build it under a runtime capacity lease; swap back when health fails; an agents sync failure keeps the code, alerts the proxy and retries each tick
settle                 on the host that settles the fleet: update here, deploy where current differs, probe every host, run one offloaded canary land check per promoted runtime per host and roll a host back when it fails, and judge each landing once by landing_live_deadline_sec
deploy [HOST...]       build here, then ship and promote that runtime on each HOST (default: every other declared host with an ssh destination); a HOST whose live runtime descends from it keeps its own
health                 check the live runtime
rollback --from SHA    relink current to the previous runtime and hold SHA back for an hour, when current is SHA; settle runs it on a host whose canary land check failed
promote                after a landing: wait for any running update, bring this host to origin/main in one pass, then ship the runtime now live here to every declared host; when this host runs the lander, also run north land promote against the new current so the lander link follows it, or log the named reason it is held
follow --repo-dir DIR  after a landing: start promote in the background when DIR's origin is north
mirror                 on the host that settles the fleet: copy its lander and runtime records into north/records-mirror on every host it ships runtimes to, as each fleet tick does in the background
agents [VERSION]       update this host's north runtime, then the Claude and Agy runtimes, as the twice-daily timer does
claude|agy [VERSION]   install a checksum-verified Claude or Agy runtime and select it
covers [--head REV] [--code PATH,...]
                       print the live runtime's north when REV (default HEAD) in this checkout changes no Rust build input since it, or with --code none of those path prefixes nor rust-toolchain.toml nor any package Cargo.lock held at the live runtime (a lock that only adds packages still covers), so a land check runs it instead of building north from the tree";

pub fn stage_on(host: &str, sha: &str, runtime: &Path) -> Result<(), String> {
    deploy::stage_runtime(host, sha, runtime).map_err(|failure| failure.message)
}

pub fn own_runtime() -> Option<(String, std::path::PathBuf)> {
    let executable = std::fs::canonicalize(std::env::current_exe().ok()?).ok()?;
    let directory = executable.parent()?;
    let sha = directory.file_name()?.to_str()?;
    (layout::is_sha(sha) && layout::has_programs(directory)).then(|| (sha.to_owned(), directory.to_path_buf()))
}

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

fn covers(layout: &layout::Layout, flags: &[String]) -> Option<Result<(), failure::Failure>> {
    let (mut head, mut code) = ("HEAD", Vec::new());
    let mut rest = flags.iter();
    while let Some(flag) = rest.next() {
        match (flag.as_str(), rest.next()) {
            ("--head", Some(value)) => head = value.as_str(),
            ("--code", Some(value)) => code.extend(value.split(',').filter(|path| !path.is_empty()).map(str::to_owned)),
            _ => return None,
        }
    }
    Some(reuse::covers(layout, Path::new("."), head, &code).map(|north| println!("{}", north.display())))
}

fn dispatch(layout: &layout::Layout, arguments: &[String]) -> Option<Result<(), failure::Failure>> {
    match arguments.split_first()? {
        (command, flags) if command == "update" => update_command(layout, flags),
        (command, []) if command == "settle" => Some(settle::settle(layout)),
        (command, [flag, sha]) if command == "rollback" && flag == "--from" && layout::is_sha(sha) => Some(update::roll_back(layout, sha).map(|_| ())),
        (command, []) if command == "health" => Some(health::health(layout)),
        (command, []) if command == "promote" => Some(follow::promote(layout)),
        (command, []) if command == "mirror" => Some(mirror::run()),
        (command, flags) if command == "covers" => covers(layout, flags),
        (command, [flag, repo]) if command == "follow" && flag == "--repo-dir" => Some(follow::follow(layout, Path::new(repo))),
        (command, hosts) if command == "deploy" => Some(match hosts.iter().find(|host| host.starts_with('-')) {
            Some(flag) => Err(failure::failed(64, format!("unknown flag {flag}\n{USAGE}"))),
            None => deploy::deploy(layout, hosts),
        }),
        _ => None,
    }
}
