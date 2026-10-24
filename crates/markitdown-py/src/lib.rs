//! The native half of the `markitdown` Python package: conversion and URI
//! loading from markitdown-core. The Python half (python/markitdown) keeps
//! markitdown's API and turns these results into its exceptions.

use markitdown_core::fetch::{self, SourceError};
use markitdown_core::{Error, Options, StreamInfo};
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};

type Info = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// Convert bytes with the given stream info (mimetype, extension, charset,
/// filename, local_path, url). Returns ("ok", markdown), ("unsupported",
/// message) or ("failed", message).
#[pyfunction]
#[pyo3(signature = (data, info, keep_data_uris=false))]
fn convert(
    py: Python<'_>,
    data: &[u8],
    info: Info,
    keep_data_uris: bool,
) -> (&'static str, String) {
    let (mimetype, extension, charset, filename, local_path, url) = info;
    let info = StreamInfo {
        mimetype,
        extension,
        charset,
        filename,
        local_path,
        url,
    };
    let opts = Options { keep_data_uris };
    match py.detach(|| markitdown_core::convert(data, &info, &opts)) {
        Ok(md) => ("ok", md),
        Err(Error::Unsupported(m)) => ("unsupported", m),
        Err(e) => ("failed", e.to_string()),
    }
}

/// Read an http(s), file: or data: URI: (bytes, stream info dict). Raises
/// ValueError for invalid URIs and OSError for unreadable files; HTTP and
/// connection failures are OSErrors tagged ("http", status, url) or
/// ("request", message, url) for the Python side to re-raise.
#[pyfunction]
fn load_uri<'py>(
    py: Python<'py>,
    uri: &str,
) -> PyResult<(Bound<'py, PyBytes>, Bound<'py, PyDict>)> {
    let (data, info) = py.detach(|| fetch::load_uri(uri)).map_err(|e| match e {
        SourceError::Invalid(m) => PyValueError::new_err(m),
        SourceError::Io { path, error } => match error.raw_os_error() {
            Some(code) => PyOSError::new_err((code, strerror(&error), path)),
            None => PyOSError::new_err(format!("{path}: {error}")),
        },
        SourceError::Http { url, status } => PyOSError::new_err(("http", status, url)),
        SourceError::Request { url, message } => PyOSError::new_err(("request", message, url)),
    })?;
    let d = PyDict::new(py);
    for (k, v) in [
        ("mimetype", info.mimetype),
        ("extension", info.extension),
        ("charset", info.charset),
        ("filename", info.filename),
        ("local_path", info.local_path),
        ("url", info.url),
    ] {
        d.set_item(k, v)?;
    }
    Ok((PyBytes::new(py, &data), d))
}

/// The OS message without Rust's " (os error N)".
fn strerror(e: &std::io::Error) -> String {
    let text = e.to_string();
    match text.rfind(" (os error ") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

#[pymodule]
fn _markitdown_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(convert, m)?)?;
    m.add_function(wrap_pyfunction!(load_uri, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
