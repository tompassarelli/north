mod bounded;
mod build_host;
mod cadence;
mod cargo_build;
mod canary;
pub mod checkouts;
mod deploy;
mod entry_links;
mod failure;
mod follow;
mod growth;
pub mod lifecycle;
mod health;
mod history;
mod host_facts;
pub mod lag;
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
mod verdict;
mod walk;
mod watch;
#[cfg(test)]
pub(crate) use update::collect;

use crate::land_offload;
use std::path::Path;

const USAGE: &str = "usage: north runtime update [--sha SHA] | north runtime settle | north runtime status | north runtime deploy [HOST...] | north runtime build SHA | north runtime rollback --from SHA | north runtime health | north runtime promote | north runtime mirror | north runtime checkouts | north runtime lag --processes|--json | north runtime build-ahead --tree TREE --commit COMMIT [--checkout DIR] | north runtime follow --repo-dir DIR | north runtime agents|claude|agy|pi|opencode [VERSION] | north runtime probe launch claude|opencode|pi | north runtime probe launch PROVIDER  one `--test` worker of PROVIDER (claude, opencode or pi) is asked for the heading of the shared policy it was given and answers it, then is forgotten; exit 0 when it answered; the canary runs it on each host
covers [--head REV] [--code PATH,...]

update [--sha SHA]     bring this host's current runtime to the pinned runtime (or SHA): on the host that settles the fleet, origin/main is promoted at six fixed slots a day (one every runtime_promotion_slot_sec from 00:00 UTC: the newest head whose runtime is ready, built once ten minutes before the slot, when commits since the pin change a runtime input) or at once for a landing whose commit carries a `Live-fix: REASON` trailer, which takes the next slot's place (one promotion at a time, a landing between slots waits for its slot, and the coordinator is paged once when a promotion that fell due is not live 5 min later; a sha rolled back after its canary failed stays quarantined); every other host runs only what the settling host ships it after the runtime walked the spine there; pull it from a peer, reuse the live binaries when no Rust build input changed, else build it on the build host it reaches with the most free cores and take its binaries, or build it here under a runtime capacity lease when no build host answers, saying so; swap back when health fails, and for 10 min after a swap roll back when session hosts stayed stopped (not ones restarted within a minute, nor `vn-*` scratch hosts) or land refusals ran faster than over the hour before, or launches failed more often for a cause the hour before never saw (only current moves, so running session hosts keep their runtime); an agents sync failure keeps the code, alerts the proxy and retries each tick
settle                 on the host that settles the fleet: update here, run the core-loop probes here and ship the live runtime to the hosts behind it only when it passed them, probe every host, run the core-loop probes (a sandboxed worker shell reaching the hub host, the hub fetching a repository, each lander minting its push credential, with a host with no running lander or a repository no running lander serves reported once as not applicable, `depbox run -- true` in a scratch checkout carrying a worker command's removal roots, so depbox execs the judged north inside its sandbox, one `--test` worker per provider the host can launch answering, with a provider it cannot launch reported once as not applicable) on each host per promoted runtime and roll a host back, alarming the proxy with the failing probe, when one fails, and judge the newest landing the pinned runtime holds once by landing_live_deadline_sec after its promotion
status                 print the pinned runtime, when it was promoted, whether it walked the spine here, since when landings that change a runtime input wait for a promotion, the promotions and fast paths in the last 24 h and the next slot, from runtime-pins.jsonl
deploy [HOST...]       build here, then ship and promote that runtime on each HOST (default: every other declared host with an ssh destination); a HOST whose live runtime descends from it keeps its own
build SHA              on a build host (offload_builds): build SHA, a commit on origin/main or a candidate a lander sent ahead of its landing under refs/build-ahead/SHA, into this host's runtimes under its runtime capacity lease for the settling host to take, without promoting it here
health                 check the live runtime
rollback --from SHA    relink current to the previous runtime and hold SHA back for an hour, when current is SHA; settle runs it on a host whose canary probes failed
promote                after a landing: bring this host to the pinned runtime, waiting up to 10 min for a promotion already running here and then taking origin/main's newest head; on the host that settles the fleet, run the core-loop probes on it here and ship it to every declared host only when it passed them; when this host runs the lander, also run north land promote against the new current so the lander link follows it, or log the named reason it is held
build-ahead --tree TREE --commit COMMIT [--checkout DIR]  have the build host with the most free cores build the release north for the candidate TREE from COMMIT (sent there under refs/build-ahead/COMMIT and built by its `north runtime build COMMIT`) and keep it under runtime-artifacts/TREE; this host never builds it, and with no build host answering nothing is kept and the promotion has one build it, so the promotion of the landing that carries TREE installs it instead of building it (artifact-ready 0 s in runtime-pins.jsonl); the lander asks for its candidate's tree here
follow --repo-dir DIR  after a landing: start promote in the background when DIR's origin is north
mirror                 on the host that settles the fleet: copy its lander and runtime records into north/records-mirror on every host it ships runtimes to, as each fleet tick does in the background
checkouts              on the host that settles the fleet: ask every host it ships runtimes to fast-forward its ~/code/<project>/main checkouts onto origin/main (south, then north, nixos-config, wisp and makgora), naming each move and each checkout a host could not move, as each fleet tick does in the background
agents [VERSION]       update this host's north runtime, then the Claude, Agy, Pi and OpenCode runtimes, as the twice-daily timer does
claude|agy|pi|opencode [VERSION]   install a checksum-verified Claude, Agy, Pi or OpenCode runtime and select it
lag --processes        list each long-lived north process on this host (landers, session hosts and links, watchdogs, brokers, the offload taker) with the runtime it runs, and `behind` with the runtime its link names for each that runs another one
lag --json             print an array with one object per long-lived north process on every declared host: host, process, runtime and `seconds` behind the newest landing that changed a runtime input (0 when it runs it; null for a host that did not answer); each settle samples the worst for the `ready-runtime-lag` bar and pages the proxy once per episode past 5 min
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
        [] => {
            let updated = update::update(layout, &mut || {});
            watch::watch(layout);
            Some(updated.map(|_| ()))
        }
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
        (command, [sha]) if command == "build" && layout::is_sha(sha) => Some(update::build_for_peer(layout, sha)),
        (command, [flag, sha]) if command == "rollback" && flag == "--from" && layout::is_sha(sha) => Some(update::roll_back(layout, sha).map(|_| ())),
        (command, []) if command == "health" => Some(health::health(layout)),
        (command, []) if command == "status" => {
            print!("{}{}", pin::status(&layout.state, layout.live_sha().as_deref()), cadence::status(&layout.state, pin::now()));
            Some(Ok(()))
        }
        (command, []) if command == "promote" => Some(follow::promote_detached(layout)),
        (command, flags) if command == "build-ahead" => Some(update::build_ahead(layout, flags)),
        (command, []) if command == "mirror" => Some(mirror::run()),
        (command, []) if command == "checkouts" => Some(checkouts::run()),
        (command, flags) if command == "lag" => lag::command(layout, flags),
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
