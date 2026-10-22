//! Standard base64 (with padding), for `keep_data_uris` image output.

pub fn encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `base64.b64decode(s, altchars="-_")` without validation (binascii's
/// non-strict mode): characters outside the alphabet are skipped, padding
/// before the second character of a group is ignored, and padding that
/// completes a group ends the input.
pub fn decode_urlsafe_lenient(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut group: Vec<u8> = Vec::with_capacity(4);
    let mut pads = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => {
                if group.len() >= 2 {
                    pads += 1;
                    if group.len() + pads >= 4 {
                        break;
                    }
                }
                continue;
            }
            _ => continue,
        };
        group.push(v);
        if group.len() == 4 {
            let n = group.iter().fold(0u32, |acc, &x| (acc << 6) | u32::from(x));
            out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
            group.clear();
        }
    }
    match group.len() {
        0 => Some(out),
        1 => None,
        len if len + pads < 4 => None, // incorrect padding
        len => {
            let n = group.iter().fold(0u32, |acc, &x| (acc << 6) | u32::from(x)) << (6 * (4 - len));
            out.push((n >> 16) as u8);
            if len == 3 {
                out.push((n >> 8) as u8);
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lenient_decode_matches_python() {
        assert_eq!(
            super::decode_urlsafe_lenient("aHR0cHM6Ly9leGFtcGxlLmNvbS8=="),
            Some(b"https://example.com/".to_vec())
        );
        assert_eq!(
            super::decode_urlsafe_lenient("Zm9v=="),
            Some(b"foo".to_vec())
        );
        assert_eq!(super::decode_urlsafe_lenient("Zm8=="), Some(b"fo".to_vec()));
        assert_eq!(super::decode_urlsafe_lenient("Zg=="), Some(b"f".to_vec()));
        assert_eq!(super::decode_urlsafe_lenient("Z=="), None);
    }

    #[test]
    fn rfc4648_vectors() {
        let cases = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, want) in cases {
            assert_eq!(super::encode(input.as_bytes()), want);
        }
    }
}
