use super::database::{OffloadRow, capacity_database, offload_rows};
use super::{args, fail};
use crate::capacity::build_offload::median;

const DEFAULT_LAST: usize = 5;

fn seconds(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_owned(), |seconds| format!("{seconds:.1}s"))
}

pub(super) fn summary(rows: &[OffloadRow]) -> String {
    let of = |state: Option<&str>| -> Vec<f64> { rows.iter().filter(|row| state.is_none_or(|state| row.cache_state.as_deref() == Some(state))).map(|row| row.ship_seconds).collect() };
    let mut lines: Vec<String> = rows
        .iter()
        .map(|row| format!("{}  {}  {}  {}  ship {}  wall {}", row.at, row.host.as_deref().unwrap_or("-"), row.owner.as_deref().unwrap_or("-"), row.cache_state.as_deref().unwrap_or("-"), seconds(Some(row.ship_seconds)), seconds(row.wall_seconds)))
        .collect();
    lines.push(format!("offloads={} median ship {} (warm {}: {}; cold {}: {})", rows.len(), seconds(median(&of(None))), of(Some("warm")).len(), seconds(median(&of(Some("warm")))), of(Some("cold")).len(), seconds(median(&of(Some("cold"))))));
    lines.join("\n")
}

/// Prints the newest offloaded runs recorded on this host with each one's ship time and cache state, then the median ship time.
pub(super) fn print(argv: &[String]) -> u8 {
    let options = args::Options::parse(argv, 1, &["--last"], &[]);
    let last = options.get("--last").map_or(DEFAULT_LAST, |count| args::positive_integer(count, "--last", None) as usize);
    let db = capacity_database().unwrap_or_else(|error| fail(&format!("capacity database: {error}"), 1));
    let rows = offload_rows(&db, last).unwrap_or_else(|error| fail(&format!("capacity database: {error}"), 1));
    println!("{}", summary(&rows));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(state: &str, ship: f64) -> OffloadRow {
        OffloadRow { at: "2026-11-04T10:00:00.000Z".to_owned(), host: Some("gex45".to_owned()), owner: Some("worker".to_owned()), ship_seconds: ship, cache_state: Some(state.to_owned()), wall_seconds: Some(60.0) }
    }

    #[test]
    fn the_summary_prints_the_median_ship_time_overall_and_by_cache_state() {
        let rows = [row("warm", 2.0), row("cold", 31.0), row("warm", 4.0), row("warm", 3.0), row("cold", 29.0)];
        let text = summary(&rows);
        assert!(text.ends_with("offloads=5 median ship 4.0s (warm 3: 3.0s; cold 2: 30.0s)"), "{text}");
        assert_eq!(text.lines().count(), 6);
    }

    #[test]
    fn no_offloaded_runs_print_a_dash_median() {
        assert_eq!(summary(&[]), "offloads=0 median ship - (warm 0: -; cold 0: -)");
    }
}
