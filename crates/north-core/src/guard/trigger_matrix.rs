const PREFIXES: &[&str] = &[
    "",
    "CHECK=1 ",
    "CHECK=\"some value\" ",
    "command ",
    "sudo ",
    "env CHECK=1 ",
    "CHECK=1 command sudo env OTHER=2 ",
    "CHECK_0=1 sudo env command ",
    "CHECK_1=2 env sudo ",
    "sudo -u tom ",
    "env -u CHECK CHECK=1 ",
    "CHECK=${HOME} ",
    "CHECK=$HOME ",
];
const SEPARATORS: &[&str] = &[";", "&&", "|", "\n"];

fn single_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

fn double_quoted(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    format!("\"{escaped}\"")
}

pub fn exercise(
    denies: impl Fn(&str) -> bool,
    triggers: &[&str],
    near_misses: &[&str],
    quoted_whole_is_denied: bool,
) {
    let check = |expected: bool, command: &str| {
        assert_eq!(denies(command), expected, "{command:?}");
    };
    for trigger in triggers {
        check(true, trigger);
        check(true, &format!("CHECK=$({trigger}); echo $CHECK"));
        for prefix in PREFIXES {
            let wrapped = format!("{prefix}{trigger}");
            check(true, &wrapped);
            for separator in SEPARATORS {
                check(true, &format!("printf ready {separator} {wrapped}"));
            }
        }
        let single = single_quoted(trigger);
        let double = double_quoted(trigger);
        check(quoted_whole_is_denied, &single);
        check(quoted_whole_is_denied, &double);
        check(false, &format!("echo {trigger}"));
        check(false, &format!("echo {single}"));
        check(false, &format!("echo {double}"));
        check(false, &format!("git commit -m {single}"));
        check(false, &format!("cat <<'BODY'\n{trigger}\nBODY"));
        for separator in [";", "&&", "|", "\n", "("] {
            let prose = format!("example {separator} {trigger}");
            check(false, &format!("echo {}", single_quoted(&prose)));
            check(false, &format!("echo {}", double_quoted(&prose)));
        }
    }
    for command in near_misses {
        check(false, command);
    }
}
