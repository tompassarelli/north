use super::{manifest_findings, source_findings};

const LIB: &str = "#![no_std]\n#![forbid(unsafe_code)]\nextern crate alloc;\npub mod admission;\n";

pub(in crate::land::scans) fn assert_a_strategy_that_could_reach_the_host_is_refused_and_a_sealed_one_passes() {
    assert_eq!(source_findings("lib.rs", LIB, true), Vec::<String>::new());
    assert_eq!(source_findings("admission.rs", "use alloc::string::String;\nuse core::cmp::Ordering; // not std::time\n", false), Vec::<String>::new());
    assert_eq!(source_findings("lib.rs", "#![forbid(unsafe_code)]\n", true), ["lib.rs lacks #![no_std]"]);
    let opened = source_findings("lib.rs", "#![no_std]\n#![forbid(unsafe_code)]\nextern crate std;\nuse std::time::Instant;\nstatic CALLS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);\nconst HOST: &str = include_str!(\"/etc/hostname\");\n", true);
    for hit in ["`extern crate std`", "`std::`", "`AtomicU64`", "`include_str!`"] {
        assert!(opened.iter().any(|finding| finding.contains(hit)), "{hit} slipped through: {opened:?}");
    }
}

pub(in crate::land::scans) fn assert_a_dependency_outside_the_allowlist_or_a_build_script_is_refused() {
    let allowed = ["serde"];
    assert_eq!(manifest_findings("[package]\nname = \"x\"\n[dependencies]\nserde = { version = \"1\", default-features = false }\n", &allowed), Vec::<String>::new());
    let manifest = "[package]\nname = \"x\"\nbuild = \"build.rs\"\n[dependencies]\nserde = \"1\"\nchrono = \"0.4\"\n[dev-dependencies]\ntempfile = \"3\"\n";
    let found = manifest_findings(manifest, &allowed);
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found.iter().any(|finding| finding.contains("chrono")) && found.iter().any(|finding| finding.contains("tempfile")) && found.iter().any(|finding| finding.contains("build script")));
}
