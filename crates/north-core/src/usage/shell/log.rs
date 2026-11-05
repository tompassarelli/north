use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde_json::json;

use super::super::decide::world::{Request, Unix, World};

pub struct Entry<'a> {
    pub at: Unix,
    pub request: &'a Request,
    pub world: &'a World<'a>,
    pub in_flight: &'a [String],
    pub line: &'a str,
}

pub fn append(path: &Path, entry: &Entry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let record = json!({
        "at": entry.at.0,
        "world": digest(entry.world),
        "request": format!("{:?}", entry.request),
        "in_flight": entry.in_flight,
        "decision": entry.line,
    });
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    writeln!(file, "{record}").map_err(|error| format!("{}: {error}", path.display()))
}

fn digest(world: &World) -> serde_json::Value {
    let text = format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
        world.accounts,
        world.readings,
        world.limits,
        world.sessions,
        world.tripped,
        world.credit,
        world.settings
    );
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    json!({
        "hash": format!("{hash:016x}"),
        "accounts": world.accounts.len(),
        "readings": world.readings.len(),
        "limits": world.limits.len(),
        "sessions": world.sessions.len(),
    })
}
