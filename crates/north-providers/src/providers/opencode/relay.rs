use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};

use super::route::{GO_API_PATH, GO_ORIGIN};

const MOST_BODY: u64 = 16 << 20;
const MOST_HEAD: u64 = 64 << 10;
const DROPPED: [&str; 7] = ["host", "connection", "keep-alive", "accept-encoding", "transfer-encoding", "content-length", "expect"];

/// The one holder of the paid Go key in a session: a loopback listener in the host, outside the agent's sandbox, that takes a request carrying the session's own token and sends it on to Go with the key read from its file in the token's place, so the agent and every program it starts (its shell, a formatter, a language server, an MCP entry) hold the token and never the key (north#1041).
pub struct Relay {
    base: String,
    token: String,
}

impl Relay {
    pub fn start(key_file: &Path) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let base = format!("http://{}", listener.local_addr()?);
        let mut bytes = [0u8; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        let token: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let (key_file, expected) = (key_file.to_owned(), format!("Bearer {token}"));
        std::thread::spawn(move || {
            for stream in listener.incoming().map_while(Result::ok) {
                let (key_file, expected) = (key_file.clone(), expected.clone());
                std::thread::spawn(move || {
                    if let Err(error) = relay(stream, &key_file, &expected) {
                        eprintln!("north: go relay: {error}");
                    }
                });
            }
        });
        Ok(Self { base, token })
    }

    /// The Go API base the agent sends its requests to in place of Go's own.
    pub fn api(&self) -> String {
        format!("{}{GO_API_PATH}", self.base)
    }

    /// The key the agent sends, which only this relay accepts.
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// One request the agent sent the relay: what Go is asked, the headers passed on, and the key the agent named.
pub(super) struct Request {
    pub(super) method: String,
    pub(super) target: String,
    pub(super) headers: Vec<String>,
    pub(super) authorization: String,
    length: u64,
}

/// The request head on `reader`, read no further than `MOST_HEAD` bytes; `None` when it does not end within them, when its method is no plain word, or when its target lies outside Go's API or holds a `%`, `..`, `{` or `[`, since the paid key goes only there; a header whose name is not letters, digits and hyphens is left out, since curl reads a header argument that starts with `@` as a file of headers.
pub(super) fn read_request(reader: &mut impl BufRead) -> std::io::Result<Option<Request>> {
    let mut head = Read::take(&mut *reader, MOST_HEAD);
    let mut line = String::new();
    head.read_line(&mut line)?;
    let words: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
    let (mut headers, mut length, mut authorization, mut ended) = (Vec::new(), 0u64, String::new(), false);
    loop {
        line.clear();
        if head.read_line(&mut line)? == 0 || !line.ends_with('\n') {
            break;
        }
        if line.trim_end().is_empty() {
            ended = true;
            break;
        }
        let Some((name, value)) = line.trim_end().split_once(':') else { continue };
        let (name, value) = (name.trim(), value.trim());
        match name.to_ascii_lowercase().as_str() {
            "authorization" => value.clone_into(&mut authorization),
            "content-length" => length = value.parse().unwrap_or(0),
            lower if DROPPED.contains(&lower) || !http_token(lower) => {}
            _ => headers.push(format!("{name}: {value}")),
        }
    }
    let inside = |target: &str| target.strip_prefix(GO_API_PATH).is_some_and(|rest| rest.starts_with('/') && !rest.contains("..") && !rest.contains(['%', '{', '[']));
    Ok(match words.as_slice() {
        [method, target, ..] if ended && http_token(method) && inside(target) => Some(Request { method: method.clone(), target: target.clone(), headers, authorization, length }),
        _ => None,
    })
}

fn relay(stream: TcpStream, key_file: &Path, expected: &str) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let Some(request) = read_request(&mut reader)? else { return answer(&stream, "400 Bad Request") };
    if request.authorization != expected {
        return answer(&stream, "401 Unauthorized");
    }
    if request.length > MOST_BODY {
        return answer(&stream, "413 Content Too Large");
    }
    let mut body = Vec::new();
    let read = Read::by_ref(&mut reader).take(request.length).read_to_end(&mut body)?;
    if u64::try_from(read).ok() != Some(request.length) {
        return answer(&stream, "400 Bad Request");
    }
    let mut curl = Command::new("curl");
    curl.args(["--silent", "--show-error", "--no-buffer", "--http1.1", "--include", "--globoff", "--request", &request.method, "--variable"]).arg(format!("key@{}", key_file.display()));
    curl.args(["--expand-header", "Authorization: Bearer {{key:trim}}", "--header", "Expect:"]);
    for header in &request.headers {
        curl.args(["--header", header]);
    }
    if !body.is_empty() {
        curl.args(["--data-binary", "@-"]);
    }
    let mut child = curl.arg(format!("{GO_ORIGIN}{}", request.target)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&body)?;
    }
    let sent = child.stdout.take().map_or(Ok(()), |stdout| pass_back(stdout, &stream));
    let _ = child.kill();
    let _ = child.wait();
    sent
}

fn pass_back(stdout: impl Read, mut stream: &TcpStream) -> std::io::Result<()> {
    let mut answer = BufReader::new(stdout);
    let mut head = String::new();
    let mut line = String::new();
    while answer.read_line(&mut line)? > 0 && !line.trim_end().is_empty() {
        let name = line.split_once(':').map_or("", |(name, _)| name.trim()).to_ascii_lowercase();
        if !["connection", "keep-alive", "transfer-encoding"].contains(&name.as_str()) {
            head.push_str(line.trim_end());
            head.push_str("\r\n");
        }
        line.clear();
    }
    if head.is_empty() {
        return self::answer(stream, "502 Bad Gateway");
    }
    stream.write_all(format!("{head}Connection: close\r\n\r\n").as_bytes())?;
    std::io::copy(&mut answer, &mut stream)?;
    stream.flush()
}

fn answer(mut stream: &TcpStream, status: &str) -> std::io::Result<()> {
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())
}

fn http_token(word: &str) -> bool {
    !word.is_empty() && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}
