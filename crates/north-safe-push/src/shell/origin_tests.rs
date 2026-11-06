use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use super::*;

#[test]
fn push_command_env() {
    let text = || "\\PC{0,40}";
    let fields = ((text(), text(), proptest::option::of(text()), text()), (any::<bool>(), proptest::option::of(text())));
    TestRunner::default()
        .run(&fields, |((url, refspec, lease, claimed), (skip_hooks, source))| {
            let hooks = if skip_hooks { Hooks::Skip } else { Hooks::Run };
            let push = Push { source: source.as_deref().map(Path::new), url: &url, refspec: &refspec, lease: lease.as_deref(), hooks, claimed: &claimed };
            let command = push_command(&push, Path::new("."));
            prop_assert_eq!(command.get_program(), "git");
            prop_assert_eq!(command.get_current_dir(), Some(Path::new(source.as_deref().unwrap_or("."))));
            let environment: Vec<(String, Option<String>)> = command
                .get_envs()
                .map(|(key, value)| (key.to_string_lossy().into_owned(), value.map(|value| value.to_string_lossy().into_owned())))
                .collect();
            prop_assert_eq!(
                environment,
                vec![("SAFE_PUSH_ACTIVE".to_owned(), Some("1".to_owned())), ("SAFE_PUSH_OUTPUT_CLAIMED".to_owned(), Some(claimed.clone()))]
            );
            let arguments: Vec<String> = command.get_args().map(|argument| argument.to_string_lossy().into_owned()).collect();
            let mut expected = vec!["push".to_owned(), "--no-follow-tags".to_owned()];
            if skip_hooks {
                expected.push("--no-verify".to_owned());
            }
            expected.extend(lease);
            expected.extend([url, refspec]);
            prop_assert_eq!(arguments, expected);
            Ok(())
        })
        .unwrap();
}
