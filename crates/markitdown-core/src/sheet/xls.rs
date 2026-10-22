//! Legacy XLS (BIFF) workbooks: cells read with calamine, mapped the way
//! pandas' xlrd reader does (`_parse_cell`).

use super::{Cell, DateTime, Time};
use crate::Error;
use calamine::{Data, ExcelDateTime, ExcelDateTimeType, Reader, SheetType, Xls};

pub fn read(data: &[u8]) -> Result<Vec<super::Sheet>, Error> {
    let mut wb: Xls<_> = Xls::new(std::io::Cursor::new(data))
        .map_err(|e| Error::Conversion(format!("cannot read XLS: {e}")))?;
    let mut sheets = Vec::new();
    // xlrd keeps worksheets only
    let names: Vec<String> = wb
        .sheets_metadata()
        .iter()
        .filter(|s| s.typ == SheetType::WorkSheet)
        .map(|s| s.name.clone())
        .collect();
    for name in names {
        let range = wb
            .worksheet_range(&name)
            .map_err(|e| Error::Conversion(format!("cannot read sheet {name}: {e}")))?;
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        if let (Some((r0, c0)), Some((r1, c1))) = (range.start(), range.end()) {
            // xlrd rows start at A1 and are padded to the used width
            let (height, width) = (r1 as usize + 1, c1 as usize + 1);
            rows = vec![vec![Cell::Empty; width]; height];
            for (r, row) in range.rows().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    rows[r + r0 as usize][c + c0 as usize] = cell(v);
                }
            }
        }
        sheets.push(super::Sheet { name, rows });
    }
    Ok(sheets)
}

fn cell(v: &Data) -> Cell {
    match v {
        Data::Empty => Cell::Empty,
        Data::String(s) => Cell::Str(s.clone()),
        Data::Bool(b) => Cell::Bool(*b),
        Data::Error(_) => Cell::NaN,
        Data::Int(i) => Cell::Int(*i as i128),
        Data::Float(f) => {
            if f.is_finite() && f.fract() == 0.0 {
                super::int_cell(*f)
            } else {
                Cell::Float(*f)
            }
        }
        Data::DateTime(dt) => {
            // calamine keeps the workbook's date system private; a 1900-based
            // copy converts identically only if the workbook uses 1900
            let as_1900 = ExcelDateTime::new(dt.as_f64(), ExcelDateTimeType::DateTime, false);
            xldate(
                dt.as_f64(),
                as_1900.to_ymd_hms_milli() != dt.to_ymd_hms_milli(),
            )
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::Str(s.clone()),
    }
}

/// xlrd's `xldate_as_datetime`, then pandas turns dates on the epoch into
/// times.
fn xldate(v: f64, date1904: bool) -> Cell {
    let epoch = if date1904 {
        super::days_from_civil(1904, 1, 1)
    } else if v < 60.0 {
        super::days_from_civil(1899, 12, 31)
    } else {
        super::days_from_civil(1899, 12, 30)
    };
    let days = v.trunc() as i64;
    let ms = ((v - v.trunc()) * 86_400_000.0).round_ties_even() as i64;
    let Some(dt) = DateTime::from_epoch_micros(epoch + days, ms * 1000) else {
        return Cell::Float(v);
    };
    let on_epoch = if date1904 {
        (dt.year, dt.month, dt.day) == (1904, 1, 1)
    } else {
        (dt.year, dt.month, dt.day) == (1899, 12, 31)
    };
    if on_epoch {
        Cell::Time(Time { ..dt.time })
    } else {
        Cell::DateTime(dt)
    }
}
