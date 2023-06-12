use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use bin_lib::{BinaryInfo, LibraryInfo};

mod component;
mod ipk;
mod links;

#[derive(Debug)]
pub struct Package {
    pub id: String,
    pub app: Component<AppInfo>,
    pub services: Vec<Component<ServiceInfo>>,
}

#[derive(Debug)]
pub struct Component<T> {
    pub id: String,
    pub info: T,
    pub exe: Option<BinaryInfo>,
    pub libs: Vec<LibraryInfo>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PackageInfo {
    app: String,
    #[serde(default)]
    services: Vec<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppInfo {
    pub id: String,
    pub r#type: String,
    pub main: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceInfo {
    pub id: String,
    pub engine: Option<String>,
    pub executable: Option<String>,
}

#[derive(Debug)]
pub(crate) struct Symlinks {
    pub(crate) mapping: HashMap<PathBuf, PathBuf>,
}
