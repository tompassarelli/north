use serde_json::json;

use crate::cli::infra::report::Report;

pub struct Infra {
    pub report: Report,
    pub unread: Vec<String>,
}

const USAGE: &str = "usage: north infra [--json]";

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
    match super::Snapshot::new().infra() {
        Ok(snapshot) => {
            let Some(infra) = &snapshot.infra else { return 1 };
            if infra.report.resources.is_empty() && !infra.unread.is_empty() {
                for why in &infra.unread {
                    eprintln!("north infra: {why}");
                }
                return 1;
            }
            if json {
                println!("{}", figures(infra));
            } else {
                println!("{}", infra.report.text());
            }
            0
        }
        Err(why) => {
            eprintln!("north infra: {why}");
            1
        }
    }
}

pub fn read() -> Result<Infra, String> {
    let (resources, unread) = crate::cli::infra::read();
    Ok(Infra { report: Report::of(crate::foundation::clock::Timestamp::now().iso(), resources, unread.clone()), unread })
}

fn figures(infra: &Infra) -> serde_json::Value {
    json!({
        "at": infra.report.at,
        "resources": infra.report.resources,
        "totals": infra.report.totals,
        "total": infra.report.total_line(),
        "unread": infra.unread,
    })
}
