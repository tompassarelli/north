use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use super::*;

#[test]
fn push_command_env() {
    let fields = ("\\PC{0,40}", "\\PC{0,40}", "\\PC{0,40}", "\\PC{0,40}");
    TestRunner::default()
        .run(&fields, |(url, refspec, lease, claimed)| {
            let push = Push { url: &url, refspec: &refspec, lease: &lease, claimed: &claimed };
            let command = push_command(&push, Path::new("."));
            prop_assert_eq!(command.get_program(), "git");
            prop_assert_eq!(command.get_current_dir(), Some(Path::new(".")));
            let environment: Vec<(String, Option<String>)> = command
                .get_envs()
                .map(|(key, value)| (key.to_string_lossy().into_owned(), value.map(|value| value.to_string_lossy().into_owned())))
                .collect();
            prop_assert_eq!(
                environment,
                vec![("SAFE_PUSH_ACTIVE".to_owned(), Some("1".to_owned())), ("SAFE_PUSH_OUTPUT_CLAIMED".to_owned(), Some(claimed.clone()))]
            );
            let arguments: Vec<String> = command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect();
            prop_assert_eq!(&arguments[..4], &["push", "--no-follow-tags", "--no-verify", lease.as_str()]);
            prop_assert_eq!(&arguments[4..], &[url.as_str(), refspec.as_str()]);
            Ok(())
        })
        .unwrap();
}
