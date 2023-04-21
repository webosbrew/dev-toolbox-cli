use serde::{Deserialize, Serialize};

pub mod bin_info;
pub mod lib_info;
pub mod fw_info;

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FirmwareInfo {
    pub version: String,
    pub ota_id: String,
    pub release: String,
}
