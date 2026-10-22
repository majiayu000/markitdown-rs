//! pdfminer's stream filters and predictors, including where they raise.

use flate2::{Decompress, FlushDecompress, Status};

use super::lexer::is_space;
use super::object::{Result, other};

/// FlateDecode as PDFStream.decode does it: `zlib.decompress`, else
/// `decompress_corrupted`, else nothing.
pub fn flate(data: &[u8]) -> Vec<u8> {
    inflate(data)
        .or_else(|| decompress_corrupted(data))
        .unwrap_or_default()
}

/// `zlib.decompress()`: the whole stream must be there and check out.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut d = Decompress::new(true);
    let mut out = Vec::with_capacity(data.len().saturating_mul(4).max(1024));
    loop {
        if out.capacity() - out.len() < 4096 {
            out.reserve(out.capacity());
        }
        let (tin, tout) = (d.total_in(), d.total_out());
        let st = d
            .decompress_vec(&data[tin as usize..], &mut out, FlushDecompress::None)
            .ok()?;
        if st == Status::StreamEnd {
            return Some(out);
        }
        if d.total_in() == tin && d.total_out() == tout {
            return None;
        }
    }
}

/// Feed the stream a byte at a time and keep what decodes; an error well
/// before the end (not just in the checksum) loses everything.
fn decompress_corrupted(data: &[u8]) -> Option<Vec<u8>> {
    let mut d = Decompress::new(true);
    let mut out = Vec::new();
    for (i, b) in data.iter().enumerate() {
        let mark = out.len();
        let start = d.total_in();
        loop {
            if out.capacity() - out.len() < 4096 {
                out.reserve(out.capacity().max(8192));
            }
            let used = (d.total_in() - start) as usize;
            match d.decompress_vec(&[*b][used.min(1)..], &mut out, FlushDecompress::None) {
                Err(_) => {
                    out.truncate(mark);
                    return if (i as i64) < data.len() as i64 - 3 {
                        None
                    } else {
                        Some(out)
                    };
                }
                Ok(Status::StreamEnd) => return Some(out),
                Ok(_) => {
                    if d.total_in() - start >= 1 && out.len() < out.capacity() {
                        break;
                    }
                }
            }
        }
    }
    Some(out)
}

/// LZWDecode; a bad first code raises IndexError, as in pdfminer.
pub fn lzw(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut table: Vec<Option<Vec<u8>>> = Vec::new();
    let mut prevbuf: Option<Vec<u8>> = None;
    let (mut nbits, mut buff, mut bpos, mut pos) = (9u32, 0u32, 8u32, 0usize);
    'outer: loop {
        // readbits
        let mut bits = nbits;
        let mut v: u32 = 0;
        loop {
            let r = 8 - bpos;
            if bits <= r {
                v = (v << bits) | ((buff >> (r - bits)) & ((1 << bits) - 1));
                bpos += bits;
                break;
            }
            v = (v << r) | (buff & ((1 << r) - 1));
            bits -= r;
            let Some(&x) = data.get(pos) else {
                break 'outer;
            };
            pos += 1;
            buff = x as u32;
            bpos = 0;
        }
        let code = v as usize;
        if code == 256 {
            table = (0..=255u8).map(|c| Some(vec![c])).collect();
            table.push(None);
            table.push(None);
            prevbuf = Some(Vec::new());
            nbits = 9;
        } else if code == 257 {
        } else if prevbuf.as_ref().is_none_or(|p| p.is_empty()) {
            let Some(entry) = table.get(code) else {
                return other("list index out of range");
            };
            let Some(x) = entry.clone() else {
                return other("LZW table entry is empty");
            };
            out.extend_from_slice(&x);
            prevbuf = Some(x);
        } else {
            let prev = prevbuf.take().unwrap();
            let x = if code < table.len() {
                let Some(x) = table[code].clone() else {
                    return other("LZW table entry is empty");
                };
                let mut e = prev.clone();
                e.extend_from_slice(&x[..x.len().min(1)]);
                table.push(Some(e));
                x
            } else if code == table.len() {
                let mut e = prev.clone();
                e.extend_from_slice(&prev[..prev.len().min(1)]);
                table.push(Some(e.clone()));
                e
            } else {
                // CorruptDataError ends the stream
                break;
            };
            nbits = match table.len() {
                511 => 10,
                1023 => 11,
                2047 => 12,
                _ => nbits,
            };
            out.extend_from_slice(&x);
            prevbuf = Some(x);
        }
    }
    Ok(out)
}

fn is_re_space(c: u8) -> bool {
    is_space(c)
}

/// ASCII85Decode: strip `<~`/`~>` loosely, then `base64.a85decode`.
pub fn ascii85(data: &[u8]) -> Result<Vec<u8>> {
    // ^\s*<?\s*~\s*
    let mut s = 0;
    let skip = |mut i: usize| {
        while i < data.len() && is_re_space(data[i]) {
            i += 1;
        }
        i
    };
    let mut i = skip(0);
    if data.get(i) == Some(&b'<') {
        i = skip(i + 1);
    }
    if data.get(i) == Some(&b'~') {
        s = skip(i + 1);
    }
    let data = &data[s..];
    // \s*~\s*>?\s*$ at the leftmost '~' it can match from
    let mut e = data.len();
    for (p, &c) in data.iter().enumerate() {
        if c != b'~' {
            continue;
        }
        let mut j = p + 1;
        while j < data.len() && is_re_space(data[j]) {
            j += 1;
        }
        if data.get(j) == Some(&b'>') {
            j += 1;
        }
        while j < data.len() && is_re_space(data[j]) {
            j += 1;
        }
        if j == data.len() {
            let mut q = p;
            while q > 0 && is_re_space(data[q - 1]) {
                q -= 1;
            }
            e = q;
            break;
        }
    }
    let data = &data[..e];
    let mut out = Vec::new();
    let mut curr: Vec<u8> = Vec::new();
    for &x in data.iter().chain(b"uuuu") {
        if (b'!'..=b'u').contains(&x) {
            curr.push(x);
            if curr.len() == 5 {
                let acc = curr.iter().fold(0u64, |a, &c| 85 * a + (c - 33) as u64);
                if acc > u32::MAX as u64 {
                    return other("Ascii85 overflow");
                }
                out.extend_from_slice(&(acc as u32).to_be_bytes());
                curr.clear();
            }
        } else if x == b'z' {
            if !curr.is_empty() {
                return other("z inside Ascii85 5-tuple");
            }
            out.extend_from_slice(&[0; 4]);
        } else if matches!(x, b' ' | b'\t' | b'\n' | b'\r' | 0x0b) {
            continue;
        } else {
            return other("Non-Ascii85 digit found");
        }
    }
    let padding = 4 - curr.len();
    out.truncate(out.len().saturating_sub(padding));
    Ok(out)
}

/// ASCIIHexDecode (`binascii.unhexlify` raises on odd or bad digits).
pub fn ascii_hex(data: &[u8]) -> Result<Vec<u8>> {
    let mut d: Vec<u8> = data.iter().copied().filter(|&c| !is_re_space(c)).collect();
    if let Some(idx) = d.iter().position(|&c| c == b'>') {
        d.truncate(idx);
        if idx % 2 == 1 {
            d.push(b'0');
        }
    }
    if !d.len().is_multiple_of(2) {
        return other("Odd-length string");
    }
    d.chunks(2)
        .map(|p| {
            std::str::from_utf8(p)
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| super::object::PdfError::Other("Non-hexadecimal digit found".into()))
        })
        .collect()
}

/// RunLengthDecode; running out of data mid-run raises.
pub fn run_length(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut it = data.iter().copied();
    loop {
        let length = it.next().unwrap_or(128);
        if length == 128 {
            break;
        }
        if length < 128 {
            for _ in 0..=length {
                let Some(b) = it.next() else {
                    return other("generator raised StopIteration");
                };
                out.push(b);
            }
        } else {
            let Some(b) = it.next() else {
                return other("StopIteration");
            };
            out.extend(std::iter::repeat_n(b, 257 - length as usize));
        }
    }
    Ok(out)
}

pub fn tiff_predictor(colors: i64, columns: i64, bpc: i64, data: &[u8]) -> Result<Vec<u8>> {
    if bpc != 8 {
        return other(format!("Unsupported `bitspercomponent': {bpc}"));
    }
    let bpp = colors * (bpc / 8);
    let nbytes = columns * bpp;
    if nbytes == 0 {
        return other("range() arg 3 must not be zero");
    }
    if nbytes < 0 {
        return Ok(Vec::new());
    }
    let (bpp, nbytes) = (bpp as usize, nbytes as usize);
    let mut buf = Vec::with_capacity(data.len());
    for start in (0..data.len()).step_by(nbytes) {
        let mut raw: Vec<u8> = Vec::with_capacity(nbytes);
        for i in 0..nbytes {
            let Some(&v) = data.get(start + i) else {
                return other("index out of range");
            };
            let v = if i >= bpp {
                v.wrapping_add(raw[i - bpp])
            } else {
                v
            };
            raw.push(v);
        }
        buf.extend_from_slice(&raw);
    }
    Ok(buf)
}

pub fn png_predictor(colors: i64, columns: i64, bpc: i64, data: &[u8]) -> Result<Vec<u8>> {
    if bpc != 8 && bpc != 1 {
        return other(format!("Unsupported `bitspercomponent': {bpc}"));
    }
    let nbytes = (colors * columns * bpc).div_euclid(8);
    let bpp = (colors * bpc).div_euclid(8);
    let step = nbytes + 1;
    if step <= 0 {
        return if step == 0 {
            other("range() arg 3 must not be zero")
        } else {
            Ok(Vec::new())
        };
    }
    let mut buf = Vec::new();
    let mut line_above: Vec<u8> = vec![0; columns.max(0) as usize];
    let idx = |v: &Vec<u8>, i: i64| -> Result<u8> {
        if i < 0 {
            // Python's negative index counts from the end
            let j = v.len() as i64 + i;
            if j < 0 {
                return other("list index out of range");
            }
            return Ok(v[j as usize]);
        }
        v.get(i as usize)
            .copied()
            .ok_or_else(|| super::object::PdfError::Other("list index out of range".into()))
    };
    for start in (0..data.len()).step_by(step as usize) {
        let filter_type = data[start];
        let end = (start + 1)
            .saturating_add(nbytes.max(0) as usize)
            .min(data.len());
        let line = &data[start + 1..end];
        let mut raw: Vec<u8> = Vec::with_capacity(line.len());
        match filter_type {
            0 => raw.extend_from_slice(line),
            1 => {
                for (j, &x) in line.iter().enumerate() {
                    let j = j as i64;
                    let left = if j - bpp < 0 { 0 } else { idx(&raw, j - bpp)? };
                    raw.push(x.wrapping_add(left));
                }
            }
            2 => {
                for (&x, &up) in line.iter().zip(line_above.iter()) {
                    raw.push(x.wrapping_add(up));
                }
            }
            3 => {
                for (j, &x) in line.iter().enumerate() {
                    let j = j as i64;
                    let left = if j - bpp < 0 { 0 } else { idx(&raw, j - bpp)? } as u32;
                    let up = idx(&line_above, j)? as u32;
                    raw.push(x.wrapping_add(((left + up) / 2) as u8));
                }
            }
            4 => {
                for (j, &x) in line.iter().enumerate() {
                    let j = j as i64;
                    let (left, upper_left) = if j - bpp < 0 {
                        (0, 0)
                    } else {
                        (idx(&raw, j - bpp)?, idx(&line_above, j - bpp)?)
                    };
                    let up = idx(&line_above, j)?;
                    raw.push(
                        x.wrapping_add(paeth(left as i32, up as i32, upper_left as i32) as u8),
                    );
                }
            }
            t => return other(format!("Unsupported predictor value: {t}")),
        }
        buf.extend_from_slice(&raw);
        line_above = raw;
    }
    Ok(buf)
}

fn paeth(left: i32, above: i32, upper_left: i32) -> i32 {
    let p = left + above - upper_left;
    let (pa, pb, pc) = ((p - left).abs(), (p - above).abs(), (p - upper_left).abs());
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        above
    } else {
        upper_left
    }
}
