use std::path::PathBuf;

use north_graph::default_path;
use crate::backlog::read;
use crate::rules::{held_query, ready_query};

const USAGE: &str = "usage: north graph ready [--db PATH] [--query ready|held]\nlists the ready boxes in rank order, then each held box with the edge that holds it; --query prints the `north graph ask` query behind a list";

pub fn command(arguments: &[String]) -> u8 {
    let mut db = default_path();
    let mut query = None;
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match (argument.as_str(), rest.next()) {
            ("--db", Some(path)) => db = PathBuf::from(path),
            ("--query", Some(which)) if which == "ready" || which == "held" => query = Some(which.clone()),
            _ => {
                eprintln!("north graph ready: unknown or incomplete {argument}\n{USAGE}");
                return 2;
            }
        }
    }
    if let Some(which) = query {
        println!("{}", if which == "ready" { ready_query() } else { held_query() });
        return 0;
    }
    match read(&db) {
        Ok(backlog) => {
            for line in backlog.lines() {
                println!("{line}");
            }
            0
        }
        Err(why) => {
            eprintln!("north graph ready: {why}");
            1
        }
    }
}
