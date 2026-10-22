//! Convert documents to Markdown with the exact output of Python
//! [markitdown](https://github.com/microsoft/markitdown) 0.1.8.
//!
//! ```no_run
//! use markitdown_core::{StreamInfo, convert};
//! let data = std::fs::read("report.docx")?;
//! let md = convert(&data, &StreamInfo::from_path("report.docx"), &Default::default())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod base64;
mod convert;
mod detect;
mod docx;
mod dom;
mod formats;
mod html;
mod markdownify;
mod plain;
mod pptx;
mod pystr;
mod pyurl;
mod rss;
mod sheet;
mod text;
mod xml;
mod zip;

use std::fmt;

/// What is known about the input besides its bytes, as markitdown's
/// `StreamInfo`: the format is decided from these hints and the content.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StreamInfo {
    /// with the leading dot, e.g. ".docx"
    pub extension: Option<String>,
    pub mimetype: Option<String>,
    pub charset: Option<String>,
    /// the source URL; Wikipedia and Bing pages are only recognised by it
    pub url: Option<String>,
}

impl StreamInfo {
    /// Hints from a file name or path (its extension).
    pub fn from_path(path: &str) -> Self {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        // os.path.splitext: leading dots do not start an extension
        let stem_start = name.len() - name.trim_start_matches('.').len();
        let extension = name[stem_start..]
            .rfind('.')
            .map(|i| name[stem_start + i..].to_string());
        StreamInfo {
            extension,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Keep images as full base64 data URIs instead of truncating them
    /// (markitdown's `--keep-data-uris`).
    pub keep_data_uris: bool,
}

#[derive(Debug)]
pub enum Error {
    Zip(String),
    Xml(String),
    /// A part the format requires is missing from the package.
    MissingPart(String),
    /// No converter accepts the input.
    Unsupported(String),
    /// Every converter that accepted the input failed.
    Conversion(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "invalid zip package: {e}"),
            Error::Xml(e) => write!(f, "invalid XML: {e}"),
            Error::MissingPart(p) => write!(f, "missing part: {p}"),
            Error::Unsupported(e) => write!(f, "unsupported input: {e}"),
            Error::Conversion(e) => write!(f, "conversion failed: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Convert a document to Markdown, as `MarkItDown().convert_stream(...)`.
pub fn convert(data: &[u8], info: &StreamInfo, opts: &Options) -> Result<String, Error> {
    let md = convert::convert(data, info, opts)?;
    Ok(pystr::normalize_output(&md))
}
