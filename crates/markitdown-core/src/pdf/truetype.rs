//! pdfminer's TrueTypeFont: the Unicode map from an embedded font's
//! `cmap` table, used for Identity-encoded CID fonts without ToUnicode.

use std::collections::HashMap;
use std::rc::Rc;

use super::cmap::{UnicodeMap, add_cid2unichr};
use super::object::{Obj, Result, other};

/// A BytesIO over the font file; short reads make `struct.unpack` fail.
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn read(&mut self, n: usize) -> Result<&[u8]> {
        let start = self.pos.min(self.data.len());
        let end = start.saturating_add(n).min(self.data.len());
        self.pos = self.pos.max(end);
        if end - start < n {
            return other("struct.error: unpack requires a buffer of more bytes");
        }
        Ok(&self.data[start..end])
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.read(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.read(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// An insertion-ordered dict of char code to glyph id.
#[derive(Default)]
struct Char2Gid {
    order: Vec<(u32, i64)>,
    index: HashMap<u32, usize>,
}

impl Char2Gid {
    fn set(&mut self, c: u32, gid: i64) {
        match self.index.get(&c) {
            Some(&i) => self.order[i].1 = gid,
            None => {
                self.index.insert(c, self.order.len());
                self.order.push((c, gid));
            }
        }
    }
}

/// TrueTypeFont.create_unicode_map; Ok(None) is CMapNotFound.
pub fn unicode_map(data: &[u8]) -> Result<Option<UnicodeMap>> {
    let mut fp = Cursor { data, pos: 0 };
    let _ = fp.read(4);
    let mut tables: HashMap<Vec<u8>, u32> = HashMap::new();
    if let Ok(h) = fp.read(8) {
        let ntables = u16::from_be_bytes([h[0], h[1]]);
        for _ in 0..ntables {
            let Ok(t) = fp.read(16) else {
                break;
            };
            tables.insert(
                t[..4].to_vec(),
                u32::from_be_bytes([t[8], t[9], t[10], t[11]]),
            );
        }
    }
    let Some(&base) = tables.get(&b"cmap"[..]) else {
        return Ok(None);
    };
    fp.pos = base as usize;
    fp.u16()?;
    let nsub = fp.u16()?;
    let mut subtables = Vec::new();
    for _ in 0..nsub {
        subtables.push((fp.u16()?, fp.u16()?, fp.u32()?));
    }
    let mut c2g = Char2Gid::default();
    const CAP: u64 = 1 << 21;
    for (pid, eid, off) in subtables {
        if !(pid == 0 || (pid == 3 && (eid == 1 || eid == 10))) {
            continue;
        }
        fp.pos = base as usize + off as usize;
        match fp.u16()? {
            0 => {
                fp.read(4)?;
                let gids = fp.read(256)?.to_vec();
                for (c, g) in gids.into_iter().enumerate() {
                    c2g.set(c as u32, g as i64);
                }
            }
            2 => {
                fp.read(4)?;
                let mut keys = Vec::with_capacity(256);
                for _ in 0..256 {
                    keys.push(fp.u16()?);
                }
                let mut firstbytes = vec![0u32; 8192];
                for (i, &k) in keys.iter().enumerate() {
                    firstbytes[(k / 8) as usize] = i as u32;
                }
                let nhdrs = keys.iter().max().copied().unwrap_or(0) / 8 + 1;
                let mut hdrs = Vec::new();
                for i in 0..nhdrs {
                    let (firstcode, entcount, delta, offset) =
                        (fp.u16()?, fp.u16()?, fp.i16()?, fp.u16()?);
                    hdrs.push((i, firstcode, entcount, delta, fp.pos - 2 + offset as usize));
                }
                for (i, firstcode, entcount, delta, pos) in hdrs {
                    if entcount == 0 {
                        continue;
                    }
                    let first = firstcode as u32 + (firstbytes[i as usize] << 8);
                    fp.pos = pos;
                    for c in 0..entcount as u32 {
                        let mut gid = fp.u16()? as i64;
                        if gid != 0 {
                            gid += delta as i64;
                        }
                        c2g.set(first + c, gid);
                    }
                }
            }
            4 => {
                fp.read(4)?;
                let segcount = (fp.u16()? / 2) as usize;
                fp.read(6)?;
                let read = |fp: &mut Cursor| -> Result<Vec<u16>> {
                    (0..segcount).map(|_| fp.u16()).collect()
                };
                let ecs = read(&mut fp)?;
                fp.read(2)?;
                let scs = read(&mut fp)?;
                let idds: Vec<i16> = read(&mut fp)?.into_iter().map(|v| v as i16).collect();
                let pos = fp.pos;
                let idrs = read(&mut fp)?;
                for (((ec, sc), idd), idr) in ecs.into_iter().zip(scs).zip(idds).zip(idrs) {
                    if idr != 0 {
                        fp.pos = pos + idr as usize;
                        for c in sc as u32..=ec as u32 {
                            let b = fp.u16()? as i64;
                            c2g.set(c, (b + idd as i64) & 0xffff);
                        }
                    } else {
                        for c in sc as u32..=ec as u32 {
                            c2g.set(c, (c as i64 + idd as i64) & 0xffff);
                        }
                    }
                }
            }
            6 => {
                fp.read(4)?;
                let (firstcode, entcount) = (fp.u16()? as u32, fp.u16()?);
                for i in 0..entcount as u32 {
                    let g = fp.u16()? as i64;
                    c2g.set(firstcode + i, g);
                }
            }
            10 => {
                fp.read(10)?;
                let (start, n) = (fp.u32()?, fp.u32()?);
                if fp.data.len() < fp.pos + 2 * n as usize {
                    return other("struct.error: unpack requires a buffer of more bytes");
                }
                for i in 0..n {
                    let g = fp.u16()? as i64;
                    c2g.set(start.wrapping_add(i), g);
                }
            }
            12 => {
                fp.read(10)?;
                let groups = fp.u32()?;
                for _ in 0..groups {
                    let (sc, ec, mut sgid) = (fp.u32()?, fp.u32()?, fp.u32()? as i64);
                    for code in (sc as u64..=ec as u64).take(CAP as usize) {
                        c2g.set(code as u32, sgid);
                        sgid += 1;
                    }
                }
            }
            f => return other(format!("AssertionError: ('Unhandled', {f})")),
        }
    }
    if c2g.order.is_empty() {
        return Ok(None);
    }
    let mut map = HashMap::new();
    for (c, gid) in c2g.order {
        add_cid2unichr(&mut map, gid, &Obj::Int(c as i64))?;
    }
    Ok(Some(UnicodeMap::File(Rc::new(map))))
}
