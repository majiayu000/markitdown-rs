//! What markitdown asks of pdfplumber: page geometry, chars in top-down
//! coordinates, `extract_words()` and `extract_text()`.

use std::collections::HashMap;

use super::document::Document;
use super::interp::{Item, PageItems};
use super::object::{Obj, PdfError, Result};
use super::page::Page;
use crate::pystr;

pub struct PChar {
    pub text: String,
    pub x0: f64,
    pub x1: f64,
    pub top: f64,
    pub bottom: f64,
    pub upright: bool,
}

pub struct Word {
    pub text: String,
    pub x0: f64,
    pub x1: f64,
    pub top: f64,
}

/// pdfplumber's Page geometry; Err when pdfplumber itself would raise.
pub struct Geometry {
    pub width: f64,
    pub height: f64,
    mb_x0: f64,
    mb_top: f64,
}

/// pdfplumber's `resolve_all()` (it leaves references to pages alone).
fn resolve_all(doc: &Document, x: &Obj, depth: u32) -> Result<Obj> {
    if depth > 200 {
        return Err(PdfError::Other("RecursionError".into()));
    }
    Ok(match x {
        Obj::Ref(_) => {
            let r = doc.resolve(x)?;
            if let Obj::Dict(d) = &r
                && d.get("Type").is_some_and(|t| t.is_name("Page"))
            {
                return Ok(x.clone());
            }
            resolve_all(doc, &r, depth + 1)?
        }
        Obj::Array(a) => Obj::Array(std::rc::Rc::new(
            a.iter()
                .map(|v| resolve_all(doc, v, depth + 1))
                .collect::<Result<_>>()?,
        )),
        _ => x.clone(),
    })
}

fn fail<T>() -> Result<T> {
    Err(PdfError::Other("pdfplumber".into()))
}

/// `_normalize_box()`, and that box inverted against `mb_height` (the
/// box's own height for the media box).
fn page_box(
    doc: &Document,
    page: &Page,
    key: &str,
    rotation: f64,
    mb_height: Option<f64>,
) -> Result<([f64; 4], [f64; 4])> {
    let raw = match page.attrs.get(key) {
        Some(v) => resolve_all(doc, v, 0)?,
        None => Obj::Null,
    };
    let nums: Vec<f64> = match &raw {
        Obj::Array(a) => a
            .iter()
            .map(|v| v.as_num())
            .collect::<Option<_>>()
            .map_or_else(fail, Ok)?,
        Obj::Str(s) => s.iter().map(|&b| b as f64).collect(),
        _ => return fail(),
    };
    if nums.len() < 4 {
        return fail();
    }
    let sorted = |a: f64, b: f64| if b < a { (b, a) } else { (a, b) };
    let (x0, x1) = sorted(nums[0], nums[2]);
    let (y0, y1) = sorted(nums[1], nums[3]);
    let b = if rotation == 90.0 || rotation == 270.0 {
        [y0, x0, y1, x1]
    } else {
        [x0, y0, x1, y1]
    };
    let h = mb_height.unwrap_or(b[3] - b[1]);
    Ok((b, [b[0], h - b[3], b[2], h - b[1]]))
}

impl Geometry {
    pub fn new(doc: &Document, page: &Page) -> Result<Self> {
        let rotation = match page.attrs.get("Rotate") {
            None => 0.0,
            Some(v) => match resolve_all(doc, v, 0)? {
                Obj::Null => 0.0,
                r => r.as_num().map_or_else(fail, Ok)?.rem_euclid(360.0),
            },
        };
        let (raw, mb) = page_box(doc, page, "MediaBox", rotation, None)?;
        let mb_height = raw[3] - raw[1];
        for key in ["CropBox", "TrimBox", "BleedBox", "ArtBox"] {
            if page.attrs.contains(key) {
                page_box(doc, page, key, rotation, Some(mb_height))?;
            }
        }
        Ok(Geometry {
            width: mb[2] - mb[0],
            height: mb[3] - mb[1],
            mb_x0: mb[0],
            mb_top: mb[1],
        })
    }

    /// `page.chars` from the LTPage items, flattened in drawing order.
    pub fn chars(&self, items: &PageItems) -> Vec<PChar> {
        let mut out = Vec::new();
        self.flatten(&items.items, &mut out);
        out
    }

    fn flatten(&self, items: &[Item], out: &mut Vec<PChar>) {
        for it in items {
            match it {
                Item::Figure(f) => self.flatten(f, out),
                Item::Char(c) => {
                    let top = self.height - c.y1 + self.mb_top;
                    let (mut x0, mut x1) = (c.x0, c.x1);
                    if self.mb_x0 != 0.0 {
                        x0 += self.mb_x0;
                        x1 += self.mb_x0;
                    }
                    out.push(PChar {
                        text: c.text.clone(),
                        x0,
                        x1,
                        top,
                        bottom: self.height - c.y0 + self.mb_top,
                        upright: c.upright,
                    });
                }
            }
        }
    }
}

const LIGATURES: &[(&str, &str)] = &[
    ("\u{fb00}", "ff"),
    ("\u{fb03}", "ffi"),
    ("\u{fb04}", "ffl"),
    ("\u{fb01}", "fi"),
    ("\u{fb02}", "fl"),
    ("\u{fb06}", "st"),
    ("\u{fb05}", "st"),
];

fn expand(t: &str) -> &str {
    LIGATURES
        .iter()
        .find(|(l, _)| *l == t)
        .map_or(t, |(_, e)| e)
}

/// `cluster_objects(xs, key, tolerance)`: groups of indices.
fn cluster(keys: &[f64], tolerance: f64, preserve_order: bool) -> Vec<Vec<usize>> {
    // floats keyed as Python dicts and sets key them (0.0 == -0.0)
    let bits = |v: f64| (v + 0.0).to_bits();
    let mut seen = std::collections::HashSet::new();
    let mut values: Vec<f64> = Vec::new();
    for &k in keys {
        if seen.insert(bits(k)) {
            values.push(k);
        }
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut ids: HashMap<u64, usize> = HashMap::new();
    let mut group = 0;
    for (i, &v) in values.iter().enumerate() {
        if i > 0 && !(tolerance != 0.0 && values.len() >= 2 && v <= values[i - 1] + tolerance) {
            group += 1;
        }
        ids.insert(bits(v), group);
    }
    let id_of = |k: f64| ids.get(&bits(k)).copied();
    let mut tagged: Vec<(usize, Option<usize>)> = keys
        .iter()
        .enumerate()
        .map(|(i, &k)| (i, id_of(k)))
        .collect();
    if !preserve_order {
        tagged.sort_by_key(|t| t.1);
    }
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut last: Option<Option<usize>> = None;
    for (i, g) in tagged {
        if last == Some(g) {
            out.last_mut().unwrap().push(i);
        } else {
            out.push(vec![i]);
            last = Some(g);
        }
    }
    out
}

fn py_sort_by_key<T>(v: &mut [T], key: impl Fn(&T) -> (f64, f64)) {
    v.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        ka.0.partial_cmp(&kb.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(ka.1.partial_cmp(&kb.1).unwrap_or(std::cmp::Ordering::Equal))
    });
}

/// WordExtractor with pdfplumber's defaults (x/y tolerance 3) and
/// `keep_blank_chars`; the words and their chars, in extraction order.
pub fn words(chars: &[PChar], keep_blank_chars: bool) -> Vec<(Word, Vec<usize>)> {
    const TOL: f64 = 3.0;
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let upright = chars[start].upright;
        let mut end = start;
        while end < chars.len() && chars[end].upright == upright {
            end += 1;
        }
        let group: Vec<usize> = (start..end).collect();
        start = end;
        // lines run top to bottom for upright chars, else left to right
        let keys: Vec<f64> = group
            .iter()
            .map(|&i| if upright { chars[i].top } else { chars[i].x0 })
            .collect();
        for sc in cluster(&keys, TOL, false) {
            let mut line: Vec<usize> = sc.into_iter().map(|j| group[j]).collect();
            if upright {
                py_sort_by_key(&mut line, |&i| (chars[i].x0, chars[i].x0));
            } else {
                py_sort_by_key(&mut line, |&i| (chars[i].top, chars[i].bottom));
            }
            let mut current: Vec<usize> = Vec::new();
            let emit = |w: &mut Vec<usize>, out: &mut Vec<(Word, Vec<usize>)>| {
                if !w.is_empty() {
                    let w = std::mem::take(w);
                    out.push((merge(chars, &w), w));
                }
            };
            for i in line {
                let text = &chars[i].text;
                if !keep_blank_chars && !text.is_empty() && text.chars().all(pystr::is_space) {
                    emit(&mut current, &mut out);
                } else if text.is_empty() {
                    emit(&mut current, &mut out);
                    current.push(i);
                    emit(&mut current, &mut out);
                } else if let Some(&prev) = current.last()
                    && begins_new_word(&chars[prev], &chars[i], upright, TOL)
                {
                    emit(&mut current, &mut out);
                    current.push(i);
                } else {
                    current.push(i);
                }
            }
            emit(&mut current, &mut out);
        }
    }
    out
}

fn begins_new_word(prev: &PChar, curr: &PChar, upright: bool, tol: f64) -> bool {
    let (ax, bx, cx, ay, cy) = if upright {
        (prev.x0, prev.x1, curr.x0, prev.top, curr.top)
    } else {
        (prev.top, prev.bottom, curr.top, prev.x0, curr.x0)
    };
    cx < ax || cx > bx + tol || (cy - ay).abs() > tol
}

fn merge(chars: &[PChar], ids: &[usize]) -> Word {
    let first = |f: fn(&PChar) -> f64, less: bool| {
        ids.iter().skip(1).fold(f(&chars[ids[0]]), |m, &i| {
            let v = f(&chars[i]);
            if (less && v < m) || (!less && v > m) {
                v
            } else {
                m
            }
        })
    };
    Word {
        text: ids.iter().map(|&i| expand(&chars[i].text)).collect(),
        x0: first(|c| c.x0, true),
        x1: first(|c| c.x1, false),
        top: first(|c| c.top, true),
    }
}

/// `page.extract_text()` without layout: words joined by spaces, lines
/// (by word top, in extraction order) by newlines.
pub fn extract_text(chars: &[PChar]) -> String {
    let words = words(chars, false);
    let keys: Vec<f64> = words.iter().map(|(w, _)| w.top).collect();
    let mut out = String::new();
    for (n, line) in cluster(&keys, 3.0, true).into_iter().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        for (k, &wi) in line.iter().enumerate() {
            if k > 0 {
                out.push(' ');
            }
            for &c in &words[wi].1 {
                out.push_str(expand(&chars[c].text));
            }
        }
    }
    out
}

/// `PARTIAL_NUMBERING_PATTERN.match(s)`: `^\.\d+$`
pub fn is_partial_numbering(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('.') else {
        return false;
    };
    let rest = rest.strip_suffix('\n').unwrap_or(rest);
    !rest.is_empty() && rest.chars().all(is_decimal)
}

/// Python `str.isdecimal()` (category Nd) for one character.
fn is_decimal(c: char) -> bool {
    let v = c as u32;
    super::tables::DECIMAL_STARTS
        .iter()
        .any(|&s| v >= s && v < s + 10)
}

fn ljust(s: &str, w: usize) -> String {
    let n = s.chars().count();
    format!("{s}{}", " ".repeat(w.saturating_sub(n)))
}

struct RowInfo {
    words: Vec<usize>,
    text: String,
    x_groups: Vec<f64>,
    is_paragraph: bool,
    has_partial_numbering: bool,
    is_table_row: bool,
}

/// markitdown's `_extract_form_content_from_words`: Ok(None) when the
/// page does not look like a form.
pub fn form_content(chars: &[PChar], page_width: f64) -> Result<Option<String>> {
    let words: Vec<Word> = words(chars, true).into_iter().map(|(w, _)| w).collect();
    if words.is_empty() {
        return Ok(None);
    }
    let mut rows: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, w) in words.iter().enumerate() {
        let q = (w.top / 5.0).round_ties_even();
        if !q.is_finite() {
            return Err(PdfError::Other("cannot convert float to integer".into()));
        }
        rows.entry(q as i64 * 5).or_default().push(i);
    }
    let mut keys: Vec<i64> = rows.keys().copied().collect();
    keys.sort();
    let mut info: Vec<RowInfo> = Vec::new();
    for k in keys {
        let mut rw = rows.remove(&k).unwrap();
        rw.sort_by(|a, b| {
            words[*a]
                .x0
                .partial_cmp(&words[*b].x0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let line_width = words[*rw.last().unwrap()].x1 - words[rw[0]].x0;
        let text = rw
            .iter()
            .map(|&i| words[i].text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut xs: Vec<f64> = rw.iter().map(|&i| words[i].x0).collect();
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut x_groups: Vec<f64> = Vec::new();
        for x in xs {
            if x_groups.last().is_none_or(|&g| x - g > 50.0) {
                x_groups.push(x);
            }
        }
        info.push(RowInfo {
            is_paragraph: line_width > page_width * 0.55 && text.chars().count() > 60,
            has_partial_numbering: is_partial_numbering(pystr::strip(&words[rw[0]].text)),
            words: rw,
            text,
            x_groups,
            is_table_row: false,
        });
    }
    let mut all_x: Vec<f64> = info
        .iter()
        .filter(|r| r.x_groups.len() >= 3 && !r.is_paragraph)
        .flat_map(|r| r.x_groups.iter().copied())
        .collect();
    if all_x.is_empty() {
        return Ok(None);
    }
    all_x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut gaps: Vec<f64> = all_x
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|&g| g > 5.0)
        .collect();
    let tolerance = if gaps.len() >= 3 {
        gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let t = gaps[(gaps.len() as f64 * 0.70) as usize];
        let t = if t < 50.0 { t } else { 50.0 };
        if t > 25.0 { t } else { 25.0 }
    } else {
        35.0
    };
    let mut cols: Vec<f64> = Vec::new();
    for x in all_x {
        if cols.last().is_none_or(|&c| x - c > tolerance) {
            cols.push(x);
        }
    }
    if cols.len() <= 1 {
        return Ok(None);
    }
    let content_width = cols[cols.len() - 1] - cols[0];
    if (content_width / cols.len() as f64) < 30.0 {
        return Ok(None);
    }
    if cols.len() as f64 / (content_width / 72.0) > 10.0 {
        return Ok(None);
    }
    let max_cols = ((20.0 * (page_width / 612.0)) as i64).max(15);
    if cols.len() as i64 > max_cols {
        return Ok(None);
    }
    for r in &mut info {
        if r.is_paragraph || r.has_partial_numbering {
            continue;
        }
        let mut aligned = std::collections::HashSet::new();
        for &w in &r.words {
            if let Some(c) = cols.iter().position(|&cx| (words[w].x0 - cx).abs() < 40.0) {
                aligned.insert(c);
            }
        }
        r.is_table_row = aligned.len() >= 2;
    }
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < info.len() {
        if info[i].is_table_row {
            let s = i;
            while i < info.len() && info[i].is_table_row {
                i += 1;
            }
            regions.push((s, i));
        } else {
            i += 1;
        }
    }
    let table_rows: usize = regions.iter().map(|(s, e)| e - s).sum();
    if (table_rows as f64) / (info.len() as f64) < 0.2 {
        return Ok(None);
    }
    let n = cols.len();
    let cells = |r: &RowInfo| -> Vec<String> {
        let mut cells = vec![String::new(); n];
        for &w in &r.words {
            let x = words[w].x0;
            let col = (0..n - 1)
                .find(|&c| x < cols[c + 1] - 20.0)
                .unwrap_or(n - 1);
            if cells[col].is_empty() {
                cells[col] = words[w].text.clone();
            } else {
                cells[col].push(' ');
                cells[col].push_str(&words[w].text);
            }
        }
        cells
    };
    let mut lines: Vec<String> = Vec::new();
    let mut idx = 0;
    while idx < info.len() {
        if let Some(&(s, e)) = regions.iter().find(|(s, _)| *s == idx) {
            let data: Vec<Vec<String>> = (s..e).map(|r| cells(&info[r])).collect();
            let widths: Vec<usize> = (0..n)
                .map(|c| {
                    data.iter()
                        .map(|row| row[c].chars().count())
                        .max()
                        .unwrap_or(0)
                        .max(3)
                })
                .collect();
            let fmt = |row: &[String]| {
                let cells: Vec<String> =
                    row.iter().zip(&widths).map(|(c, w)| ljust(c, *w)).collect();
                format!("| {} |", cells.join(" | "))
            };
            lines.push(fmt(&data[0]));
            let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            lines.push(format!("| {} |", sep.join(" | ")));
            for row in &data[1..] {
                lines.push(fmt(row));
            }
            idx = e;
        } else {
            if !regions.iter().any(|&(s, e)| s < idx && idx < e) {
                lines.push(info[idx].text.clone());
            }
            idx += 1;
        }
    }
    Ok(Some(lines.join("\n")))
}
