//! Byte-for-byte comparison with Python markitdown 0.1.9.
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

/// The converters that only apply to content fetched from a URL
/// (Wikipedia, Bing, YouTube): tests/url/cases.tsv, snapshots from
/// scripts/gen_url_snapshots.py.
#[test]
fn url_converters_match_python() {
    let cases = std::fs::read_to_string(root().join("tests/url/cases.tsv")).unwrap();
    let mut failures = Vec::new();
    let mut n = 0;
    for line in cases
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
    {
        let f: Vec<&str> = line.split('\t').collect();
        let opt = |i: usize| f.get(i).filter(|s| !s.is_empty()).map(|s| s.to_string());
        let info = StreamInfo {
            url: opt(2),
            mimetype: opt(3),
            charset: opt(4),
            ..Default::default()
        };
        let data = std::fs::read(root().join("tests/fixtures").join(f[1])).unwrap();
        let want = std::fs::read_to_string(root().join(format!("tests/url/{}.md", f[0]))).unwrap();
        let got =
            convert(&data, &info, &Options::default()).unwrap_or_else(|e| format!("ERROR {e}"));
        if got != want {
            failures.push(format!(
                "{}: {}",
                f[0],
                got.chars().take(200).collect::<String>()
            ));
        }
        n += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {n} URL cases differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
