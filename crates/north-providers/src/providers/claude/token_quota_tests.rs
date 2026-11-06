use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;

use super::{curl_config, read_all_from, rows_from};
use crate::fleet::model::{Account, ProviderName};

const HEADERS: &str = "HTTP/2 200 \r\ncontent-type: application/json\r\nAnthropic-Ratelimit-Unified-5h-Status: allowed\r\nanthropic-ratelimit-unified-5h-utilization: 0.37\r\nanthropic-ratelimit-unified-5h-reset: 1793118000\r\nanthropic-ratelimit-unified-7d-utilization: 0.0449\r\nanthropic-ratelimit-unified-7d-reset: 1793692800\r\nanthropic-ratelimit-unified-representative-claim: five_hour\r\n\r\n";

#[test]
fn a_token_that_could_add_curl_config_lines_is_refused_without_echoing_it() {
    let config = curl_config("setup-token").unwrap();
    assert_eq!(config.lines().count(), 4);
    for hostile in ["", "two words", "line\nheader = \"x: y\"", "quote\"", "back\\slash"] {
        let refusal = curl_config(hostile).unwrap_err();
        assert!(!refusal.contains("quote") && !refusal.contains("slash"), "{refusal}");
    }
}

#[test]
fn one_hung_account_read_ends_on_its_deadline_while_the_others_read() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/messages", listener.local_addr().unwrap());
    let hung = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hung);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || answer(stream, &counter));
        }
    });
    let home = tempfile::tempdir().unwrap();
    let account = |label: &str, token: &str| {
        let file = home.path().join(format!("{label}.token"));
        std::fs::write(&file, format!("{token}\n")).unwrap();
        Account { provider: ProviderName::new("claude"), label: label.into(), home: home.path().into(), token_file: Some(file) }
    };
    let started = Instant::now();
    let reads = read_all_from(&endpoint, Duration::from_millis(300), &[account("a", "slow-token"), account("b", "fast-token"), account("c", "fast-token")]);
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(5), "the run took {elapsed:?}");
    let read = |label: &str| reads.iter().find(|(read, _)| read == label).unwrap().1.clone();
    assert!(read("a").unwrap_err().contains("2 attempts"));
    assert_eq!(hung.load(Ordering::SeqCst), 2);
    for label in ["b", "c"] {
        let rows = rows_from(label, &read(label).unwrap(), 1793100000.0).unwrap();
        let window = |name: &str| rows.iter().find(|row| row["name"] == name).unwrap().clone();
        assert_eq!(rows.len(), 2);
        assert_eq!((window("session")["used_pct"].clone(), window("session")["window_min"].clone(), window("session")["resets_at"].clone()), (json!(37.0), json!(300), json!("2026-10-27T16:20:00Z")));
        assert_eq!((window("weekly_all")["used_pct"].clone(), window("weekly_all")["resets_at"].clone()), (json!(4.0), json!("2026-11-03T08:00:00Z")));
        assert!(rows.iter().all(|row| row["provider"] == "claude" && row["account"] == label && row["kind"] == "window"));
    }
}

fn answer(mut stream: std::net::TcpStream, hung: &AtomicUsize) {
    let mut request = Vec::new();
    let mut byte = [0; 1];
    while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|read| read == 1) {
        request.push(byte[0]);
    }
    if String::from_utf8_lossy(&request).contains("Bearer slow-token") {
        hung.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    let text = String::from_utf8_lossy(&request).to_lowercase();
    let length = text.lines().find_map(|line| line.strip_prefix("content-length:")).and_then(|length| length.trim().parse().ok()).unwrap_or(0);
    let _ = stream.read_exact(&mut vec![0; length]);
    let body = HEADERS.replacen("HTTP/2 200 ", "HTTP/1.1 200 OK", 1).replacen("\r\n\r\n", "\r\ncontent-length: 0\r\nconnection: close\r\n\r\n", 1);
    let _ = stream.write_all(body.as_bytes());
}
