//! XLSX and XLS workbooks, as markitdown converts them: every sheet is read
//! into a pandas DataFrame (`pd.read_excel`) and written with
//! `DataFrame.to_html(index=False)`, which then goes through the HTML
//! converter.

mod frame;
mod xls;
mod xlsx;

use crate::Error;

/// A cell value as pandas' Excel readers hand it to the parser.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// empty cell, reported as ""
    Empty,
    Int(i128),
    /// an integral value beyond i128, as Python prints the int: every digit
    BigInt(String),
    Float(f64),
    Str(String),
    Bool(bool),
    DateTime(DateTime),
    Time(Time),
    /// timedelta in microseconds
    Duration(i64),
    /// error cells (#N/A, #DIV/0!, ...) become NaN
    NaN,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DateTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub time: Time,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Time {
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub micro: u32,
}

impl Time {
    fn from_micros(us: i64) -> Time {
        let s = us / 1_000_000;
        Time {
            hour: (s / 3600) as u32,
            minute: (s / 60 % 60) as u32,
            second: (s % 60) as u32,
            micro: (us % 1_000_000) as u32,
        }
    }

    /// `str(datetime.time)`
    pub fn py_str(&self) -> String {
        let base = format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second);
        if self.micro == 0 {
            base
        } else {
            format!("{base}.{:06}", self.micro)
        }
    }
}

/// Days since 1970-01-01 to a civil date (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl DateTime {
    /// `epoch + timedelta(microseconds=us)`; None outside Python's year range.
    fn from_epoch_micros(epoch_days: i64, us: i64) -> Option<DateTime> {
        let days = epoch_days + us.div_euclid(86_400_000_000);
        let (year, month, day) = civil_from_days(days);
        if !(1..=9999).contains(&year) {
            return None;
        }
        Some(DateTime {
            year,
            month,
            day,
            time: Time::from_micros(us.rem_euclid(86_400_000_000)),
        })
    }

    pub fn is_midnight(&self) -> bool {
        self.time
            == Time {
                hour: 0,
                minute: 0,
                second: 0,
                micro: 0,
            }
    }

    /// `str(datetime.datetime)`
    pub fn py_str(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {}",
            self.year,
            self.month,
            self.day,
            self.time.py_str()
        )
    }
}

/// 1899-12-30 and 1904-01-01 as days since 1970-01-01.
fn epoch_days(date1904: bool) -> i64 {
    if date1904 {
        days_from_civil(1904, 1, 1)
    } else {
        days_from_civil(1899, 12, 30)
    }
}

/// `int(value)` of an integral float.
fn int_cell(f: f64) -> Cell {
    if f.abs() < 1e38 {
        Cell::Int(f as i128)
    } else {
        Cell::BigInt(format!("{f:.0}"))
    }
}

pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<Cell>>,
}

fn render(sheets: Vec<Sheet>, keep_data_uris: bool) -> String {
    let mut md = String::new();
    for sheet in sheets {
        md.push_str(&format!("## {}\n", sheet.name));
        let html = frame::to_html(&sheet.rows);
        md.push_str(crate::pystr::strip(&crate::html::convert_string(
            &html,
            keep_data_uris,
        )));
        md.push_str("\n\n");
    }
    crate::pystr::strip(&md).to_string()
}

pub fn convert_xlsx(data: &[u8], keep_data_uris: bool) -> Result<String, Error> {
    Ok(render(xlsx::read(data)?, keep_data_uris))
}

pub fn convert_xls(data: &[u8], keep_data_uris: bool) -> Result<String, Error> {
    Ok(render(xls::read(data)?, keep_data_uris))
}
