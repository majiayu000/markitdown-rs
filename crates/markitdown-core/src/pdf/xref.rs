//! pdfminer's xref handling: the trailer chain of xref tables and xref
//! streams, and the line-scanning fallback for damaged files.

use std::collections::HashMap;
use std::rc::Rc;

use super::document::{Document, FileKw, MAX_DEPTH, usize_pos};
use super::lexer::{Stack, is_space};
use super::object::{Dict, Obj, PdfError, Result, no_xref, nunpack, other, py_int, syntax};

type Pos = (Option<i64>, i64, i64);

pub(super) enum XRef {
    /// PDFXRef and PDFXRefFallback: offsets in insertion order.
    Table {
        order: Vec<i64>,
        offsets: HashMap<i64, Pos>,
        trailer: Dict,
    },
    Stream {
        data: Rc<[u8]>,
        w: [usize; 3],
        ranges: Vec<(i64, i64)>,
        trailer: Dict,
    },
}

impl XRef {
    pub(super) fn trailer(&self) -> &Dict {
        match self {
            XRef::Table { trailer, .. } | XRef::Stream { trailer, .. } => trailer,
        }
    }

    fn entry(data: &[u8], w: [usize; 3], index: i64) -> (u64, u64, u64) {
        let entlen = (w[0] + w[1] + w[2]) as i64;
        let start = (entlen * index).clamp(0, data.len() as i64) as usize;
        let ent = &data[start..(start + entlen as usize).min(data.len())];
        let part = |a: usize, b: usize| &ent[a.min(ent.len())..b.min(ent.len())];
        (
            nunpack(part(0, w[0]), 1),
            nunpack(part(w[0], w[0] + w[1]), 0),
            nunpack(part(w[0] + w[1], ent.len()), 0),
        )
    }

    pub(super) fn get_pos(&self, objid: i64) -> Option<Pos> {
        match self {
            XRef::Table { offsets, .. } => offsets.get(&objid).copied(),
            XRef::Stream {
                data, w, ranges, ..
            } => {
                let mut index = 0;
                let mut found = false;
                for &(start, n) in ranges {
                    if start <= objid && objid < start + n {
                        index += objid - start;
                        found = true;
                        break;
                    }
                    index += n;
                }
                if !found {
                    return None;
                }
                match Self::entry(data, *w, index) {
                    (1, f2, f3) => Some((None, f2 as i64, f3 as i64)),
                    (2, f2, f3) => Some((Some(f2 as i64), f3 as i64, 0)),
                    _ => None,
                }
            }
        }
    }

    pub(super) fn objids(&self) -> Vec<i64> {
        match self {
            XRef::Table { order, .. } => order.clone(),
            XRef::Stream {
                data, w, ranges, ..
            } => {
                let mut out = Vec::new();
                for &(start, n) in ranges {
                    for i in 0..n.max(0) {
                        // pdfminer restarts the entry offset for every range
                        if matches!(Self::entry(data, *w, i).0, 1 | 2) {
                            out.push(start + i);
                        }
                    }
                }
                out
            }
        }
    }
}

fn table_insert(order: &mut Vec<i64>, offsets: &mut HashMap<i64, Pos>, id: i64, pos: Pos) {
    if offsets.insert(id, pos).is_none() {
        order.push(id);
    }
}

fn strip(b: &[u8]) -> &[u8] {
    let s = b.iter().position(|&c| !is_space(c)).unwrap_or(b.len());
    let e = b.iter().rposition(|&c| !is_space(c)).map_or(s, |e| e + 1);
    &b[s..e.max(s)]
}

/// `PDFOBJ_CUE.match(line.decode('latin-1'))`: `^(\d+)\s+(\d+)\s+obj\b`
fn obj_cue(line: &[u8]) -> Option<(i64, i64)> {
    let ws = |c: u8| matches!(c, 0x09..=0x0d | 0x1c..=0x20 | 0x85 | 0xa0);
    let word = |c: u8| {
        c.is_ascii_alphanumeric()
            || c == b'_'
            || matches!(c, 0xaa | 0xb2 | 0xb3 | 0xb5 | 0xb9 | 0xba | 0xbc..=0xbe)
            || (0xc0..=0xff).contains(&c) && c != 0xd7 && c != 0xf7
    };
    let mut i = 0;
    let digits = |i: &mut usize| {
        let s = *i;
        while *i < line.len() && line[*i].is_ascii_digit() {
            *i += 1;
        }
        (*i > s)
            .then(|| std::str::from_utf8(&line[s..*i]).ok()?.parse::<i64>().ok())
            .flatten()
    };
    let spaces = |i: &mut usize| {
        let s = *i;
        while *i < line.len() && ws(line[*i]) {
            *i += 1;
        }
        *i > s
    };
    let objid = digits(&mut i)?;
    if !spaces(&mut i) {
        return None;
    }
    let genno = digits(&mut i)?;
    if !spaces(&mut i) || !line[i..].starts_with(b"obj") {
        return None;
    }
    let after = line.get(i + 3).copied();
    if after.is_some_and(word) {
        return None;
    }
    Some((objid, genno))
}

impl Document {
    /// The xref position after the last `startxref`, read backwards in
    /// 4096-byte chunks as `revreadlines` does.
    pub(super) fn find_xref(&self) -> Result<usize> {
        let data = &self.data[..];
        let mut prev: Vec<u8> = Vec::new();
        let mut pos = data.len();
        let mut buf: Vec<u8> = Vec::new();
        while pos > 0 {
            let prevpos = pos;
            pos = pos.saturating_sub(super::lexer::BUFSIZ);
            let mut s = data[pos..prevpos].to_vec();
            loop {
                let n = s.iter().rposition(|&c| c == b'\r' || c == b'\n');
                let Some(n) = n else {
                    s.extend_from_slice(&buf);
                    buf = s;
                    break;
                };
                let mut line = s[n..].to_vec();
                line.extend_from_slice(&buf);
                s.truncate(n);
                buf.clear();
                let line = strip(&line);
                if line == b"startxref" {
                    if prev.is_empty() || !prev.iter().all(u8::is_ascii_digit) {
                        return no_xref("Invalid xref position, no digit");
                    }
                    let start: u64 = std::str::from_utf8(&prev)
                        .unwrap()
                        .parse()
                        .unwrap_or(u64::MAX);
                    if start >= 1 << 31 {
                        return no_xref("Invalid xref position, too large");
                    }
                    return Ok(start as usize);
                }
                if !line.is_empty() {
                    prev = line.to_vec();
                }
            }
        }
        no_xref("Unexpected EOF")
    }

    pub(super) fn read_xref_from(&self, start: usize, depth: u32) -> Result<()> {
        if depth > MAX_DEPTH {
            return other("maximum recursion depth exceeded");
        }
        let mut st = self.parser();
        st.seek(start)?;
        let (pos, token) = match st.lex.nexttoken() {
            Err(PdfError::Eof) => return no_xref("Unexpected EOF"),
            r => r?,
        };
        let xref = if token.as_int().is_some() {
            st.seek(pos)?;
            self.load_xref_stream(&mut st)?
        } else {
            if matches!(&token, Obj::Kw(k) if &**k == b"xref") {
                st.lex.nextline()?;
            }
            self.load_xref_table(&mut st)?
        };
        let trailer = xref.trailer().clone();
        self.xrefs.borrow_mut().push(xref);
        if let Some(p) = trailer.get("XRefStm") {
            self.read_xref_from(usize_pos(self.int_value(p)?)?, depth + 1)?;
        }
        if let Some(p) = trailer.get("Prev") {
            self.read_xref_from(usize_pos(self.int_value(p)?)?, depth + 1)?;
        }
        Ok(())
    }

    pub(super) fn load_xref_table(&self, st: &mut Stack) -> Result<XRef> {
        let (mut order, mut offsets) = (Vec::new(), HashMap::new());
        loop {
            let (pos, line) = match st.lex.nextline() {
                Err(PdfError::Eof) => return no_xref("Unexpected EOF - file corrupted?"),
                r => r?,
            };
            let line = strip(&line);
            if line.is_empty() {
                continue;
            }
            if line.starts_with(b"trailer") {
                st.seek(pos)?;
                break;
            }
            let f: Vec<&[u8]> = line.split(|&b| b == b' ').collect();
            if f.len() != 2 {
                return no_xref("Trailer not found");
            }
            let parse = |b: &[u8]| py_int(&String::from_utf8_lossy(b));
            let (Some(start), Some(nobjs)) = (parse(f[0]), parse(f[1])) else {
                return no_xref("Invalid line");
            };
            for objid in start..start.saturating_add(nobjs) {
                let line = match st.lex.nextline() {
                    Err(PdfError::Eof) => return no_xref("Unexpected EOF - file corrupted?"),
                    r => r?.1,
                };
                let f: Vec<&[u8]> = strip(&line).split(|&b| b == b' ').collect();
                if f.len() != 3 {
                    return no_xref("Invalid XRef format");
                }
                if f[2] != b"n" {
                    continue;
                }
                if let (Some(p), Some(g)) = (parse(f[0]), parse(f[1])) {
                    table_insert(&mut order, &mut offsets, objid, (None, p, g));
                }
            }
        }
        let mut trailer = Dict::default();
        self.load_trailer(st, &mut trailer)?;
        Ok(XRef::Table {
            order,
            offsets,
            trailer,
        })
    }

    pub(super) fn load_trailer(&self, st: &mut Stack, trailer: &mut Dict) -> Result<()> {
        let parsed = (|| {
            let (_, kwd) = st.lex.nexttoken()?;
            if !matches!(&kwd, Obj::Kw(k) if &**k == b"trailer") {
                return other(format!("AssertionError: {kwd:?}"));
            }
            Ok(st.nextobject(&mut FileKw(self))?.1)
        })();
        let dic = match parsed {
            Ok(d) => d,
            Err(PdfError::Eof) => match st.pop(1).pop() {
                Some((_, d)) => d,
                None => return no_xref("Unexpected EOF - file corrupted"),
            },
            Err(e) => return Err(e),
        };
        for (k, v) in &self.dict_value(&dic)?.0 {
            trailer.set(k.clone(), v.clone());
        }
        Ok(())
    }

    pub(super) fn load_xref_stream(&self, st: &mut Stack) -> Result<XRef> {
        for _ in 0..3 {
            st.lex.nexttoken()?;
        }
        let (_, stream) = st.nextobject(&mut FileKw(self))?;
        let Obj::Stream(stream) = stream else {
            return no_xref("Invalid PDF stream spec.");
        };
        if !stream.attrs.get("Type").is_some_and(|t| t.is_name("XRef")) {
            return no_xref("Invalid PDF stream spec.");
        }
        let key = |k: &str| {
            stream.attrs.get(k).cloned().ok_or_else(|| {
                PdfError::Other(format!("PDF stream object does not have attribute '{k}'"))
            })
        };
        let size = key("Size")?;
        let index = match stream.attrs.get("Index") {
            Some(Obj::Array(a)) => a.to_vec(),
            Some(_) => return other("object of type has no len()"),
            None => vec![Obj::Int(0), size],
        };
        if index.len() % 2 != 0 {
            return syntax("Invalid index number");
        }
        let num = |o: &Obj| {
            o.as_int()
                .ok_or_else(|| PdfError::Other("TypeError in xref index".into()))
        };
        let ranges = index
            .chunks(2)
            .map(|p| Ok((num(&p[0])?, num(&p[1])?)))
            .collect::<Result<Vec<_>>>()?;
        let Obj::Array(w) = key("W")? else {
            return other("cannot unpack /W");
        };
        if w.len() != 3 {
            return other("cannot unpack /W");
        }
        let mut ws = [0usize; 3];
        for (slot, v) in ws.iter_mut().zip(w.iter()) {
            *slot = num(v)?.max(0) as usize;
        }
        let data = self.stream_data(&stream)?;
        Ok(XRef::Stream {
            data,
            w: ws,
            ranges,
            trailer: stream.attrs.clone(),
        })
    }

    pub(super) fn load_fallback(&self) -> Result<XRef> {
        let (mut order, mut offsets) = (Vec::new(), HashMap::new());
        let mut trailer = Dict::default();
        let mut st = self.parser();
        st.seek(0)?;
        loop {
            let (pos, line) = match st.lex.nextline() {
                Err(PdfError::Eof) => break,
                r => r?,
            };
            if line.starts_with(b"trailer") {
                st.seek(pos)?;
                self.load_trailer(&mut st, &mut trailer)?;
                break;
            }
            let Some((objid, genno)) = obj_cue(&line) else {
                continue;
            };
            table_insert(&mut order, &mut offsets, objid, (None, pos as i64, genno));
            st.seek(pos)?;
            let (_, obj) = st.nextobject(&mut FileKw(self))?;
            if let Obj::Stream(s) = &obj
                && s.attrs.get("Type").is_some_and(|t| t.is_name("ObjStm"))
            {
                let n = s.attrs.get("N").and_then(Obj::as_int).unwrap_or(0);
                let objs = self.objstm_objects(s)?;
                let n = n.min(objs.len() as i64 / 2);
                for index in 0..n.max(0) {
                    if let Some(id) = objs[(index * 2) as usize].as_int() {
                        table_insert(&mut order, &mut offsets, id, (Some(objid), index, 0));
                    }
                }
            }
        }
        Ok(XRef::Table {
            order,
            offsets,
            trailer,
        })
    }
}
