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

/// Sample 64 KiB, completing a split UTF-8 character with at most three
/// extra bytes. Leave invalid UTF-8 unchanged for legacy charset detection.
fn charset_sample(data: &[u8]) -> &[u8] {
    let sample = &data[..data.len().min(65536)];
    if let Err(error) = std::str::from_utf8(sample)
        && error.error_len().is_none()
    {
        for end in sample.len() + 1..=data.len().min(sample.len() + 3) {
            match std::str::from_utf8(&data[error.valid_up_to()..end]) {
                Ok(_) => return &data[..end],
                Err(error) if error.error_len().is_some() => break,
                Err(_) => {}
            }
        }
    }
    sample
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
    let charset = s.is_text.then(|| text::detect(charset_sample(data)));
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
        ..base.clone()
    };
    if compatible {
        vec![StreamInfo {
            mimetype: base.mimetype.clone().or(sniffed.mimetype),
            extension: base.extension.clone().or(sniffed.extension),
            charset: base.charset.clone().or(sniffed.charset),
            ..base.clone()
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

#[cfg(test)]
mod tests {
    use super::charset_sample;

    const LIMIT: usize = 65536;

    #[test]
    fn charset_sample_completes_each_utf8_split() {
        for ch in ['é', '€', '😀'] {
            for split in 1..ch.len_utf8() {
                let mut input = "a".repeat(LIMIT - split);
                input.push(ch);
                let end = input.len();
                // Do not include another character or inspect the whole input.
                let mut data = input.into_bytes();
                data.extend_from_slice(b"\xfftail");
                let sample = charset_sample(&data);
                assert_eq!(sample, &data[..end], "{ch:?}, split {split}");
                assert!(std::str::from_utf8(sample).is_ok());
                assert!(sample.len() <= LIMIT + 3);
            }
        }
    }

    #[test]
    fn charset_sample_keeps_complete_samples_bounded() {
        for len in [0, LIMIT - 1, LIMIT, LIMIT + 1] {
            let data = vec![b'a'; len];
            assert_eq!(charset_sample(&data), &data[..len.min(LIMIT)]);
        }
        let mut data = "a".repeat(LIMIT - 'é'.len_utf8());
        data.push('é');
        data.push('€');
        assert_eq!(charset_sample(data.as_bytes()), &data.as_bytes()[..LIMIT]);
    }

    #[test]
    fn charset_sample_preserves_invalid_and_truncated_samples() {
        // Invalid interior bytes, invalid completion bytes, and EOF before
        // completion must still reach the existing charset detector unchanged.
        for (tail, prefix) in [
            (&b"\xff\xc3\xa9"[..], LIMIT - 2),
            (&b"\xc3x"[..], LIMIT - 1),
            (&b"\xf0\x80\x80\x80"[..], LIMIT - 1),
            (&b"\xed\xa0\x80"[..], LIMIT - 1),
            (&b"\xe2\x82"[..], LIMIT - 1),
            (&b"\xf0\x9f\x98"[..], LIMIT - 1),
            (&b"\xc3"[..], LIMIT - 1),
        ] {
            let mut data = vec![b'a'; prefix];
            data.extend_from_slice(tail);
            assert_eq!(charset_sample(&data), &data[..data.len().min(LIMIT)]);
        }
    }
}
