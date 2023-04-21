use std::collections::HashMap;
use std::io::{BufReader, Error, ErrorKind};
use std::fs::File;
use std::path::{Path, PathBuf};

use clap::Parser;

use common::{BinaryInfo, LibraryInfo};
use common::FirmwareInfo;

#[derive(Parser, Debug)]
struct Args {
    #[arg(short, long, num_args(1..))]
    executables: Vec<String>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

fn main() {
    let args = Args::parse();
    for executable in args.executables {
        let file = File::open(Path::new(&executable)).unwrap();
        let info = BinaryInfo::parse(file).expect("parse error");
        for firmware in Firmware::list(Path::new("data")).unwrap() {
            info.verify(&firmware);
        };
    }
}

trait VerifyElf {
    fn verify(&self, firmware: &Firmware);
}

impl VerifyElf for BinaryInfo {
    fn verify(&self, firmware: &Firmware) {
        for needed in &self.needed {
            println!("{:?}", firmware.find_library(needed));
        }
    }
}

struct Firmware {
    path: PathBuf,
    info: FirmwareInfo,
    index: HashMap<String, String>,
}

impl Firmware {
    pub fn list<P>(data_path: P) -> Result<Vec<Firmware>, Error> where P: AsRef<Path> {
        return Ok(data_path.as_ref().read_dir()?.filter_map(|ent| {
            if let Ok(ent) = ent {
                let index: HashMap<String, String> = match File::open(ent.path().join("index.json")).and_then(|file| {
                    return serde_json::from_reader(BufReader::new(file))
                        .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad firmware info: {e:?}")));
                }) {
                    Ok(index) => index,
                    Err(_) => return None,
                };
                let info: FirmwareInfo = match File::open(ent.path().join("info.json")).and_then(|file| {
                    return serde_json::from_reader(BufReader::new(file))
                        .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad firmware info: {e:?}")));
                }) {
                    Ok(info) => info,
                    Err(_) => return None,
                };

                return Some(Firmware {
                    path: ent.path(),
                    info,
                    index,
                });
            }
            return None;
        }).collect());
    }

    fn find_library(&self, name: &str) -> Option<LibraryInfo> {
        if let Some(lib_name) = self.index.get(name) {
            let path = self.path.join(lib_name);
            return File::open(path).and_then(|file| {
                return serde_json::from_reader(BufReader::new(file))
                    .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad library info: {e:?}")));
            }).ok();
        }
        return None;
    }
}