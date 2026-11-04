//! Inputs that once came out garbled or crashed, against what Python
//! markitdown 0.1.9 gives for them.

use markitdown_core::{Options, StreamInfo, convert};

fn utf16(text: &str, big_endian: bool) -> Vec<u8> {
    let mut out = if big_endian {
        vec![0xfe, 0xff]
    } else {
        vec![0xff, 0xfe]
    };
    for unit in text.encode_utf16() {
        let bytes = if big_endian {
            unit.to_be_bytes()
        } else {
            unit.to_le_bytes()
        };
        out.extend_from_slice(&bytes);
    }
    out
}

fn md(data: &[u8], info: StreamInfo) -> String {
    convert(data, &info, &Options::default()).unwrap()
}

#[test]
fn utf16_with_bom_is_decoded() {
    let html = "<html><head><title>T</title></head><body><h1>Hé</h1><p>x</p></body></html>";
    let text = "Hello wörld\nline 2\n";
    for be in [false, true] {
        let page = utf16(html, be);
        assert_eq!(md(&page, StreamInfo::from_path("a.html")), "# Hé\n\nx");
        // sniffed from the content alone, and with an explicit html hint
        assert_eq!(md(&page, StreamInfo::default()), "# Hé\n\nx");
        let hint = StreamInfo {
            extension: Some(".html".into()),
            ..Default::default()
        };
        assert_eq!(md(&page, hint), "# Hé\n\nx");
        let plain = utf16(text, be);
        assert_eq!(md(&plain, StreamInfo::from_path("a.txt")), text);
        assert_eq!(md(&plain, StreamInfo::default()), text);
    }
}

fn nested(n: usize, inner: &str) -> Vec<u8> {
    format!(
        "<html><body>{}{inner}{}</body></html>",
        "<div>".repeat(n),
        "</div>".repeat(n)
    )
    .into_bytes()
}

#[test]
fn deep_html_falls_back_to_plain_text_at_pythons_depth() {
    let info = || StreamInfo::from_path("deep.html");
    // markdownify recurses until 493 elements below <body>
    assert_eq!(md(&nested(491, "<b>x</b>"), info()), "**x**");
    assert_eq!(md(&nested(492, "<b>x</b>"), info()), "x");
    assert_eq!(md(&nested(492, "x"), info()), "x");
    assert_eq!(md(&nested(50_000, "<p>a</p><b>b</b>"), info()), "a\nb");
}

#[test]
fn deep_html_on_a_wikipedia_url_uses_the_plain_html_fallback() {
    let info = StreamInfo {
        url: Some("https://en.wikipedia.org/wiki/X".into()),
        ..StreamInfo::from_path("X.html")
    };
    let page = format!(
        "<html><head><title>X</title></head><body><div id=\"mw-content-text\">{}<b>x</b>{}</div></body></html>",
        "<div>".repeat(600),
        "</div>".repeat(600)
    );
    assert_eq!(md(page.as_bytes(), info), "x");
}
