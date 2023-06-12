use std::io::Error;
use std::path::Path;

use crate::IpkHash;

impl IpkHash {
    pub(crate) fn from<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        return Ok(Self {
            sha256: sha256::try_digest(path.as_ref())?,
        });
    }
}
