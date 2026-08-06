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
        // Read the control archive in one pass: `DebPkg` hands it out once, and
        // the maintainer scripts sit next to the control file inside it.
        let (control, install_hooks) = {
            let mut archive = deb.control().map_err(Self::deb_err)?;
            let mut control: Option<Control> = None;
            let mut hooks: Vec<String> = Vec::new();
            for entry in archive.entries()? {
                let entry = entry?;
                // Take the name before the entry moves into the parser below.
                let path = entry.path()?.to_string_lossy().into_owned();
                let name = path.trim_start_matches("./");
                if name == "control" {
                    control = Some(Control::parse(entry).map_err(Self::deb_err)?);
                } else if let Some(hook) = install_hook(name) {
                    hooks.push(String::from(hook));
                }
            }
            let Some(control) = control else {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "Bad package: the control archive has no control file",
                ));
            };
            hooks.sort_unstable();
            (control, hooks)
        };
        let mut data = deb.data().map_err(Self::deb_err)?;

        let id = String::from(control.name());
        let installed_size = control
            .get("Installed-Size")
            .filter(|s| *s != "1234")
            .and_then(|s| s.parse::<u64>().ok());
        let hand_rolled = PACKAGER_FIELDS
            .iter()
            .any(|field| control.get(field).is_none());

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
            let service_dir = ensure_within(
                root,
                &root.join(Cow::from_slash(&format!("usr/palm/services/{id}"))),
            )?;
            let service = Component::<ServiceInfo>::parse(service_dir, &links)?;
            services.push(service);
        }
        return Ok(Self {
            id,
            installed_size,
            install_hooks,
            hand_rolled,
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

/// Control fields that every official packager writes — `ares-package` from
/// webos-tools/cli, and `ares-cli-rs`. A package missing them was assembled by
/// hand, and what else it got wrong is anyone's guess.
const PACKAGER_FIELDS: [&str; 3] = [
    "Installed-Size",
    "webOS-Package-Format-Version",
    "webOS-Packager-Version",
];

/// The Debian maintainer scripts. The webOS installer runs none of them, so a
/// package that relies on one installs and does nothing it meant to do.
const INSTALL_HOOKS: [&str; 4] = ["preinst", "postinst", "prerm", "postrm"];

/// The maintainer script this control-archive entry is, if it is one.
fn install_hook(name: &str) -> Option<&'static str> {
    return INSTALL_HOOKS.iter().copied().find(|hook| *hook == name);
}

#[cfg(test)]
mod tests {
    use super::install_hook;
    use crate::Package;
    use std::io::Cursor;

    /// `ares_packaged.ipk` is the output of `ares-package` on a two-file web
    /// app. `hand_rolled.ipk` holds the same app, assembled by hand: no
    /// `Installed-Size`, no `webOS-*` fields, and a `postinst`/`prerm` pair.
    /// Both are the real thing, not a mock.
    fn parse(bytes: &[u8]) -> Package {
        return Package::parse(Cursor::new(bytes)).expect("fixture parses");
    }

    #[test]
    fn ares_package_output_raises_nothing() {
        let pkg = parse(include_bytes!("fixtures/ares_packaged.ipk"));
        assert_eq!(pkg.id, "com.example.fixture");
        assert!(!pkg.hand_rolled, "ares-package writes every packager field");
        assert!(pkg.install_hooks.is_empty());
        // The report still describes it: the app parsed, the web app detected.
        assert!(pkg.app.info.web.is_some());
    }

    #[test]
    fn hand_assembled_package_raises_both_warnings() {
        let pkg = parse(include_bytes!("fixtures/hand_rolled.ipk"));
        assert_eq!(pkg.id, "com.example.fixture");
        assert!(pkg.hand_rolled, "no Installed-Size, no webOS-* fields");
        assert_eq!(pkg.install_hooks, vec!["postinst", "prerm"]);
        // A package the tool warns about is still a package it reads.
        assert!(pkg.app.info.web.is_some());
    }

    #[test]
    fn names_the_maintainer_scripts() {
        for name in ["preinst", "postinst", "prerm", "postrm"] {
            assert_eq!(install_hook(name), Some(name), "{name} is a hook");
        }
    }

    #[test]
    fn passes_over_everything_else() {
        // `control` is read as the control file, and md5sums/conffiles carry no
        // code to run.
        for name in ["control", "md5sums", "conffiles", "postinst.bak", ""] {
            assert_eq!(install_hook(name), None, "{name} is not a hook");
        }
    }
}
