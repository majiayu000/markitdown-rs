//! OLE2 compound files read as olefile 0.47 reads them with its default
//! `raise_defects=DEFECT_FATAL`, which is how markitdown opens Outlook
//! messages: only fatal defects (no header, a FAT or directory sector that
//! is not in the file) fail; a damaged or truncated stream reads as the
//! sectors that are present.

use std::cell::OnceCell;

const MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";
const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
const FREESECT: u32 = 0xFFFF_FFFF;
const NOSTREAM: u32 = 0xFFFF_FFFF;
const STGTY_STORAGE: u8 = 1;
const STGTY_STREAM: u8 = 2;
/// olefile forces the mini stream cutoff to 4096
const MINI_CUTOFF: u64 = 0x1000;
const MINI_SECTOR: u64 = 64;
/// where Python's recursion limit would stop olefile's directory walk
const MAX_TREE_DEPTH: usize = 900;

#[derive(Debug, Clone)]
struct Entry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
    /// children, sorted by name
    kids: Vec<usize>,
    used: bool,
}

pub struct Ole<'a> {
    data: &'a [u8],
    sector: u64,
    fat: Vec<u32>,
    entries: Vec<Option<Entry>>,
    first_minifat: u32,
    num_minifat: u64,
    mini: OnceCell<Result<MiniStream, String>>,
}

/// the MiniFAT and the mini stream's bytes
type MiniStream = (Vec<u32>, Vec<u8>);

fn u32s(b: &[u8]) -> Result<Vec<u32>, String> {
    if !b.len().is_multiple_of(4) {
        return Err("bytes length not a multiple of item size".into());
    }
    Ok(b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect())
}

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// `OleStream`: follow the chain from `sect`; sectors past the end of `fp`
/// read short or empty, and the result is cut to `size` when known.
fn read_chain(
    fp: &[u8],
    offset: u64,
    sector: u64,
    fat: &[u32],
    mut sect: u32,
    size: Option<u64>,
) -> Vec<u8> {
    let size_or_all = size.unwrap_or(fat.len() as u64 * sector);
    let nb_sectors = size_or_all.div_ceil(sector);
    let mut data = Vec::new();
    for _ in 0..nb_sectors {
        if sect == ENDOFCHAIN && size.is_none() {
            break;
        }
        if sect as usize >= fat.len() {
            break;
        }
        let start = (offset + sector * sect as u64).min(fp.len() as u64) as usize;
        let end = (start as u64 + sector).min(fp.len() as u64) as usize;
        data.extend_from_slice(&fp[start..end]);
        sect = fat[sect as usize];
    }
    if let Some(size) = size {
        data.truncate(size as usize);
    }
    data
}

impl<'a> Ole<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 512 || &data[..8] != MAGIC {
            return Err("not an OLE2 structured storage file".into());
        }
        let shift = u16_at(data, 30);
        if !(7..=16).contains(&shift) {
            return Err("incorrect sector_size in OLE header".into());
        }
        let sector = 1u64 << shift;
        let num_fat = u32_at(data, 44) as i64;
        let first_dir = u32_at(data, 48);
        let first_minifat = u32_at(data, 60);
        let num_minifat = u32_at(data, 64) as u64;
        let first_difat = u32_at(data, 68);
        let num_difat = u32_at(data, 72) as i64;
        let nb_sect = (data.len() as u64).div_ceil(sector) as i64 - 1;
        let getsect = |s: u32| -> Result<&[u8], String> {
            let start = sector * (s as u64 + 1);
            data.get(start as usize..(start + sector) as usize)
                .ok_or_else(|| "incomplete OLE sector".to_string())
        };
        let mut fat = Vec::new();
        let load = |ids: &[u32], fat: &mut Vec<u32>| -> Result<(), String> {
            for &s in ids {
                if s == ENDOFCHAIN || s == FREESECT {
                    break;
                }
                fat.extend(u32s(getsect(s)?)?);
            }
            Ok(())
        };
        load(&u32s(&data[76..512])?, &mut fat)?;
        if num_difat != 0 {
            if first_difat as i64 >= nb_sect {
                return Err("incorrect DIFAT, first index out of range".into());
            }
            let per = (sector / 4) as i64 - 1;
            let nb_difat = (num_fat - 109 + per - 1).div_euclid(per);
            if num_difat != nb_difat {
                return Err("incorrect DIFAT".into());
            }
            let mut s = first_difat;
            for _ in 0..nb_difat.max(0) {
                let difat = u32s(getsect(s)?)?;
                load(&difat[..per as usize], &mut fat)?;
                s = difat[per as usize];
            }
            if s != ENDOFCHAIN && s != FREESECT {
                return Err("incorrect end of DIFAT".into());
            }
        }
        // a FAT longer than the file is cut to the file's sectors
        if fat.len() as i64 > nb_sect {
            fat.truncate(nb_sect.max(0) as usize);
        }
        let dir = read_chain(data, sector, sector, &fat, first_dir, None);
        let max_entries = dir.len() / 128;
        let mut ole = Ole {
            data,
            sector,
            fat,
            entries: vec![None; max_entries],
            first_minifat,
            num_minifat,
            mini: OnceCell::new(),
        };
        if max_entries == 0 {
            return Err("OLE directory index out of range".into());
        }
        ole.load_entry(&dir, 0, sector);
        let kids = ole.append_kids(&dir, ole.entry(0).child, 0)?;
        ole.set_kids(0, kids);
        Ok(ole)
    }

    fn entry(&self, sid: usize) -> &Entry {
        self.entries[sid].as_ref().expect("loaded entry")
    }

    fn load_entry(&mut self, dir: &[u8], sid: usize, sector: u64) {
        if self.entries[sid].is_some() {
            return;
        }
        let e = &dir[sid * 128..sid * 128 + 128];
        let namelength = (u16_at(e, 64) as usize).min(64);
        let raw = &e[..namelength.saturating_sub(2)];
        let units: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        let (low, high) = (u32_at(e, 120) as u64, u32_at(e, 124) as u64);
        self.entries[sid] = Some(Entry {
            name: String::from_utf16_lossy(&units),
            kind: e[66],
            left: u32_at(e, 68),
            right: u32_at(e, 72),
            child: u32_at(e, 76),
            start: u32_at(e, 116),
            size: if sector == 512 {
                low
            } else {
                low + (high << 32)
            },
            kids: Vec::new(),
            used: false,
        });
    }

    fn set_kids(&mut self, sid: usize, mut kids: Vec<usize>) {
        kids.sort_by(|&a, &b| self.entry(a).name.cmp(&self.entry(b).name));
        self.entries[sid].as_mut().unwrap().kids = kids;
    }

    /// `append_kids` + `build_storage_tree`: the red-black tree below a
    /// storage, in order; bad or repeated references are skipped.
    fn append_kids(&mut self, dir: &[u8], sid: u32, depth: usize) -> Result<Vec<usize>, String> {
        if sid == NOSTREAM {
            return Ok(Vec::new());
        }
        if depth > MAX_TREE_DEPTH {
            return Err("maximum recursion depth exceeded".into());
        }
        let sid = sid as usize;
        if sid >= self.entries.len() {
            return Ok(Vec::new());
        }
        self.load_entry(dir, sid, self.sector);
        if self.entry(sid).used {
            return Ok(Vec::new());
        }
        self.entries[sid].as_mut().unwrap().used = true;
        let (left, right, child) = {
            let e = self.entry(sid);
            (e.left, e.right, e.child)
        };
        let mut out = self.append_kids(dir, left, depth + 1)?;
        out.push(sid);
        out.extend(self.append_kids(dir, right, depth + 1)?);
        let kids = self.append_kids(dir, child, depth + 1)?;
        self.set_kids(sid, kids);
        Ok(out)
    }

    /// `_find`: a case-insensitive path below the root.
    fn find(&self, path: &str) -> Option<usize> {
        let mut node = 0;
        for part in path.split('/') {
            let part = part.to_lowercase();
            node = *self
                .entry(node)
                .kids
                .iter()
                .find(|&&k| self.entry(k).name.to_lowercase() == part)?;
        }
        Some(node)
    }

    /// The paths of all streams, as `listdir()` gives them.
    pub fn stream_paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![(0usize, String::new())];
        while let Some((sid, prefix)) = stack.pop() {
            for &k in self.entry(sid).kids.iter().rev() {
                let e = self.entry(k);
                let path = if prefix.is_empty() {
                    e.name.clone()
                } else {
                    format!("{prefix}/{}", e.name)
                };
                match e.kind {
                    STGTY_STORAGE => stack.push((k, path)),
                    STGTY_STREAM => out.push(path),
                    _ => {}
                }
            }
        }
        out
    }

    fn mini(&self) -> Result<&MiniStream, String> {
        self.mini
            .get_or_init(|| {
                let root = self.entry(0);
                let nb_minisectors = root.size.div_ceil(MINI_SECTOR) as usize;
                let stream = read_chain(
                    self.data,
                    self.sector,
                    self.sector,
                    &self.fat,
                    self.first_minifat,
                    Some(self.num_minifat * self.sector),
                );
                let mut minifat = u32s(&stream)?;
                minifat.truncate(nb_minisectors);
                let ministream = read_chain(
                    self.data,
                    self.sector,
                    self.sector,
                    &self.fat,
                    root.start,
                    Some(root.size),
                );
                Ok((minifat, ministream))
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    /// `openstream(path).read()`, or None when there is no such stream or
    /// it cannot be read.
    pub fn read_stream(&self, path: &str) -> Option<Vec<u8>> {
        let e = self.entry(self.find(path)?);
        if e.kind != STGTY_STREAM {
            return None;
        }
        if e.size < MINI_CUTOFF {
            let (minifat, ministream) = self.mini().ok()?;
            Some(read_chain(
                ministream,
                0,
                MINI_SECTOR,
                minifat,
                e.start,
                Some(e.size),
            ))
        } else {
            Some(read_chain(
                self.data,
                self.sector,
                self.sector,
                &self.fat,
                e.start,
                Some(e.size),
            ))
        }
    }
}
