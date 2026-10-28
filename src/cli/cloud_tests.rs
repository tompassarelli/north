use super::*;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;

type Calls = RefCell<Vec<(String, String, Option<String>)>>;

fn fake<'a>(
    responses: &'a RefCell<BTreeMap<String, VecDeque<String>>>,
    calls: &'a Calls,
) -> impl FnMut(&str, &str, Option<&str>) -> Result<(u16, String), String> + 'a {
    move |method, path, body| {
        calls
            .borrow_mut()
            .push((method.to_owned(), path.to_owned(), body.map(str::to_owned)));
        let mut responses = responses.borrow_mut();
        let queue = responses
            .get_mut(path)
            .ok_or_else(|| format!("unexpected {method} {path}"))?;
        let text = if queue.len() > 1 {
            queue.pop_front()
        } else {
            queue.front().cloned()
        };
        Ok((200, text.unwrap_or_default()))
    }
}

fn script(entries: &[(&str, &[Value])]) -> RefCell<BTreeMap<String, VecDeque<String>>> {
    RefCell::new(
        entries
            .iter()
            .map(|(path, bodies)| {
                (
                    (*path).to_owned(),
                    bodies.iter().map(Value::to_string).collect(),
                )
            })
            .collect(),
    )
}

fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn cloud_parse_reads_each_command_and_the_wait_timeout() {
    let words = |text: &str| text.split(' ').map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(parse(&words("list")), Some(Cloud::List));
    assert_eq!(
        parse(&words("send s1 msg.txt")),
        Some(Cloud::Send("s1".into(), "msg.txt".into()))
    );
    assert_eq!(
        parse(&words("collect events.jsonl out.mbox")),
        Some(Cloud::Collect("events.jsonl".into(), "out.mbox".into()))
    );
    assert_eq!(
        parse(&words("wait s1 out.jsonl")),
        Some(Cloud::Wait("s1".into(), "out.jsonl".into(), 3600, None))
    );
    assert_eq!(
        parse(&words("wait s1 out.jsonl --timeout 90")),
        Some(Cloud::Wait("s1".into(), "out.jsonl".into(), 90, None))
    );
    assert_eq!(
        parse(&words("wait s1 out.jsonl --after u-1 --timeout 90")),
        Some(Cloud::Wait(
            "s1".into(),
            "out.jsonl".into(),
            90,
            Some("u-1".into())
        ))
    );
    assert_eq!(parse(&words("wait s1 out.jsonl --after")), None);
    assert_eq!(parse(&words("wait s1 out.jsonl --timeout soon")), None);
    assert_eq!(parse(&words("send s1")), None);
}

#[test]
fn cloud_send_body_is_one_user_event_for_the_session() {
    assert_eq!(
        send_body("session_1", "u-1", "finish and hand back"),
        json!({"events": [{"uuid": "u-1", "session_id": "session_1", "type": "user", "parent_tool_use_id": null,
            "message": {"role": "user", "content": "finish and hand back"}}]})
    );
}

#[test]
fn cloud_turn_ends_on_a_result_or_an_end_turn_reply_and_not_between_tool_calls() {
    let assistant = |stop: Value| json!({"type": "assistant", "message": {"stop_reason": stop}});
    let result = json!({"type": "result", "subtype": "success"});
    let tool_result = json!({"type": "user", "message": {"content": [{"type": "tool_result"}]}});
    let system = json!({"type": "system", "subtype": "status"});
    assert!(turn_ended(
        &[assistant(json!("tool_use")), result.clone()],
        None
    ));
    assert!(turn_ended(&[assistant(json!("end_turn"))], None));
    assert!(turn_ended(&[result.clone(), system.clone()], None));
    assert!(!turn_ended(
        &[result.clone(), assistant(json!("tool_use"))],
        None
    ));
    assert!(!turn_ended(
        &[assistant(json!("end_turn")), tool_result.clone()],
        None
    ));
    assert!(!turn_ended(&[result.clone(), tool_result, system], None));
    assert!(!turn_ended(&[assistant(Value::Null)], None));
    assert!(!turn_ended(&[], None));
}

#[test]
fn cloud_turn_after_a_sent_message_ends_only_on_a_later_result() {
    let result = json!({"type": "result", "subtype": "success"});
    let sent = json!({"type": "user", "uuid": "u-1", "message": {"content": "finish"}});
    assert!(!turn_ended(std::slice::from_ref(&result), Some("u-1")));
    assert!(!turn_ended(&[result.clone(), sent.clone()], Some("u-1")));
    assert!(turn_ended(&[result.clone(), sent, result], Some("u-1")));
}

#[test]
fn cloud_events_follow_pages_until_has_more_ends() {
    let path = "/v1/sessions/s1/events?beta=true&limit=1000";
    let responses = script(&[
        (
            path,
            &[json!({"data": [{"n": 1}], "has_more": true, "last_id": "e1"})],
        ),
        (
            &format!("{path}&after_id=e1"),
            &[json!({"data": [{"n": 2}], "has_more": false, "last_id": "e2"})],
        ),
    ]);
    let calls = Calls::default();
    assert_eq!(
        events(&mut fake(&responses, &calls), "s1").unwrap(),
        vec![json!({"n": 1}), json!({"n": 2})]
    );
    assert_eq!(calls.borrow().len(), 2);
}

#[test]
fn cloud_sweep_archives_only_cloud_sessions_waiting_on_a_human() {
    let sessions = json!({"data": [
        {"id": "s1", "session_status": "requires_action", "environment_kind": "anthropic_cloud", "title": "box"},
        {"id": "s2", "session_status": "requires_action", "environment_kind": "bridge", "title": "proxy"},
        {"id": "s3", "session_status": "idle", "environment_kind": "anthropic_cloud", "title": "done"},
    ], "has_more": false});
    let responses = script(&[
        ("/v1/sessions?beta=true&limit=100", &[sessions]),
        (
            "/v1/sessions/s1/events?beta=true&limit=1000",
            &[json!({"data": [{"type": "user"}], "has_more": false})],
        ),
        ("/v1/sessions/s1/archive?beta=true", &[json!({})]),
    ]);
    let calls = Calls::default();
    let dir = tempfile::tempdir().unwrap();
    let mut out = Vec::new();
    sweep(&mut fake(&responses, &calls), dir.path(), &mut out).unwrap();
    let posts = calls
        .borrow()
        .iter()
        .filter(|(method, ..)| method == "POST")
        .map(|(_, path, body)| (path.clone(), body.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        posts,
        vec![(
            "/v1/sessions/s1/archive?beta=true".to_owned(),
            Some("{}".to_owned())
        )]
    );
    assert_eq!(
        lines(&dir.path().join("s1.jsonl")),
        vec![json!({"type": "user"})]
    );
    assert!(
        String::from_utf8(out)
            .unwrap()
            .starts_with("stopped s1 (box): it was waiting on a human")
    );
}

#[test]
fn cloud_wait_returns_once_idle_with_an_ended_turn_and_times_out_otherwise() {
    let status = |state: &str| json!({"id": "s1", "session_status": state});
    let page = |event: Value| json!({"data": [event], "has_more": false});
    let responses = script(&[
        (
            "/v1/sessions/s1?beta=true",
            &[status("running"), status("idle"), status("idle")],
        ),
        (
            "/v1/sessions/s1/events?beta=true&limit=1000",
            &[
                page(json!({"type": "assistant", "message": {"stop_reason": "tool_use"}})),
                page(json!({"type": "assistant", "message": {"stop_reason": "tool_use"}})),
                page(json!({"type": "result", "subtype": "success"})),
            ],
        ),
    ]);
    let calls = Calls::default();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("s1.jsonl");
    let mut sleeps = 0;
    assert_eq!(
        wait(
            &mut fake(&responses, &calls),
            "s1",
            &out,
            None,
            1,
            &mut || { sleeps += 1 }
        )
        .unwrap(),
        None
    );
    assert!(!out.exists());
    assert_eq!(
        wait(
            &mut fake(&responses, &calls),
            "s1",
            &out,
            None,
            5,
            &mut || { sleeps += 1 }
        )
        .unwrap(),
        Some(1)
    );
    assert_eq!(sleeps, 2);
    assert_eq!(
        lines(&out),
        vec![json!({"type": "result", "subtype": "success"})]
    );
}

#[test]
fn cloud_curl_keeps_the_token_out_of_argv() {
    let args = curl_args(
        "POST",
        "https://api.anthropic.com/v1/sessions",
        Some(Path::new("/tmp/body")),
    );
    assert!(!args.iter().any(|arg| arg.contains("secret-token")));
    assert!(args.windows(2).any(|pair| pair == ["--config", "-"]));
    let config = curl_config("secret-token", "org-1");
    assert!(config.contains("header = \"Authorization: Bearer secret-token\"\n"));
    assert!(config.contains("header = \"x-organization-uuid: org-1\"\n"));
}

fn stub_server(status: &str) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let reply = format!("HTTP/1.1 {status}\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{{}}");
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        let mut head = String::new();
        let mut length = 0;
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap();
            }
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        let mut body = vec![0; length];
        reader.read_exact(&mut body).unwrap();
        reader.get_mut().write_all(reply.as_bytes()).unwrap();
        head + &String::from_utf8(body).unwrap()
    });
    (base, server)
}

#[test]
fn cloud_send_posts_the_message_to_a_stub_server_through_curl() {
    let (base, server) = stub_server("200 OK");
    let curl = Curl {
        base,
        token: "tok-1".into(),
        org: "org-1".into(),
    };
    let status = send(
        &mut |method, path, body| curl.call(method, path, body),
        "session_1",
        "u-1",
        "finish \"now\"\n",
    )
    .unwrap();
    assert_eq!(status, 200);
    let request = server.join().unwrap();
    let (head, body) = request.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("POST /v1/sessions/session_1/events?beta=true HTTP/1.1\r\n"));
    for header in [
        "Authorization: Bearer tok-1",
        "x-organization-uuid: org-1",
        "anthropic-beta: ccr-byoc-2025-07-29",
        "anthropic-version: 2023-06-01",
    ] {
        assert!(
            head.contains(&format!("{header}\r\n")),
            "{header} missing from {head}"
        );
    }
    assert_eq!(
        serde_json::from_str::<Value>(body).unwrap(),
        send_body("session_1", "u-1", "finish \"now\"\n")
    );
}
