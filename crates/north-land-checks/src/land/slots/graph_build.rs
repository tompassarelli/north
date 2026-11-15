//! `north land graph-build`: builds the whole Buck2 graph of the tree for a lane that changes a file defining a
//! target (north#1115), on the workers, and judges it by Buck2's exit status alone: 0 passes, 3 (the tree's own
//! targets or code failed) fails, any other Buck2 status says nothing of the tree. A tree whose Buck2 configuration is
//! not the base's gets no graph build and passes, as the lint gate leaves its line as declared. The tests of such a lane
//! stay on Cargo. With `--changed` and the packages the lane changed (`-p NAME` each), it builds those packages alone:
//! the compile a lane gets before it queues when its other checks run after the landing. On a host that holds no store
//! generation, that run goes to a build host through `north capacity offload run`, never compiling on this host, and
//! a build host with no store compiles the packages with `cargo check`.

use north_base::foundation::exit::NO_VERDICT;

const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
const TREE_ERROR: i32 = 3;
const FAILED: u8 = 1;

fn verdict(status: Option<i32>) -> u8 {
    match status {
        Some(0) => 0,
        Some(TREE_ERROR) => FAILED,
        _ => NO_VERDICT,
    }
}

/// The packages a `--changed` selection names (`-p NAME` each), or `None` for the whole workspace; `Err` for any other word.
fn changed(selection: &[String]) -> Result<Option<Vec<String>>, ()> {
    let mut names = Vec::new();
    let mut words = selection.iter();
    while let Some(word) = words.next() {
        match (word.as_str(), words.next()) {
            ("-p", Some(name)) => names.push(name.clone()),
            ("--workspace", None) if names.is_empty() => return Ok(None),
            _ => return Err(()),
        }
    }
    if names.is_empty() { Err(()) } else { Ok(Some(names)) }
}

const OFFLOAD_SECONDS: &str = "900";

/// Whether this host is a build host, whose policy entry sets `offload_builds`: where a compile may run.
fn builds_here() -> bool {
    let host = north_base::platform::host::name();
    north_base::platform::orchestration::read().ok().and_then(|policy| policy.get("hosts")?.get(host.as_str())?.get("offload_builds")?.as_bool()) == Some(true)
}

/// The exit of `command` when it exits, else no verdict: a build host's own answers are the verdicts of the check, and 75 or a signal says nothing of the tree.
fn judged(status: Option<i32>) -> u8 {
    match status {
        Some(0) => 0,
        Some(1) => FAILED,
        _ => NO_VERDICT,
    }
}

/// The compile of the changed packages on a host with no shared build store: on a build host, `cargo check` of them, the clippy line's compile without its lints; on any other host, the same `graph-build` run on a build host that holds a store, through `north capacity offload run`, so the compile never runs on this host; with no build host answering it reports no verdict, the lane is not queued uncompiled and the caller tries again (north#1631).
fn no_store_check(root: &std::path::Path, packages: Option<&[String]>) -> u8 {
    if !root.join("Cargo.toml").is_file() || !root.join("bin/land-shell").is_file() {
        eprintln!("north land graph-build: this tree has no Cargo workspace with bin/land-shell, so it has nothing to compile here");
        return 0;
    }
    if builds_here() {
        return cargo_check(root, packages);
    }
    let words: Vec<&str> = packages.map_or_else(|| vec!["--workspace"], |names| names.iter().flat_map(|name| ["-p", name.as_str()]).collect());
    let command = format!("north land graph-build --changed {}", words.join(" "));
    let north = std::env::var("NORTH_LAND_NORTH").ok().filter(|north| !north.is_empty()).unwrap_or_else(|| "north".to_owned());
    let chosen = std::process::Command::new(&north).args(["capacity", "offload", "choose"]).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().filter(|output| output.status.success());
    let host = chosen.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|host| !host.is_empty() && !host.contains(char::is_whitespace));
    let Some(host) = host else {
        eprintln!("north land graph-build: this host has no shared build store and no build host answers, so the compile has no verdict; it never runs on this host");
        return NO_VERDICT;
    };
    let head = std::env::var("SAFE_PUSH_LAND_HEAD").ok().filter(|head| !head.is_empty()).unwrap_or_else(|| "HEAD".to_owned());
    let base = std::env::var(BASE_ENV).unwrap_or_default();
    eprintln!("north land graph-build: this host has no shared build store, so {host} compiles: {command}");
    let status = std::process::Command::new(&north).current_dir(root).args(["capacity", "offload", "run", "--host", &host, "--base", &base, "--head", &head, "--timeout", OFFLOAD_SECONDS, "--", &command]).status().ok().and_then(|status| status.code());
    judged(status)
}

fn cargo_check(root: &std::path::Path, packages: Option<&[String]>) -> u8 {
    let selection: Vec<&str> = packages.map_or_else(|| vec!["--workspace"], |names| names.iter().flat_map(|name| ["-p", name.as_str()]).collect());
    eprintln!("north land graph-build: this host has no shared build store, so cargo checks {}", selection.join(" "));
    let status = std::process::Command::new("./bin/land-shell").current_dir(root).env("CARGO_TARGET_DIR", "target").args(["--dev", "--", "cargo", "check", "--all-targets", "--locked", "--offline"]).args(&selection).status().ok().and_then(|status| status.code());
    match status {
        Some(0) => 0,
        Some(101) => FAILED,
        _ => NO_VERDICT,
    }
}

pub fn run(arguments: &[String]) -> u8 {
    let selection = match arguments.split_first() {
        None => Ok(None),
        Some((flag, selection)) if flag == "--changed" => changed(selection),
        Some(_) => Err(()),
    };
    let Ok(selection) = selection else {
        eprintln!("usage: north land graph-build [--changed --workspace | --changed -p PACKAGE...]");
        return 64;
    };
    let base = std::env::var(BASE_ENV).ok().filter(|base| !base.is_empty());
    let Ok(root) = std::env::current_dir() else { return NO_VERDICT };
    let build = match north_build_system::graph_build(&root, base.as_deref(), selection.as_deref()) {
        Ok(build) => build,
        Err(_) if arguments.first().is_some_and(|flag| flag == "--changed") && !north_build_system::has_store() => return no_store_check(&root, selection.as_deref()),
        Err(why) => {
            eprintln!("north land graph-build: {why}, so this lane gets no graph build here");
            return 0;
        }
    };
    eprintln!("north land graph-build: {}", build.words.join(" "));
    let status = build.command().status().ok().and_then(|status| status.code());
    let verdict = verdict(status);
    if verdict == NO_VERDICT {
        eprintln!("north land graph-build: buck2 exited {}, which says nothing of the tree, so the check has no verdict", status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string()));
    }
    verdict
}
