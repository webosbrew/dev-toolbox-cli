use crate::Application;
use common::{BinaryInfo, LibraryInfo};
use serde::Deserialize;
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::Path;

#[derive(Debug, Deserialize)]
struct AppInfo {
    pub r#type: String,
    pub main: String,
}

impl Application {
    pub fn parse<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let path = path.as_ref();
        let appinfo: AppInfo = serde_json::from_reader(File::open(path.join("appinfo.json"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad appinfo.json: {e:?}")))?;
        if appinfo.r#type != "native" {
            return Ok(Self {
                main: None,
                libs: vec![],
            });
        }
        let mut libs = Vec::new();
        if let Ok(entries) = fs::read_dir(path.join("lib")) {
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                if let Ok(lib) = LibraryInfo::parse(File::open(entry.path())?, true) {
                    libs.push(lib);
                }
            }
        }
        return Ok(Self {
            main: Some(
                BinaryInfo::parse(File::open(path.join(&appinfo.main))?).map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidData,
                        format!("Bad app executable {}: {e:?}", appinfo.main),
                    )
                })?,
            ),
            libs,
        });
    }
}
