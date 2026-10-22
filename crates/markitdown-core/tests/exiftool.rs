//! Image and audio metadata against a stub exiftool (tests/exiftool-stub.sh),
//! in its own test binary so EXIFTOOL_PATH stays out of the other tests.

mod common;

#[cfg(unix)]
#[test]
fn media_metadata_matches_python() {
    let stub = common::root().join("tests/exiftool-stub.sh");
    // SAFETY: this test binary runs this single test, before any threads read the environment
    unsafe { std::env::set_var("EXIFTOOL_PATH", stub) };
    common::check_format("media-exiftool");
}
