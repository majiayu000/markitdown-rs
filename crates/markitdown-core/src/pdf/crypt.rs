//! pdfminer's standard security handlers (V1/V2 RC4, V4 RC4/AESV2,
//! V5 AESV3), authenticated with the empty password as markitdown does.

use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use aes::{Aes128, Aes192, Aes256, Block};
use md5::{Digest, Md5};
use sha2::{Sha256, Sha384, Sha512};

use super::document::Document;
use super::object::{Dict, Obj, Result, literal_name, other};

const PADDING: &[u8; 32] =
    b"(\xbfN^Nu\x8aAd\x00NV\xff\xfa\x01\x08..\x00\xb6\xd0h>\x80/\x0c\xa9\xfedSiz";

#[derive(Clone, Copy, Debug)]
enum Method {
    Rc4,
    Aes128,
    Aes256,
    Identity,
}

pub struct Security {
    key: Vec<u8>,
    method: Method,
    /// V4 and later look at /EncryptMetadata and the stream's /Type.
    crypt_filters: bool,
    encrypt_metadata: bool,
}

fn md5(parts: &[&[u8]]) -> Vec<u8> {
    let mut h = Md5::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().to_vec()
}

fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: Vec<u8> = (0..=255).collect();
    let mut j = 0usize;
    for i in 0..256 {
        j = (j + s[i] as usize + key[i % key.len()] as usize) % 256;
        s.swap(i, j);
    }
    let (mut i, mut j) = (0usize, 0usize);
    data.iter()
        .map(|&c| {
            i = (i + 1) % 256;
            j = (j + s[i] as usize) % 256;
            s.swap(i, j);
            c ^ s[(s[i] as usize + s[j] as usize) % 256]
        })
        .collect()
}

enum Aes {
    K128(Aes128),
    K192(Aes192),
    K256(Aes256),
}

impl Aes {
    fn new(key: &[u8]) -> Result<Self> {
        Ok(match key.len() {
            16 => Aes::K128(Aes128::new_from_slice(key).unwrap()),
            24 => Aes::K192(Aes192::new_from_slice(key).unwrap()),
            32 => Aes::K256(Aes256::new_from_slice(key).unwrap()),
            n => return other(format!("Invalid key size ({}) for AES.", n * 8)),
        })
    }

    fn block(&self, b: &mut Block, encrypt: bool) {
        match (self, encrypt) {
            (Aes::K128(c), true) => c.encrypt_block(b),
            (Aes::K128(c), false) => c.decrypt_block(b),
            (Aes::K192(c), true) => c.encrypt_block(b),
            (Aes::K192(c), false) => c.decrypt_block(b),
            (Aes::K256(c), true) => c.encrypt_block(b),
            (Aes::K256(c), false) => c.decrypt_block(b),
        }
    }
}

/// CBC without finalize: only whole blocks come out.
fn aes_cbc(key: &[u8], iv: &[u8], data: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    let cipher = Aes::new(key)?;
    if iv.len() != 16 {
        return other(format!("Invalid IV size ({}) for CBC.", iv.len()));
    }
    let mut prev = Block::clone_from_slice(iv);
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut b = Block::clone_from_slice(chunk);
        if encrypt {
            for (x, p) in b.iter_mut().zip(prev.iter()) {
                *x ^= p;
            }
            cipher.block(&mut b, true);
            prev = b;
        } else {
            cipher.block(&mut b, false);
            for (x, p) in b.iter_mut().zip(prev.iter()) {
                *x ^= p;
            }
            prev = Block::clone_from_slice(chunk);
        }
        out.extend_from_slice(&b);
    }
    Ok(out)
}

fn unpad_aes(mut padded: Vec<u8>) -> Vec<u8> {
    let Some(&padding) = padded.last() else {
        return padded;
    };
    let n = padding as usize;
    if padding > 16 || n > padded.len() {
        return padded;
    }
    if padded[padded.len() - n..].iter().all(|&x| x == padding) {
        padded.truncate(padded.len() - n);
    }
    padded
}

/// `struct.pack('<L', v)` raises outside 0..2**32.
fn pack_u32(v: i64) -> Result<[u8; 4]> {
    u32::try_from(v)
        .map(u32::to_le_bytes)
        .or_else(|_| other("argument out of range"))
}

impl Security {
    pub fn new(doc: &Document, docid: &[Obj], param: &Dict) -> Result<Self> {
        if literal_name(param.get("Filter").unwrap_or(&Obj::Null)) != "Standard" {
            return other("Unknown filter");
        }
        let v = doc.int_value(param.get("V").unwrap_or(&Obj::Int(0)))?;
        if !matches!(v, 1 | 2 | 4 | 5) {
            return other("Unknown algorithm");
        }
        let get = |k: &str| {
            param
                .get(k)
                .ok_or_else(|| super::object::PdfError::Other(format!("KeyError: {k}")))
        };
        let r = doc.int_value(get("R")?)?;
        let p = doc.int_value(get("P")?)?;
        let p = if p > 0 { p } else { p + (1 << 32) };
        let o = doc.str_value(get("O")?)?.to_vec();
        let u = doc.str_value(get("U")?)?.to_vec();
        let mut length = doc.int_value(param.get("Length").unwrap_or(&Obj::Int(40)))?;
        let docid0 = match docid.first() {
            Some(Obj::Str(s)) => s.to_vec(),
            Some(_) => return other("Strings must be encoded before hashing"),
            None => return other("tuple index out of range"),
        };
        let mut sec = Security {
            key: Vec::new(),
            method: Method::Rc4,
            crypt_filters: v >= 4,
            encrypt_metadata: true,
        };
        if v >= 4 {
            length = if v == 5 { 256 } else { 128 };
            let cf = doc.dict_value(param.get("CF").unwrap_or(&Obj::Null))?;
            let stmf = literal_name(get("StmF")?);
            let strf = literal_name(get("StrF")?);
            sec.encrypt_metadata = param.get("EncryptMetadata").is_none_or(Obj::truthy);
            if stmf != strf {
                return other("Unsupported crypt filter");
            }
            let mut found = None;
            for (k, f) in &cf.0 {
                let Obj::Dict(f) = f else {
                    return other("crypt filter is not a dict");
                };
                let cfm = literal_name(
                    f.get("CFM")
                        .ok_or_else(|| super::object::PdfError::Other("KeyError: CFM".into()))?,
                );
                let m = match (v, cfm.as_str()) {
                    (4, "V2") => Method::Rc4,
                    (4, "AESV2") => Method::Aes128,
                    (5, "AESV3") => Method::Aes256,
                    _ => return other("Unknown crypt filter method"),
                };
                if **k == *strf.as_bytes() {
                    found = Some(m);
                }
            }
            // pdfminer adds Identity after the /CF entries, replacing any
            sec.method = match found {
                _ if strf == "Identity" => Method::Identity,
                Some(m) => m,
                None => return other("Undefined crypt filter"),
            };
        }
        let supported = match v {
            1 | 2 => matches!(r, 2 | 3),
            4 => r == 4,
            _ => matches!(r, 5 | 6),
        };
        if !supported {
            return other("Unsupported revision");
        }
        let key = if v == 5 {
            v5_key(
                r,
                &o,
                &u,
                &doc.str_value(get("OE")?)?,
                &doc.str_value(get("UE")?)?,
            )?
        } else {
            let k = Rc4Key {
                r,
                p,
                o: &o,
                u: &u,
                length,
                docid0: &docid0,
                meta: sec.encrypt_metadata,
            };
            match k.user(b"")? {
                Some(key) => Some(key),
                None => k.owner(b"")?,
            }
        };
        sec.key =
            key.ok_or_else(|| super::object::PdfError::Other("PDFPasswordIncorrect".into()))?;
        Ok(sec)
    }

    /// `decipher(objid, genno, data, attrs)`
    pub fn decrypt(
        &self,
        objid: i64,
        genno: i64,
        data: &[u8],
        attrs: Option<&Dict>,
    ) -> Result<Vec<u8>> {
        if self.crypt_filters
            && !self.encrypt_metadata
            && let Some(t) = attrs.and_then(|a| a.get("Type"))
            && literal_name(t) == "Metadata"
        {
            return Ok(data.to_vec());
        }
        let salted = |salt: &[u8]| -> Result<Vec<u8>> {
            let mut key = self.key.clone();
            key.extend_from_slice(&pack_u32(objid)?[..3]);
            key.extend_from_slice(&pack_u32(genno)?[..2]);
            key.extend_from_slice(salt);
            let n = key.len().min(16);
            Ok(md5(&[&key])[..n].to_vec())
        };
        match self.method {
            Method::Identity => Ok(data.to_vec()),
            Method::Rc4 => Ok(rc4(&salted(b"")?, data)),
            Method::Aes128 => {
                let key = salted(b"sAlT")?;
                let iv = &data[..data.len().min(16)];
                Ok(unpad_aes(aes_cbc(&key, iv, &data[iv.len()..], false)?))
            }
            Method::Aes256 => {
                let iv = &data[..data.len().min(16)];
                Ok(unpad_aes(aes_cbc(&self.key, iv, &data[iv.len()..], false)?))
            }
        }
    }
}

struct Rc4Key<'a> {
    r: i64,
    p: i64,
    o: &'a [u8],
    u: &'a [u8],
    length: i64,
    docid0: &'a [u8],
    meta: bool,
}

impl Rc4Key<'_> {
    fn n(&self) -> usize {
        if self.r >= 3 {
            (self.length.div_euclid(8)).max(0) as usize
        } else {
            5
        }
    }

    fn padded(password: &[u8]) -> Vec<u8> {
        let mut p = password.to_vec();
        p.extend_from_slice(PADDING);
        p.truncate(32);
        p
    }

    fn compute_key(&self, password: &[u8]) -> Result<Vec<u8>> {
        let p = pack_u32(self.p)?;
        let mut parts: Vec<&[u8]> = vec![&[], self.o, &p, self.docid0];
        let padded = Self::padded(password);
        parts[0] = &padded;
        if self.r >= 4 && !self.meta {
            parts.push(b"\xff\xff\xff\xff");
        }
        let mut result = md5(&parts);
        let n = self.n();
        if self.r >= 3 {
            for _ in 0..50 {
                result = md5(&[&result[..n.min(result.len())]]);
            }
        }
        result.truncate(n);
        Ok(result)
    }

    fn compute_u(&self, key: &[u8]) -> Vec<u8> {
        if key.is_empty() {
            // ZeroDivisionError in Arcfour; treat as a mismatch
            return Vec::new();
        }
        if self.r == 2 {
            return rc4(key, PADDING);
        }
        let mut result = rc4(key, &md5(&[PADDING, self.docid0]));
        for i in 1..20u8 {
            let k: Vec<u8> = key.iter().map(|c| c ^ i).collect();
            result = rc4(&k, &result);
        }
        let mut doubled = result.clone();
        doubled.extend_from_slice(&result);
        doubled
    }

    fn user(&self, password: &[u8]) -> Result<Option<Vec<u8>>> {
        let key = self.compute_key(password)?;
        let u = self.compute_u(&key);
        let ok = if self.r == 2 {
            u == self.u
        } else {
            u[..u.len().min(16)] == self.u[..self.u.len().min(16)]
        };
        Ok(ok.then_some(key))
    }

    fn owner(&self, password: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut hash = md5(&[&Self::padded(password)]);
        if self.r >= 3 {
            for _ in 0..50 {
                hash = md5(&[&hash]);
            }
        }
        let n = self.n();
        let key = &hash[..n.min(hash.len())];
        if key.is_empty() {
            return other("integer division or modulo by zero");
        }
        let user_password = if self.r == 2 {
            rc4(key, self.o)
        } else {
            let mut up = self.o.to_vec();
            for i in (0..20u8).rev() {
                let k: Vec<u8> = key.iter().map(|c| c ^ i).collect();
                up = rc4(&k, &up);
            }
            up
        };
        self.user(&user_password)
    }
}

fn v5_key(r: i64, o: &[u8], u: &[u8], oe: &[u8], ue: &[u8]) -> Result<Option<Vec<u8>>> {
    let sl = |b: &[u8], a: usize, z: usize| b[a.min(b.len())..z.min(b.len())].to_vec();
    let (o_hash, o_vsalt, o_ksalt) = (sl(o, 0, 32), sl(o, 32, 40), sl(o, 40, o.len()));
    let (u_hash, u_vsalt, u_ksalt) = (sl(u, 0, 32), sl(u, 32, 40), sl(u, 40, u.len()));
    let hash = |salt: &[u8], vector: Option<&[u8]>| -> Result<Vec<u8>> {
        if r == 5 {
            let mut h = Sha256::new();
            h.update(salt);
            if let Some(v) = vector {
                h.update(v);
            }
            return Ok(h.finalize().to_vec());
        }
        r6_hash(&salt[..salt.len().min(8)], vector)
    };
    let zero = [0u8; 16];
    if hash(&o_vsalt, Some(u))? == o_hash {
        return Ok(Some(aes_cbc(&hash(&o_ksalt, Some(u))?, &zero, oe, false)?));
    }
    if hash(&u_vsalt, None)? == u_hash {
        return Ok(Some(aes_cbc(&hash(&u_ksalt, None)?, &zero, ue, false)?));
    }
    Ok(None)
}

/// Revision 6 hash of the empty password.
fn r6_hash(salt: &[u8], vector: Option<&[u8]>) -> Result<Vec<u8>> {
    let mut h = Sha256::new();
    h.update(salt);
    if let Some(v) = vector {
        h.update(v);
    }
    let mut k = h.finalize().to_vec();
    let (mut round, mut last) = (0i64, 0i64);
    while round < 64 || last > round - 32 {
        let mut unit = k.clone();
        unit.extend_from_slice(vector.unwrap_or(&[]));
        let k1 = unit.repeat(64);
        let e = aes_cbc(&k[..16], &k[16..32], &k1, true)?;
        let m = e[..16].iter().map(|&b| (b % 3) as u32).sum::<u32>() % 3;
        k = match m {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        last = *e.last().unwrap() as i64;
        round += 1;
    }
    k.truncate(32);
    Ok(k)
}
