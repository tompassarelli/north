use super::*;
use crate::watch::verdicts::{LOST as NO_VERDICT, Outcome, after_landing, hand_in, unanswered};

const RECORDED: &str = "\
1794637571.050523 gex45 bb_worker[2997224]: 2026/11/14 06:26:11 Worker {\"hostname\":\"gex45\",\"slot\":\"2\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637571.484523 gex45 bb_worker[2997224]: 2026/11/14 06:26:11 Worker {\"hostname\":\"gex45\",\"slot\":\"3\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637572.223106 gex45 bb_worker[2997224]: 2026/11/14 06:26:12 Worker {\"hostname\":\"gex45\",\"slot\":\"1\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637573.232444 gex45 bb_worker[2997224]: 2026/11/14 06:26:13 Worker {\"hostname\":\"gex45\",\"slot\":\"0\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637573.488701 gex45 bb_worker[2997224]: 2026/11/14 06:26:13 Worker {\"hostname\":\"gex45\",\"slot\":\"2\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637574.253607 gex45 bb_worker[2997224]: 2026/11/14 06:26:14 Worker {\"hostname\":\"gex45\",\"slot\":\"1\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637574.457103 gex45 bb_worker[2997224]: 2026/11/14 06:26:14 Worker {\"hostname\":\"gex45\",\"slot\":\"0\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637574.999603 gex45 bb_worker[2997224]: 2026/11/14 06:26:14 Worker {\"hostname\":\"gex45\",\"slot\":\"3\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637576.407625 gex45 bb_worker[2997224]: 2026/11/14 06:26:16 Worker {\"hostname\":\"gex45\",\"slot\":\"3\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637577.052449 gex45 bb_worker[2997224]: 2026/11/14 06:26:17 Worker {\"hostname\":\"gex45\",\"slot\":\"2\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637577.308618 gex45 bb_worker[2997224]: 2026/11/14 06:26:17 Worker {\"hostname\":\"gex45\",\"slot\":\"1\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
1794637578.772925 gex45 bb_worker[2997224]: 2026/11/14 06:26:18 Worker {\"hostname\":\"gex45\",\"slot\":\"1\"}: rpc error: code = Internal desc = Worker failed readiness check: Failed to run cleaning command: exit status 1
";

const REFUSALS: &str = r#"{"micros":1794637022178483,"session":"reg-async","lane":"n1036-register-beside","error":"the checks before queueing reached no verdict 3 times (exit 75: the host or its connection failed before they reported); the lane is not queued uncompiled, so run safe-push again"}
{"micros":1794637085338960,"session":"burnb2-h2-gex45","lane":"h2-gex45","error":"a check before queueing exited 1; fix the failure above; failing check `cargo clippy`"}
{"micros":1794637200000000,"session":"warm-launch","lane":"warm-launch","error":"deny-list scan refused the landing"}
{"micros":1794637514821407,"session":"finish-overlap","lane":"finish-overlap-2","error":"the checks before queueing reached no verdict 3 times (exit 75: the host or its connection failed before they reported); the lane is not queued uncompiled, so run safe-push again"}
{"micros":1794637518821537,"session":"burnb2-h2-gex45","lane":"h2-gex45","error":"the checks before queueing reached no verdict 3 times (exit 75: the host or its connection failed before they reported); the lane is not queued uncompiled, so run safe-push again"}
{"micros":1794638062487331,"session":"burnb2-h2-gex45","lane":"h2-gex45","error":"a check before queueing exited 1; fix the failure above; to rerun clippy alone: cargo clippy --all-targets -p north -- -D warnings"}"#;

fn grid_from(block: &str, offsets: &[f64]) -> Grid {
    let failures = block.lines().filter_map(parse_failure).collect::<Vec<_>>();
    let mut repeated: Vec<Failure> = offsets.iter().flat_map(|offset| failures.iter().map(move |failure| Failure { at: failure.at + offset, ..failure.clone() })).collect();
    repeated.sort_by(|earlier, later| earlier.at.total_cmp(&later.at));
    Grid { host: "gex45".into(), runners: 4, failures: repeated }
}

#[test]
fn nothing_could_land_on_2026_11_14_and_the_recorded_grid_and_hand_ins_page_the_proxy_then_clear_north_1036() {
    let recorded = grid_from(RECORDED, &[0.0]);
    assert_eq!(recorded.failures.len(), 12);
    assert_eq!(zero_ready_since(&recorded, 1794637578.8), Some(1794637573.232444), "every slot first failed by then");
    let block = RECORDED.lines().filter_map(parse_failure).next_back().map(|failure| failure.at).unwrap_or_default();
    let minute = grid_from(RECORDED, &[0.0, 8.0, 16.0, 24.0, 32.0, 40.0, 48.0, 56.0, 64.0]);
    assert!(zero_ready(&minute, block + 40.0).is_none(), "a grid failing for under a minute is not paged");
    let paged = zero_ready(&minute, block + 64.0).expect("a minute of zero ready workers pages");
    assert_eq!(paged.key, "grid:zero-ready:gex45");
    assert_eq!(paged.severity, Severity::Page);
    assert!(paged.summary.starts_with(LOST) && paged.summary.ends_with("on gex45 for 70 s, all 4 slots failing their readiness check: Failed to run cleaning command: exit status 1"), "{}", paged.summary);
    assert!(zero_ready(&minute, block + 64.0 + 31.0).is_none(), "a slot that answers ends the zero-ready state");
    let one_slot = Grid { failures: minute.failures.iter().filter(|failure| failure.slot == "1").cloned().collect(), ..minute.clone() };
    assert!(zero_ready(&one_slot, block + 64.0).is_none(), "one failing slot of four leaves three ready");
    let outcomes: Vec<Outcome> = REFUSALS.lines().filter_map(hand_in).collect();
    assert_eq!(outcomes.iter().map(|outcome| outcome.verdict).collect::<Vec<_>>(), [false, true, false, false, true], "a deny-list refusal is no check verdict");
    let until = |micros: u64| outcomes.iter().filter(|outcome| outcome.at <= micros as f64 / 1e6).cloned().collect::<Vec<_>>();
    assert!(unanswered(&until(1794637022178483), 1794637022.2 + 121.0).is_some(), "the hand-in at 6:17:02 alone pages two minutes on");
    assert!(unanswered(&until(1794637085338960), 1794637022.2 + 150.0).is_none(), "the verdict at 6:18:05 answers the one at 6:17:02");
    assert!(unanswered(&until(1794637518821537), 1794637514.8 + 119.0).is_none(), "under two minutes is not paged");
    let lost = unanswered(&until(1794637518821537), 1794637514.8 + 121.0).expect("two minutes with no verdict pages");
    assert_eq!((lost.key.as_str(), lost.severity, lost.since), ("land:no-verdict", Severity::Page, 1794637514));
    assert!(lost.summary.starts_with(NO_VERDICT) && lost.summary.contains("2 of them without one; newest: hand-in h2-gex45 (burnb2-h2-gex45): the checks before queueing reached no verdict 3 times"), "{}", lost.summary);
    assert!(unanswered(&outcomes, 1794638062.5 + 60.0).is_none(), "the check verdict at 6:34:22 clears it");
    let after = after_landing("1794554447\tfirn\tb488e749cc6facdc34bfc022834648b47d06d070\ttransient\tpost-land checks passed their 726 s deadline and were killed with their process group, so they reached no verdict").expect("a transient after-landing row");
    assert!(!after.verdict && after.run == "after-landing run of firn b488e749cc", "{after:?}");
    assert!(unanswered(std::slice::from_ref(&after), after.at + 121.0).is_some(), "an after-landing run without a verdict pages too");
}
