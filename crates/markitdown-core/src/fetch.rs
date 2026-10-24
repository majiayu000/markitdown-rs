//! markitdown's `convert()` source handling (the `fetch` feature): http(s),
//! file: and data: URIs besides local paths, with the stream info
//! `convert_response` and `convert_uri` derive from them.

use crate::StreamInfo;
use std::fmt;
use std::io::Read;

/// Why a source could not be read, in the classes markitdown's frontends
/// report differently.
#[derive(Debug)]
pub enum SourceError {
    /// What markitdown raises as ValueError: a malformed or unsupported URI.
    Invalid(String),
    /// The server answered with an HTTP error status.
    Http { url: String, status: u16 },
    /// The request failed (DNS, TLS, connection, body).
    Request { url: String, message: String },
    /// A local file could not be read.
    Io { path: String, error: std::io::Error },
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::Invalid(m) => f.write_str(m),
            SourceError::Http { url, status } => write!(f, "{url}: HTTP status {status}"),
            SourceError::Request { url, message } => write!(f, "{url}: {message}"),
            SourceError::Io { path, error } => write!(f, "{path}: {error}"),
        }
    }
}

impl std::error::Error for SourceError {}

type Loaded = Result<(Vec<u8>, StreamInfo), SourceError>;

/// What `urlparse(source.strip()).scheme.lower()` finds, if markitdown
/// handles it as a URI.
pub fn uri_scheme(source: &str) -> Option<&'static str> {
    let s = source.trim();
    let i = s.find(':')?;
    let scheme = &s[..i];
    let valid = s.as_bytes()[0].is_ascii_alphabetic()
        && scheme
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'));
    if !valid {
        return None;
    }
    ["http", "https", "file", "data"]
        .into_iter()
        .find(|k| scheme.eq_ignore_ascii_case(k))
}

/// `os.path.splitext(p)[1]`
fn splitext(p: &str) -> &str {
    let base = &p[p.rfind('/').map_or(0, |i| i + 1)..];
    let stem_start = base.len() - base.trim_start_matches('.').len();
    match base[stem_start..].rfind('.') {
        Some(i) => &base[stem_start + i..],
        None => "",
    }
}

/// `urllib.parse.unquote_to_bytes`
fn unquote_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(v) = std::str::from_utf8(&b[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// `base64.b64decode(data)`: characters outside the alphabet are dropped,
/// bad padding is an error.
fn b64decode(s: &str) -> Result<Vec<u8>, SourceError> {
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let body = s.split('=').next().unwrap_or("");
    let digits: Vec<u8> = body.bytes().filter_map(val).collect();
    let pad = s[body.len()..].bytes().filter(|&c| c == b'=').count();
    match digits.len() % 4 {
        1 => return Err(SourceError::Invalid("Invalid base64-encoded string".into())),
        r if r != 0 && pad < 4 - r => return Err(SourceError::Invalid("Incorrect padding".into())),
        _ => {}
    }
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let n = chunk.iter().fold(0u32, |a, &d| a << 6 | d as u32) << (6 * (4 - chunk.len()));
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Ok(out)
}

/// `parse_data_uri()`: the data, and its mimetype and charset as stream info
fn parse_data_uri(uri: &str) -> Loaded {
    let (header, data) = uri
        .split_once(',')
        .ok_or_else(|| SourceError::Invalid("Malformed data URI, missing ',' separator".into()))?;
    let mut parts: Vec<&str> = header[5..].split(';').collect();
    let is_base64 = parts
        .last()
        .is_some_and(|p| p.eq_ignore_ascii_case("base64"));
    if is_base64 {
        parts.pop();
    }
    let mut mimetype = None;
    if parts.first().is_some_and(|p| !p.is_empty()) {
        mimetype = Some(parts.remove(0).to_string());
    }
    let mut charset = None;
    for p in parts {
        if let Some((k, v)) = p.split_once('=')
            && k.eq_ignore_ascii_case("charset")
        {
            charset = Some(v.to_string());
        }
    }
    let content = if is_base64 {
        b64decode(data)?
    } else {
        unquote_bytes(data)
    };
    let info = StreamInfo {
        mimetype,
        charset,
        ..Default::default()
    };
    Ok((content, info))
}

/// The filename in a Content-Disposition header (RFC 2231 `filename*`
/// first), as `email.message.Message.get_params` reads it.
fn disposition_filename(header: &str) -> Option<String> {
    let mut fallback = None;
    let mut extended = None;
    for param in header.split(';').skip(1) {
        let Some((key, value)) = param.split_once('=') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();
        let unquoted = match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Some(v) => v.replace("\\\"", "\"").replace("\\\\", "\\"),
            None => value.to_string(),
        };
        if key == "filename*" {
            // charset'language'percent-encoded
            let mut it = unquoted.splitn(3, '\'');
            let (charset, _, text) = (it.next(), it.next(), it.next());
            if let Some(text) = text {
                let bytes = unquote_bytes(text);
                let decoded = match charset.map(str::to_lowercase).as_deref() {
                    Some("utf-8" | "utf8") | None | Some("") => {
                        String::from_utf8_lossy(&bytes).into_owned()
                    }
                    _ => bytes.iter().map(|&b| b as char).collect(),
                };
                extended = Some(decoded);
            }
        } else if key == "filename" && fallback.is_none() {
            fallback = Some(unquoted);
        }
    }
    extended.or(fallback)
}

/// Fetch with markitdown's session: its Accept header, redirects followed,
/// HTTP errors raised.
fn fetch(url: &str) -> Loaded {
    use ureq::ResponseExt;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(30)
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .header(
            "Accept",
            "text/markdown, text/html;q=0.9, text/plain;q=0.8, */*;q=0.1",
        )
        .header(
            "User-Agent",
            concat!("markitdown-rs/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(status) => SourceError::Http {
                url: url.to_string(),
                status,
            },
            e => SourceError::Request {
                url: url.to_string(),
                message: e.to_string(),
            },
        })?;
    let final_url = resp.get_uri().to_string();
    let header = |name: &str| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let mut info = StreamInfo::default();
    if let Some(ct) = header("content-type") {
        let mut parts = ct.split(';');
        info.mimetype = parts.next().map(|m| m.trim().to_string());
        for part in parts {
            if let Some(cs) = part.trim().strip_prefix("charset=") {
                let cs = cs.split('=').next().unwrap_or("").trim();
                if !cs.is_empty() {
                    info.charset = Some(cs.to_string());
                }
            }
        }
    }
    if let Some(name) = header("content-disposition").and_then(|cd| disposition_filename(&cd)) {
        let ext = splitext(&name);
        if !ext.is_empty() {
            info.extension = Some(ext.to_string());
        }
        info.filename = Some(name);
    }
    if info.filename.is_none() {
        let path = final_url.split(['?', '#']).next().unwrap_or("");
        let path = path
            .split_once("://")
            .map_or(path, |(_, rest)| rest.find('/').map_or("", |i| &rest[i..]));
        let ext = splitext(path);
        if !ext.is_empty() {
            info.filename = Some(path.rsplit('/').next().unwrap_or("").to_string());
            info.extension = Some(ext.to_string());
        }
    }
    info.url = Some(final_url);
    let mut data = Vec::new();
    resp.body_mut()
        .as_reader()
        .read_to_end(&mut data)
        .map_err(|e| SourceError::Request {
            url: url.to_string(),
            message: e.to_string(),
        })?;
    Ok((data, info))
}

/// A local path as `convert_local` reads it.
pub fn load_path(path: &str) -> Loaded {
    let data = std::fs::read(path).map_err(|error| SourceError::Io {
        path: path.to_string(),
        error,
    })?;
    Ok((data, StreamInfo::from_path(path)))
}

/// What `MarkItDown.convert(source)` reads: a URI, else a local path.
pub fn load(source: &str) -> Loaded {
    if uri_scheme(source).is_some() {
        load_uri(source)
    } else {
        load_path(source)
    }
}

/// `convert_uri`: http(s), file: and data: URIs only.
pub fn load_uri(source: &str) -> Loaded {
    let uri = source.trim();
    let Some(scheme) = uri_scheme(uri) else {
        let shown = uri.split(':').next().unwrap_or(uri);
        return Err(SourceError::Invalid(format!(
            "Unsupported URI scheme: {shown}. Supported schemes are: file:, data:, http:, https:"
        )));
    };
    match scheme {
        "file" => {
            let rest = &uri[5..];
            let (netloc, path) = match rest.strip_prefix("//") {
                Some(r) => r.find('/').map_or((r, ""), |i| (&r[..i], &r[i..])),
                None => ("", rest),
            };
            let path = path.split(['?', '#']).next().unwrap_or("");
            let decoded = unquote_bytes(path);
            if decoded.iter().take(2).all(|&b| b == b'/' || b == b'\\') && decoded.len() >= 2 {
                return Err(SourceError::Invalid(format!(
                    "Unsupported file URI: {uri}. UNC and Windows device paths are not supported."
                )));
            }
            if !netloc.is_empty() && netloc != "localhost" {
                return Err(SourceError::Invalid(format!(
                    "Unsupported file URI: {uri}. Netloc must be empty or localhost."
                )));
            }
            let mut path = String::from_utf8_lossy(&decoded).into_owned();
            // file:///C:/x on Windows
            if cfg!(windows) && path.starts_with(['/', '\\']) && path.get(2..3) == Some(":") {
                path.remove(0);
            }
            let abs = std::path::absolute(&path).map_err(|error| SourceError::Io {
                path: path.clone(),
                error,
            })?;
            load_path(&abs.to_string_lossy())
        }
        "data" => parse_data_uri(uri),
        _ => fetch(uri),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemes_as_urlparse_sees_them() {
        assert_eq!(uri_scheme(" HTTPS://example.com"), Some("https"));
        assert_eq!(uri_scheme("data:,x"), Some("data"));
        assert_eq!(uri_scheme("file:///tmp/a.docx"), Some("file"));
        assert_eq!(uri_scheme("C:\\docs\\a.docx"), None);
        assert_eq!(uri_scheme("report.docx"), None);
        assert_eq!(uri_scheme("ftp://host/x"), None);
    }

    #[test]
    fn data_uris() {
        let (d, i) = parse_data_uri("data:text/plain;charset=latin-1;base64,Y2Fm6Q==").unwrap();
        assert_eq!(d, b"caf\xe9");
        assert_eq!(
            (i.mimetype.as_deref(), i.charset.as_deref()),
            (Some("text/plain"), Some("latin-1"))
        );
        let (d, i) = parse_data_uri("data:,a%20b").unwrap();
        assert_eq!((d, i.mimetype), (b"a b".to_vec(), None));
        assert!(parse_data_uri("data:;base64,abc").is_err());
    }

    #[test]
    fn content_disposition() {
        assert_eq!(
            disposition_filename("attachment; filename=\"report.pdf\"").as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            disposition_filename(
                "attachment; filename=x.pdf; filename*=UTF-8''%E2%82%AC%20rates.docx"
            )
            .as_deref(),
            Some("€ rates.docx")
        );
        assert_eq!(disposition_filename("inline").as_deref(), None);
    }

    #[test]
    fn extensions() {
        assert_eq!(splitext("/files/report.v2.pdf"), ".pdf");
        assert_eq!(splitext("/files/.hidden"), "");
        assert_eq!(splitext("/wiki/Rust_(programming_language)"), "");
    }
}
