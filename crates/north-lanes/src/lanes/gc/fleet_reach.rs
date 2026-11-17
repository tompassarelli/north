use std::io::Read;

use serde_json::Value;

use north_base::platform::{host, orchestration};

pub(super) fn on_registry_host(home: &std::path::Path) -> bool {
    orchestration::owners_machine(&orchestration::path_in(home, None, None)).is_some_and(|name| name == host::name())
}

pub(super) fn supplied(wanted: bool, known: Option<Vec<Value>>) -> Result<Option<Vec<Value>>, String> {
    if wanted && known.is_none() { sessions_from_stdin().map(Some).map_err(|error| format!("sessions on stdin: {error}")) } else { Ok(known) }
}

pub(super) fn refuses(sessions_known: bool, home: &std::path::Path) -> bool {
    let refused = !sessions_known && !on_registry_host(home);
    if refused {
        println!("lane gc: this host does not hold the fleet registry and the laptop passed no session list; kept everything");
    }
    refused
}

fn sessions_from_stdin() -> Result<Vec<Value>, String> {
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).map_err(|error| error.to_string())?;
    match serde_json::from_str(&text).map_err(|error| error.to_string())? {
        Value::Array(sessions) => Ok(sessions),
        _ => Err("session list on stdin is not an array".into()),
    }
}

pub(super) fn sweep_build_hosts() {
    println!("lane gc: no fleet session source, so no build host was swept");
}
