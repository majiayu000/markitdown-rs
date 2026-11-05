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
        "utf-32" | "utf32" => match bytes {
            [0xff, 0xfe, 0, 0, rest @ ..] => utf32(rest, false, lossy),
            [0, 0, 0xfe, 0xff, rest @ ..] => utf32(rest, true, lossy),
            _ => utf32(bytes, false, lossy),
        },
        "utf-32-le" | "utf-32le" => utf32(bytes, false, lossy),
        "utf-32-be" | "utf-32be" => utf32(bytes, true, lossy),
        _ => {
            let enc = Encoding::for_label(python_to_whatwg(&label).as_bytes())?;
            let undefined = python_undefined(enc.name());
            if !undefined.is_empty() && bytes.iter().any(|b| undefined.contains(b)) {
                if !lossy {
                    return None;
                }
                // single-byte code pages: one character per byte
                let text = enc.decode_without_bom_handling(bytes).0;
                return Some(
                    text.chars()
                        .zip(bytes)
                        .map(|(c, b)| if undefined.contains(b) { '\u{fffd}' } else { c })
                        .collect(),
                );
            }
            if lossy {
                Some(enc.decode_without_bom_handling(bytes).0.into_owned())
            } else {
                enc.decode_without_bom_handling_and_without_replacement(bytes)
                    .map(|c| c.into_owned())
            }
        }
    }
}

/// Bytes CPython's Windows code pages leave undefined (the WHATWG tables
/// encoding_rs follows map them to C1 controls). cp1252 is handled above.
fn python_undefined(whatwg: &str) -> &'static [u8] {
    match whatwg {
        "windows-1250" => &[0x81, 0x83, 0x88, 0x90, 0x98],
        "windows-1251" => &[0x98],
        "windows-1253" => &[
            0x81, 0x88, 0x8A, 0x8C, 0x8D, 0x8E, 0x8F, 0x90, 0x98, 0x9A, 0x9C, 0x9D, 0x9E, 0x9F,
            0xAA, 0xD2, 0xFF,
        ],
        "windows-1254" => &[0x81, 0x8D, 0x8E, 0x8F, 0x90, 0x9D, 0x9E],
        "windows-1255" => &[
            0x81, 0x8A, 0x8C, 0x8D, 0x8E, 0x8F, 0x90, 0x9A, 0x9C, 0x9D, 0x9E, 0x9F, 0xCA, 0xD9,
            0xDA, 0xDB, 0xDC, 0xDD, 0xDE, 0xDF, 0xFB, 0xFC, 0xFF,
        ],
        "windows-1257" => &[
            0x81, 0x83, 0x88, 0x8A, 0x8C, 0x90, 0x98, 0x9A, 0x9C, 0x9F, 0xA1, 0xA5,
        ],
        "windows-1258" => &[0x81, 0x8A, 0x8D, 0x8E, 0x8F, 0x90, 0x9A, 0x9D, 0x9E],
        _ => &[],
    }
}

/// UTF-32 (encoding_rs has none): invalid code points and a trailing
/// partial unit are errors, or U+FFFD when lossy.
fn utf32(bytes: &[u8], big_endian: bool, lossy: bool) -> Option<String> {
    let (units, rest) = bytes.as_chunks::<4>();
    let mut out = String::with_capacity(units.len());
    for unit in units {
        let v = if big_endian {
            u32::from_be_bytes(*unit)
        } else {
            u32::from_le_bytes(*unit)
        };
        match char::from_u32(v) {
            Some(c) => out.push(c),
            None if lossy => out.push('\u{fffd}'),
            None => return None,
        }
    }
    if !rest.is_empty() {
        if !lossy {
            return None;
        }
        out.push('\u{fffd}');
    }
    Some(out)
}

/// Text a reader would expect: ASCII and Latin, general and CJK
/// punctuation, kana, fullwidth forms, emoji, and the ideographs and Hangul
/// in the frequent levels of GB2312, Big5, JIS X 0208 and KS X 1001. Legacy
/// double-byte text misread as UTF-16 turns into rare ideographs instead.
fn is_common(c: char) -> bool {
    let o = c as u32;
    if matches!(c, '\t' | '\n' | '\r')
        || (0x20..0x7f).contains(&o)
        || (0xa0..0x250).contains(&o)
        || (0x370..0x530).contains(&o)
        || (0x2000..0x2070).contains(&o)
        || (0x3000..0x3100).contains(&o)
        || (0xff00..0xfff0).contains(&o)
        || (0x1f300..0x1fb00).contains(&o)
    {
        return true;
    }
    if !((0x3400..0xa000).contains(&o) || (0xac00..0xd7a4).contains(&o)) {
        return false;
    }
    let mut buf = [0u8; 4];
    let s = c.encode_utf8(&mut buf);
    [
        (encoding_rs::GBK, 0xb0a1, 0xd7f9),
        (encoding_rs::BIG5, 0xa440, 0xc67e),
        (encoding_rs::SHIFT_JIS, 0x889f, 0x9872),
        (encoding_rs::EUC_KR, 0xb0a1, 0xc8fe),
    ]
    .iter()
    .any(|(enc, lo, hi)| {
        let (bytes, _, errors) = enc.encode(s);
        !errors
            && bytes.len() == 2
            && (*lo..=*hi).contains(&(u32::from(bytes[0]) << 8 | u32::from(bytes[1])))
    })
}

/// UTF-32 or UTF-16 without a byte order mark that charset_normalizer would
/// pick: a clean decode with no control characters whose text is almost all
/// common characters (real CJK text scores about 1, legacy encodings read
/// as UTF-16 at most about 0.4).
fn bomless_unicode_by_content(bytes: &[u8]) -> Option<&'static str> {
    let score = |name: &str| -> Option<f64> {
        let text = decode(bytes, name)?;
        let mut n = 0usize;
        let mut common = 0usize;
        for c in text.chars() {
            if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r' | '\x0c') {
                return None;
            }
            n += 1;
            common += usize::from(is_common(c));
        }
        (n > 0).then(|| common as f64 / n as f64)
    };
    let pick = |names: [&'static str; 2]| {
        names
            .into_iter()
            .filter_map(|n| score(n).filter(|s| *s >= 0.9).map(|s| (n, s)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n)
    };
    if bytes.len().is_multiple_of(4)
        && let Some(n) = pick(["utf-32-le", "utf-32-be"])
    {
        return Some(n);
    }
    if bytes.len().is_multiple_of(2) {
        return pick(["utf-16-le", "utf-16-be"]);
    }
    None
}

/// A Unicode encoding the bytes are in, as charset_normalizer finds it:
/// a UTF-32 byte order mark, UTF-16 by its NUL bytes, or UTF-32/UTF-16
/// by content when the bytes are not UTF-8.
pub fn sniff_unicode(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\xff\xfe\0\0") || bytes.starts_with(b"\0\0\xfe\xff") {
        return Some("utf-32");
    }
    if let Some(name) = bomless_utf16(bytes) {
        return Some(name);
    }
    if std::str::from_utf8(bytes).is_ok()
        || bytes.starts_with(b"\xff\xfe")
        || bytes.starts_with(b"\xfe\xff")
    {
        return None;
    }
    bomless_unicode_by_content(bytes)
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
/// UTF-16 without a byte order mark, as charset_normalizer finds it: an
/// even length, NUL bytes on one side of the code units only (from spaces,
/// newlines and other ASCII-range characters), and a clean decode in that
/// byte order with no control characters.
pub fn bomless_utf16(bytes: &[u8]) -> Option<&'static str> {
    if bytes.len() < 4
        || bytes.len() % 2 == 1
        || bytes.starts_with(b"\xff\xfe")
        || bytes.starts_with(b"\xfe\xff")
    {
        return None;
    }
    let zeros = |parity: usize| {
        bytes
            .iter()
            .skip(parity)
            .step_by(2)
            .filter(|&&b| b == 0)
            .count()
    };
    let (even, odd) = (zeros(0), zeros(1));
    let (high, low, name, enc) = if odd > even {
        (odd, even, "utf-16-le", encoding_rs::UTF_16LE)
    } else {
        (even, odd, "utf-16-be", encoding_rs::UTF_16BE)
    };
    // CJK characters can put a NUL in the other half (U+4E00 and so on);
    // the clean decode below is what rules out binary data
    if high == 0 || low * 2 > high {
        return None;
    }
    // a NUL or two (in CJK text, from U+4E00 and the like) says little
    // about the byte order: decide by content then
    if high * 20 < bytes.len() / 2 {
        return bomless_unicode_by_content(bytes).filter(|n| n.starts_with("utf-16"));
    }
    let text = enc.decode_without_bom_handling_and_without_replacement(bytes)?;
    let control = text
        .chars()
        .any(|c| (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r' | '\x0c'));
    (!control).then_some(name)
}

pub fn detect(bytes: &[u8]) -> String {
    if let Some(name) = sniff_unicode(bytes) {
        return name.into();
    }
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
