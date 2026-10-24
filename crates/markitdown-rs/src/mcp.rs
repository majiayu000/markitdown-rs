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
    markitdown_core::convert(&data, &info, &Options::default()).map_err(|e| match e {
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
fn handle(msg: &Value) -> Option<Value> {
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
        assert_eq!(strerror(&e), "No such file or directory");
        let r = call_tool(&json!({"name": "nope", "arguments": {}}));
        assert_eq!(r["content"][0]["text"], "Unknown tool: nope");
        assert_eq!(r["isError"], true);
        assert!(
            handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none()
        );
    }
}
