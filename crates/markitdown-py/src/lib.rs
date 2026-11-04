//! The native half of the `markitdown` Python package: conversion and URI
//! loading from markitdown-core. The Python half (python/markitdown) keeps
//! markitdown's API and turns these results into its exceptions.

use markitdown_core::fetch::{self, SourceError};
use markitdown_core::{EmbeddedImage, Error, ImageHook, ImageRequest, Options, StreamInfo};
use pyo3::exceptions::{PyOSError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict};
use std::sync::{Arc, Mutex};

type Info = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn stream_info(info: Info) -> StreamInfo {
    let (mimetype, extension, charset, filename, local_path, url) = info;
    StreamInfo {
        mimetype,
        extension,
        charset,
        filename,
        local_path,
        url,
    }
}

fn info_tuple(info: StreamInfo) -> Info {
    (
        info.mimetype,
        info.extension,
        info.charset,
        info.filename,
        info.local_path,
        info.url,
    )
}

fn outcome(result: Result<String, Error>) -> (&'static str, String) {
    match result {
        Ok(md) => ("ok", md),
        Err(Error::Unsupported(m)) => ("unsupported", m),
        Err(e) => ("failed", e.to_string()),
    }
}

/// Convert bytes with the given stream info (mimetype, extension, charset,
/// filename, local_path, url) through every built-in converter. Returns
/// ("ok", markdown), ("unsupported", message) or ("failed", message).
#[pyfunction]
#[pyo3(signature = (data, info, keep_data_uris=false, exiftool_path=None))]
fn convert(
    py: Python<'_>,
    data: &[u8],
    info: Info,
    keep_data_uris: bool,
    exiftool_path: Option<String>,
) -> (&'static str, String) {
    let info = stream_info(info);
    let opts = Options {
        keep_data_uris,
        exiftool_path,
        ..Default::default()
    };
    outcome(py.detach(|| markitdown_core::convert(data, &info, &opts)))
}

/// The built-in converters' names, in markitdown's trial order.
#[pyfunction]
fn converter_names() -> Vec<&'static str> {
    markitdown_core::converter_names().collect()
}

/// markitdown's stream-info guesses for these bytes and hints.
#[pyfunction]
fn guesses(py: Python<'_>, data: &[u8], info: Info) -> Vec<Info> {
    let info = stream_info(info);
    py.detach(|| markitdown_core::stream_info_guesses(data, &info))
        .into_iter()
        .map(info_tuple)
        .collect()
}

/// Whether the named built-in converter accepts the input.
#[pyfunction]
fn accepts(name: &str, data: &[u8], info: Info) -> PyResult<bool> {
    markitdown_core::converter_accepts(name, data, &stream_info(info))
        .map_err(|e| PyValueError::new_err(e.to_string()))
}

/// `image_hook(request, data, mimetype, extension, filename)` for images in
/// documents, with request "caption" or "html"; the first exception it
/// raises is kept to be re-raised.
fn image_hook(hook: Py<PyAny>, raised: Arc<Mutex<Option<PyErr>>>) -> ImageHook {
    ImageHook(Arc::new(move |request, image: &EmbeddedImage| {
        Python::attach(|py| {
            let request = match request {
                ImageRequest::Caption => "caption",
                ImageRequest::Html => "html",
            };
            let answer = hook
                .call1(
                    py,
                    (
                        request,
                        PyBytes::new(py, image.data),
                        image.info.mimetype.clone(),
                        image.info.extension.clone(),
                        image.info.filename.clone(),
                    ),
                )
                .and_then(|a| a.extract::<Option<String>>(py));
            answer.map_err(|e| {
                let message = e.to_string();
                raised.lock().unwrap().get_or_insert(e);
                message
            })
        })
    }))
}

/// Run one built-in converter: ("ok", markdown) or ("failed", message).
/// An exception from `image_hook` is raised as is.
#[pyfunction]
#[pyo3(signature = (name, data, info, keep_data_uris=false, exiftool_path=None, image_hook=None))]
fn convert_with(
    py: Python<'_>,
    name: &str,
    data: &[u8],
    info: Info,
    keep_data_uris: bool,
    exiftool_path: Option<String>,
    image_hook: Option<Py<PyAny>>,
) -> PyResult<(&'static str, String)> {
    let info = stream_info(info);
    let raised = Arc::new(Mutex::new(None));
    let opts = Options {
        keep_data_uris,
        exiftool_path,
        image_hook: image_hook.map(|h| self::image_hook(h, raised.clone())),
    };
    let result = py.detach(|| markitdown_core::convert_with(name, data, &info, &opts));
    if let Some(e) = raised.lock().unwrap().take() {
        return Err(e);
    }
    Ok(outcome(result))
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
    m.add_function(wrap_pyfunction!(converter_names, m)?)?;
    m.add_function(wrap_pyfunction!(guesses, m)?)?;
    m.add_function(wrap_pyfunction!(accepts, m)?)?;
    m.add_function(wrap_pyfunction!(convert_with, m)?)?;
    m.add_function(wrap_pyfunction!(load_uri, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
