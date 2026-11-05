use std::cell::RefCell;

use super::*;

struct Fake {
    issue: Issue,
    sent: RefCell<Vec<String>>,
}

impl Io for Fake {
    fn issues(&self, repo: &str) -> Result<Vec<Issue>, String> {
        assert_eq!(repo, "o/r");
        Ok(vec![self.issue.clone()])
    }

    fn run_finished(&self, _: &str, run: &str) -> Option<Timestamp> {
        assert_eq!(run, "77");
        Timestamp::parse_rfc3339("2026-10-30T23:05:28Z")
    }

    fn sessions(&self) -> Vec<Session> {
        serde_json::from_str(r#"[{"name":"final-image","status":"idle","cwd":"/home/tom/code/wisp/worktrees/final-image-0"},{"name":"author","status":"idle","cwd":"/home/tom/code/wisp/worktrees/final-image"}]"#).unwrap()
    }

    fn send(&self, target: &str, text: &str) -> Result<(), String> {
        self.sent.borrow_mut().push(format!("{target} {text}"));
        Ok(())
    }
}

#[test]
fn messages_each_culprit_lane_and_the_proxy_once_per_failing_commit() {
    let sha = "b".repeat(40);
    let body = format!("CI on `main` is failing.\n\n- First failing commit: {sha} (https://github.com/o/r/actions/runs/77)\n- Culprit lane: claude/land-final-image-0123456789ab (bbbbbbbbbb)\n- Culprit lane: claude/land-other-fedcba987654 (cccccccccc)\n\n## Failing tests\n\n- cargo: render::gpu_tests::blend\n- cargo: render::gpu_tests::modulate\n");
    let io = Fake {
        issue: Issue { number: 135, title: TITLE.into(), url: "https://github.com/o/r/issues/135".into(), body },
        sent: RefCell::default(),
    };
    let state = tempfile::tempdir().unwrap();
    notify(&io, &["o/r"], state.path()).unwrap();
    let sent = io.sent.borrow().clone();
    assert!(sent.iter().any(|s| s.starts_with("author main is red in o/r from bbbbbbbbbb: your lane final-image landed in the range that broke it (cargo: render::gpu_tests::blend;cargo: render::gpu_tests::modulate)")), "{sent:#?}");
    assert!(sent.iter().any(|s| s.starts_with("proxy (sent to author) ")), "{sent:#?}");
    assert!(sent.iter().any(|s| s.starts_with("proxy (no live session for lane other) ")), "{sent:#?}");
    assert!(sent.iter().all(|s| !s.contains("lane other") || s.contains("reverted at ")), "{sent:#?}");
    notify(&io, &["o/r"], state.path()).unwrap();
    assert_eq!(io.sent.borrow().len(), 3);
}
