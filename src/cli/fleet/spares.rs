//! `north spare-fill NAME` and `north spare-retire HOST DIR`: the fill each launch of a worker on a build host starts apart from itself, keeping spares there for the next launch like it, and the call that takes down every spare under a directory tree.
use super::router::open_fleet;
use crate::fleet::spares;

pub fn fill(arguments: &[String]) -> u8 {
    let [name] = arguments else {
        eprintln!("Usage: north spare-fill NAME");
        return 2;
    };
    match open_fleet().and_then(|fleet| fleet.fill_spares(name)) {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(error) => {
            eprintln!("north spare-fill: {}", error.user_message());
            1
        }
    }
}

/// `north spare-retire HOST DIR`: retires every spare on build host HOST whose directory is DIR or under it, printing one line for each.
pub fn retire(arguments: &[String]) -> u8 {
    let [host, tree] = arguments else {
        eprintln!("Usage: north spare-retire HOST DIR");
        return 2;
    };
    let Some(machine) = crate::pacing::placement::machine(host) else {
        eprintln!("north spare-retire: no [hosts.{host}] with an ssh destination");
        return 1;
    };
    let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(std::path::PathBuf::from) else {
        eprintln!("north spare-retire: HOME is not set");
        return 1;
    };
    let ssh = crate::fleet::runtime::binary("ssh");
    match spares::retire_under(&spares::Place { ssh: &ssh, machine: &machine, home: &home }, std::path::Path::new(tree)) {
        Some(lines) => {
            lines.iter().for_each(|line| println!("{host}: {line}"));
            println!("{host}: retired {} spares under {tree}", lines.len());
            0
        }
        None => {
            eprintln!("north spare-retire: {host} did not answer, or {tree} is not an absolute path");
            1
        }
    }
}
