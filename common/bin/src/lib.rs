use serde::{Deserialize, Serialize};

pub mod artifact;
pub mod binary;
pub mod library;
mod reloc;

pub use artifact::{ArtifactKind, BundledArtifact};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryInfo {
    pub name: String,
    pub rpath: Vec<String>,
    pub needed: Vec<String>,
    pub undefined: Vec<String>,
    /// Imports the loader binds lazily, on the first call. A missing one of
    /// these does not stop the program from starting, so it is a warning rather
    /// than a failure. See [`crate::reloc::lazy_bound_symbols`].
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub undefined_lazy: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub package: Option<String>,
    pub needed: Vec<String>,
    pub symbols: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub names: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub undefined: Vec<String>,
    /// Imports the loader binds lazily. See [`BinaryInfo::undefined_lazy`].
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub undefined_lazy: Vec<String>,
    #[serde(skip_serializing, default)]
    pub rpath: Vec<String>,
    #[serde(skip_serializing, default = "LibraryPriority::default")]
    pub priority: LibraryPriority,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub enum LibraryPriority {
    Rpath,
    System,
    Package,
}

impl Default for LibraryPriority {
    fn default() -> Self {
        return LibraryPriority::System;
    }
}
