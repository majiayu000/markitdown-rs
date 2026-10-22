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

/// `str.splitlines()`
pub fn splitlines(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let end = match c {
            '\r' => {
                if let Some(&(_, '\n')) = it.peek() {
                    it.next();
                    i + 2
                } else {
                    i + 1
                }
            }
            '\n' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => i + c.len_utf8(),
            _ => continue,
        };
        out.push(&s[start..i]);
        start = end;
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// `textwrap.dedent()`
pub fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    // whitespace-only lines are emptied and do not count for the margin
    let blank = |l: &str| !l.is_empty() && l.chars().all(|c| c == ' ' || c == '\t');
    let mut margin: Option<&str> = None;
    for l in &lines {
        if l.is_empty() || blank(l) {
            continue;
        }
        let indent = &l[..l.len() - l.trim_start_matches([' ', '\t']).len()];
        margin = Some(match margin {
            None => indent,
            Some(m) if indent.starts_with(m) => m,
            Some(m) if m.starts_with(indent) => indent,
            Some(m) => {
                let common = m
                    .chars()
                    .zip(indent.chars())
                    .take_while(|(a, b)| a == b)
                    .count();
                &m[..common]
            }
        });
    }
    let margin = margin.unwrap_or("");
    lines
        .iter()
        .map(|l| {
            if blank(l) {
                ""
            } else {
                l.strip_prefix(margin).unwrap_or(l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `str(float)` as CPython prints it.
pub fn float_repr(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    let sci = format!("{v:e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |m| ("-", m));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let decpt = exp + 1;
    if (-4 < decpt) && (decpt <= 16) {
        let d = digits.len() as i32;
        let s = if decpt <= 0 {
            format!("0.{}{digits}", "0".repeat((-decpt) as usize))
        } else if decpt >= d {
            format!("{digits}{}.0", "0".repeat((decpt - d) as usize))
        } else {
            format!(
                "{}.{}",
                &digits[..decpt as usize],
                &digits[decpt as usize..]
            )
        };
        format!("{sign}{s}")
    } else {
        let m = if digits.len() > 1 {
            format!("{}.{}", &digits[..1], &digits[1..])
        } else {
            digits
        };
        format!(
            "{sign}{m}e{}{:02}",
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    }
}
