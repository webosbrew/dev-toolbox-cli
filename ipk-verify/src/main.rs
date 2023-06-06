use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::PathBuf;

use clap::Parser;
use debpkg::{Control, DebPkg};
use path_slash::CowExt;
use serde::Deserialize;

use common::{BinaryInfo, Firmware, LibraryInfo};

mod application;
mod service;

#[derive(Parser, Debug)]
struct Args {
    #[arg(required = true, help = "Packages to verify")]
    packages: Vec<PathBuf>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    debug: u8,
}

fn main() {
    let args = Args::parse();
    for package in args.packages {
        match File::open(&package) {
            Ok(file) => {
                let package = Package::parse(file).unwrap();
                println!("{:?}", package.app);
            }
            Err(e) => eprintln!(
                "Failed to open {}",
                package.file_name().unwrap().to_string_lossy()
            ),
        }
    }
}

#[derive(Debug)]
struct Package {
    id: String,
    app: Application,
    services: Vec<Service>,
    links: HashMap<PathBuf, PathBuf>,
}

#[derive(Debug)]
struct Application {
    main: Option<BinaryInfo>,
    libs: Vec<LibraryInfo>,
}

#[derive(Debug)]
struct Service {}

#[derive(Debug, Deserialize)]
pub struct PackageInfo {
    id: String,
    version: String,
    app: String,
    #[serde(default)]
    services: Vec<String>,
}

impl Package {
    fn parse<R>(read: R) -> Result<Package, Error>
    where
        R: Read,
    {
        let mut deb = DebPkg::parse(read).map_err(Self::deb_err)?;
        let control = Control::extract(deb.control().unwrap()).map_err(Self::deb_err)?;
        let mut data = deb.data().map_err(Self::deb_err)?;
        let id = String::from(control.name());
        let tmp = tempfile::TempDir::new()?;
        let mut links = HashMap::new();
        for entry in data.entries()? {
            let mut entry = entry?;
            let entry_type = entry.header().entry_type();
            if entry_type.is_symlink() {
                let path = tmp
                    .as_ref()
                    .join(Cow::from_slash(&entry.path()?.to_string_lossy()));
                let target = path.parent().unwrap().join(Cow::from_slash(
                    &entry.link_name()?.unwrap().to_string_lossy(),
                ));
                links.insert(path, target);
            } else if entry_type.is_file() {
                entry.unpack_in(&tmp)?;
            } else if !entry_type.is_dir() {
                println!("Ignore special file {}", entry.path()?.to_string_lossy());
            }
        }
        let package_info = File::open(tmp.as_ref().join(Cow::from_slash(&format!(
            "usr/palm/packages/{id}/packageinfo.json"
        ))))?;
        let package_info: PackageInfo = serde_json::from_reader(package_info).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Bad packageinfo.json: {e:?}"),
            )
        })?;
        let app = Application::parse(tmp.as_ref().join(Cow::from_slash(&format!(
            "usr/palm/applications/{}",
            package_info.app
        ))))?;
        let mut services = Vec::new();
        for id in &package_info.services {
            services.push(Service::parse(
                tmp.as_ref()
                    .join(Cow::from_slash(&format!("usr/palm/services/{id}"))),
            )?);
        }
        return Ok(Package {
            id,
            app,
            services,
            links,
        });
    }

    fn deb_err(e: debpkg::Error) -> Error {
        return Error::new(ErrorKind::InvalidData, format!("Bad package: {e:?}"));
    }
}

trait VerifyElf {
    fn verify(&self, firmware: &Firmware) -> BinVerifyResult;
}

#[derive(Default, Debug)]
struct BinVerifyResult {
    missing_lib: Vec<String>,
    undefined_sym: Vec<String>,
}

impl VerifyElf for BinaryInfo {
    fn verify(&self, firmware: &Firmware) -> BinVerifyResult {
        let mut result = BinVerifyResult::default();
        result.undefined_sym.extend(self.undefined.clone());
        for needed in &self.needed {
            if let Some(lib) = firmware.find_library(needed) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
            } else if let Some(lib) = self.find_library(needed) {
                result.undefined_sym.retain(|sym| !lib.has_symbol(sym));
            } else {
                result.missing_lib.push(needed.clone());
            }
        }
        return result;
    }
}
