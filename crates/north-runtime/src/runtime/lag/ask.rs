//! Each declared host's long-lived processes and the runtime the settling host pinned, as this host can read them.
use super::behind::{Process, Seen};
use north_offload::land_offload;
use north_base::platform::host;
use crate::runtime::layout::Layout;
use crate::runtime::pin::{self, Event};
use crate::runtime::live::Swap;
use crate::runtime::probe::{self, Shell};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

const ASK: &str = "state=\"${XDG_STATE_HOME:-$HOME/.local/state}/north\"\nreadlink \"$state/current\" || echo\nstat -c %Y \"$state/current\" || echo\n\"$state/current/north\" runtime lag --processes --json\n";
const WHY_SHOWN: usize = 160;

pub(super) fn seen(layout: &Layout, probed: &[Value]) -> Vec<Seen> {
    let here = Seen { host: host::name(), swap: here_swap(&layout.state), processes: Ok(super::processes(Path::new("/proc"), &layout.state)) };
    let others = std::thread::scope(|scope| {
        let asked: Vec<_> = land_offload::other_destinations().into_iter().map(|destination| scope.spawn(move || remote(destination, probed))).collect();
        asked.into_iter().filter_map(|asked| asked.join().ok()).collect::<Vec<_>>()
    });
    std::iter::once(here).chain(others).collect()
}

fn remote(destination: String, probed: &[Value]) -> Seen {
    let unreachable = probed.iter().find(|record| record["host"] == destination.as_str()).and_then(|record| record["unreachable"].as_str());
    if let Some(why) = unreachable {
        return Seen { host: destination, swap: None, processes: Err(why.chars().take(WHY_SHOWN).collect()) };
    }
    let said = Shell::Ssh(destination.clone()).run(ASK);
    let (swap, processes) = match &said {
        Ok(text) => {
            let mut lines = text.splitn(3, '\n');
            let swap = probe::host_swap(lines.next().unwrap_or_default(), lines.next().unwrap_or_default());
            (swap, serde_json::from_str::<Vec<Process>>(lines.next().unwrap_or_default().trim()).map_err(|error| format!("answered no process list: {error}")))
        }
        Err(why) => (None, Err(why.clone())),
    };
    Seen { host: destination, swap, processes: processes.map_err(|why| why.chars().take(WHY_SHOWN).collect()) }
}

/// The runtime the settling host pinned, once a promotion made it live.
pub(super) fn pin(layout: &Layout) -> Option<String> {
    let sha = layout.live_sha()?;
    let promoted = pin::events_here_or_mirrored(&layout.state, "north runtime lag").iter().any(|event| matches!(event, Event::Promoted { sha: promoted, .. } if *promoted == sha));
    promoted.then_some(sha)
}

fn here_swap(state: &Path) -> Option<Swap> {
    let link = state.join("current");
    let target = fs::read_link(&link).ok()?;
    let at = fs::symlink_metadata(&link).and_then(|metadata| metadata.modified()).ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    probe::host_swap(&target.to_string_lossy(), &at.to_string())
}
