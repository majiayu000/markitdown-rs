//! `markitdown mcp` over real stdio JSON-RPC, against the answers
//! microsoft/markitdown-mcp gives.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn stdio_round_trip() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_markitdown"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |msg: Value| -> Option<Value> {
        writeln!(stdin, "{msg}").unwrap();
        msg.get("id")?;
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        Some(serde_json::from_str(&line).unwrap())
    };
    let call = |uri: &str| {
        json!({"jsonrpc": "2.0", "id": 9, "method": "tools/call",
               "params": {"name": "convert_to_markdown", "arguments": {"uri": uri}}})
    };

    let init = ask(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {},
        "clientInfo": {"name": "test", "version": "0"}}}),
    )
    .unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(init["result"]["serverInfo"]["name"], "markitdown");
    assert!(ask(json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());

    let list = ask(json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
    let tool = &list["result"]["tools"][0];
    assert_eq!(tool["name"], "convert_to_markdown");
    assert_eq!(tool["inputSchema"]["required"], json!(["uri"]));

    // a file: URI and a plain path give the CLI's Markdown (without its
    // final newline), also as structured content
    let docx = root().join("tests/fixtures/docx/markitdown-test.docx");
    let want = std::fs::read_to_string(root().join("tests/snapshots/docx/markitdown-test.docx.md"))
        .unwrap();
    let want = want.strip_suffix('\n').unwrap();
    let uri = format!("file://{}", docx.display().to_string().replace('\\', "/"));
    for source in [uri.as_str(), docx.to_str().unwrap()] {
        if cfg!(windows) && source.starts_with("file://") {
            continue;
        }
        let r = ask(call(source)).unwrap();
        assert_eq!(r["result"]["isError"], false, "{r}");
        assert_eq!(r["result"]["content"][0]["text"], want);
        assert_eq!(r["result"]["structuredContent"]["result"], want);
    }

    let r = ask(call("data:text/html;base64,PGgxPkhpPC9oMT4=")).unwrap();
    assert_eq!(r["result"]["content"][0]["text"], "# Hi");

    // 50,000 nested divs: plain text, as markitdown-mcp falls back to, and
    // the server keeps answering
    let deep = format!(
        "data:text/html,<html><body>{}<b>deep</b>{}</body></html>",
        "<div>".repeat(50_000),
        "</div>".repeat(50_000)
    );
    let r = ask(call(&deep)).unwrap();
    assert_eq!(r["result"]["content"][0]["text"], "deep");

    let missing = root().join("nope.docx");
    let r = ask(call(&format!("file://{}", missing.display()))).unwrap();
    assert_eq!(r["result"]["isError"], true);
    if cfg!(unix) {
        assert_eq!(
            r["result"]["content"][0]["text"],
            "Error executing tool convert_to_markdown: Could not read the resource: No such file or directory."
        );
    }

    let r = ask(json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
                       "params": {"name": "nope", "arguments": {}}}))
    .unwrap();
    assert_eq!(r["result"]["content"][0]["text"], "Unknown tool: nope");
    let r = ask(json!({"jsonrpc": "2.0", "id": 5, "method": "ping"})).unwrap();
    assert_eq!(r["result"], json!({}));
    let r = ask(json!({"jsonrpc": "2.0", "id": 6, "method": "foo/bar"})).unwrap();
    assert_eq!(r["error"]["code"], -32601);

    drop(stdin);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    // nothing but JSON-RPC on stdout; the log line is on stderr
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("MCP server on stdio"));
}

/// Send one HTTP request; the status and body.
fn http(
    addr: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (u16, String) {
    use std::io::Read;
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    let mut req = format!(
        "{method} {path} HTTP/1.1\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !headers.iter().any(|(k, _)| *k == "Host") {
        req.push_str(&format!("Host: {addr}\r\n"));
    }
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out[9..12].parse().unwrap();
    let body = out
        .split_once("\r\n\r\n")
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn http_transports() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_markitdown"))
        .args(["mcp", "--http", "--port", "0"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut log = BufReader::new(child.stderr.take().unwrap());
    let mut line = String::new();
    log.read_line(&mut line).unwrap();
    let addr = line
        .split("http://")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .unwrap()
        .to_string();
    let json = [
        ("Content-Type", "application/json"),
        ("Accept", "application/json, text/event-stream"),
    ];

    // Streamable HTTP: a request gets its response, a notification 202
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#;
    let (status, body) = http(&addr, "POST", "/mcp", &json, init);
    assert_eq!(status, 200);
    let reply: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reply["result"]["serverInfo"]["name"], "markitdown");
    let note = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert_eq!(http(&addr, "POST", "/mcp", &json, note).0, 202);
    let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"convert_to_markdown","arguments":{"uri":"data:text/html,<h1>Hi</h1>"}}}"#;
    let (_, body) = http(&addr, "POST", "/mcp", &json, call);
    let reply: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(reply["result"]["content"][0]["text"], "# Hi");
    assert_eq!(http(&addr, "GET", "/mcp", &[], "").0, 405);
    assert_eq!(
        http(
            &addr,
            "POST",
            "/mcp",
            &[("Content-Type", "text/plain")],
            "{}"
        )
        .0,
        415
    );
    // DNS rebinding protection on localhost
    let rebound = [json[0], json[1], ("Host", "evil.example")];
    assert_eq!(http(&addr, "POST", "/mcp", &rebound, init).0, 421);

    // SSE: the endpoint event, then replies on the stream
    let mut sse = std::net::TcpStream::connect(&addr).unwrap();
    write!(
        sse,
        "GET /sse HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\n\r\n"
    )
    .unwrap();
    let mut events = BufReader::new(sse);
    let mut endpoint = String::new();
    while !endpoint.starts_with("data: ") {
        endpoint.clear();
        events.read_line(&mut endpoint).unwrap();
    }
    let path = endpoint.trim_start_matches("data: ").trim().to_string();
    assert!(path.starts_with("/messages/?session_id="));
    let ping = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
    assert_eq!(http(&addr, "POST", &path, &json, ping).0, 202);
    let mut data = String::new();
    while !data.starts_with("data: ") {
        data.clear();
        events.read_line(&mut data).unwrap();
    }
    let reply: Value = serde_json::from_str(data.trim_start_matches("data: ").trim()).unwrap();
    assert_eq!(reply, json!({"jsonrpc": "2.0", "id": 7, "result": {}}));
    assert_eq!(
        http(&addr, "POST", "/messages/?session_id=nope", &json, ping).0,
        404
    );

    child.kill().unwrap();
    child.wait().unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_markitdown"))
        .args(["mcp", "--port", "3001"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("only valid when using streamable HTTP"));
}
