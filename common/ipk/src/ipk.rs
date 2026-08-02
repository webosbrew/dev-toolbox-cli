use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Error, ErrorKind, Read};
use std::path::Path;

use debpkg::{Control, DebPkg};
use path_slash::CowExt;

use crate::path::ensure_within;
use crate::{AppInfo, Component, Package, PackageInfo, ServiceInfo, Symlinks};

impl Package {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        return File::open(path.as_ref()).and_then(Package::parse);
    }

    pub fn parse<R>(read: R) -> Result<Self, Error>
    where
        R: Read,
    {
        let mut deb = DebPkg::parse(read).map_err(Self::deb_err)?;
        let control =
            Control::extract(deb.control().map_err(Self::deb_err)?).map_err(Self::deb_err)?;
        let mut data = deb.data().map_err(Self::deb_err)?;

        let id = String::from(control.name());
        let installed_size = control
            .get("Installed-Size")
            .filter(|s| *s != "1234")
            .and_then(|s| s.parse::<u64>().ok());

        let tmp = tempfile::TempDir::new()?;
        let mut links = HashMap::new();
        for entry in data.entries()? {
            let mut entry = entry?;
            let entry_type = entry.header().entry_type();
            if entry_type.is_symlink() {
                let path = tmp
                    .as_ref()
                    .join(Cow::from_slash(&entry.path()?.to_string_lossy()));
                // A symlink header without a target is malformed. Ignore it.
                let (Some(parent), Some(link_name)) = (path.parent(), entry.link_name()?) else {
                    continue;
                };
                let target = parent.join(Cow::from_slash(&link_name.to_string_lossy()));
                links.insert(path, target);
            } else if entry_type.is_file() {
                entry.unpack_in(&tmp)?;
            } else if !entry_type.is_dir() {
                println!("Ignore special file {}", entry.path()?.to_string_lossy());
            }
        }
        let links = Symlinks::new(&links);
        // The package id and the app/service ids come from untrusted metadata;
        // guard every path joined onto the extraction dir against traversal.
        let root = tmp.as_ref();
        let package_info_path = ensure_within(
            root,
            &root.join(Cow::from_slash(&format!(
                "usr/palm/packages/{id}/packageinfo.json"
            ))),
        )?;
        let package_info = File::open(package_info_path)?;
        let package_info: PackageInfo = serde_json::from_reader(package_info).map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Bad packageinfo.json: {e:?}"),
            )
        })?;
        let app_dir = ensure_within(
            root,
            &root.join(Cow::from_slash(&format!(
                "usr/palm/applications/{}",
                package_info.app
            ))),
        )?;
        let app = Component::<AppInfo>::parse(app_dir, &links)?;
        let mut services = Vec::new();
        for id in &package_info.services {
            let service_dir =
                ensure_within(root, &root.join(Cow::from_slash(&format!("usr/palm/services/{id}"))))?;
            let service = Component::<ServiceInfo>::parse(service_dir, &links)?;
            services.push(service);
        }
        return Ok(Self {
            id,
            installed_size,
            app,
            services,
        });
    }

    // Taken by value so it can be passed straight to `map_err`.
    #[allow(clippy::needless_pass_by_value)]
    fn deb_err(e: debpkg::Error) -> Error {
        return Error::new(ErrorKind::InvalidData, format!("Bad package: {e:?}"));
    }
}
