use crate::Service;
use std::io::Error;
use std::path::Path;

impl Service {
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        return Ok(Self {});
    }
}
