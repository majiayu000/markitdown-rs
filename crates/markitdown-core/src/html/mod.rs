//! HTML -> Markdown as markitdown's HtmlConverter does it: decode like
//! BeautifulSoup's UnicodeDammit, build bs4's html.parser tree, drop
//! `<script>`/`<style>`, convert `<body>` (or the whole document).

mod entities;
pub mod parser;

use crate::dom::{Dom, ROOT};
use crate::{markdownify, pystr, text};
use entities::{HTML5, INVALID_CHARREFS, INVALID_CODEPOINTS};

/// `html.escape(s, quote=True)`
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// `html.unescape()`
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        if b[i] != b'&' {
            i += 1;
            continue;
        }
        let Some((end, rep)) = charref_at(s, i) else {
            i += 1;
            continue;
        };
        out.push_str(&s[copied..i]);
        out.push_str(&rep);
        i = end;
        copied = end;
    }
    out.push_str(&s[copied..]);
    out
}

/// One match of `&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)`
/// at `i`, with its replacement.
fn charref_at(s: &str, i: usize) -> Option<(usize, String)> {
    let b = s.as_bytes();
    let j = i + 1;
    if b.get(j) == Some(&b'#') {
        let (start, radix) = match b.get(j + 1) {
            Some(b'x' | b'X') if b.get(j + 2).is_some_and(u8::is_ascii_hexdigit) => (j + 2, 16),
            Some(c) if c.is_ascii_digit() => (j + 1, 10),
            _ => return None,
        };
        let mut e = start;
        while e < b.len()
            && (if radix == 16 {
                b[e].is_ascii_hexdigit()
            } else {
                b[e].is_ascii_digit()
            })
        {
            e += 1;
        }
        let num = u128::from_str_radix(&s[start..e], radix).unwrap_or(u128::MAX);
        let end = if b.get(e) == Some(&b';') { e + 1 } else { e };
        let rep = match INVALID_CHARREFS.binary_search_by_key(&num, |(k, _)| *k as u128) {
            Ok(k) => INVALID_CHARREFS[k].1.to_string(),
            Err(_) if (0xd800..=0xdfff).contains(&num) || num > 0x10ffff => "\u{fffd}".into(),
            Err(_) if INVALID_CODEPOINTS.binary_search(&(num as u32)).is_ok() => String::new(),
            Err(_) => char::from_u32(num as u32)
                .map(String::from)
                .unwrap_or_default(),
        };
        return Some((end, rep));
    }
    // named: up to 32 chars outside [\t\n\f <&#;], then an optional ';'
    let mut e = j;
    let mut count = 0;
    for (off, c) in s[j..].char_indices() {
        if matches!(c, '\t' | '\n' | '\x0c' | ' ' | '<' | '&' | '#' | ';') || count == 32 {
            e = j + off;
            break;
        }
        count += 1;
        e = j + off + c.len_utf8();
    }
    if count == 0 {
        return None;
    }
    let end = if b.get(e) == Some(&b';') { e + 1 } else { e };
    let name = &s[j..end];
    let lookup = |n: &str| {
        HTML5
            .binary_search_by(|(k, _)| (*k).cmp(n))
            .ok()
            .map(|i| HTML5[i].1)
    };
    if let Some(v) = lookup(name) {
        return Some((end, v.to_string()));
    }
    let boundaries: Vec<usize> = name
        .char_indices()
        .map(|(o, _)| o)
        .filter(|&o| o >= 2)
        .collect();
    for &x in boundaries.iter().rev() {
        if let Some(v) = lookup(&name[..x]) {
            return Some((end, format!("{v}{}", &name[x..])));
        }
    }
    Some((end, format!("&{name}")))
}

/// UnicodeDammit with `known_definite_encodings=[encoding]`: strip a BOM,
/// try the given encoding, then the BOM's, the `<meta>` one, a detected one,
/// utf-8 and windows-1252, and finally decode with replacement characters.
pub fn decode(bytes: &[u8], encoding: &str) -> String {
    let (markup, sniffed) = strip_bom(bytes);
    let mut tried: Vec<String> = Vec::new();
    let mut candidates: Vec<String> = vec![encoding.to_string()];
    candidates.extend(sniffed.map(str::to_string));
    candidates.extend(declared_encoding(markup));
    candidates.push(text::detect(markup));
    candidates.push("utf-8".into());
    candidates.push("windows-1252".into());
    for enc in candidates {
        let key = enc.to_lowercase();
        if tried.contains(&key) {
            continue;
        }
        tried.push(key);
        if let Some(s) = text::decode(markup, &enc) {
            return s;
        }
    }
    text::decode_lossy(markup, "utf-8").unwrap_or_default()
}

fn strip_bom(data: &[u8]) -> (&[u8], Option<&'static str>) {
    if data.len() >= 4 && data.starts_with(b"\xfe\xff") && &data[2..4] != b"\0\0" {
        (&data[2..], Some("utf-16be"))
    } else if data.len() >= 4 && data.starts_with(b"\xff\xfe") && &data[2..4] != b"\0\0" {
        (&data[2..], Some("utf-16le"))
    } else if let Some(rest) = data.strip_prefix(b"\xef\xbb\xbf") {
        (rest, Some("utf-8"))
    } else if let Some(rest) = data.strip_prefix(b"\0\0\xfe\xff") {
        (rest, Some("utf-32be"))
    } else if let Some(rest) = data.strip_prefix(b"\xff\xfe\0\0") {
        (rest, Some("utf-32le"))
    } else {
        (data, None)
    }
}

/// The charset of an XML declaration or `<meta>` near the start.
fn declared_encoding(markup: &[u8]) -> Option<String> {
    let html_end = markup.len().min(2048.max(markup.len() / 20));
    let head = String::from_utf8_lossy(&markup[..html_end]).to_lowercase();
    let pos = head.find("charset=").or_else(|| head.find("encoding="))?;
    let rest = &head[pos..];
    let rest = rest.split_once('=')?.1.trim_start_matches(['"', '\'', ' ']);
    let value: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || "-_.:".contains(*c))
        .collect();
    (!value.is_empty()).then_some(value)
}

/// Remove every `<script>` and `<style>` element (`soup(["script", "style"])`).
pub fn remove_scripts(dom: &mut Dom) {
    for id in dom.find_all(ROOT, &["script", "style"]) {
        dom.remove(id);
    }
}

pub(crate) fn parse_bytes(bytes: &[u8], charset: Option<&str>) -> Dom {
    parser::parse(&decode(bytes, charset.unwrap_or("utf-8")))
}

fn markdownify(dom: &Dom, root: usize, keep_data_uris: bool) -> String {
    markdownify::convert(dom, root, &markdownify::Options { keep_data_uris })
}

/// markitdown's `HtmlConverter.convert` on raw bytes.
pub fn convert(bytes: &[u8], charset: Option<&str>, keep_data_uris: bool) -> String {
    let mut dom = parse_bytes(bytes, charset);
    remove_scripts(&mut dom);
    let root = dom.find(ROOT, "body").unwrap_or(ROOT);
    pystr::strip(&markdownify(&dom, root, keep_data_uris)).to_string()
}

/// `^https?:\/\/[a-zA-Z]{2,3}\.wikipedia.org\/`
/// `^https?:\/\/[a-zA-Z]{2,3}(\.m)?\.wikipedia.org\/` (the mobile host
/// since 0.1.9)
pub fn is_wikipedia_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let lang = rest.bytes().take_while(u8::is_ascii_alphabetic).count();
    let host = |r: &str| {
        r.strip_prefix(".wikipedia").is_some_and(|r| {
            let mut it = r.chars();
            it.next().is_some_and(|c| c != '\n') && it.as_str().starts_with("org/")
        })
    };
    let rest = &rest[lang..];
    (2..=3).contains(&lang) && (rest.strip_prefix(".m").is_some_and(host) || host(rest))
}

pub fn convert_wikipedia(bytes: &[u8], charset: Option<&str>, keep_data_uris: bool) -> String {
    let mut dom = parse_bytes(bytes, charset);
    remove_scripts(&mut dom);
    let mut found = Vec::new();
    dom.find_all_by(
        ROOT,
        &|d, c| d.name(c) == Some("div") && d.attr(c, "id") == Some("mw-content-text"),
        &mut found,
    );
    let body = found.first().copied();
    // the title span exists only for plain-text titles; titles with markup
    // (italic species or film names) are in the first heading
    let mut titles = Vec::new();
    dom.find_all_by(
        ROOT,
        &|d, c| d.name(c) == Some("span") && d.has_class(c, "mw-page-title-main"),
        &mut titles,
    );
    if titles.is_empty() {
        dom.find_all_by(
            ROOT,
            &|d, c| d.name(c) == Some("h1") && d.attr(c, "id") == Some("firstHeading"),
            &mut titles,
        );
    }
    let mut main_title = dom
        .find(ROOT, "title")
        .and_then(|t| dom.string(t))
        .map(str::to_string);
    let Some(body) = body else {
        return markdownify(&dom, ROOT, keep_data_uris);
    };
    if let Some(&t) = titles.first() {
        main_title = Some(dom.get_text(t, "", false));
    }
    let main_title = main_title
        .map(|t| pystr::strip(&t).to_string())
        .filter(|t| !t.is_empty());
    let head = main_title.map(|t| format!("# {t}\n\n")).unwrap_or_default();
    head + &markdownify(&dom, body, keep_data_uris)
}

pub fn convert_bing(
    bytes: &[u8],
    info: &crate::StreamInfo,
    keep_data_uris: bool,
) -> Result<String, crate::Error> {
    let url = info.url.as_deref().unwrap_or_default();
    let query = crate::pyurl::urlparse(url, "")
        .ok()
        .and_then(|u| {
            crate::pyurl::parse_qs(&u.query)
                .into_iter()
                .find(|(k, _)| k == "q")
        })
        .and_then(|(_, v)| v.into_iter().next())
        .unwrap_or_default();
    let mut dom = parse_bytes(bytes, info.charset.as_deref());
    let mut found = Vec::new();
    dom.find_all_by(ROOT, &|d, c| d.has_class(c, "tptt"), &mut found);
    for t in found {
        if let Some(s) = dom
            .string(t)
            .filter(|s| !s.is_empty())
            .map(|s| format!("{s} "))
        {
            dom.set_string(t, s);
        }
    }
    let mut slugs = Vec::new();
    dom.find_all_by(ROOT, &|d, c| d.has_class(c, "algoSlug_icon"), &mut slugs);
    for s in slugs {
        dom.remove(s);
    }
    let mut algos = Vec::new();
    dom.find_all_by(ROOT, &|d, c| d.has_class(c, "b_algo"), &mut algos);
    let mut results = Vec::new();
    for result in algos {
        let mut links = Vec::new();
        dom.find_all_by(
            result,
            &|d, c| d.name(c) == Some("a") && d.attr(c, "href").is_some(),
            &mut links,
        );
        for a in links {
            let href = dom.attr(a, "href").unwrap_or_default().to_string();
            let Ok(parsed) = crate::pyurl::urlparse(&href, "") else {
                continue;
            };
            let qs = crate::pyurl::parse_qs(&parsed.query);
            let Some((_, u)) = qs.iter().find(|(k, _)| k == "u") else {
                continue;
            };
            let u = &u[0];
            let tail: String = u.chars().skip(2).collect();
            let encoded = format!("{}==", pystr::strip(&tail));
            if let Some(decoded) = crate::base64::decode_urlsafe_lenient(&encoded)
                .and_then(|b| String::from_utf8(b).ok())
            {
                dom.set_attr(a, "href", decoded);
            }
        }
        let md = markdownify(&dom, result, keep_data_uris);
        let lines: Vec<&str> = pystr::strip(&md)
            .split('\n')
            .map(pystr::strip)
            .filter(|l| !l.is_empty())
            .collect();
        results.push(lines.join("\n"));
    }
    Ok(format!(
        "## A Bing search for '{query}' found the following results:\n\n{}",
        results.join("\n\n")
    ))
}

/// `HtmlConverter.convert_string`, used by converters that build HTML.
pub fn convert_string(html: &str, keep_data_uris: bool) -> String {
    let mut dom = parser::parse(html);
    remove_scripts(&mut dom);
    let root = dom.find(ROOT, "body").unwrap_or(ROOT);
    pystr::strip(&markdownify(&dom, root, keep_data_uris)).to_string()
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn unescape_matches_python() {
        let cases = [
            ("&amp;", "&"),
            ("&amp", "&"),
            ("&ampx", "&x"),
            ("&notit;", "\u{ac}it;"),
            ("&#65;&#x42;&#67", "ABC"),
            ("&#0;", "\u{fffd}"),
            ("&#128;", "\u{20ac}"),
            ("&#1;x", "x"),
            ("&#x110000;", "\u{fffd}"),
            ("& &# &x;", "& &# &x;"),
        ];
        for (input, want) in cases {
            assert_eq!(unescape(input), want, "{input}");
        }
    }
}
