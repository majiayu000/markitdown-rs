//! The converter registry in markitdown's trial order: format-specific
//! converters (most recently registered first), then the generic HTML, ZIP
//! and plain-text ones. Each `accepts` mirrors the Python converter's.

use crate::{Error, Options, StreamInfo, docx, html, plain, pptx, rss};

pub struct Converter {
    pub name: &'static str,
    pub accepts: fn(&[u8], &StreamInfo) -> bool,
    pub convert: fn(&[u8], &StreamInfo, &Options) -> Result<String, Error>,
}

/// extension in `exts` or mimetype starting with one of `mimes`
fn matches(info: &StreamInfo, exts: &[&str], mimes: &[&str]) -> bool {
    let ext = info.extension.as_deref().unwrap_or_default().to_lowercase();
    let mime = info.mimetype.as_deref().unwrap_or_default().to_lowercase();
    exts.contains(&ext.as_str()) || mimes.iter().any(|m| mime.starts_with(m))
}

const HTML_EXTS: &[&str] = &[".html", ".htm"];
const HTML_MIMES: &[&str] = &["text/html", "application/xhtml"];

fn url_matches(info: &StreamInfo, prefix_check: fn(&str) -> bool) -> bool {
    info.url.as_deref().is_some_and(prefix_check)
}

pub static CONVERTERS: &[Converter] = &[
    Converter {
        name: "CsvConverter",
        accepts: |_, i| matches(i, &[".csv"], &["text/csv", "application/csv"]),
        convert: |d, i, _| plain::convert_csv(d, i.charset.as_deref()),
    },
    Converter {
        name: "IpynbConverter",
        accepts: plain::ipynb_accepts,
        convert: |d, i, _| plain::convert_ipynb(d, i.charset.as_deref()),
    },
    Converter {
        name: "PptxConverter",
        accepts: |_, i| {
            matches(
                i,
                &[".pptx"],
                &["application/vnd.openxmlformats-officedocument.presentationml"],
            )
        },
        convert: |d, _, o| pptx::convert(d, o),
    },
    Converter {
        name: "DocxConverter",
        accepts: |_, i| {
            matches(
                i,
                &[".docx"],
                &["application/vnd.openxmlformats-officedocument.wordprocessingml.document"],
            )
        },
        convert: |d, _, o| docx::convert(d, o),
    },
    Converter {
        name: "BingSerpConverter",
        accepts: |_, i| {
            url_matches(i, |u| u.starts_with("https://www.bing.com/search?q="))
                && matches(i, HTML_EXTS, HTML_MIMES)
        },
        convert: |d, i, o| html::convert_bing(d, i, o.keep_data_uris),
    },
    Converter {
        name: "WikipediaConverter",
        accepts: |_, i| url_matches(i, html::is_wikipedia_url) && matches(i, HTML_EXTS, HTML_MIMES),
        convert: |d, i, o| {
            Ok(html::convert_wikipedia(
                d,
                i.charset.as_deref(),
                o.keep_data_uris,
            ))
        },
    },
    Converter {
        name: "RssConverter",
        accepts: rss::accepts,
        convert: |d, i, o| rss::convert(d, i, o.keep_data_uris),
    },
    Converter {
        name: "HtmlConverter",
        accepts: |_, i| matches(i, HTML_EXTS, HTML_MIMES),
        convert: |d, i, o| Ok(html::convert(d, i.charset.as_deref(), o.keep_data_uris)),
    },
    Converter {
        name: "PlainTextConverter",
        accepts: |_, i| {
            i.charset.is_some()
                || matches(
                    i,
                    &[".txt", ".text", ".md", ".markdown", ".json", ".jsonl"],
                    &["text/", "application/json", "application/markdown"],
                )
        },
        convert: |d, i, _| plain::convert_text(d, i.charset.as_deref()),
    },
];
