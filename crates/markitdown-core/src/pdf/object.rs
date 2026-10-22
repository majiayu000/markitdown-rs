//! pdfminer's object model: Python values as pdfminer leaves them after
//! parsing (ints, floats, bytes, interned literals and keywords, lists,
//! dicts keyed by literal name, streams and object references).

use std::cell::{Cell, OnceCell};
use std::fmt;
use std::rc::Rc;

use crate::pystr;

#[derive(Clone, Debug)]
pub enum Obj {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Str(Rc<[u8]>),
    Name(Rc<[u8]>),
    Kw(Rc<[u8]>),
    Array(Rc<Vec<Obj>>),
    Dict(Rc<Dict>),
    Stream(Rc<Stream>),
    Ref(i64),
}

/// A Python dict keyed by `literal_name()`, in insertion order.
#[derive(Clone, Debug, Default)]
pub struct Dict(pub Vec<(Rc<[u8]>, Obj)>);

impl Dict {
    pub fn get(&self, key: &str) -> Option<&Obj> {
        self.0
            .iter()
            .find(|(k, _)| &**k == key.as_bytes())
            .map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `d[k] = v`: replaces in place, else appends.
    pub fn set(&mut self, key: Rc<[u8]>, value: Obj) {
        if let Some(slot) = self.0.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.0.push((key, value));
        }
    }

    pub fn get_any(&self, keys: &[&str]) -> Option<&Obj> {
        keys.iter().find_map(|k| self.get(k))
    }
}

pub struct Stream {
    pub attrs: Dict,
    pub raw: Vec<u8>,
    /// Whether the document's decipher was set when the stream was parsed.
    pub decipher: bool,
    /// (objid, genno) once fetched through the xref.
    pub id: Cell<Option<(i64, i64)>>,
    pub(crate) data: OnceCell<Rc<[u8]>>,
}

impl fmt::Debug for Stream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<PDFStream({:?}): raw={}>",
            self.id.get(),
            self.raw.len()
        )
    }
}

impl Stream {
    pub fn new(attrs: Dict, raw: Vec<u8>, decipher: bool) -> Self {
        Stream {
            attrs,
            raw,
            decipher,
            id: Cell::new(None),
            data: OnceCell::new(),
        }
    }

    pub fn objid(&self) -> Option<i64> {
        self.id.get().map(|(o, _)| o)
    }
}

/// Errors with the distinctions pdfminer's `except` clauses make.
#[derive(Debug, Clone)]
pub enum PdfError {
    /// PSEOF
    Eof,
    /// PDFSyntaxError and its subclass PDFNoValidXRef (`no_xref`)
    Syntax { msg: String, no_xref: bool },
    /// PDFObjectNotFound
    NotFound(i64),
    /// anything else, which only ends the conversion
    Other(String),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::Eof => f.write_str("Unexpected EOF"),
            PdfError::Syntax { msg, .. } => f.write_str(msg),
            PdfError::NotFound(id) => write!(f, "object {id} not found"),
            PdfError::Other(m) => f.write_str(m),
        }
    }
}

pub type Result<T> = std::result::Result<T, PdfError>;

pub fn syntax<T>(msg: impl Into<String>) -> Result<T> {
    Err(PdfError::Syntax {
        msg: msg.into(),
        no_xref: false,
    })
}

pub fn no_xref<T>(msg: impl Into<String>) -> Result<T> {
    Err(PdfError::Syntax {
        msg: msg.into(),
        no_xref: true,
    })
}

pub fn other<T>(msg: impl Into<String>) -> Result<T> {
    Err(PdfError::Other(msg.into()))
}

impl Obj {
    pub fn name(&self) -> Option<&[u8]> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn is_name(&self, name: &str) -> bool {
        self.name() == Some(name.as_bytes())
    }

    /// `isinstance(x, int)` (bools are ints)
    pub fn as_int(&self) -> Option<i64> {
        match *self {
            Obj::Int(i) => Some(i),
            Obj::Bool(b) => Some(b as i64),
            _ => None,
        }
    }

    /// `isinstance(x, (int, float))`
    pub fn as_num(&self) -> Option<f64> {
        match *self {
            Obj::Int(i) => Some(i as f64),
            Obj::Real(r) => Some(r),
            Obj::Bool(b) => Some(b as i64 as f64),
            _ => None,
        }
    }

    /// Python truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Obj::Null => false,
            Obj::Bool(b) => *b,
            Obj::Int(i) => *i != 0,
            Obj::Real(r) => *r != 0.0,
            Obj::Str(s) => !s.is_empty(),
            Obj::Array(a) => !a.is_empty(),
            Obj::Dict(d) => !d.0.is_empty(),
            _ => true,
        }
    }
}

/// `safe_float()`: `float(x)` or None.
pub fn safe_float(o: &Obj) -> Option<f64> {
    match o {
        Obj::Str(s) => py_float(&String::from_utf8_lossy(s)),
        _ => o.as_num(),
    }
}

/// `safe_int()`: `int(x)` or None. `int()` of an infinite float raises
/// OverflowError, which safe_int lets through.
pub fn safe_int(o: &Obj) -> Result<Option<i64>> {
    Ok(match o {
        Obj::Real(r) if !r.is_finite() => return other("cannot convert float to integer"),
        Obj::Real(r) => Some(r.trunc() as i64),
        Obj::Str(s) => py_int(&String::from_utf8_lossy(s)),
        _ => o.as_int(),
    })
}

/// Python `float(str)`.
pub fn py_float(s: &str) -> Option<f64> {
    let t = pystr::strip(s);
    let lower = t.to_ascii_lowercase();
    let unsigned = lower.trim_start_matches(['+', '-']);
    if lower.len() - unsigned.len() > 1 {
        return None;
    }
    let neg = lower.starts_with('-');
    let special = match unsigned {
        "inf" | "infinity" => Some(f64::INFINITY),
        "nan" => Some(f64::NAN),
        _ => None,
    };
    if let Some(v) = special {
        return Some(if neg { -v } else { v });
    }
    let digits = without_underscores(t)?;
    let valid = !digits.is_empty()
        && digits
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'));
    if valid { digits.parse().ok() } else { None }
}

/// Python `int(str)`.
pub fn py_int(s: &str) -> Option<i64> {
    let t = pystr::strip(s);
    let digits = without_underscores(t)?;
    let body = digits.trim_start_matches(['+', '-']);
    if digits.len() - body.len() > 1 || body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    digits.parse().ok()
}

/// Underscores are allowed only between digits.
fn without_underscores(t: &str) -> Option<String> {
    let b = t.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'_'
            && !(i > 0 && i + 1 < b.len() && b[i - 1].is_ascii_digit() && b[i + 1].is_ascii_digit())
        {
            return None;
        }
    }
    Some(t.replace('_', ""))
}

/// Python `repr(bytes)`.
pub fn bytes_repr(b: &[u8]) -> String {
    let quote = if b.contains(&b'\'') && !b.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = format!("b{quote}");
    for &c in b {
        match c {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            _ if c as char == quote => {
                out.push('\\');
                out.push(quote);
            }
            0x20..=0x7e => out.push(c as char),
            _ => out.push_str(&format!("\\x{c:02x}")),
        }
    }
    out.push(quote);
    out
}

/// Python `str(x)` for the values pdfminer stringifies.
pub fn py_str(o: &Obj) -> String {
    match o {
        Obj::Null => "None".into(),
        Obj::Bool(b) => if *b { "True" } else { "False" }.into(),
        Obj::Int(i) => i.to_string(),
        Obj::Real(r) => pystr::float_repr(*r),
        Obj::Str(s) => bytes_repr(s),
        Obj::Name(n) => format!("/{}", name_str(n)),
        Obj::Kw(k) => format!("/{}", bytes_repr(k)),
        Obj::Ref(id) => format!("<PDFObjRef:{id}>"),
        Obj::Stream(s) => format!("{s:?}"),
        Obj::Array(a) => format!("[{}]", a.iter().map(py_str).collect::<Vec<_>>().join(", ")),
        Obj::Dict(_) => "{...}".into(),
    }
}

/// A literal's name as pdfminer holds it: str when it is UTF-8, else what
/// `str()` makes of its bytes.
pub fn name_str(n: &[u8]) -> String {
    match std::str::from_utf8(n) {
        Ok(s) => s.to_string(),
        Err(_) => bytes_repr(n),
    }
}

/// `literal_name()`
pub fn literal_name(o: &Obj) -> String {
    match o {
        Obj::Name(n) => name_str(n),
        _ => py_str(o),
    }
}

/// `nunpack()`
pub fn nunpack(s: &[u8], default: u64) -> u64 {
    if s.is_empty() {
        return default;
    }
    s.iter().fold(0u64, |v, &b| v.wrapping_shl(8) | b as u64)
}
