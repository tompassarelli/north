use std::cell::RefCell;

use super::*;

const LOG: &str = include_str!("fixtures/firn-run-38133514013.log");
const REPO: &str = "tompassarelli/firn";

fn run(id: u64, conclusion: &str, head_sha: &str) -> Run {
    Run {
        database_id: id,
        workflow_name: "Tests".into(),
        conclusion: Some(conclusion.into()),
        status: "completed".into(),
        event: "push".into(),
        head_sha: head_sha.into(),
        url: format!("https://github.com/tompassarelli/firn/actions/runs/{id}"),
    }
}

struct Fake {
    stage1: RefCell<String>,
    cause: String,
    comments: RefCell<Vec<String>>,
    prompts: RefCell<Vec<String>>,
}

impl Io for Fake {
    fn issues(&self, _: &str) -> Result<Vec<Issue>, String> {
        Ok(vec![Issue { number: 7, title: TITLE.into() }])
    }

    fn comments(&self, _: &str, _: u64) -> Result<Vec<String>, String> {
        Ok(self.comments.borrow().clone())
    }

    fn runs(&self, _: &str) -> Result<Vec<Run>, String> {
        Ok(vec![
            run(38133514013, "failure", "bad"),
            run(38133004576, "success", "good"),
            Run { workflow_name: "Main is red".into(), ..run(38133600000, "failure", "x") },
        ])
    }

    fn log(&self, _: &str, run: u64) -> Result<String, String> {
        assert_eq!(run, 38133514013);
        Ok(LOG.into())
    }

    fn compare(&self, _: &str, base: &str, head: &str) -> Result<Compare, String> {
        assert_eq!((base, head), ("good", "bad"));
        Ok(Compare { files: Some(vec![FileChange { filename: "native/system_policy.ts".into(), additions: 4, deletions: 2 }]) })
    }

    fn claude(&self, model: Model, prompt: &str) -> Result<String, String> {
        self.prompts.borrow_mut().push(prompt.into());
        Ok(if model == Model::Haiku { self.stage1.borrow().clone() } else { self.cause.clone() })
    }

    fn post(&self, repo: &str, issue: u64, body: &str) -> Result<(), String> {
        assert_eq!((repo, issue), (REPO, 7));
        self.comments.borrow_mut().push(body.into());
        Ok(())
    }
}

#[test]
fn recorded_firn_run_failed_log_through_both_stages_posts_one_diagnosis() {
    let step = "Tests (discovered): Discovered shell tests";
    let error = "system-policy-native: line 126 failed: jq -e '.hookSpecificOutput.hookEventName == \"PreToolUse\" and (.hookSpecificOutput.additionalContext | type) == \"string\"' \"$scratch/inject.out\" > /dev/null";
    let cause = "The inject hook no longer emits additionalContext as a string.";
    let extraction = serde_json::json!({ "step": step, "errors": [error, "##[error]native/system_policy_native.test.sh failed"], "culprit": "native/system_policy_native.test.sh:126" });
    let io = Fake {
        stage1: RefCell::new(format!("```json\n{extraction}\n```")),
        cause: cause.into(),
        comments: RefCell::default(),
        prompts: RefCell::default(),
    };
    assert_eq!(diagnose(&io, &[REPO]).len(), 1);
    assert_eq!(io.comments.borrow().len(), 1);
    for part in [step, error, cause, "<!-- ci-diagnose:38133514013 -->"] {
        assert!(io.comments.borrow()[0].contains(part), "comment lacks {part}");
    }
    assert!(io.prompts.borrow()[1].contains("native/system_policy.ts +4 -2"));
    assert_eq!(diagnose(&io, &[REPO]).len(), 0);
    assert_eq!(io.comments.borrow().len(), 1);
    io.comments.borrow_mut().clear();
    *io.stage1.borrow_mut() = serde_json::json!({ "step": "Invented step", "errors": [error], "culprit": null }).to_string();
    assert_eq!(diagnose(&io, &[REPO]).len(), 0);
    *io.stage1.borrow_mut() = "The failing step is the shell tests.".into();
    assert_eq!(diagnose(&io, &[REPO]).len(), 0);
    assert!(io.comments.borrow().is_empty());
}
