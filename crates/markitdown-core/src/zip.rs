//! Read-only access to the parts of an OPC (zip) package.

use crate::Error;
use std::io::{Cursor, Read};

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
        let mut f = self
            .inner
            .by_index(i)
            .map_err(|e| Error::Zip(e.to_string()))?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .map_err(|e| Error::Zip(e.to_string()))?;
        Ok(buf)
    }

    pub fn read(&mut self, name: &str) -> Result<Option<Vec<u8>>, Error> {
        let Some(i) = self.inner.index_for_name(name) else {
            return Ok(None);
        };
        let mut f = self
            .inner
            .by_index(i)
            .map_err(|e| Error::Zip(e.to_string()))?;
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf)
            .map_err(|e| Error::Zip(format!("{name}: {e}")))?;
        Ok(Some(buf))
    }
}
