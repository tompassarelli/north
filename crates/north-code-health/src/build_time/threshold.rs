pub(super) const LEAF_TEST_NO_RUN_SECONDS_MAX: f64 = 10.0;

pub(super) fn limit(workspace: &str, configured: Option<f64>) -> (f64, Option<String>) {
    match configured {
        Some(seconds) if seconds > LEAF_TEST_NO_RUN_SECONDS_MAX => (
            LEAF_TEST_NO_RUN_SECONDS_MAX,
            Some(format!("warning: {workspace}: structure.toml test_no_run_seconds = {seconds} is above the {LEAF_TEST_NO_RUN_SECONDS_MAX} s cap, which it may only lower; using {LEAF_TEST_NO_RUN_SECONDS_MAX} s")),
        ),
        Some(seconds) => (seconds, None),
        None => (LEAF_TEST_NO_RUN_SECONDS_MAX, None),
    }
}
