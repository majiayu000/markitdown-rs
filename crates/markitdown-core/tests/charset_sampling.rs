use markitdown_core::{Options, StreamInfo, convert};

#[test]
fn text_conversion_preserves_utf8_split_at_charset_sample_boundary() {
    for ch in ['é', '€', '😀'] {
        for split in 1..ch.len_utf8() {
            let mut input = "a".repeat(65536 - split);
            input.push(ch);
            input.push_str(" after the sample");
            let output = convert(
                input.as_bytes(),
                &StreamInfo::from_path("boundary.txt"),
                &Options::default(),
            )
            .unwrap();
            assert_eq!(output, input, "{ch:?}, split {split}");
        }
    }
}

#[test]
fn text_conversion_preserves_explicit_legacy_charset() {
    let mut data = vec![b'a'; 65535];
    data.extend_from_slice(b"\xe9 after the sample");
    let info = StreamInfo {
        charset: Some("cp1252".into()),
        ..StreamInfo::from_path("legacy.txt")
    };
    let output = convert(&data, &info, &Options::default()).unwrap();
    assert_eq!(output, format!("{}é after the sample", "a".repeat(65535)));
}
