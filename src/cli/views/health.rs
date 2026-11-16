use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

pub struct Health {
    pub text: String,
    pub json: Value,
    pub ok: bool,
}

pub fn run(arguments: &[String]) -> std::process::ExitCode {
    let json = match arguments {
        [] => false,
        [flag] if flag == "--json" => true,
        _ => {
            eprintln!("Usage: north health [--json]");
            return std::process::ExitCode::from(2);
        }
    };
    match super::Snapshot::new().health() {
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

pub fn read() -> Result<Health, String> {
    let now = Timestamp::now().unix_seconds();
    let bars = crate::cli::scoreboard::ready::run(now, crate::cli::scoreboard::ready::View::default())?;
    let concerns = north_concerns::tick::latest(now);
    let mut text = String::from("Health\n\nReadiness bars:\n");
    text.push_str(&bars);
    text.push_str(&format!("\nConcerns: {} rows, {} red, measured {} min ago\n", concerns.rows, concerns.red, (now - concerns.at).max(0) / 60));
    for line in &concerns.lines {
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
        }),
        ok: concerns.red == 0,
    })
}
