//! ZIP archives: every member converted on its own, as markitdown's
//! ZipConverter does (unsupported or failing members are skipped).

use crate::zip::Archive;
use crate::{Error, Options, StreamInfo};

pub fn convert(data: &[u8], info: &StreamInfo, opts: &Options) -> Result<String, Error> {
    let path = info
        .url
        .as_deref()
        .or(info.local_path.as_deref())
        .or(info.filename.as_deref())
        .unwrap_or("(unknown)");
    let mut md = format!("Content from the zip file `{path}`:\n\n");
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
                md.push_str(&format!("## File: {name}\n\n"));
                md.push_str(&crate::pystr::normalize_output(&text));
                md.push_str("\n\n");
            }
            Err(Error::Unsupported(_) | Error::Conversion(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(crate::pystr::strip(&md).to_string())
}
