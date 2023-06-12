use std::collections::BTreeMap;
use std::fs;
use std::fs::{DirEntry, File};
use std::io::{BufRead, BufReader, BufWriter, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};

use path_slash::PathBufExt;
use regex::Regex;
use semver::Version;

use bin_lib::LibraryInfo;
use fw_lib::FirmwareInfo;

use crate::FirmwareExtractor;

impl FirmwareExtractor {
    pub fn extract_libs<P: AsRef<Path>>(&self, mappings: &mut BTreeMap<String, String>, output: P) {
        for lib_path in &self.lib_paths {
            if let Ok(dir) = lib_path.read_dir() {
                for ent in dir {
                    match ent {
                        Ok(ent) => {
                            self.handle_entry(ent, &self.so_regex, mappings, output.as_ref(), 0);
                        }
                        Err(e) => {
                            eprintln!("{e:?}");
                        }
                    }
                }
            }
        }
    }

    fn handle_entry<P>(
        &self,
        ent: DirEntry,
        so_regex: &Regex,
        mappings: &mut BTreeMap<String, String>,
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
                let lib_info = match File::open(&ent_path).and_then(|file| {
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
                mappings.insert(String::from(name), symbols_name);
            } else if metadata.is_symlink() {
                match self.final_link_target(&ent_path) {
                    Ok(Some(target)) => {
                        mappings.insert(
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
            if let Ok(l) = target.read_link() {
                target = self.join_link_target(l)?;
            } else {
                return Ok(None);
            };
        }
        return Ok(Some(target));
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
        return Err(Error::new(
            ErrorKind::NotFound,
            format!(
                "Can't find link target {}",
                target.file_name().unwrap().to_string_lossy()
            ),
        ));
    }

    pub fn create<P: AsRef<Path>>(input: P) -> Result<Self, Error> {
        let input = input.as_ref();
        let fw_info = Self::extract_fw_info(input)?;
        let rootfs_path = input.join("rootfs.pak.unsquashfs");
        let lib_paths = Self::extract_lib_paths(input, &rootfs_path)?;
        let so_regex = Regex::new("^.+.so(\\.\\w+)*$").unwrap();
        return Ok(Self {
            fw_info,
            rootfs_path,
            lib_paths,
            so_regex,
        });
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
        return Ok(FirmwareInfo {
            version: String::from(version),
            ota_id: String::from(ota_id),
            release: Version::parse(release.as_str()).map_err(|e| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!("Invalid version {}: {e:?}", release.as_str()),
                )
            })?,
        });
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
        return Ok(vec!["lib", "usr/lib"]
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
            .collect());
    }
}
