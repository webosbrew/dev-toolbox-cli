use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind};
use std::path::Path;

use crate::ReleaseCodename::{
    Afro, Beehive, Dreadlocks, Dreadlocks2, Goldilocks, Goldilocks2, Jhericurl, Kisscurl, Mullet,
    Number1,
};
use crate::{Firmware, FirmwareInfo, LibraryInfo, ReleaseCodename};

impl FirmwareInfo {
    pub fn codename(&self) -> Option<ReleaseCodename> {
        return match self.release.major {
            1 => Some(Afro),
            2 => Some(Beehive),
            3 => Some(if self.release.minor >= 5 {
                Dreadlocks
            } else {
                Dreadlocks2
            }),
            4 => Some(if self.release.minor >= 5 {
                Goldilocks
            } else {
                Goldilocks2
            }),
            5 => Some(Jhericurl),
            6 => Some(Kisscurl),
            7 => Some(Mullet),
            8 => Some(Number1),
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
            .read_dir()?
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
}
