use std::fs;

use super::refusal;

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

/// The `[buck2_re_client]` keys of Buck2 2026-04-15 and the files it reads them from (its buckconfig documentation).
#[test]
fn a_buck2_command_on_the_laptop_is_refused_with_one_line_once_anything_names_the_store() {
    let machine = tempfile::tempdir().unwrap();
    let (lane, etc, home) = (machine.path().join("lane"), machine.path().join("etc"), machine.path().join("home"));
    for directory in [&lane, &etc.join("buckconfig.d"), &home] {
        fs::create_dir_all(directory).unwrap();
    }
    fs::write(lane.join(".buckconfig"), "[cells]\n  root = .\n[buck2_re_client]\n  tls = false\n").unwrap();
    let build = words("env -u EMAIL buck2 build //...");
    let refused = |laptop: bool, command: &[String]| refusal(laptop, command, &lane, &etc, &home);
    assert_eq!(refused(true, &build), None, "with no service named, the laptop's own buck2 runs, and builds locally");

    let client = etc.join("buckconfig.d/remote-execution");
    fs::write(&client, "[buck2_re_client]\n  engine_address = grpc://10.0.0.1:8980\n").unwrap();
    let line = refused(true, &build).expect("the laptop started a client of the store its system configuration named");
    assert!(line.contains(&client.display().to_string()) && !line.contains('\n'), "{line}");
    assert_eq!(refused(false, &build), None, "a build host, which is a client of the store, was refused");
    assert_eq!(refused(true, &words("cargo test -p north-capacity")), None, "a command that runs no buck2 was refused");

    fs::remove_file(&client).unwrap();
    assert!(refused(true, &words("buck2 build --config buck2_re_client.cas_address=grpc://10.0.0.1:8980 //...")).is_some(), "an address given on the command line started a client");
    fs::write(lane.join(".buckconfig.local"), "[buck2_re_client]\naction_cache_address=grpc://10.0.0.1:8980\n").unwrap();
    assert!(refused(true, &["bash", "-c", "buck2 build //..."].map(str::to_owned)).is_some(), "an address in the lane's local file started a client");
}
