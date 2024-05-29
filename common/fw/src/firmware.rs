use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind};
use std::path::{Path, PathBuf};

use bin_lib::LibraryInfo;

use crate::{Firmware, FirmwareInfo, ReleaseCodename};

impl FirmwareInfo {
    pub fn codename(&self) -> Option<ReleaseCodename> {
        return match self.release.major {
            1 => Some(ReleaseCodename::Afro),
            2 => Some(ReleaseCodename::Beehive),
            3 => Some(if self.release.minor >= 5 {
                ReleaseCodename::Dreadlocks
            } else {
                ReleaseCodename::Dreadlocks2
            }),
            4 => Some(if self.release.minor >= 5 {
                ReleaseCodename::Goldilocks
            } else {
                ReleaseCodename::Goldilocks2
            }),
            5 => Some(ReleaseCodename::Jhericurl),
            6 => Some(ReleaseCodename::Kisscurl),
            7 => Some(ReleaseCodename::Mullet),
            8 => Some(ReleaseCodename::Number1),
            _ => None,
        };
    }
}

impl Display for FirmwareInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Firmware {}, webOS {}, OTA ID: {}",
            self.version, self.release, self.ota_id
        ))
    }
}

impl Firmware {
    pub fn find_library(&self, name: &str) -> Option<LibraryInfo> {
        if let Some(lib_name) = self.index.get(name) {
            let path = self.path.join(lib_name);
            return File::open(path)
                .and_then(|file| {
                    return serde_json::from_reader(BufReader::new(file)).map_err(|e| {
                        Error::new(ErrorKind::InvalidData, format!("Bad library info: {e:?}"))
                    });
                })
                .ok();
        }
        return None;
    }

    pub fn load<P>(path: P) -> Result<Firmware, Error>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        let index: HashMap<String, String> =
            File::open(path.join("index.json")).and_then(|file| {
                return serde_json::from_reader(BufReader::new(file)).map_err(|e| {
                    Error::new(ErrorKind::InvalidData, format!("Bad firmware index: {e:?}"))
                });
            })?;
        let info: FirmwareInfo = File::open(path.join("info.json")).and_then(|file| {
            return serde_json::from_reader(BufReader::new(file)).map_err(|e| {
                Error::new(ErrorKind::InvalidData, format!("Bad firmware info: {e:?}"))
            });
        })?;

        return Ok(Firmware {
            path: path.to_path_buf(),
            info,
            index,
        });
    }

    pub fn list<P>(data_path: P) -> Result<Vec<Firmware>, Error>
    where
        P: AsRef<Path>,
    {
        let mut firmwares: Vec<Firmware> = data_path
            .as_ref()
            .read_dir()
            .map_err(|e| {
                Error::new(
                    e.kind(),
                    format!(
                        "Failed to open data directory {}: {e}",
                        data_path.as_ref().to_string_lossy()
                    ),
                )
            })?
            .filter_map(|ent| {
                if let Ok(ent) = ent {
                    return Firmware::load(ent.path()).ok();
                }
                return None;
            })
            .collect();
        firmwares.sort_by(|a, b| a.info.release.cmp(&b.info.release));
        return Ok(firmwares);
    }

    pub fn data_path() -> PathBuf {
        return if cfg!(feature = "linux-install") {
            PathBuf::from("/usr/share/webosbrew/compat-checker/data")
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("data").canonicalize().unwrap()
        };
    }
}
