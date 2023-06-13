use serde::{Deserialize, Serialize};

pub mod binary;
pub mod library;

#[derive(Debug, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub name: String,
    pub rpath: Vec<String>,
    pub needed: Vec<String>,
    pub undefined: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub name: String,
    pub needed: Vec<String>,
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub undefined: Vec<String>,
}
