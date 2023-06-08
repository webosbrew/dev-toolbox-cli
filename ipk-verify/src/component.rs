use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::Path;

use path_slash::CowExt;
use serde::Deserialize;

use common::{BinaryInfo, BinVerifyResult, Firmware, LibraryInfo, VerifyWithFirmware};

use crate::{Component, ComponentVerifyResult, Symlinks};

#[derive(Debug, Deserialize)]
struct AppInfo {
    pub id: String,
    pub r#type: String,
    pub main: String,
}

#[derive(Debug, Deserialize)]
struct ServiceInfo {
    pub id: String,
    pub engine: Option<String>,
    pub executable: Option<String>,
}

impl Component {
    pub fn parse_app<P: AsRef<Path>>(dir: P, links: &Symlinks) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let info: AppInfo = serde_json::from_reader(File::open(dir.join("appinfo.json"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad appinfo.json: {e:?}")))?;
        if info.r#type != "native" {
            return Ok(Self {
                id: info.id,
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let libs = Self::list_libs(dir, links)?;
        let exe_path = dir.join(Cow::from_slash(&info.main));
        return Ok(Self {
            id: info.id,
            exe: Some(
                BinaryInfo::parse(
                    File::open(&exe_path)?,
                    exe_path.file_name().unwrap().to_string_lossy(),
                )
                .map_err(|e| {
                    Error::new(
                        ErrorKind::InvalidData,
                        format!("Bad app executable {}: {e:?}", info.main),
                    )
                })?,
            ),
            libs,
        });
    }

    pub fn parse_service<P: AsRef<Path>>(dir: P, links: &Symlinks) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let info: ServiceInfo = serde_json::from_reader(File::open(dir.join("services.json"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad appinfo.json: {e:?}")))?;
        let executable = if let Some(executable) = info.executable {
            executable
        } else {
            return Ok(Self {
                id: info.id,
                exe: Default::default(),
                libs: Default::default(),
            });
        };
        if info.engine.as_deref() != Some("native") {
            return Ok(Self {
                id: info.id,
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let libs = Self::list_libs(dir, links)?;
        let exe_path = dir.join(Cow::from_slash(&executable));
        return Ok(Self {
            id: info.id,
            exe: Some(
                BinaryInfo::parse(
                    File::open(dir.join(&exe_path))?,
                    exe_path.file_name().unwrap().to_string_lossy(),
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

impl Component {
    fn find_lib(&self, name: &str) -> Option<&'_ LibraryInfo> {
        return self.libs.iter().find(|lib| lib.has_name(name));
    }

    fn is_required(&self, lib: &LibraryInfo) -> bool {
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
}

impl Component {
    fn verify_bin(&self, bin: &BinaryInfo, firmware: &Firmware) -> BinVerifyResult {
        let mut result = bin.verify(firmware);
        result.missing_lib.retain_mut(|lib| {
            if let Some(lib) = self.find_lib(lib) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
                return false;
            }
            return true;
        });
        return result;
    }
}

impl VerifyWithFirmware<ComponentVerifyResult> for Component {
    fn verify(&self, firmware: &Firmware) -> ComponentVerifyResult {
        let exe = if let Some(bin) = &self.exe {
            self.verify_bin(bin, firmware)
        } else {
            return ComponentVerifyResult {
                id: self.id.clone(),
                exe: None,
                libs: Default::default(),
            };
        };
        let mut libs: Vec<(bool, BinVerifyResult)> = self
            .libs
            .iter()
            .map(|lib| {
                (
                    self.is_required(lib),
                    self.verify_bin(
                        &BinaryInfo {
                            name: lib.name.clone(),
                            rpath: Default::default(),
                            needed: lib.needed.clone(),
                            undefined: lib.undefined.clone(),
                        },
                        firmware,
                    ),
                )
            })
            .collect();
        libs.sort_by(|(required_a, lib_a), (required_b, lib_b)| {
            let required_cmp = required_a.cmp(required_b);
            if !required_cmp.is_eq() {
                return required_cmp.reverse();
            }
            return lib_a.name.cmp(&lib_b.name);
        });
        return ComponentVerifyResult {
            id: self.id.clone(),
            exe: Some(exe),
            libs,
        };
    }
}
