//! markitdown's YouTubeConverter as it runs without the optional
//! youtube-transcript-api package: the page's title, metadata and
//! description, or the generic HTML conversion when it has none.

use crate::dom::{Dom, Kind, ROOT};
use crate::{StreamInfo, html, pyurl};

/// `_get_video_id`: youtu.be/ID, youtube.com/watch?v=ID, /shorts/ID, /embed/ID.
fn video_id(url: &str) -> Option<String> {
    let u = pyurl::urlparse(url, "").ok()?;
    let host = u.netloc.to_lowercase();
    let parts: Vec<&str> = u.path.split('/').filter(|p| !p.is_empty()).collect();
    match host.as_str() {
        "youtu.be" | "www.youtu.be" => parts.first().map(|p| p.to_string()),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => match parts.first() {
            Some(&"watch") => pyurl::parse_qs(&u.query)
                .into_iter()
                .find(|(k, _)| k == "v")
                .and_then(|(_, v)| v.into_iter().next()),
            Some(&("shorts" | "embed")) => parts.get(1).map(|p| p.to_string()),
            _ => None,
        },
        _ => None,
    }
    .filter(|id| !id.is_empty())
}

pub fn accepts(info: &StreamInfo) -> bool {
    let url = pyurl::unquote(info.url.as_deref().unwrap_or(""), false)
        .replace("\\?", "?")
        .replace("\\=", "=");
    if video_id(&url).is_none() {
        return false;
    }
    let ext = info.extension.as_deref().unwrap_or("").to_lowercase();
    let mime = info.mimetype.as_deref().unwrap_or("").to_lowercase();
    ext == ".html"
        || ext == ".htm"
        || mime.starts_with("text/html")
        || mime.starts_with("application/xhtml")
}

/// The first `var ytInitialData = ({.*?});` on one line.
fn initial_data(script: &str) -> Option<&str> {
    const MARK: &str = "var ytInitialData = {";
    let mut from = 0;
    while let Some(i) = script[from..].find(MARK) {
        let start = from + i + MARK.len() - 1;
        let line_end = script[start..]
            .find('\n')
            .map_or(script.len(), |n| start + n);
        if let Some(j) = script[start..line_end].find("};") {
            return Some(&script[start..start + j + 1]);
        }
        from = start;
    }
    None
}

/// `_findKey`: depth-first, keys in document order; falsy finds keep looking.
fn find_key<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    use serde_json::Value;
    let truthy = |v: &Value| match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    };
    match v {
        Value::Array(items) => items.iter().find_map(|e| find_key(e, key)),
        Value::Object(map) => {
            for (k, val) in map {
                if k == key {
                    return Some(val);
                }
                if let Some(found) = find_key(val, key).filter(|f| truthy(f)) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn metadata(dom: &Dom) -> Vec<(String, String)> {
    let mut meta: Vec<(String, String)> = Vec::new();
    let mut set = |k: String, v: String| match meta.iter_mut().find(|(mk, _)| *mk == k) {
        Some(slot) => slot.1 = v,
        None => meta.push((k, v)),
    };
    if let Some(title) = dom.find(ROOT, "title").and_then(|t| dom.string(t))
        && !title.is_empty()
    {
        set("title".into(), title.to_string());
    }
    for m in dom.find_all(ROOT, &["meta"]) {
        let Kind::Element { attrs, .. } = dom.kind(m) else {
            continue;
        };
        if let Some((_, key)) = attrs
            .iter()
            .find(|(a, _)| matches!(a.as_str(), "itemprop" | "property" | "name"))
        {
            let content = dom.attr(m, "content").unwrap_or("");
            if !key.is_empty() && !content.is_empty() {
                set(key.clone(), content.to_string());
            }
        }
    }
    for s in dom.find_all(ROOT, &["script"]) {
        let Some(content) = dom.string(s).filter(|c| !c.is_empty()) else {
            continue;
        };
        if !content.contains("ytInitialData") {
            continue;
        }
        // markitdown prints JSON errors to stdout and carries on; this skips them
        if let Some(data) =
            initial_data(content).and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
            && let Some(serde_json::Value::Object(desc)) =
                find_key(&data, "attributedDescriptionBodyText")
        {
            let text = match desc.get("content") {
                None => String::new(),
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
            };
            set("description".into(), text);
        }
        break;
    }
    meta
}

pub fn convert(data: &[u8], info: &StreamInfo, keep_data_uris: bool) -> String {
    let dom = html::parse_bytes(data, Some(info.charset.as_deref().unwrap_or("utf-8")));
    let meta = metadata(&dom);
    let get = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| meta.iter().find(|(mk, _)| mk == k).map(|(_, v)| v.as_str()))
    };
    let mut text = String::new();
    if let Some(title) = get(&["title", "og:title", "name"]).filter(|t| !t.is_empty()) {
        text += &format!("\n## {title}\n");
    }
    let mut stats = String::new();
    for (label, key) in [
        ("Views", "interactionCount"),
        ("Keywords", "keywords"),
        ("Runtime", "duration"),
    ] {
        if let Some(v) = get(&[key]) {
            stats += &format!("- **{label}:** {v}\n");
        }
    }
    if !stats.is_empty() {
        text += &format!("\n### Video Metadata\n{stats}\n");
    }
    if let Some(d) = get(&["description", "og:description"]).filter(|d| !d.is_empty()) {
        text += &format!("\n### Description\n{d}\n");
    }
    if text.is_empty() {
        return html::convert(data, info.charset.as_deref(), keep_data_uris);
    }
    format!("# YouTube\n{text}")
}
