use super::billing;
use super::render;
use super::Screen;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{AccountOutlook, Runway};

const MANIFEST: &str = r#"
[[machine]]
id = "whiterabbit"
monthly_usd = 0.0

[[machine]]
id = "nexus"
monthly_usd = 48.0

[[machine]]
id = "cpx62"
hourly_eur = 0.2452
monthly_cap_eur = 152.99
monthly_usd = 171.71

[[service]]
id = "anthropic-a"
plan = "Ultra"
monthly_usd = 216.67

[[service]]
id = "github"
plan = "free"
monthly_usd = "unread"
"#;

fn account(name: &str) -> AccountOutlook {
    AccountOutlook { account: name.to_owned(), week_used: Some(82.0), week_resets: Some(Timestamp::from_unix_seconds(1_794_000_000)), burn_per_hour: Some(5.8), runway: Runway::Empties { minutes: 188.0 } }
}

#[test]
fn the_billing_section_names_each_plan_and_machine_within_fifteen_lines_of_120_characters() {
    let screen = Screen {
        window: "24h".to_owned(),
        now: 1_793_990_000,
        bars: Err("unread".to_owned()),
        accounts: Ok(["a", "b", "c", "d"].map(account).to_vec()),
        runs: Err("unread".to_owned()),
        attempts: Err("unread".to_owned()),
        billing: billing::parse(MANIFEST),
    };
    let text = render::screen(&screen);
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|line| line.starts_with("Billing:")).expect("a billing section");
    let end = lines[start..].iter().position(|line| line.is_empty()).map_or(lines.len(), |gap| start + gap);
    assert!(end - start <= 15, "{} billing lines", end - start);
    assert!(lines.iter().all(|line| line.chars().count() <= 120), "{text}");
    let section = lines[start..end].join("\n");
    assert!(section.contains("Ultra $216.67/month"), "{section}");
    assert!(section.contains("€0.2452/h, capped at €152.99/month"), "{section}");
    assert!(section.contains("up to $219.71"), "{section}");
}
