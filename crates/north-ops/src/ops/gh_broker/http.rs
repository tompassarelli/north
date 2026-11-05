use std::io::{self, BufRead, BufReader, Read, Write};

const MAX_HEADERS: usize = 256;

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

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        header(&self.headers, name)
    }

    pub fn json(status: u16, headers: Headers, message: &str) -> Self {
        let mut headers = headers;
        headers.push(("Content-Type".to_owned(), "application/json; charset=utf-8".to_owned()));
        Self { status, headers, body: serde_json::json!({ "message": message }).to_string().into_bytes() }
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

pub fn status_code(line: &str) -> Option<u16> {
    let mut words = line.split_whitespace();
    words.next().filter(|version| version.starts_with("HTTP/"))?;
    words.next()?.parse().ok()
}

pub fn write_response(mut stream: impl Write, response: &Response, source: &str) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason(response.status));
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("X-North-Broker: {source}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", response.body.len()));
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        304 => "Not Modified",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        _ => "Status",
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn read_line(reader: &mut impl BufRead) -> io::Result<String> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "connection closed before a message"));
    }
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

pub(super) fn read_headers(reader: &mut impl BufRead) -> io::Result<Headers> {
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
    if header(headers, "transfer-encoding").is_some_and(|coding| coding.eq_ignore_ascii_case("chunked")) {
        return read_chunked(reader);
    }
    let length = header(headers, "content-length").map_or(Ok(0), |length| length.parse::<usize>().map_err(|_| invalid("bad content-length")))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(body)
}

fn read_chunked(reader: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line = read_line(reader)?;
        let size = usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16).map_err(|_| invalid("bad chunk size"))?;
        if size == 0 {
            while !read_line(reader)?.is_empty() {}
            return Ok(body);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        read_line(reader)?;
    }
}
