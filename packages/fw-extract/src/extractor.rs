use crate::FirmwareExtractor;
use bin_lib::LibraryInfo;
use debian_control::Control;
use debversion::{AsVersion, Version as DebVersion};
use fw_lib::FirmwareInfo;
use path_slash::PathBufExt;
use regex::Regex;
use semver::Version as SemVer;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::fs::{DirEntry, File};
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug)]
struct PackageVersion {
    #[serde(skip_serializing_if = "Option::is_none")]
    epoch: Option<u32>,
    upstream: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    debian_revision: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct SystemPackage {
    version: PackageVersion,
}

impl FirmwareExtractor {
    pub fn extract_pkgs<P: AsRef<Path>>(
        &self,
        files_pkg_index: &mut BTreeMap<PathBuf, String>,
        output: P,
    ) {
        let mut packages = BTreeMap::<String, SystemPackage>::new();
        for path in &self.opkg_info_paths {
            for ent in path.read_dir().unwrap() {
                let Ok(ent) = ent else {
                    continue;
                };
                let path = ent.path();
                let Some(ext) = path.extension() else {
                    continue;
                };
                let root = path.ancestors().nth(5).unwrap();
                if ext == "list" {
                    for line in BufReader::new(File::open(&path).unwrap()).lines() {
                        let Ok(line) = line else {
                            continue;
                        };
                        let Some(line) = line.split("\t").next() else {
                            continue;
                        };
                        files_pkg_index.insert(
                            root.join(PathBuf::from_slash(line.trim_start_matches("/"))),
                            String::from(path.file_stem().unwrap().to_string_lossy()),
                        );
                    }
                } else if ext == "control" {
                    let ctrl = Control::from_file(&path).unwrap();
                    let Some(bin) = ctrl.binaries().next() else {
                        continue;
                    };
                    let version_str = bin.as_deb822().get("Version").unwrap();
                    let version =
                        version_str
                            .as_str()
                            .into_version()
                            .unwrap_or_else(|_| DebVersion {
                                epoch: None,
                                upstream_version: version_str.clone(),
                                debian_revision: None,
                            });

                    let pkg_name = bin.name().unwrap();
                    packages.insert(
                        pkg_name.clone(),
                        SystemPackage {
                            version: PackageVersion {
                                epoch: version.epoch,
                                upstream: version.upstream_version,
                                debian_revision: version.debian_revision,
                            },
                        },
                    );
                }
            }
        }
        let Ok(file) = File::create(output.as_ref().join("packages.json")) else {
            return;
        };
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &packages).expect("Failed to save packages.json");
    }

    pub fn extract_libs<P: AsRef<Path>>(
        &self,
        files_pkg_index: &BTreeMap<PathBuf, String>,
        lib_index: &mut BTreeMap<String, String>,
        output: P,
    ) {
        for lib_path in &self.lib_paths {
            let Ok(dir) = lib_path.read_dir() else {
                continue;
            };
            for ent in dir {
                match ent {
                    Ok(ent) => {
                        self.handle_entry(
                            ent,
                            &self.so_regex,
                            files_pkg_index,
                            lib_index,
                            output.as_ref(),
                            0,
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "Failed to read entry in directory: {}: {e:?}",
                            lib_path.to_string_lossy()
                        );
                    }
                }
            }
        }
    }

    fn handle_entry<P>(
        &self,
        ent: DirEntry,
        so_regex: &Regex,
        files_pkg_index: &BTreeMap<PathBuf, String>,
        lib_index: &mut BTreeMap<String, String>,
        output: P,
        debug: u8,
    ) where
        P: AsRef<Path>,
    {
        let ent_path = ent.path();
        if let (Some(name), Some(metadata)) =
            (ent_path.file_name().unwrap().to_str(), ent.metadata().ok())
        {
            if !so_regex.is_match(name) {
                return;
            }
            if metadata.is_file() {
                let mut lib_info = match File::open(&ent_path).and_then(|file| {
                    LibraryInfo::parse(file, false, ent_path.file_name().unwrap().to_string_lossy())
                        .map_err(|e| {
                            Error::new(
                                ErrorKind::InvalidData,
                                format!("Failed to parse library {name}: {e:?}"),
                            )
                        })
                }) {
                    Ok(info) => info,
                    Err(e) => {
                        eprintln!("Ignoring library {name}: {e:?}");
                        return;
                    }
                };
                lib_info.package = files_pkg_index.get(&ent_path).cloned();
                if debug > 0 {
                    println!("Saving symbols list for {name}");
                }
                let symbols_name = format!("{}.json", name);
                File::create(output.as_ref().join(&symbols_name))
                    .and_then(|file| {
                        let writer = BufWriter::new(file);
                        return serde_json::to_writer_pretty(writer, &lib_info).map_err(|e| {
                            Error::new(ErrorKind::InvalidData, format!("Failed to write {:?}", e))
                        });
                    })
                    .unwrap();
                lib_index.insert(String::from(name), symbols_name);
            } else if metadata.is_symlink() {
                match self.final_link_target(&ent_path) {
                    Ok(Some(target)) => {
                        lib_index.insert(
                            String::from(name),
                            format!("{}.json", target.file_name().unwrap().to_string_lossy()),
                        );
                    }
                    Ok(None) => {}
                    Err(e) => {
                        eprintln!(
                            "Failed to resolve link target for {}: {e}",
                            ent_path.to_string_lossy()
                        );
                    }
                }
            }
        }
    }

    fn final_link_target<P: AsRef<Path>>(&self, link: P) -> Result<Option<PathBuf>, Error> {
        let mut target = link.as_ref().to_path_buf();
        loop {
            match fs::symlink_metadata(&target) {
                Ok(metadata) => {
                    if !metadata.is_symlink() {
                        break;
                    }
                }
                Err(e) => {
                    return Err(Error::new(
                        e.kind(),
                        format!("Can't find symlink info for {}", target.to_string_lossy()),
                    ));
                }
            }
            match target.read_link() {
                Ok(l) => {
                    target = self.join_link_target(l)?;
                }
                Err(e) => {
                    eprintln!(
                        "Failed to find link target for {}: {:?}",
                        link.as_ref().to_string_lossy(),
                        e
                    );
                    return Ok(None);
                }
            };
        }
        Ok(Some(target))
    }

    fn join_link_target<P>(&self, target: P) -> Result<PathBuf, Error>
    where
        P: AsRef<Path>,
    {
        let target = target.as_ref();
        if target.is_absolute() {
            let joined = self.rootfs_path.join(target.strip_prefix("/").unwrap());
            if joined.exists() {
                return Ok(joined);
            }
        } else {
            for lib_path in &self.lib_paths {
                let joined = lib_path.join(target);
                if joined.exists() {
                    return Ok(joined);
                }
            }
        }
        Err(Error::new(
            ErrorKind::NotFound,
            format!(
                "Can't find link target {}",
                target.file_name().unwrap().to_string_lossy()
            ),
        ))
    }

    pub fn create<P: AsRef<Path>>(input: P) -> Result<Self, Error> {
        let input = input.as_ref();
        let fw_info = Self::extract_fw_info(input)?;
        let rootfs_path = input.join("rootfs.pak.unsquashfs");
        let lib_paths = Self::extract_lib_paths(input, &rootfs_path)?;
        let opkg_info_paths = Self::extract_opkg_info_paths(input)?;
        let so_regex = Regex::new("^.+.so(\\.\\w+)*$").unwrap();
        Ok(Self {
            fw_info,
            rootfs_path,
            lib_paths,
            opkg_info_paths,
            so_regex,
        })
    }

    fn extract_fw_info(input: &Path) -> Result<FirmwareInfo, Error> {
        let (version, ota_id) = input
            .file_name()
            .map(|name| name.to_str())
            .flatten()
            .map(|s| s.split_once("-"))
            .flatten()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    format!("Bad input path {}", input.display()),
                )
            })?;
        let mut starfish_release = String::new();

        File::open(
            input
                .join("rootfs.pak.unsquashfs")
                .join("etc")
                .join("starfish-release"),
        )?
            .read_to_string(&mut starfish_release)?;
        let release_regex = Regex::new("release (\\d+\\.\\d+\\.\\d+)").unwrap();
        let release = release_regex
            .captures(&starfish_release)
            .map(|cap| cap.get(1))
            .flatten()
            .ok_or_else(|| {
                Error::new(
                    ErrorKind::NotFound,
                    format!("Bad starfish-release: {starfish_release}"),
                )
            })?;
        Ok(FirmwareInfo {
            version: String::from(version),
            ota_id: String::from(ota_id),
            release: SemVer::parse(release.as_str()).map_err(|e| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!("Invalid version {}: {e:?}", release.as_str()),
                )
            })?,
        })
    }
    fn extract_lib_paths<Input, Root>(input: Input, root: Root) -> Result<Vec<PathBuf>, Error>
    where
        Input: AsRef<Path>,
        Root: AsRef<Path>,
    {
        let root = root.as_ref();
        let input = input.as_ref();
        let ldconf_path = root.join("etc").join("ld.so.conf");
        let reader = BufReader::new(File::open(ldconf_path)?);
        Ok(vec!["lib", "usr/lib"]
            .iter()
            .map(|p| root.join(PathBuf::from_slash(*p)))
            .chain(reader.lines().filter_map(|line| {
                let trimmed = line
                    .as_ref()
                    .map_or("", |r| r.trim().strip_prefix("/").unwrap_or(""));
                if trimmed.is_empty() {
                    return None;
                }
                if let Some(bsp) = trimmed.strip_prefix("mnt/bsppart/") {
                    return fs::read_dir(input)
                        .and_then(|mut dir| {
                            return Ok(dir.find_map(|entry| {
                                if let Ok(entry) = entry {
                                    let file_name = entry.file_name();
                                    if Regex::new(r"bsppart(-\w+)?.pak.unsquashfs")
                                        .unwrap()
                                        .is_match(&*file_name.to_string_lossy())
                                    {
                                        return Some(
                                            input.join(file_name).join(PathBuf::from_slash(bsp)),
                                        );
                                    }
                                }
                                return None;
                            }));
                        })
                        .ok()
                        .flatten();
                }
                return Some(root.join(PathBuf::from_slash(trimmed)));
            }))
            .collect())
    }

    fn extract_opkg_info_paths<Input>(input: Input) -> Result<Vec<PathBuf>, Error>
    where
        Input: AsRef<Path>,
    {
        Ok(input
            .as_ref()
            .read_dir()?
            .flat_map(|ent| {
                let Ok(ent) = ent else {
                    return vec![];
                };
                let path = ent.path();
                return vec![
                    "usr/lib/opkg/info",
                    "bsp/var/lib/opkg/info", /*, "var/lib/opkg/info"*/
                ]
                    .iter()
                    .map(|x| path.join(PathBuf::from_slash(x)))
                    .filter(|p| p.is_dir())
                    .collect();
            })
            .collect())
    }
}
