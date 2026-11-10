mod bounded;
mod cargo_build;
mod canary;
mod deploy;
mod entry_links;
mod failure;
mod follow;
mod growth;
mod lifecycle;
mod health;
mod history;
mod host_facts;
pub mod layout;
pub mod live;
mod peers;
pub mod pin;
mod pull;
mod probe;
mod remote;
pub mod mirror;
mod reuse;
mod settle;
mod store;
mod update;
mod walk;
#[cfg(test)]
pub(crate) use update::collect;

use crate::land_offload;
use std::path::Path;

const USAGE: &str = "usage: north runtime update [--sha SHA] | north runtime settle | north runtime status | north runtime deploy [HOST...] | north runtime rollback --from SHA | north runtime health | north runtime promote | north runtime mirror | north runtime follow --repo-dir DIR | north runtime agents|claude|pi|opencode [VERSION] | north runtime probe launch claude|opencode|pi | north runtime probe launch PROVIDER  one `--test` worker of PROVIDER (claude, opencode or pi) is asked for the heading of the shared policy it was given and answers it, then is forgotten; exit 0 when it answered; the canary runs it on each host
covers [--head REV] [--code PATH,...]

update [--sha SHA]     bring this host's current runtime to the pinned runtime (or SHA): on the host that settles the fleet, origin/main is promoted at most 6 times a day, 4 h apart, or at once when the commits since the pin carry a Main-red: trailer; every other host runs only what the settling host ships it after the runtime walked the spine there; pull it from a peer, reuse the live binaries when no Rust build input changed, else build it under a runtime capacity lease; swap back when health fails; an agents sync failure keeps the code, alerts the proxy and retries each tick
settle                 on the host that settles the fleet: update here, run the core-loop probes here and ship the live runtime to the hosts behind it only when it passed them, probe every host, run the core-loop probes (a sandboxed worker shell reaching the hub host, the hub fetching a repository, each lander minting its push credential, with a host with no running lander or a repository no running lander serves reported once as not applicable, one `--test` worker per provider the host can launch answering, with a provider it cannot launch reported once as not applicable) on each host per promoted runtime and roll a host back, alarming the proxy with the failing probe, when one fails, and judge the newest landing the pinned runtime holds once by landing_live_deadline_sec after its promotion
status                 print the pinned runtime, when and why it was promoted, whether it walked the spine here, the promotions in the last 24 h and since when origin/main has waited for the next one, from runtime-pins.jsonl
deploy [HOST...]       build here, then ship and promote that runtime on each HOST (default: every other declared host with an ssh destination); a HOST whose live runtime descends from it keeps its own
health                 check the live runtime
rollback --from SHA    relink current to the previous runtime and hold SHA back for an hour, when current is SHA; settle runs it on a host whose canary probes failed
promote                after a landing: wait for any running update, bring this host to the pinned runtime in one pass; on the host that settles the fleet, run the core-loop probes on it here and ship it to every declared host only when it passed them; when this host runs the lander, also run north land promote against the new current so the lander link follows it, or log the named reason it is held
follow --repo-dir DIR  after a landing: start promote in the background when DIR's origin is north
mirror                 on the host that settles the fleet: copy its lander and runtime records into north/records-mirror on every host it ships runtimes to, as each fleet tick does in the background
agents [VERSION]       update this host's north runtime, then the Claude, Pi and OpenCode runtimes, as the twice-daily timer does
claude|pi|opencode [VERSION]   install a checksum-verified Claude, Pi or OpenCode runtime and select it
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
        (command, []) if command == "status" => {
            print!("{}", pin::status(&layout.state, layout.live_sha().as_deref()));
            Some(Ok(()))
        }
        (command, []) if command == "promote" => Some(follow::promote_detached(layout)),
        (command, []) if command == "mirror" => Some(mirror::run()),
        (command, flags) if command == "covers" => covers(layout, flags),
        (command, [verb, provider]) if command == "probe" && verb == "launch" => Some(canary::launch_probe(provider)),
        (command, [flag, repo]) if command == "follow" && flag == "--repo-dir" => Some(follow::follow(layout, Path::new(repo))),
        (command, hosts) if command == "deploy" => Some(match hosts.iter().find(|host| host.starts_with('-')) {
            Some(flag) => Err(failure::failed(64, format!("unknown flag {flag}\n{USAGE}"))),
            None => deploy::deploy(layout, hosts),
        }),
        _ => None,
    }
}
