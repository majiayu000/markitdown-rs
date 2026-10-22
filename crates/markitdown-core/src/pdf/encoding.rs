//! pdfminer's encodingdb: glyph names to Unicode and the Latin encodings.

use std::collections::HashMap;
use std::rc::Rc;

use super::object::Obj;
use super::tables::{ENCODING, GLYPHS};

/// Why `name2unicode` failed: KeyError or ValueError (callers catch
/// different ones).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NameError {
    Key,
    Value,
}

fn glyph(name: &str) -> Option<&'static str> {
    GLYPHS
        .binary_search_by(|(n, _)| (*n).cmp(name))
        .ok()
        .map(|i| GLYPHS[i].1)
}

/// `int(s, 16)`, which also takes a sign.
fn hex_int(s: &str) -> Result<u32, NameError> {
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'+') => (false, &s[1..]),
        Some(b'-') => (true, &s[1..]),
        _ => (false, s),
    };
    let v = u32::from_str_radix(digits, 16).map_err(|_| NameError::Value)?;
    if neg && v != 0 {
        // chr() of a negative number
        return Err(NameError::Value);
    }
    Ok(v)
}

fn chr(v: u32) -> Result<char, NameError> {
    // lone surrogates (which Python allows) have no Rust char
    char::from_u32(v)
        .or(if v <= 0x10ffff {
            Some('\u{fffd}')
        } else {
            None
        })
        .ok_or(NameError::Value)
}

pub fn name2unicode(name: &str) -> Result<String, NameError> {
    let name = name.split('.').next().unwrap_or("");
    if name.contains('_') {
        return name.split('_').map(name2unicode).collect();
    }
    if let Some(u) = glyph(name) {
        return Ok(u.to_string());
    }
    let is_hex_start = |s: &str| s.as_bytes().first().is_some_and(u8::is_ascii_hexdigit);
    if name.starts_with("uni") {
        let w = name.trim_matches(['u', 'n', 'i']);
        if is_hex_start(w) && w.chars().count().is_multiple_of(4) {
            let chars: Vec<char> = w.chars().collect();
            let digits = chars
                .chunks(4)
                .map(|c| hex_int(&c.iter().collect::<String>()))
                .collect::<Result<Vec<_>, _>>()?;
            if digits.iter().any(|d| (0xd800..0xe000).contains(d)) {
                return Err(NameError::Key);
            }
            return digits.into_iter().map(chr).collect();
        }
    } else if name.starts_with('u') {
        let w = name.trim_matches('u');
        let n = w.chars().count();
        if is_hex_start(w) && (4..=6).contains(&n) {
            let d = hex_int(w)?;
            if (0xd800..0xe000).contains(&d) {
                return Err(NameError::Key);
            }
            return Ok(chr(d)?.to_string());
        }
    }
    Err(NameError::Key)
}

pub type Cid2Unicode = HashMap<i64, String>;

struct Encodings {
    std: Rc<Cid2Unicode>,
    mac: Rc<Cid2Unicode>,
    win: Rc<Cid2Unicode>,
    pdf: Rc<Cid2Unicode>,
}

thread_local! {
    static ENCODINGS: Encodings = {
        let mut maps: [Cid2Unicode; 4] = Default::default();
        for &(name, s, m, w, p) in ENCODING {
            let c = name2unicode(name).unwrap_or_default();
            for (map, code) in maps.iter_mut().zip([s, m, w, p]) {
                if code != 0 {
                    map.insert(code as i64, c.clone());
                }
            }
        }
        let [std, mac, win, pdf] = maps.map(Rc::new);
        Encodings { std, mac, win, pdf }
    };
}

/// `EncodingDB.get_encoding(name, diff)`
pub fn get_encoding(name: &str, diff: Option<&[Obj]>) -> Rc<Cid2Unicode> {
    let base = ENCODINGS.with(|e| match name {
        "MacRomanEncoding" => e.mac.clone(),
        "WinAnsiEncoding" => e.win.clone(),
        "PDFDocEncoding" => e.pdf.clone(),
        _ => e.std.clone(),
    });
    let Some(diff) = diff.filter(|d| !d.is_empty()) else {
        return base;
    };
    let mut map = (*base).clone();
    let mut cid = 0i64;
    for x in diff {
        if let Some(i) = x.as_int() {
            cid = i;
        } else if let Obj::Name(n) = x {
            if let Ok(s) = std::str::from_utf8(n)
                && let Ok(u) = name2unicode(s)
            {
                map.insert(cid, u);
            }
            cid += 1;
        }
    }
    Rc::new(map)
}
