//! pdfminer's PDFParser, PDFDocument and PDFPage: xref tables and streams
//! (with the line-scanning fallback), object streams, decryption and the
//! page tree.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use super::crypt::Security;
use super::filters;
use super::lexer::{Keywords, Lexer, Stack};
use super::object::{Dict, Obj, PdfError, Result, Stream, literal_name, other, safe_int, syntax};
use super::xref::XRef;

/// Python's default recursion limit, roughly, for the recursive walks.
pub(super) const MAX_DEPTH: u32 = 900;

type ObjStm = Rc<(Vec<Obj>, Obj)>;

pub struct Document {
    pub(super) data: Rc<[u8]>,
    pub(super) xrefs: RefCell<Vec<XRef>>,
    cache: RefCell<HashMap<i64, Obj>>,
    /// parsed object streams and their /N, by stream id
    objstms: RefCell<HashMap<Option<i64>, ObjStm>>,
    security: RefCell<Option<Rc<Security>>>,
    fallback: Cell<bool>,
    depth: Cell<u32>,
    pub catalog: Rc<Dict>,
}

/// PDFParser's keyword handling.
pub(super) struct FileKw<'a>(pub(super) &'a Document);

impl Keywords for FileKw<'_> {
    fn do_keyword(&mut self, st: &mut Stack, pos: usize, kw: Rc<[u8]>) -> Result<()> {
        match &*kw {
            b"xref" | b"startxref" => {
                let x = st.pop(1);
                st.results.extend(x);
            }
            b"endobj" => {
                let x = st.pop(4);
                st.results.extend(x);
            }
            b"null" => st.push((pos, Obj::Null)),
            b"R" => {
                if st.curstack.len() >= 2 {
                    let v = st.pop(2);
                    if let Some(id) = safe_int(&v[0].1)? {
                        st.push((pos, Obj::Ref(id)));
                    }
                }
            }
            b"stream" => self.0.read_stream(st, pos)?,
            _ => st.push((pos, Obj::Kw(kw))),
        }
        Ok(())
    }
}

/// PDFStreamParser's keyword handling (object streams).
struct ObjStmKw;

impl Keywords for ObjStmKw {
    fn do_keyword(&mut self, st: &mut Stack, pos: usize, kw: Rc<[u8]>) -> Result<()> {
        match &*kw {
            b"R" => {
                if st.curstack.len() < 2 {
                    return other("not enough values to unpack");
                }
                let v = st.pop(2);
                if let Some(id) = safe_int(&v[0].1)? {
                    st.push((pos, Obj::Ref(id)));
                }
            }
            b"obj" | b"endobj" => {}
            _ => st.push((pos, Obj::Kw(kw))),
        }
        Ok(())
    }

    fn flushes(&self) -> bool {
        true
    }
}

fn num_eq(o: &Obj, v: i64) -> bool {
    o.as_num() == Some(v as f64)
}

pub(super) fn usize_pos(p: i64) -> Result<usize> {
    usize::try_from(p).or_else(|_| other("negative seek value"))
}

impl Document {
    pub fn new(data: &[u8]) -> Result<Self> {
        let mut doc = Document {
            data: Rc::from(data),
            xrefs: RefCell::new(Vec::new()),
            cache: RefCell::new(HashMap::new()),
            objstms: RefCell::new(HashMap::new()),
            security: RefCell::new(None),
            fallback: Cell::new(false),
            depth: Cell::new(0),
            catalog: Rc::new(Dict::default()),
        };
        let loaded = doc.find_xref().and_then(|pos| doc.read_xref_from(pos, 0));
        match loaded {
            Ok(()) => {}
            Err(PdfError::Syntax { no_xref: true, .. }) => {
                doc.fallback.set(true);
                let x = doc.load_fallback()?;
                doc.xrefs.borrow_mut().push(x);
            }
            Err(e) => return Err(e),
        }
        let n = doc.xrefs.borrow().len();
        let mut catalog = None;
        for i in 0..n {
            let trailer = doc.xrefs.borrow()[i].trailer().clone();
            if trailer.0.is_empty() {
                continue;
            }
            if let Some(enc) = trailer.get("Encrypt") {
                let id = match trailer.get("ID") {
                    Some(id) => doc.list_value(id)?.to_vec(),
                    None => vec![Obj::Str(Rc::from(&b""[..])), Obj::Str(Rc::from(&b""[..]))],
                };
                let param = doc.dict_value(enc)?;
                let sec = Security::new(&doc, &id, &param)?;
                *doc.security.borrow_mut() = Some(Rc::new(sec));
                doc.fallback.set(false);
            }
            if let Some(info) = trailer.get("Info") {
                doc.dict_value(info)?;
            }
            if let Some(root) = trailer.get("Root") {
                catalog = Some(doc.dict_value(root)?);
                break;
            }
        }
        doc.catalog = match catalog {
            Some(c) => c,
            None => return syntax("No /Root object! - Is this really a PDF?"),
        };
        Ok(doc)
    }

    pub(super) fn parser(&self) -> Stack {
        Stack::new(Lexer::new(self.data.clone()))
    }

    fn enter(&self) -> Result<DepthGuard<'_>> {
        let d = self.depth.get() + 1;
        if d > MAX_DEPTH {
            return other("maximum recursion depth exceeded");
        }
        self.depth.set(d);
        Ok(DepthGuard(&self.depth))
    }

    /// The `stream` keyword: the stream data runs /Length bytes (or, in
    /// fallback mode, up to `endstream`).
    fn read_stream(&self, st: &mut Stack, pos: usize) -> Result<()> {
        let popped = st.pop(1);
        let Some((_, dic)) = popped.into_iter().next() else {
            return syntax("Invalid stream dictionary");
        };
        let dic = self.dict_value(&dic)?;
        let fallback = self.fallback.get();
        let mut objlen: i64 = 0;
        if !fallback && let Some(l) = dic.get("Length") {
            objlen = self.int_value(l)?;
        }
        st.seek(pos)?;
        let line = match st.lex.nextline() {
            Err(PdfError::Eof) => return Ok(()),
            r => r?.1,
        };
        let pos = pos + line.len();
        let start = pos.min(self.data.len());
        let end = if objlen < 0 {
            self.data.len()
        } else {
            start.saturating_add(objlen as usize).min(self.data.len())
        };
        let mut data = self.data[start..end].to_vec();
        st.seek(usize_pos(pos as i64 + objlen)?)?;
        loop {
            let line = match st.lex.nextline() {
                Err(PdfError::Eof) => break,
                r => r?.1,
            };
            if let Some(i) = line.windows(9).position(|w| w == b"endstream") {
                objlen += i as i64;
                if fallback {
                    data.extend_from_slice(&line[..i]);
                }
                break;
            }
            objlen += line.len() as i64;
            if fallback {
                data.extend_from_slice(&line);
            }
        }
        st.seek(usize_pos(pos as i64 + objlen)?)?;
        let decipher = self.security.borrow().is_some();
        st.push((
            pos,
            Obj::Stream(Rc::new(Stream::new((*dic).clone(), data, decipher))),
        ));
        Ok(())
    }

    /// Each xref's object ids, for the page search without a page tree.
    pub(super) fn xref_objids(&self) -> Vec<Vec<i64>> {
        self.xrefs.borrow().iter().map(XRef::objids).collect()
    }

    pub fn getobj(&self, objid: i64) -> Result<Obj> {
        if self.xrefs.borrow().is_empty() {
            return other("PDFDocument is not initialized");
        }
        if let Some(o) = self.cache.borrow().get(&objid) {
            return Ok(o.clone());
        }
        let _guard = self.enter()?;
        let mut found = None;
        let n = self.xrefs.borrow().len();
        for i in 0..n {
            let Some((strmid, index, genno)) = self.xrefs.borrow()[i].get_pos(objid) else {
                continue;
            };
            let obj = match strmid {
                Some(sid) => {
                    let stream = self.stream_value(&self.getobj(sid)?)?;
                    self.objstm_get(&stream, index)
                }
                None => self.parse_at(index, objid).and_then(|o| {
                    if self.security.borrow().is_some() {
                        self.decipher_all(objid, genno, o)
                    } else {
                        Ok(o)
                    }
                }),
            };
            match obj {
                Ok(o) => {
                    if let Obj::Stream(s) = &o {
                        s.id.set(Some((objid, genno)));
                    }
                    found = Some(o);
                    break;
                }
                Err(PdfError::Eof | PdfError::Syntax { .. }) => continue,
                Err(e) => return Err(e),
            }
        }
        let obj = found.ok_or(PdfError::NotFound(objid))?;
        self.cache.borrow_mut().insert(objid, obj.clone());
        Ok(obj)
    }

    fn parse_at(&self, pos: i64, objid: i64) -> Result<Obj> {
        let mut st = self.parser();
        st.seek(usize_pos(pos)?)?;
        let (_, mut objid1) = st.lex.nexttoken()?;
        st.lex.nexttoken()?;
        let (_, mut kwd) = st.lex.nexttoken()?;
        let is_obj = |k: &Obj| matches!(k, Obj::Kw(k) if &**k == b"obj");
        if !num_eq(&objid1, objid) {
            let mut x = Vec::new();
            while !is_obj(&kwd) {
                kwd = st.lex.nexttoken()?.1;
                x.push(kwd.clone());
            }
            if x.len() >= 2 {
                objid1 = x[x.len() - 2].clone();
            }
        }
        if !num_eq(&objid1, objid) {
            return syntax(format!("objid mismatch: {objid1:?}={objid}"));
        }
        if !is_obj(&kwd) {
            return syntax(format!("Invalid object spec: offset={pos}"));
        }
        Ok(st.nextobject(&mut FileKw(self))?.1)
    }

    fn decipher_all(&self, objid: i64, genno: i64, x: Obj) -> Result<Obj> {
        Ok(match x {
            Obj::Str(s) if !s.is_empty() => {
                let sec = self.security.borrow().clone().unwrap();
                Obj::Str(Rc::from(sec.decrypt(objid, genno, &s, None)?))
            }
            Obj::Array(a) => Obj::Array(Rc::new(
                a.iter()
                    .map(|v| self.decipher_all(objid, genno, v.clone()))
                    .collect::<Result<_>>()?,
            )),
            Obj::Dict(d) => {
                let mut out = Dict::default();
                for (k, v) in &d.0 {
                    out.0
                        .push((k.clone(), self.decipher_all(objid, genno, v.clone())?));
                }
                Obj::Dict(Rc::new(out))
            }
            other => other,
        })
    }

    pub(super) fn objstm_objects(&self, stream: &Stream) -> Result<Vec<Obj>> {
        let mut st = Stack::new(Lexer::new(self.stream_data(stream)?));
        let mut objs = Vec::new();
        loop {
            match st.nextobject(&mut ObjStmKw) {
                Ok((_, o)) => objs.push(o),
                Err(PdfError::Eof) => return Ok(objs),
                Err(e) => return Err(e),
            }
        }
    }

    fn objstm_get(&self, stream: &Rc<Stream>, index: i64) -> Result<Obj> {
        let key = stream.objid();
        let cached = self.objstms.borrow().get(&key).cloned();
        let parsed = match cached {
            Some(p) => p,
            None => {
                let n = stream.attrs.get("N").cloned().unwrap_or(Obj::Int(0));
                let p = Rc::new((self.objstm_objects(stream)?, n));
                self.objstms.borrow_mut().insert(key, p.clone());
                p
            }
        };
        let (objs, n) = &*parsed;
        let Some(n) = n.as_int() else {
            return other("unsupported operand type for *");
        };
        let i = n * 2 + index;
        let i = if i < 0 { i + objs.len() as i64 } else { i };
        match usize::try_from(i).ok().and_then(|i| objs.get(i)) {
            Some(o) => Ok(o.clone()),
            None => syntax(format!("index too big: {index}")),
        }
    }

    pub fn resolve(&self, x: &Obj) -> Result<Obj> {
        let mut x = x.clone();
        while let Obj::Ref(id) = x {
            x = match self.getobj(id) {
                Ok(o) => o,
                Err(PdfError::NotFound(_)) => Obj::Null,
                Err(e) => return Err(e),
            };
        }
        Ok(x)
    }

    pub fn dict_value(&self, x: &Obj) -> Result<Rc<Dict>> {
        Ok(match self.resolve(x)? {
            Obj::Dict(d) => d,
            _ => Rc::new(Dict::default()),
        })
    }

    pub fn int_value(&self, x: &Obj) -> Result<i64> {
        Ok(self.resolve(x)?.as_int().unwrap_or(0))
    }

    pub fn num_value(&self, x: &Obj) -> Result<f64> {
        Ok(self.resolve(x)?.as_num().unwrap_or(0.0))
    }

    pub fn list_value(&self, x: &Obj) -> Result<Rc<Vec<Obj>>> {
        Ok(match self.resolve(x)? {
            Obj::Array(a) => a,
            _ => Rc::new(Vec::new()),
        })
    }

    pub fn str_value(&self, x: &Obj) -> Result<Rc<[u8]>> {
        Ok(match self.resolve(x)? {
            Obj::Str(s) => s,
            _ => Rc::from(&b""[..]),
        })
    }

    pub fn stream_value(&self, x: &Obj) -> Result<Rc<Stream>> {
        Ok(match self.resolve(x)? {
            Obj::Stream(s) => s,
            _ => Rc::new(Stream::new(Dict::default(), Vec::new(), false)),
        })
    }

    /// `PDFStream.get_data()`
    pub fn stream_data(&self, s: &Stream) -> Result<Rc<[u8]>> {
        if let Some(d) = s.data.get() {
            return Ok(d.clone());
        }
        let mut data = s.raw.clone();
        if s.decipher {
            let (objid, genno) =
                s.id.get()
                    .ok_or_else(|| PdfError::Other("AssertionError".into()))?;
            let sec = self.security.borrow().clone().unwrap();
            data = sec.decrypt(objid, genno, &data, Some(&s.attrs))?;
        }
        for (f, params) in self.filters(s)? {
            let name = f.name().map(|n| n.to_vec()).unwrap_or_default();
            data = match &name[..] {
                b"FlateDecode" | b"Fl" => filters::flate(&data),
                b"LZWDecode" | b"LZW" => filters::lzw(&data)?,
                b"ASCII85Decode" | b"A85" => filters::ascii85(&data)?,
                b"ASCIIHexDecode" | b"AHx" => filters::ascii_hex(&data)?,
                b"RunLengthDecode" | b"RL" => filters::run_length(&data)?,
                b"CCITTFaxDecode" | b"CCF" => return other("CCITTFaxDecode is not supported"),
                b"DCTDecode" | b"DCT" | b"JBIG2Decode" | b"JPXDecode" => data,
                b"Crypt" => return other("/Crypt filter is unsupported"),
                _ => return other(format!("Unsupported filter: {}", literal_name(&f))),
            };
            let predictor = match &params {
                Obj::Dict(p) => p.get("Predictor").map(|v| (p.clone(), v.clone())),
                Obj::Array(_) => None,
                p if !p.truthy() => None,
                _ => return other("argument of type is not iterable"),
            };
            if let Some((p, pred)) = predictor {
                let get = |k: &str, d: i64| -> Result<i64> {
                    p.get(k).map_or(Ok(d), |v| self.int_value(v))
                };
                let pred = self.int_value(&pred)?;
                data = match pred {
                    1 => data,
                    2 => filters::tiff_predictor(
                        get("Colors", 1)?,
                        get("Columns", 1)?,
                        get("BitsPerComponent", 8)?,
                        &data,
                    )?,
                    p if p >= 10 => filters::png_predictor(
                        get("Colors", 1)?,
                        get("Columns", 1)?,
                        get("BitsPerComponent", 8)?,
                        &data,
                    )?,
                    p => return other(format!("Unsupported predictor: {p}")),
                };
            }
        }
        let data: Rc<[u8]> = Rc::from(data);
        let _ = s.data.set(data.clone());
        Ok(data)
    }

    fn filters(&self, s: &Stream) -> Result<Vec<(Obj, Obj)>> {
        let empty = Obj::Array(Rc::new(Vec::new()));
        let filters = self.resolve(s.attrs.get_any(&["F", "Filter"]).unwrap_or(&empty))?;
        let no_params = Obj::Dict(Rc::new(Dict::default()));
        let params = self.resolve(
            s.attrs
                .get_any(&["DP", "DecodeParms", "FDecodeParms"])
                .unwrap_or(&no_params),
        )?;
        if !filters.truthy() {
            return Ok(Vec::new());
        }
        let filters = match filters {
            Obj::Array(a) => a.to_vec(),
            f => vec![f],
        };
        let params = match params {
            Obj::Array(a) => a.to_vec(),
            p => vec![p; filters.len()],
        };
        filters
            .iter()
            .zip(params.iter())
            .map(|(f, p)| Ok((self.resolve(f)?, self.resolve(p)?)))
            .collect()
    }
}

struct DepthGuard<'a>(&'a Cell<u32>);

impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
