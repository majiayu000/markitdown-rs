//! Byte decoding with Python codec semantics for the encodings that matter,
//! and charset detection standing in for charset_normalizer.

use encoding_rs::Encoding;

/// Python's cp1252 table for 0x80..=0x9f; None marks bytes the codec rejects.
const CP1252_HIGH: [Option<char>; 32] = {
    const fn c(u: u32) -> Option<char> {
        char::from_u32(u)
    }
    [
        c(0x20ac),
        None,
        c(0x201a),
        c(0x192),
        c(0x201e),
        c(0x2026),
        c(0x2020),
        c(0x2021),
        c(0x2c6),
        c(0x2030),
        c(0x160),
        c(0x2039),
        c(0x152),
        None,
        c(0x17d),
        None,
        None,
        c(0x2018),
        c(0x2019),
        c(0x201c),
        c(0x201d),
        c(0x2022),
        c(0x2013),
        c(0x2014),
        c(0x2dc),
        c(0x2122),
        c(0x161),
        c(0x203a),
        c(0x153),
        None,
        c(0x17e),
        c(0x178),
    ]
};

fn normalize(label: &str) -> String {
    label.trim().to_ascii_lowercase().replace('_', "-")
}

/// `bytes.decode(encoding)` (strict). None when the bytes do not decode or
/// the encoding is unknown.
pub fn decode(bytes: &[u8], encoding: &str) -> Option<String> {
    decode_with(bytes, encoding, false)
}

/// `bytes.decode(encoding, errors="replace")`
pub fn decode_lossy(bytes: &[u8], encoding: &str) -> Option<String> {
    decode_with(bytes, encoding, true)
}

fn decode_with(bytes: &[u8], encoding: &str, lossy: bool) -> Option<String> {
    let label = normalize(encoding);
    match label.as_str() {
        "utf-8" | "utf8" | "u8" | "utf" => {
            if lossy {
                Some(String::from_utf8_lossy(bytes).into_owned())
            } else {
                std::str::from_utf8(bytes).ok().map(str::to_string)
            }
        }
        "ascii" | "us-ascii" | "646" => {
            if bytes.is_ascii() {
                Some(String::from_utf8_lossy(bytes).into_owned())
            } else if lossy {
                Some(
                    bytes
                        .iter()
                        .map(|&b| if b < 0x80 { b as char } else { '\u{fffd}' })
                        .collect(),
                )
            } else {
                None
            }
        }
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" | "l1" | "8859" | "cp819" => {
            Some(bytes.iter().map(|&b| b as char).collect())
        }
        "cp1252" | "windows-1252" => bytes
            .iter()
            .map(|&b| match b {
                0x80..=0x9f => CP1252_HIGH[(b - 0x80) as usize].or(lossy.then_some('\u{fffd}')),
                _ => Some(b as char),
            })
            .collect(),
        "utf-16" | "utf16" => {
            let (body, enc) = match bytes {
                [0xfe, 0xff, rest @ ..] => (rest, encoding_rs::UTF_16BE),
                [0xff, 0xfe, rest @ ..] => (rest, encoding_rs::UTF_16LE),
                _ => (bytes, encoding_rs::UTF_16LE),
            };
            utf16(body, enc, lossy)
        }
        "utf-16-le" | "utf-16le" => utf16(bytes, encoding_rs::UTF_16LE, lossy),
        "utf-16-be" | "utf-16be" => utf16(bytes, encoding_rs::UTF_16BE, lossy),
        _ => {
            let enc = Encoding::for_label(python_to_whatwg(&label).as_bytes())?;
            if lossy {
                Some(enc.decode_without_bom_handling(bytes).0.into_owned())
            } else {
                enc.decode_without_bom_handling_and_without_replacement(bytes)
                    .map(|c| c.into_owned())
            }
        }
    }
}

fn utf16(bytes: &[u8], enc: &'static Encoding, lossy: bool) -> Option<String> {
    if bytes.len() % 2 == 1 && !lossy {
        return None;
    }
    if lossy {
        Some(enc.decode_without_bom_handling(bytes).0.into_owned())
    } else {
        enc.decode_without_bom_handling_and_without_replacement(bytes)
            .map(|c| c.into_owned())
    }
}

/// Python codec names to WHATWG labels where they differ.
fn python_to_whatwg(label: &str) -> String {
    let l = label.replace('-', "");
    match l.as_str() {
        "cp932" | "ms932" | "mskanji" | "windows31j" | "shiftjis" | "sjis" | "shiftjis2004" => {
            "shift_jis".into()
        }
        "eucjp" | "ujis" => "euc-jp".into(),
        "iso2022jp" => "iso-2022-jp".into(),
        "gb2312" | "gbk" | "cp936" | "ms936" | "gb18030" => "gb18030".into(),
        "big5" | "big5hkscs" | "cp950" => "big5".into(),
        "euckr" | "cp949" | "uhc" => "euc-kr".into(),
        "koi8r" => "koi8-r".into(),
        "koi8u" => "koi8-u".into(),
        "macroman" | "macintosh" => "macintosh".into(),
        _ if l.starts_with("cp125") => format!("windows-{}", &l[2..]),
        _ if l.starts_with("iso8859") => format!("iso-8859-{}", &l[7..]),
        _ => label.to_string(),
    }
}

/// The charset charset_normalizer would pick, as a Python codec name.
pub fn detect(bytes: &[u8]) -> String {
    if std::str::from_utf8(bytes).is_ok() {
        return if bytes.is_ascii() {
            "ascii".into()
        } else {
            "utf-8".into()
        };
    }
    if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        return "utf-16".into();
    }
    let mut det = chardetng::EncodingDetector::new();
    det.feed(bytes, true);
    let enc = det.guess(None, true);
    match enc.name() {
        "Shift_JIS" => "cp932".into(),
        "GBK" | "gb18030" => "gb18030".into(),
        "macintosh" => "mac-roman".into(),
        name if name.starts_with("windows-") => format!("cp{}", &name[8..]),
        name => name.to_ascii_lowercase(),
    }
}

/// `str(charset_normalizer.from_bytes(data).best())`, falling back to
/// `data.decode("utf-8", errors="ignore")`.
pub fn decode_detected(bytes: &[u8]) -> String {
    let (bytes, bom_utf8) = match bytes.strip_prefix(b"\xEF\xBB\xBF") {
        Some(rest) => (rest, true),
        None => (bytes, false),
    };
    let enc = if bom_utf8 {
        "utf-8".to_string()
    } else {
        detect(bytes)
    };
    decode(bytes, &enc).unwrap_or_else(|| String::from_utf8_lossy(bytes).replace('\u{fffd}', ""))
}
