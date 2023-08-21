use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::Path;

use path_slash::CowExt;

use bin_lib::{BinaryInfo, LibraryInfo};

use crate::{AppInfo, Component, ServiceInfo, Symlinks};

impl AppInfo {
    fn is_native(&self) -> bool {
        return self.r#type == "native";
    }
}

impl ServiceInfo {
    fn is_native(&self) -> bool {
        if let (Some(engine), Some(_)) = (&self.engine, &self.executable) {
            return engine == "native";
        }
        return false;
    }
}

impl Component<AppInfo> {
    pub(crate) fn parse<P: AsRef<Path>>(dir: P, links: &Symlinks) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let info: AppInfo = serde_json::from_reader(
            File::open(dir.join("appinfo.json"))
                .map_err(|e| Error::new(e.kind(), format!("Failed to open appinfo.json: {e}")))?,
        )
        .map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to parse appinfo.json: {e}"),
            )
        })?;
        if !info.is_native() {
            return Ok(Self {
                id: info.id.clone(),
                info,
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let libs = Self::list_libs(dir, links)?;
        let exe_path = dir.join(Cow::from_slash(&info.main));
        return Ok(Self {
            id: info.id.clone(),
            info: info.clone(),
            exe: Some(
                BinaryInfo::parse(
                    File::open(&exe_path).map_err(|e| {
                        Error::new(
                            e.kind(),
                            format!("Failed to open main executable {}: {e}", info.main),
                        )
                    })?,
                    exe_path.file_name().unwrap().to_string_lossy(),
                    exe_path.parent()
                )
                .map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidData,
                        format!("Bad app executable {}: {e}", info.main),
                    )
                })?,
            ),
            libs,
        });
    }
}
impl Component<ServiceInfo> {
    pub(crate) fn parse<P: AsRef<Path>>(dir: P, links: &Symlinks) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let info: ServiceInfo = serde_json::from_reader(File::open(dir.join("services.json"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad appinfo.json: {e:?}")))?;
        if !info.is_native() {
            return Ok(Self {
                id: info.id.clone(),
                info: info.clone(),
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let executable = info.executable.as_ref().unwrap();
        let libs = Self::list_libs(dir, links)?;
        let exe_path = dir.join(Cow::from_slash(executable));
        return Ok(Self {
            id: info.id.clone(),
            info: info.clone(),
            exe: Some(
                BinaryInfo::parse(
                    File::open(dir.join(&exe_path))?,
                    exe_path.file_name().unwrap().to_string_lossy(),
                    exe_path.parent()
                )
                .map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidData,
                        format!("Bad app executable {}: {e:?}", executable),
                    )
                })?,
            ),
            libs,
        });
    }
}

impl<T> Component<T> {
    pub fn find_lib(&self, name: &str) -> Option<&'_ LibraryInfo> {
        return self.libs.iter().find(|lib| lib.has_name(name));
    }

    pub fn is_required(&self, lib: &LibraryInfo) -> bool {
        if let Some(exe) = &self.exe {
            if exe
                .needed
                .iter()
                .find(|needed| lib.has_name(needed))
                .is_some()
            {
                return true;
            }
        }
        return false;
    }

    fn list_libs(dir: &Path, links: &Symlinks) -> Result<Vec<LibraryInfo>, Error> {
        let mut libs = HashMap::new();
        if let Ok(entries) = fs::read_dir(dir.join("lib")) {
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let path = entry.path();
                if let Ok(lib) = LibraryInfo::parse(
                    File::open(&path)?,
                    true,
                    path.file_name().unwrap().to_string_lossy(),
                ) {
                    libs.insert(path, lib);
                }
            }
        }
        for (path, lib) in &mut libs {
            lib.names
                .push(String::from(path.file_name().unwrap().to_string_lossy()));
            lib.names.extend(
                links
                    .links(path)
                    .iter()
                    .map(|p| String::from(p.file_name().unwrap().to_string_lossy())),
            );
        }
        Ok(libs.into_values().collect())
    }
}
