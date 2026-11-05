//! Excel 3 and 4 worksheets: a bare BIFF stream (no OLE container), which
//! xlrd reads as a single sheet named "Sheet 1". Ported from xlrd's
//! `Sheet.read`, `handle_format`, `handle_xf` and `xf_epilogue`.

use super::{Cell, xls::xldate};
use crate::{Error, text};

const BOF3: u16 = 0x0209;
const BOF4: u16 = 0x0409;
const EOF: u16 = 0x000a;
const DIMENSIONS: u16 = 0x0200;
const BLANK: u16 = 0x0201;
const NUMBER: u16 = 0x0203;
const LABEL: u16 = 0x0204;
const BOOLERR: u16 = 0x0205;
const STRING: u16 = 0x0207;
const RK: u16 = 0x027e;
const FORMULA3: u16 = 0x0206;
const FORMULA4: u16 = 0x0406;
const FORMAT2: u16 = 0x001e;
const FORMAT4: u16 = 0x041e;
const XF3: u16 = 0x0243;
const XF4: u16 = 0x0443;
const DATEMODE: u16 = 0x0022;
const CODEPAGE: u16 = 0x0042;

/// Whether `data` is a bare Excel 3/4 worksheet stream.
pub fn is_biff34_worksheet(data: &[u8]) -> bool {
    data.len() >= 8
        && matches!(u16::from_le_bytes([data[0], data[1]]), BOF3 | BOF4)
        && u16::from_le_bytes([data[6], data[7]]) == 0x0010
}

#[derive(Clone, Copy, PartialEq)]
enum CellType {
    Number,
    Date,
    Text,
}

struct Records<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Iterator for Records<'a> {
    type Item = (u16, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        let head = self.data.get(self.pos..self.pos + 4)?;
        let rc = u16::from_le_bytes([head[0], head[1]]);
        let len = u16::from_le_bytes([head[2], head[3]]) as usize;
        let start = self.pos + 4;
        let body = self.data.get(start..(start + len).min(self.data.len()))?;
        self.pos = start + len;
        Some((rc, body))
    }
}

fn u16_at(d: &[u8], i: usize) -> Result<u16, Error> {
    d.get(i..i + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| Error::Conversion("truncated BIFF record".into()))
}

fn f64_at(d: &[u8], i: usize) -> Result<f64, Error> {
    d.get(i..i + 8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| Error::Conversion("truncated BIFF record".into()))
}

/// xlrd's `unpack_RK`
fn rk(b: &[u8]) -> f64 {
    let flags = b[0];
    let v = if flags & 2 != 0 {
        (i32::from_le_bytes([b[0], b[1], b[2], b[3]]) >> 2) as f64
    } else {
        f64::from_le_bytes([0, 0, 0, 0, flags & 252, b[1], b[2], b[3]])
    };
    if flags & 1 != 0 { v / 100.0 } else { v }
}

/// xlrd's `Book.derive_encoding` for BIFF before 8.
fn encoding(codepage: Option<u16>) -> String {
    match codepage {
        None => "iso-8859-1".into(),
        Some(1200) => "utf-16-le".into(),
        Some(10000 | 32768) => "mac-roman".into(),
        Some(32769) => "cp1252".into(),
        Some(c @ 300..=1999) => format!("cp{c}"),
        Some(c) => format!("unknown_codepage_{c}"),
    }
}

fn string(d: &[u8], pos: usize, lenlen: usize, enc: &str) -> Result<String, Error> {
    let n = match lenlen {
        1 => *d
            .get(pos)
            .ok_or_else(|| Error::Conversion("truncated BIFF string".into()))?
            as usize,
        _ => u16_at(d, pos)? as usize,
    };
    let bytes = d
        .get(pos + lenlen..pos + lenlen + n)
        .ok_or_else(|| Error::Conversion("truncated BIFF string".into()))?;
    text::decode(bytes, enc)
        .ok_or_else(|| Error::Conversion(format!("cannot decode BIFF string as {enc}")))
}

/// xlrd's `is_date_format_string`
fn is_date_format(fmt: &str) -> bool {
    let mut reduced = String::new();
    let mut state = 0;
    for c in fmt.chars() {
        match state {
            0 if c == '"' => state = 1,
            0 if "\\_*".contains(c) => state = 2,
            0 if "$-+/(): ".contains(c) => {}
            0 => reduced.push(c),
            1 if c == '"' => state = 0,
            2 => state = 0,
            _ => {}
        }
    }
    // drop [bracketed] parts
    let mut s = String::new();
    let mut rest = reduced.as_str();
    while let Some(open) = rest.find('[') {
        match rest[open..].find(']') {
            Some(close) => {
                s.push_str(&rest[..open]);
                rest = &rest[open + close + 1..];
            }
            None => break,
        }
    }
    s.push_str(rest);
    if matches!(
        s.as_str(),
        "0.00E+00" | "##0.0E+0" | "General" | "GENERAL" | "general" | "@"
    ) {
        return false;
    }
    let (mut dates, mut nums) = (0, 0);
    for c in s.chars() {
        if "ymdhsYMDHS".contains(c) {
            dates += 5;
        } else if "0#?".contains(c) {
            nums += 5;
        }
    }
    if dates > 0 && nums == 0 {
        return true;
    }
    if nums > 0 && dates == 0 {
        return false;
    }
    dates > nums
}

pub fn read(data: &[u8]) -> Result<Vec<super::Sheet>, Error> {
    let bv = if u16::from_le_bytes([data[0], data[1]]) == BOF4 {
        40
    } else {
        30
    };
    let mut codepage = None;
    let mut date1904 = false;
    // FORMAT records in order (their index is the XF's format key), and
    // the cell type of each XF
    let mut format_is_date: Vec<bool> = Vec::new();
    let mut xf_formats: Vec<u8> = Vec::new();
    let mut xf_types: Vec<CellType> = Vec::new();
    let mut epilogue_done = false;
    let mut cells: Vec<(usize, usize, Cell)> = Vec::new();
    let mut records = Records { data, pos: 0 };
    records.next(); // BOF
    let typed = |types: &[CellType], xf: u16| types.get(xf as usize).copied();
    let number = |v: f64, ty: Option<CellType>, date1904: bool| match ty {
        Some(CellType::Date) => xldate(v, date1904),
        // an XF without a known format: xlrd types the cell as text, and
        // pandas leaves the float as it is
        Some(CellType::Text) => Cell::Float(v),
        _ if v.is_finite() && v.fract() == 0.0 => super::int_cell(v),
        _ => Cell::Float(v),
    };
    while let Some((rc, d)) = records.next() {
        let enc = encoding(codepage);
        match rc {
            EOF => break,
            CODEPAGE => codepage = Some(u16_at(d, 0)?),
            DATEMODE => date1904 = u16_at(d, 0)? == 1,
            FORMAT2 | FORMAT4 => {
                let pos = if bv >= 40 && rc == FORMAT4 { 2 } else { 0 };
                format_is_date.push(is_date_format(&string(d, pos, 1, &enc)?));
            }
            XF3 | XF4 => {
                let key = *d
                    .get(1)
                    .ok_or_else(|| Error::Conversion("truncated XF".into()))?;
                xf_formats.push(key);
                let ty = match format_is_date.get(key as usize) {
                    Some(true) => CellType::Date,
                    Some(false) => CellType::Number,
                    None => CellType::Number,
                };
                xf_types.push(ty);
            }
            DIMENSIONS if !xf_formats.is_empty() && !epilogue_done => {
                // xf_epilogue: formats are known by now; an XF whose format
                // is missing makes its cells text
                epilogue_done = true;
                xf_types = xf_formats
                    .iter()
                    .map(|&k| match format_is_date.get(k as usize) {
                        Some(true) => CellType::Date,
                        Some(false) => CellType::Number,
                        None => CellType::Text,
                    })
                    .collect();
            }
            NUMBER => {
                let (r, c, xf) = (u16_at(d, 0)?, u16_at(d, 2)?, u16_at(d, 4)?);
                let v = number(f64_at(d, 6)?, typed(&xf_types, xf), date1904);
                cells.push((r as usize, c as usize, v));
            }
            RK => {
                let (r, c, xf) = (u16_at(d, 0)?, u16_at(d, 2)?, u16_at(d, 4)?);
                let b = d
                    .get(6..10)
                    .ok_or_else(|| Error::Conversion("truncated RK".into()))?;
                let v = number(rk(b), typed(&xf_types, xf), date1904);
                cells.push((r as usize, c as usize, v));
            }
            LABEL => {
                let (r, c) = (u16_at(d, 0)?, u16_at(d, 2)?);
                cells.push((r as usize, c as usize, Cell::Str(string(d, 6, 2, &enc)?)));
            }
            BOOLERR => {
                let (r, c) = (u16_at(d, 0)?, u16_at(d, 2)?);
                let (value, is_err) = (d.get(6).copied(), d.get(7).copied());
                let v = match (value, is_err) {
                    (Some(v), Some(0)) => Cell::Bool(v != 0),
                    _ => Cell::NaN,
                };
                cells.push((r as usize, c as usize, v));
            }
            FORMULA3 | FORMULA4 => {
                let (r, c, xf) = (u16_at(d, 0)?, u16_at(d, 2)?, u16_at(d, 4)?);
                let result = d
                    .get(6..14)
                    .ok_or_else(|| Error::Conversion("truncated FORMULA".into()))?;
                let v = if result[6..8] == [0xff, 0xff] {
                    match result[0] {
                        0 => match records.next() {
                            Some((STRING, s)) => Cell::Str(string(s, 0, 2, &enc)?),
                            _ => return Err(Error::Conversion("Expected STRING record".into())),
                        },
                        1 => Cell::Bool(result[2] != 0),
                        2 => Cell::NaN,
                        3 => Cell::Str(String::new()),
                        b => {
                            return Err(Error::Conversion(format!(
                                "unexpected special case (0x{b:02x}) in FORMULA"
                            )));
                        }
                    }
                } else {
                    number(f64_at(result, 0)?, typed(&xf_types, xf), date1904)
                };
                cells.push((r as usize, c as usize, v));
            }
            BLANK => {}
            _ => {}
        }
    }
    let height = cells.iter().map(|(r, _, _)| r + 1).max().unwrap_or(0);
    let width = cells.iter().map(|(_, c, _)| c + 1).max().unwrap_or(0);
    // Excel 3 and 4 sheets: 16,384 rows of 256 columns
    let mut rows = super::grid(height, width, 16_384, 256)?;
    for (r, c, v) in cells {
        rows[r][c] = v;
    }
    Ok(vec![super::Sheet {
        name: "Sheet 1".into(),
        rows,
        images: Vec::new(),
    }])
}

const BOUNDSHEET: u16 = 0x0085;
const XF8: u16 = 0x00e0;

/// xlrd's `std_format_code_types` for the built-in format keys.
fn builtin_type(key: u16) -> Option<CellType> {
    Some(match key {
        0..=13 | 37..=44 | 48 | 59..=62 | 67..=70 => CellType::Number,
        14..=22 | 27..=36 | 45..=47 | 50..=58 | 71..=81 => CellType::Date,
        49 => CellType::Text,
        _ => return None,
    })
}

/// A BIFF8 Unicode string with a `lenlen`-byte length and a flags byte.
fn unicode(d: &[u8], pos: usize, lenlen: usize) -> Option<String> {
    let n = match lenlen {
        1 => *d.get(pos)? as usize,
        _ => u16::from_le_bytes([*d.get(pos)?, *d.get(pos + 1)?]) as usize,
    };
    let flags = *d.get(pos + lenlen)?;
    let start = pos + lenlen + 1;
    if flags & 1 != 0 {
        let units: Vec<u16> = d
            .get(start..start + 2 * n)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes(*b))
            .collect();
        Some(String::from_utf16_lossy(&units))
    } else {
        Some(
            d.get(start..start + n)?
                .iter()
                .map(|&b| b as char)
                .collect(),
        )
    }
}

/// Cells of FORMULA records with the BIFF3/BIFF4 opcodes (0x0206, 0x0406)
/// in an Excel 97 workbook, as (sheet, row, column, value). Some writers
/// (JExcelApi) emit them; xlrd reads them, calamine skips them.
pub fn biff8_old_formulas(data: &[u8]) -> Vec<(String, usize, usize, Cell)> {
    let Ok(mut ole) = cfb::CompoundFile::open(std::io::Cursor::new(data)) else {
        return Vec::new();
    };
    let mut stream = Vec::new();
    let read = ["/Workbook", "/Book"].iter().any(|name| {
        ole.open_stream(name)
            .and_then(|mut s| std::io::Read::read_to_end(&mut s, &mut stream))
            .is_ok()
    });
    if !read {
        return Vec::new();
    }
    let mut sheets: Vec<(String, usize)> = Vec::new();
    let mut formats: Vec<(u16, CellType)> = Vec::new();
    let mut xf_keys: Vec<u16> = Vec::new();
    let mut date1904 = false;
    let mut records = Records {
        data: &stream,
        pos: 0,
    };
    records.next(); // BOF
    for (rc, d) in records.by_ref() {
        match rc {
            EOF => break,
            BOUNDSHEET if d.len() >= 6 && d[5] == 0 => {
                let offset = u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize;
                if let Some(name) = unicode(d, 6, 1) {
                    sheets.push((name, offset));
                }
            }
            FORMAT4 if d.len() >= 2 => {
                let key = u16::from_le_bytes([d[0], d[1]]);
                if let Some(s) = unicode(d, 2, 2) {
                    let ty = if is_date_format(&s) {
                        CellType::Date
                    } else {
                        CellType::Number
                    };
                    formats.push((key, ty));
                }
            }
            XF8 if d.len() >= 4 => xf_keys.push(u16::from_le_bytes([d[2], d[3]])),
            DATEMODE if d.len() >= 2 => date1904 = d[0] == 1,
            _ => {}
        }
    }
    // xf_epilogue: an XF whose format is unknown makes its cells text
    let xf_type = |xf: u16| {
        let key = *xf_keys.get(xf as usize)?;
        formats
            .iter()
            .rev()
            .find(|(k, _)| *k == key)
            .map(|(_, t)| *t)
            .or_else(|| builtin_type(key))
            .or(Some(CellType::Text))
    };
    let mut out = Vec::new();
    for (name, offset) in sheets {
        let mut records = Records {
            data: &stream,
            pos: offset,
        };
        records.next(); // BOF
        while let Some((rc, d)) = records.next() {
            if rc == EOF {
                break;
            }
            if !matches!(rc, FORMULA3 | FORMULA4) || d.len() < 14 {
                continue;
            }
            let r = u16::from_le_bytes([d[0], d[1]]) as usize;
            let c = u16::from_le_bytes([d[2], d[3]]) as usize;
            let xf = u16::from_le_bytes([d[4], d[5]]);
            let result = &d[6..14];
            let value = if result[6..8] == [0xff, 0xff] {
                match result[0] {
                    0 => match records.next() {
                        Some((STRING, s)) => Cell::Str(unicode(s, 0, 2).unwrap_or_default()),
                        _ => continue,
                    },
                    1 => Cell::Bool(result[2] != 0),
                    2 => Cell::NaN,
                    3 => Cell::Str(String::new()),
                    _ => continue,
                }
            } else {
                let v = f64::from_le_bytes(result.try_into().unwrap());
                match xf_type(xf) {
                    Some(CellType::Date) => xldate(v, date1904),
                    Some(CellType::Text) => Cell::Float(v),
                    _ if v.is_finite() && v.fract() == 0.0 => super::int_cell(v),
                    _ => Cell::Float(v),
                }
            };
            out.push((name.clone(), r, c, value));
        }
    }
    out
}

const LABELSST: u16 = 0x00fd;
const RSTRING: u16 = 0x00d6;
const FORMULA8: u16 = 0x0006;
const MULRK: u16 = 0x00bd;
const MULBLANK: u16 = 0x00be;

/// A cell record in an Excel 97 workbook with a column the format cannot
/// have (past 255), as (row, column). calamine lays every sheet out as one
/// dense array from its smallest to its largest cell position while
/// opening the workbook, so a damaged record with a wild column makes it
/// allocate tens of gigabytes; this scan, with calamine's record framing,
/// rejects such a file first.
pub fn biff8_cell_out_of_range(data: &[u8]) -> Option<(usize, usize)> {
    let ole = crate::ole::Ole::open(data).ok()?;
    let stream = ole
        .read_stream("Workbook")
        .or_else(|| ole.read_stream("Book"))?;
    let mut sheets = Vec::new();
    let mut records = Records {
        data: &stream,
        pos: 0,
    };
    records.next(); // BOF
    for (rc, d) in records.by_ref() {
        match rc {
            EOF => break,
            BOUNDSHEET if d.len() >= 4 => {
                sheets.push(u32::from_le_bytes([d[0], d[1], d[2], d[3]]) as usize)
            }
            _ => {}
        }
    }
    for offset in sheets {
        let mut records = Records {
            data: &stream,
            pos: offset,
        };
        records.next(); // BOF
        for (rc, d) in records {
            if rc == EOF {
                break;
            }
            let at = |i: usize| {
                d.get(i..i + 2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
            };
            let (row, last_col) = match rc {
                NUMBER | LABEL | BOOLERR | RK | LABELSST | BLANK | RSTRING | FORMULA8
                | FORMULA3 | FORMULA4 => match (at(0), at(2)) {
                    (Some(r), Some(c)) => (r, c),
                    _ => continue,
                },
                // row, first column, ..., last column
                MULRK | MULBLANK if d.len() >= 6 => match (at(0), at(d.len() - 2)) {
                    (Some(r), Some(c)) => (r, c),
                    _ => continue,
                },
                _ => continue,
            };
            if last_col > 255 {
                return Some((row, last_col));
            }
        }
    }
    None
}
