//! The pandas half of `pd.read_excel(...).to_html(index=False)`: the
//! python parser's header handling and per-column type inference, then the
//! value formatting of `DataFrame.to_html`.

use super::{Cell, DateTime};
use crate::pystr;

/// pandas' default STR_NA_VALUES
const NA_VALUES: &[&str] = &[
    "", "#N/A", "#N/A N/A", "#NA", "-1.#IND", "-1.#QNAN", "-NaN", "-nan", "1.#IND", "1.#QNAN",
    "<NA>", "N/A", "NA", "NULL", "NaN", "None", "n/a", "nan", "null",
];

#[derive(Debug, Clone)]
enum Column {
    Int(Vec<i128>),
    Float(Vec<f64>),
    Bool(Vec<bool>),
    Str(Vec<Option<String>>),
    DateTime(Vec<Option<DateTime>>),
    /// object dtype; None is NaN
    Object(Vec<Option<Cell>>),
}

fn is_na(c: &Cell) -> bool {
    match c {
        Cell::Empty | Cell::NaN => true,
        Cell::Float(f) => f.is_nan(),
        Cell::Str(s) => NA_VALUES.contains(&s.as_str()),
        _ => false,
    }
}

enum Num {
    Int(i128),
    Float(f64),
}

/// The numbers `lib.maybe_convert_numeric` accepts in strings.
fn parse_number(s: &str) -> Option<Num> {
    let t = pystr::strip(s);
    let digits = t.strip_prefix(['+', '-']).unwrap_or(t);
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        return t.parse::<i128>().ok().map(Num::Int);
    }
    let lower = digits.to_ascii_lowercase();
    if matches!(lower.as_str(), "inf" | "infinity") {
        return t.parse::<f64>().ok().map(Num::Float);
    }
    let valid = !digits.is_empty()
        && digits
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'));
    if valid {
        t.parse::<f64>().ok().map(Num::Float)
    } else {
        None
    }
}

fn infer(values: &[&Cell]) -> Column {
    // maybe_convert_numeric
    let mut nums: Vec<Option<Num>> = Vec::with_capacity(values.len());
    let mut numeric = true;
    let (mut seen_float, mut seen_null, mut all_bool, mut seen_big) = (false, false, true, false);
    for v in values {
        let n = match v {
            v if is_na(v) => None,
            Cell::Int(i) => Some(Num::Int(*i)),
            Cell::Float(f) => Some(Num::Float(*f)),
            Cell::Bool(b) => Some(Num::Int(*b as i128)),
            Cell::Str(s) => match parse_number(s) {
                Some(n) => Some(n),
                None => {
                    numeric = false;
                    break;
                }
            },
            _ => {
                numeric = false;
                break;
            }
        };
        match &n {
            None => seen_null = true,
            Some(Num::Float(_)) => seen_float = true,
            Some(Num::Int(i)) => seen_big |= *i > i64::MAX as i128 || *i < i64::MIN as i128,
        }
        all_bool &= matches!(v, Cell::Bool(_));
        nums.push(n);
    }
    if numeric && !seen_big {
        if seen_null || seen_float {
            return Column::Float(
                nums.into_iter()
                    .map(|n| match n {
                        None => f64::NAN,
                        Some(Num::Int(i)) => i as f64,
                        Some(Num::Float(f)) => f,
                    })
                    .collect(),
            );
        }
        if all_bool && !values.is_empty() {
            return Column::Bool(
                values
                    .iter()
                    .map(|v| matches!(v, Cell::Bool(true)))
                    .collect(),
            );
        }
        return Column::Int(
            nums.into_iter()
                .map(|n| if let Some(Num::Int(i)) = n { i } else { 0 })
                .collect(),
        );
    }
    // object: NA values become NaN, then maybe_convert_bool
    let objs = sanitize_objects(values);
    let as_bool = |c: &Cell| match c {
        Cell::Bool(b) => Some(*b),
        Cell::Str(s) if matches!(s.as_str(), "True" | "TRUE" | "true") => Some(true),
        Cell::Str(s) if matches!(s.as_str(), "False" | "FALSE" | "false") => Some(false),
        _ => None,
    };
    if !objs.is_empty()
        && objs
            .iter()
            .all(|o| o.as_ref().is_none_or(|c| as_bool(c).is_some()))
    {
        if objs.iter().all(Option::is_some) {
            return Column::Bool(
                objs.iter()
                    .map(|o| o.as_ref().and_then(as_bool).unwrap_or(false))
                    .collect(),
            );
        }
        return Column::Object(
            objs.into_iter()
                .map(|o| o.map(|c| Cell::Bool(as_bool(&c).unwrap_or(false))))
                .collect(),
        );
    }
    let present: Vec<&Cell> = objs.iter().flatten().collect();
    if !present.is_empty() && present.iter().all(|c| matches!(c, Cell::Str(_))) {
        return Column::Str(
            objs.into_iter()
                .map(|o| {
                    o.map(|c| {
                        if let Cell::Str(s) = c {
                            s
                        } else {
                            String::new()
                        }
                    })
                })
                .collect(),
        );
    }
    if !present.is_empty() && present.iter().all(|c| matches!(c, Cell::DateTime(_))) {
        return Column::DateTime(
            objs.into_iter()
                .map(|o| {
                    o.map(|c| {
                        if let Cell::DateTime(d) = c {
                            d
                        } else {
                            unreachable!()
                        }
                    })
                })
                .collect(),
        );
    }
    Column::Object(objs)
}

/// Python equality classes for dict keys: True == 1 == 1.0 and False == 0.
#[derive(PartialEq, Eq, Hash)]
enum Key {
    Int(i128),
    Float(u64),
    Str(String),
    Other(String),
}

fn key(c: &Cell) -> Key {
    match c {
        Cell::Int(i) => Key::Int(*i),
        Cell::Bool(b) => Key::Int(*b as i128),
        Cell::Float(f) if f.fract() == 0.0 && f.abs() < 1e38 => Key::Int(*f as i128),
        Cell::Float(f) => Key::Float(f.to_bits()),
        Cell::Str(s) => Key::Str(s.clone()),
        other => Key::Other(format!("{other:?}")),
    }
}

/// `parsers.sanitize_objects`: NA values become NaN, and every other value is
/// replaced by the first equal value seen (a `memo` dict), so `True` after
/// `1` becomes `1` and `1.0` after `1` becomes `1`.
fn sanitize_objects(values: &[&Cell]) -> Vec<Option<Cell>> {
    let mut memo: std::collections::HashMap<Key, Cell> = std::collections::HashMap::new();
    values
        .iter()
        .map(|v| {
            if is_na(v) {
                return None;
            }
            Some(memo.entry(key(v)).or_insert_with(|| (*v).clone()).clone())
        })
        .collect()
}

/// `"{:.6e}"` as Python prints it (at least two exponent digits).
fn py_exp(v: f64) -> String {
    if !v.is_finite() {
        return py_fixed(v);
    }
    let s = format!("{v:.6e}");
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let e: i32 = e.parse().unwrap_or(0);
    format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
}

/// `"{:.6f}"` as Python prints it.
fn py_fixed(v: f64) -> String {
    if v.is_nan() {
        "nan".into()
    } else if v.is_infinite() {
        if v > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        format!("{v:.6}")
    }
}

fn is_number_with_decimal(s: &str) -> bool {
    let t = s.trim_start();
    let t = t.strip_prefix(['+', '-']).unwrap_or(t);
    match t.split_once('.') {
        Some((int, frac)) => {
            !int.is_empty()
                && int.bytes().all(|b| b.is_ascii_digit())
                && frac.bytes().all(|b| b.is_ascii_digit())
        }
        None => false,
    }
}

/// `_trim_zeros_float`
fn trim_zeros(mut vals: Vec<String>) -> Vec<String> {
    loop {
        let nums: Vec<&String> = vals.iter().filter(|x| is_number_with_decimal(x)).collect();
        if nums.is_empty() || !nums.iter().all(|x| x.ends_with('0')) {
            break;
        }
        for v in &mut vals {
            if is_number_with_decimal(v) {
                v.pop();
            }
        }
    }
    vals.into_iter()
        .map(|v| {
            if is_number_with_decimal(&v) && v.ends_with('.') {
                v + "0"
            } else {
                v
            }
        })
        .collect()
}

/// FloatArrayFormatter with display.precision 6.
fn format_floats(vals: &[f64]) -> Vec<String> {
    let fmt = |f: fn(f64) -> String| {
        trim_zeros(
            vals.iter()
                .map(|&v| if v.is_nan() { "NaN".to_string() } else { f(v) })
                .collect(),
        )
    };
    let fixed = fmt(py_fixed);
    let too_long = fixed.iter().map(|s| s.chars().count()).max().unwrap_or(0) > 12;
    let large = vals.iter().any(|v| v.abs() > 1e6);
    let small = vals.iter().any(|v| v.abs() < 1e-6 && v.abs() > 0.0);
    if small || (too_long && large) {
        fmt(py_exp)
    } else {
        fixed
    }
}

/// `pprint_thing(x, escape_chars=("\t", "\r", "\n"))`
fn escape_controls(s: &str) -> String {
    s.replace('\t', "\\t")
        .replace('\r', "\\r")
        .replace('\n', "\\n")
}

/// `str(datetime.timedelta)`
fn py_timedelta(us: i64) -> String {
    let days = us.div_euclid(86_400_000_000);
    let rest = us.rem_euclid(86_400_000_000);
    let t = super::Time::from_micros(rest);
    let hms = format!("{}:{:02}:{:02}", t.hour, t.minute, t.second);
    let hms = if t.micro == 0 {
        hms
    } else {
        format!("{hms}.{:06}", t.micro)
    };
    match days {
        0 => hms,
        1 | -1 => format!("{days} day, {hms}"),
        _ => format!("{days} days, {hms}"),
    }
}

fn format_object(c: &Option<Cell>) -> String {
    match c {
        None => "NaN".into(),
        Some(Cell::Int(i)) => i.to_string(),
        Some(Cell::BigInt(s)) => s.clone(),
        Some(Cell::Float(f)) => {
            // _trim_zeros_single_float(f"{x: .6f}")
            let s = py_fixed(*f);
            let s = s.trim_end_matches('0');
            if s.ends_with('.') {
                format!("{s}0")
            } else {
                s.to_string()
            }
        }
        Some(Cell::Str(s)) => escape_controls(s),
        Some(Cell::Bool(b)) => {
            if *b {
                "True".into()
            } else {
                "False".into()
            }
        }
        Some(Cell::DateTime(d)) => d.py_str(),
        Some(Cell::Time(t)) => t.py_str(),
        Some(Cell::Duration(us)) => py_timedelta(*us),
        Some(Cell::Empty | Cell::NaN) => "NaN".into(),
    }
}

fn format_column(col: &Column) -> Vec<String> {
    match col {
        Column::Int(v) => v.iter().map(i128::to_string).collect(),
        Column::Float(v) => format_floats(v),
        Column::Bool(v) => v
            .iter()
            .map(|b| if *b { "True".into() } else { "False".into() })
            .collect(),
        Column::Str(v) => v
            .iter()
            .map(|s| s.as_deref().map_or_else(|| "NaN".into(), escape_controls))
            .collect(),
        Column::DateTime(v) => {
            let present: Vec<&DateTime> = v.iter().flatten().collect();
            let dates_only = present.iter().all(|d| d.is_midnight());
            let frac = if present.iter().all(|d| d.time.micro == 0) {
                0
            } else if present.iter().all(|d| d.time.micro % 1000 == 0) {
                3
            } else {
                6
            };
            v.iter()
                .map(|d| match d {
                    None => "NaT".into(),
                    Some(d) if dates_only => format!("{:04}-{:02}-{:02}", d.year, d.month, d.day),
                    Some(d) => {
                        let t = &d.time;
                        let base = format!(
                            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                            d.year, d.month, d.day, t.hour, t.minute, t.second
                        );
                        match frac {
                            3 => format!("{base}.{:03}", t.micro / 1000),
                            6 => format!("{base}.{:06}", t.micro),
                            _ => base,
                        }
                    }
                })
                .collect()
        }
        Column::Object(v) => v.iter().map(format_object).collect(),
    }
}

/// `_write_cell`: html-escape, strip, and keep double spaces as &nbsp;.
fn cell_html(s: &str) -> String {
    let escaped = s
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    pystr::strip(&escaped).replace("  ", "&nbsp;&nbsp;")
}

/// Header labels: "Unnamed: i" for blanks, then `dedup_names`, whose
/// counts dict treats `True` and `1` as the same name.
fn column_names(header: &[Cell]) -> Vec<String> {
    let mut labels: Vec<(Key, String)> = header
        .iter()
        .enumerate()
        .map(|(i, c)| match c {
            Cell::Empty => {
                let name = format!("Unnamed: {i}");
                (Key::Str(name.clone()), name)
            }
            Cell::NaN => (Key::Other("nan".into()), "nan".into()),
            Cell::Float(f) => (key(c), pystr::float_repr(*f)),
            Cell::Str(s) => (key(c), escape_controls(s)),
            other => (key(other), format_object(&Some(other.clone()))),
        })
        .collect();
    let mut counts: std::collections::HashMap<Key, usize> = std::collections::HashMap::new();
    for label in labels.iter_mut() {
        let mut cur = *counts.get(&label.0).unwrap_or(&0);
        while cur > 0 {
            counts.insert(
                std::mem::replace(&mut label.0, Key::Other(String::new())),
                cur + 1,
            );
            let shown = format!("{}.{cur}", label.1);
            *label = (Key::Str(shown.clone()), shown);
            cur = *counts.get(&label.0).unwrap_or(&0);
        }
        let k = std::mem::replace(&mut label.0, Key::Other(String::new()));
        counts.insert(k, cur + 1);
    }
    labels.into_iter().map(|(_, shown)| shown).collect()
}

/// `pd.read_excel(sheet).to_html(index=False)` for one sheet's rows.
pub fn to_html(rows: &[Vec<Cell>]) -> String {
    let width = rows.first().map_or(0, Vec::len);
    let mut html = String::from(
        r#"<table border="1" class="dataframe"><thead><tr style="text-align: right;">"#,
    );
    let Some(header) = rows.first() else {
        html.push_str("</tr></thead><tbody></tbody></table>");
        return html;
    };
    for name in column_names(header) {
        html.push_str(&format!("<th>{}</th>", cell_html(&name)));
    }
    html.push_str("</tr></thead><tbody>");
    let body = &rows[1..];
    let columns: Vec<Vec<String>> = (0..width)
        .map(|c| format_column(&infer(&body.iter().map(|r| &r[c]).collect::<Vec<_>>())))
        .collect();
    for r in 0..body.len() {
        html.push_str("<tr>");
        for col in &columns {
            html.push_str(&format!("<td>{}</td>", cell_html(&col[r])));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
    html
}
