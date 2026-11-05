//! `markitdown mcp --http`: the MCP server over HTTP, with the routes
//! markitdown-mcp serves: Streamable HTTP at `/mcp` (stateless, JSON
//! responses) and the older SSE transport at `/sse` + `/messages/`.

use crate::mcp;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Largest request body accepted.
const MAX_BODY: usize = 4 << 20;

type Sessions = Arc<Mutex<HashMap<String, Sender<String>>>>;

struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn read_request(stream: &mut BufReader<TcpStream>) -> Result<Request, (u16, &'static str)> {
    let bad = (400, "Bad Request");
    let mut line = String::new();
    stream.read_line(&mut line).map_err(|_| bad)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().ok_or(bad)?, parts.next().ok_or(bad)?);
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if stream.read_line(&mut h).map_err(|_| bad)? == 0 {
            return Err(bad);
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        let (k, v) = h.split_once(':').ok_or(bad)?;
        headers.push((k.trim().to_string(), v.trim().to_string()));
        if headers.len() > 100 {
            return Err(bad);
        }
    }
    let mut req = Request {
        method: method.to_string(),
        path: path.to_string(),
        query: query.to_string(),
        headers,
        body: Vec::new(),
    };
    if req.header("transfer-encoding").is_some() {
        return Err((411, "Length Required"));
    }
    let len: usize = match req.header("content-length") {
        Some(l) => l.parse().map_err(|_| bad)?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err((413, "Payload Too Large"));
    }
    req.body = vec![0; len];
    stream.read_exact(&mut req.body).map_err(|_| bad)?;
    Ok(req)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        411 => "Length Required",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        _ => "Error",
    }
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = reason(status);
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(body);
}

fn jsonrpc_error(code: i64, message: &str) -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": null, "error": {"code": code, "message": message}})
        .to_string()
        .into_bytes()
}

/// A random 128-bit hex id, from the standard library's random hash keys.
fn session_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut id = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(std::time::UNIX_EPOCH.elapsed().map_or(0, |d| d.as_nanos()));
        id.push_str(&format!("{:016x}", h.finish()));
    }
    id
}

/// The MCP SDK's DNS-rebinding protection when bound to localhost: the
/// Host (and any Origin) must name a loopback address.
fn host_allowed(req: &Request, localhost: bool) -> Result<(), (u16, &'static str)> {
    if !localhost {
        return Ok(());
    }
    let loopback = |h: &str| {
        let host = h
            .strip_prefix("http://")
            .or_else(|| h.strip_prefix("https://"))
            .unwrap_or(h);
        let name = if let Some(rest) = host.strip_prefix('[') {
            rest.split(']')
                .next()
                .map(|n| format!("[{n}]"))
                .unwrap_or_default()
        } else {
            host.split(':').next().unwrap_or_default().to_string()
        };
        matches!(name.as_str(), "127.0.0.1" | "localhost" | "[::1]")
    };
    if !req.header("host").is_some_and(loopback) {
        return Err((421, "Invalid Host header"));
    }
    if req.header("origin").is_some_and(|o| !loopback(o)) {
        return Err((403, "Invalid Origin header"));
    }
    Ok(())
}

fn parse_message(req: &Request) -> Result<Value, (u16, Vec<u8>)> {
    let ct = req.header("content-type").unwrap_or_default();
    if !ct.to_ascii_lowercase().starts_with("application/json") {
        return Err((
            415,
            jsonrpc_error(
                -32600,
                "Unsupported Media Type: Content-Type must be application/json",
            ),
        ));
    }
    serde_json::from_slice(&req.body)
        .map_err(|e| (400, jsonrpc_error(-32700, &format!("Parse error: {e}"))))
}

/// `POST /mcp`: one JSON-RPC message; a request gets its response as JSON,
/// a notification or response gets 202.
fn streamable_http(stream: &mut TcpStream, req: &Request) {
    if req.method != "POST" {
        respond(
            stream,
            405,
            "application/json",
            &jsonrpc_error(-32000, "Method Not Allowed"),
        );
        return;
    }
    let accept = req.header("accept").unwrap_or("*/*");
    if !(accept.contains("application/json") || accept.contains("*/*")) {
        respond(
            stream,
            406,
            "application/json",
            &jsonrpc_error(
                -32600,
                "Not Acceptable: Client must accept application/json",
            ),
        );
        return;
    }
    let msg = match parse_message(req) {
        Ok(m) => m,
        Err((status, body)) => {
            respond(stream, status, "application/json", &body);
            return;
        }
    };
    match mcp::handle(&msg) {
        Some(reply) => respond(
            stream,
            200,
            "application/json",
            reply.to_string().as_bytes(),
        ),
        None => respond(stream, 202, "application/json", b""),
    }
}

/// `GET /sse`: the event stream; the endpoint event names where to POST.
fn sse(mut stream: TcpStream, sessions: &Sessions) {
    let id = session_id();
    let (tx, rx) = mpsc::channel::<String>();
    sessions.lock().unwrap().insert(id.clone(), tx);
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: keep-alive\r\n\r\n";
    let mut ok = stream
        .write_all(head.as_bytes())
        .and_then(|_| {
            write!(
                stream,
                "event: endpoint\r\ndata: /messages/?session_id={id}\r\n\r\n"
            )
        })
        .and_then(|_| stream.flush())
        .is_ok();
    while ok {
        // a comment every 15 seconds notices clients that went away
        ok = match rx.recv_timeout(Duration::from_secs(15)) {
            Ok(msg) => write!(stream, "event: message\r\ndata: {msg}\r\n\r\n"),
            Err(mpsc::RecvTimeoutError::Timeout) => write!(stream, ": ping\r\n\r\n"),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        .and_then(|_| stream.flush())
        .is_ok();
    }
    sessions.lock().unwrap().remove(&id);
}

/// `POST /messages/?session_id=…`: answered on that session's event stream.
fn sse_message(stream: &mut TcpStream, req: &Request, sessions: &Sessions) {
    let id = req
        .query
        .split('&')
        .find_map(|kv| kv.strip_prefix("session_id="))
        .unwrap_or_default();
    let Some(tx) = sessions.lock().unwrap().get(id).cloned() else {
        let status = if id.is_empty() { 400 } else { 404 };
        let text: &[u8] = if id.is_empty() {
            b"session_id is required"
        } else {
            b"Could not find session"
        };
        respond(stream, status, "text/plain", text);
        return;
    };
    let msg = match parse_message(req) {
        Ok(m) => m,
        Err((status, body)) => {
            respond(stream, status, "application/json", &body);
            return;
        }
    };
    respond(stream, 202, "text/plain", b"Accepted");
    if let Some(reply) = mcp::handle(&msg) {
        let _ = tx.send(reply.to_string());
    }
}

fn connection(stream: TcpStream, sessions: Sessions, localhost: bool) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let req = match read_request(&mut reader) {
        Ok(r) => r,
        Err((status, reason)) => {
            respond(&mut stream, status, "text/plain", reason.as_bytes());
            return;
        }
    };
    if let Err((status, reason)) = host_allowed(&req, localhost) {
        respond(&mut stream, status, "text/plain", reason.as_bytes());
        return;
    }
    match (req.method.as_str(), req.path.as_str()) {
        (_, "/mcp" | "/mcp/") => streamable_http(&mut stream, &req),
        ("GET", "/sse") => {
            let _ = stream.set_read_timeout(None);
            sse(stream, &sessions);
        }
        ("POST", "/messages" | "/messages/") => sse_message(&mut stream, &req, &sessions),
        _ => respond(&mut stream, 404, "text/plain", b"Not Found"),
    }
}

/// Serve until the process is stopped.
pub fn serve(host: &str, port: u16) -> Result<(), String> {
    let localhost = matches!(host, "127.0.0.1" | "localhost");
    if !localhost {
        eprintln!(
            "\nWARNING: The server is being bound to a non-localhost interface ({host}).\n\
             This exposes the server to other machines on the network or Internet.\n\
             The server has NO authentication and runs with your user's privileges.\n\
             Any process or user that can reach this interface can read files and\n\
             fetch network resources accessible to this user.\n\
             Only proceed if you understand the security implications.\n"
        );
    }
    let listener = TcpListener::bind((host, port)).map_err(|e| format!("{host}:{port}: {e}"))?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    eprintln!(
        "markitdown-rs {} MCP server on http://{addr} (Streamable HTTP at /mcp, SSE at /sse)",
        env!("CARGO_PKG_VERSION")
    );
    let sessions: Sessions = Arc::default();
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let sessions = sessions.clone();
        std::thread::spawn(move || connection(stream, sessions, localhost));
    }
    Ok(())
}
