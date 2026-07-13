use common_path::common_path;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use path_slash::{CowExt, PathExt};

use bin_lib::{BinaryInfo, BundledArtifact, LibraryInfo, LibraryPriority};

use crate::path::ensure_within;
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
            // Web/hosted app: detect the frontend framework and JS syntax level
            // from the shipped HTML/JS while the extracted files still exist.
            // `main` is untrusted; keep it inside the app directory.
            let index_html = ensure_within(dir, &dir.join(Cow::from_slash(&info.main)))?;
            let mut info = info;
            info.web = Some(webdetect_lib::detect_web_app(dir, &index_html));
            return Ok(Self {
                id: info.id.clone(),
                info,
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let exe_path = ensure_within(dir, &dir.join(Cow::from_slash(&info.main)))?;
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
            // JS/Node service: detect the declared Node.js runtime from the
            // bundled package.json while the extracted files still exist, and
            // note any native binaries it ships (its own node/ffmpeg/.so).
            let mut info = info;
            info.runtime = Some(webdetect_lib::detect_service_runtime(dir));
            info.bundled = scan_bundled_artifacts(dir);
            return Ok(Self {
                id: info.id.clone(),
                info: info.clone(),
                exe: Default::default(),
                libs: Default::default(),
            });
        }
        let executable = info.executable.as_ref().unwrap();
        let exe_path = ensure_within(dir, &dir.join(Cow::from_slash(executable)))?;
        let bin_info = BinaryInfo::parse(
            File::open(&exe_path)?,
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

/// Recursion depth cap for the bundled-artifact walk.
const BUNDLED_MAX_DEPTH: usize = 12;
/// Stop after collecting this many bundled artifacts.
const BUNDLED_MAX: usize = 256;

/// Walk a service directory and classify every bundled ELF (its own `node`,
/// `ffmpeg`, `.so`s, ...). Non-ELF files (scripts, JSON, assets) are skipped.
/// Paths are relative to `dir`, slash-separated, and the list is sorted for
/// stable report output.
fn scan_bundled_artifacts(dir: &Path) -> Vec<BundledArtifact> {
    let mut out = Vec::new();
    walk_bundled(dir, dir, 0, &mut out);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

fn walk_bundled(root: &Path, dir: &Path, depth: usize, out: &mut Vec<BundledArtifact>) {
    if depth > BUNDLED_MAX_DEPTH || out.len() >= BUNDLED_MAX {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= BUNDLED_MAX {
            return;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk_bundled(root, &path, depth + 1, out);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Ok(file) = File::open(&path) else { continue };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_slash_lossy()
            .into_owned();
        if let Some(artifact) = BundledArtifact::identify(file, rel) {
            out.push(artifact);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use bin_lib::ArtifactKind;
    use std::collections::HashMap;

    fn empty_links() -> Symlinks {
        Symlinks::new(HashMap::new())
    }

    #[test]
    fn js_service_reports_bundled_binaries() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        // A non-native service: no `engine`/`executable` → runs on system Node.
        fs::write(d.join("services.json"), r#"{"id":"com.example.app.service"}"#).unwrap();
        fs::write(d.join("package.json"), r#"{"main":"launch.js"}"#).unwrap();
        fs::write(d.join("launch.js"), "var x = 1;").unwrap();
        // A bundled native binary next to the scripts.
        fs::create_dir_all(d.join("bin")).unwrap();
        fs::write(
            d.join("bin/node"),
            &include_bytes!("../../bin/src/fixtures/sample.bin")[..],
        )
        .unwrap();

        let svc = Component::<ServiceInfo>::parse(d, &empty_links()).unwrap();
        assert!(
            svc.info
                .bundled
                .iter()
                .any(|a| a.path == "bin/node" && a.arch.is_some()),
            "expected bin/node to be reported, got {:?}",
            svc.info.bundled
        );
    }

    #[test]
    fn js_service_without_binaries_reports_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        fs::write(d.join("services.json"), r#"{"id":"com.example.app.service"}"#).unwrap();
        fs::write(d.join("package.json"), r#"{"main":"launch.js"}"#).unwrap();
        fs::write(d.join("launch.js"), "var x = 1;").unwrap();

        let svc = Component::<ServiceInfo>::parse(d, &empty_links()).unwrap();
        assert!(svc.info.bundled.is_empty());
    }

    #[test]
    fn kind_classifies_sample_fixture() {
        // Sanity: the shared fixture classifies as one of the two kinds.
        let d = tempfile::TempDir::new().unwrap();
        fs::write(
            d.path().join("f"),
            &include_bytes!("../../bin/src/fixtures/lib_runpath.so")[..],
        )
        .unwrap();
        let a =
            BundledArtifact::identify(File::open(d.path().join("f")).unwrap(), "lib_runpath.so")
                .unwrap();
        assert_eq!(a.kind, ArtifactKind::SharedLibrary);
    }
}
