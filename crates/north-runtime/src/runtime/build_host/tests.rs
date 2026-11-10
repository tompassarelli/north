use super::*;

const TARGET: &str = "1111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222";

fn host(name: &str, free_cpus: Option<f64>) -> BuildHost {
    BuildHost { name: name.to_owned(), ssh: format!("tom@{name}"), free_cpus }
}

fn fleet() -> Vec<BuildHost> {
    vec![host("gex45", Some(4.0)), host("cpx62", Some(12.0)), host("cpx62b", None)]
}

fn built_for(commit: &'static str) -> impl FnOnce(&BuildHost, &str, &Path) -> Result<(), Failure> {
    move |_, _, runtime| {
        fs::create_dir_all(runtime)?;
        for program in PROGRAMS {
            fs::write(runtime.join(program), format!("{program} built for {commit}"))?;
        }
        layout::stamp(runtime, commit)?;
        Ok(())
    }
}

#[test]
fn a_runtime_with_no_artifact_builds_on_the_freest_host() {
    let state = tempfile::tempdir().unwrap();
    let layout = Layout { state: state.path().to_path_buf() };
    let runtime = layout.runtime(TARGET);
    let mut asked = Vec::new();
    let build_on = |host: &BuildHost, target: &str, runtime: &Path| {
        asked.push(host.name.clone());
        built_for(TARGET)(host, target, runtime)
    };
    let built = build(TARGET, &runtime, fleet(), build_on).unwrap();
    assert_eq!(asked.as_slice(), ["cpx62"], "cpx62 had the most free cores of the hosts that answered");
    assert_eq!(built.host, "cpx62");
    assert!(layout::usable(&runtime, true), "the settling host promotes a runtime it had a build host build");
    let marker = fs::read_to_string(runtime.join(BUILT_ON)).unwrap();
    assert_eq!(marker, format!("cpx62 {}\n", built.binary));

    let unanswered = layout.runtime(OTHER);
    let refused = build(OTHER, &unanswered, vec![host("gex45", None), host("cpx62", None)], |_: &BuildHost, _: &str, _: &Path| panic!("no build host answered")).err().unwrap();
    assert!(refused.contains("no build host answered") && refused.contains("gex45") && refused.contains("cpx62"), "{refused}");
    assert!(!layout::usable(&unanswered, false), "with no build host answering, the caller builds here");

    let refused = build(OTHER, &unanswered, fleet(), built_for(TARGET)).err().unwrap();
    assert!(refused.contains("cpx62 did not build") && refused.contains("not built from its own commit"), "{refused}");
    assert!(!layout::has_programs(&unanswered) && !unanswered.join(BUILT_ON).exists(), "a build stamped for another commit is not taken");
}
