use std::fs;

use super::{Fixture, git};

impl Fixture {
    fn opt_into_app(&mut self) {
        fs::write(self.repo.join(".safe-push"), "[landing]\n\tpush-app = lander\n").unwrap();
        git(&self.repo, &["add", "."]);
        git(&self.repo, &["commit", "-qm", "opt into the lander App"]);
        git(&self.repo, &["push", "-q", "origin", "main"]);
        self.base = git(&self.repo, &["rev-parse", "HEAD"]);
    }
}

#[test]
fn serve_push_actor_app() {
    let mut fixture = Fixture::new();
    fixture.opt_into_app();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    let pushes = fixture.read("pushes");
    assert_eq!(pushes.lines().count(), 1);
    assert!(pushes.contains("token=none app=app-token"), "the push carries the App token and no ambient token: {pushes}");
    assert!(!pushes.contains("push-token"), "the ambient token reached the push: {pushes}");
    assert!(pushes.contains("credential.helper="), "the push must reset the inherited credential helpers: {pushes}");
    assert!(!pushes.contains("app-token "), "the token must not be on the command line: {pushes}");
    assert_eq!(fixture.read("mints"), "x");
    let env = fixture.read("env-1");
    assert!(env.contains("LAND_TRACE="));
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE=", "NORTH_LAND_PUSH_TOKEN="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the check saw {name}");
    }
}

#[test]
fn serve_app_mint_failure_keeps_the_ticket() {
    let mut fixture = Fixture::new();
    fixture.opt_into_app();
    let a = fixture.lane("a", "a.txt", "a");
    let ticket = fixture.enqueue(&a, None);
    fs::write(fixture.trace.join("mint-fail"), "").unwrap();
    let output = fixture.serve();
    assert_eq!(output.status.code(), Some(75));
    assert!(String::from_utf8_lossy(&output.stdout).contains("transient: no push credential"), "{}", String::from_utf8_lossy(&output.stdout));
    assert!(ticket.exists(), "a mint failure decided the lane");
    assert_eq!(fixture.verdict(&ticket), None);
    assert_eq!(fixture.main(), fixture.base);
    assert_eq!(fixture.read("pushes"), "", "a mint failure fell back to another credential");
    fs::remove_file(fixture.trace.join("mint-fail")).unwrap();
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.verdict(&ticket), Some(format!("landed {}", fixture.main())));
    assert!(fixture.read("pushes").contains("app=app-token"));
}

#[test]
fn serve_unconfigured_repository_keeps_its_credential() {
    let mut fixture = Fixture::new();
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    assert!(fixture.serve().status.success());
    assert!(fixture.read("pushes").contains("token=push-token app=none"));
    assert_eq!(fixture.read("mints"), "");
}
