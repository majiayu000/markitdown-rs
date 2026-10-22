//! `markitdown` command line, compatible with the Python CLI.

mod fetch;

use markitdown_core::{Options, StreamInfo};
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str =
    "usage: markitdown [-h] [-v] [-o OUTPUT] [-x EXTENSION] [-m MIME_TYPE] [-c CHARSET]
                  [--keep-data-uris] [filename]

Convert various file formats to markdown.

FILENAME can be a path, an http(s):// URL, a file: URI or a data: URI.
If FILENAME is empty, markitdown reads from stdin.

options:
  -h, --help            show this help message and exit
  -v, --version         show the version number and exit
  -o, --output OUTPUT   Output file name. If not provided, output is written to stdout.
  -x, --extension EXTENSION
                        Provide a hint about the file extension (e.g., when reading from stdin).
  -m, --mime-type MIME_TYPE
                        Provide a hint about the file's MIME type.
  -c, --charset CHARSET
                        Provide a hint about the file's charset (accepted for compatibility).
  --keep-data-uris      Keep data URIs (like base64-encoded images) in the output.

Supported formats: see https://github.com/majiayu000/markitdown-rs#compatibility";

struct Args {
    filename: Option<String>,
    output: Option<String>,
    extension: Option<String>,
    mime_type: Option<String>,
    charset: Option<String>,
    keep_data_uris: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        filename: None,
        output: None,
        extension: None,
        mime_type: None,
        charset: None,
        keep_data_uris: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (a.clone(), None),
        };
        let mut value = |name: &str| {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or(format!("argument {name}: expected one argument"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "-v" | "--version" => {
                println!(
                    "markitdown {} (markitdown-rs, compatible with markitdown 0.1.9)",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "-o" | "--output" => args.output = Some(value("-o/--output")?),
            "-x" | "--extension" => args.extension = Some(value("-x/--extension")?),
            "-m" | "--mime-type" => args.mime_type = Some(value("-m/--mime-type")?),
            "-c" | "--charset" => args.charset = Some(value("-c/--charset")?),
            "--keep-data-uris" => args.keep_data_uris = true,
            "-d"
            | "--use-docintel"
            | "--use-cu"
            | "--use-content-understanding"
            | "-p"
            | "--use-plugins"
            | "--list-plugins"
            | "-e"
            | "--endpoint"
            | "--cu-endpoint"
            | "--cu-analyzer"
            | "--cu-file-types" => {
                return Err(format!("{flag} is not supported by markitdown-rs"));
            }
            f if f.starts_with('-') && f != "-" => {
                return Err(format!("unrecognized arguments: {f}"));
            }
            _ if args.filename.is_none() => args.filename = Some(a),
            _ => return Err(format!("unrecognized arguments: {a}")),
        }
    }
    Ok(args)
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    // the source's own stream info (path, URL response or data URI), then
    // the -x/-m/-c hints on top
    let (data, mut info) = match &args.filename {
        Some(f) => match fetch::uri_scheme(f) {
            Some(scheme) => fetch::load_uri(f, scheme)?,
            None => (
                std::fs::read(f).map_err(|e| format!("{f}: {e}"))?,
                StreamInfo::from_path(f),
            ),
        },
        None => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| e.to_string())?;
            (buf, StreamInfo::default())
        }
    };
    if let Some(x) = args
        .extension
        .as_deref()
        .map(|x| x.trim().to_lowercase())
        .filter(|x| !x.is_empty())
    {
        info.extension = Some(if x.starts_with('.') {
            x
        } else {
            format!(".{x}")
        });
    }
    if let Some(m) = args
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        if m.matches('/').count() != 1 {
            return Err(format!("Invalid MIME type: {m}"));
        }
        info.mimetype = Some(m.to_string());
    }
    if let Some(c) = args
        .charset
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        info.charset = Some(c.to_string());
    }
    let opts = Options {
        keep_data_uris: args.keep_data_uris,
    };
    let md = markitdown_core::convert(&data, &info, &opts).map_err(|e| e.to_string())?;
    match &args.output {
        Some(path) => std::fs::write(path, md).map_err(|e| format!("{path}: {e}"))?,
        None => {
            let mut out = std::io::stdout().lock();
            out.write_all(md.as_bytes())
                .and_then(|_| out.write_all(b"\n"))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("markitdown: {e}");
            ExitCode::FAILURE
        }
    }
}
