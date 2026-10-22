//! markitdown's `_convert`: build stream-info guesses (hints, then the
//! content sniff), then offer the stream to each converter in markitdown's
//! priority order until one accepts it and succeeds.

use crate::{Error, Options, StreamInfo, detect, formats, text};

/// CPython's built-in `mimetypes` table (no system files) for the
/// extensions the converters look at.
const MIMETYPES: &[(&str, &str)] = &[
    (".atom", ""),
    (".csv", "text/csv"),
    (".gif", "image/gif"),
    (".htm", "text/html"),
    (".html", "text/html"),
    (".jpeg", "image/jpeg"),
    (".jpg", "image/jpeg"),
    (".json", "application/json"),
    (".markdown", "text/markdown"),
    (".md", "text/markdown"),
    (".mp3", "audio/mpeg"),
    (".mp4", "video/mp4"),
    (".pdf", "application/pdf"),
    (".png", "image/png"),
    (".py", "text/x-python"),
    (".rst", "text/x-rst"),
    (".svg", "image/svg+xml"),
    (".tsv", "text/tab-separated-values"),
    (".txt", "text/plain"),
    (".wav", "audio/x-wav"),
    (".xls", "application/vnd.ms-excel"),
    (".xml", "text/xml"),
    (".zip", "application/zip"),
    (".c", "text/plain"),
    (".h", "text/plain"),
    (".css", "text/css"),
    (".js", "text/javascript"),
];

fn mimetype_for(ext: &str) -> Option<&'static str> {
    let ext = ext.to_lowercase();
    MIMETYPES
        .iter()
        .find(|(e, m)| *e == ext && !m.is_empty())
        .map(|(_, m)| *m)
}

fn extension_for(mimetype: &str) -> Option<&'static str> {
    MIMETYPES
        .iter()
        .find(|(_, m)| *m == mimetype)
        .map(|(e, _)| *e)
}

/// Python's `codecs.lookup(name).name` for the names we produce.
fn normalize_charset(c: &str) -> String {
    match c.trim().to_lowercase().replace('_', "-").as_str() {
        "utf8" | "utf-8" => "utf-8".into(),
        "ascii" | "us-ascii" => "ascii".into(),
        other => other.to_string(),
    }
}

fn guesses(data: &[u8], base: &StreamInfo) -> Vec<StreamInfo> {
    let mut enhanced = base.clone();
    if base.mimetype.is_none()
        && let Some(ext) = &base.extension
    {
        enhanced.mimetype = mimetype_for(ext).map(str::to_string);
    }
    if enhanced.extension.is_none()
        && let Some(m) = &base.mimetype
    {
        enhanced.extension = extension_for(m).map(str::to_string);
    }
    let Some(s) = detect::identify(data) else {
        return vec![enhanced];
    };
    let charset = s
        .is_text
        .then(|| text::detect(&data[..data.len().min(65536)]));
    let guessed_ext = s.extensions.first().map(|e| format!(".{e}"));
    let compatible = base.mimetype.as_deref().is_none_or(|m| m == s.mimetype)
        && base
            .extension
            .as_deref()
            .is_none_or(|e| s.extensions.contains(&e.trim_start_matches('.')))
        && base
            .charset
            .as_deref()
            .is_none_or(|c| Some(normalize_charset(c)) == charset);
    let sniffed = StreamInfo {
        mimetype: Some(s.mimetype.to_string()),
        extension: guessed_ext,
        charset,
        url: base.url.clone(),
    };
    if compatible {
        vec![StreamInfo {
            mimetype: base.mimetype.clone().or(sniffed.mimetype),
            extension: base.extension.clone().or(sniffed.extension),
            charset: base.charset.clone().or(sniffed.charset),
            url: base.url.clone(),
        }]
    } else {
        vec![enhanced, sniffed]
    }
}

pub fn convert(data: &[u8], base: &StreamInfo, opts: &Options) -> Result<String, Error> {
    let mut failures = Vec::new();
    let mut candidates = guesses(data, base);
    candidates.push(StreamInfo::default());
    for info in &candidates {
        for conv in formats::CONVERTERS {
            if !(conv.accepts)(data, info) {
                continue;
            }
            match (conv.convert)(data, info, opts) {
                Ok(md) => return Ok(md),
                Err(e) => failures.push(format!("{} threw {e}", conv.name)),
            }
        }
    }
    if failures.is_empty() {
        Err(Error::Unsupported(
            "no converter accepted the input (markitdown-rs does not support this format yet)"
                .into(),
        ))
    } else {
        Err(Error::Conversion(failures.join("; ")))
    }
}
