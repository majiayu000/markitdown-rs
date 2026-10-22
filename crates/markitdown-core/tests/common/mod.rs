//! Shared parity harness: fixtures, snapshots and known differences.
#![allow(dead_code)]

use markitdown_core::{Options, StreamInfo, convert};
use std::path::{Path, PathBuf};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn first_diff(want: &str, got: &str) -> String {
    for (i, (w, g)) in want.lines().zip(got.lines()).enumerate() {
        if w != g {
            return format!("line {}:\n  python: {w:?}\n  rust:   {g:?}", i + 1);
        }
    }
    format!(
        "line counts differ: python {} vs rust {}",
        want.lines().count(),
        got.lines().count()
    )
}

/// `<format>/<fixture>` names listed in tests/known-diffs.txt
fn known_diffs() -> Vec<String> {
    let text = std::fs::read_to_string(root().join("tests/known-diffs.txt")).unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(':').map(|(name, _)| name.trim().to_string()))
        .collect()
}

pub fn check_format(dir: &str) {
    let known = known_diffs();
    let fixtures = root().join("tests/fixtures").join(dir);
    let snapshots = root().join("tests/snapshots").join(dir);
    let mut files: Vec<_> = std::fs::read_dir(&fixtures)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no fixtures in {}", fixtures.display());
    let mut failures = Vec::new();
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let data = std::fs::read(f).unwrap();
        // the path as gen_snapshots.sh passes it (the ZIP converter prints it)
        let got = convert(
            &data,
            &StreamInfo::from_path(&format!("tests/fixtures/{dir}/{name}")),
            &Options::default(),
        );
        let md_snapshot = snapshots.join(format!("{name}.md"));
        let err_snapshot = snapshots.join(format!("{name}.py-error"));
        let is_known = known.contains(&format!("{dir}/{name}"));
        if let Ok(want) = std::fs::read_to_string(&md_snapshot) {
            // xlrd logs container warnings to stdout ahead of the markdown
            let want: String = want
                .split_inclusive('\n')
                .filter(|l| !l.starts_with("WARNING *** "))
                .collect();
            match got.map(|md| md + "\n") {
                Ok(md) if md == want && is_known => failures.push(format!(
                    "{name}: matches now, remove it from tests/known-diffs.txt"
                )),
                Ok(md) if md == want => {}
                Ok(_) if is_known => eprintln!("known difference: {dir}/{name}"),
                Ok(md) => failures.push(format!("{name}: {}", first_diff(&want, &md))),
                Err(e) => failures.push(format!("{name}: conversion failed: {e}")),
            }
        } else if err_snapshot.exists() {
            // Python fails on this file; we only require a clean result
            if let Err(e) = got {
                eprintln!("{dir}/{name}: python fails, we return an error too: {e}");
            }
        } else {
            failures.push(format!("{name}: no snapshot, run scripts/gen_snapshots.sh"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} {dir} files differ:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}
