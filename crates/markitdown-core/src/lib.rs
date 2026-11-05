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
mod epub;
#[cfg(feature = "fetch")]
pub mod fetch;
mod formats;
mod html;
mod markdownify;
mod media;
mod msg;
mod ole;
mod pdf;
mod plain;
mod pptx;
mod pystr;
mod pyurl;
mod rss;
mod sheet;
mod text;
mod xml;
mod youtube;
mod zip;
mod zipfile;

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
    /// the file name, e.g. of a ZIP member
    pub filename: Option<String>,
    /// the path as given on the command line (shown by the ZIP converter)
    pub local_path: Option<String>,
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
            filename: Some(name.to_string()),
            local_path: Some(path.to_string()),
            ..Default::default()
        }
    }
}

/// A converted document: its Markdown and, for formats that have one, the
/// title markitdown reports (`DocumentConverterResult.title`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub markdown: String,
    pub title: Option<String>,
}

impl From<String> for Document {
    fn from(markdown: String) -> Self {
        Document {
            markdown,
            title: None,
        }
    }
}

/// What a converter asks `Options::image_hook` about an embedded image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageRequest {
    /// A description for a slide picture's alt text (markitdown's LLM
    /// caption); PowerPoint only.
    Caption,
    /// An HTML fragment to render in place of the image (markitdown's
    /// `_image_to_html` hook of the DOCX, PPTX and XLSX converters).
    Html,
}

/// An image inside a document: its bytes, content type, extension and name.
pub struct EmbeddedImage<'a> {
    pub data: &'a [u8],
    pub info: StreamInfo,
}

type ImageHookFn =
    dyn Fn(ImageRequest, &EmbeddedImage) -> Result<Option<String>, String> + Send + Sync;

/// A callback for images embedded in documents. `Ok(None)` (or blank text)
/// keeps the image as markitdown renders it; an error fails the conversion.
#[derive(Clone)]
pub struct ImageHook(pub std::sync::Arc<ImageHookFn>);

impl fmt::Debug for ImageHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImageHook")
    }
}

impl ImageHook {
    fn ask(&self, request: ImageRequest, image: &EmbeddedImage) -> Result<Option<String>, Error> {
        let answer = (self.0)(request, image).map_err(Error::Conversion)?;
        Ok(answer.filter(|a| !pystr::strip(a).is_empty()))
    }
}

/// Fetches a YouTube video's transcript by video id (markitdown's
/// youtube-transcript-api step); `None` when there is none.
#[derive(Clone)]
pub struct TranscriptHook(pub std::sync::Arc<TranscriptHookFn>);

type TranscriptHookFn = dyn Fn(&str) -> Option<String> + Send + Sync;

impl fmt::Debug for TranscriptHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TranscriptHook")
    }
}

#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Keep images as full base64 data URIs instead of truncating them
    /// (markitdown's `--keep-data-uris`).
    pub keep_data_uris: bool,
    /// The exiftool to read image and audio metadata with; `None` looks it
    /// up as markitdown does, an empty path disables it.
    pub exiftool_path: Option<String>,
    pub image_hook: Option<ImageHook>,
    /// Adds a `### Transcript` section to YouTube pages.
    pub youtube_transcript: Option<TranscriptHook>,
    /// Decompressed bytes a conversion may read from archives and Office
    /// packages; `None`: MARKITDOWN_MAX_UNZIPPED_BYTES, else 1 GiB.
    pub max_unzipped_bytes: Option<u64>,
    /// How deep ZIP archives may nest; `None`: MARKITDOWN_MAX_ZIP_DEPTH,
    /// else 10.
    pub max_zip_depth: Option<usize>,
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
    /// The input exceeds a safety limit (decompressed size, nesting).
    Limit(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Zip(e) => write!(f, "invalid zip package: {e}"),
            Error::Xml(e) => write!(f, "invalid XML: {e}"),
            Error::MissingPart(p) => write!(f, "missing part: {p}"),
            Error::Unsupported(e) => write!(f, "unsupported input: {e}"),
            Error::Conversion(e) => write!(f, "conversion failed: {e}"),
            Error::Limit(e) => write!(f, "input too large: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Convert a document to Markdown, as `MarkItDown().convert_stream(...)`.
pub fn convert(data: &[u8], info: &StreamInfo, opts: &Options) -> Result<String, Error> {
    Ok(convert_document(data, info, opts)?.markdown)
}

/// `convert`, with the document's title.
pub fn convert_document(data: &[u8], info: &StreamInfo, opts: &Options) -> Result<Document, Error> {
    let doc = zip::with_budget(opts, || convert::convert(data, info, opts))?;
    Ok(Document {
        markdown: pystr::normalize_output(&doc.markdown),
        title: doc.title,
    })
}

/// The built-in converters by name, in markitdown's trial order.
pub fn converter_names() -> impl Iterator<Item = &'static str> {
    formats::CONVERTERS.iter().map(|c| c.name)
}

/// The stream-info guesses markitdown tries in turn: the hints, then what
/// the content sniff says (when it disagrees with them).
pub fn stream_info_guesses(data: &[u8], base: &StreamInfo) -> Vec<StreamInfo> {
    convert::guesses(data, base)
}

fn converter(name: &str) -> Result<&'static formats::Converter, Error> {
    formats::CONVERTERS
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| Error::Unsupported(format!("no converter named {name}")))
}

/// One built-in converter's `accepts`.
pub fn converter_accepts(name: &str, data: &[u8], info: &StreamInfo) -> Result<bool, Error> {
    Ok((converter(name)?.accepts)(data, info))
}

/// One built-in converter's `convert`, without markitdown's final
/// whitespace normalization.
pub fn convert_with(
    name: &str,
    data: &[u8],
    info: &StreamInfo,
    opts: &Options,
) -> Result<Document, Error> {
    let convert = converter(name)?.convert;
    zip::with_budget(opts, || {
        convert::guarded(name, || convert(data, info, opts))
    })
}
