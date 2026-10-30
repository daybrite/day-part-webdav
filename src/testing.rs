// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
//! Bounded in-memory DAV fixture. Synthetic data only; binds numeric loopback, never the LAN.
use std::{
    collections::BTreeMap,
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[derive(Clone)]
struct Entry {
    bytes: Vec<u8>,
    etag: String,
    collection: bool,
}
struct State {
    entries: BTreeMap<String, Entry>,
    sequence: u64,
}
struct Shared {
    stop: AtomicBool,
    state: Mutex<State>,
}
pub struct Server {
    port: u16,
    shared: Arc<Shared>,
}
impl Server {
    pub fn start() -> io::Result<Self> {
        Self::bind(0)
    }
    pub fn bind(port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let mut entries = BTreeMap::new();
        entries.insert(
            "/dav/".into(),
            Entry {
                bytes: vec![],
                etag: "\"root\"".into(),
                collection: true,
            },
        );
        entries.insert(
            "/dav/fixture.txt".into(),
            Entry {
                bytes: b"Synthetic WebDAV fixture\n".to_vec(),
                etag: "\"fixture-0\"".into(),
                collection: false,
            },
        );
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            state: Mutex::new(State {
                entries,
                sequence: 0,
            }),
        });
        let state = shared.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if state.stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Ok(stream) = stream {
                    let state = state.clone();
                    std::thread::spawn(move || {
                        let _ = serve(stream, &state);
                    });
                }
            }
        });
        Ok(Self { port, shared })
    }
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/dav/", self.port)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn serve(mut stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut first = String::new();
    reader.by_ref().take(8193).read_line(&mut first)?;
    let mut words = first.split_whitespace();
    let method = words.next().unwrap_or("");
    let raw = words.next().unwrap_or("");
    if first.len() > 8192 {
        return Ok(());
    }
    let mut headers = BTreeMap::new();
    let mut size = first.len();
    loop {
        let mut line = String::new();
        reader.by_ref().take(8193).read_line(&mut line)?;
        size += line.len();
        if size > 32768 {
            return Ok(());
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.to_ascii_lowercase(), v.trim().to_owned());
        }
    }
    let length = headers
        .get("content-length")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    if length > 8 * 1024 * 1024 {
        return answer(&mut stream, 413, vec![], vec![], false);
    }
    if headers
        .get("expect")
        .is_some_and(|v| v.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    let endpoint = crate::Endpoint::loopback("http://127.0.0.1/dav/").unwrap();
    let Ok(resolved) = endpoint.resolve_href(endpoint.as_str(), raw) else {
        return answer(&mut stream, 400, vec![], vec![], false);
    };
    let path = url::Url::parse(&resolved).unwrap().path().to_owned();
    if method == "OPTIONS" {
        return answer(
            &mut stream,
            200,
            vec![
                ("DAV", "1".into()),
                (
                    "Allow",
                    "OPTIONS, GET, HEAD, PROPFIND, PUT, MKCOL, COPY, MOVE, DELETE".into(),
                ),
            ],
            vec![],
            false,
        );
    }
    if path == "/dav/__slow__" {
        std::thread::sleep(Duration::from_secs(3));
        return answer(
            &mut stream,
            200,
            vec![],
            b"synthetic slow response".to_vec(),
            false,
        );
    }
    if path == "/dav/__busy__" {
        return answer(
            &mut stream,
            503,
            vec![("Retry-After", "1".into())],
            vec![],
            false,
        );
    }
    if path == "/dav/__drop__" {
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 99\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\npartial")?;
        return Ok(());
    }
    let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
    let entry = state.entries.get(&path).cloned();
    let mut status = 200;
    let mut out = vec![];
    let mut response_headers = vec![];
    match method {
        "GET" | "HEAD" => {
            if let Some(e) = entry.filter(|e| !e.collection) {
                response_headers.push(("ETag", e.etag.clone()));
                out = e.bytes;
                if headers.get("if-none-match") == Some(&e.etag) {
                    status = 304;
                    out.clear();
                } else if let Some(range) = headers
                    .get("range")
                    .filter(|_| headers.get("if-range") == Some(&e.etag))
                {
                    if let Some((a, b)) = range
                        .strip_prefix("bytes=")
                        .and_then(|s| s.split_once('-'))
                        .and_then(|(a, b)| {
                            Some((a.parse::<usize>().ok()?, b.parse::<usize>().ok()?))
                        })
                    {
                        if a <= b && a < out.len() {
                            let end = b.min(out.len() - 1);
                            response_headers
                                .push(("Content-Range", format!("bytes {a}-{end}/{}", out.len())));
                            out = out[a..=end].to_vec();
                            status = 206;
                        } else {
                            status = 416;
                            out.clear();
                        }
                    } else {
                        status = 400;
                        out.clear();
                    }
                }
            } else {
                status = 404;
            }
        }
        "PROPFIND" => {
            if entry.is_none() {
                status = 404;
            } else {
                let mut xml =
                    String::from("<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">");
                for (key, e) in &state.entries {
                    let direct = key.strip_prefix(&path).is_some_and(|tail| {
                        !tail.is_empty() && !tail.trim_end_matches('/').contains('/')
                    });
                    if key != &path && !(headers.get("depth").is_some_and(|d| d == "1") && direct) {
                        continue;
                    }
                    xml.push_str(&format!("<d:response><d:href>{}</d:href><d:propstat><d:prop><d:resourcetype>{}</d:resourcetype><d:getetag>{}</d:getetag><d:getcontentlength>{}</d:getcontentlength></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",escape(key),if e.collection{"<d:collection/>"}else{""},escape(&e.etag),e.bytes.len()));
                }
                xml.push_str("</d:multistatus>");
                out = xml.into_bytes();
                status = 207;
                response_headers.push(("Content-Type", "application/xml; charset=utf-8".into()));
            }
        }
        "PUT" | "MKCOL" => {
            let parent = path
                .trim_end_matches('/')
                .rsplit_once('/')
                .map(|(p, _)| format!("{p}/"))
                .unwrap_or_default();
            if headers.get("if-none-match").is_some_and(|v| v == "*") && entry.is_some()
                || headers
                    .get("if-match")
                    .is_some_and(|v| entry.as_ref().is_none_or(|e| &e.etag != v))
            {
                status = 412;
            } else if method == "MKCOL" && entry.is_some() {
                status = 405;
            } else if !state.entries.get(&parent).is_some_and(|e| e.collection) {
                status = 409;
            } else if state.entries.len() >= 256
                || state.entries.values().map(|e| e.bytes.len()).sum::<usize>() + body.len()
                    > 16 * 1024 * 1024
            {
                status = 507;
            } else {
                state.sequence += 1;
                let etag = format!("\"fixture-{}\"", state.sequence);
                response_headers.push(("ETag", etag.clone()));
                state.entries.insert(
                    path,
                    Entry {
                        bytes: body,
                        etag,
                        collection: method == "MKCOL",
                    },
                );
                status = if entry.is_some() { 204 } else { 201 };
            }
        }
        "DELETE" => {
            if entry.is_none() {
                status = 404;
            } else if path == "/dav/" {
                status = 403;
            } else {
                state.entries.retain(|key, _| {
                    key != &path && !(path.ends_with('/') && key.starts_with(&path))
                });
                status = 204;
            }
        }
        "COPY" | "MOVE" => {
            let destination = headers
                .get("destination")
                .and_then(|s| url::Url::parse(s).ok())
                .map(|u| u.path().to_owned());
            if let (Some(e), Some(dest)) = (entry, destination.filter(|s| s.starts_with("/dav/"))) {
                if e.collection {
                    status = 501;
                } else if state.entries.contains_key(&dest)
                    && headers.get("overwrite").is_none_or(|s| s != "T")
                {
                    status = 412;
                } else {
                    let exists = state.entries.contains_key(&dest);
                    state.entries.insert(dest, e);
                    if method == "MOVE" {
                        state.entries.remove(&path);
                    }
                    status = if exists { 204 } else { 201 };
                }
            } else {
                status = 404;
            }
        }
        _ => status = 405,
    }
    drop(state);
    answer(&mut stream, status, response_headers, out, method == "HEAD")
}
fn answer(
    stream: &mut TcpStream,
    status: u16,
    headers: Vec<(&str, String)>,
    body: Vec<u8>,
    head: bool,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        416 => "Range Not Satisfiable",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        507 => "Insufficient Storage",
        _ => "Fixture",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nConnection: close\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: OPTIONS, GET, HEAD, PROPFIND, PUT, MKCOL, COPY, MOVE, DELETE\r\nAccess-Control-Allow-Headers: Authorization, Content-Type, Depth, Destination, Overwrite, If-Match, If-None-Match, Range, If-Range, Cache-Control\r\nAccess-Control-Expose-Headers: ETag, DAV, Allow, Content-Range, Content-Length, Retry-After\r\n",
        body.len()
    )?;
    for (k, v) in headers {
        write!(stream, "{k}: {v}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    if !head {
        stream.write_all(&body)?;
    }
    Ok(())
}
