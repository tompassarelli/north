use super::{Fixture, git};

#[test]
fn serve_token_scoped() {
    let mut fixture = Fixture::new();
    git(&fixture.repo, &["config", "remote.origin.url", "https://github.com/north-test/north.git"]);
    git(&fixture.repo, &["config", &format!("url.{}.insteadOf", fixture.origin.display()), "https://github.com/north-test/north.git"]);
    let a = fixture.lane("a", "a.txt", "a");
    fixture.enqueue(&a, None);
    let rewrite = format!("url.{}.insteadOf", fixture.origin.display());
    let served = fixture.command().env("GIT_CONFIG_COUNT", "1").env("GIT_CONFIG_KEY_0", &rewrite).env("GIT_CONFIG_VALUE_0", "https://github.com/north-test/north.git").output().unwrap();
    assert!(served.status.success(), "{}{}", String::from_utf8_lossy(&served.stdout), String::from_utf8_lossy(&served.stderr));
    let env = fixture.read("after-env");
    assert!(env.contains("LAND_TRACE="), "the after-landing check never ran");
    for name in ["GH_TOKEN=", "GITHUB_TOKEN=", "SOPS_AGE_KEY_FILE="] {
        assert!(!env.lines().any(|line| line.starts_with(name)), "the after-landing check saw {name}");
    }
    assert!(fixture.read("pushes").contains("token=none app=app-token"), "the push step pushes with the lander App token and no inherited login");
}


