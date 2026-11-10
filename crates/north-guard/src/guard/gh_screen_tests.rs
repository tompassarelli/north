use super::*;

struct Fake {
    body: String,
    stdin: Vec<u8>,
    rows: Vec<serde_json::Value>,
}

impl unrunnable_box::Log for Fake {
    fn enforcing(&self) -> bool {
        false
    }

    fn record(&mut self, row: serde_json::Value) {
        self.rows.push(row);
    }
}

impl World for Fake {
    fn current(&mut self, _: &str, _: Option<&str>) -> Option<String> {
        Some(self.body.clone())
    }

    fn repo(&mut self) -> Option<String> {
        Some("tompassarelli/north".to_owned())
    }

    fn stdin(&mut self) -> std::io::Result<Vec<u8>> {
        Ok(std::mem::take(&mut self.stdin))
    }
}

fn run(words: &[&str], stdin: &str) -> Result<(Vec<String>, Option<Vec<u8>>), String> {
    let words: Vec<String> = words.iter().map(|word| (*word).to_owned()).collect();
    let (kept, allowed, edit) = parse(&words).expect("an issue edit that sets the body");
    let mut world = Fake { body: "x".repeat(69_741), stdin: stdin.as_bytes().to_vec(), rows: Vec::new() };
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

#[test]
fn a_box_with_no_runnable_check_in_a_body_file_is_logged_when_gh_runs_whatever_wrote_the_file_north_1041() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("body.md");
    std::fs::write(&file, "Item: x\n\n## Done when\n\n- [ ] A box naming no command\n- [ ] A checked box. Done when: `true` exits 0\n").unwrap();
    let file = file.to_str().unwrap();
    for words in [vec!["issue", "create", "--title", "x", "--label", "bug", "--body-file", file], vec!["issue", "edit", "1041", "-R", "tompassarelli/north", "-F", file]] {
        let words: Vec<String> = words.into_iter().map(str::to_owned).collect();
        let (_, _, write) = parse(&words).unwrap();
        let mut world = Fake { body: "- [ ] An old box".to_owned(), stdin: Vec::new(), rows: Vec::new() };
        judge(&write, None, &mut world).unwrap();
        let logged: Vec<(&str, &str)> = world.rows.iter().map(|row| (row["verb"].as_str().unwrap(), row["box"].as_str().unwrap())).collect();
        assert_eq!(logged, [(if write.creates { "create" } else { "edit" }, "A box naming no command")]);
    }
}
