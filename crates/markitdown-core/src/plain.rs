//! Plain text, CSV and Jupyter notebooks, as markitdown's PlainTextConverter,
//! CsvConverter and IpynbConverter.

use crate::{Error, StreamInfo, text};

/// Decode with the stream charset (strict), else detect it.
fn decode(data: &[u8], charset: Option<&str>) -> Result<String, Error> {
    match charset {
        Some(c) => text::decode(data, c)
            .ok_or_else(|| Error::Conversion(format!("cannot decode input as {c}"))),
        None => Ok(text::decode_detected(data)),
    }
}

pub fn convert_text(data: &[u8], charset: Option<&str>) -> Result<String, Error> {
    decode(data, charset)
}

pub fn convert_csv(data: &[u8], charset: Option<&str>) -> Result<String, Error> {
    let content = decode(data, charset)?;
    let mut rows = csv_rows(content.trim_start_matches('\u{feff}'));
    trim_outer_blank_rows(&mut rows);
    if rows.is_empty() {
        return Ok(String::new());
    }
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut rows {
        row.resize(ncols, String::new());
    }
    let line = |row: &[String]| {
        format!(
            "| {} |",
            row.iter()
                .map(|c| escape_cell(c))
                .collect::<Vec<_>>()
                .join(" | ")
        )
    };
    let mut out = vec![
        line(&rows[0]),
        format!("| {} |", vec!["---"; ncols].join(" | ")),
    ];
    out.extend(rows[1..].iter().map(|r| line(r)));
    Ok(out.join("\n"))
}

/// `_escape_table_cell`: escape pipes (doubling the backslashes in front of
/// them), collapse line breaks to spaces.
fn escape_cell(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let chars: Vec<char> = v.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' {
            let start = i;
            while i < chars.len() && chars[i] == '\\' {
                i += 1;
            }
            let run = i - start;
            if chars.get(i) == Some(&'|') {
                // (?<!\\)(\\*)\| matches the whole backslash run
                out.extend(std::iter::repeat_n('\\', run * 2));
                out.push_str("\\|");
                i += 1;
            } else {
                out.extend(std::iter::repeat_n('\\', run));
            }
            continue;
        }
        if chars[i] == '|' {
            out.push_str("\\|");
        } else {
            out.push(chars[i]);
        }
        i += 1;
    }
    out.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

fn trim_outer_blank_rows(rows: &mut Vec<Vec<String>>) {
    let Some(header) = rows.iter().position(|r| !r.is_empty()) else {
        rows.clear();
        return;
    };
    let mut start = header + 1;
    while start < rows.len() && rows[start].is_empty() {
        start += 1;
    }
    let mut end = rows.len();
    while end > start && rows[end - 1].is_empty() {
        end -= 1;
    }
    rows.truncate(end);
    rows.drain(header + 1..start);
    rows.drain(..header);
}

/// `list(csv.reader(io.StringIO(content, newline="")))`, excel dialect.
fn csv_rows(content: &str) -> Vec<Vec<String>> {
    #[derive(PartialEq, Clone, Copy)]
    enum S {
        StartRecord,
        StartField,
        InField,
        InQuoted,
        QuoteInQuoted,
        EatCrnl,
    }
    let mut rows = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut field_len = 0usize;
    let mut state = S::StartRecord;
    // None stands for the end-of-line sentinel fed after each line
    let process = |c: Option<char>,
                   state: &mut S,
                   fields: &mut Vec<String>,
                   field: &mut String,
                   field_len: &mut usize| {
        let nl = matches!(c, Some('\n' | '\r'));
        let save = |fields: &mut Vec<String>, field: &mut String, field_len: &mut usize| {
            fields.push(std::mem::take(field));
            *field_len = 0;
        };
        if *state == S::StartRecord {
            if c.is_none() {
                return;
            } else if nl {
                *state = S::EatCrnl;
                return;
            }
            *state = S::StartField;
        }
        match *state {
            S::StartField => {
                if nl || c.is_none() {
                    save(fields, field, field_len);
                    *state = if c.is_none() {
                        S::StartRecord
                    } else {
                        S::EatCrnl
                    };
                } else if c == Some('"') {
                    *state = S::InQuoted;
                } else if c == Some(',') {
                    save(fields, field, field_len);
                } else {
                    field.push(c.unwrap_or_default());
                    *field_len += 1;
                    *state = S::InField;
                }
            }
            S::InField => {
                if nl || c.is_none() {
                    save(fields, field, field_len);
                    *state = if c.is_none() {
                        S::StartRecord
                    } else {
                        S::EatCrnl
                    };
                } else if c == Some(',') {
                    save(fields, field, field_len);
                    *state = S::StartField;
                } else {
                    field.push(c.unwrap_or_default());
                    *field_len += 1;
                }
            }
            S::InQuoted => match c {
                None => {}
                Some('"') => *state = S::QuoteInQuoted,
                Some(ch) => {
                    field.push(ch);
                    *field_len += 1;
                }
            },
            S::QuoteInQuoted => {
                if c == Some('"') {
                    field.push('"');
                    *field_len += 1;
                    *state = S::InQuoted;
                } else if c == Some(',') {
                    save(fields, field, field_len);
                    *state = S::StartField;
                } else if nl || c.is_none() {
                    save(fields, field, field_len);
                    *state = if c.is_none() {
                        S::StartRecord
                    } else {
                        S::EatCrnl
                    };
                } else {
                    field.push(c.unwrap_or_default());
                    *field_len += 1;
                    *state = S::InField;
                }
            }
            S::EatCrnl => {
                if c.is_none() {
                    *state = S::StartRecord;
                }
                // a character after a line break inside one line cannot
                // happen with newline="" line splitting
            }
            S::StartRecord => unreachable!(),
        }
    };
    for line in split_lines_keepends(content) {
        for c in line.chars() {
            process(Some(c), &mut state, &mut fields, &mut field, &mut field_len);
        }
        process(None, &mut state, &mut fields, &mut field, &mut field_len);
        if state == S::StartRecord {
            rows.push(std::mem::take(&mut fields));
        }
    }
    // end of input inside a record (quoted field or partial line)
    if field_len != 0 || state == S::InQuoted {
        fields.push(field);
        rows.push(fields);
    }
    rows
}

/// `io.StringIO(s, newline="")` iteration: lines end at \n, \r or \r\n.
fn split_lines_keepends(s: &str) -> Vec<&str> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\n' => {
                out.push(&s[start..=i]);
                start = i + 1;
            }
            b'\r' => {
                let end = if b.get(i + 1) == Some(&b'\n') {
                    i + 2
                } else {
                    i + 1
                };
                out.push(&s[start..end]);
                start = end;
                i = end;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    if start < b.len() {
        out.push(&s[start..]);
    }
    out
}

pub fn ipynb_accepts(data: &[u8], info: &StreamInfo) -> bool {
    let ext = info.extension.as_deref().unwrap_or_default().to_lowercase();
    if ext == ".ipynb" {
        return true;
    }
    if info
        .mimetype
        .as_deref()
        .unwrap_or_default()
        .to_lowercase()
        .starts_with("application/json")
    {
        return text::decode(data, info.charset.as_deref().unwrap_or("utf-8"))
            .is_some_and(|s| s.contains("nbformat") && s.contains("nbformat_minor"));
    }
    false
}

pub fn convert_ipynb(data: &[u8], charset: Option<&str>) -> Result<String, Error> {
    let enc = charset.unwrap_or("utf-8");
    let s = text::decode(data, enc)
        .ok_or_else(|| Error::Conversion(format!("cannot decode notebook as {enc}")))?;
    let nb: serde_json::Value = serde_json::from_str(s.trim_start_matches('\u{feff}'))
        .map_err(|e| Error::Conversion(format!("invalid notebook JSON: {e}")))?;
    let bad = || Error::Conversion("Error converting .ipynb file".into());
    let mut parts = Vec::new();
    let cells = match nb.as_object().ok_or_else(bad)?.get("cells") {
        None => return Ok(String::new()),
        Some(serde_json::Value::Array(c)) => c,
        Some(_) => return Err(bad()),
    };
    for cell in cells {
        let cell = cell.as_object().ok_or_else(bad)?;
        let cell_type = cell
            .get("cell_type")
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        let source = match cell.get("source") {
            None => String::new(),
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(lines)) => lines
                .iter()
                .map(|l| l.as_str().map(str::to_string).ok_or_else(bad))
                .collect::<Result<String, _>>()?,
            Some(_) => return Err(bad()),
        };
        match cell_type {
            "markdown" => parts.push(source),
            "code" => parts.push(format!("```python\n{source}\n```")),
            "raw" => parts.push(format!("```\n{source}\n```")),
            _ => {}
        }
    }
    Ok(parts.join("\n\n"))
}
