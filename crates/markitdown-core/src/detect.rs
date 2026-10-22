//! Content sniffing standing in for magika: a label with the MIME type and
//! extensions magika reports for it. Only the labels the converters can act
//! on are produced; anything else is "unknown", like magika's fallback.

use crate::zip::Archive;

#[derive(Debug, Clone, PartialEq)]
pub struct Sniff {
    pub mimetype: &'static str,
    pub extensions: &'static [&'static str],
    pub is_text: bool,
}

const fn sniff(
    mimetype: &'static str,
    extensions: &'static [&'static str],
    is_text: bool,
) -> Sniff {
    Sniff {
        mimetype,
        extensions,
        is_text,
    }
}

pub fn identify(data: &[u8]) -> Option<Sniff> {
    if data.is_empty() {
        return None;
    }
    if data.starts_with(b"%PDF-") {
        return Some(sniff("application/pdf", &["pdf"], false));
    }
    if data.starts_with(b"PK\x03\x04") {
        return Some(identify_zip(data));
    }
    if data.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        return identify_ole(data);
    }
    if data.starts_with(b"\xFF\xD8\xFF") {
        return Some(sniff("image/jpeg", &["jpg", "jpeg"], false));
    }
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(sniff("image/png", &["png"], false));
    }
    if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        return Some(sniff("image/gif", &["gif"], false));
    }
    if data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WAVE" {
        return Some(sniff("audio/x-wav", &["wav"], false));
    }
    if data.starts_with(b"ID3") || (data.len() > 1 && data[0] == 0xFF && data[1] & 0xE0 == 0xE0) {
        return Some(sniff("audio/mpeg", &["mp3"], false));
    }
    if data.len() > 12 && &data[4..8] == b"ftyp" {
        return Some(sniff(
            "video/mp4",
            &["mov", "mp4", "m4a", "m4v", "3gp"],
            false,
        ));
    }
    identify_text(data)
}

fn identify_zip(data: &[u8]) -> Sniff {
    let Ok(mut z) = Archive::new(data) else {
        return sniff("application/zip", &["zip"], false);
    };
    if z.contains("word/document.xml") {
        return sniff(
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            &["docx", "docm"],
            false,
        );
    }
    if z.contains("ppt/presentation.xml") {
        return sniff(
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            &["pptx", "pptm"],
            false,
        );
    }
    if z.contains("xl/workbook.xml") {
        return sniff(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            &["xlsx", "xlsm"],
            false,
        );
    }
    if z.read("mimetype")
        .ok()
        .flatten()
        .is_some_and(|m| m.starts_with(b"application/epub+zip"))
    {
        return sniff("application/epub+zip", &["epub"], false);
    }
    sniff("application/zip", &["zip"], false)
}

fn identify_ole(data: &[u8]) -> Option<Sniff> {
    // directory entry names are UTF-16LE; look for the stream names
    let has = |name: &str| {
        let pat: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        data.windows(pat.len()).any(|w| w == pat.as_slice())
    };
    if has("__substg1.0_") {
        Some(sniff("application/vnd.ms-outlook", &["msg"], false))
    } else if has("Workbook") || has("Book") {
        Some(sniff(
            "application/vnd.ms-excel",
            &["xls", "xlt", "xla"],
            false,
        ))
    } else if has("WordDocument") {
        Some(sniff("application/msword", &["doc"], false))
    } else if has("PowerPoint Document") {
        Some(sniff("application/vnd.ms-powerpoint", &["ppt"], false))
    } else {
        None
    }
}

fn identify_text(data: &[u8]) -> Option<Sniff> {
    let head = &data[..data.len().min(65536)];
    let text = match std::str::from_utf8(head) {
        Ok(t) => t.to_string(),
        // a split character at the sample boundary is still text
        Err(e) if e.error_len().is_none() => {
            String::from_utf8_lossy(&head[..e.valid_up_to()]).into_owned()
        }
        Err(_) => {
            // legacy-encoded text: no NUL bytes and mostly printable
            let printable = head
                .iter()
                .filter(|&&b| b >= 0x20 || matches!(b, b'\t' | b'\n' | b'\r'))
                .count();
            if head.contains(&0) || printable * 100 < head.len() * 95 {
                return None;
            }
            String::from_utf8_lossy(head).into_owned()
        }
    };
    if text.contains('\0') {
        return None;
    }
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let lower: String = t.chars().take(1024).collect::<String>().to_lowercase();
    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.contains("<head")
        || lower.contains("<body")
    {
        return Some(sniff("text/html", &["html", "htm", "xhtml", "xht"], true));
    }
    if lower.starts_with("<?xml") || lower.starts_with("<rss") || lower.starts_with("<feed") {
        return Some(sniff("text/xml", &["xml"], true));
    }
    if (t.starts_with('{') || t.starts_with('['))
        && serde_json::from_str::<serde_json::Value>(&text).is_ok()
    {
        if t.contains("\"nbformat\"") && t.contains("\"cells\"") {
            return Some(sniff("application/json", &["ipynb"], true));
        }
        return Some(sniff("application/json", &["json"], true));
    }
    if looks_like_csv(t) {
        return Some(sniff("text/csv", &["csv"], true));
    }
    if t.starts_with("# ") || t.contains("\n## ") || t.contains("\n# ") {
        return Some(sniff("text/markdown", &["md", "markdown"], true));
    }
    Some(sniff("text/plain", &["txt"], true))
}

/// At least two lines with the same number (> 0) of commas.
fn looks_like_csv(t: &str) -> bool {
    let counts: Vec<usize> = t
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(20)
        .map(|l| l.matches(',').count())
        .collect();
    counts.len() >= 2 && counts[0] > 0 && counts.iter().all(|&c| c == counts[0])
}
