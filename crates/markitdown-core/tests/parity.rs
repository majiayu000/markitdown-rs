//! Byte-for-byte comparison with Python markitdown 0.1.8.
//!
//! Every file in tests/fixtures/<format>/ has a snapshot in
//! tests/snapshots/<format>/ produced by scripts/gen_snapshots.sh: either
//! `<name>.md` (the Python CLI's stdout) or `<name>.py-error` when Python
//! fails on the file, in which case we only require that we do not panic.
//! Warning lines that xlrd prints to stdout before the markdown are ignored.

mod common;

use common::{check_format, root};
use markitdown_core::{Options, StreamInfo, convert};

macro_rules! parity {
    ($($name:ident => $dir:literal),* $(,)?) => {
        $(#[test]
        fn $name() {
            check_format($dir);
        })*
    };
}

parity! {
    docx_matches_python => "docx",
    pptx_matches_python => "pptx",
    html_matches_python => "html",
    text_matches_python => "text",
    csv_matches_python => "csv",
    ipynb_matches_python => "ipynb",
    rss_matches_python => "rss",
    xlsx_matches_python => "xlsx",
    xls_matches_python => "xls",
    epub_matches_python => "epub",
    zip_matches_python => "zip",
    msg_matches_python => "msg",
    media_matches_python => "media",
    pdf_matches_python => "pdf",
}

/// Every input of html5lib-tests' tokenizer suite through the HTML
/// converter (tests/snapshots/html-cases.json, scripts/gen_html_cases.py).
#[test]
fn html5lib_tokenizer_inputs_match_python() {
    let path = root().join("tests/snapshots/html-cases.json");
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let info = StreamInfo {
        extension: Some(".html".into()),
        ..Default::default()
    };
    let mut failures = Vec::new();
    for case in &cases {
        let input = case["input"].as_str().unwrap();
        let got = convert(input.as_bytes(), &info, &Options::default())
            .unwrap_or_else(|e| format!("ERROR {e}"));
        let want = case["output"].as_str().unwrap_or("<python error>");
        if got != want {
            failures.push(format!(
                "{}: {input:?}\n  python: {want:?}\n  rust:   {got:?}",
                case["source"]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures[..failures.len().min(25)].join("\n")
    );
}
