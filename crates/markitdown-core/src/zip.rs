//! Read-only access to the parts of an OPC (zip) package, within the
//! conversion's budget of decompressed bytes.

use crate::{Error, Options};
use std::cell::Cell;
use std::io::{Cursor, Read};

/// Decompressed bytes one conversion may read from archives, nested ones
/// and Office packages included (MARKITDOWN_MAX_UNZIPPED_BYTES).
const DEFAULT_MAX_UNZIPPED_BYTES: u64 = 1 << 30;

thread_local! {
    /// bytes left for the conversion running on this thread
    static BUDGET: Cell<Option<(u64, u64)>> = const { Cell::new(None) };
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

/// Run a conversion with a fresh byte budget, unless one is running
/// already (a nested conversion shares its parent's).
pub fn with_budget<T>(opts: &Options, f: impl FnOnce() -> T) -> T {
    if BUDGET.get().is_some() {
        return f();
    }
    let limit = opts
        .max_unzipped_bytes
        .unwrap_or_else(|| env_or("MARKITDOWN_MAX_UNZIPPED_BYTES", DEFAULT_MAX_UNZIPPED_BYTES));
    BUDGET.set(Some((limit, limit)));
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            BUDGET.set(None);
        }
    }
    let _reset = Reset;
    f()
}

fn over_budget(limit: u64) -> Error {
    Error::Limit(format!(
        "archive contents exceed {limit} bytes (raise MARKITDOWN_MAX_UNZIPPED_BYTES to allow more)"
    ))
}

/// Read one entry, charging what it decompresses to against the budget.
fn read_entry(
    mut f: ::zip::read::ZipFile<'_, Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, Error> {
    let (left, limit) = BUDGET.get().unwrap_or((u64::MAX, u64::MAX));
    if f.size() > left {
        return Err(over_budget(limit));
    }
    let mut buf = Vec::with_capacity(f.size() as usize);
    // the declared size may lie: never read past what is left
    (&mut f)
        .take(left.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| Error::Zip(format!("{name}: {e}")))?;
    let used = buf.len() as u64;
    if used > left {
        return Err(over_budget(limit));
    }
    if BUDGET.get().is_some() {
        BUDGET.set(Some((left - used, limit)));
    }
    Ok(buf)
}

pub struct Archive<'a> {
    inner: ::zip::ZipArchive<Cursor<&'a [u8]>>,
}

impl<'a> Archive<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, Error> {
        let inner =
            ::zip::ZipArchive::new(Cursor::new(data)).map_err(|e| Error::Zip(e.to_string()))?;
        Ok(Archive { inner })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.inner.index_for_name(name).is_some()
    }

    /// Entry names in central-directory order.
    pub fn names(&self) -> Vec<String> {
        (0..self.inner.len())
            .filter_map(|i| {
                self.inner
                    .name_for_index(i)
                    .and_then(Result::ok)
                    .map(|n| n.into_owned())
            })
            .collect()
    }

    pub fn read_index(&mut self, i: usize) -> Result<Vec<u8>, Error> {
        let f = self
            .inner
            .by_index(i)
            .map_err(|e| Error::Zip(e.to_string()))?;
        let name = format!("entry {i}");
        read_entry(f, &name)
    }

    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, Error> {
        let Some(i) = self.inner.index_for_name(name) else {
            return Ok(None);
        };
        let f = self
            .inner
            .by_index(i)
            .map_err(|e| Error::Zip(e.to_string()))?;
        read_entry(f, name).map(Some)
    }
}
