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
    pub name: String,
    pub rpath: Vec<String>,
    pub needed: Vec<String>,
    pub undefined: Vec<String>,
}

pub trait VerifyResult {
    fn is_good(&self) -> bool;
}

#[derive(Debug)]
pub struct BinVerifyResult {
    pub name: String,
    pub missing_lib: Vec<String>,
    pub undefined_sym: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub name: String,
    pub needed: Vec<String>,
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
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

pub trait VerifyWithFirmware<R> {
    fn verify(&self, firmware: &Firmware) -> R;
}
