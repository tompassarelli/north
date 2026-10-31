use serde_json::Value;
use serde_json::json;
use crate::platform::git as platform_git;
use std::path::Path;

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub root: String,
    pub cases: Vec<(String, Value, bool)>,
}
fn bash(command: impl Into<String>, cwd: &str) -> Value {
    json!({"tool_name":"Bash", "cwd":cwd, "tool_input":{"command":command.into()}})
}
fn git(args: &[&str]) {
    assert!(platform_git::run(Path::new("."), args).unwrap().status.success());
}
pub fn ordered_payload(root: &str) -> Vec<u8> {
    format!(
        r#"{{"tool_name":"apply_patch","tool_input":{{"z":{},"a":{}}}}}"#,
        json!(format!(
            "*** Begin Patch\n*** Delete File: {root}/north/main/README.md\n*** End Patch"
        )),
        json!("*** Begin Patch\n*** Delete File: /tmp/ok\n*** End Patch")
    )
    .into_bytes()
}
pub fn fixture() -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("code").display().to_string();
    let main = format!("{root}/proj/main");
    let pin = format!("{root}/proj/pins/0123456789abcdef0123456789abcdef01234567");
    let lane = format!("{root}/proj/worktrees/main");
    for path in [
        &main,
        &pin,
        &lane,
        &format!("{root}/client/acme/app/main/.git"),
        &format!("{root}/resources/upstream/main/.git"),
        &format!("{root}/runtime-data/.git"),
    ] {
        std::fs::create_dir_all(path).unwrap();
    }
    for path in [&main, &pin] {
        git(&["init", "-q", path]);
    }
    std::fs::create_dir_all(format!("{main}/src")).unwrap();
    std::fs::write(format!("{main}/src/kept.txt"), "tracked\n").unwrap();
    std::fs::create_dir_all(format!("{main}/build")).unwrap();
    std::fs::write(format!("{main}/build/out.js"), "generated\n").unwrap();
    git(&["-C", &main, "add", "src/kept.txt"]);
    git(&[
        "-C",
        &main,
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "commit",
        "-qm",
        "base",
    ]);
    let sidecar = format!("{pin}.pin");
    std::fs::write(&sidecar, "Consumers: docs-boundary-consumer.\n").unwrap();
    std::fs::write(format!("{pin}/.gitignore"), "build/\n").unwrap();
    std::fs::create_dir_all(format!("{pin}/build")).unwrap();
    let mut cases = Vec::new();
    let mut edit = |path: String, denied: bool| {
        cases.push((
            format!("Edit {path}"),
            json!({"tool_name":"Edit", "tool_input":{"file_path":path}}),
            denied,
        ))
    };
    for (path, denied) in [
        (format!("{main}/src/kept.txt"), true),
        (format!("{root}/client/acme/app/main/src/x.py"), true),
        (format!("{root}/proj/worktrees/x/src/x.py"), false),
        (format!("{lane}/src/x.py"), false),
        (format!("{root}/proj/scratch.txt"), false),
        (format!("{root}/proj/worktrees"), false),
        (format!("{root}/proj/pins"), false),
        (format!("{pin}/index.html"), true),
        (sidecar.clone(), false),
        (format!("{pin}/nested.pin"), true),
        (format!("{pin}/build/out.js"), true),
        (format!("{root}/runtime-data/state.json"), false),
        (format!("{root}/resources/upstream/main/README.md"), false),
        (String::new(), false),
    ] {
        edit(path, denied);
    }
    for (command, denied) in [
        (format!("rm -rf {main}/build"), false),
        (format!("rm {main}/src/kept.txt"), true),
        (format!("rm -rf {main}/src"), true),
        (format!("rm -rf {main}"), true),
        (format!("echo x > {main}/build/new.js"), true),
        (
            format!("CARGO_TARGET_DIR={root}/proj/target-agent-a cargo test"),
            true,
        ),
        (
            format!("cargo test --target-dir {root}/proj/target-agent-b"),
            true,
        ),
        (format!("CARGO_TARGET_DIR={lane}/target cargo test"), false),
        (
            format!("W={root}/proj/worktrees/x; git -C {main} worktree add $W"),
            false,
        ),
        (
            format!("W={root}/proj/worktrees/x && git -C {main} worktree add -b x ${{W}}"),
            false,
        ),
        (
            format!("W={main}/inside; git -C {main} worktree add $W"),
            true,
        ),
        (format!("git -C {main} worktree add $UNSET"), true),
        (
            "cargo test --target-dir /tmp/galileo-target-agent-a".into(),
            false,
        ),
    ] {
        cases.push((command.clone(), bash(command, &lane), denied));
    }
    for (path, denied) in [
        (format!("{main}/src/kept.txt"), true),
        (format!("{lane}/x.txt"), false),
        (format!("{pin}/x.txt"), true),
    ] {
        let envelope =
            format!("*** Begin Patch\n*** Update File: {path}\n@@\n-a\n+b\n*** End Patch");
        cases.push((
            format!("patch {path}"),
            json!({"tool_name":"apply_patch", "cwd":lane, "tool_input":{"input":envelope}}),
            denied,
        ));
    }
    cases.push((
        "unreadable patch".into(),
        json!({"tool_name":"apply_patch", "tool_input":{"input":"not a patch"}}),
        true,
    ));
    for input in [
        json!("not a patch"),
        json!([]),
        json!(format!(
            "*** Begin Patch\n*** Delete File: {main}/src/kept.txt\n*** End Patch"
        )),
    ] {
        cases.push((
            "direct patch string or array".into(),
            json!({"tool_name":"apply_patch", "tool_input":input}),
            true,
        ));
    }
    let north = format!("{root}/north/main");
    let galileo = format!("{root}/galileo/main");
    for path in [&north, &galileo] {
        std::fs::create_dir_all(format!("{path}/.git")).unwrap();
    }
    for (command, cwd) in [
        (
            "python3 - <<'PYEOF'\nopen('cli/x.clj','w')\nPYEOF".into(),
            north.clone(),
        ),
        ("git add cli/x.clj".into(), north.clone()),
        ("git commit -q -m 'x'".into(), north.clone()),
        (
            format!("git -C {galileo} reset --hard origin/main"),
            root.clone(),
        ),
        ("FILES=$(cat list); git add $FILES".into(), galileo.clone()),
        ("git push origin main".into(), north.clone()),
        (format!("echo x > {north}/cli/x.clj"), root.clone()),
        (
            format!("sed -i s/a/b/ {galileo}/bin/galileo-land-check"),
            root.clone(),
        ),
        (
            format!("perl -i -pe s/a/b/ {north}/cli/x.clj"),
            root.clone(),
        ),
        ("perl -pi.bak -e s/a/b/ cli/x.clj".into(), north.clone()),
        ("ruby -i -pe s/a/b/ cli/x.clj".into(), north.clone()),
        ("sed -i s/a/b/ cli/x.clj".into(), north.clone()),
        (
            format!("python3 -c \"open('{north}/cli/x.clj','w').write('x')\""),
            root.clone(),
        ),
        (
            "python3 -c \"import pathlib; pathlib.Path('cli/x.clj').write_text('x')\"".into(),
            north.clone(),
        ),
        (format!("cp /tmp/x.clj {north}/cli/x.clj"), root.clone()),
        (format!("rsync -a /tmp/x.clj {north}/x"), root.clone()),
        (format!("scp /tmp/x.clj {north}/x"), root.clone()),
        ("rsync -a /tmp/x.clj ./x".into(), north.clone()),
        (format!("rm {galileo}/bin/galileo-land-check"), root.clone()),
        (format!("cd {north} && echo x > cli/x.clj"), root.clone()),
    ] {
        cases.push((command.clone(), bash(command, &cwd), true));
    }
    for command in [
        "rsync -a /tmp/x.clj gex45:/tmp/x",
        "scp /tmp/x.clj tom@gex45:/tmp/x",
        "perl -ne print cli/x.clj",
        "ruby -ne print cli/x.clj",
        "python3 -c \"print(open('cli/x.clj').read())\"",
        "python3 -c \"open('/tmp/out.txt','w').write('x')\"",
        "perl -Mstrict -ne print cli/x.clj",
        "north convo install map warcraft --limit 5",
        "north convo search install out/x",
        "gh issue list --search \"closed:>=2026-10-22\" --limit 5",
        "echo \"closed: $(gh issue list --search \"closed:>=$(date -u -d '10 hours ago' +%FT%H:%M)\" --limit 5)\"",
        "gh search issues --search 'created:>=2026-10-22'",
    ] {
        let command = command.to_string();
        cases.push((command.clone(), bash(command, &north), false));
    }
    for command in [
        "perl -pi -e s/a/b/ src/x.txt",
        "ruby -i -pe s/a/b/ src/x.txt",
        "sed -i s/a/b/ src/x.txt",
        "python3 -c \"open('src/x.txt','w').write('x')\"",
    ] {
        let command = command.to_string();
        cases.push((command.clone(), bash(command, &lane), false));
    }
    let envelope =
        "*** Begin Patch\n*** Update File: code/north/main/cli/x.clj\n@@\n-a\n+b\n*** End Patch";
    cases.push((
        "relative patch".into(),
        json!({"tool_name":"apply_patch", "cwd":directory.path(), "tool_input":{"input":envelope}}),
        true,
    ));
    cases.push((
        "Bash top-level cwd overrides workdir".into(),
        json!({"tool_name":"Bash", "cwd":north, "tool_input":{"command":"git add README.md", "workdir":"/tmp"}}),
        true,
    ));
    let property_main = format!("{root}/property/main");
    std::fs::create_dir_all(format!("{property_main}/.git")).unwrap();
    for trigger in [
        format!("git -C {property_main} reset --hard HEAD"),
        format!("rm {property_main}/src/main.txt"),
        format!("sed -i s/a/b/ {property_main}/src/main.txt"),
        format!("perl -pi -e s/a/b/ {property_main}/src/main.txt"),
        format!("cargo test --target-dir {root}/property/target-agent"),
    ] {
        for prefix in ["", "CHECK=1 ", "command ", "sudo ", "env CHECK=1 "] {
            for separator in ["", ";", "&&", "|"] {
                let command = if separator.is_empty() {
                    format!("{prefix}{trigger}")
                } else {
                    format!("printf ready {separator} {prefix}{trigger}")
                };
                cases.push((command.clone(), bash(command, &lane), true));
            }
        }
        for command in [
            format!("echo '{trigger}'"),
            format!("git commit -m '{trigger}'"),
            format!("cat <<'BODY'\n{trigger}\nBODY"),
        ] {
            cases.push((command.clone(), bash(command, &lane), false));
        }
    }
    Fixture {
        directory,
        root,
        cases,
    }
}
