use std::path::Path;

use crate::agent_catalog::{self, sync_gate};
use crate::error::{NorthError, NorthResult};
use crate::ops::agents::{self, Verb, plan, script};
use crate::ops::{flow, pacing, report};
use crate::platform::harness;

use super::{fleet, fleet_placement, token_usage};

pub(super) fn run(arguments: &[String]) -> u8 {
    let Some(verb) = arguments.first().and_then(|name| Verb::named(name)) else {
        eprintln!("{}", agents::USAGE);
        return 2;
    };
    let rest = &arguments[1..];
    match verb {
        Verb::Catalog => exit(agent_catalog::run(arguments).map(|()| 0)),
        Verb::Sync => exit(sync_gate::run(arguments)),
        Verb::Pace => pace(&[vec!["sample".to_owned()], rest.to_vec()].concat()),
        Verb::Busy => agents::busy(rest),
        Verb::Plan => with_harness(|root| {
            plan::run(
                rest,
                &plan::Ports {
                    harness: root,
                    operator: &sync_gate::operator_directory,
                    place: &pace,
                },
            )
        }),
        Verb::Command("flow") => flow::run(rest),
        Verb::Command("news") => crate::ops::model_watch::run(&[vec!["news".to_owned()], rest.to_vec()].concat()),
        Verb::Command(_) => report::run(rest),
        Verb::Script("usage.py") => usage(rest),
        Verb::Script(file) => with_harness(|root| script::run(root, file, rest)),
    }
}

pub(super) fn usage(arguments: &[String]) -> u8 {
    if let Some(provider) = gate_provider(arguments) {
        return pace(&["gate".to_owned(), provider]);
    }
    if crate::usage::handles(arguments) {
        return with_usage_host(|host| crate::usage::run(arguments, host));
    }
    with_harness(|root| script::run(root, "usage.py", arguments))
}

fn with_usage_host(work: impl FnOnce(&crate::usage::Host) -> u8) -> u8 {
    let env: std::collections::BTreeMap<String, String> = std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect();
    let ssh = move |arguments: &[&str]| crate::platform::ssh::argv(&["ssh".into()], &env, arguments);
    work(&crate::usage::Host {
        name: crate::platform::host::name(),
        refresh_snapshot: &crate::threads::client::refresh_mirrored_snapshot,
        ingest: &crate::threads::client::ingest,
        run: &|command, limit| crate::platform::process::run_command_within(command, None, limit),
        ssh: &ssh,
        harness_lib: &|| Ok(crate::platform::harness::harness_root()?.join("harness/lib")),
    })
}

fn gate_provider(arguments: &[String]) -> Option<String> {
    let precedes = arguments.iter().any(|argument| matches!(argument.as_str(), "--reservation-json" | "--write-pace-gate"));
    let at = arguments.iter().position(|argument| argument == "--gate")?;
    arguments.get(at + 1).filter(|_| !precedes).cloned()
}

fn exit(outcome: NorthResult<u8>) -> u8 {
    outcome.unwrap_or_else(|error| {
        eprintln!("error: {}", error.user_message());
        if matches!(error, NorthError::Usage(_)) {
            2
        } else {
            1
        }
    })
}

fn pace(arguments: &[String]) -> u8 {
    pacing::run(
        arguments,
        pacing::FleetFacts {
            roster: fleet::roster,
            machine_load: fleet_placement::fleet_load,
        },
        token_usage::read,
    )
}

fn with_harness(work: impl FnOnce(&Path) -> u8) -> u8 {
    match harness::harness_root() {
        Ok(root) => work(&root),
        Err(skew) => {
            eprintln!("{skew}");
            1
        }
    }
}
