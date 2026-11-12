use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

const RETIRE: &[north_base::lifecycle::Retire] = &[
    north_base::lifecycle::retire::<north_landing::land::after_landing_lifecycle::AfterLandingRun>,
    north_base::lifecycle::retire::<north_core::land_ticket::LandTicket>,
];

pub struct Health {
    pub text: String,
    pub json: Value,
    pub ok: bool,
}

pub fn run(arguments: &[String], citations: fn(&std::path::Path) -> Result<(), String>) -> std::process::ExitCode {
    let (dry_run, json) = match arguments {
        [] => (false, false),
        [flag] if flag == "--dry-run" => (true, false),
        [flag] if flag == "--json" => (false, true),
        [first, second] if (first == "--dry-run" && second == "--json") || (first == "--json" && second == "--dry-run") => (true, true),
        [verb] if verb == "facts" => return crate::health::run(arguments, citations, RETIRE),
        _ => {
            eprintln!("Usage: north health [--dry-run] [--json] | facts");
            return std::process::ExitCode::from(2);
        }
    };
    match super::Snapshot::new().health(dry_run) {
        Ok(snapshot) => {
            let Some(health) = &snapshot.health else { return std::process::ExitCode::from(1) };
            if json {
                println!("{}", health.json);
            } else {
                print!("{}", health.text);
            }
            if health.ok { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::from(1) }
        }
        Err(why) => {
            eprintln!("north health: {why}");
            std::process::ExitCode::from(1)
        }
    }
}

pub fn read(dry_run: bool) -> Result<Health, String> {
    let now = Timestamp::now().unix_seconds();
    let bars = crate::cli::scoreboard::ready::run(now, crate::cli::scoreboard::ready::View::default())?;
    let concerns = north_concerns::tick::latest(now);
    let (invariants_ok, invariants) = crate::health::report(dry_run, |repo| crate::ops::citation_check::verdict(repo, crate::cli::tools::defines))?;
    let mut text = String::from("Health\n\nReadiness bars:\n");
    text.push_str(&bars);
    text.push_str(&format!("\nConcerns: {} rows, {} red, measured {} min ago\n", concerns.rows, concerns.red, (now - concerns.at).max(0) / 60));
    for line in &concerns.lines {
        text.push_str(&format!("  {line}\n"));
    }
    text.push_str("\nHarness invariants:\n");
    for line in &invariants {
        text.push_str(&format!("  {line}\n"));
    }
    Ok(Health {
        text,
        json: json!({
            "at": Timestamp::from_unix_seconds(now).iso(),
            "bars": bars,
            "concerns": concerns.lines,
            "red_concerns": concerns.red,
            "concerns_at": Timestamp::from_unix_seconds(concerns.at).iso(),
            "invariants": invariants,
        }),
        ok: invariants_ok,
    })
}
