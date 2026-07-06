use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

use bin_lib::{BinaryInfo, LibraryInfo};
use webdetect_lib::{ServiceRuntimeDetection, WebAppDetection};

mod component;
mod ipk;
mod links;
mod path;

#[derive(Debug)]
pub struct Package {
    pub id: String,
    pub installed_size: Option<u64>,
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
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub id: String,
    pub version: String,
    pub r#type: String,
    pub title: String,
    pub app_description: Option<String>,
    pub main: String,
    /// Web/frontend technology detected for non-native apps (filled at parse
    /// time; not part of appinfo.json).
    #[serde(skip)]
    pub web: Option<WebAppDetection>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceInfo {
    pub id: String,
    pub engine: Option<String>,
    pub executable: Option<String>,
    /// Node.js runtime detected for non-native services (filled at parse time;
    /// not part of services.json).
    #[serde(skip)]
    pub runtime: Option<ServiceRuntimeDetection>,
}

#[derive(Debug)]
pub(crate) struct Symlinks {
    pub(crate) mapping: HashMap<PathBuf, PathBuf>,
}
