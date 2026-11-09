use std::io::{self, BufRead, BufReader, Read, Write};

const MAX_HEADERS: usize = 128;
const MAX_LINE: u64 = 16 * 1024;
const MAX_BODY: usize = 4 * 1024 * 1024;

pub type Headers = Vec<(String, String)>;

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct Response {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
}

pub fn header<'h>(headers: &'h [(String, String)], name: &str) -> Option<&'h str> {
    headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
}

impl Response {
    pub fn json(status: u16, body: &serde_json::Value) -> Self {
        Self { status, headers: vec![("Content-Type".to_owned(), "application/json".to_owned())], body: body.to_string().into_bytes() }
    }

    pub fn refusal(status: u16, message: &str) -> Self {
        Self::json(status, &serde_json::json!({ "error": message }))
    }
}

pub fn read_request(stream: impl Read) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let line = read_line(&mut reader)?;
    let mut words = line.split_whitespace();
    let (Some(method), Some(target)) = (words.next(), words.next()) else {
        return Err(invalid("malformed request line"));
    };
    let headers = read_headers(&mut reader)?;
    let body = read_body(&mut reader, &headers)?;
    Ok(Request { method: method.to_owned(), target: target.to_owned(), headers, body })
}

pub fn read_response(stream: impl Read) -> io::Result<Response> {
    let mut reader = BufReader::new(stream);
    let line = read_line(&mut reader)?;
    let mut words = line.split_whitespace();
    let status = words.next().filter(|version| version.starts_with("HTTP/")).and_then(|_| words.next()).and_then(|code| code.parse().ok()).ok_or_else(|| invalid("malformed status line"))?;
    let headers = read_headers(&mut reader)?;
    let body = read_body(&mut reader, &headers)?;
    Ok(Response { status, headers, body })
}

pub fn write_request(mut stream: impl Write, request: &Request) -> io::Result<()> {
    let mut head = format!("{} {} HTTP/1.1\r\nHost: north-broker\r\n", request.method, request.target);
    for (name, value) in &request.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", request.body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&request.body)?;
    stream.flush()
}

pub fn write_response(mut stream: impl Write, response: &Response) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason(response.status));
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", response.body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Status",
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_line(reader: &mut impl BufRead) -> io::Result<String> {
    let mut line = String::new();
    if reader.take(MAX_LINE).read_line(&mut line)? == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed before a message"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

fn read_headers(reader: &mut impl BufRead) -> io::Result<Headers> {
    let mut headers = Vec::new();
    loop {
        let line = read_line(reader)?;
        if line.is_empty() {
            return Ok(headers);
        }
        if headers.len() == MAX_HEADERS {
            return Err(invalid("too many headers"));
        }
        let (name, value) = line.split_once(':').ok_or_else(|| invalid("malformed header"))?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
}

fn read_body(reader: &mut impl BufRead, headers: &[(String, String)]) -> io::Result<Vec<u8>> {
    if header(headers, "transfer-encoding").is_some() {
        return Err(invalid("a chunked body is not accepted"));
    }
    let length = header(headers, "content-length").map_or(Ok(0), |length| length.parse::<usize>().map_err(|_| invalid("bad content-length")))?;
    if length > MAX_BODY {
        return Err(invalid("body too large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}
