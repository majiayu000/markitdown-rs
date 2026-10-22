//! The parts of CPython 3.12's `urllib.parse` the converters depend on:
//! `urlparse`/`urlunparse`, `urljoin`, `parse_qs` and `quote`.

const USES_PARAMS: &[&str] = &[
    "", "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp", "rtsps", "rtspu",
    "sip", "sips", "mms", "sftp", "tel",
];
const USES_NETLOC: &[&str] = &[
    "",
    "ftp",
    "http",
    "gopher",
    "nntp",
    "telnet",
    "imap",
    "wais",
    "file",
    "mms",
    "https",
    "shttp",
    "snews",
    "prospero",
    "rtsp",
    "rtsps",
    "rtspu",
    "rsync",
    "svn",
    "svn+ssh",
    "sftp",
    "nfs",
    "git",
    "git+ssh",
    "ws",
    "wss",
    "itms-services",
];
const USES_RELATIVE: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "imap", "wais", "file", "https", "shttp", "mms",
    "prospero", "rtsp", "rtsps", "rtspu", "sftp", "svn", "svn+ssh", "ws", "wss",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Url {
    pub scheme: String,
    pub netloc: String,
    pub path: String,
    pub params: String,
    pub query: String,
    pub fragment: String,
}

/// `urlparse(url, scheme=default_scheme)`. Err for an invalid IPv6 netloc.
pub fn urlparse(url: &str, default_scheme: &str) -> Result<Url, ()> {
    let mut url: String = url.trim_start_matches(|c: char| c <= ' ').to_string();
    url.retain(|c| !matches!(c, '\t' | '\r' | '\n'));
    let mut scheme: String = default_scheme.trim_matches(|c: char| c <= ' ').to_string();
    scheme.retain(|c| !matches!(c, '\t' | '\r' | '\n'));
    if let Some(i) = url.find(':') {
        let cand = &url[..i];
        if i > 0
            && cand.starts_with(|c: char| c.is_ascii_alphabetic())
            && cand
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            scheme = cand.to_ascii_lowercase();
            url = url[i + 1..].to_string();
        }
    }
    let mut netloc = String::new();
    if url.starts_with("//") {
        let delim = ['/', '?', '#']
            .iter()
            .filter_map(|&c| url[2..].find(c).map(|i| i + 2))
            .min();
        let delim = delim.unwrap_or(url.len());
        netloc = url[2..delim].to_string();
        url = url[delim..].to_string();
        if netloc.contains('[') != netloc.contains(']') {
            return Err(());
        }
    }
    let mut fragment = String::new();
    if let Some(i) = url.find('#') {
        fragment = url[i + 1..].to_string();
        url.truncate(i);
    }
    let mut query = String::new();
    if let Some(i) = url.find('?') {
        query = url[i + 1..].to_string();
        url.truncate(i);
    }
    let mut params = String::new();
    if USES_PARAMS.contains(&scheme.as_str()) && url.contains(';') {
        let i = match url.rfind('/') {
            Some(slash) => url[slash..].find(';').map(|j| j + slash),
            None => url.find(';'),
        };
        if let Some(i) = i {
            params = url[i + 1..].to_string();
            url.truncate(i);
        }
    }
    Ok(Url {
        scheme,
        netloc,
        path: url,
        params,
        query,
        fragment,
    })
}

pub fn urlunparse(u: &Url) -> String {
    let mut path = u.path.clone();
    if !u.params.is_empty() {
        path = format!("{path};{}", u.params);
    }
    let mut out = if !u.netloc.is_empty() {
        if !path.is_empty() && !path.starts_with('/') {
            path.insert(0, '/');
        }
        format!("//{}{path}", u.netloc)
    } else if path.starts_with("//")
        || (!u.scheme.is_empty()
            && USES_NETLOC.contains(&u.scheme.as_str())
            && (path.is_empty() || path.starts_with('/')))
    {
        format!("//{path}")
    } else {
        path
    };
    if !u.scheme.is_empty() {
        out = format!("{}:{out}", u.scheme);
    }
    if !u.query.is_empty() {
        out = format!("{out}?{}", u.query);
    }
    if !u.fragment.is_empty() {
        out = format!("{out}#{}", u.fragment);
    }
    out
}

/// `urljoin(base, url)`; Err where Python raises ValueError.
pub fn urljoin(base: &str, url: &str) -> Result<String, ()> {
    if base.is_empty() {
        return Ok(url.to_string());
    }
    if url.is_empty() {
        return Ok(base.to_string());
    }
    let b = urlparse(base, "")?;
    let mut u = urlparse(url, &b.scheme)?;
    if u.scheme != b.scheme || !USES_RELATIVE.contains(&u.scheme.as_str()) {
        return Ok(url.to_string());
    }
    if USES_NETLOC.contains(&u.scheme.as_str()) {
        if !u.netloc.is_empty() {
            return Ok(urlunparse(&u));
        }
        u.netloc = b.netloc.clone();
    }
    if u.path.is_empty() && u.params.is_empty() {
        u.path = b.path.clone();
        u.params = b.params.clone();
        if u.query.is_empty() {
            u.query = b.query.clone();
        }
        return Ok(urlunparse(&u));
    }
    let mut base_parts: Vec<&str> = b.path.split('/').collect();
    if base_parts.last() != Some(&"") {
        base_parts.pop();
    }
    let segments: Vec<&str> = if u.path.starts_with('/') {
        u.path.split('/').collect()
    } else {
        let mut s = base_parts;
        s.extend(u.path.split('/'));
        if s.len() > 2 {
            let last = s.len() - 1;
            let mid: Vec<&str> = s[1..last]
                .iter()
                .copied()
                .filter(|x| !x.is_empty())
                .collect();
            let mut v = vec![s[0]];
            v.extend(mid);
            v.push(s[last]);
            s = v;
        }
        s
    };
    let mut resolved: Vec<&str> = Vec::new();
    for seg in &segments {
        match *seg {
            ".." => {
                resolved.pop();
            }
            "." => {}
            s => resolved.push(s),
        }
    }
    if matches!(segments.last(), Some(&".") | Some(&"..")) {
        resolved.push("");
    }
    let joined = resolved.join("/");
    u.path = if joined.is_empty() {
        "/".to_string()
    } else {
        joined
    };
    Ok(urlunparse(&u))
}

/// `quote(s, safe=safe)` with UTF-8 encoding.
pub fn quote(s: &str, safe: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &c in s.as_bytes() {
        if c.is_ascii_alphanumeric() || b"_.-~".contains(&c) || safe.as_bytes().contains(&c) {
            out.push(c as char);
        } else {
            out.push_str(&format!("%{c:02X}"));
        }
    }
    out
}

/// `unquote(s)` / `unquote_plus(s)` with errors='replace'.
pub fn unquote(s: &str, plus: bool) -> String {
    let s = if plus {
        s.replace('+', " ")
    } else {
        s.to_string()
    };
    if !s.contains('%') {
        return s;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(c: u8) -> Option<u8> {
    (c as char).to_digit(16).map(|d| d as u8)
}

/// `parse_qs(qs)` (blank values dropped), keys in first-seen order.
pub fn parse_qs(qs: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for field in qs.split('&') {
        if field.is_empty() {
            continue;
        }
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        if value.is_empty() {
            continue;
        }
        let (name, value) = (unquote(name, true), unquote(value, true));
        match out.iter_mut().find(|(k, _)| *k == name) {
            Some((_, v)) => v.push(value),
            None => out.push((name, vec![value])),
        }
    }
    out
}

/// markitdown's link rewrite: drop non-web schemes, quote the path while
/// keeping existing `%HH` escapes. None means "render the text, no link".
pub fn requote(href: &str) -> Option<String> {
    let mut u = urlparse(href, "").ok()?;
    if !u.scheme.is_empty() && !matches!(u.scheme.as_str(), "http" | "https" | "file") {
        return None;
    }
    u.path = quote_preserving_escapes(&u.path);
    Some(urlunparse(&u))
}

/// `quote(path)` on the stretches between existing `%HH` escapes.
fn quote_preserving_escapes(path: &str) -> String {
    let b = path.as_bytes();
    let mut out = String::with_capacity(path.len());
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
        {
            out.push_str(&quote(&path[start..i], "/"));
            out.push_str(&path[i..i + 3]);
            i += 3;
            start = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&quote(&path[start..], "/"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urljoin_matches_python() {
        let cases = [
            ("http://a/b/c/d;p?q", "g", "http://a/b/c/g"),
            ("http://a/b/c/d;p?q", "../g", "http://a/b/g"),
            ("http://a/b/c/d;p?q", "../../../g", "http://a/g"),
            ("http://a/b/c/d;p?q", "//g", "http://g"),
            ("http://a/b/c/d;p?q", "?y", "http://a/b/c/d;p?y"),
            ("http://a/b/c/d;p?q", "#s", "http://a/b/c/d;p?q#s"),
            ("http://a/b/c/d;p?q", ".", "http://a/b/c/"),
            ("http://a/b/c/d;p?q", "mailto:x", "mailto:x"),
            ("", "rel", "rel"),
        ];
        for (base, url, want) in cases {
            assert_eq!(urljoin(base, url).unwrap(), want, "{base} + {url}");
        }
    }
}
