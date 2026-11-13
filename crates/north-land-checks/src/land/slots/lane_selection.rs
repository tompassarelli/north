//! `north land selection --host HOST --base BASE --head HEAD -- COMMAND`: prints, as `NAME=VALUE` lines, the package and
//! tests/it selection a check before a landing hands COMMAND for BASE..HEAD, its test listing run on HOST as the runner
//! runs it on its runner host, so a replay runs the line as that check did (north#1115).

use std::path::Path;

use super::graph_selection::Away;
use super::verdict_key::lane_environment;
use crate::platform::north_binary::land_north;

const USAGE: &str = "north land selection --host HOST --base BASE --head HEAD -- COMMAND";

pub fn run(arguments: &[String]) -> u8 {
    let [host_flag, host, base_flag, base, head_flag, head, separator, command] = arguments else {
        eprintln!("usage: {USAGE}");
        return 64;
    };
    if host_flag != "--host" || base_flag != "--base" || head_flag != "--head" || separator != "--" {
        eprintln!("usage: {USAGE}");
        return 64;
    }
    let program = land_north();
    let away = Away::on(Path::new(&program), host, base, head);
    for (name, value) in lane_environment(Path::new("."), base, head, away.as_ref(), std::slice::from_ref(command)) {
        println!("{name}={value}");
    }
    0
}
