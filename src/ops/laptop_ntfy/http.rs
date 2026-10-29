use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const CONNECTION_WAIT: Duration = Duration::from_secs(15);
const BODY_LIMIT: u64 = 65_536;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) path: String,
    pub(super) authorization: String,
}

pub(super) type Respond = Arc<dyn Fn(&Request) -> (u16, String) + Send + Sync>;
pub(super) type Log = Arc<dyn Fn(&str) + Send + Sync>;

pub(super) fn serve(listener: &TcpListener, respond: &Respond, log: &Log) {
    for stream in listener.incoming().flatten() {
        let (respond, log) = (Arc::clone(respond), Arc::clone(log));
        std::thread::spawn(move || {
            let _ = connection(stream, &*respond, &*log);
        });
    }
}

fn connection(stream: TcpStream, respond: &dyn Fn(&Request) -> (u16, String), log: &dyn Fn(&str)) -> std::io::Result<()> {
    stream.set_read_timeout(Some(CONNECTION_WAIT))?;
    stream.set_write_timeout(Some(CONNECTION_WAIT))?;
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or_default().to_owned(), parts.next().unwrap_or_default().to_owned());
    let mut authorization = String::new();
    let mut length = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "authorization" => value.trim().clone_into(&mut authorization),
                "content-length" => length = value.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    std::io::copy(&mut reader.by_ref().take(length.min(BODY_LIMIT)), &mut std::io::sink())?;
    let (code, text) = if method.is_empty() || path.is_empty() {
        (400, "bad request".to_owned())
    } else {
        respond(&Request { method: method.clone(), path: path.clone(), authorization })
    };
    log(&format!("{method} {} {code}", path.split('?').next().unwrap_or_default()));
    let body = format!("{text}\n");
    let head = format!(
        "HTTP/1.0 {code} {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(code),
        body.len()
    );
    (&stream).write_all(head.as_bytes())?;
    (&stream).write_all(body.as_bytes())
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        501 => "Not Implemented",
        _ => "Internal Server Error",
    }
}

pub(super) fn post_json(url: &str, bearer: &str, body: &str, limit: Duration) -> Result<(), String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("{url}: only http:// ntfy addresses are supported"))?;
    let (authority, path) = rest.split_once('/').map_or((rest, "/".to_owned()), |(authority, path)| (authority, format!("/{path}")));
    let address = authority.to_socket_addrs().map_err(|error| format!("{authority}: {error}"))?.next().ok_or_else(|| format!("{authority}: no address"))?;
    let mut stream = TcpStream::connect_timeout(&address, limit).map_err(|error| format!("{authority}: {error}"))?;
    stream.set_read_timeout(Some(limit)).and_then(|()| stream.set_write_timeout(Some(limit))).map_err(|error| error.to_string())?;
    let request = format!(
        "POST {path} HTTP/1.0\r\nHost: {authority}\r\nAuthorization: Bearer {bearer}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).map_err(|error| format!("{authority}: {error}"))?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer).map_err(|error| format!("{authority}: {error}"))?;
    let text = String::from_utf8_lossy(&answer);
    let status = text.lines().next().unwrap_or_default();
    let code = status.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok());
    if code.is_some_and(|code| (200..300).contains(&code)) { Ok(()) } else { Err(format!("{authority} answered {status:?}")) }
}
