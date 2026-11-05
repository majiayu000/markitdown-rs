//! `markitdown mcp`: a Model Context Protocol server on stdio, answering as
//! microsoft/markitdown-mcp does (one `convert_to_markdown` tool, the same
//! schemas, result shape and error texts). Logs go to stderr only.

use markitdown_core::fetch::{self, SourceError};
use markitdown_core::{Error, Options};
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const TOOL: &str = "convert_to_markdown";
/// The versions the Python MCP SDK negotiates; an unknown request gets the
/// newest.
/// Stack for each conversion thread: deeply nested documents need more than
/// a default thread gets.
const CONVERT_STACK: usize = 256 << 20;
const PROTOCOL_VERSIONS: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

fn tool_list() -> Value {
    json!({"tools": [{
        "name": TOOL,
        "description": "Convert a resource described by an http:, https:, file: or data: URI to markdown",
        "inputSchema": {
            "properties": {"uri": {"title": "Uri", "type": "string"}},
            "required": ["uri"],
            "type": "object",
            "title": "convert_to_markdownArguments"
        },
        "outputSchema": {
            "properties": {"result": {"title": "Result", "type": "string"}},
            "required": ["result"],
            "type": "object",
            "title": "convert_to_markdownOutput"
        }
    }]})
}

/// `os.strerror(errno)`: the OS message without Rust's " (os error N)".
fn strerror(e: &std::io::Error) -> String {
    match e.raw_os_error() {
        Some(code) => {
            let text = std::io::Error::from_raw_os_error(code).to_string();
            match text.rfind(" (os error ") {
                Some(i) => text[..i].to_string(),
                None => text,
            }
        }
        None => "the resource could not be read".into(),
    }
}

/// The tool body: Markdown, or the message markitdown-mcp's ToolError carries.
fn tool_body(uri: &str) -> Result<String, String> {
    // markitdown-mcp only takes URIs; plain local paths are accepted too
    let (data, info) = fetch::load(uri).map_err(|e| match e {
        SourceError::Invalid(m) => m,
        SourceError::Http { status, .. } => {
            format!("Fetching the resource failed with HTTP status {status}.")
        }
        SourceError::Request { .. } => "Could not fetch the resource.".into(),
        SourceError::Io { error, .. } => {
            format!("Could not read the resource: {}.", strerror(&error))
        }
    })?;
    // a conversion that panics fails this call, not the server
    let converted = std::thread::Builder::new()
        .stack_size(CONVERT_STACK)
        .spawn(move || {
            let opts = Options {
                youtube_transcript: Some(fetch::transcript_hook()),
                ..Default::default()
            };
            markitdown_core::convert(&data, &info, &opts)
        })
        .map_err(|_| "File conversion failed.".to_string())?
        .join()
        .map_err(|_| "File conversion failed.".to_string())?;
    converted.map_err(|e| match e {
        Error::Unsupported(_) => "Could not convert stream to Markdown. No converter attempted a \
                                  conversion, suggesting that the filetype is simply not supported."
            .into(),
        _ => "File conversion failed.".into(),
    })
}

fn tool_text(text: String, is_error: bool) -> Value {
    let mut r = json!({"content": [{"type": "text", "text": text}], "isError": is_error});
    if !is_error {
        r["structuredContent"] = json!({ "result": r["content"][0]["text"] });
    }
    r
}

fn call_tool(params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or("");
    if name != TOOL {
        return tool_text(format!("Unknown tool: {name}"), true);
    }
    let fail = |m: String| tool_text(format!("Error executing tool {TOOL}: {m}"), true);
    let validation = |what: &str| {
        fail(format!(
            "1 validation error for convert_to_markdownArguments\nuri\n  {what}"
        ))
    };
    match params["arguments"].get("uri") {
        None => validation("Field required"),
        Some(Value::String(uri)) => match tool_body(uri) {
            Ok(md) => tool_text(md, false),
            Err(m) => fail(m),
        },
        Some(_) => validation("Input should be a valid string"),
    }
}

/// One JSON-RPC message; None for notifications.
pub(crate) fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = PROTOCOL_VERSIONS
                .into_iter()
                .find(|v| *v == asked)
                .unwrap_or(PROTOCOL_VERSIONS[3]);
            json!({
                "capabilities": {
                    "prompts": {"listChanged": false},
                    "resources": {"listChanged": false, "subscribe": false},
                    "tools": {"listChanged": false}
                },
                "protocolVersion": version,
                "serverInfo": {"name": "markitdown", "version": env!("CARGO_PKG_VERSION")}
            })
        }
        "ping" => json!({}),
        "tools/list" => tool_list(),
        "tools/call" => call_tool(&msg["params"]),
        "prompts/list" => json!({"prompts": []}),
        "resources/list" => json!({"resources": []}),
        "resources/templates/list" => json!({"resourceTemplates": []}),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "Method not found", "data": method}
            }));
        }
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// `markitdown mcp [--http] [--host HOST] [--port PORT]`, markitdown-mcp's
/// options (`--sse` is its deprecated alias of `--http`).
pub fn main(args: impl Iterator<Item = String>) -> Result<(), String> {
    let (mut http, mut host, mut port) = (false, None, None);
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |what: &str| {
            inline
                .clone()
                .or_else(|| args.next())
                .ok_or_else(|| format!("argument {what}: expected one argument"))
        };
        match name.as_str() {
            "--http" | "--sse" => http = true,
            "--host" => host = Some(value("--host")?),
            "--port" => {
                let v = value("--port")?;
                port = Some(
                    v.parse::<u16>()
                        .map_err(|_| format!("argument --port: invalid int value: '{v}'"))?,
                );
            }
            "-h" | "--help" => {
                println!(
                    "usage: markitdown mcp [-h] [--http] [--sse] [--host HOST] [--port PORT]\n\n\
                     Run a MarkItDown MCP server (stdio unless --http).\n\n\
                     options:\n  \
                     --http       Run the server with Streamable HTTP and SSE transport rather than STDIO\n  \
                     --sse        (Deprecated) An alias for --http\n  \
                     --host HOST  Host to bind to (default: 127.0.0.1)\n  \
                     --port PORT  Port to listen on (default: 3001)"
                );
                return Ok(());
            }
            other => return Err(format!("unrecognized arguments: {other}")),
        }
    }
    if !http && (host.is_some() || port.is_some()) {
        return Err(
            "Host and port arguments are only valid when using streamable HTTP or SSE \
                    transport (see: --http)."
                .into(),
        );
    }
    if http {
        crate::mcp_http::serve(host.as_deref().unwrap_or("127.0.0.1"), port.unwrap_or(3001))
    } else {
        serve()
    }
}

/// Serve newline-delimited JSON-RPC on stdin/stdout until stdin closes.
pub fn serve() -> Result<(), String> {
    eprintln!(
        "markitdown-rs {} MCP server on stdio",
        env!("CARGO_PKG_VERSION")
    );
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&msg),
            Err(e) => Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32700, "message": format!("Parse error: {e}")}
            })),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")
                .and_then(|_| out.flush())
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_like_markitdown_mcp() {
        let e = std::io::Error::from_raw_os_error(2);
        if cfg!(unix) {
            assert_eq!(strerror(&e), "No such file or directory");
        }
        assert!(!strerror(&e).contains("os error"));
        let r = call_tool(&json!({"name": "nope", "arguments": {}}));
        assert_eq!(r["content"][0]["text"], "Unknown tool: nope");
        assert_eq!(r["isError"], true);
        assert!(
            handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none()
        );
    }
}
