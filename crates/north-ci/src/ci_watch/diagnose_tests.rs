use std::cell::RefCell;

use super::*;

struct Fake {
    runs: Vec<Run>,
    comments: RefCell<Vec<String>>,
    step: &'static str,
}

impl Io for Fake {
    fn issues(&self, _: &str) -> Result<Vec<Issue>, String> {
        Ok(vec![Issue { number: 1, title: TITLE.into() }])
    }
    fn comments(&self, _: &str, _: u64) -> Result<Vec<String>, String> {
        Ok(self.comments.borrow().clone())
    }
    fn runs(&self, _: &str) -> Result<Vec<Run>, String> {
        Ok(self.runs.clone())
    }
    fn log(&self, _: &str, _: u64) -> Result<String, String> {
        Ok(format!("test\t{}\t2026-11-06T03:00:00.0Z error: boom\n", self.step))
    }
    fn compare(&self, _: &str, _: &str, _: &str) -> Result<Compare, String> {
        Ok(Compare::default())
    }
    fn claude(&self, model: Model, _: &str) -> Result<String, String> {
        Ok(match model {
            Model::Haiku => format!(r#"{{"step": "test: {}", "errors": ["error: boom"], "culprit": null}}"#, self.step),
            Model::Opus => "A bad change.".into(),
        })
    }
    fn post(&self, _: &str, _: u64, body: &str) -> Result<(), String> {
        self.comments.borrow_mut().push(body.to_owned());
        Ok(())
    }
}

fn failing(id: u64) -> Run {
    Run {
        database_id: id,
        workflow_name: "Cargo".into(),
        conclusion: Some("failure".into()),
        status: "completed".into(),
        event: "push".into(),
        head_sha: format!("sha{id}"),
        url: format!("https://x/{id}"),
    }
}

#[test]
fn a_run_failing_the_same_steps_as_the_latest_diagnosis_posts_nothing_until_the_failure_changes() {
    let mut io = Fake { runs: vec![failing(10)], comments: RefCell::default(), step: "cargo test" };
    assert_eq!(diagnose(&io, &["r/x"]).len(), 1);
    io.runs.insert(0, failing(11));
    assert!(diagnose(&io, &["r/x"]).is_empty(), "same workflow and steps: no second comment");
    io.step = "cargo clippy";
    io.runs.insert(0, failing(12));
    assert_eq!(diagnose(&io, &["r/x"]).len(), 1, "a different failing step is a new failure");
    assert_eq!(io.comments.borrow().len(), 2);
}
