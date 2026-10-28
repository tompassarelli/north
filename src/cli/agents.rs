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
        Verb::Command(_) => report::run(rest),
        Verb::Script("usage.py") if crate::usage::handles(rest) => usage(rest),
        Verb::Script(file) => with_harness(|root| script::run(root, file, rest)),
    }
}

pub(super) fn usage(arguments: &[String]) -> u8 {
    crate::usage::run(arguments, &crate::threads::client::refresh_mirrored_snapshot)
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
