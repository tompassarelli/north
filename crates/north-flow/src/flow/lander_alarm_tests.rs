use serde_json::json;

use super::*;

fn row(stalled: bool) -> Value {
    json!({
        "repo": "north",
        "slug": "tompassarelli/north",
        "lander": { "depth": 9, "oldest_minutes": 19.6, "idle_minutes": 11.4, "stall_limit_minutes": 10.0, "stalled": stalled, "last_line": "candidate ended without a verdict", "reasons": [] },
    })
}

#[test]
fn a_lander_stall_tells_the_proxy_and_files_once_then_tells_once_more_when_it_clears() {
    let (state, steps) = plan(&[row(true)], &State::new());
    assert!(matches!(steps.as_slice(), [Step::Tell { text, .. }, Step::File { slug, .. }] if text.contains("9 lanes queued") && text.contains("without a verdict") && slug == "tompassarelli/north"), "{steps:?}");
    let told: State = state.into_iter().map(|(repo, episode)| (repo, Episode { told: true, filed: true, ..episode })).collect();
    let (state, steps) = plan(&[row(true)], &told);
    assert!(steps.is_empty(), "{steps:?}");
    let (state, steps) = plan(&[row(false)], &state);
    assert!(matches!(steps.as_slice(), [Step::Recover { told: true, slug: Some(_), .. }]), "{steps:?}");
    assert!(state.is_empty());
}
