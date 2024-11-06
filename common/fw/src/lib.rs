use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub mod firmware;
pub(crate) mod version;
use version::version_deserialize;
use version::version_serialize;

#[derive(Debug, Serialize, Deserialize)]
pub struct FirmwareInfo {
    pub version: String,
    pub ota_id: String,
    #[serde(
        serialize_with = "version_serialize",
        deserialize_with = "version_deserialize"
    )]
    pub release: Version,
}

#[derive(Debug)]
pub struct Firmware {
    pub info: FirmwareInfo,
    path: PathBuf,
    index: HashMap<String, String>,
}

pub enum ReleaseCodename {
    Afro,
    Beehive,
    Dreadlocks,
    Dreadlocks2,
    Goldilocks,
    Goldilocks2,
    Jhericurl,
    Kisscurl,
    Mullet,
    Number1,
    Ombre,
}
