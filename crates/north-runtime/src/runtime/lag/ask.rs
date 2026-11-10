//! Each declared host's long-lived processes and the landings that changed a runtime input, as this host can read them.
use super::behind::{Landing, Process, Seen};
use crate::land_offload;
use crate::platform::host;
use crate::runtime::layout::Layout;
use crate::runtime::pin::{self, Event};
use crate::runtime::probe::Shell;
use serde_json::Value;
use std::path::Path;

const ASK: &str = "\"${XDG_STATE_HOME:-$HOME/.local/state}/north/current/north\" runtime lag --processes --json\n";
const WHY_SHOWN: usize = 160;

pub(super) fn seen(layout: &Layout, probed: &[Value]) -> Vec<Seen> {
    let here = Seen { host: host::name(), processes: Ok(super::processes(Path::new("/proc"), &layout.state)) };
    let others = std::thread::scope(|scope| {
        let asked: Vec<_> = land_offload::other_destinations().into_iter().map(|destination| scope.spawn(move || remote(destination, probed))).collect();
        asked.into_iter().filter_map(|asked| asked.join().ok()).collect::<Vec<_>>()
    });
    std::iter::once(here).chain(others).collect()
}

fn remote(destination: String, probed: &[Value]) -> Seen {
    let unreachable = probed.iter().find(|record| record["host"] == destination.as_str()).and_then(|record| record["unreachable"].as_str());
    if let Some(why) = unreachable {
        return Seen { host: destination, processes: Err(why.chars().take(WHY_SHOWN).collect()) };
    }
    let processes = Shell::Ssh(destination.clone()).run(ASK).and_then(|said| serde_json::from_str::<Vec<Process>>(said.trim()).map_err(|error| format!("answered no process list: {error}")));
    Seen { host: destination, processes: processes.map_err(|why| why.chars().take(WHY_SHOWN).collect()) }
}

pub(super) fn landings(layout: &Layout) -> Vec<Landing> {
    pin::events_here_or_mirrored(&layout.state, "north runtime lag")
        .into_iter()
        .filter_map(|event| match event {
            Event::Landed { sha, at } => Some(Landing { at, sha }),
            _ => None,
        })
        .collect()
}
