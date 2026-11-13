const TRUSTED_WRITERS: [&str; 4] = ["checker", "mirror", "lander", "git"];

/// Why a `tell` or `retract` under a name harness/graph-rules/corroborated.json trusts is refused to a caller whose `/proc/PID/cgroup` reads `cgroup`, or whose cgroup cannot be read (`None`, counted as confined as `lane_only_process` counts it): it stops the direct call from a fleet worker's scope. A session that leaves its scope (`systemd-run --user --scope`) and an `ingest` from a made-up state tree, which tells as mirror, lander and git, are held by the sandbox, which leaves the graph database unwritable to a confined worker, not by this guard (north#1279).
pub(crate) fn refusal(verb: &str, writer: &str, cgroup: Option<&str>) -> Option<String> {
    ((verb == "tell" || verb == "retract") && TRUSTED_WRITERS.contains(&writer) && cgroup.is_none_or(north_base::platform::session_scope::worker_cgroup))
        .then(|| format!("the writer `{writer}` belongs to the run that owns it, so a fleet session cannot assert or retract as it; its box is observed by `north task check`"))
}
