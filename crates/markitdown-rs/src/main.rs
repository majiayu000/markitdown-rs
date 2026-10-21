//! `markitdown` command line, compatible with the Python CLI.

use markitdown_core::{Format, Options};
use std::io::{Read, Write};
use std::process::ExitCode;

const USAGE: &str =
    "usage: markitdown [-h] [-v] [-o OUTPUT] [-x EXTENSION] [-m MIME_TYPE] [-c CHARSET]
                  [--keep-data-uris] [filename]

Convert various file formats to markdown.

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

Supported formats: .docx, .pptx";

struct Args {
    filename: Option<String>,
    output: Option<String>,
    extension: Option<String>,
    mime_type: Option<String>,
    keep_data_uris: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        filename: None,
        output: None,
        extension: None,
        mime_type: None,
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
                    "markitdown {} (markitdown-rs, compatible with markitdown 0.1.8)",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            "-o" | "--output" => args.output = Some(value("-o/--output")?),
            "-x" | "--extension" => args.extension = Some(value("-x/--extension")?),
            "-m" | "--mime-type" => args.mime_type = Some(value("-m/--mime-type")?),
            "-c" | "--charset" => drop(value("-c/--charset")?),
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
    let data = match &args.filename {
        Some(f) => std::fs::read(f).map_err(|e| format!("{f}: {e}"))?,
        None => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| e.to_string())?;
            buf
        }
    };
    let from_name = args
        .filename
        .as_deref()
        .and_then(|f| f.rsplit_once('.'))
        .and_then(|(_, ext)| Format::from_hint(ext));
    let format = args
        .extension
        .as_deref()
        .and_then(Format::from_hint)
        .or_else(|| args.mime_type.as_deref().and_then(Format::from_hint))
        .or(from_name)
        .or_else(|| Format::sniff(&data))
        .ok_or("unsupported format: markitdown-rs converts .docx and .pptx")?;
    let opts = Options {
        keep_data_uris: args.keep_data_uris,
    };
    let md = markitdown_core::convert(&data, format, &opts).map_err(|e| e.to_string())?;
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
