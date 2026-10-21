//! Convert Office documents to Markdown with the exact output of Python
//! [markitdown](https://github.com/microsoft/markitdown) 0.1.8.
//!
//! ```no_run
//! let data = std::fs::read("report.docx")?;
//! let md = markitdown_core::convert(&data, markitdown_core::Format::Docx, &Default::default())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod base64;
mod docx;
mod markdownify;
mod pystr;
mod xml;
mod zip;

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Docx,
}

impl Format {
    /// From a file extension (with or without the leading dot) or MIME type.
    pub fn from_hint(hint: &str) -> Option<Format> {
        let h = hint.trim().trim_start_matches('.').to_ascii_lowercase();
        if h == "docx"
            || h.starts_with(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            )
        {
            Some(Format::Docx)
        } else {
            None
        }
    }

    /// Guess from the package contents.
    pub fn sniff(data: &[u8]) -> Option<Format> {
        let archive = zip::Archive::new(data).ok()?;
        archive
            .contains("word/document.xml")
            .then_some(Format::Docx)
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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "invalid zip package: {e}"),
            Error::Xml(e) => write!(f, "invalid XML: {e}"),
            Error::MissingPart(p) => write!(f, "missing part: {p}"),
        }
    }
}

impl std::error::Error for Error {}

/// Convert a document to Markdown, as `MarkItDown().convert(...).markdown`.
pub fn convert(data: &[u8], format: Format, opts: &Options) -> Result<String, Error> {
    let md = match format {
        Format::Docx => docx::convert(data, opts)?,
    };
    Ok(pystr::normalize_output(&md))
}
