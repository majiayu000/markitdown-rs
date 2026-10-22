//! pdfminer's cmapdb: code-to-CID maps (identity, predefined, empty),
//! CID-to-Unicode maps, and the ToUnicode CMap parser.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Read;
use std::rc::Rc;

use super::cmap_index::CMAPS;
use super::encoding::name2unicode;
use super::lexer::{Keywords, Lexer, Stack};
use super::object::{Obj, PdfError, Result, literal_name, nunpack, other};

static BLOB: &[u8] = include_bytes!("cmaps.bin");

/// How cmap_index.rs describes each predefined CMap.
pub enum Entry {
    Full(usize, usize),
    /// a vertical map stored as its differences from the horizontal one
    Vertical(&'static str, usize, usize),
    /// the UTF-32 map with codes re-encoded
    Recode(&'static str, Recode),
    ToUnicode(usize, usize),
}

pub enum Recode {
    Utf8,
    Utf16,
}

/// A nested code-to-CID dict as a table of 256-entry nodes: 0 is absent,
/// values with the top bit set point at a child node, others are CID + 1.
pub struct Trie {
    nodes: Vec<[u32; 256]>,
}

const CHILD: u32 = 1 << 31;

impl Trie {
    fn build(map: &[(Vec<u8>, u32)]) -> Self {
        let mut t = Trie {
            nodes: vec![[0; 256]],
        };
        for (code, cid) in map {
            let Some((last, prefix)) = code.split_last() else {
                continue;
            };
            let mut node = 0;
            for &b in prefix {
                let e = t.nodes[node][b as usize];
                node = if e & CHILD != 0 {
                    (e & !CHILD) as usize
                } else {
                    t.nodes.push([0; 256]);
                    let n = t.nodes.len() - 1;
                    t.nodes[node][b as usize] = CHILD | n as u32;
                    n
                };
            }
            t.nodes[node][*last as usize] = cid + 1;
        }
        t
    }

    fn decode(&self, code: &[u8]) -> Vec<u32> {
        let mut out = Vec::new();
        let mut node = 0;
        for &b in code {
            let e = self.nodes[node][b as usize];
            if e == 0 {
                node = 0;
            } else if e & CHILD != 0 {
                node = (e & !CHILD) as usize;
            } else {
                out.push(e - 1);
                node = 0;
            }
        }
        out
    }
}

#[derive(Clone)]
pub enum CMap {
    /// `CMap()` for an unknown name: decodes to nothing
    Empty,
    Identity {
        vertical: bool,
    },
    IdentityByte {
        vertical: bool,
    },
    Predefined {
        trie: Rc<Trie>,
        vertical: bool,
    },
}

impl CMap {
    pub fn is_vertical(&self) -> bool {
        match self {
            CMap::Empty => false,
            CMap::Identity { vertical }
            | CMap::IdentityByte { vertical }
            | CMap::Predefined { vertical, .. } => *vertical,
        }
    }

    pub fn decode(&self, code: &[u8]) -> Vec<u32> {
        match self {
            CMap::Empty => Vec::new(),
            CMap::Identity { .. } => code
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_be_bytes(*p) as u32)
                .collect(),
            CMap::IdentityByte { .. } => code.iter().map(|&b| b as u32).collect(),
            CMap::Predefined { trie, .. } => trie.decode(code),
        }
    }

    /// `CMapDB.get_cmap(name)`; None stands for CMapNotFound.
    pub fn get(name: &str) -> Option<CMap> {
        match name {
            "Identity-H" => return Some(CMap::Identity { vertical: false }),
            "Identity-V" => return Some(CMap::Identity { vertical: true }),
            "OneByteIdentityH" => return Some(CMap::IdentityByte { vertical: false }),
            "OneByteIdentityV" => return Some(CMap::IdentityByte { vertical: true }),
            _ => {}
        }
        let name = name.replace('\0', "");
        thread_local! {
            static CACHE: RefCell<HashMap<String, (Rc<Trie>, bool)>> = RefCell::new(HashMap::new());
        }
        if let Some((trie, vertical)) = CACHE.with(|c| c.borrow().get(&name).cloned()) {
            return Some(CMap::Predefined { trie, vertical });
        }
        let (map, vertical) = load_code2cid(&name)?;
        let trie = Rc::new(Trie::build(&map));
        CACHE.with(|c| c.borrow_mut().insert(name, (trie.clone(), vertical)));
        Some(CMap::Predefined { trie, vertical })
    }
}

fn entry(name: &str) -> Option<&'static Entry> {
    CMAPS.iter().find(|(n, _)| *n == name).map(|(_, e)| e)
}

fn inflate(off: usize, len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(&BLOB[off..off + len])
        .read_to_end(&mut out)
        .expect("cmaps.bin is valid zlib");
    out
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn varint(&mut self) -> u64 {
        let mut v = 0u64;
        let mut shift = 0;
        loop {
            let b = self.data[self.pos];
            self.pos += 1;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return v;
            }
            shift += 7;
        }
    }

    fn byte(&mut self) -> u8 {
        self.pos += 1;
        self.data[self.pos - 1]
    }

    /// The generator's `runs_of()` encoding.
    fn runs(&mut self) -> Flat {
        let count = self.varint();
        let mut out = Vec::new();
        let (mut len, mut prev_code, mut prev_val) = (0usize, 0u64, 0i64);
        for _ in 0..count {
            let mut d = self.varint();
            if d == 0 {
                len = self.byte() as usize;
                prev_code = 0;
                d = self.varint();
            }
            let value = prev_code + d - 1;
            let n = self.varint();
            let z = self.varint() as i64;
            let val = prev_val + if z & 1 == 0 { z >> 1 } else { -((z + 1) >> 1) };
            for i in 0..n {
                let code = (value + i).to_be_bytes();
                out.push((code[8 - len..].to_vec(), (val + i as i64) as u32));
            }
            prev_code = value + n;
            prev_val = val + n as i64;
        }
        out
    }
}

/// Applies the generator's `diff()`: removed codes, then changed ones.
fn apply_diff(base: Flat, r: &mut Reader) -> Flat {
    let removed: std::collections::HashSet<Vec<u8>> =
        r.runs().into_iter().map(|(c, _)| c).collect();
    let mut map: HashMap<Vec<u8>, u32> = base
        .into_iter()
        .filter(|(c, _)| !removed.contains(c))
        .collect();
    map.extend(r.runs());
    map.into_iter().collect()
}

/// Codes and their CIDs (or code points), as decoded from cmaps.bin.
type Flat = Vec<(Vec<u8>, u32)>;

fn load_code2cid(name: &str) -> Option<(Flat, bool)> {
    match entry(name)? {
        Entry::Full(off, len) => {
            let data = inflate(*off, *len);
            let mut r = Reader {
                data: &data,
                pos: 1,
            };
            Some((r.runs(), data[0] != 0))
        }
        Entry::Vertical(base, off, len) => {
            let (map, _) = load_code2cid(base)?;
            let data = inflate(*off, *len);
            Some((
                apply_diff(
                    map,
                    &mut Reader {
                        data: &data,
                        pos: 0,
                    },
                ),
                true,
            ))
        }
        Entry::Recode(base, how) => {
            let (map, vertical) = load_code2cid(base)?;
            let recoded = map
                .into_iter()
                .map(|(code, cid)| {
                    let cp = nunpack(&code, 0) as u32;
                    let bytes = match how {
                        Recode::Utf8 => wtf8(cp),
                        Recode::Utf16 => utf16(cp),
                    };
                    (bytes, cid)
                })
                .collect();
            Some((recoded, vertical))
        }
        Entry::ToUnicode(..) => None,
    }
}

/// UTF-8 with surrogates allowed (`surrogatepass`).
fn wtf8(cp: u32) -> Vec<u8> {
    match cp {
        0..=0x7f => vec![cp as u8],
        0x80..=0x7ff => vec![0xc0 | (cp >> 6) as u8, 0x80 | (cp & 0x3f) as u8],
        0x800..=0xffff => vec![
            0xe0 | (cp >> 12) as u8,
            0x80 | ((cp >> 6) & 0x3f) as u8,
            0x80 | (cp & 0x3f) as u8,
        ],
        _ => vec![
            0xf0 | (cp >> 18) as u8,
            0x80 | ((cp >> 12) & 0x3f) as u8,
            0x80 | ((cp >> 6) & 0x3f) as u8,
            0x80 | (cp & 0x3f) as u8,
        ],
    }
}

fn utf16(cp: u32) -> Vec<u8> {
    if cp < 0x10000 {
        return (cp as u16).to_be_bytes().to_vec();
    }
    let v = cp - 0x10000;
    let (hi, lo) = (0xd800 | (v >> 10) as u16, 0xdc00 | (v & 0x3ff) as u16);
    [hi.to_be_bytes(), lo.to_be_bytes()].concat()
}

type CidChars = Rc<HashMap<u32, char>>;

#[derive(Clone)]
pub enum UnicodeMap {
    File(Rc<HashMap<i64, String>>),
    Identity,
    Predefined(Rc<HashMap<u32, char>>),
}

impl UnicodeMap {
    pub fn get(&self, cid: i64) -> Option<String> {
        match self {
            UnicodeMap::File(m) => m.get(&cid).cloned(),
            UnicodeMap::Identity => Some(
                // chr(cid); lone surrogates have no Rust char
                char::from_u32(cid as u32)
                    .filter(|_| (0..=0x10ffff).contains(&cid))
                    .unwrap_or('\u{fffd}')
                    .to_string(),
            ),
            UnicodeMap::Predefined(m) => u32::try_from(cid)
                .ok()
                .and_then(|c| m.get(&c))
                .map(|c| c.to_string()),
        }
    }

    /// `CMapDB.get_unicode_map(cidcoding, vertical)`; None is CMapNotFound.
    pub fn predefined(cidcoding: &str, vertical: bool) -> Option<UnicodeMap> {
        thread_local! {
            static CACHE: RefCell<HashMap<(String, bool), CidChars>> = RefCell::new(HashMap::new());
        }
        let key = (cidcoding.to_string(), vertical);
        if let Some(m) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
            return Some(UnicodeMap::Predefined(m));
        }
        let name = format!("to-unicode-{cidcoding}").replace('\0', "");
        let Some(Entry::ToUnicode(off, len)) = entry(&name) else {
            return None;
        };
        let data = inflate(*off, *len);
        let mut r = Reader {
            data: &data,
            pos: 0,
        };
        let h = r.runs();
        let v = apply_diff(h.clone(), &mut r);
        let to_map = |m: Flat| -> Rc<HashMap<u32, char>> {
            Rc::new(
                m.into_iter()
                    .map(|(code, cp)| {
                        (
                            nunpack(&code, 0) as u32,
                            char::from_u32(cp).unwrap_or('\u{fffd}'),
                        )
                    })
                    .collect(),
            )
        };
        let (h, v) = (to_map(h), to_map(v));
        CACHE.with(|c| {
            let mut c = c.borrow_mut();
            c.insert((cidcoding.to_string(), false), h.clone());
            c.insert((cidcoding.to_string(), true), v.clone());
        });
        Some(UnicodeMap::Predefined(if vertical { v } else { h }))
    }
}

/// `bytes.decode('UTF-16BE', 'ignore')`
pub fn utf16be_ignore(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u16::from_be_bytes(*p))
        .collect();
    char::decode_utf16(units).filter_map(|r| r.ok()).collect()
}

/// FileUnicodeMap.add_cid2unichr
pub fn add_cid2unichr(map: &mut HashMap<i64, String>, cid: i64, code: &Obj) -> Result<()> {
    let unichr = match code {
        Obj::Name(n) => {
            let Ok(name) = std::str::from_utf8(n) else {
                return other("AssertionError: name is not str");
            };
            name2unicode(name)
                .or_else(|_| other(format!("Could not convert unicode name \"{name}\"")))?
        }
        Obj::Str(s) => utf16be_ignore(s),
        Obj::Int(_) | Obj::Bool(_) => {
            let v = code.as_int().unwrap();
            if !(0..=0x10ffff).contains(&v) {
                return other("chr() arg not in range(0x110000)");
            }
            char::from_u32(v as u32).unwrap_or('\u{fffd}').to_string()
        }
        _ => return other("PDFTypeError"),
    };
    if unichr == "\u{a0}" && map.get(&cid).is_some_and(|s| s == " ") {
        return Ok(());
    }
    map.insert(cid, unichr);
    Ok(())
}

/// CMapParser's keyword handling, building a FileUnicodeMap.
struct CMapKw {
    map: HashMap<i64, String>,
    in_cmap: bool,
}

/// Ranges in a hostile CMap could ask for billions of entries.
const MAX_RANGE: u64 = 1 << 20;

fn bytes_of(o: &Obj) -> Option<&[u8]> {
    match o {
        Obj::Str(s) => Some(s),
        _ => None,
    }
}

/// `prefix + struct.pack('>L', v)[-vlen:]`, where vlen 0 keeps all four.
fn packed(prefix: &[u8], v: u64, vlen: usize) -> Result<Vec<u8>> {
    let v = u32::try_from(v).or_else(|_| other("'L' format requires 0 <= number <= 4294967295"))?;
    let b = v.to_be_bytes();
    let tail = if vlen == 0 {
        &b[..]
    } else {
        &b[4 - vlen.min(4)..]
    };
    Ok([prefix, tail].concat())
}

impl Keywords for CMapKw {
    fn do_keyword(&mut self, st: &mut Stack, pos: usize, kw: Rc<[u8]>) -> Result<()> {
        match &*kw {
            b"begincmap" => {
                self.in_cmap = true;
                st.popall();
                return Ok(());
            }
            b"endcmap" => {
                self.in_cmap = false;
                return Ok(());
            }
            _ => {}
        }
        if !self.in_cmap {
            return Ok(());
        }
        let objs = |st: &mut Stack| st.popall().into_iter().map(|(_, o)| o).collect::<Vec<_>>();
        match &*kw {
            b"def" => {
                if st.pop(2).len() < 2 {
                    return other("not enough values to unpack");
                }
            }
            b"usecmap" => {
                let v = st.pop(1);
                let Some((_, name)) = v.first() else {
                    return other("not enough values to unpack");
                };
                let _ = CMap::get(&literal_name(name));
            }
            b"begincodespacerange"
            | b"endcodespacerange"
            | b"begincidrange"
            | b"begincidchar"
            | b"beginbfrange"
            | b"beginbfchar"
            | b"beginnotdefrange"
            | b"endnotdefrange" => {
                st.popall();
            }
            b"endcidrange" => {
                for t in objs(st).as_chunks::<3>().0 {
                    let (Some(s), Some(e), Some(cid)) =
                        (bytes_of(&t[0]), bytes_of(&t[1]), t[2].as_int())
                    else {
                        continue;
                    };
                    if s.len() != e.len() {
                        continue;
                    }
                    let (sp, ep) = (
                        &s[..s.len().saturating_sub(4)],
                        &e[..e.len().saturating_sub(4)],
                    );
                    if sp != ep {
                        continue;
                    }
                    let (svar, evar) = (&s[sp.len()..], &e[ep.len()..]);
                    let (start, end) = (nunpack(svar, 0), nunpack(evar, 0));
                    for i in 0..(end + 1).saturating_sub(start).min(MAX_RANGE) {
                        let x = packed(sp, start + i, svar.len())?;
                        add_cid2unichr(&mut self.map, cid + i as i64, &Obj::Str(Rc::from(x)))?;
                    }
                }
            }
            b"endcidchar" => {
                for p in objs(st).as_chunks::<2>().0 {
                    if let (Some(cid), Obj::Str(_)) = (p[0].as_int(), &p[1]) {
                        add_cid2unichr(&mut self.map, cid, &p[1])?;
                    }
                }
            }
            b"endbfrange" => {
                for t in objs(st).as_chunks::<3>().0 {
                    let (Some(s), Some(e)) = (bytes_of(&t[0]), bytes_of(&t[1])) else {
                        continue;
                    };
                    if s.len() != e.len() {
                        continue;
                    }
                    let (start, end) = (nunpack(s, 0) as i64, nunpack(e, 0) as i64);
                    match &t[2] {
                        Obj::Array(code) => {
                            for (cid, v) in (start..=end).zip(code.iter()) {
                                add_cid2unichr(&mut self.map, cid, v)?;
                            }
                        }
                        Obj::Str(code) => {
                            let var = &code[code.len().saturating_sub(4)..];
                            let base = nunpack(var, 0);
                            let prefix = &code[..code.len() - var.len()];
                            let n = (end - start + 1).max(0) as u64;
                            for i in 0..n.min(MAX_RANGE) {
                                let x = packed(prefix, base + i, var.len())?;
                                add_cid2unichr(
                                    &mut self.map,
                                    start + i as i64,
                                    &Obj::Str(Rc::from(x)),
                                )?;
                            }
                        }
                        _ => return other("AssertionError: bfrange code is not bytes"),
                    }
                }
            }
            b"endbfchar" => {
                for p in objs(st).as_chunks::<2>().0 {
                    if let (Some(cid), Obj::Str(_)) = (bytes_of(&p[0]), &p[1]) {
                        add_cid2unichr(&mut self.map, nunpack(cid, 0) as i64, &p[1])?;
                    }
                }
            }
            _ => st.push((pos, Obj::Kw(kw))),
        }
        Ok(())
    }
}

/// `CMapParser(FileUnicodeMap(), data).run()`
pub fn parse_tounicode(data: Rc<[u8]>) -> Result<UnicodeMap> {
    let mut st = Stack::new(Lexer::new(data));
    let mut kw = CMapKw {
        map: HashMap::new(),
        in_cmap: true,
    };
    match st.nextobject(&mut kw) {
        Ok(_) | Err(PdfError::Eof) => {}
        Err(e) => return Err(e),
    }
    Ok(UnicodeMap::File(Rc::new(kw.map)))
}
