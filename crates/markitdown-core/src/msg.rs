//! Outlook .msg files, as markitdown's OutlookMsgConverter: sender,
//! recipients and subject from the MAPI string properties, then the body.

use crate::{Error, StreamInfo, pystr, text};
use std::io::{Cursor, Read};

type Ole<'a> = cfb::CompoundFile<Cursor<&'a [u8]>>;

/// Windows code pages Python names differently from `cp<N>`.
fn codepage_codec(cp: u32) -> Option<String> {
    let name = match cp {
        0 => return None,
        708 | 28596 => "iso8859-6",
        20127 => "ascii",
        20866 => "koi8-r",
        21866 => "koi8-u",
        28591 => "iso8859-1",
        28592 => "iso8859-2",
        28593 => "iso8859-3",
        28594 => "iso8859-4",
        28595 => "iso8859-5",
        28597 => "iso8859-7",
        28598 => "iso8859-8",
        28599 => "iso8859-9",
        28603 => "iso8859-13",
        28605 => "iso8859-15",
        10000 => "mac-roman",
        50220..=50222 => "iso2022-jp",
        51932 => "euc-jp",
        51936 => "gb2312",
        51949 => "euc-kr",
        54936 => "gb18030",
        65001 => "utf-8",
        _ => return Some(format!("cp{cp}")),
    };
    Some(name.to_string())
}

fn read_stream(ole: &mut Ole, name: &str) -> Option<Vec<u8>> {
    let path = format!("/{name}");
    if !ole.is_stream(&path) {
        return None;
    }
    let mut buf = Vec::new();
    ole.open_stream(&path).ok()?.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// PT_LONG values of the top-level property stream, keyed by property id.
fn long_properties(ole: &mut Ole) -> Vec<(u32, u32)> {
    let Some(data) = read_stream(ole, "__properties_version1.0") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut off = 32;
    while off + 16 <= data.len() {
        let tag = u32::from_le_bytes(data[off..off + 4].try_into().unwrap_or_default());
        let value = u32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap_or_default());
        if tag & 0xFFFF == 0x0003 {
            match out.iter_mut().find(|(k, _)| *k == tag >> 16) {
                Some(slot) => *slot = (tag >> 16, value),
                None => out.push((tag >> 16, value)),
            }
        }
        off += 16;
    }
    out
}

/// PT_UNICODE (001F), else PT_STRING8 (001E) in the declared code page.
fn property(ole: &mut Ole, tag: &str, encoding: Option<&str>) -> Option<String> {
    if let Some(data) = read_stream(ole, &format!("__substg1.0_{tag}001F")) {
        let s = encoding_rs::UTF_16LE
            .decode_without_bom_handling_and_without_replacement(&data)
            .filter(|_| data.len() % 2 == 0)
            .map(|c| c.into_owned())
            .or_else(|| String::from_utf8(data.clone()).ok())
            .unwrap_or_else(|| String::from_utf8_lossy(&data).replace('\u{fffd}', ""));
        let s = pystr::strip(&s).to_string();
        if !s.is_empty() {
            return Some(s);
        }
    }
    let data = read_stream(ole, &format!("__substg1.0_{tag}001E"))?;
    let end = data.iter().rposition(|&b| b != 0).map_or(0, |i| i + 1);
    let data = &data[..end];
    if data.is_empty() {
        return None;
    }
    if let Some(s) = encoding.and_then(|e| text::decode(data, e)) {
        return Some(pystr::strip(&s).to_string());
    }
    Some(pystr::strip(&text::decode_detected(data)).to_string())
}

pub fn accepts(data: &[u8], info: &StreamInfo) -> bool {
    let ext = info.extension.as_deref().unwrap_or_default().to_lowercase();
    if ext == ".msg"
        || info
            .mimetype
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .starts_with("application/vnd.ms-outlook")
    {
        return true;
    }
    let Ok(ole) = cfb::CompoundFile::open(Cursor::new(data)) else {
        return false;
    };
    let names: Vec<String> = ole.walk().map(|e| e.name().to_string()).collect();
    names.iter().any(|n| n.contains("__properties_version1.0"))
        && names
            .iter()
            .any(|n| n.contains("__recip_version1.0_#00000000"))
}

pub fn convert(data: &[u8]) -> Result<String, Error> {
    let mut ole = cfb::CompoundFile::open(Cursor::new(data))
        .map_err(|e| Error::Conversion(format!("not an OLE file: {e}")))?;
    let props = long_properties(&mut ole);
    let prop = |id: u32| props.iter().find(|(k, _)| *k == id).map(|(_, v)| *v);
    let known = |c: Option<String>| c.filter(|name| text::decode(b"", name).is_some());
    let message = known(prop(0x3FFD).and_then(codepage_codec));
    let internet = known(prop(0x3FDE).and_then(codepage_codec));
    let header_enc = message.clone().or(internet.clone());
    let body_enc = internet.or(message);
    let mut md = String::from("# Email Message\n\n");
    for (key, tag) in [("From", "0C1F"), ("To", "0E04"), ("Subject", "0037")] {
        if let Some(v) = property(&mut ole, tag, header_enc.as_deref()).filter(|v| !v.is_empty()) {
            md.push_str(&format!("**{key}:** {v}\n"));
        }
    }
    md.push_str("\n## Content\n\n");
    if let Some(body) = property(&mut ole, "1000", body_enc.as_deref()) {
        md.push_str(&body);
    }
    Ok(pystr::strip(&md).to_string())
}
