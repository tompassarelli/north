mod bounded;
mod deploy;
mod failure;
mod follow;
mod health;
pub(crate) mod layout;
mod pull;
mod remote;
mod store;
mod update;

use std::path::Path;

const USAGE: &str = "usage: north runtime update | north runtime deploy [HOST...] | north runtime health | north runtime promote | north runtime follow --repo-dir DIR";

pub(crate) fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let layout = layout::Layout::from_env();
    let result = match arguments.split_first() {
        Some((command, [])) if command == "update" => update::update(&layout),
        Some((command, [])) if command == "health" => health::health(&layout),
        Some((command, [])) if command == "promote" => follow::promote(&layout),
        Some((command, [flag, repo])) if command == "follow" && flag == "--repo-dir" => follow::follow(&layout, Path::new(repo)),
        Some((command, hosts)) if command == "deploy" => {
            if let Some(flag) = hosts.iter().find(|host| host.starts_with('-')) {
                eprintln!("north runtime deploy: unknown flag {flag}\n{USAGE}");
                return 64;
            }
            deploy::deploy(&layout, hosts)
        }
        _ => {
            eprintln!("{USAGE}");
            return 64;
        }
    };
    match result {
        Ok(()) => 0,
        Err(failure) => {
            eprintln!("north runtime {}: {}", arguments[0], failure.message);
            failure.code
        }
    }
}
