use std::fs;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

const SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/harness/scripts/mutant-tests.sh");
const STUB: &str = "#!/usr/bin/env bash\n\
printf '%s %s start %s\\n' \"$CARGO_TARGET_DIR\" \"$CARGO_INCREMENTAL\" \"$(date +%s%N)\" >>\"$STUB_LOG\"\n\
sleep 1\n\
printf '%s %s end %s\\n' \"$CARGO_TARGET_DIR\" \"$CARGO_INCREMENTAL\" \"$(date +%s%N)\" >>\"$STUB_LOG\"\n\
printf 'test m::kills_it ... %s\\n' \"$(grep -q mutated src.txt && echo FAILED || echo ok)\"\n";

struct Machine {
    root: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("cargo"), STUB).unwrap();
        Command::new("chmod").arg("+x").arg(bin.join("cargo")).status().unwrap();
        let debug = root.path().join("cache/north/mutant-cargo/target/debug");
        fs::create_dir_all(&debug).unwrap();
        fs::write(debug.join("dep"), "warm").unwrap();
        for lane in ["a", "b", "c"] {
            for (tree, text) in [("change", "plain\n"), ("mutant", "mutated\n")] {
                fs::create_dir_all(root.path().join(lane).join(tree)).unwrap();
                fs::write(root.path().join(lane).join(tree).join("src.txt"), text).unwrap();
            }
        }
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.path().join(relative)
    }

    fn start(&self, lane: &str) -> Child {
        let trees = format!("{}\n{}", self.path(&format!("{lane}/change")).display(), self.path(&format!("{lane}/mutant")).display());
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap());
        Command::new("bash")
            .arg(SCRIPT)
            .current_dir(self.path(&format!("{lane}/change")))
            .env("PATH", path)
            .env("STUB_LOG", self.path("cargo.log"))
            .env("XDG_CACHE_HOME", self.path("cache"))
            .env("MUTANT_TESTS", "m::kills_it")
            .env("MUTANT_TREES", trees)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }

    fn verdicts(&self, lane: &str) -> (String, String) {
        let read = |tree: &str| fs::read_to_string(self.path(&format!("{lane}/{tree}/.mutant-results"))).unwrap();
        (read("change"), read("mutant"))
    }

    fn windows(&self) -> Vec<(String, String, u128, u128)> {
        let log = fs::read_to_string(self.path("cargo.log")).unwrap();
        let mut spans: Vec<(String, String, u128, u128)> = Vec::new();
        for line in log.lines() {
            let [target, incremental, kind, nanos] = line.split(' ').collect::<Vec<_>>()[..] else { panic!("{line}") };
            let nanos: u128 = nanos.parse().unwrap();
            match spans.iter_mut().find(|span| span.0 == target) {
                Some(span) if kind == "end" => span.3 = nanos,
                Some(_) => {}
                None => spans.push((target.to_owned(), incremental.to_owned(), nanos, nanos)),
            }
        }
        spans
    }
}

#[test]
fn concurrent_landings_take_separate_incremental_slots_seeded_from_a_warm_snapshot() {
    let machine = Machine::new();
    assert!(machine.start("a").wait().unwrap().success());
    assert!(!machine.path("cache/north/mutant-cargo/target").exists());
    assert_eq!(fs::read_to_string(machine.path("cache/north/mutant-cargo/slot-seed/target/debug/dep")).unwrap(), "warm");
    fs::remove_file(machine.path("cargo.log")).unwrap();

    let (mut b, mut c) = (machine.start("b"), machine.start("c"));
    assert!(b.wait().unwrap().success() && c.wait().unwrap().success());

    for lane in ["a", "b", "c"] {
        assert_eq!(machine.verdicts(lane), ("pass\tm::kills_it\n".to_owned(), "fail\tm::kills_it\n".to_owned()));
    }
    let slots = machine.windows();
    assert_eq!(slots.len(), 2, "{slots:?}");
    let used: Vec<&str> = slots.iter().map(|slot| slot.0.rsplit('/').nth(1).unwrap()).collect();
    assert!(used.contains(&"slot-0") && used.contains(&"slot-1"), "{used:?}");
    assert!(slots.iter().all(|slot| slot.1 == "1"), "{slots:?}");
    let (first, second) = (&slots[0], &slots[1]);
    assert!(first.2 < second.3 && second.2 < first.3, "slots ran one after the other: {slots:?}");
    assert_eq!(fs::read_to_string(machine.path("cache/north/mutant-cargo/slot-1/target/debug/dep")).unwrap(), "warm");
}

#[test]
fn a_bun_test_claim_runs_in_each_tree_and_records_its_verdict_without_cargo() {
    let root = tempfile::tempdir().unwrap();
    let name = "adds (1 + 1), then checks";
    for (tree, expected) in [("change", 2), ("mutant", 3)] {
        fs::create_dir_all(root.path().join(tree).join("harness")).unwrap();
        let source = format!("import {{ expect, test }} from \"bun:test\";\ntest(\"{name}\", () => expect(1 + 1).toBe({expected}));\n");
        fs::write(root.path().join(tree).join("harness/sum.test.ts"), source).unwrap();
    }
    let status = Command::new("bash")
        .arg(SCRIPT)
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("MUTANT_TESTS", name)
        .env("MUTANT_TREES", format!("{}\n{}", root.path().join("change").display(), root.path().join("mutant").display()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    let read = |tree: &str| fs::read_to_string(root.path().join(tree).join(".mutant-results")).unwrap();
    assert_eq!((read("change"), read("mutant")), (format!("pass\t{name}\n"), format!("fail\t{name}\n")));
    assert!(!root.path().join("cache/north/mutant-cargo/slot-0").exists(), "a bun-only claim takes no cargo slot");
}
