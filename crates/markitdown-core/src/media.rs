//! Images and audio, as markitdown's ImageConverter and AudioConverter:
//! metadata from `exiftool` when one is installed. LLM captions and speech
//! transcription (a call to Google's web API) are not supported.

use crate::{Error, pystr};
use std::io::Write;
use std::process::{Command, Stdio};

const IMAGE_FIELDS: &[&str] = &[
    "ImageSize",
    "Title",
    "Caption",
    "Description",
    "Keywords",
    "Artist",
    "Author",
    "DateTimeOriginal",
    "CreateDate",
    "GPSPosition",
];
const AUDIO_FIELDS: &[&str] = &[
    "Title",
    "Artist",
    "Author",
    "Band",
    "Album",
    "Genre",
    "Track",
    "DateTimeOriginal",
    "CreateDate",
    "NumChannels",
    "SampleRate",
    "AvgBytesPerSec",
    "BitsPerSample",
];

/// markitdown's lookup: EXIFTOOL_PATH, else `exiftool` on PATH when it lives
/// in a well-known directory.
fn exiftool_path() -> Option<String> {
    if let Ok(p) = std::env::var("EXIFTOOL_PATH") {
        return Some(p);
    }
    let path = std::env::var_os("PATH")?;
    let names: &[&str] = if cfg!(windows) {
        &["exiftool.exe", "exiftool"]
    } else {
        &["exiftool"]
    };
    for dir in std::env::split_paths(&path) {
        for n in names {
            let candidate = dir.join(n);
            if candidate.is_file() {
                let known = [
                    "/usr/bin",
                    "/usr/local/bin",
                    "/opt",
                    "/opt/bin",
                    "/opt/local/bin",
                    "/opt/homebrew/bin",
                    "C:\\Windows\\System32",
                    "C:\\Program Files",
                    "C:\\Program Files (x86)",
                ];
                let abs = std::fs::canonicalize(&dir).unwrap_or(dir.clone());
                return known
                    .iter()
                    .any(|k| abs == std::path::Path::new(k))
                    .then(|| candidate.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn metadata(data: &[u8]) -> Result<Option<serde_json::Map<String, serde_json::Value>>, Error> {
    let Some(tool) = exiftool_path() else {
        return Ok(None);
    };
    let fail = |e: String| Error::Conversion(format!("Failed to invoke exiftool at {tool}: {e}"));
    let ver = Command::new(&tool)
        .arg("-ver")
        .output()
        .map_err(|e| fail(e.to_string()))?;
    let version: Vec<u64> = pystr::strip(&String::from_utf8_lossy(&ver.stdout))
        .split('.')
        .map(|p| {
            p.parse()
                .map_err(|_| Error::Conversion("Failed to verify ExifTool version.".into()))
        })
        .collect::<Result<_, _>>()?;
    if !ver.status.success() {
        return Err(Error::Conversion(
            "Failed to verify ExifTool version.".into(),
        ));
    }
    if version < vec![12, 24] {
        return Err(Error::Conversion(
            "ExifTool is vulnerable to CVE-2021-22204; upgrade to 12.24 or later".into(),
        ));
    }
    let mut child = Command::new(&tool)
        .args(["-json", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(e.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        // exiftool may stop reading early; a broken pipe is not an error
        let _ = stdin.write_all(data);
    }
    let out = child.wait_with_output().map_err(|e| fail(e.to_string()))?;
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| Error::Conversion(format!("exiftool output: {e}")))?;
    match json.get(0) {
        Some(serde_json::Value::Object(m)) => Ok(Some(m.clone())),
        _ => Err(Error::Conversion(
            "exiftool output: expected a list of objects".into(),
        )),
    }
}

/// `repr()` of a JSON value as Python would print the decoded object.
fn py_repr(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => {
            let quote = if s.contains('\'') && !s.contains('"') {
                '"'
            } else {
                '\''
            };
            let mut out = String::from(quote);
            for c in s.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c == quote => {
                        out.push('\\');
                        out.push(c);
                    }
                    c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                        out.push_str(&format!("\\x{:02x}", c as u32))
                    }
                    c => out.push(c),
                }
            }
            out.push(quote);
            out
        }
        other => py_str(other),
    }
}

/// `str()` of a JSON value as Python would print the decoded object.
fn py_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "None".into(),
        serde_json::Value::Bool(b) => {
            if *b {
                "True".into()
            } else {
                "False".into()
            }
        }
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => n.as_u64().map_or_else(
                || pystr::float_repr(n.as_f64().unwrap_or(f64::NAN)),
                |u| u.to_string(),
            ),
        },
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => {
            format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", "))
        }
        serde_json::Value::Object(o) => {
            format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| format!(
                        "{}: {}",
                        py_repr(&serde_json::Value::String(k.clone())),
                        py_repr(v)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }
}

fn fields(data: &[u8], wanted: &[&str]) -> Result<String, Error> {
    let mut md = String::new();
    if let Some(meta) = metadata(data)? {
        for f in wanted {
            if let Some(v) = meta.get(*f) {
                md.push_str(&format!("{f}: {}\n", py_str(v)));
            }
        }
    }
    Ok(md)
}

pub fn convert_image(data: &[u8]) -> Result<String, Error> {
    fields(data, IMAGE_FIELDS)
}

pub fn convert_audio(data: &[u8]) -> Result<String, Error> {
    Ok(pystr::strip(&fields(data, AUDIO_FIELDS)?).to_string())
}
