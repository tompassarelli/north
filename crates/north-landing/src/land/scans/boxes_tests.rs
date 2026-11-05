use super::*;
use std::path::PathBuf;
use std::process::Command;

const BODY: &str = "## Proxy role\n- [x] W1 the sender is a required value\n- [ ] L1 `Item:` and `Box:` trailers, checked before the push\n- [ ] L2 the tick function\n- [ ] (observed) a week of ticks\n";

struct Fake(Result<String, String>);

impl Github for Fake {
    fn body(&self, issue: &Issue) -> Result<String, String> {
        assert_eq!(issue.to_string(), "tompassarelli/north#112");
        self.0.clone()
    }

    fn set_body(&self, _: &Issue, _: &str) -> Result<(), String> {
        panic!("the scan never writes")
    }

    fn close(&self, _: &Issue, _: &str) -> Result<(), String> {
        panic!("the scan never closes")
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Lane {
    _root: tempfile::TempDir,
    repo: PathBuf,
    base: String,
}

impl Lane {
    fn new() -> Lane {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().to_path_buf();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "user.name", "box-scan-test"]);
        git(&repo, &["config", "user.email", "box-scan-test@example.invalid"]);
        git(&repo, &["config", "core.hooksPath", "/dev/null"]);
        git(&repo, &["commit", "-q", "--allow-empty", "-m", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        Lane { _root: root, repo, base }
    }

    fn commit(&self, message: &str) -> &Self {
        git(&self.repo, &["commit", "-q", "--allow-empty", "-m", message]);
        self
    }

    fn scan(&self, github: &Fake) -> (u8, String) {
        let mut out = Vec::new();
        let code = scan(&self.repo, &self.base, "HEAD", github, &mut out);
        (code, String::from_utf8(out).unwrap())
    }
}

fn issue_body() -> Fake {
    Fake(Ok(BODY.to_owned()))
}

#[test]
fn a_box_trailer_matching_an_unticked_box_of_its_item_passes() {
    let lane = Lane::new();
    lane.commit("Check trailers before the push\n\nItem: north#112\nBox: L1 `Item:` and `Box:` trailers, checked before the push");
    let (code, out) = lane.scan(&issue_body());
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("Box: L1 `Item:` and `Box:` trailers, checked before the push matches an unticked box on tompassarelli/north#112"), "{out}");
}

#[test]
fn a_box_trailer_matching_no_unticked_box_of_its_item_is_refused() {
    for (box_text, why) in [("R9 a box nobody wrote", "no box matches"), ("W1 the sender", "already ticked"), ("L", "unticked boxes match")] {
        let lane = Lane::new();
        lane.commit("A lane\n\nItem: north#112").commit(&format!("Another commit\n\nItem: north#112\nBox: {box_text}"));
        let (code, out) = lane.scan(&issue_body());
        assert_eq!(code, 1, "{box_text}: {out}");
        assert!(out.contains(&format!("Box: {box_text} matches no unticked box on tompassarelli/north#112")) && out.contains(why), "{out}");
    }
    let lane = Lane::new();
    lane.commit("A lane\n\nItem: north#112\nBox: L2 the tick function");
    let (code, out) = lane.scan(&Fake(Ok("1. a numbered part\n".into())));
    assert_eq!(code, 1);
    assert!(out.contains("the issue has no checkboxes"), "{out}");
}

#[test]
fn a_box_that_needs_observation_passes_with_a_note_that_the_landing_leaves_it() {
    let lane = Lane::new();
    lane.commit("Lane\n\nItem: north#112\nBox: a week of ticks");
    let (code, out) = lane.scan(&issue_body());
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("needs observation"), "{out}");
}

#[test]
fn an_orphan_box_or_an_unreadable_issue_is_refused_and_a_lane_without_trailers_passes() {
    let lane = Lane::new();
    lane.commit("Lane\n\nBox: L2 the tick function");
    let (code, out) = lane.scan(&issue_body());
    assert_eq!(code, 1);
    assert!(out.contains("has no Item: line before it"), "{out}");

    let lane = Lane::new();
    lane.commit("Lane\n\nItem: north#112\nBox: L2 the tick function");
    let (code, out) = lane.scan(&Fake(Err("gh issue view timed out after 30 s".into())));
    assert_eq!(code, 1);
    assert!(out.contains("cannot read tompassarelli/north#112") && out.contains("timed out"), "{out}");

    let lane = Lane::new();
    lane.commit("A plain change");
    assert_eq!(lane.scan(&Fake(Err("never asked".into()))), (0, String::new()));
}

#[test]
fn north_cloud_land_keeps_both_trailers_once_when_it_folds_the_handback() {
    let first = "First\n\nBody.\n\nItem: north#112\nBox: L1 trailers";
    let second = "Second\n\nItem: north#112\nBox: L2 the tick function\nBox: L1 trailers\nItem: wisp#9\nBox: L1 trailers";
    let (bodies, trailers) = gather_trailers(&[first, second, "Third"]);
    assert_eq!(bodies, ["First\n\nBody.", "Second", "Third"]);
    assert_eq!(trailers, ["Item: north#112", "Box: L1 trailers", "Box: L2 the tick function", "Item: wisp#9", "Box: L1 trailers"]);
    let folded = format!("{}\n\n{}\n", bodies.join("\n\n"), trailers.join("\n"));
    let pairs: Vec<(String, String)> = tick::trailers(&folded).unwrap().into_iter().map(|trailer| (trailer.item, trailer.box_text)).collect();
    assert_eq!(pairs, [("north#112".into(), "L1 trailers".into()), ("north#112".into(), "L2 the tick function".into()), ("wisp#9".into(), "L1 trailers".into())]);
}
