use super::super::peers::{newest, promoted_among};
use super::admitted;
use crate::ops::land_offload::pull_sources_in;
use std::cell::Cell;
use toml_edit::DocumentMut;

const HOSTS: &str = include_str!("../../../fixtures/orchestration-hosts.toml");
const NEWER: &str = "1111111111111111111111111111111111111111";

fn caught_up_from(cfg: &DocumentMut) -> Option<String> {
    let promoted = promoted_among(pull_sources_in(cfg, "nexus"), |host| (host == "tom@gex45").then(|| format!("runtimes/{NEWER}\n")));
    newest(promoted, |_, _| true).map(|peer| peer.host)
}

#[test]
fn nexus_catch_up_picks_no_source_when_the_newer_runtime_is_on_a_host_with_runtime_source_false() {
    let marked: DocumentMut = HOSTS.replace("[hosts.gex45]\n", "[hosts.gex45]\nruntime_source = false\n").parse().unwrap();
    assert_eq!(caught_up_from(&marked), None);
    assert_eq!(caught_up_from(&HOSTS.parse().unwrap()).as_deref(), Some("tom@gex45"));
}

#[test]
fn a_host_that_takes_no_peer_runtime_never_fetches_one_from_the_host_that_built_it() {
    let fetches = Cell::new(0);
    let counted = || {
        fetches.set(fetches.get() + 1);
        true
    };
    assert!(!admitted(false, "tom@gex45", counted));
    assert_eq!(fetches.get(), 0);
    assert!(admitted(true, "tom@gex45", counted));
    assert_eq!(fetches.get(), 1);
}
