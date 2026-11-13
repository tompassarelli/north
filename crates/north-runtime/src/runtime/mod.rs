mod bounded;
mod build_host;
mod cadence;
mod cargo_build;
mod canary;
pub mod checkouts;
mod deadline;
mod deploy;
mod failure;
mod follow;
mod order;
mod growth;
pub mod lifecycle;
mod health;
mod history;
mod host_facts;
pub mod lag;
pub use north_runtime_follow::{layout, own_runtime};
pub mod live;
mod peers;
pub mod pin;
mod pull;
mod north_processes;
mod probe;
mod remote;
pub mod mirror;
mod reuse;
pub(crate) mod review_accept;
pub(crate) mod review_hold;
mod after_landing;
mod settle;
mod store;
mod tree_build;
mod update;
mod verdict;
mod walk;
mod watch;
#[cfg(test)]
pub(crate) use canary::scratch_project_probes;

use north_offload::land_offload;
use std::path::Path;

const USAGE: &str = "usage: north runtime update [--sha SHA [--ordered]] | north runtime settle | north runtime status | north runtime deploy [HOST...] | north runtime build SHA | north runtime rollback --from SHA | north runtime health | north runtime promote | north runtime order --reason REASON [--commit REV] [--except HOST]... | north runtime hold COMMIT [--host HOST] --why TEXT | north runtime accept COMMIT | north runtime hold --lift COMMIT --why TEXT | north runtime mirror | north runtime checkouts | north runtime lag --processes|--json | north runtime build-ahead --tree TREE --commit COMMIT [--checkout DIR] | north runtime follow --repo-dir DIR | north runtime agents|claude|agy|pi|opencode [VERSION] | north runtime probe services | north runtime probe list HOST | north runtime walk | north runtime probe launch claude|opencode|pi | north runtime probe launch PROVIDER  one `--test` worker of PROVIDER (claude, opencode or pi) is asked for the heading of the shared policy it was given and answers it, then is forgotten; exit 0 when it answered; the canary runs it on each host
covers [--head REV] [--code PATH,...] | north runtime tree-build

update [--sha SHA [--ordered]]  bring this host's current runtime to the pinned runtime (or SHA, which moves no host back from a newer runtime unless --ordered says an order chose it): on the host that settles the fleet, origin/main is promoted at six fixed slots a day (one every runtime_promotion_slot_sec from 00:00 UTC: the newest head whose runtime is ready, built once ten minutes before the slot, when commits since the pin change a runtime input) or at once for a landing whose commit carries a `Live-fix: REASON` trailer, which takes the next slot's place (one promotion at a time, a landing between slots waits for its slot, and the coordinator is paged once when a promotion that fell due is not live 5 min later; a sha rolled back after its canary failed stays quarantined); every other host runs only what the settling host ships it after the runtime walked the spine there; pull it from a peer, reuse the live binaries when no Rust build input changed, else build it on the build host it reaches with the most free cores and take its binaries, or build it here under a runtime capacity lease when no build host answers, saying so; swap back when health fails, and for 10 min after a swap roll back when session hosts died with nobody asking and stayed down (not ones their host parked at idle, ones restarted within a minute, nor `vn-*` scratch hosts) or land refusals ran faster than over the hour before, or launches failed more often for a cause the hour before never saw, and in a window a rollback opened only report it once (only current moves, so running session hosts keep their runtime); an agents sync failure keeps the code, alerts the proxy and retries each tick
settle                 on the host that settles the fleet: update here, run the core-loop probes here and ship the live runtime to the hosts behind it only when it passed them, probe every host, run the core-loop probes (a sandboxed worker shell held to the sandbox's rules and, where sessions reach the hub host, one reaching it through `north capacity offload`, both served by a broker the judged runtime starts, the hub fetching a repository, each lander minting its push credential, with a host with no running lander or a repository no running lander serves reported once as not applicable, `depbox run -- true` in a scratch checkout carrying a worker command's removal roots, so depbox execs the judged north inside its sandbox, one `--test` worker per provider the host can launch answering, with a provider it cannot launch, and every provider on a host placement starts no fleet session on, reported once as not applicable) on each host per promoted runtime and roll a host back, alarming the proxy with the failing probe, when one fails, and judge the newest landing the pinned runtime holds once by landing_live_deadline_sec after its promotion
status                 print the pinned runtime, when it was promoted, whether it walked the spine here, since when landings that change a runtime input wait for a promotion, the promotions and fast paths in the last 24 h, the last slot with its time and outcome (promoted, refused with its reason, or failed with its probe), a line saying no slot promotes until the hand-order pin is released while one is held, and the next slot, from runtime-pins.jsonl
deploy [HOST...]       build here, then ship and promote that runtime on each HOST (default: every other declared host with an ssh destination); a HOST whose live runtime descends from it keeps its own
build SHA              on a build host (offload_builds): build SHA, a commit on origin/main or a candidate a lander sent ahead of its landing under refs/build-ahead/SHA, into this host's runtimes under its runtime capacity lease for the settling host to take, without promoting it here
health                 check the live runtime
rollback --from SHA    relink current to the previous runtime and hold SHA back for an hour, when current is SHA; settle runs it on a host whose canary probes failed
promote                after a landing: bring this host to the pinned runtime, waiting up to 10 min for a promotion already running here and then taking origin/main's newest head; on the host that settles the fleet, have the promoted runtime run its own core-loop probes here (north runtime walk) and ship it to every declared host only when it passed them; when this host runs the lander, also run north land promote against the new current so the lander link follows it, or log the named reason it is held
order --reason REASON [--commit REV] [--except HOST]...   the urgent route: promote REV (default origin/main's head), a commit already on origin/main, to this host and every deploy host at once, as the coordinator's step on the host that settles the fleet: each host builds or fetches it, runs its canary probes against it before it is live, goes live only on a pass, rewrites its stable pin, and one runtime-pins.jsonl row per host records the commit, who ordered it, the reason and the canary verdict; a failed canary leaves that host on its old runtime and names the probe; every host is moved to REV even back from a newer runtime, and one that stays on another runtime is named and fails the order; a promotion that starts while an order runs here leaves it the promotion lock; --except HOST leaves a deploy host out: nothing is shipped to it, no canary runs there, one left-out row names who and why, and status and lag name it held until an order includes it again; this host, a name that is no deploy host and an order that leaves no deploy host are refused; north-stable-hold.service is stopped for the run and started again after; a head that contains a commit a reviewer holds is refused before anything is built, naming the commit and the reviewer, and a hold on it lifted within the last day is named with who lifted it and why
hold COMMIT [--host HOST] --why TEXT   on the host that settles the fleet: place the calling fleet session's hold on COMMIT (resolved in the working directory), read from the session its cgroup scope names, through the outside broker inside a worker sandbox; while it stands `north runtime order` refuses any head containing COMMIT (with --host, only an order that promotes HOST) and the slot and Live-fix promotions refuse it whatever host it names, each naming the commit and the reviewer; the switch review_holds turns every such refusal off
accept COMMIT        on the host that settles the fleet: record the calling fleet session's accept of the patch of COMMIT (`git patch-id --stable` of its merge-base with origin/main to COMMIT, resolved in the working directory), read from the session its cgroup scope names, through the outside broker inside a worker sandbox; a lane that changes a file of trust_boundary_paths lands only with the accepts of two distinct sessions, or one for a test-only change or an exact revert (the land scan review-accept, in shadow)
hold --lift COMMIT --why TEXT   lift the holds on COMMIT: the session that placed a hold lifts its own and the coordinator (a session whose record names the role proxy) lifts any; any other session is refused, naming who holds it; the record keeps who lifted it and why
build-ahead --tree TREE --commit COMMIT [--checkout DIR]  have a build host with no build ahead running build the release north for the candidate TREE, one request per build host at a time, from COMMIT (sent there under refs/build-ahead/COMMIT and built by its `north runtime build COMMIT`) and keep it under runtime-artifacts/TREE; this host never builds it, and with no build host answering nothing is kept and the promotion has one build it, so the promotion of the landing that carries TREE installs it instead of building it (artifact-ready 0 s in runtime-pins.jsonl); the lander asks for its candidate's tree here
follow --repo-dir DIR  after a landing: start promote in the background when DIR's origin is north
mirror                 on the host that settles the fleet: copy its lander and runtime records into north/records-mirror on every host it ships runtimes to, as each fleet tick does in the background
checkouts              on the host that settles the fleet: ask every host it ships runtimes to fast-forward its ~/code/<project>/main checkouts onto origin/main (south, then north, nixos-config, wisp and makgora), naming each move and each checkout a host could not move, as each fleet tick does in the background
agents [VERSION]       update this host's north runtime, then the Claude, Agy, Pi and OpenCode runtimes, as the twice-daily timer does
claude|agy|pi|opencode [VERSION]   install a checksum-verified Claude, Agy, Pi or OpenCode runtime and select it
probe list HOST        print, as one line of JSON, the core-loop probes this north's runtime judges itself by on HOST (this host by its name, another by its ssh destination); a promotion and an order ask the runtime they judge for it, so a probe that runtime retired no longer judges it, and the order prints the probes added, removed and changed in their script against the list of the runtime live on the host
walk                   on the host that settles the fleet: run the core-loop probes on the runtime live here unless it is judged already, ship it to every declared host once it passed and move the lander link after it; north runtime promote leaves this step after its swap to the runtime it promoted
probe services         exit 0 when no host service here (a broker, a lander, a watchdog, the offload taker) runs a runtime older than the one this north's runtime replaces, 65 naming each that does, since it follows no promotion; a session process behind is counted, not failed; an on-demand one (the user broker, a GitHub broker no unit keeps) on a runtime older than the live one is first asked to exit once it serves no client, as a promotion asks again when its runtime is live, and one still serving a client that no new client reaches is named and not failed; the canary runs it on each host
lag --processes        list each long-lived north process on this host (landers, session hosts and links, watchdogs, brokers, the offload taker) with the runtime it runs, and `behind` with the runtime its link names for each that runs another one
lag --json             print an array with one object per long-lived north process on every declared host: host, process, runtime and `seconds` older than the runtime the settling host pinned (0 when it runs the pin or a runtime built on it; null for a host that did not answer); each fleet tick on the settling host samples the worst for the `ready-runtime-lag` bar, at most once a minute whatever the stable pin, and pages the proxy once per session episode past 5 min and once per host service (broker, lander, watchdog, offload taker) each time it starts to lag past 5 min, by name
covers [--head REV] [--code PATH,...]
                       print the live runtime's north when REV (default HEAD) in this checkout changes no Rust build input since it, or with --code none of those path prefixes nor rust-toolchain.toml nor any package Cargo.lock held at the live runtime (a lock that only adds packages still covers), so a land check runs it instead of building north from the tree
tree-build             build north from the checkout it runs in, with Buck2 on a host set to buck2_tree_build = true for a tree whose Buck2 configuration and targets are its base's and Cargo otherwise or when Buck2 fails for a reason other than the tree, and print the binary's path; exit 1 when the tree does not build and 75 when the build failed for another reason; bin/head-north runs it when no built runtime covers the tree";

pub fn stage_on(host: &str, sha: &str, runtime: &Path) -> Result<(), String> {
    deploy::stage_runtime(host, sha, runtime).map_err(|failure| failure.message)
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
    if let Err(refused) = update::agent_link::guard() {
        return Some(Err(refused));
    }
    match flags {
        [] if land_offload::settles_fleet_here() => Some(settle::settle(layout)),
        [] => {
            let updated = update::update(layout, &mut || {});
            watch::watch(layout);
            Some(updated.map(|_| ()))
        }
        [flag, sha] if flag == "--sha" && layout::is_sha(sha) => Some(update::update_to(layout, sha)),
        [flag, sha, ordered] if flag == "--sha" && layout::is_sha(sha) && ordered == "--ordered" => Some(update::update_ordered_to(layout, sha)),
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
        (command, flags) if command == "order" => Some(order::command(layout, flags)),
        (command, flags) if command == "build-ahead" => Some(update::build_ahead(layout, flags)),
        (command, []) if command == "mirror" => Some(mirror::run()),
        (command, []) if command == "checkouts" => Some(checkouts::run()),
        (command, flags) if command == "lag" => lag::command(layout, flags),
        (command, flags) if command == "covers" => covers(layout, flags),
        (command, []) if command == "tree-build" => Some(tree_build::run()),
        (command, [verb, provider]) if command == "probe" && verb == "launch" => Some(canary::launch_probe(provider)),
        (command, [verb]) if command == "probe" && verb == "services" => Some(canary::services_probe(layout)),
        (command, [verb, host]) if command == "probe" && verb == "list" => Some(canary::list_probes(layout, host)),
        (command, []) if command == "walk" => Some(follow::walk(layout)),
        (command, [flag, repo]) if command == "follow" && flag == "--repo-dir" => Some(follow::follow(layout, Path::new(repo))),
        (command, hosts) if command == "deploy" => Some(match hosts.iter().find(|host| host.starts_with('-')) {
            Some(flag) => Err(failure::failed(64, format!("unknown flag {flag}\n{USAGE}"))),
            None => deploy::deploy(layout, hosts),
        }),
        _ => None,
    }
}
