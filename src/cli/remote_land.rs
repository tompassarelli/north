use toml_edit::{DocumentMut, Item};

use crate::pacing::placement;
use north_safe_push::entry::{self, HostFacts};

pub fn run(arguments: &[String]) -> u8 {
    entry::remote_land(arguments, &host_facts())
}

pub fn safe_push(arguments: &[String]) -> u8 {
    entry::safe_push(arguments, &host_facts())
}

pub fn ledger_client(config: &DocumentMut, here: &str) -> bool {
    config
        .get("hosts")
        .and_then(Item::as_table_like)
        .and_then(|hosts| hosts.get(here))
        .and_then(|host| host.get("ledger_client"))
        .and_then(Item::as_bool)
        .unwrap_or(false)
}

fn host_facts() -> HostFacts {
    HostFacts { ledger_client: placement::config().is_ok_and(|config| ledger_client(&config, &crate::platform::host::name())) }
}
