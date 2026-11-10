use super::*;

struct Fake {
    body: String,
    stdin: Vec<u8>,
}

impl World for Fake {
    fn current(&mut self, _: &str, _: Option<&str>) -> Option<String> {
        Some(self.body.clone())
    }

    fn stdin(&mut self) -> std::io::Result<Vec<u8>> {
        Ok(std::mem::take(&mut self.stdin))
    }
}

fn run(words: &[&str], stdin: &str) -> Result<(Vec<String>, Option<Vec<u8>>), String> {
    let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let (kept, allowed, edit) = parse(&words).expect("an issue edit that sets the body");
    let mut world = Fake { body: "x".repeat(69_741), stdin: stdin.as_bytes().to_vec() };
    judge(&edit, allowed.as_deref(), &mut world).map(|kept_stdin| (kept, kept_stdin))
}

#[test]
fn a_body_file_written_by_the_same_command_is_judged_when_gh_runs_so_an_emptied_issue_is_refused_north_706() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("706.md");
    std::fs::write(&file, "").unwrap();
    let file = file.to_str().unwrap();
    assert!(run(&["issue", "edit", "706", "-R", "tompassarelli/north", "--body-file", file], "").is_err());
    assert!(run(&["issue", "edit", "706", "--body-file", "-"], "").is_err());
    assert!(run(&["issue", "edit", "706", "-b", "short"], "").is_err());
    std::fs::write(file, "y".repeat(70_000)).unwrap();
    assert!(run(&["issue", "edit", "706", "--body-file", file], "").is_ok());
    let piped = "z".repeat(70_000);
    assert_eq!(run(&["issue", "edit", "706", "-F", "-"], &piped).unwrap().1, Some(piped.into_bytes()));
    std::fs::write(file, "").unwrap();
    let (kept, _) = run(&["issue", "edit", "706", "--body-file", file, "--allow-shrink", "superseded by north#900"], "").unwrap();
    assert_eq!(kept, ["issue", "edit", "706", "--body-file", file]);
    assert!(parse(&["issue".to_owned(), "edit".to_owned(), "706".to_owned(), "--title".to_owned(), "New".to_owned()]).is_none());
}
