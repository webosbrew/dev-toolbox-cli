use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

pub mod binary;
pub mod firmware;
pub mod library;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub rpath: Vec<String>,
    pub needed: Vec<String>,
    pub undefined: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub needed: Vec<String>,
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub undefined: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FirmwareInfo {
    pub version: String,
    pub ota_id: String,
    pub release: String,
}

#[derive(Debug)]
pub struct Firmware {
    pub info: FirmwareInfo,
    path: PathBuf,
    index: HashMap<String, String>,
}
