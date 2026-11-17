//! `north infra`: the live paid resources and what they cost a month. Hetzner Cloud servers come live from the
//! broker's `hcloud` allowlist (`GET /v1/servers`), and south's infra manifest adds the machines and paid services
//! no cloud API here lists, such as the Robot server and the Storage Box.
//! One unread source leaves the others standing.
use crate::platform::broker_client::Client;

mod hcloud;
pub(crate) mod manifest;
pub(crate) mod report;

pub(crate) struct Read {
    pub resources: Vec<report::Resource>,
    pub unread: Vec<String>,
}

pub(crate) fn read() -> (Vec<report::Resource>, Vec<String>) {
    let read = read_with_billing();
    (read.resources, read.unread)
}

pub(crate) fn read_with_billing() -> Read {
    let mut unread = Vec::new();
    let live = match Client::discover()
        .call("GET", hcloud::PATH, None)
        .and_then(|answer| hcloud::servers(&answer))
    {
        Ok(servers) => Some(servers),
        Err(why) => {
            unread.push(format!("hcloud: {why}"));
            None
        }
    };
    let billing = manifest::read();
    let parsed = match &billing {
        Ok(billing) => Some(billing),
        Err(why) => {
            unread.push(format!("manifest: {why}"));
            None
        }
    };
    let resources = match (live, parsed) {
        (Some(live), Some(billing)) => report::merge(live, billing),
        (Some(live), None) => live,
        (None, Some(billing)) => report::merge(Vec::new(), billing),
        (None, None) => Vec::new(),
    };
    Read { resources, unread }
}
