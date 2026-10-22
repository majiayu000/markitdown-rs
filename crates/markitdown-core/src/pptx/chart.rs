//! Chart parts: the category and series caches python-pptx reads, laid out
//! as markitdown's chart table.

use super::{frame_text, leading_text};
use crate::pystr;
use crate::xml::{Element, Node};
use std::collections::HashMap;

/// `str(float)` as CPython prints it.
fn py_float(v: f64) -> String {
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

pub(super) enum ChartError {
    /// what markitdown prints as "[unsupported chart]"
    Unsupported,
    /// a ValueError that escapes markitdown's handler and fails the file
    Value(String),
}

const PLOT_TAGS: &[&str] = &[
    "c:area3DChart",
    "c:areaChart",
    "c:bar3DChart",
    "c:barChart",
    "c:bubbleChart",
    "c:doughnutChart",
    "c:line3DChart",
    "c:lineChart",
    "c:ofPieChart",
    "c:pie3DChart",
    "c:pieChart",
    "c:radarChart",
    "c:scatterChart",
    "c:stockChart",
    "c:surface3DChart",
    "c:surfaceChart",
];

fn descendants<'e>(e: &'e Element, name: &str, out: &mut Vec<&'e Element>) {
    for c in e.elements() {
        if &*c.name == name {
            out.push(c);
        }
        descendants(c, name, out);
    }
}

fn first_descendant<'e>(e: &'e Element, name: &str) -> Option<&'e Element> {
    let mut v = Vec::new();
    descendants(e, name, &mut v);
    v.into_iter().next()
}

fn parse_int(v: Option<&str>) -> Result<i64, ChartError> {
    let v = v.unwrap_or_default();
    v.trim()
        .parse()
        .map_err(|_| ChartError::Value(format!("invalid integer {v:?}")))
}

pub(super) fn chart_markdown(chart: &Element) -> Result<String, ChartError> {
    let mut md = "\n\n### Chart".to_string();
    if let Some(rich) = chart.path(&["c:title", "c:tx", "c:rich"]) {
        md.push_str(": ");
        md.push_str(&frame_text(Some(rich)));
    }
    md.push_str("\n\n");
    let plot_area = chart.child("c:plotArea").ok_or(ChartError::Unsupported)?;
    let x_charts: Vec<&Element> = plot_area
        .elements()
        .filter(|e| PLOT_TAGS.contains(&e.name.as_ref()))
        .collect();
    let first = x_charts.first().ok_or(ChartError::Unsupported)?;
    let supported_plots = [
        "c:areaChart",
        "c:area3DChart",
        "c:barChart",
        "c:bubbleChart",
        "c:doughnutChart",
        "c:lineChart",
        "c:pieChart",
        "c:radarChart",
        "c:scatterChart",
    ];
    if !supported_plots.contains(&first.name.as_ref()) {
        return Err(ChartError::Unsupported);
    }

    // categories of the first plot
    let mut counts = Vec::new();
    for ser in first.children_named("c:ser") {
        if let Some(cat) = ser.child("c:cat") {
            descendants(cat, "c:ptCount", &mut counts);
        }
    }
    let cat_count = match counts.first() {
        Some(c) => parse_int(c.attr("val"))?,
        None => 0,
    };
    let mut cat_pts = Vec::new();
    if let Some(cat) = first.child("c:ser").and_then(|s| s.child("c:cat")) {
        let mut lvls = Vec::new();
        first_lvls(cat, &mut lvls);
        for l in lvls {
            cat_pts.extend(l.children_named("c:pt"));
        }
        if cat_pts.is_empty() {
            descendants(cat, "c:pt", &mut cat_pts);
        }
    }
    let mut by_idx: HashMap<i64, &Element> = HashMap::new();
    for pt in cat_pts {
        by_idx.insert(parse_int(pt.attr("idx"))?, pt);
    }
    let categories: Vec<String> = (0..cat_count)
        .map(|i| match by_idx.get(&i) {
            None => String::new(),
            Some(pt) => pt
                .child("c:v")
                .map(|v| {
                    if v.children.is_empty() {
                        "None".into()
                    } else {
                        leading_text(v)
                    }
                })
                .unwrap_or_default(),
        })
        .collect();

    // every series of every plot, by c:order within each plot
    let mut names = Vec::new();
    let mut values: Vec<Vec<String>> = Vec::new();
    for xc in &x_charts {
        let value_tag = match xc.name.as_ref() {
            "c:areaChart" | "c:barChart" | "c:doughnutChart" | "c:lineChart" | "c:pieChart"
            | "c:radarChart" => "c:val",
            "c:bubbleChart" | "c:scatterChart" => "c:yVal",
            _ => return Err(ChartError::Unsupported), // NotImplementedError
        };
        let mut sers: Vec<(i64, &Element)> = Vec::new();
        for s in xc.children_named("c:ser") {
            let order = s.child("c:order").ok_or(ChartError::Unsupported)?;
            sers.push((parse_int(order.attr("val"))?, s));
        }
        sers.sort_by_key(|(o, _)| *o);
        for (_, ser) in sers {
            names.push(series_name(ser));
            values.push(series_values(ser.child(value_tag))?);
        }
    }

    let mut rows = vec![format!("| Category | {} |", names.join(" | "))];
    if names.is_empty() {
        rows[0] = "| Category |".into();
    }
    for (i, cat) in categories.iter().enumerate() {
        let mut row = vec![cat.clone()];
        for v in &values {
            row.push(v.get(i).cloned().unwrap_or_else(|| "None".into()));
        }
        rows.push(format!("| {} |", row.join(" | ")));
    }
    let separator = format!("|{}|", vec!["---"; names.len() + 1].join("|"));
    rows.insert(1, separator);
    Ok(md + &rows.join("\n"))
}

/// `c:lvl[1]` descendants: the first `c:lvl` child of any element.
fn first_lvls<'e>(e: &'e Element, out: &mut Vec<&'e Element>) {
    if let Some(l) = e.child("c:lvl") {
        out.push(l);
    }
    for c in e.elements() {
        first_lvls(c, out);
    }
}

/// `./c:tx//c:pt/c:v/text()`, first match.
fn series_name(ser: &Element) -> String {
    let Some(tx) = ser.child("c:tx") else {
        return String::new();
    };
    let mut pts = Vec::new();
    descendants(tx, "c:pt", &mut pts);
    pts.iter()
        .flat_map(|pt| pt.children_named("c:v"))
        .flat_map(|v| v.children.iter())
        .find_map(|n| {
            if let Node::Text(t) = n {
                Some(t.clone())
            } else {
                None
            }
        })
        .unwrap_or_default()
}

fn series_values(val: Option<&Element>) -> Result<Vec<String>, ChartError> {
    let Some(val) = val else {
        return Ok(Vec::new());
    };
    let count = match first_descendant(val, "c:ptCount") {
        Some(c) => parse_int(c.attr("val"))?,
        None => 0,
    };
    let mut pts = Vec::new();
    descendants(val, "c:pt", &mut pts);
    let mut out = Vec::new();
    for i in 0..count {
        let mut found = None;
        for pt in &pts {
            if parse_int(pt.attr("idx"))? == i {
                found = Some(*pt);
                break;
            }
        }
        out.push(match found {
            None => "None".to_string(),
            Some(pt) => {
                let text = pt.child("c:v").map(leading_text).unwrap_or_default();
                let f: f64 = pystr::strip(&text).replace('_', "").parse().map_err(|_| {
                    ChartError::Value(format!("could not convert string to float: {text:?}"))
                })?;
                py_float(f)
            }
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::py_float;

    #[test]
    fn float_repr_matches_python() {
        let cases = [
            (1.0, "1.0"),
            (4.3, "4.3"),
            (-2.5, "-2.5"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e16, "1e+16"),
            (123456789012345680.0, "1.2345678901234568e+17"),
            (1234567890123456.0, "1234567890123456.0"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100.0"),
        ];
        for (v, want) in cases {
            assert_eq!(py_float(v), want, "{v}");
        }
    }
}
