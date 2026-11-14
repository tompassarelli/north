use std::cell::RefCell;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Desktop, Failure, deliver, parse};

struct Fake {
    titles: RefCell<Vec<(&'static str, &'static str)>>,
    secrets: Value,
    typed: RefCell<Vec<String>>,
}

impl Desktop for Fake {
    fn active_window(&self, _: &str, _: &str) -> Result<(String, String), Failure> {
        let mut titles = self.titles.borrow_mut();
        let (window, title) = if titles.len() > 1 { titles.remove(0) } else { titles[0] };
        Ok((window.to_owned(), title.to_owned()))
    }

    fn secrets(&self) -> Result<Value, Failure> {
        Ok(self.secrets.clone())
    }

    fn type_text(&self, _: &Path, text: &str) -> Result<(), Failure> {
        self.typed.borrow_mut().push(text.to_owned());
        Ok(())
    }
}

fn fake(titles: Vec<(&'static str, &'static str)>) -> Fake {
    Fake {
        titles: RefCell::new(titles),
        secrets: json!({"b": {"username": "bee", "password": "hunter2"}, "a": {"username": "me", "password": "mine", "owner_reserved_unencrypted": true}, "c": {"username": "see"}}),
        typed: RefCell::default(),
    }
}

fn private_run(root: &Path) -> (PathBuf, UnixListener) {
    let run = root.join("private-desktop.abc");
    std::fs::create_dir_all(run.join("runtime")).unwrap();
    let socket = UnixListener::bind(run.join("runtime/wayland-0")).unwrap();
    std::fs::write(run.join("display"), ":7\n").unwrap();
    std::fs::write(run.join("xauthority"), "/tmp/xauth\n").unwrap();
    (run, socket)
}

fn request(run: &Path, account: &str, field: &str, authorized: bool) -> super::Request {
    let mut words = vec![run.to_string_lossy().into_owned(), account.to_owned(), field.to_owned()];
    authorized.then(|| words.push("--owner-authorized".to_owned()));
    parse(&words).unwrap_or_else(|_| unreachable!())
}

fn outcome(result: Result<&'static str, Failure>) -> (String, u8) {
    match result {
        Ok(message) => (message.to_owned(), 0),
        Err(Failure(message, code)) => (message, code),
    }
}

#[test]
fn the_field_is_typed_into_the_active_login_window_and_never_printed() {
    let root = tempfile::tempdir().unwrap();
    let (run, _socket) = private_run(root.path());
    let desktop = fake(vec![("12", "Battle.net Login")]);
    let (message, code) = outcome(deliver(&request(&run, "b", "password", false), root.path(), &desktop));
    assert_eq!((message.as_str(), code), ("Field delivered; verify the login result privately.", 0));
    assert_eq!(desktop.typed.into_inner(), ["hunter2"]);
}

#[test]
fn delivery_refuses_a_wrong_window_a_missing_field_a_non_private_run_and_a_focus_change() {
    let root = tempfile::tempdir().unwrap();
    let (run, _socket) = private_run(root.path());
    let wrong = fake(vec![("12", "Terminal")]);
    assert_eq!(outcome(deliver(&request(&run, "b", "username", false), root.path(), &wrong)), ("Login window is not active.".to_owned(), 1));
    assert!(wrong.typed.borrow().is_empty());
    let missing = fake(vec![("12", "Battle.net")]);
    assert_eq!(outcome(deliver(&request(&run, "c", "password", false), root.path(), &missing)), ("Missing account field".to_owned(), 1));
    let elsewhere = tempfile::tempdir().unwrap();
    let (stray, _socket) = private_run(elsewhere.path());
    assert_eq!(outcome(deliver(&request(&stray, "b", "username", false), root.path(), &fake(vec![("12", "Battle.net")]))), ("Private desktop required.".to_owned(), 1));
    let moved = fake(vec![("12", "Battle.net"), ("13", "Battle.net")]);
    assert_eq!(outcome(deliver(&request(&run, "b", "username", false), root.path(), &moved)).0, "Focus changed during delivery; inspect privately before proceeding.");
}


#[test]
fn an_owner_reserved_account_needs_the_owner_flag() {
    let root = tempfile::tempdir().unwrap();
    let (run, _socket) = private_run(root.path());
    let refused = fake(vec![("12", "Battle.net")]);
    let (message, code) = outcome(deliver(&request(&run, "a", "username", false), root.path(), &refused));
    assert_eq!(code, 3);
    assert!(message.starts_with("Refused: account a is the owner's personal account"));
    assert!(refused.typed.borrow().is_empty());
    let allowed = fake(vec![("12", "Battle.net")]);
    assert_eq!(outcome(deliver(&request(&run, "a", "username", true), root.path(), &allowed)).1, 0);
    assert_eq!(allowed.typed.into_inner(), ["me"]);
}
