use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, ErrorKind, Read};

use debpkg::{Control, DebPkg};
use path_slash::CowExt;

use common::{Firmware, VerifyWithFirmware};

use crate::{Component, Package, PackageInfo, PackageVerifyResult, Symlinks};

impl Package {
    pub fn parse<R>(read: R) -> Result<Package, Error>
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
        let links = Symlinks::new(links);
        let package_info = File::open(tmp.as_ref().join(Cow::from_slash(&format!(
            "usr/palm/packages/{id}/packageinfo.json"
        ))))?;
        let package_info: PackageInfo = serde_json::from_reader(package_info).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Bad packageinfo.json: {e:?}"),
            )
        })?;
        let app = Component::parse_app(
            tmp.as_ref().join(Cow::from_slash(&format!(
                "usr/palm/applications/{}",
                package_info.app
            ))),
            &links,
        )?;
        let mut services = Vec::new();
        for id in &package_info.services {
            let service = Component::parse_service(
                tmp.as_ref()
                    .join(Cow::from_slash(&format!("usr/palm/services/{id}"))),
                &links,
            )?;
            services.push(service);
        }
        return Ok(Package { id, app, services });
    }

    fn deb_err(e: debpkg::Error) -> Error {
        return Error::new(ErrorKind::InvalidData, format!("Bad package: {e:?}"));
    }
}

impl VerifyWithFirmware<PackageVerifyResult> for Package {
    fn verify(&self, firmware: &Firmware) -> PackageVerifyResult {
        return PackageVerifyResult {
            app: self.app.verify(firmware),
            services: self
                .services
                .iter()
                .map(|svc| svc.verify(firmware))
                .collect(),
        };
    }
}
