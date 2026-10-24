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
