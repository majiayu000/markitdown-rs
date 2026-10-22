//! The `markitdown` binary on URL sources, against a local HTTP server.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn snapshot(name: &str) -> String {
    std::fs::read_to_string(root().join("tests/snapshots").join(name)).unwrap()
}

/// Serves `n` requests: each path maps to (status line, headers, body).
fn serve(n: usize, route: fn(&str) -> (String, Vec<u8>)) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(n) {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() <= 2 {
                    break;
                }
            }
            let path = request.split(' ').nth(1).unwrap_or("/").to_string();
            let (head, body) = route(&path);
            let _ = write!(
                stream,
                "{head}Content-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{addr}")
}

fn run(arg: &str) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_markitdown"))
        .arg(arg)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn docx() -> Vec<u8> {
    std::fs::read(root().join("tests/fixtures/docx/markitdown-test.docx")).unwrap()
}

#[test]
fn http_source_named_by_content_disposition() {
    let base = serve(1, |_| {
        let head = "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
                    Content-Disposition: attachment; filename=\"report.docx\"\r\n";
        (head.into(), docx())
    });
    let (ok, out, err) = run(&format!("{base}/download?id=7"));
    assert!(ok, "{err}");
    assert_eq!(out, snapshot("docx/markitdown-test.docx.md"));
}

#[test]
fn http_redirect_then_extension_from_url() {
    let base = serve(2, |path| match path {
        "/old" => (
            "HTTP/1.1 302 Found\r\nLocation: /files/test.docx\r\n".into(),
            Vec::new(),
        ),
        _ => (
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n".into(),
            docx(),
        ),
    });
    let (ok, out, err) = run(&format!("{base}/old"));
    assert!(ok, "{err}");
    assert_eq!(out, snapshot("docx/markitdown-test.docx.md"));
}

#[test]
fn http_error_status_fails() {
    let base = serve(1, |_| {
        (
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/html\r\n".into(),
            b"nope".to_vec(),
        )
    });
    let (ok, _, err) = run(&format!("{base}/missing.pdf"));
    assert!(!ok);
    assert!(err.contains("404"), "{err}");
}

#[test]
fn data_uri() {
    let (ok, out, _) = run("data:text/html;base64,PGgxPkhlbGxvPC9oMT48cD53b3JsZDwvcD4=");
    assert!(ok);
    assert_eq!(out, "# Hello\n\nworld\n");
}

#[cfg(unix)]
#[test]
fn file_uri() {
    let path = root()
        .join("tests/fixtures/docx/markitdown-test.docx")
        .canonicalize()
        .unwrap();
    let (ok, out, err) = run(&format!("file://{}", path.display()));
    assert!(ok, "{err}");
    assert_eq!(out, snapshot("docx/markitdown-test.docx.md"));
}
