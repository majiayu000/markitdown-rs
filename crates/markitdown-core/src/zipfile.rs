//! ZIP archives: every member converted on its own, as markitdown's
//! ZipConverter does (unsupported or failing members are skipped).

use crate::zip::Archive;
use crate::{Error, Options, StreamInfo};
use std::cell::Cell;

/// How deep ZIP archives may nest inside each other (MARKITDOWN_MAX_ZIP_DEPTH).
const DEFAULT_MAX_DEPTH: usize = 10;

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

pub fn convert(data: &[u8], info: &StreamInfo, opts: &Options) -> Result<String, Error> {
    let max = opts.max_zip_depth.unwrap_or_else(|| {
        std::env::var("MARKITDOWN_MAX_ZIP_DEPTH")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_MAX_DEPTH)
    });
    let depth = DEPTH.get();
    if depth >= max {
        return Err(Error::Limit(format!(
            "ZIP archives nested more than {max} deep (raise MARKITDOWN_MAX_ZIP_DEPTH to allow more)"
        )));
    }
    DEPTH.set(depth + 1);
    let result = convert_members(data, info, opts);
    DEPTH.set(depth);
    result
}

fn convert_members(data: &[u8], info: &StreamInfo, opts: &Options) -> Result<String, Error> {
    let path = info
        .url
        .as_deref()
        .or(info.local_path.as_deref())
        .or(info.filename.as_deref())
        .unwrap_or("(unknown)");
    // members' text is kept apart and joined once at the end: a growing
    // String would hold up to twice its size while it reallocates
    let mut parts = vec![format!("Content from the zip file `{path}`:\n\n")];
    let mut zip = Archive::new(data)?;
    for (i, name) in zip.names().into_iter().enumerate() {
        let bytes = zip.read_index(i)?;
        let base = name.rsplit('/').next().unwrap_or(&name).to_string();
        // os.path.splitext on the member name
        let stem_start = base.len() - base.trim_start_matches('.').len();
        let extension = base[stem_start..]
            .rfind('.')
            .map(|j| base[stem_start + j..].to_string())
            .unwrap_or_default();
        let member = StreamInfo {
            extension: Some(extension),
            filename: Some(base),
            ..Default::default()
        };
        match crate::convert::convert(&bytes, &member, opts) {
            Ok(text) => {
                parts.push(format!("## File: {name}\n\n"));
                parts.push(crate::pystr::normalize_output(text.markdown));
                parts.push("\n\n".into());
            }
            Err(Error::Unsupported(_) | Error::Conversion(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let mut md = parts.concat();
    drop(parts);
    // str.strip(), in place
    let kept = crate::pystr::strip(&md);
    let start = kept.as_ptr() as usize - md.as_ptr() as usize;
    let end = start + kept.len();
    md.truncate(end);
    md.drain(..start);
    Ok(md)
}
