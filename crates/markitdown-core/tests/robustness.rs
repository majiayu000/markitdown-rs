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

/// The workbook with every FORMULA record written with the BIFF4 opcode
/// 0x0406, as JExcelApi does for some cells; xlrd reads those the same.
fn with_biff4_formula_opcodes(xls: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read, Seek, Write};
    let mut ole = cfb::CompoundFile::open(Cursor::new(xls.to_vec())).unwrap();
    let mut stream = Vec::new();
    ole.open_stream("/Workbook")
        .unwrap()
        .read_to_end(&mut stream)
        .unwrap();
    let mut pos = 0;
    while pos + 4 <= stream.len() {
        let rc = u16::from_le_bytes([stream[pos], stream[pos + 1]]);
        let len = u16::from_le_bytes([stream[pos + 2], stream[pos + 3]]) as usize;
        if rc == 0x0006 {
            stream[pos..pos + 2].copy_from_slice(&0x0406u16.to_le_bytes());
        }
        pos += 4 + len;
    }
    // same length, written over the old bytes
    let mut workbook = ole.open_stream("/Workbook").unwrap();
    workbook.seek(std::io::SeekFrom::Start(0)).unwrap();
    workbook.write_all(&stream).unwrap();
    workbook.flush().unwrap();
    drop(workbook);
    ole.flush().unwrap();
    ole.into_inner().into_inner()
}

#[test]
fn xls_formulas_with_biff4_opcodes_keep_their_values() {
    for name in ["poi-46368.xls", "poi-27349-vlookupAcrossSheets.xls"] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let xls = std::fs::read(root.join("tests/fixtures/xls").join(name)).unwrap();
        let want =
            std::fs::read_to_string(root.join("tests/snapshots/xls").join(format!("{name}.md")))
                .unwrap();
        let patched = with_biff4_formula_opcodes(&xls);
        assert_ne!(patched, xls);
        assert_eq!(
            md(&patched, StreamInfo::from_path(name)) + "\n",
            want,
            "{name}"
        );
    }
}

#[test]
fn youtube_transcript_hook_adds_a_section() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let page = std::fs::read(root.join("tests/fixtures/html/gen-youtube.html")).unwrap();
    let want = std::fs::read_to_string(root.join("tests/url/youtube.md")).unwrap();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = seen.clone();
    let opts = markitdown_core::Options {
        youtube_transcript: Some(markitdown_core::TranscriptHook(std::sync::Arc::new(
            move |id: &str| {
                log.lock().unwrap().push(id.to_string());
                Some("never gonna give you up".into())
            },
        ))),
        ..Default::default()
    };
    let info = StreamInfo {
        url: Some("https://youtu.be/5C_HPTJg5ek".into()),
        ..StreamInfo::from_path("watch.html")
    };
    let got = convert(&page, &info, &opts).unwrap();
    assert_eq!(
        got,
        format!(
            "{}\n\n### Transcript\nnever gonna give you up\n",
            want.trim_end()
        )
    );
    assert_eq!(*seen.lock().unwrap(), ["5C_HPTJg5ek"]);
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in entries {
        w.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

#[test]
fn archive_limits_fail_the_conversion_cleanly() {
    let text = vec![b'a'; 10_000];
    let archive = zip_of(&[("a.txt", &text), ("b.txt", &text)]);
    let info = || StreamInfo::from_path("x.zip");
    let opts = |bytes, depth| markitdown_core::Options {
        max_unzipped_bytes: Some(bytes),
        max_zip_depth: Some(depth),
        ..Default::default()
    };
    // within the limits
    assert!(convert(&archive, &info(), &opts(20_000, 1)).is_ok());
    // the second member goes over the byte budget
    let err = convert(&archive, &info(), &opts(15_000, 1)).unwrap_err();
    assert!(matches!(err, markitdown_core::Error::Limit(_)), "{err}");
    // Office packages share the budget
    let docx = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/docx/markitdown-test.docx"),
    )
    .unwrap();
    let err = convert(&docx, &StreamInfo::from_path("t.docx"), &opts(1_000, 1)).unwrap_err();
    assert!(matches!(err, markitdown_core::Error::Limit(_)), "{err}");
    // nesting: a zip in a zip in a zip
    let inner = zip_of(&[("a.txt", b"hi")]);
    let middle = zip_of(&[("inner.zip", &inner)]);
    let outer = zip_of(&[("middle.zip", &middle)]);
    assert!(
        convert(&outer, &info(), &opts(1 << 20, 3))
            .unwrap()
            .contains("hi")
    );
    let err = convert(&outer, &info(), &opts(1 << 20, 2)).unwrap_err();
    assert!(err.to_string().contains("nested more than 2 deep"), "{err}");
}

#[test]
fn declared_sizes_are_not_trusted() {
    let opts = markitdown_core::Options {
        max_unzipped_bytes: Some(1 << 20),
        ..Default::default()
    };
    // an entry whose central directory claims 4 GiB but inflates to 5 bytes
    let mut forged = zip_of(&[("a.txt", b"hello")]);
    let cd = forged.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    forged[cd + 24..cd + 28].copy_from_slice(&0xFFFF_FFF0u32.to_le_bytes());
    let md = convert(&forged, &StreamInfo::from_path("f.zip"), &opts).unwrap();
    assert!(md.ends_with("## File: a.txt\n\nhello"), "{md}");
    // a real bomb: 4 MiB of zeros, a few KiB compressed, over the 1 MiB budget
    let bomb = zip_of(&[("zeros.txt", &vec![b'0'; 4 << 20])]);
    assert!(bomb.len() < 64 << 10);
    let err = convert(&bomb, &StreamInfo::from_path("b.zip"), &opts).unwrap_err();
    assert!(matches!(err, markitdown_core::Error::Limit(_)), "{err}");
}

#[test]
fn truncated_outlook_message_keeps_what_is_there() {
    // the first 70% of a message: its FAT names sectors past the end of the
    // file. markitdown's olefile reads the streams that are still there.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let msg = std::fs::read(root.join("tests/fixtures/msg/poi-simple_test_msg.msg")).unwrap();
    let cut = &msg[..msg.len() * 70 / 100];
    let doc = markitdown_core::convert_document(
        cut,
        &StreamInfo::from_path("cut.msg"),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        doc.markdown,
        "# Email Message\n\n**From:** travis@overwrittenstack.com\n**To:** travis@overwrittenstack.com\n\n## Content\n\nThis is a test message."
    );
    // the subject stream was cut off
    assert_eq!(doc.title, None);
}

#[test]
fn bytes_python_leaves_undefined_fail_a_charset_hint() {
    // 0x98 is undefined in Python's cp1251, so a cp1251 hint on Shift-JIS
    // text fails and markitdown falls through to the sniffed CSV as cp932
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let csv = std::fs::read(root.join("tests/fixtures/csv/markitdown-test_mskanji.csv")).unwrap();
    let want =
        std::fs::read_to_string(root.join("tests/snapshots/csv/markitdown-test_mskanji.csv.md"))
            .unwrap();
    let info = StreamInfo {
        extension: Some(".txt".into()),
        charset: Some("cp1251".into()),
        ..Default::default()
    };
    assert_eq!(md(&csv, info) + "\n", want);
}

#[test]
fn utf16_without_a_bom_is_decoded_as_markitdown_does() {
    let enc = |s: &str, be: bool| -> Vec<u8> {
        s.encode_utf16()
            .flat_map(|u| if be { u.to_be_bytes() } else { u.to_le_bytes() })
            .collect()
    };
    let page = "<html><body><p>Grüße 你好 café</p></body></html>";
    let english = "<html><head><title>T</title></head><body><h1>Hello</h1><p>Plain English text here.</p></body></html>";
    let texts = [
        "Grüße 你好 café\nsecond line\n",
        "你好世界，这是一个测试。今天天气很好，我们去公园散步吧。\n第二行文字也是中文。\n",
        "Привет мир, это тест кодировки без метки порядка байтов.\n",
        "Smile 😀 and wave 👋 to everyone here today.\n",
    ];
    for be in [false, true] {
        assert_eq!(
            md(&enc(page, be), StreamInfo::from_path("p.html")),
            "Grüße 你好 café"
        );
        for t in texts {
            assert_eq!(md(&enc(t, be), StreamInfo::from_path("t.txt")), t, "{t}");
        }
    }
    // markitdown sniffs UTF-16BE HTML as HTML from its content alone
    let be = enc(english, true);
    assert_eq!(
        md(&be, StreamInfo::default()),
        "# Hello\n\nPlain English text here."
    );
    assert_eq!(
        md(&be, StreamInfo::from_path("e.html")),
        "# Hello\n\nPlain English text here."
    );
    // a CSV without hints becomes a table
    let csv = enc("name,age\nAnn,30\nBob,25\n", false);
    assert_eq!(
        md(&csv, StreamInfo::default()),
        "| name | age |\n| --- | --- |\n| Ann | 30 |\n| Bob | 25 |"
    );
    // legacy 8-bit text is not mistaken for UTF-16
    let gbk = b"\xd6\xd0\xce\xc4\xb1\xe0\xc2\xeb\xb2\xe2\xca\xd4\n";
    let info = StreamInfo {
        charset: Some("gbk".into()),
        ..StreamInfo::from_path("g.txt")
    };
    assert_eq!(md(gbk, info), "中文编码测试\n");
}
