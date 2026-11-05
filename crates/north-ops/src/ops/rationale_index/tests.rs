use super::*;
use crate::ops::docs::run;

fn fixture(notes: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join(INDEX.source_dir);
    fs::create_dir_all(source.join("watch")).unwrap();
    for (name, text) in notes {
        fs::write(source.join(name), text).unwrap();
    }
    dir
}

const NOTES: [(&str, &str); 3] = [
    ("b-note.md", "# Beta: the second\n\nMeasured on 2026-10-31 at 1.5 s. The rest\nis detail.\n"),
    ("a-note.md", "# Alpha\n\n## Context\n\nTom asked for e.g.x a thing,\nacross two lines? Then more.\n"),
    ("notes.txt", "# Ignored\n\nNot markdown.\n"),
];

#[test]
fn render_lists_each_note_by_name_with_its_title_link_and_first_sentence() {
    let dir = fixture(&NOTES);
    let text = render(&dir.path().join(INDEX.source_dir)).unwrap();
    let lines = text.strip_prefix(HEADER).unwrap();
    assert_eq!(lines, "\n- [Alpha](a-note.md): Tom asked for e.g.x a thing, across two lines?\n- [Beta: the second](b-note.md): Measured on 2026-10-31 at 1.5 s.\n");
}

#[test]
fn the_generated_readme_is_not_listed_and_check_goes_stale_when_a_note_is_added() {
    let dir = fixture(&NOTES);
    let root = dir.path().display().to_string();
    assert_eq!(run(&["rationale-index".into(), "--root".into(), root.clone()]), 0);
    assert_eq!(run(&["rationale-index".into(), "--check".into(), "--root".into(), root.clone()]), 0);
    assert!(!fs::read_to_string(dir.path().join(INDEX.file)).unwrap().contains("(README.md)"));
    fs::write(dir.path().join(INDEX.source_dir).join("c-note.md"), "# Gamma\n\nThird.\n").unwrap();
    assert!(INDEX.check(dir.path()).unwrap_err().contains("stale"));
    assert_eq!(run(&["rationale-index".into(), "--check".into(), "--root".into(), root]), 1);
}

#[test]
fn a_note_without_a_title_or_a_paragraph_is_refused_by_name() {
    let dir = fixture(&[("untitled.md", "No title.\n")]);
    assert!(render(&dir.path().join(INDEX.source_dir)).unwrap_err().contains("untitled.md"));
    let dir = fixture(&[("empty.md", "# Only a title\n\n## And a heading\n")]);
    assert!(render(&dir.path().join(INDEX.source_dir)).unwrap_err().contains("empty.md"));
}
