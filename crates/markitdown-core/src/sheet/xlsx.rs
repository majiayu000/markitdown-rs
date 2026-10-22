//! XLSX cells the way openpyxl (read-only, data_only) and pandas'
//! `_convert_cell` produce them.

use super::{Cell, DateTime, Time, epoch_days};
use crate::xml::{self, Element, Node, ParseOptions};
use crate::zip::Archive;
use crate::{Error, pystr};

const NAMESPACES: &[(&str, &str)] = &[
    (
        "s",
        "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    ),
    ("s", "http://purl.oclc.org/ooxml/spreadsheetml/main"),
    (
        "r",
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    ),
    (
        "r",
        "http://purl.oclc.org/ooxml/officeDocument/relationships",
    ),
    (
        "rel",
        "http://schemas.openxmlformats.org/package/2006/relationships",
    ),
];

/// openpyxl's BUILTIN_FORMATS (ids without an entry have no format code).
fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        0 => "General",
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        5 => "\"$\"#,##0_);(\"$\"#,##0)",
        6 => "\"$\"#,##0_);[Red](\"$\"#,##0)",
        7 => "\"$\"#,##0.00_);(\"$\"#,##0.00)",
        8 => "\"$\"#,##0.00_);[Red](\"$\"#,##0.00)",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0_);(#,##0)",
        38 => "#,##0_);[Red](#,##0)",
        39 => "#,##0.00_);(#,##0.00)",
        40 => "#,##0.00_);[Red](#,##0.00)",
        41 => r#"_(* #,##0_);_(* \(#,##0\);_(* "-"_);_(@_)"#,
        42 => r#"_("$"* #,##0_);_("$"* \(#,##0\);_("$"* "-"_);_(@_)"#,
        43 => r#"_(* #,##0.00_);_(* \(#,##0.00\);_(* "-"??_);_(@_)"#,
        44 => r#"_("$"* #,##0.00_)_("$"* \(#,##0.00\)_("$"* "-"??_)_(@_)"#,
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// openpyxl's `is_date_format`: after dropping quoted literals and
/// bracketed locale/colour parts, a d/m/h/y/s not escaped by `_` or `\`.
fn is_date_format(fmt: &str) -> bool {
    let first = fmt.split(';').next().unwrap_or_default();
    let mut stripped = String::new();
    let chars: Vec<char> = first.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '"' => match chars[i + 1..].iter().position(|&c| c == '"') {
                Some(p) => i += p + 2,
                None => {
                    stripped.push('"');
                    i += 1;
                }
            },
            '[' => {
                let rest: String = chars[i + 1..].iter().collect();
                let keep = ["h]", "hh]", "m]", "mm]", "s]", "ss]"]
                    .iter()
                    .any(|k| rest.starts_with(k));
                match (keep, chars[i + 1..].iter().position(|&c| c == ']')) {
                    (false, Some(p)) => i += p + 2,
                    _ => {
                        stripped.push('[');
                        i += 1;
                    }
                }
            }
            c => {
                stripped.push(c);
                i += 1;
            }
        }
    }
    let s: Vec<char> = stripped.chars().collect();
    s.iter()
        .enumerate()
        .any(|(i, c)| "dmhysDMHYS".contains(*c) && (i == 0 || !matches!(s[i - 1], '_' | '\\')))
}

/// openpyxl's TIMEDELTA_RE: `[h]`, `[mm]:ss` and similar.
fn is_timedelta_format(fmt: &str) -> bool {
    let first = fmt.split(';').next().unwrap_or_default().to_lowercase();
    ["[h]", "[hh]", "[m]", "[mm]", "[s]", "[ss]"]
        .iter()
        .any(|p| first.contains(p))
}

fn parse(bytes: &[u8]) -> Result<Element, Error> {
    let opts = ParseOptions {
        namespaces: NAMESPACES,
        cdata_as_text: true,
        normalize_after_unescape: false,
        raw_names: false,
    };
    xml::parse(bytes, &opts)
}

/// lxml `.text` of a child, or the empty string.
fn text_of(e: Option<&Element>) -> String {
    e.map(|t| {
        t.children
            .iter()
            .map_while(|n| {
                if let Node::Text(s) = n {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

/// `Text.content`: the plain `<t>` plus every run's `<t>`.
fn text_content(si: &Element) -> String {
    let mut out = text_of(si.child("s:t"));
    for r in si.children_named("s:r") {
        out.push_str(&text_of(r.child("s:t")));
    }
    out
}

fn rels(zip: &mut Archive, part: &str) -> Result<Vec<(String, String, String)>, Error> {
    let (dir, base) = part.rsplit_once('/').unwrap_or(("", part));
    let path = if dir.is_empty() {
        format!("_rels/{base}.rels")
    } else {
        format!("{dir}/_rels/{base}.rels")
    };
    let Some(bytes) = zip.read(&path)? else {
        return Ok(Vec::new());
    };
    let root = parse(&bytes)?;
    Ok(root
        .children_named("rel:Relationship")
        .filter(|r| r.attr("TargetMode") != Some("External"))
        .filter_map(|r| {
            let target = r.attr("Target")?;
            let resolved = if let Some(abs) = target.strip_prefix('/') {
                abs.to_string()
            } else if dir.is_empty() {
                target.to_string()
            } else {
                normalize(&format!("{dir}/{target}"))
            };
            Some((
                r.attr("Id")?.to_string(),
                r.attr("Type")?.to_string(),
                resolved,
            ))
        })
        .collect())
}

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

struct Styles {
    date: Vec<bool>,
    timedelta: Vec<bool>,
}

fn read_styles(zip: &mut Archive, path: Option<&str>) -> Result<Styles, Error> {
    let mut styles = Styles {
        date: Vec::new(),
        timedelta: Vec::new(),
    };
    let Some(bytes) = (match path {
        Some(p) => zip.read(p)?,
        None => None,
    }) else {
        return Ok(styles);
    };
    let root = parse(&bytes)?;
    let custom: Vec<(u32, String)> = root
        .child("s:numFmts")
        .map(|n| {
            n.children_named("s:numFmt")
                .filter_map(|f| {
                    Some((
                        f.attr("numFmtId")?.parse().ok()?,
                        f.attr("formatCode")?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    for xf in root
        .child("s:cellXfs")
        .into_iter()
        .flat_map(|x| x.children_named("s:xf"))
    {
        let id: u32 = xf
            .attr("numFmtId")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let fmt = custom
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, f)| f.as_str())
            .or_else(|| builtin_format(id));
        styles.date.push(fmt.is_some_and(is_date_format));
        styles.timedelta.push(fmt.is_some_and(is_timedelta_format));
    }
    Ok(styles)
}

/// `openpyxl.utils.datetime.from_excel`
fn from_excel(value: f64, date1904: bool, timedelta: bool) -> Option<Cell> {
    if timedelta {
        let us = (value * 86_400_000_000.0).round() as i64;
        // round the sub-second part to milliseconds
        let (secs, micros) = (us.div_euclid(1_000_000), us.rem_euclid(1_000_000));
        let ms = ((micros as f64) / 1000.0).round_ties_even() as i64 * 1000;
        return Some(Cell::Duration(secs * 1_000_000 + ms));
    }
    let day = value.div_euclid(1.0);
    let fraction = value - day;
    let ms = (fraction * 86_400_000.0).round_ties_even() as i64;
    if (0.0..1.0).contains(&value) && ms < 86_400_000 {
        return Some(Cell::Time(Time::from_micros(ms * 1000)));
    }
    let mut day = day as i64;
    if value > 0.0 && value < 60.0 && !date1904 {
        day += 1;
    }
    DateTime::from_epoch_micros(epoch_days(date1904) + day, ms * 1000).map(Cell::DateTime)
}

/// `openpyxl.utils.datetime.from_ISO8601` for the common forms.
fn from_iso8601(s: &str) -> Option<Cell> {
    let s = s.trim();
    let (date, time) = match s.split_once(['T', ' ']) {
        Some((d, t)) => (Some(d), Some(t)),
        None if s.contains(':') => (None, Some(s)),
        None => (Some(s), None),
    };
    let parse_time = |t: &str| -> Option<Time> {
        let t = t.trim_end_matches('Z');
        let mut it = t.split(':');
        let hour = it.next()?.parse().ok()?;
        let minute = it.next().unwrap_or("0").parse().ok()?;
        let sec = it.next().unwrap_or("0");
        let (s, frac) = sec.split_once('.').unwrap_or((sec, ""));
        let micro = format!("{frac:0<6}").get(..6)?.parse().ok()?;
        Some(Time {
            hour,
            minute,
            second: s.parse().ok()?,
            micro,
        })
    };
    match (date, time) {
        (Some(d), t) => {
            let mut it = d.split('-');
            let (y, m, dd) = (
                it.next()?.parse().ok()?,
                it.next()?.parse().ok()?,
                it.next()?.parse().ok()?,
            );
            let time = match t {
                Some(t) => parse_time(t)?,
                None => Time {
                    hour: 0,
                    minute: 0,
                    second: 0,
                    micro: 0,
                },
            };
            Some(Cell::DateTime(DateTime {
                year: y,
                month: m,
                day: dd,
                time,
            }))
        }
        (None, Some(t)) => parse_time(t).map(Cell::Time),
        (None, None) => None,
    }
}

/// Column number (1-based) of an A1 reference.
fn column_of(r: &str) -> Option<usize> {
    let letters: String = r.chars().take_while(char::is_ascii_alphabetic).collect();
    if letters.is_empty() {
        return None;
    }
    Some(
        letters
            .to_ascii_uppercase()
            .bytes()
            .fold(0, |acc, b| acc * 26 + (b - b'A' + 1) as usize),
    )
}

fn row_of(r: &str) -> Option<usize> {
    r.trim_start_matches(|c: char| c.is_ascii_alphabetic())
        .parse()
        .ok()
}

pub fn read(data: &[u8]) -> Result<Vec<super::Sheet>, Error> {
    let mut zip = Archive::new(data)?;
    let root_rels = rels(&mut zip, "")?;
    let wb_path = root_rels
        .iter()
        .find(|(_, t, _)| t.ends_with("/officeDocument"))
        .map(|(_, _, p)| p.clone())
        .unwrap_or_else(|| "xl/workbook.xml".into());
    let wb = parse(
        &zip.read(&wb_path)?
            .ok_or_else(|| Error::MissingPart(wb_path.clone()))?,
    )?;
    let wb_rels = rels(&mut zip, &wb_path)?;
    let date1904 = wb
        .child("s:workbookPr")
        .and_then(|p| p.attr("date1904"))
        .is_some_and(|v| v == "1" || v == "true");
    let styles_path = wb_rels
        .iter()
        .find(|(_, t, _)| t.ends_with("/styles"))
        .map(|(_, _, p)| p.clone());
    let styles = read_styles(&mut zip, styles_path.as_deref())?;
    let strings_path = wb_rels
        .iter()
        .find(|(_, t, _)| t.ends_with("/sharedStrings"))
        .map(|(_, _, p)| p.clone());
    let shared: Vec<String> = match strings_path.map(|p| zip.read(&p)).transpose()?.flatten() {
        Some(b) => parse(&b)?
            .children_named("s:si")
            .map(|si| text_content(si).replace("x005F_", ""))
            .collect(),
        None => Vec::new(),
    };
    let mut sheets = Vec::new();
    for sheet in wb
        .child("s:sheets")
        .into_iter()
        .flat_map(|s| s.children_named("s:sheet"))
    {
        let name = sheet.attr("name").unwrap_or_default().to_string();
        let Some((_, typ, path)) = sheet
            .attr("r:id")
            .and_then(|id| wb_rels.iter().find(|(i, _, _)| i == id))
        else {
            continue;
        };
        // pandas lists `book.worksheets`: chart sheets are left out
        if !typ.ends_with("/worksheet") {
            continue;
        }
        let rows = match zip.read(path)? {
            Some(bytes) => sheet_rows(&parse(&bytes)?, &shared, &styles, date1904)?,
            None => Vec::new(),
        };
        sheets.push(super::Sheet { name, rows });
    }
    Ok(sheets)
}

fn cell_value(
    c: &Element,
    shared: &[String],
    styles: &Styles,
    date1904: bool,
) -> Result<Cell, Error> {
    let t = c.attr("t").unwrap_or("n");
    if t == "inlineStr" {
        return Ok(match c.child("s:is") {
            Some(is) => Cell::Str(text_content(is)),
            None => Cell::Empty,
        });
    }
    let v = text_of(c.child("s:v"));
    if v.is_empty() {
        return Ok(Cell::Empty);
    }
    let style: usize = c.attr("s").and_then(|s| s.parse().ok()).unwrap_or(0);
    let bad = |what: &str| Error::Conversion(format!("invalid {what} cell value {v:?}"));
    Ok(match t {
        "n" => {
            let num = if v.contains(['.', 'E', 'e']) {
                pystr::strip(&v).parse::<f64>().map_err(|_| bad("number"))?
            } else {
                // Python int(): Excel never stores more than 15 digits
                return Ok(match pystr::strip(&v).parse::<i128>() {
                    Ok(i) if styles.date.get(style).copied().unwrap_or(false) => from_excel(
                        i as f64,
                        date1904,
                        styles.timedelta.get(style).copied().unwrap_or(false),
                    )
                    .unwrap_or(Cell::NaN),
                    Ok(i) => Cell::Int(i),
                    Err(_) => return Err(bad("number")),
                });
            };
            if styles.date.get(style).copied().unwrap_or(false) {
                from_excel(
                    num,
                    date1904,
                    styles.timedelta.get(style).copied().unwrap_or(false),
                )
                .unwrap_or(Cell::NaN)
            } else if num.is_finite() && num.fract() == 0.0 {
                // pandas: int(value) when it equals the value
                super::int_cell(num)
            } else if num.is_finite() {
                Cell::Float(num)
            } else {
                return Err(bad("number"));
            }
        }
        "s" => {
            let i: usize = pystr::strip(&v).parse().map_err(|_| bad("shared string"))?;
            Cell::Str(shared.get(i).cloned().ok_or_else(|| bad("shared string"))?)
        }
        "b" => Cell::Bool(
            pystr::strip(&v)
                .parse::<i64>()
                .map_err(|_| bad("boolean"))?
                != 0,
        ),
        "e" => Cell::NaN,
        "d" => from_iso8601(&v).ok_or_else(|| bad("date"))?,
        _ => Cell::Str(v),
    })
}

/// openpyxl read-only rows, then pandas' `get_sheet_data` trimming.
fn sheet_rows(
    ws: &Element,
    shared: &[String],
    styles: &Styles,
    date1904: bool,
) -> Result<Vec<Vec<Cell>>, Error> {
    let mut data: Vec<Vec<Cell>> = Vec::new();
    let Some(sheet_data) = ws.child("s:sheetData") else {
        return Ok(data);
    };
    let mut counter = 1usize;
    let mut row_counter = 0usize;
    for row in sheet_data.children_named("s:row") {
        row_counter = row
            .attr("r")
            .and_then(|r| pystr::strip(r).parse::<f64>().ok())
            .map_or(row_counter + 1, |v| v as usize);
        let idx = row_counter;
        while counter < idx {
            data.push(Vec::new());
            counter += 1;
        }
        if counter > idx {
            continue;
        }
        counter += 1;
        let mut cells: Vec<(usize, Cell)> = Vec::new();
        let mut col_counter = 0usize;
        for c in row.children_named("s:c") {
            col_counter = c.attr("r").and_then(column_of).unwrap_or(col_counter + 1);
            let _ = c.attr("r").and_then(row_of);
            cells.push((col_counter, cell_value(c, shared, styles, date1904)?));
        }
        let width = cells.last().map_or(0, |(col, _)| *col);
        let mut out = vec![Cell::Empty; width];
        for (col, v) in cells {
            if (1..=width).contains(&col) {
                out[col - 1] = v;
            }
        }
        data.push(out);
    }
    // pandas: drop trailing empty cells per row, trailing empty rows, then pad
    for row in &mut data {
        while row.last() == Some(&Cell::Empty) {
            row.pop();
        }
    }
    while data.last().is_some_and(Vec::is_empty) {
        data.pop();
    }
    let width = data.iter().map(Vec::len).max().unwrap_or(0);
    for row in &mut data {
        row.resize(width, Cell::Empty);
    }
    Ok(data)
}
