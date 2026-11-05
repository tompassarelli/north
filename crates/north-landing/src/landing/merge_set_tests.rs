use super::*;
use north_base::test_inputs::{Generator, cases};

fn entries(words: &[&str]) -> Vec<Entry> {
    words.iter().map(|word| line_entry(word)).collect()
}

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

#[test]
fn both_sides_removing_the_same_entry_removes_it_once() {
    let base = entries(&["a", "b", "c"]);
    let merged = merge_entries(&base, &entries(&["a", "c"]), &entries(&["a", "c", "d"])).unwrap();
    assert_eq!(merged, entries(&["a", "c", "d"]));
    assert_eq!(merge_entries(&base, &entries(&["a", "c"]), &entries(&["a", "c"])).unwrap(), entries(&["a", "c"]));
}

#[test]
fn one_side_removing_what_the_other_edits_is_refused() {
    let base = entries(&["tests/a.sh # old", "tests/b.sh"]);
    let removes = entries(&["tests/b.sh"]);
    let edits = entries(&["tests/a.sh # new", "tests/b.sh", "tests/c.sh"]);
    assert!(merge_entries(&base, &removes, &edits).unwrap_err().contains("disagree"));
    assert!(merge_entries(&base, &edits, &removes).unwrap_err().contains("disagree"));
    let other = entries(&["tests/a.sh # other", "tests/b.sh"]);
    assert!(merge_entries(&base, &edits, &other).is_err());
}

#[test]
fn a_duplicate_is_refused_whether_one_side_repeats_it_or_both_add_it() {
    let base = entries(&["a", "b"]);
    assert!(merge_entries(&base, &entries(&["a", "b", "c"]), &entries(&["a", "b", "c", "d"])).unwrap_err().contains("duplicate"));
    assert!(merge_entries(&base, &entries(&["a", "b", "b"]), &entries(&["a", "b", "z"])).unwrap_err().contains("duplicate"));
    assert!(merge_entries(&base, &entries(&["a", "b"]), &entries(&["a", "a", "b"])).is_err());
}

#[test]
fn a_sorted_list_stays_sorted_and_an_unsorted_one_gets_additions_at_the_end() {
    let sorted = merge_entries(&entries(&["b", "d"]), &entries(&["a", "b", "d"]), &entries(&["b", "c", "d"])).unwrap();
    assert_eq!(sorted, entries(&["a", "b", "c", "d"]));
    let loose = merge_entries(&entries(&["z", "a"]), &entries(&["z", "a", "q"]), &entries(&["z", "a", "m"])).unwrap();
    assert_eq!(loose, entries(&["z", "a", "m", "q"]));
}

#[test]
fn two_sorted_sides_merge_sorted_even_over_an_unsorted_base() {
    let merged = merge_entries(&entries(&["b", "a"]), &entries(&["a", "b", "c"]), &entries(&["a", "b", "d"])).unwrap();
    assert_eq!(merged, entries(&["a", "b", "c", "d"]), "a lane and main that both sort their list must not get additions appended because the merge base predates the sort");
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
fn an_array_edit_in_one_section_keeps_untouched_arrays_and_their_spacing() {
    let spaced = BASE.replace("may_use = []", "may_use = [\"x\",\"y\"]");
    let theirs = guard(r#"["ledger", "autoland_status", "shell", "domain", "platform", "foundation"]"#).replace("may_use = []", "may_use = [\"x\",\"y\"]");
    let ours = spaced.replace("\"y\"]", "\"y\",\"z\"]");
    let merged = merge_arrays(&spaced, &ours, &theirs, "may_use").unwrap();
    assert!(merged.contains("may_use = [\"x\",\"y\",\"z\"]"));
    assert!(merged.contains(r#"["ledger", "autoland_status", "shell", "domain", "platform", "foundation"]"#));
}

#[test]
fn a_section_dropped_by_one_side_while_the_other_edits_its_array_is_refused() {
    let dropped = BASE.replace(&BASE[BASE.find("[[module]]\nname = \"guard\"").unwrap()..BASE.find("[[registered]]").unwrap()], "");
    let edited = guard(r#"["ledger"]"#);
    assert!(merge_arrays(BASE, &dropped, &edited, "may_use").unwrap_err().contains("drops"));
}

#[test]
fn an_array_that_is_not_one_line_of_plain_strings_is_refused() {
    let multi = BASE.replace("may_use = []", "may_use = [\n  \"a\",\n]");
    assert!(merge_arrays(BASE, &multi, BASE, "may_use").unwrap_err().contains("one-line"));
}

#[test]
fn a_line_file_with_two_appends_merges_and_comments_may_not_interleave() {
    let base = "# header\nbin/a.sh\n";
    assert_eq!(merge_lines(base, "# header\nbin/a.sh\nbin/b.sh\n", "# header\nbin/a.sh\nbin/c.sh\n", Identity::Line).unwrap(), "# header\nbin/a.sh\nbin/b.sh\nbin/c.sh\n");
    assert!(merge_lines(base, "# header\nbin/a.sh\n# note\n", base, Identity::Line).is_err());
    assert!(merge_lines(base, "# one\nbin/a.sh\n", "# two\nbin/a.sh\n", Identity::Line).is_err());
}

fn scratch_files(dir: &Path, base: &str, ours: &str, theirs: &str) -> [PathBuf; 3] {
    let paths = ["base", "ours", "theirs"].map(|name| dir.join(name));
    for (path, text) in paths.iter().zip([base, ours, theirs]) {
        fs::write(path, text).unwrap();
    }
    paths
}

#[test]
fn the_driver_refuses_an_unlisted_path_and_leaves_markers_for_the_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("sets.toml");
    fs::write(&config, "[[set]]\npath = \"listed.txt\"\nkind = \"lines\"\n").unwrap();
    let [base, ours, theirs] = scratch_files(dir.path(), "a\n", "a\nb\n", "a\nc\n");
    let error = drive(&config, &base, &ours, &theirs, "hook.rs").unwrap_err();
    assert!(error.contains("allowlist"), "{error}");
    let text = fs::read_to_string(&ours).unwrap();
    assert!(text.contains("<<<<<<<") && text.contains(">>>>>>>"), "{text}");
    let [base, ours, theirs] = scratch_files(dir.path(), "a\n", "a\nb\n", "a\nc\n");
    drive(&config, &base, &ours, &theirs, "listed.txt").unwrap();
    assert_eq!(fs::read_to_string(&ours).unwrap(), "a\nb\nc\n");
}

#[test]
fn a_refused_duplicate_still_leaves_conflict_markers_though_git_alone_merges_it_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("sets.toml");
    fs::write(&config, "[[set]]\npath = \"listed.txt\"\nkind = \"lines\"\n").unwrap();
    let [base, ours, theirs] = scratch_files(dir.path(), "a\n", "a\nb\n", "a\nb\nc\n");
    assert!(drive(&config, &base, &ours, &theirs, "listed.txt").is_err());
    assert!(fs::read_to_string(&ours).unwrap().contains("<<<<<<< ours"));
}

#[test]
fn every_merge_attribute_has_a_driver() {
    let attributes = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.gitattributes")).unwrap();
    assert!(attributes.contains("merge=set"), "{attributes}");
    assert_eq!(undriven_attributes(&attributes), Vec::<String>::new());
    let planted = format!("{attributes}docs/new.md merge=nosuch\n");
    assert_eq!(undriven_attributes(&planted), vec!["nosuch".to_owned()]);
    assert_eq!(undriven_attributes("# a.txt merge=nosuch\nb.bin merge=binary\nc.log merge=union\n"), Vec::<String>::new());
}

#[test]
fn an_unknown_kind_is_skipped_and_structure_toml_still_merges_as_a_set() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("sets.toml");
    fs::write(&config, "[[set]]\npath = \"later.rs\"\nkind = \"rust-items\"\n\n[[set]]\npath = \"structure.toml\"\nkind = \"toml-array\"\nkey = \"may_use\"\n").unwrap();
    let rules = read_rules(&config).unwrap();
    assert_eq!(rules.iter().map(|rule| rule.path.as_str()).collect::<Vec<_>>(), ["structure.toml"]);
    let base = "[[module]]\nname = \"a\"\nmay_use = [\"x\"]\n";
    let [b, ours, theirs] = scratch_files(dir.path(), base, "[[module]]\nname = \"a\"\nmay_use = [\"x\", \"y\"]\n", "[[module]]\nname = \"a\"\nmay_use = [\"x\", \"z\"]\n");
    drive(&config, &b, &ours, &theirs, "structure.toml").unwrap();
    assert_eq!(fs::read_to_string(&ours).unwrap(), "[[module]]\nname = \"a\"\nmay_use = [\"x\", \"y\", \"z\"]\n");
    let [b, ours, theirs] = scratch_files(dir.path(), "a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n");
    drive(&config, &b, &ours, &theirs, "later.rs").unwrap();
    assert_eq!(fs::read_to_string(&ours).unwrap(), "A\nb\nC\n");
}

#[test]
fn a_regenerate_generator_change_ahead_defers_a_pick_on_its_sources_or_file_only() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("sets.toml");
    fs::write(&config, "[[set]]\npath = \"docs/index.md\"\nkind = \"regenerate\"\ncommand = \"north docs index\"\nsources = [\"docs/notes/\"]\ngenerator = [\"src/index/\"]\n").unwrap();
    let rules = read_rules(&config).unwrap();
    let paths = |paths: &[&str]| paths.iter().map(|path| (*path).to_owned()).collect::<Vec<_>>();
    let generator = paths(&["src/index/mod.rs"]);
    let deferred = generator_moved(&rules, &generator, &paths(&["docs/notes/new.md"])).unwrap();
    assert!(deferred.contains("docs/index.md"), "{deferred}");
    assert!(generator_moved(&rules, &generator, &paths(&["docs/index.md"])).is_some());
    assert!(generator_moved(&rules, &generator, &paths(&["src/other.rs"])).is_none());
    assert!(generator_moved(&rules, &paths(&["src/other.rs"]), &paths(&["docs/notes/new.md"])).is_none());
}

#[test]
fn the_drivers_run_the_quoted_program_with_their_merge_driver_arguments() {
    assert_eq!(shell_quoted(Path::new("/tmp/it's/north")), "'/tmp/it'\\''s/north'");
    assert!(DRIVERS.iter().all(|driver| driver.arguments.starts_with("merge-driver ") && driver.arguments.ends_with(" %O %A %B %P")));
}

#[test]
fn a_block_with_two_added_lines_merges_sorted_and_keeps_the_text_around_it() {
    let base = "use x;\nconst T: &[u8] = &[\n    (\"a\", A),\n    (\"c\", C),\n];\nfn f() {}\n";
    let ours = base.replace("    (\"c\"", "    (\"b\", B),\n    (\"c\"");
    let theirs = base.replace("];\n", "    (\"d\", D),\n];\n").replace("fn f() {}", "fn g() {}");
    let (start, end) = ("const T: &[u8] = &[", "];");
    let merged = block::merge_block(base, &ours, &theirs, start, end).unwrap();
    assert_eq!(merged, "use x;\nconst T: &[u8] = &[\n    (\"a\", A),\n    (\"b\", B),\n    (\"c\", C),\n    (\"d\", D),\n];\nfn g() {}\n");
    assert!(block::merge_block(base, &ours, &ours, start, end).unwrap_err().contains("both sides add"));
    assert!(block::merge_block(base, "const T: &[u8] = &[\n", base, start, end).unwrap_err().contains("closes"));
    assert!(block::merge_block(base, base, "use x;\n", start, end).unwrap_err().contains("opens"));
}

#[test]
fn two_keys_of_one_path_merge_as_one_rule_and_both_arrays_merge_as_sets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("sets.toml");
    fs::write(&config, "[[set]]\npath = \"structure.toml\"\nkind = \"toml-array\"\nkey = \"may_use\"\n\n[[set]]\npath = \"structure.toml\"\nkind = \"toml-array\"\nkey = \"paths\"\n\n[[set]]\npath = \"verbs.rs\"\nkind = \"block\"\nstart = \"[\"\nend = \"]\"\n").unwrap();
    let rules = read_rules(&config).unwrap();
    assert_eq!(rules[0].kind, Kind::TomlArray(vec!["may_use".to_owned(), "paths".to_owned()]));
    assert_eq!(rules[1].kind, Kind::Block { start: "[".to_owned(), end: "]".to_owned() });
    assert_eq!(rules.len(), 2);
    let base = "[[module]]\nname = \"a\"\npaths = [\"a/**\"]\nmay_use = [\"x\"]\n";
    let ours = "[[module]]\nname = \"a\"\npaths = [\"a/**\", \"b.rs\"]\nmay_use = [\"x\", \"y\"]\n";
    let theirs = "[[module]]\nname = \"a\"\npaths = [\"a/**\", \"c.rs\"]\nmay_use = [\"x\", \"z\"]\n";
    let [b, o, t] = scratch_files(dir.path(), base, ours, theirs);
    drive(&config, &b, &o, &t, "structure.toml").unwrap();
    assert_eq!(fs::read_to_string(&o).unwrap(), "[[module]]\nname = \"a\"\npaths = [\"a/**\", \"b.rs\", \"c.rs\"]\nmay_use = [\"x\", \"y\", \"z\"]\n");
}

#[test]
fn a_first_word_line_file_refuses_two_lines_for_one_key_instead_of_keeping_both() {
    let base = "a north\n";
    let refused = merge_lines(base, "a north\nb north-threads\n", "a north\nb north-threads north-ops\n", Identity::FirstWord).unwrap_err();
    assert!(refused.contains("\"b\""), "{refused}");
    assert_eq!(merge_lines(base, "a north\nb north\n", "a north\nc north\n", Identity::FirstWord).unwrap(), "a north\nb north\nc north\n");
    assert_eq!(merge_lines(base, "a north north-ops\n", base, Identity::FirstWord).unwrap(), "a north north-ops\n");
    assert_eq!(merge_lines(base, "a north\nb north-threads\n", "a north\nb north-threads north-ops\n", Identity::Line).unwrap(), "a north\nb north-threads\nb north-threads north-ops\n");
}
