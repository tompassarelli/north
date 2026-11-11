use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::probe_at;

fn key() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("key"), "sk-ant-test\n").unwrap();
    home
}

fn answer_once(status: u16, body: &'static str, seen: Arc<Mutex<String>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = request_body(&mut stream);
        *seen.lock().unwrap() = request;
        let reason = if status == 200 { "OK" } else { "Bad Request" };
        let _ = stream.write_all(format!("HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes());
    });
    endpoint
}

fn request_body(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut byte = [0; 1];
    while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|read| read == 1) {
        request.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&request).into_owned();
    let length = text
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|length| length.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    let _ = stream.read_exact(&mut body);
    format!("{text}\n{}", String::from_utf8_lossy(&body))
}

#[test]
fn a_probe_passes_on_a_200_and_asks_for_exactly_one_token() {
    let home = key();
    let seen = Arc::new(Mutex::new(String::new()));
    let endpoint = answer_once(200, r#"{"type":"message","content":[]}"#, Arc::clone(&seen));
    let answer = probe_at(&endpoint, Duration::from_secs(60), &home.path().join("key")).unwrap();
    assert!(answer.passed, "{}", answer.body);
    assert!(super::MINIMAL_TURN.contains("\"max_tokens\":1"), "the probe asks for more than one token: {}", super::MINIMAL_TURN);
    assert!(!seen.lock().unwrap().is_empty());
}

#[test]
fn a_refused_probe_keeps_the_providers_body_for_the_mark() {
    let home = key();
    let seen = Arc::new(Mutex::new(String::new()));
    let refusal = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Your credit balance is too low to access the Anthropic API."}}"#;
    let endpoint = answer_once(400, refusal, Arc::clone(&seen));
    let answer = probe_at(&endpoint, Duration::from_secs(60), &home.path().join("key")).unwrap();
    assert!(!answer.passed);
    assert_eq!(answer.status, 400);
    assert!(
        answer.body.contains("Your credit balance is too low"),
        "{}",
        answer.body
    );
}
