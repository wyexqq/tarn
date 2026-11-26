use std::collections::HashMap;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::api::{Params, Upload};

const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 60 * 1024 * 1024;
const READ_CHUNK: usize = 16 * 1024;
const HEAD_END: &[u8] = b"\r\n\r\n";

pub struct Request {
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).map(String::as_str).unwrap_or_default()
    }

    pub fn wants_close(&self) -> bool {
        self.header("connection").eq_ignore_ascii_case("close")
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

struct Head {
    path: String,
    query: String,
    headers: HashMap<String, String>,
}

fn parse_head(raw: &[u8]) -> Result<Head, String> {
    let text = String::from_utf8_lossy(raw);
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    parts.next().ok_or("пустой запрос")?;
    let target = parts.next().ok_or("нет адреса")?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_string()))
        .collect();
    Ok(Head { path: path.to_string(), query: query.to_string(), headers })
}

fn decode_chunked(raw: &[u8]) -> Option<(Vec<u8>, usize)> {
    let mut body = Vec::new();
    let mut position = 0;
    loop {
        let line_end = position + find(&raw[position..], b"\r\n")?;
        let size_text = String::from_utf8_lossy(&raw[position..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next()?.trim(), 16).ok()?;
        let data_start = line_end + 2;
        if size == 0 {
            let trailer_end = data_start + find(&raw[data_start..], b"\r\n")?;
            return Some((body, trailer_end + 2));
        }
        let data_end = data_start + size;
        if raw.len() < data_end + 2 {
            return None;
        }
        body.extend_from_slice(&raw[data_start..data_end]);
        position = data_end + 2;
    }
}

async fn read_more(stream: &mut (impl AsyncRead + Unpin), buffer: &mut Vec<u8>) -> Result<bool, String> {
    let mut chunk = vec![0u8; READ_CHUNK];
    let count = stream.read(&mut chunk).await.map_err(|error| error.to_string())?;
    buffer.extend_from_slice(&chunk[..count]);
    Ok(count > 0)
}

pub async fn read_request(stream: &mut (impl AsyncRead + AsyncWrite + Unpin), buffer: &mut Vec<u8>) -> Result<Option<Request>, String> {
    let head_end = loop {
        if let Some(found) = find(buffer, HEAD_END) {
            break found;
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return Err("слишком длинные заголовки".to_string());
        }
        if !read_more(stream, buffer).await? {
            return if buffer.is_empty() { Ok(None) } else { Err("соединение оборвалось".to_string()) };
        }
    };
    let head = parse_head(&buffer[..head_end])?;
    let body_start = head_end + HEAD_END.len();
    let is_chunked = head.headers.get("transfer-encoding").is_some_and(|value| value.to_ascii_lowercase().contains("chunked"));
    let length: usize = head.headers.get("content-length").and_then(|value| value.parse().ok()).unwrap_or(0);
    if length > MAX_BODY_BYTES {
        return Err("слишком большое тело запроса".to_string());
    }
    let expects_continue = head.headers.get("expect").is_some_and(|value| value.eq_ignore_ascii_case("100-continue"));
    if expects_continue && buffer.len() == body_start {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await.map_err(|error| error.to_string())?;
    }
    let (body, consumed) = if is_chunked {
        loop {
            if let Some((body, used)) = decode_chunked(&buffer[body_start..]) {
                break (body, body_start + used);
            }
            if buffer.len() - body_start > MAX_BODY_BYTES || !read_more(stream, buffer).await? {
                return Err("тело запроса оборвалось".to_string());
            }
        }
    } else {
        while buffer.len() < body_start + length {
            if !read_more(stream, buffer).await? {
                return Err("тело запроса оборвалось".to_string());
            }
        }
        (buffer[body_start..body_start + length].to_vec(), body_start + length)
    };
    buffer.drain(..consumed);
    Ok(Some(Request { path: head.path, query: head.query, headers: head.headers, body }))
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    }
}

pub fn response(status: u16, content_type: &str, body: &[u8], keep_alive: bool) -> Vec<u8> {
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let head = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: {connection}\r\n\r\n",
        status_text(status),
        body.len()
    );
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

fn hex_value(letter: u8) -> Option<u8> {
    (letter as char).to_digit(16).map(|digit| digit as u8)
}

fn escaped_byte(bytes: &[u8], index: usize) -> Option<u8> {
    if bytes.get(index) != Some(&b'%') {
        return None;
    }
    let high = hex_value(*bytes.get(index + 1)?)?;
    let low = hex_value(*bytes.get(index + 2)?)?;
    Some(high * 16 + low)
}

pub fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if let Some(value) = escaped_byte(bytes, index) {
            decoded.push(value);
            index += 3;
        } else if bytes[index] == b'+' {
            decoded.push(b' ');
            index += 1;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&decoded).to_string()
}

fn add_form_fields(params: &mut Params, form: &str) {
    for pair in form.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        params.fields.insert(percent_decode(name), Value::String(percent_decode(value)));
    }
}

fn disposition_value(disposition: &str, name: &str) -> Option<String> {
    disposition.split(';').map(str::trim).find_map(|piece| {
        let (key, value) = piece.split_once('=')?;
        (key.trim().eq_ignore_ascii_case(name)).then(|| value.trim().trim_matches('"').to_string())
    })
}

fn boundary_of(content_type: &str) -> Option<String> {
    disposition_value(content_type, "boundary")
}

fn add_multipart_fields(params: &mut Params, body: &[u8], boundary: &str) {
    let delimiter = format!("--{boundary}").into_bytes();
    let separator = [b"\r\n".as_slice(), &delimiter].concat();
    let Some(first) = find(body, &delimiter) else {
        return;
    };
    let mut position = first + delimiter.len();
    while body.get(position..position + 2) == Some(b"\r\n".as_slice()) {
        let part_start = position + 2;
        let Some(head_length) = find(&body[part_start..], HEAD_END) else {
            return;
        };
        let head_text = String::from_utf8_lossy(&body[part_start..part_start + head_length]).to_string();
        let content_start = part_start + head_length + HEAD_END.len();
        let Some(content_length) = find(&body[content_start..], &separator) else {
            return;
        };
        let content = &body[content_start..content_start + content_length];
        add_part(params, &head_text, content);
        position = content_start + content_length + separator.len();
    }
}

fn add_part(params: &mut Params, head_text: &str, content: &[u8]) {
    let mut disposition = "";
    let mut mime = "application/octet-stream";
    for line in head_text.split("\r\n") {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-disposition") {
            disposition = value.trim();
        } else if name.trim().eq_ignore_ascii_case("content-type") {
            mime = value.trim();
        }
    }
    let Some(field) = disposition_value(disposition, "name") else {
        return;
    };
    match disposition_value(disposition, "filename") {
        Some(file_name) => {
            let upload = Upload { bytes: content.to_vec(), name: file_name, mime: mime.to_string() };
            params.uploads.insert(field, upload);
        }
        None => {
            params.fields.insert(field, Value::String(String::from_utf8_lossy(content).to_string()));
        }
    }
}

pub fn params_of(request: &Request) -> Result<Params, String> {
    let mut params = Params::default();
    add_form_fields(&mut params, &request.query);
    let content_type = request.header("content-type").to_ascii_lowercase();
    if request.body.is_empty() {
        return Ok(params);
    }
    if content_type.starts_with("application/json") {
        let parsed: Value = serde_json::from_slice(&request.body).map_err(|_| "тело запроса не JSON".to_string())?;
        if let Value::Object(fields) = parsed {
            params.fields.extend(fields);
        }
    } else if content_type.starts_with("multipart/form-data") {
        let boundary = boundary_of(request.header("content-type")).ok_or("нет boundary")?;
        add_multipart_fields(&mut params, &request.body, &boundary);
    } else {
        add_form_fields(&mut params, &String::from_utf8_lossy(&request.body));
    }
    Ok(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read_one(raw: &[u8]) -> Request {
        let (mut client, mut server) = tokio::io::duplex(1 << 20);
        client.write_all(raw).await.unwrap();
        drop(client);
        let mut buffer = Vec::new();
        read_request(&mut server, &mut buffer).await.unwrap().unwrap()
    }

    #[tokio::test]
    async fn reads_json_request() {
        let body = r#"{"chat_id":100001,"text":"Привет"}"#;
        let raw = format!("POST /bot123:abc/sendMessage HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let request = read_one(raw.as_bytes()).await;
        assert_eq!(request.path, "/bot123:abc/sendMessage");
        let params = params_of(&request).unwrap();
        assert_eq!(params.text("text").as_deref(), Some("Привет"));
        assert_eq!(params.int("chat_id"), Some(100001));
    }

    #[tokio::test]
    async fn reads_chunked_form() {
        let raw = b"POST /bot1/sendMessage?chat_id=5 HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\na\r\ntext=a%2Bb\r\n4\r\n+c+d\r\n0\r\n\r\n";
        let request = read_one(raw).await;
        let params = params_of(&request).unwrap();
        assert_eq!(params.text("text").as_deref(), Some("a+b c d"));
        assert_eq!(params.int("chat_id"), Some(5));
    }

    #[tokio::test]
    async fn reads_multipart_upload() {
        let body = "--XYZ\r\nContent-Disposition: form-data; name=\"chat_id\"\r\n\r\n100001\r\n--XYZ\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\nPNGDATA\r\n--XYZ--\r\n";
        let raw = format!("POST /bot1/sendPhoto HTTP/1.1\r\nContent-Type: multipart/form-data; boundary=XYZ\r\nContent-Length: {}\r\n\r\n{body}", body.len());
        let request = read_one(raw.as_bytes()).await;
        let params = params_of(&request).unwrap();
        assert_eq!(params.int("chat_id"), Some(100001));
        let photo = params.uploads.get("photo").unwrap();
        assert_eq!(photo.bytes, b"PNGDATA");
        assert_eq!(photo.mime, "image/png");
        assert_eq!(photo.name, "a.png");
    }

    #[test]
    fn response_has_length_and_connection() {
        let bytes = response(200, "application/json", b"{}", true);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 2\r\n"));
        assert!(text.ends_with("\r\n\r\n{}"));
    }
}
