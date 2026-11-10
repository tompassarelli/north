use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::{References, apply, path};

fn stage(home: &Path, rows: &str) -> PathBuf {
    let directory = home.join("archive/plan");
    fs::create_dir_all(&directory).unwrap();
    let plan = directory.join("plan.tsv");
    fs::write(&plan, rows).unwrap();
    plan
}

fn references() -> References {
    References::from_parts(Vec::new(), Vec::new())
}

fn log(home: &Path) -> Option<String> {
    fs::read_to_string(home.join("archive/plan/moves.tsv")).ok()
}

#[test]
fn a_row_moves_one_entry_and_is_logged_before_it_is_reported_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/tidy")).unwrap();
    fs::write(home.join("notes.txt"), "notes\n").unwrap();
    let plan = stage(home, "notes.txt\tarchive/tidy/notes.txt\n");

    let (done, failed) = apply(&plan, home, false, &references());

    assert_eq!(done, ["moved: notes.txt -> archive/tidy/notes.txt"]);
    assert!(failed.is_empty(), "{failed:?}");
    assert!(!home.join("notes.txt").exists());
    assert_eq!(
        fs::read_to_string(home.join("archive/tidy/notes.txt")).unwrap(),
        "notes\n"
    );
    assert_eq!(
        log(home).as_deref(),
        Some("notes.txt\tarchive/tidy/notes.txt\n")
    );
}

#[test]
fn a_second_run_appends_to_the_log_and_never_replaces_an_existing_destination_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/tidy")).unwrap();
    fs::write(home.join("one.txt"), "one\n").unwrap();
    fs::write(home.join("two.txt"), "two\n").unwrap();
    fs::write(home.join("archive/tidy/two.txt"), "already\n").unwrap();
    let plan = stage(
        home,
        "one.txt\tarchive/tidy/one.txt\ntwo.txt\tarchive/tidy/two.txt\n",
    );

    let (done, failed) = apply(&plan, home, false, &references());

    assert_eq!(done, ["moved: one.txt -> archive/tidy/one.txt"]);
    assert_eq!(failed.len(), 1);
    assert!(failed[0].contains("already exists"), "{failed:?}");
    assert_eq!(
        fs::read_to_string(home.join("archive/tidy/two.txt")).unwrap(),
        "already\n"
    );
    assert_eq!(
        log(home).as_deref(),
        Some("one.txt\tarchive/tidy/one.txt\n")
    );
}

#[test]
fn dotfiles_dot_directories_git_checkouts_and_the_code_container_are_refused_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/tidy")).unwrap();
    fs::create_dir_all(home.join(".config")).unwrap();
    fs::write(home.join(".dotfile"), "x\n").unwrap();
    fs::create_dir_all(home.join("clone/.git")).unwrap();
    fs::create_dir_all(home.join("code")).unwrap();
    let plan = stage(
        home,
        ".dotfile\tarchive/tidy/.dotfile\n.config\tarchive/tidy/config\nclone\tarchive/tidy/clone\ncode\tarchive/tidy/code\nnotes.txt\tarchive/.hidden/notes.txt\n",
    );

    let (done, failed) = apply(&plan, home, false, &references());

    assert!(done.is_empty(), "{done:?}");
    assert_eq!(failed.len(), 5, "{failed:?}");
    for reason in [".dotfile", ".config"] {
        assert!(
            failed.iter().any(
                |line| line.starts_with(&format!("kept {reason}:")) && line.contains("dotfile")
            ),
            "{failed:?}"
        );
    }
    assert!(
        failed
            .iter()
            .any(|line| line.starts_with("kept clone:") && line.contains("git checkout")),
        "{failed:?}"
    );
    assert!(
        failed
            .iter()
            .any(|line| line.starts_with("kept code:") && line.contains("container")),
        "{failed:?}"
    );
    assert!(
        failed
            .iter()
            .any(|line| line.starts_with("kept notes.txt:") && line.contains("dotfile")),
        "{failed:?}"
    );
    assert!(home.join(".dotfile").exists());
}

#[test]
fn a_config_user_unit_or_live_process_that_names_the_source_or_destination_refuses_it_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/tidy")).unwrap();
    fs::write(home.join("cache"), "cache\n").unwrap();
    fs::write(home.join("books"), "books\n").unwrap();
    fs::write(home.join("pictures"), "pictures\n").unwrap();
    let plan = stage(
        home,
        "cache\tarchive/tidy/cache\nbooks\tarchive/tidy/books\npictures\tarchive/tidy/pictures\n",
    );
    let named = format!(
        "# unit\nExecStart=/usr/bin/tool --cache {}\n",
        home.join("cache").display()
    );
    let destination = format!(
        "target = \"{}\"\n",
        home.join("archive/tidy/books").display()
    );
    let references = References::from_parts(
        vec![named, destination],
        vec![home.join("pictures/current")],
    );

    let (done, failed) = apply(&plan, home, false, &references);

    assert!(done.is_empty(), "{done:?}");
    assert_eq!(failed.len(), 3, "{failed:?}");
    assert!(failed[0].contains("names it"), "{failed:?}");
    assert!(failed[1].contains("names the destination"), "{failed:?}");
    assert!(failed[2].contains("names it"), "{failed:?}");
    assert!(
        home.join("pictures").exists()
            && home.join("cache").exists()
            && home.join("books").exists()
    );
    assert_eq!(log(home), None);
}

#[test]
fn a_dry_run_checks_every_row_and_moves_or_logs_nothing_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/tidy")).unwrap();
    fs::write(home.join("notes.txt"), "notes\n").unwrap();
    let plan = stage(home, "notes.txt\tarchive/tidy/notes.txt\n");

    let (done, failed) = apply(&plan, home, true, &references());

    assert_eq!(done, ["would move: notes.txt -> archive/tidy/notes.txt"]);
    assert!(failed.is_empty(), "{failed:?}");
    assert!(home.join("notes.txt").exists());
    assert_eq!(log(home), None);
}

#[test]
fn a_malformed_plan_moves_nothing_and_names_the_row_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::write(home.join("notes.txt"), "notes\n").unwrap();
    let plan = stage(home, "notes.txt\tarchive/notes.txt\textra\n");

    let (done, failed) = apply(&plan, home, false, &references());

    assert!(done.is_empty());
    assert_eq!(failed.len(), 1);
    assert!(failed[0].contains("plan.tsv:1:"), "{failed:?}");
    assert!(home.join("notes.txt").exists());
}

#[test]
fn a_source_that_holds_the_plan_or_a_destination_that_climbs_out_is_refused_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/plan")).unwrap();
    fs::create_dir_all(home.join("elsewhere")).unwrap();
    let plan = stage(
        home,
        "archive\telsewhere/archive\n../escape\telsewhere/escape\n",
    );
    let plan = plan.canonicalize().unwrap();

    let (done, failed) = apply(&plan, home, false, &references());

    assert!(done.is_empty(), "{done:?}");
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert!(
        failed[0].contains("plan or its log lives in the source"),
        "{failed:?}"
    );
    assert!(failed[1].contains("climbs out with .."), "{failed:?}");
    assert!(home.join("archive").exists());
}

#[test]
fn a_destination_directory_that_does_not_exist_is_refused_rather_than_created_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::write(home.join("notes.txt"), "notes\n").unwrap();
    let plan = stage(home, "notes.txt\tarchive/missing/notes.txt\n");

    let (done, failed) = apply(&plan, home, false, &references());

    assert!(done.is_empty());
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert!(failed[0].contains("does not exist"), "{failed:?}");
    assert!(home.join("notes.txt").exists());
    assert!(!home.join("archive/missing").exists());
}

#[test]
fn the_plan_header_and_comments_are_not_rows_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive/tidy")).unwrap();
    fs::write(home.join("notes.txt"), "notes\n").unwrap();
    let plan = stage(
        home,
        "# tidy\nfrom\tto\nnotes.txt\tarchive/tidy/notes.txt\n",
    );

    let (done, failed) = apply(&plan, home, false, &references());

    assert_eq!(done.len(), 1, "{failed:?}");
    assert!(failed.is_empty());
}

#[test]
fn a_config_under_dot_config_is_read_by_the_live_reference_loader_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join(".config/nixos")).unwrap();
    fs::write(
        home.join(".config/nixos/home.nix"),
        format!(
            "home.file.\"{}\".text = \"x\";\n",
            home.join("books").display()
        ),
    )
    .unwrap();

    let references = References::load(home);

    assert!(references.names(&home.join("books"), home));
    assert!(!references.names(&home.join("notes"), home));
}

#[test]
fn a_tilde_or_home_prefix_names_the_same_path_and_a_parent_escape_is_refused_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();

    for field in ["~/notes", "$HOME/notes", "${HOME}/notes"] {
        assert_eq!(path(home, field).unwrap(), home.join("notes"), "{field}");
    }
    assert!(path(home, "../notes").is_err());
    assert!(path(home, "~other/notes").is_err());
}

#[test]
fn a_destination_reached_through_a_symbolic_link_is_refused_north_992() {
    let home = TempDir::new().unwrap();
    let home = home.path();
    fs::create_dir_all(home.join("archive")).unwrap();
    fs::create_dir_all(home.join("elsewhere")).unwrap();
    fs::create_dir_all(home.join("notes")).unwrap();
    fs::write(home.join("notes/a.txt"), "a\n").unwrap();
    std::os::unix::fs::symlink(home.join("elsewhere"), home.join("archive/link")).unwrap();
    let plan = stage(home, "notes\tarchive/link/notes\n");

    let (done, failed) = apply(&plan, home, false, &references());

    assert!(done.is_empty(), "{done:?}");
    assert!(failed[0].contains("symbolic link"), "{failed:?}");
    assert!(home.join("notes/a.txt").exists());
    assert!(!home.join("elsewhere/notes").exists());
}
