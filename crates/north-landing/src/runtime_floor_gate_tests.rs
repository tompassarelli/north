use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::{Added, Floor, Gap, bash_runs, Invocation, Surface, Verdict, added_invocations, commands, invocations, judge, live_runtimes, oldest, refusals, violations};

const SERVE: &str = r#"
            ("--once", _) => once = true,
            ("--repo", Some(value)) => {}
            ("--checker", Some(value)) => {}
            ("--depth", Some(value)) if matches!(value.as_str(), "1" | "2") => {}
"#;
const SERVE_WITH_ALL: &str = r#"
            ("--once", _) => once = true,
            ("--all", _) => every = true,
            ("--repo", Some(value)) => {}
            ("--checker", Some(value)) => {}
"#;
const MODULE_BEFORE: &str = "  ExecStart = \"${northBin}/north land serve --repo ${homeDir}/code/north/main\";\n";
const MODULE_AFTER: &str = "  ExecStart = \"${northBin}/north land serve --all --repo ${homeDir}/code/north/main\";\n";
const SERVE_PATH: &str = "crates/north-landing/src/land/serve.rs";
const MODULE_PATH: &str = "modules/north-land/default.nix";

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(repo).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", "step"]);
    git(repo, &["rev-parse", "HEAD"])
}

fn init() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Floor"]);
    git(dir.path(), &["config", "user.email", "floor@example.invalid"]);
    dir
}

fn repo() -> (tempfile::TempDir, String) {
    let dir = init();
    let old = commit(dir.path(), &[("src/cli/mod.rs", "match command { \"language-gate\" => 1, _ => 0 }\n"), ("bin/safe-push", "#!/usr/bin/env bash\nnorth language-gate --repo .\n")]);
    (dir, old)
}

fn north() -> (tempfile::TempDir, String) {
    let dir = init();
    let old = commit(
        dir.path(),
        &[
            ("src/cli/tools.rs", "(\"land\", crate::land::run),\n(\"fleet\", crate::fleet::run),\n(\"config\", crate::agent_catalog::run),\n(\"watch\", crate::watch::run),\n"),
            (SERVE_PATH, SERVE),
            ("crates/north-landing/src/watch/mod.rs", "match arguments { [\"--once\"] => false, _ => true }\n"),
            ("crates/north-landing/src/land/mod.rs", "Some((command, rest)) if command == \"serve\" => serve::run(rest),\n"),
            ("crates/north-core/src/agent_catalog.rs", "#[path = \"catalog/registration_checks.rs\"]\nmod registration_checks;\n\"agents\" => 0,\n\"check-hook-paths\" => registration_checks::parse(rest),\n"),
            ("crates/north-core/src/catalog/registration_checks.rs", "\"--claude\" => 1, \"--codex\" => 2,\n"),
            ("crates/north-landing/src/land/queue_tests.rs", "(\"--all\", _) => {}\n"),
            ("crates/north-core/src/lanes/reap.rs", "\"--all\" => all = true,\n"),
        ],
    );
    (dir, old)
}

fn nixos() -> (tempfile::TempDir, String, String) {
    let dir = init();
    let base = commit(dir.path(), &[(MODULE_PATH, MODULE_BEFORE), ("flake.nix", "{ description = \"the machine\"; }\n")]);
    let head = commit(dir.path(), &[(MODULE_PATH, MODULE_AFTER)]);
    (dir, base, head)
}

fn state(current: &str, hosts: &[(&str, &str)]) -> tempfile::TempDir {
    let state = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{current}"), state.path().join("current")).unwrap();
    let rows: Vec<String> = hosts.iter().map(|(host, runtime)| format!(r#"{{"host": "{host}", "runtime": "{runtime}"}}"#)).collect();
    fs::write(state.path().join("runtime-settle.json"), format!(r#"{{"hosts": [{}]}}"#, rows.join(", "))).unwrap();
    state
}

fn words(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn call(text: &str) -> Invocation {
    invocations(text).remove(0)
}

#[test]
fn invocations_name_each_north_call_with_its_words_and_flags() {
    let nix = call(MODULE_AFTER.trim());
    assert_eq!((nix.words, nix.flags), (vec!["land".to_owned(), "serve".to_owned()], vec!["--all".to_owned(), "--repo".to_owned()]));
    let two = invocations("  north build-cache-gate --repo \"$root\" || x=$(north dep-gate --strict a b)");
    assert_eq!(two.iter().map(Invocation::path).collect::<Vec<_>>(), ["build-cache-gate", "dep-gate"]);
    assert_eq!(two[0].flags, ["--repo"]);
    assert_eq!(two[1].flags, ["--strict"]);
    assert_eq!(call("exec north land enqueue --to=main \"$@\"").flags, ["--to"]);
    assert_eq!(call("run = \"north fleet list --json; north land serve --once\";").flags, ["--json"]);
    assert_eq!(call("ExecStart = \"${pkgs.north}/bin/north health\";").words, ["health"]);
}

#[test]
fn invocations_ignore_comments_look_alike_paths_and_bare_north() {
    assert!(invocations("  # north build-cache-gate --all is new").is_empty());
    assert!(invocations("// north land serve --all").is_empty());
    assert!(invocations("bin/northstar go --all; ~/north go --all").is_empty());
    assert!(invocations("exec north --help").is_empty());
    assert!(invocations("description = \"north-land: the lander\";").is_empty());
}

#[test]
fn a_flag_counts_only_inside_the_commands_own_non_test_modules() {
    let surface = Surface::from_parts(
        &["land", "fleet"],
        &["crates/north-landing/src/land/serve.rs", "crates/north-core/src/lanes/reap.rs", "src/cli/fleet/list.rs"],
        &[(SERVE_PATH, "--once"), (SERVE_PATH, "--repo"), ("crates/north-core/src/lanes/reap.rs", "--all"), ("src/cli/fleet/list.rs", "--all")],
    );
    assert_eq!(surface.gaps(&call("north land serve --all --repo x")), [Gap::Flag { command: "land".to_owned(), flag: "--all".to_owned() }]);
    assert!(surface.gaps(&call("north land serve --once --help")).is_empty());
    assert!(surface.gaps(&call("north fleet list --all")).is_empty());
    assert_eq!(surface.gaps(&call("north gone --all")), [Gap::Command("gone".to_owned())]);
    assert!(Surface::from_parts(&["odd"], &[], &[]).gaps(&call("north odd --anything")).is_empty());
}

#[test]
fn a_flag_parsed_in_a_module_the_commands_handler_names_is_accepted() {
    let (dir, old) = north();
    let surface = Surface::read(dir.path(), &old, &words(&["config", "agents", "check-hook-paths"]));
    assert!(surface.gaps(&call("north config agents check-hook-paths --claude --codex")).is_empty());
    assert_eq!(surface.gaps(&call("north config agents check-hook-paths --all")).len(), 1);
    let watch = Surface::read(dir.path(), &old, &words(&["watch"]));
    assert!(watch.gaps(&call("north watch --once")).is_empty());
    assert_eq!(watch.gaps(&call("north watch --all")).len(), 1);
}

#[test]
fn the_surface_reads_flag_literals_from_source_and_skips_test_files() {
    let (dir, old) = north();
    let surface = Surface::read(dir.path(), &old, &words(&["land", "serve"]));
    assert!(surface.commands().contains("land"));
    assert!(surface.gaps(&call("north land serve --once --depth 2 --repo x --checker y")).is_empty());
    assert_eq!(surface.gaps(&call("north land serve --all")), [Gap::Flag { command: "land".to_owned(), flag: "--all".to_owned() }]);
    let with_all = commit(dir.path(), &[(SERVE_PATH, SERVE_WITH_ALL)]);
    assert!(Surface::read(dir.path(), &with_all, &words(&["land", "serve"])).gaps(&call("north land serve --all")).is_empty());
}

#[test]
fn the_2026_11_05_0206_north_land_serve_all_change_is_refused_until_every_live_runtime_accepts_it() {
    let (north_dir, old) = north();
    let (nixos_dir, base, head) = nixos();
    let added = added_invocations(nixos_dir.path(), &base, &head).unwrap();
    assert_eq!(added.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), [MODULE_PATH]);

    let lagging = state(&old, &[("gex45", &old)]);
    let Verdict::Refused(lines) = judge(nixos_dir.path(), Some(north_dir.path()), lagging.path(), &base, &head).unwrap() else { panic!("the invocation went live before north had --all") };
    let short = &old[..8];
    assert_eq!(lines[0], format!("runtime-floor-gate: {MODULE_PATH}: north land serve --all is missing from the oldest live runtime {short} (gex45)"));

    let with_all = commit(north_dir.path(), &[(SERVE_PATH, SERVE_WITH_ALL)]);
    let half = state(&with_all, &[("whiterabbit", &with_all), ("gex45", &old)]);
    let Verdict::Refused(lines) = judge(nixos_dir.path(), Some(north_dir.path()), half.path(), &base, &head).unwrap() else { panic!("gex45 still runs a runtime that exits 64 on --all") };
    assert!(lines[0].ends_with(&format!("the oldest live runtime {short} (gex45)")), "{lines:?}");

    let settled = state(&with_all, &[("whiterabbit", &with_all), ("gex45", &with_all)]);
    assert_eq!(judge(nixos_dir.path(), Some(north_dir.path()), settled.path(), &base, &head).unwrap(), Verdict::Passed);
}

#[test]
fn a_flag_north_main_itself_lacks_is_refused_even_when_no_runtime_is_known() {
    let (north_dir, _) = north();
    let (nixos_dir, base, head) = nixos();
    let unknown = tempfile::tempdir().unwrap();
    let Verdict::Refused(lines) = judge(nixos_dir.path(), Some(north_dir.path()), unknown.path(), &base, &head).unwrap() else { panic!("north main has no --all either") };
    assert!(lines[0].contains("north land serve --all") && lines[0].contains("(north main)"), "{lines:?}");
}

#[test]
fn prose_naming_an_unknown_north_command_and_guarded_calls_pass() {
    let added = |script: &str| vec![Added { path: "a.nix".to_owned(), script: script.to_owned(), invocations: invocations(script) }];
    let floor = Surface::from_parts(&["land"], &[], &[]);
    let head = Surface::from_parts(&["land", "fresh"], &[], &[]);
    assert!(violations(&added("note = \"north is a harness\";\nnorth star --all"), false, &head, &floor).is_empty());
    assert!(violations(&added("north fresh --x"), false, &head, &floor).iter().any(|line| line == "a.nix: north fresh"));
    assert!(violations(&added("north fresh --help >/dev/null || exit 1\nnorth fresh --x"), false, &head, &floor).is_empty());
}

#[test]
fn a_change_with_no_north_call_needs_no_north_source() {
    let dir = init();
    let base = commit(dir.path(), &[("flake.nix", "{ }\n")]);
    let head = commit(dir.path(), &[("flake.nix", "{ description = \"the machine, rebuilt\"; }\n")]);
    let missing = dir.path().join("no-such-north");
    let state = tempfile::tempdir().unwrap();
    assert_eq!(judge(dir.path(), Some(&missing), state.path(), &base, &head).unwrap(), Verdict::Passed);
}

#[test]
fn a_north_call_without_a_readable_north_source_is_refused_with_the_fix() {
    let (nixos_dir, base, head) = nixos();
    let missing = nixos_dir.path().join("no-such-north");
    let state = tempfile::tempdir().unwrap();
    let error = judge(nixos_dir.path(), Some(&missing), state.path(), &base, &head).unwrap_err();
    assert!(error.contains("cannot read north's source") && error.contains("NORTH_HOME"), "{error}");
}

#[test]
fn a_north_landing_is_refused_only_for_what_its_own_change_adds_beyond_the_floor() {
    let (dir, old) = north();
    commit(dir.path(), &[("src/cli/mod.rs", "match command { \"land\" => 1, _ => 0 }\n")]);
    let base = git(dir.path(), &["rev-parse", "HEAD"]);
    let head = commit(dir.path(), &[(SERVE_PATH, SERVE_WITH_ALL), ("bin/lander", "#!/usr/bin/env bash\nnorth land serve --all --once\nnorth land serve --depth 2\n")]);
    let added = added_invocations(dir.path(), &base, &head).unwrap();
    let (new, floor) = (Surface::read(dir.path(), &head, &words(&["land", "serve"])), Surface::read(dir.path(), &old, &words(&["land", "serve"])));
    assert_eq!(violations(&added, true, &new, &floor), ["bin/lander: north land serve --all"]);
    assert!(violations(&added, true, &floor, &floor).is_empty());
    assert!(violations(&added, true, &new, &new).is_empty());
}

#[test]
fn a_gap_every_runtime_and_north_main_share_is_not_a_floor_problem_for_a_north_landing() {
    let floor = Surface::from_parts(&["land"], &["a/land/x.rs"], &[]);
    let call = call("north land serve --nowhere");
    assert!(refusals(true, &floor, &floor, &call).is_empty());
    assert_eq!(refusals(false, &floor, &floor, &call).len(), 1);
}

#[test]
fn a_script_calling_a_subcommand_landed_in_the_same_change_is_refused_against_its_base() {
    let (dir, old) = repo();
    let head = commit(
        dir.path(),
        &[
            ("src/cli/mod.rs", "match command { \"language-gate\" => 1, \"build-cache-gate\" => 2, _ => 0 }\n"),
            ("bin/safe-push", "#!/usr/bin/env bash\nnorth language-gate --repo .\nnorth build-cache-gate --repo .\n"),
        ],
    );
    assert!(commands(dir.path(), &head).contains("build-cache-gate"));
    let state = tempfile::tempdir().unwrap();
    let Verdict::Refused(lines) = judge(dir.path(), None, state.path(), &old, &head).unwrap() else { panic!("refused") };
    assert!(lines[0].contains("bin/safe-push: north build-cache-gate is missing") && lines[0].contains("the landing's base"), "{lines:?}");
}

#[test]
fn a_call_is_refused_while_any_live_runtime_predates_its_subcommand() {
    let (dir, old) = repo();
    let added = commit(dir.path(), &[("src/cli/mod.rs", "match command { \"language-gate\" => 1, \"build-cache-gate\" => 2, _ => 0 }\n")]);
    let head = commit(dir.path(), &[("bin/safe-push", "#!/usr/bin/env bash\nnorth build-cache-gate --repo .\n")]);
    let record = format!(r#"{{"hosts": [{{"host": "gex45", "runtime": "{old}"}}, {{"host": "nexus", "runtime": null}}, {{"host": "gone", "runtime": "{}"}}]}}"#, "f".repeat(40));
    let state = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(format!("runtimes/{added}"), state.path().join("current")).unwrap();
    fs::write(state.path().join("runtime-settle.json"), record).unwrap();
    let live = live_runtimes(state.path());
    assert_eq!(live.len(), 3);
    let fallback = || Floor { sha: added.clone(), host: "the landing's base".to_owned() };
    assert_eq!(oldest(dir.path(), live, fallback()), Floor { sha: old, host: "gex45".to_owned() });
    assert!(matches!(judge(dir.path(), None, state.path(), &added, &head).unwrap(), Verdict::Refused(_)));
    let caught_up = oldest(dir.path(), vec![Floor { sha: added.clone(), host: "gex45".to_owned() }, Floor { sha: head.clone(), host: "this host".to_owned() }], fallback());
    assert_eq!(caught_up.sha, added);
    let settled = state_with(&added, &head);
    assert_eq!(judge(dir.path(), None, settled.path(), &added, &head).unwrap(), Verdict::Passed);
}

fn state_with(current: &str, remote: &str) -> tempfile::TempDir {
    state(current, &[("gex45", remote)])
}

#[test]
fn a_probed_call_and_a_call_outside_scripts_pass() {
    let (dir, old) = repo();
    let head = commit(
        dir.path(),
        &[
            ("src/cli/mod.rs", "match command { \"language-gate\" => 1, \"build-cache-gate\" => 2, \"runtime-floor-gate\" => 3, _ => 0 }\n"),
            ("bin/safe-push", "#!/usr/bin/env bash\nnorth build-cache-gate --help >/dev/null 2>&1 || exit 1\nnorth build-cache-gate --repo .\nnorth_offers runtime-floor-gate && north runtime-floor-gate --repo .\n"),
            ("docs/features.md", "north build-cache-gate --repo DIR\n"),
        ],
    );
    let state = tempfile::tempdir().unwrap();
    assert_eq!(judge(dir.path(), None, state.path(), &old, &head).unwrap(), Verdict::Passed);
}

const JOB_RUN_PATH: &str = "modules/job-run/default.nix";
const LIVENESS_PATH: &str = "modules/delivery-liveness/default.nix";
const HEALTH_PATH: &str = "modules/code-health/default.nix";

fn floor_north() -> (tempfile::TempDir, String) {
    let dir = init();
    let old = commit(
        dir.path(),
        &[
            ("src/cli/tools.rs", "(\"land\", crate::land::run),\n(\"code-health\", crate::code_health::run),\n"),
            ("crates/north-core/src/code_health/mod.rs", "match command { \"report\" => report(), _ => usage() }\n"),
        ],
    );
    commit(
        dir.path(),
        &[
            ("src/cli/tools.rs", "(\"land\", crate::land::run),\n(\"code-health\", crate::code_health::run),\n(\"job-run\", crate::job_run::run),\n"),
            ("crates/north-core/src/code_health/mod.rs", "match command { \"report\" => report(), \"tick\" => tick(), _ => usage() }\n"),
        ],
    );
    (dir, old)
}

fn replay(files: &[(&str, &str, &str)]) -> (tempfile::TempDir, String, String) {
    let dir = init();
    let before: Vec<(&str, &str)> = files.iter().map(|(path, before, _)| (*path, *before)).collect();
    let after: Vec<(&str, &str)> = files.iter().map(|(path, _, after)| (*path, *after)).collect();
    let base = commit(dir.path(), &before);
    let head = commit(dir.path(), &after);
    (dir, base, head)
}

fn refused(repo: &Path, north: &Path, old: &str, base: &str, head: &str) -> Vec<String> {
    let live = state(old, &[("laptop", old)]);
    let Verdict::Refused(lines) = judge(repo, Some(north), live.path(), base, head).unwrap() else { panic!("the gate passed a verb the floor runtime lacks") };
    lines
}

#[test]
fn runtime_floor_refuses_the_e7619134_job_run_through_a_nix_northbin_path() {
    let (north_dir, old) = floor_north();
    let (dir, base, head) = replay(&[
        (
            JOB_RUN_PATH,
            "  ExecStart = \"${pkgs.bash}/bin/bash ${northBin}/job-run --path ${\n    recordPath\n  } --user ${username} ${name} -- ${cmd}\";\n",
            "  ExecStart = \"${pkgs.bash}/bin/bash ${northBin}/north job-run --path ${\n    recordPath\n  } --user ${username} ${name} -- ${cmd}\";\n",
        ),
        (
            LIVENESS_PATH,
            "  jobRun = name: cmd:\n    \"${pkgs.bash}/bin/bash ${northBin}/job-run --path ${recordPath} --user ${username} ${name} -- ${cmd}\";\n",
            "  jobRun = name: cmd:\n    \"${pkgs.bash}/bin/bash ${northBin}/north job-run --path ${recordPath} --user ${username} ${name} -- ${cmd}\";\n",
        ),
    ]);
    let lines = refused(dir.path(), north_dir.path(), &old, &base, &head);
    assert!(lines.iter().any(|line| line.starts_with(&format!("runtime-floor-gate: {JOB_RUN_PATH}: north job-run is missing"))), "{lines:?}");
    assert!(lines.iter().any(|line| line.starts_with(&format!("runtime-floor-gate: {LIVENESS_PATH}: north job-run is missing"))), "{lines:?}");
    assert!(lines.iter().any(|line| line.contains("bash cannot run the north binary") && line.ends_with("north job-run")), "{lines:?}");
}

#[test]
fn runtime_floor_refuses_the_15158378_code_health_tick_the_floor_runtime_lacks() {
    let (north_dir, old) = floor_north();
    let (dir, base, head) = replay(&[(
        HEALTH_PATH,
        "  ExecStart = jobRun \"code-health-review\" \"${northBin}/code-health-tick\";\n",
        "  ExecStart = jobRun \"code-health-review\" \"${northBin}/north code-health tick\";\n",
    )]);
    let lines = refused(dir.path(), north_dir.path(), &old, &base, &head);
    assert!(lines.iter().any(|line| line.starts_with(&format!("runtime-floor-gate: {HEALTH_PATH}: north code-health tick is missing"))), "{lines:?}");
}

#[test]
fn a_north_path_run_through_bash_is_named_and_a_direct_call_is_not() {
    let nix = call("ExecStart = \"${pkgs.bash}/bin/bash ${northBin}/north job-run --path x\";");
    assert!(nix.via_bash);
    assert!(call("ExecStart = \"bash /opt/bin/north job-run\";").via_bash);
    assert!(!call("ExecStart = \"${northBin}/north job-run --path x\";").via_bash);
    assert!(!call("bash -c 'north job-run'").via_bash);
    let added = vec![Added { path: JOB_RUN_PATH.to_owned(), script: String::new(), invocations: vec![nix] }];
    assert_eq!(bash_runs(&added), [format!("runtime-floor-gate: {JOB_RUN_PATH}: bash cannot run the north binary; call it directly as north job-run")]);
}
