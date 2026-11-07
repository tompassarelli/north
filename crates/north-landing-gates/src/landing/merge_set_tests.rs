use super::*;
use north_base::test_inputs::{Generator, cases};

fn texts(side: &[Entry]) -> BTreeSet<String> {
    side.iter().map(|entry| entry.text.clone()).collect()
}

fn side(generator: &mut Generator, base: &[Entry], fresh: &[String], removable: &[usize]) -> (Vec<Entry>, BTreeSet<String>, BTreeSet<String>) {
    let mut out = base.to_vec();
    let mut removed = BTreeSet::new();
    let mut added = BTreeSet::new();
    for &at in removable {
        if generator.index(3) == 0 {
            removed.insert(base[at].text.clone());
        }
    }
    out.retain(|entry| !removed.contains(&entry.text));
    for word in fresh {
        if generator.index(3) == 0 {
            added.insert(word.clone());
            out.insert(generator.index(out.len() + 1), line_entry(word));
        }
    }
    (out, added, removed)
}

#[test]
fn random_add_and_remove_pairs_apply_both_sides_exactly_in_either_order() {
    let mut generator = Generator::new(0x9e37_79b9_7f4a_7c15);
    for case in 0..cases() {
        let mut words = BTreeSet::new();
        while words.len() < 14 {
            words.insert(generator.word());
        }
        let words = words.into_iter().collect::<Vec<_>>();
        let base = words[..6].iter().map(|word| line_entry(word)).collect::<Vec<_>>();
        let (ours_fresh, theirs_fresh) = (&words[6..10], &words[10..14]);
        let removable = (0..base.len()).collect::<Vec<_>>();
        let (mine, my_added, my_removed) = side(&mut generator, &base, ours_fresh, &removable);
        let shared = removable.iter().copied().filter(|at| !my_removed.contains(&base[*at].text)).collect::<Vec<_>>();
        let (theirs, their_added, their_removed) = side(&mut generator, &base, theirs_fresh, &shared);
        let merged = merge_entries(&base, &mine, &theirs).unwrap_or_else(|error| panic!("case {case}: {error}"));
        let mut expected = texts(&base);
        expected.retain(|word| !my_removed.contains(word) && !their_removed.contains(word));
        expected.extend(my_added.union(&their_added).cloned());
        assert_eq!(texts(&merged), expected, "case {case}");
        assert_eq!(merged.len(), expected.len(), "case {case}: no entry repeats");
        assert_eq!(merged, merge_entries(&base, &theirs, &mine).unwrap(), "case {case}: the order of the sides changes nothing");
    }
}

const BASE: &str = r#"[[module]]
name = "foundation"
may_use = []

[[module]]
name = "guard"
paths = ["crates/north-core/src/guard/**", "crates/north-core/src/hook.rs"]
responsibility = "Tool-call policy over provider-neutral ToolCall values."
may_use = ["ledger", "autoland_status", "shell", "domain", "platform", "foundation", "crate-roots"]

[[registered]]
id = "guard.clipboard"
path = "crates/north-core/src/guard/clipboard.rs"
"#;

fn guard(uses: &str) -> String {
    BASE.replace(r#"["ledger", "autoland_status", "shell", "domain", "platform", "foundation", "crate-roots"]"#, uses)
}

#[test]
fn the_structure_toml_conflict_of_the_fleet_and_spawn_capacity_landings_replays_to_the_hand_resolution() {
    let ours = guard(r#"["ledger", "fleet", "autoland_status", "shell", "domain", "platform", "foundation", "crate-roots"]"#);
    let theirs = guard(r#"["ledger", "autoland_status", "ops", "usage", "shell", "domain", "platform", "foundation", "crate-roots"]"#)
        + "\n[[registered]]\nid = \"guard.spawn_capacity\"\npath = \"crates/north-core/src/guard/spawn_capacity.rs\"\n";
    let (_, textual_conflict) = text_merge(BASE, &ours, &theirs).unwrap();
    assert!(textual_conflict, "plain git conflicts on this pair");
    let resolved = merge_arrays(BASE, &ours, &theirs, "may_use").unwrap();
    let by_hand = guard(r#"["ledger", "autoland_status", "shell", "domain", "platform", "foundation", "crate-roots", "fleet", "ops", "usage"]"#)
        + "\n[[registered]]\nid = \"guard.spawn_capacity\"\npath = \"crates/north-core/src/guard/spawn_capacity.rs\"\n";
    assert_eq!(resolved, by_hand);
    assert_eq!(merge_arrays(BASE, &theirs, &ours, "may_use").unwrap(), by_hand);
}

#[test]
fn a_first_word_line_file_merges_a_lane_that_inherited_a_duplicate_main_has_since_removed() {
    let base = "a north\nb north-threads\nb north-threads north-ops\n";
    let ours = "a north\nb north-threads north-ops\n";
    let theirs = "a north\nb north-threads\nb north-threads north-ops\nc north\n";
    assert_eq!(merge_lines(base, ours, theirs, Identity::FirstWord).unwrap(), "a north\nb north-threads north-ops\nc north\n");
    assert_eq!(merge_lines(base, base, "a north\nb north-threads\nb north-threads north-ops\nc north\n", Identity::FirstWord).unwrap(), "a north\nb north-threads\nb north-threads north-ops\nc north\n");
}
