use common_path::common_path;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use path_slash::CowExt;

use bin_lib::{BinaryInfo, LibraryInfo, LibraryPriority};

use crate::{AppInfo, Component, ServiceInfo, Symlinks};

impl AppInfo {
    fn is_native(&self) -> bool {
        return self.r#type == "native";
    }
}

impl ServiceInfo {
    fn is_native(&self) -> bool {
        let (Some(engine), Some(_)) = (&self.engine, &self.executable) else {
            return false;
        };
        return engine == "native";
    }
}

impl Component<AppInfo> {
    pub(crate) fn parse<P: AsRef<Path>>(dir: P, links: &Symlinks) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let info: AppInfo = serde_json::from_reader(
            File::open(dir.join("appinfo.json"))
                .map_err(|e| Error::new(e.kind(), format!("Failed to open appinfo.json: {e}")))?,
        )
        .map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Failed to parse appinfo.json: {e}"),
            )
        })?;
        if !info.is_native() {
            return Ok(Self {
                id: info.id.clone(),
                info,
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let exe_path = dir.join(Cow::from_slash(&info.main));
        let bin_info = BinaryInfo::parse(
            File::open(&exe_path).map_err(|e| {
                Error::new(
                    e.kind(),
                    format!("Failed to open main executable {}: {e}", info.main),
                )
            })?,
            exe_path.file_name().unwrap().to_string_lossy(),
            true,
        )
        .map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Bad app executable {}: {e}", info.main),
            )
        })?;
        let libs = Self::list_libs(
            dir,
            &Component::<AppInfo>::rpath(&bin_info.rpath, &exe_path),
            links,
        )?;
        return Ok(Self {
            id: info.id.clone(),
            info: info.clone(),
            exe: Some(bin_info),
            libs,
        });
    }
}
impl Component<ServiceInfo> {
    pub(crate) fn parse<P: AsRef<Path>>(dir: P, links: &Symlinks) -> Result<Self, Error> {
        let dir = dir.as_ref();
        let info: ServiceInfo = serde_json::from_reader(File::open(dir.join("services.json"))?)
            .map_err(|e| Error::new(ErrorKind::InvalidData, format!("Bad appinfo.json: {e:?}")))?;
        if !info.is_native() {
            return Ok(Self {
                id: info.id.clone(),
                info: info.clone(),
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let executable = info.executable.as_ref().unwrap();
        let exe_path = dir.join(Cow::from_slash(executable));
        let bin_info = BinaryInfo::parse(
            File::open(dir.join(&exe_path))?,
            exe_path.file_name().unwrap().to_string_lossy(),
            true,
        )
        .map_err(|e| {
            Error::new(
                ErrorKind::InvalidData,
                format!("Bad app executable {}: {e:?}", executable),
            )
        })?;
        let libs = Self::list_libs(
            dir,
            &Component::<ServiceInfo>::rpath(&bin_info.rpath, &exe_path),
            links,
        )?;
        return Ok(Self {
            id: info.id.clone(),
            info: info.clone(),
            exe: Some(bin_info),
            libs,
        });
    }
}

impl<T> Component<T> {
    pub fn find_lib(&self, name: &str) -> Option<&'_ LibraryInfo> {
        return self.libs.iter().find(|lib| lib.has_name(name));
    }

    pub fn is_required(&self, lib: &LibraryInfo) -> bool {
        let Some(exe) = &self.exe else {
            return false;
        };
        return exe
            .needed
            .iter()
            .find(|needed| lib.has_name(needed))
            .is_some();
    }

    fn rpath<P>(rpath: &Vec<String>, bin_path: P) -> Vec<PathBuf>
    where
        P: AsRef<Path>,
    {
        let origin = bin_path.as_ref().parent().unwrap();
        return rpath
            .iter()
            .filter_map(|p| {
                PathBuf::from(p.replace("$ORIGIN", origin.to_string_lossy().as_ref()))
                    .canonicalize()
                    .ok()
            })
            .filter(|p| {
                let Some(common) = common_path(&p, &origin) else {
                    return false;
                };
                return common.components().count() > 1;
            })
            .collect();
    }

    fn list_libs(
        dir: &Path,
        rpath: &Vec<PathBuf>,
        links: &Symlinks,
    ) -> Result<Vec<LibraryInfo>, Error> {
        let mut libs: HashMap<PathBuf, LibraryInfo> = HashMap::new();
        let mut visited_dirs: HashSet<PathBuf> = HashSet::new();
        let mut queue: VecDeque<(PathBuf, bool)> = VecDeque::new();

        for p in rpath {
            queue.push_back((p.clone(), true));
        }
        if let Ok(lib_dir) = dir.join("lib").canonicalize() {
            if !rpath.contains(&lib_dir) {
                queue.push_back((lib_dir, false));
            }
        }

        // Discover libraries by walking the executable's rpath directories and,
        // transitively, each bundled library's own DT_RUNPATH/DT_RPATH
        // ($ORIGIN-relative). This mirrors the dynamic loader: e.g. a bundled
        // libpulse.so.0 with RUNPATH $ORIGIN/pulseaudio pulls in
        // lib/pulseaudio/libpulsecommon-15.0.so, which a flat scan of lib/
        // would miss.
        while let Some((lib_dir, is_rpath)) = queue.pop_front() {
            if !visited_dirs.insert(lib_dir.clone()) {
                continue;
            }
            let Ok(entries) = fs::read_dir(&lib_dir) else {
                continue;
            };
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                let path = entry.path();
                let Ok(mut lib) = LibraryInfo::parse(
                    File::open(&path)?,
                    true,
                    path.file_name().unwrap().to_string_lossy(),
                ) else {
                    continue;
                };
                lib.priority = if is_rpath {
                    LibraryPriority::Rpath
                } else {
                    LibraryPriority::Package
                };
                // A bundled library's own runpath can point at further bundled
                // directories; queue them for discovery too.
                for sub_dir in Self::rpath(&lib.rpath, &path) {
                    if !visited_dirs.contains(&sub_dir) {
                        queue.push_back((sub_dir, true));
                    }
                }
                libs.insert(path, lib);
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
