//! pdfminer's PSBaseParser and PSStackParser. The tokenizer is a state
//! machine fed 4096-byte buffers; tokens split across a buffer boundary
//! behave exactly as in pdfminer (e.g. `<<` split in two opens a hex
//! string), so the buffering is reproduced.

use std::collections::VecDeque;
use std::rc::Rc;

use super::object::{Obj, PdfError, Result, other};

pub const BUFSIZ: usize = 4096;

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Main,
    Comment,
    Literal,
    LiteralHex,
    Number,
    Float,
    Keyword,
    String,
    String1,
    WOpen,
    WClose,
    HexString,
}

/// `bytes.isspace()` / regex `\s` on bytes.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn is_end_literal(c: u8) -> bool {
    matches!(
        c,
        b'#' | b'/' | b'%' | b'[' | b']' | b'(' | b')' | b'<' | b'>' | b'{' | b'}'
    ) || is_space(c)
}

pub struct Lexer {
    /// The file (one source) or a page's content streams, decoded lazily
    /// as pdfminer's PDFContentParser.fillfp does.
    srcs: Vec<Result<Rc<[u8]>>>,
    multi: bool,
    istream: usize,
    fp: Option<Rc<[u8]>>,
    fpos: usize,
    pub bufpos: usize,
    buf: Rc<[u8]>,
    buf_start: usize,
    buf_len: usize,
    pub charpos: usize,
    state: State,
    curtoken: Vec<u8>,
    curtokenpos: usize,
    tokens: VecDeque<(usize, Obj)>,
    eof: bool,
    paren: i32,
    hex: Vec<u8>,
    oct: Vec<u8>,
}

impl Lexer {
    pub fn new(data: Rc<[u8]>) -> Self {
        let mut l = Self::blank(vec![Ok(data.clone())], false);
        l.fp = Some(data);
        l.istream = 1;
        l
    }

    /// PDFContentParser over several streams. Fails with Eof when there
    /// are none, like its constructor.
    pub fn content(srcs: Vec<Result<Rc<[u8]>>>) -> Result<Self> {
        let mut l = Self::blank(srcs, true);
        l.seek(0)?;
        Ok(l)
    }

    fn blank(srcs: Vec<Result<Rc<[u8]>>>, multi: bool) -> Self {
        Lexer {
            srcs,
            multi,
            istream: 0,
            fp: None,
            fpos: 0,
            bufpos: 0,
            buf: Rc::from(&b""[..]),
            buf_start: 0,
            buf_len: 0,
            charpos: 0,
            state: State::Main,
            curtoken: Vec::new(),
            curtokenpos: 0,
            tokens: VecDeque::new(),
            eof: false,
            paren: 0,
            hex: Vec::new(),
            oct: Vec::new(),
        }
    }

    fn fillfp(&mut self) -> Result<bool> {
        if self.fp.is_some() {
            return Ok(false);
        }
        if self.istream >= self.srcs.len() {
            return Err(PdfError::Eof);
        }
        let data = self.srcs[self.istream].clone()?;
        self.istream += 1;
        self.fp = Some(data);
        self.fpos = 0;
        Ok(true)
    }

    pub fn seek(&mut self, pos: usize) -> Result<()> {
        if self.multi {
            self.fillfp()?;
        }
        self.fpos = pos;
        self.bufpos = pos;
        self.buf_len = 0;
        self.charpos = 0;
        self.state = State::Main;
        self.curtoken.clear();
        self.curtokenpos = 0;
        self.tokens.clear();
        self.eof = false;
        Ok(())
    }

    /// `fp.read(n)`
    fn read(&mut self, n: usize) -> (Rc<[u8]>, usize, usize) {
        let data = self.fp.clone().unwrap_or_else(|| Rc::from(&b""[..]));
        let start = self.fpos.min(data.len());
        let end = start.saturating_add(n).min(data.len());
        self.fpos = self.fpos.max(end);
        (data, start, end - start)
    }

    fn fillbuf(&mut self) -> Result<bool> {
        if self.charpos < self.buf_len {
            return Ok(false);
        }
        if !self.multi {
            self.bufpos = self.fpos;
            let (b, s, l) = self.read(BUFSIZ);
            if l == 0 {
                return Err(PdfError::Eof);
            }
            (self.buf, self.buf_start, self.buf_len) = (b, s, l);
            self.charpos = 0;
            return Ok(false);
        }
        let mut new_stream;
        loop {
            new_stream = self.fillfp()?;
            self.bufpos = self.fpos;
            let (b, s, l) = self.read(BUFSIZ);
            (self.buf, self.buf_start, self.buf_len) = (b, s, l);
            if l > 0 {
                break;
            }
            self.fp = None;
        }
        self.charpos = 0;
        Ok(new_stream)
    }

    pub fn nextline(&mut self) -> Result<(usize, Vec<u8>)> {
        let mut linebuf = Vec::new();
        let linepos = self.bufpos + self.charpos;
        let mut eol = false;
        loop {
            self.fillbuf()?;
            let buf = self.buf.clone();
            let buf = &buf[self.buf_start..self.buf_start + self.buf_len];
            if eol {
                if buf.get(self.charpos) == Some(&b'\n') {
                    linebuf.push(b'\n');
                    self.charpos += 1;
                }
                break;
            }
            match buf[self.charpos..]
                .iter()
                .position(|&c| c == b'\r' || c == b'\n')
            {
                Some(off) => {
                    let end = self.charpos + off + 1;
                    linebuf.extend_from_slice(&buf[self.charpos..end]);
                    self.charpos = end;
                    if linebuf.last() == Some(&b'\r') {
                        eol = true;
                    } else {
                        break;
                    }
                }
                None => {
                    linebuf.extend_from_slice(&buf[self.charpos..]);
                    self.charpos = buf.len();
                }
            }
        }
        Ok((linepos, linebuf))
    }

    fn add_token(&mut self, obj: Obj) {
        self.tokens.push_back((self.curtokenpos, obj));
    }

    fn token_bytes(&self) -> Rc<[u8]> {
        Rc::from(&self.curtoken[..])
    }

    /// One step of the state machine over `s` from `i`; returns the new
    /// position. `s` is the current buffer or a lone "\n".
    fn parse1(&mut self, s: &[u8], i: usize) -> usize {
        match self.state {
            State::Main => self.parse_main(s, i),
            State::Comment => match s[i..].iter().position(|&c| c == b'\r' || c == b'\n') {
                None => {
                    self.curtoken.extend_from_slice(&s[i..]);
                    s.len()
                }
                Some(off) => {
                    self.curtoken.extend_from_slice(&s[i..i + off]);
                    self.state = State::Main;
                    i + off
                }
            },
            State::Literal => match s[i..].iter().position(|&c| is_end_literal(c)) {
                None => {
                    self.curtoken.extend_from_slice(&s[i..]);
                    s.len()
                }
                Some(off) => {
                    let j = i + off;
                    self.curtoken.extend_from_slice(&s[i..j]);
                    if s[j] == b'#' {
                        self.hex.clear();
                        self.state = State::LiteralHex;
                        return j + 1;
                    }
                    let name = self.token_bytes();
                    self.add_token(Obj::Name(name));
                    self.state = State::Main;
                    j
                }
            },
            State::LiteralHex => {
                let c = s.get(i).copied();
                if let Some(c) = c.filter(|c| c.is_ascii_hexdigit())
                    && self.hex.len() < 2
                {
                    self.hex.push(c);
                    return i + 1;
                }
                if !self.hex.is_empty() {
                    let v =
                        u8::from_str_radix(std::str::from_utf8(&self.hex).unwrap(), 16).unwrap();
                    self.curtoken.push(v);
                }
                self.state = State::Literal;
                i
            }
            State::Number => match s[i..].iter().position(|c| !c.is_ascii_digit()) {
                None => {
                    self.curtoken.extend_from_slice(&s[i..]);
                    s.len()
                }
                Some(off) => {
                    let j = i + off;
                    self.curtoken.extend_from_slice(&s[i..j]);
                    if s[j] == b'.' {
                        self.curtoken.push(b'.');
                        self.state = State::Float;
                        return j + 1;
                    }
                    let t = std::str::from_utf8(&self.curtoken).unwrap_or("");
                    if let Ok(v) = t.parse::<i64>() {
                        self.add_token(Obj::Int(v));
                    } else if t.len() > 1
                        && let Ok(v) = t.parse::<f64>()
                    {
                        // beyond i64 (Python keeps an exact int)
                        self.add_token(Obj::Real(v));
                    }
                    self.state = State::Main;
                    j
                }
            },
            State::Float => match s[i..].iter().position(|c| !c.is_ascii_digit()) {
                None => {
                    self.curtoken.extend_from_slice(&s[i..]);
                    s.len()
                }
                Some(off) => {
                    let j = i + off;
                    self.curtoken.extend_from_slice(&s[i..j]);
                    let t = std::str::from_utf8(&self.curtoken).unwrap_or("");
                    if t.bytes().any(|b| b.is_ascii_digit())
                        && let Ok(v) = t.parse::<f64>()
                    {
                        self.add_token(Obj::Real(v));
                    }
                    self.state = State::Main;
                    j
                }
            },
            State::Keyword => match s[i..].iter().position(|&c| is_end_literal(c)) {
                None => {
                    self.curtoken.extend_from_slice(&s[i..]);
                    s.len()
                }
                Some(off) => {
                    let j = i + off;
                    self.curtoken.extend_from_slice(&s[i..j]);
                    let tok = match &self.curtoken[..] {
                        b"true" => Obj::Bool(true),
                        b"false" => Obj::Bool(false),
                        _ => Obj::Kw(self.token_bytes()),
                    };
                    self.add_token(tok);
                    self.state = State::Main;
                    j
                }
            },
            State::String => match s[i..]
                .iter()
                .position(|&c| matches!(c, b'(' | b')' | b'\\'))
            {
                None => {
                    self.curtoken.extend_from_slice(&s[i..]);
                    s.len()
                }
                Some(off) => {
                    let j = i + off;
                    self.curtoken.extend_from_slice(&s[i..j]);
                    match s[j] {
                        b'\\' => {
                            self.oct.clear();
                            self.state = State::String1;
                            return j + 1;
                        }
                        b'(' => {
                            self.paren += 1;
                            self.curtoken.push(b'(');
                            return j + 1;
                        }
                        _ => {
                            self.paren -= 1;
                            if self.paren != 0 {
                                self.curtoken.push(b')');
                                return j + 1;
                            }
                        }
                    }
                    let tok = Obj::Str(self.token_bytes());
                    self.add_token(tok);
                    self.state = State::Main;
                    j + 1
                }
            },
            State::String1 => {
                let c = s.get(i).copied();
                if let Some(c) = c.filter(|c| (b'0'..=b'7').contains(c))
                    && self.oct.len() < 3
                {
                    self.oct.push(c);
                    return i + 1;
                }
                if !self.oct.is_empty() {
                    let v =
                        u32::from_str_radix(std::str::from_utf8(&self.oct).unwrap(), 8).unwrap();
                    // pdfminer asserts the value fits in a byte
                    self.curtoken.push(v as u8);
                    self.state = State::String;
                    return i;
                }
                let mut i = i;
                match c {
                    Some(b'b') => self.curtoken.push(8),
                    Some(b't') => self.curtoken.push(9),
                    Some(b'n') => self.curtoken.push(10),
                    Some(b'f') => self.curtoken.push(12),
                    Some(b'r') => self.curtoken.push(13),
                    Some(c @ (b'(' | b')' | b'\\')) => self.curtoken.push(c),
                    Some(b'\r') if s.get(i + 1) == Some(&b'\n') => i += 1,
                    _ => {}
                }
                self.state = State::String;
                i + 1
            }
            State::WOpen => {
                if s.get(i) == Some(&b'<') {
                    self.add_token(Obj::Kw(Rc::from(&b"<<"[..])));
                    self.state = State::Main;
                    i + 1
                } else {
                    self.state = State::HexString;
                    i
                }
            }
            State::WClose => {
                let mut i = i;
                if s.get(i) == Some(&b'>') {
                    self.add_token(Obj::Kw(Rc::from(&b">>"[..])));
                    i += 1;
                }
                self.state = State::Main;
                i
            }
            State::HexString => {
                match s[i..]
                    .iter()
                    .position(|&c| !(is_space(c) || c.is_ascii_hexdigit()))
                {
                    None => {
                        self.curtoken.extend_from_slice(&s[i..]);
                        s.len()
                    }
                    Some(off) => {
                        let j = i + off;
                        self.curtoken.extend_from_slice(&s[i..j]);
                        let digits: Vec<u8> = self
                            .curtoken
                            .iter()
                            .copied()
                            .filter(|&c| !is_space(c))
                            .collect();
                        // pairs become bytes; an odd last digit is its own value
                        let tok: Vec<u8> = digits
                            .chunks(2)
                            .map(|p| {
                                u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap()
                            })
                            .collect();
                        self.add_token(Obj::Str(Rc::from(tok)));
                        self.state = State::Main;
                        j
                    }
                }
            }
        }
    }

    fn parse_main(&mut self, s: &[u8], i: usize) -> usize {
        let Some(off) = s[i..].iter().position(|&c| !is_space(c)) else {
            return s.len();
        };
        let j = i + off;
        let c = s[j];
        self.curtokenpos = self.bufpos + j;
        // NUL and one-character keywords leave the previous token text in
        // place, which PDFContentParser looks at when a stream ends
        let (state, token): (State, &[u8]) = match c {
            b'%' => (State::Comment, b"%"),
            b'/' => (State::Literal, b""),
            b'-' | b'+' | b'0'..=b'9' => (State::Number, &s[j..=j]),
            b'.' => (State::Float, b"."),
            _ if c.is_ascii_alphabetic() => (State::Keyword, &s[j..=j]),
            b'(' => {
                self.paren = 1;
                (State::String, b"")
            }
            b'<' => (State::WOpen, b""),
            b'>' => (State::WClose, b""),
            0 => return j + 1,
            _ => {
                self.add_token(Obj::Kw(Rc::from(&[c][..])));
                return j + 1;
            }
        };
        self.state = state;
        self.curtoken.clear();
        self.curtoken.extend_from_slice(token);
        j + 1
    }

    pub fn nexttoken(&mut self) -> Result<(usize, Obj)> {
        if self.eof {
            return Err(PdfError::Eof);
        }
        while self.tokens.is_empty() {
            match self.fillbuf() {
                Ok(changed) => {
                    if changed && !self.curtoken.is_empty() {
                        self.parse1(b"\n", 0);
                    } else {
                        let buf = self.buf.clone();
                        let s = &buf[self.buf_start..self.buf_start + self.buf_len];
                        self.charpos = self.parse1(s, self.charpos);
                    }
                }
                Err(PdfError::Eof) => {
                    self.charpos = self.parse1(b"\n", 0);
                    self.eof = true;
                    if self.tokens.is_empty() {
                        return Err(PdfError::Eof);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(self.tokens.pop_front().unwrap())
    }

    /// PDFContentParser.get_inline_data
    pub fn get_inline_data(&mut self, pos: usize, target: &[u8]) -> Result<(usize, Vec<u8>)> {
        self.seek(pos)?;
        let mut i = 0;
        let mut data = Vec::new();
        while i <= target.len() {
            self.fillbuf()?;
            let buf = self.buf.clone();
            let buf = &buf[self.buf_start..self.buf_start + self.buf_len];
            if i > 0 {
                let c = buf[self.charpos];
                data.push(c);
                self.charpos += 1;
                if (target.len() <= i && is_space(c)) || (i < target.len() && c == target[i]) {
                    i += 1;
                } else {
                    i = 0;
                }
            } else if let Some(off) = buf[self.charpos..].iter().position(|&c| c == target[0]) {
                let j = self.charpos + off;
                data.extend_from_slice(&buf[self.charpos..=j]);
                self.charpos = j + 1;
                i = 1;
            } else {
                data.extend_from_slice(&buf[self.charpos..]);
                self.charpos = buf.len();
            }
        }
        data.truncate(data.len().saturating_sub(target.len() + 1));
        strip_final_eol(&mut data);
        Ok((pos, data))
    }
}

/// `re.sub(rb"(\x0d\x0a|[\x0d\x0a])$", b"", data)`; `$` also matches
/// before a final newline, so up to two line ends can go.
fn strip_final_eol(data: &mut Vec<u8>) {
    let n = data.len();
    let at_end = |e: usize| e == n || (e + 1 == n && data[n - 1] == b'\n');
    let mut keep = Vec::with_capacity(n);
    let mut p = 0;
    while p < n {
        let m = if data[p] == b'\r' && data.get(p + 1) == Some(&b'\n') && at_end(p + 2) {
            2
        } else if matches!(data[p], b'\r' | b'\n') && at_end(p + 1) {
            1
        } else {
            0
        };
        if m == 0 {
            keep.push(data[p]);
            p += 1;
        } else {
            p += m;
        }
    }
    *data = keep;
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ty {
    Array,
    Dict,
    Proc,
    Inline,
}

pub type Entry = (usize, Obj);

/// PSStackParser state; keyword handling lives with each user.
pub struct Stack {
    pub lex: Lexer,
    context: Vec<(usize, Option<Ty>, Vec<Entry>)>,
    curtype: Option<Ty>,
    pub curstack: Vec<Entry>,
    pub results: VecDeque<Entry>,
}

/// What nextobject does with a keyword other than the brackets.
pub trait Keywords {
    fn do_keyword(&mut self, st: &mut Stack, pos: usize, kw: Rc<[u8]>) -> Result<()>;
    /// PDFStreamParser and PDFContentParser move each finished top-level
    /// object to the results.
    fn flushes(&self) -> bool {
        false
    }
}

impl Stack {
    pub fn new(lex: Lexer) -> Self {
        Stack {
            lex,
            context: Vec::new(),
            curtype: None,
            curstack: Vec::new(),
            results: VecDeque::new(),
        }
    }

    pub fn reset(&mut self) {
        self.context.clear();
        self.curtype = None;
        self.curstack.clear();
        self.results.clear();
    }

    pub fn seek(&mut self, pos: usize) -> Result<()> {
        self.lex.seek(pos)?;
        self.reset();
        Ok(())
    }

    pub fn push(&mut self, e: Entry) {
        self.curstack.push(e);
    }

    /// `pop(n)` for n > 0: the last n entries (fewer if the stack is short).
    pub fn pop(&mut self, n: usize) -> Vec<Entry> {
        let k = self.curstack.len().saturating_sub(n);
        self.curstack.split_off(k)
    }

    pub fn popall(&mut self) -> Vec<Entry> {
        std::mem::take(&mut self.curstack)
    }

    pub fn start_type(&mut self, pos: usize, ty: Ty) {
        let stack = std::mem::take(&mut self.curstack);
        self.context.push((pos, self.curtype, stack));
        self.curtype = Some(ty);
    }

    /// None stands for the PSTypeError pdfminer swallows.
    pub fn end_type(&mut self, ty: Ty) -> Option<(usize, Vec<Obj>)> {
        if self.curtype != Some(ty) {
            return None;
        }
        let objs = std::mem::take(&mut self.curstack)
            .into_iter()
            .map(|(_, o)| o)
            .collect();
        let (pos, curtype, stack) = self.context.pop()?;
        self.curtype = curtype;
        self.curstack = stack;
        Some((pos, objs))
    }

    pub fn nextobject(&mut self, kw: &mut dyn Keywords) -> Result<Entry> {
        while self.results.is_empty() {
            let (pos, token) = self.lex.nexttoken()?;
            match token {
                Obj::Kw(k) => match &*k {
                    b"[" => self.start_type(pos, Ty::Array),
                    b"]" => {
                        if let Some((p, objs)) = self.end_type(Ty::Array) {
                            self.push((p, Obj::Array(Rc::new(objs))));
                        }
                    }
                    b"<<" => self.start_type(pos, Ty::Dict),
                    b">>" => {
                        if let Some((p, objs)) = self.end_type(Ty::Dict) {
                            if objs.len() % 2 != 0 {
                                return other("Invalid dictionary construct");
                            }
                            self.push((p, Obj::Dict(Rc::new(make_dict(objs)))));
                        }
                    }
                    b"{" => self.start_type(pos, Ty::Proc),
                    b"}" => {
                        if let Some((p, objs)) = self.end_type(Ty::Proc) {
                            self.push((p, Obj::Array(Rc::new(objs))));
                        }
                    }
                    _ => kw.do_keyword(self, pos, k)?,
                },
                tok => self.push((pos, tok)),
            }
            if !self.context.is_empty() {
                continue;
            }
            if kw.flushes() {
                let all = self.popall();
                self.results.extend(all);
            }
        }
        Ok(self.results.pop_front().unwrap())
    }
}

/// `{literal_name(k): v for k, v in choplist(2, objs) if v is not None}`
pub fn make_dict(objs: Vec<Obj>) -> super::object::Dict {
    let mut d = super::object::Dict::default();
    let mut it = objs.into_iter();
    while let (Some(k), Some(v)) = (it.next(), it.next()) {
        if matches!(v, Obj::Null) {
            continue;
        }
        d.set(Rc::from(super::object::literal_name(&k).into_bytes()), v);
    }
    d
}
