//! `north infra`: the live paid resources and what they cost a month. Hetzner Cloud servers come live from the
//! broker's `hcloud` allowlist (`GET /v1/servers`), and south's infra manifest adds the machines and paid services
//! no cloud API here lists, such as the Robot server and the Storage Box. The capacity ledger adds each machine's last
//! day of work and load, so an idle paid box shows as idle. One unread source leaves the others standing.
use crate::foundation::clock::Timestamp;
use crate::platform::broker_client::Client;
use report::Report;

mod capacity;
mod hcloud;
pub(crate) mod manifest;
mod report;

const USAGE: &str = "usage: north infra [--json]

List every live paid resource with its monthly price and a total: Hetzner Cloud servers through the broker's read-only
hcloud allowlist, and the machines and paid services south's infra/manifest.toml records. A machine both sources name is
one row, read live. Each machine's row carries its 24-hour load per core and the sessions and jobs the capacity ledger
served there, or idle when it served none. --json prints the same rows and totals.";

pub fn run(arguments: &[String]) -> u8 {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
        [flag] if flag == "-h" || flag == "--help" => {
            println!("{USAGE}");
            return 0;
        }
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let (resources, unread) = read();
    if resources.is_empty() && !unread.is_empty() {
        for why in &unread {
            eprintln!("north infra: {why}");
        }
        return 1;
    }
    let report = Report::of(Timestamp::now().iso(), resources, unread);
    if json {
        let out = serde_json::json!({
            "at": report.at, "resources": report.resources, "totals": report.totals,
            "total": report.total_line(), "unread": report.unread,
        });
        println!("{out}");
    } else {
        println!("{}", report.text());
    }
    0
}

fn read() -> (Vec<report::Resource>, Vec<String>) {
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
    let billing = match manifest::read() {
        Ok(billing) => Some(billing),
        Err(why) => {
            unread.push(format!("manifest: {why}"));
            None
        }
    };
    let mut resources = match (live, billing.as_ref()) {
        (Some(live), Some(billing)) => report::merge(live, billing),
        (Some(live), None) => live,
        (None, Some(billing)) => report::merge(Vec::new(), billing),
        (None, None) => Vec::new(),
    };
    let ledger = match capacity::read(Timestamp::now()) {
        Ok(ledger) => Some(ledger),
        Err(why) => {
            unread.push(why);
            None
        }
    };
    report::workloads(&mut resources, ledger.as_ref());
    (resources, unread)
}
