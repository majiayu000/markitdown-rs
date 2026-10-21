//! Python `str` whitespace semantics, which the reference implementation's
//! output depends on (`str.strip()`, `str.rstrip()`, `str.isspace()`).

/// `str.isspace()` for one character. Rust's `char::is_whitespace` lacks the
/// four information separators U+001C..U+001F that Python counts.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

pub fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// `str.strip(" \t\r\n")`
pub fn strip_ascii_ws(s: &str) -> &str {
    s.trim_matches([' ', '\t', '\r', '\n'])
}

/// markitdown's final normalisation: right-strip every line and squeeze
/// runs of three or more newlines down to two.
pub fn normalize_output(s: &str) -> String {
    let mut lines = String::with_capacity(s.len());
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            lines.push('\n');
        }
        // re.split(r"\r?\n") also eats the '\r' before a '\n'
        let line = line.strip_suffix('\r').unwrap_or(line);
        lines.push_str(rstrip(line));
    }
    let mut out = String::with_capacity(lines.len());
    let mut newlines = 0;
    for c in lines.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push('\n');
            }
        } else {
            newlines = 0;
            out.push(c);
        }
    }
    out
}
