use std::fs;
use std::path::Path;

use north_foundation::test_inputs::git::git_in as git;
use north_lander::land::credential;

fn asked_for(files: &[&str]) -> String {
    let scratch = tempfile::tempdir().unwrap();
    let repo = scratch.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.invalid"]);
    for file in files.iter().copied().chain([".safe-push"]) {
        let path = repo.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "[landing]\n\tpush-app = lander\n").unwrap();
    }
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    git(&repo, &["remote", "add", "origin", "https://github.com/o/r.git"]);
    let asked = scratch.path().join("asked");
    let program = scratch.path().join("mint.sh");
    fs::write(&program, format!("printf '%s\\n' \"$*\" > {}\necho app-token\n", asked.display())).unwrap();
    let token = credential::for_push(&repo, "HEAD", &["sh".to_owned(), program.display().to_string()]).unwrap();
    assert_eq!(token.as_deref(), Some("app-token"));
    read(&asked)
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap().trim().to_owned()
}

#[test]
fn a_landing_for_a_repository_whose_landings_change_no_workflow_asks_for_a_token_without_the_workflows_permission() {
    assert_eq!(asked_for(&["README"]), "--app lander --permission contents:write");
    assert_eq!(asked_for(&["README", ".github/workflows/cargo.yml"]), "--app lander", "north's landings change workflow files, so its base keeps the lander ceiling with workflows:write");
}
