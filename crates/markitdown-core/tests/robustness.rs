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

#[test]
fn a_panicking_converter_is_a_conversion_error() {
    // calamine panics on this workbook cut short (an index out of range
    // in its compound-file reader); markitdown raises an ordinary error
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let xls = std::fs::read(root.join("tests/fixtures/xls/markitdown-test.xls")).unwrap();
    let cut = &xls[..xls.len() * 99 / 100];
    let err = convert(cut, &StreamInfo::from_path("cut.xls"), &Default::default()).unwrap_err();
    assert!(
        matches!(&err, markitdown_core::Error::Conversion(m) if m.contains("internal error in XlsConverter")),
        "{err}"
    );
}

/// A one-page PDF whose content stream, after "Hello bomb", decodes to a
/// long run of spaces.
fn pdf_with_stream(stream: &[u8], filter: &str) -> Vec<u8> {
    let objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_vec(),
        [
            format!("<< /Length {} /Filter /{filter} >>\nstream\n", stream.len()).into_bytes(),
            stream.to_vec(),
            b"\nendstream".to_vec(),
        ]
        .concat(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", i + 1).bytes());
        out.extend(o);
        out.extend(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objs.len() + 1
        )
        .bytes(),
    );
    out
}

/// LZW codes that each extend the previous string by one space.
fn lzw_spaces(codes: usize) -> Vec<u8> {
    let (mut out, mut acc, mut nacc) = (Vec::new(), 0u64, 0u32);
    let mut emit = |code: u64, bits: u32| {
        acc = (acc << bits) | code;
        nacc += bits;
        while nacc >= 8 {
            nacc -= 8;
            out.push((acc >> nacc) as u8);
        }
    };
    let (mut next, mut bits) = (258u64, 9u32);
    emit(256, 9);
    emit(32, 9);
    for _ in 0..codes {
        emit(next, bits);
        next += 1;
        if matches!(next, 511 | 1023 | 2047) {
            bits += 1;
        }
        if next >= 4094 {
            emit(256, bits);
            bits = 9;
            emit(32, 9);
            next = 258;
        }
    }
    emit(0, 7);
    out
}

#[test]
fn pdf_streams_share_the_decompression_budget() {
    use std::io::Write;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(b"BT /F1 12 Tf 72 720 Td (Hello bomb) Tj ET\n")
        .unwrap();
    z.write_all(&vec![b' '; 4 << 20]).unwrap();
    let flate = pdf_with_stream(&z.finish().unwrap(), "FlateDecode");
    // about 7 MB of spaces
    let lzw = pdf_with_stream(&lzw_spaces(4000), "LZWDecode");
    let budget = |bytes| markitdown_core::Options {
        max_unzipped_bytes: Some(bytes),
        ..Default::default()
    };
    let info = || StreamInfo::from_path("bomb.pdf");
    let ok = convert(&flate, &info(), &budget(16 << 20)).unwrap();
    assert_eq!(ok.trim_end(), "Hello bomb");
    for pdf in [&flate, &lzw] {
        let err = convert(pdf, &info(), &budget(1 << 20)).unwrap_err();
        assert!(matches!(err, markitdown_core::Error::Limit(_)), "{err}");
    }
}

/// The first cell record (NUMBER, LABELSST or RK) of a BIFF stream given a
/// column no sheet can have; returns the record's offset.
fn wild_column(stream: &mut [u8], from: usize) -> usize {
    let mut pos = from;
    while pos + 8 <= stream.len() {
        let rc = u16::from_le_bytes([stream[pos], stream[pos + 1]]);
        let len = u16::from_le_bytes([stream[pos + 2], stream[pos + 3]]) as usize;
        if matches!(rc, 0x0203 | 0x00fd | 0x027e) {
            stream[pos + 6..pos + 8].copy_from_slice(&0xF000u16.to_le_bytes());
            return pos;
        }
        pos += 4 + len;
    }
    panic!("no cell record");
}

#[test]
fn damaged_cell_positions_fail_without_huge_allocations() {
    use std::io::{Cursor, Read, Seek, Write};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Excel 97: calamine would lay out 4096 x 61440 cells while opening it
    let xls = std::fs::read(root.join("tests/fixtures/xls/markitdown-test.xls")).unwrap();
    let mut ole = cfb::CompoundFile::open(Cursor::new(xls)).unwrap();
    let mut stream = Vec::new();
    ole.open_stream("/Workbook")
        .unwrap()
        .read_to_end(&mut stream)
        .unwrap();
    // the first worksheet starts after the workbook globals' EOF record
    let globals_end = stream
        .windows(4)
        .position(|w| w == [0x0a, 0x00, 0x00, 0x00])
        .unwrap()
        + 4;
    wild_column(&mut stream, globals_end);
    let mut w = ole.open_stream("/Workbook").unwrap();
    w.seek(std::io::SeekFrom::Start(0)).unwrap();
    w.write_all(&stream).unwrap();
    w.flush().unwrap();
    drop(w);
    ole.flush().unwrap();
    let damaged = ole.into_inner().into_inner();
    let err = convert(
        &damaged,
        &StreamInfo::from_path("d.xls"),
        &Default::default(),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("cell position out of range"),
        "{err}"
    );

    // Excel 4: the grid would be 16 times the format's size
    let mut biff4 = std::fs::read(root.join("tests/fixtures/xls/tika-testEXCEL_4.xls")).unwrap();
    wild_column(&mut biff4, 0);
    let err = convert(
        &biff4,
        &StreamInfo::from_path("d4.xls"),
        &Default::default(),
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("cell position out of range"),
        "{err}"
    );
}

#[test]
fn cjk_utf16_and_utf32_are_told_from_legacy_encodings() {
    // no newline: the only NUL byte comes from 一 (U+4E00)
    let zh = "中华人民共和国是一个历史悠久的国家，人口众多，文化灿烂。我们今天讨论文档转换工具的编码识别问题。";
    let units = |be: bool| -> Vec<u8> {
        zh.encode_utf16()
            .flat_map(|u| if be { u.to_be_bytes() } else { u.to_le_bytes() })
            .collect()
    };
    let utf32 = |be: bool, bom: bool| -> Vec<u8> {
        let mut out = Vec::new();
        for c in bom.then_some('\u{feff}').into_iter().chain(zh.chars()) {
            out.extend(if be {
                (c as u32).to_be_bytes()
            } else {
                (c as u32).to_le_bytes()
            });
        }
        out
    };
    let txt = || StreamInfo::from_path("t.txt");
    for be in [false, true] {
        assert_eq!(md(&units(be), txt()), zh, "utf-16 be={be}");
    }
    assert_eq!(md(&utf32(false, true), txt()), zh);
    assert_eq!(md(&utf32(true, false), txt()), zh);
    // legacy double-byte text still decodes as itself without a hint
    for (enc, text) in [
        (
            encoding_rs::SHIFT_JIS,
            "日本語のテキストです。今日はいい天気ですね。東京は大きい都市です。",
        ),
        (encoding_rs::GBK, zh),
        (
            encoding_rs::BIG5,
            "中華民國是一個歷史悠久的國家，今天的天氣很好，我們一起去公園散步吧。",
        ),
        (
            encoding_rs::EUC_KR,
            "한국어 텍스트입니다. 오늘 날씨가 좋네요. 우리는 공원에 갑니다.",
        ),
    ] {
        let (bytes, _, _) = enc.encode(text);
        assert_eq!(md(&bytes, txt()), text, "{}", enc.name());
    }
    // and short ASCII UTF-16 keeps its NUL-based byte order
    assert_eq!(md(b"H\0i\0", txt()), "Hi");
}

#[test]
fn recovered_formula_below_and_right_of_existing_cells_is_not_lost() {
    use std::io::{Cursor, Write};
    fn record(out: &mut Vec<u8>, code: u16, data: &[u8]) {
        out.extend(code.to_le_bytes());
        out.extend((data.len() as u16).to_le_bytes());
        out.extend(data);
    }
    let mut workbook = Vec::new();
    let mut bof = vec![0; 16];
    bof[..2].copy_from_slice(&0x0600u16.to_le_bytes());
    bof[2..4].copy_from_slice(&0x0005u16.to_le_bytes());
    record(&mut workbook, 0x0809, &bof);
    let bound_pos = workbook.len() + 4;
    let mut bound = vec![0; 8];
    bound[6] = 5;
    bound.extend(b"Sheet");
    record(&mut workbook, 0x0085, &bound);
    record(&mut workbook, 0x000a, &[]);
    let sheet_offset = workbook.len() as u32;
    workbook[bound_pos..bound_pos + 4].copy_from_slice(&sheet_offset.to_le_bytes());
    bof[2..4].copy_from_slice(&0x0010u16.to_le_bytes());
    record(&mut workbook, 0x0809, &bof);
    let mut number = vec![0; 6];
    number.extend(1.0f64.to_le_bytes());
    record(&mut workbook, 0x0203, &number);
    let mut formula = vec![0; 6];
    formula[..2].copy_from_slice(&1u16.to_le_bytes());
    formula[2..4].copy_from_slice(&1u16.to_le_bytes());
    formula.extend(9876.0f64.to_le_bytes());
    formula.extend([0; 8]);
    record(&mut workbook, 0x0406, &formula);
    record(&mut workbook, 0x000a, &[]);
    let mut ole = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
    ole.create_stream("/Workbook")
        .unwrap()
        .write_all(&workbook)
        .unwrap();
    ole.flush().unwrap();
    let got = md(
        &ole.into_inner().into_inner(),
        StreamInfo::from_path("outlying.xls"),
    );
    assert!(
        got.contains("9876"),
        "recovered B2 formula must appear in Markdown: {got}"
    );
}
