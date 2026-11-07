use std::path::Path;

use super::findings;
use crate::cli::{tools, verb_help};
use crate::landing::host_calls::{self, HostCall};

const UNITS: &str = "# /etc/systemd/user/pace-enforcement.service
[Unit]
Description=pace-enforcement: sample each account's window
[Service]
ExecStart=/home/tom/.local/state/north/current/tree/bin/north pace sample
# /etc/systemd/user/fleet-tick.service
[Service]
ExecStart=/nix/store/b-bash/bin/bash /home/tom/.local/state/north/current/tree/bin/north job-run --user tom fleet-tick -- /home/tom/.local/state/north/current/tree/bin/north fleet tick
# /etc/systemd/user/host-gh-token.service
[Service]
ExecStart=/home/tom/.local/state/north/current/tree/bin/north job-run --user tom host-gh-token -- /nix/store/t-host-gh-token/bin/host-gh-token
";

const TOKEN_SCRIPT: &str = "#!/bin/sh\n# north gh-app-token was folded\ntoken=$(XDG_RUNTIME_DIR=$runtime /home/tom/.local/state/north/current/tree/bin/north ops github token)\n";

fn host() -> Vec<HostCall> {
    let mut calls = host_calls::unit_calls(UNITS, |path| (path == Path::new("/nix/store/t-host-gh-token/bin/host-gh-token")).then(|| TOKEN_SCRIPT.to_owned()));
    calls.extend(host_calls::tree_calls(Path::new(env!("CARGO_MANIFEST_DIR"))));
    calls
}

fn checked(reference: &str) -> Vec<String> {
    findings(&host(), |words| verb_help::resolve(reference, verb_help::reference(), words, tools::defines))
}

#[test]
fn a_runtime_missing_a_command_a_host_unit_or_its_own_ssh_hop_still_calls_is_refused_naming_where_it_is_called() {
    assert_eq!(
        host_calls::unit_names("north-fleet-north-259-d1824-c58f4e7f.service transient -\npace-enforcement.service linked enabled\n"),
        ["pace-enforcement.service"]
    );

    let current = checked(verb_help::reference());
    assert_eq!(current, ["/etc/systemd/user/pace-enforcement.service:4: north pace sample: this runtime has no north pace command; did you mean: north fleet pace sample"]);

    let without_observe: String = verb_help::reference().lines().filter(|line| !line.trim_start().starts_with("fleet laptop observe")).map(|line| format!("{line}\n")).collect();
    let dropped = checked(&without_observe);
    assert!(dropped.iter().any(|line| line.starts_with("crates/north-pacing/src/laptop/ctx.rs:") && line.contains("relays north fleet laptop observe from crates/north-capacity/src/capacity/laptop_auto.rs:")), "{dropped:#?}");
    assert!(dropped.iter().any(|line| line.contains("this runtime has no north fleet laptop observe; north fleet laptop takes open, close")), "{dropped:#?}");
}
