use common_path::common_path;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::fs::File;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use path_slash::{CowExt, PathExt};

use bin_lib::{BinaryInfo, BundledArtifact, LibraryInfo, LibraryPriority};

use crate::path::{ensure_within, file_label};
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
            // A web app can still ship native binaries (a payload it starts
            // through the root service). Note them the same way as a JS
            // service's.
            let scan = scan_bundled(dir, links);
            info.bundled = scan.artifacts;
            info.bundled_bins = scan.bins;
            return Ok(Self {
                id: info.id.clone(),
                info,
                exe: None,
                libs: Vec::new(),
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
            file_label(&exe_path),
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
            let scan = scan_bundled(dir, links);
            info.bundled = scan.artifacts;
            info.bundled_bins = scan.bins;
            return Ok(Self {
                id: info.id.clone(),
                info: info.clone(),
                exe: None,
                libs: Vec::new(),
            });
        }
        let executable = info.executable.as_ref().unwrap();
        let exe_path = ensure_within(dir, &dir.join(Cow::from_slash(executable)))?;
        let bin_info = BinaryInfo::parse(File::open(&exe_path)?, file_label(&exe_path), true)
            .map_err(|e| {
                Error::new(
                    ErrorKind::InvalidData,
                    format!("Bad app executable {executable}: {e:?}"),
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

/// The bundled native content of a non-native component: an inventory of every
/// ELF (kind + arch, for a quick listing) and, for each bundled *executable*, a
/// verifiable unit (its `exe` plus the libraries reachable via its rpath /
/// sibling `lib` dir) so the bundled runtime can be checked against a firmware's
/// libraries.
#[derive(Default)]
pub(crate) struct BundledScan {
    pub artifacts: Vec<BundledArtifact>,
    pub bins: Vec<Component<()>>,
}

/// Walk a component directory: classify every bundled ELF and, for each
/// executable, build a verifiable [`Component`]. Non-ELF files (scripts, JSON,
/// assets) are skipped. Output is sorted by path for stable report ordering.
pub(crate) fn scan_bundled(dir: &Path, links: &Symlinks) -> BundledScan {
    let mut scan = BundledScan::default();
    walk_bundled(dir, dir, 0, links, &mut scan);
    scan.artifacts.sort_by(|a, b| a.path.cmp(&b.path));
    scan.bins.sort_by(|a, b| a.id.cmp(&b.id));
    scan
}

fn walk_bundled(root: &Path, dir: &Path, depth: usize, links: &Symlinks, scan: &mut BundledScan) {
    if depth > BUNDLED_MAX_DEPTH || scan.artifacts.len() >= BUNDLED_MAX {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if scan.artifacts.len() >= BUNDLED_MAX {
            return;
        }
        let Ok(ft) = entry.file_type() else { continue };
        let path = entry.path();
        if ft.is_dir() {
            walk_bundled(root, &path, depth + 1, links, scan);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_slash_lossy()
            .into_owned();
        let Some(artifact) = BundledArtifact::identify(file, rel.clone()) else {
            continue;
        };
        let is_exe = artifact.kind == bin_lib::ArtifactKind::Executable;
        scan.artifacts.push(artifact);
        if is_exe {
            if let Some(component) = verifiable_bundled_exe(&path, rel, links) {
                scan.bins.push(component);
            }
        }
    }
}

/// Parse a bundled executable and discover the libraries it can load, mirroring
/// how a native component resolves its own libraries. These services locate
/// their libs via the loader's `--library-path <dir>/lib` at spawn time rather
/// than a `DT_RPATH`, so the executable's sibling `lib/` directory is treated as
/// a search path (rpath precedence) in addition to any real rpath.
fn verifiable_bundled_exe(path: &Path, rel: String, links: &Symlinks) -> Option<Component<()>> {
    let bin = BinaryInfo::parse(File::open(path).ok()?, file_label(path), true).ok()?;
    let parent = path.parent()?;
    let mut rpath = Component::<()>::rpath(&bin.rpath, path);
    if let Ok(sibling_lib) = parent.join("lib").canonicalize() {
        if !rpath.contains(&sibling_lib) {
            rpath.push(sibling_lib);
        }
    }
    let libs = Component::<()>::list_libs(parent, &rpath, links).ok()?;
    Some(Component {
        id: rel,
        info: (),
        exe: Some(bin),
        libs,
    })
}

impl<T> Component<T> {
    pub fn find_lib(&self, name: &str) -> Option<&'_ LibraryInfo> {
        return self.libs.iter().find(|lib| lib.has_name(name));
    }

    pub fn is_required(&self, lib: &LibraryInfo) -> bool {
        let Some(exe) = &self.exe else {
            return false;
        };
        return exe.needed.iter().any(|needed| lib.has_name(needed));
    }

    fn rpath<P>(rpath: &[String], bin_path: P) -> Vec<PathBuf>
    where
        P: AsRef<Path>,
    {
        let origin = bin_path.as_ref().parent().unwrap_or(Path::new("."));
        // Compare canonical forms on both sides. `canonicalize` returns a `\\?\`
        // verbatim path on Windows (and resolves symlinks everywhere), which
        // shares no prefix with a plain `C:\…` origin, so `common_path` below
        // would otherwise reject every rpath directory. Substitution still uses
        // the path as given: Windows takes a verbatim path literally, so a `..`
        // appended to one never resolves.
        let origin_canon = origin.canonicalize().unwrap_or_else(|_| origin.to_owned());
        // An rpath must stay near the package, so the shared ancestor has to be
        // deeper than the filesystem root: `/` on Unix, `\\?\C:\` on Windows.
        let root_depth = origin_canon
            .ancestors()
            .last()
            .map_or(1, |root| root.components().count());
        return rpath
            .iter()
            .filter_map(|p| {
                PathBuf::from(p.replace("$ORIGIN", origin.to_string_lossy().as_ref()))
                    .canonicalize()
                    .ok()
            })
            .filter(|p| {
                let Some(common) = common_path(p, &origin_canon) else {
                    return false;
                };
                return common.components().count() > root_depth;
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
                let Ok(mut lib) = LibraryInfo::parse(File::open(&path)?, true, file_label(&path))
                else {
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
            lib.names.push(String::from(file_label(path)));
            lib.names.extend(
                links
                    .links(path)
                    .iter()
                    .map(|p| String::from(file_label(p))),
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
        Symlinks::new(&HashMap::new())
    }

    #[test]
    fn js_service_reports_bundled_binaries() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        // A non-native service: no `engine`/`executable` → runs on system Node.
        fs::write(
            d.join("services.json"),
            r#"{"id":"com.example.app.service"}"#,
        )
        .unwrap();
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
        // The bundled executable also becomes a verifiable unit.
        assert!(
            svc.info
                .bundled_bins
                .iter()
                .any(|c| c.id == "bin/node" && c.exe.is_some()),
            "expected bin/node as a verifiable component, got {:?}",
            svc.info
                .bundled_bins
                .iter()
                .map(|c| &c.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn web_app_reports_bundled_binaries() {
        // A web app that ships a native payload it starts through the root
        // service (e.g. the WireGuard app's wireguard-go).
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        fs::write(
            d.join("appinfo.json"),
            r#"{"id":"com.example.app","version":"1.0.0","type":"web","title":"Example","main":"index.html"}"#,
        )
        .unwrap();
        fs::write(d.join("index.html"), "<html><body></body></html>").unwrap();
        fs::create_dir_all(d.join("payload/bin")).unwrap();
        fs::write(
            d.join("payload/bin/helper"),
            &include_bytes!("../../bin/src/fixtures/sample.bin")[..],
        )
        .unwrap();

        let app = Component::<AppInfo>::parse(d, &empty_links()).unwrap();
        assert!(
            app.info
                .bundled
                .iter()
                .any(|a| a.path == "payload/bin/helper" && a.arch.is_some()),
            "expected payload/bin/helper to be reported, got {:?}",
            app.info.bundled
        );
        assert!(
            app.info
                .bundled_bins
                .iter()
                .any(|c| c.id == "payload/bin/helper" && c.exe.is_some()),
            "expected payload/bin/helper as a verifiable component, got {:?}",
            app.info
                .bundled_bins
                .iter()
                .map(|c| &c.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn mixed_architectures_are_all_reported() {
        // A package may ship a payload per architecture and pick at run time.
        // Report every binary with its own arch — the package-level
        // `Architecture` field says nothing, `ares-package` always writes `all`.
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        fs::write(
            d.join("appinfo.json"),
            r#"{"id":"com.example.app","version":"1.0.0","type":"web","title":"Example","main":"index.html"}"#,
        )
        .unwrap();
        fs::write(d.join("index.html"), "<html><body></body></html>").unwrap();
        fs::create_dir_all(d.join("payload/arm")).unwrap();
        fs::create_dir_all(d.join("payload/x86_64")).unwrap();
        fs::write(
            d.join("payload/arm/helper"),
            &include_bytes!("../../bin/src/fixtures/sample.bin")[..],
        )
        .unwrap();
        fs::write(
            d.join("payload/x86_64/helper.so"),
            &include_bytes!("../../bin/src/fixtures/lib_runpath.so")[..],
        )
        .unwrap();

        let app = Component::<AppInfo>::parse(d, &empty_links()).unwrap();
        let arches: Vec<(&str, &str)> = app
            .info
            .bundled
            .iter()
            .map(|a| (a.path.as_str(), a.arch.as_deref().unwrap_or("unknown")))
            .collect();
        assert_eq!(
            arches,
            vec![
                ("payload/arm/helper", "ARM (32-bit)"),
                ("payload/x86_64/helper.so", "x86-64"),
            ],
            "expected both architectures reported"
        );
    }

    #[test]
    fn web_app_without_binaries_reports_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        fs::write(
            d.join("appinfo.json"),
            r#"{"id":"com.example.app","version":"1.0.0","type":"web","title":"Example","main":"index.html"}"#,
        )
        .unwrap();
        fs::write(d.join("index.html"), "<html><body></body></html>").unwrap();

        let app = Component::<AppInfo>::parse(d, &empty_links()).unwrap();
        assert!(app.info.bundled.is_empty());
    }

    #[test]
    fn js_service_without_binaries_reports_none() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        fs::write(
            d.join("services.json"),
            r#"{"id":"com.example.app.service"}"#,
        )
        .unwrap();
        fs::write(d.join("package.json"), r#"{"main":"launch.js"}"#).unwrap();
        fs::write(d.join("launch.js"), "var x = 1;").unwrap();

        let svc = Component::<ServiceInfo>::parse(d, &empty_links()).unwrap();
        assert!(svc.info.bundled.is_empty());
    }

    #[test]
    fn rpath_resolves_origin_relative_dirs() {
        // An app laid out like Moonlight: the executable sits in `bin/` and its
        // bundled libraries in a sibling `lib/backports/`.
        let dir = tempfile::TempDir::new().unwrap();
        let d = dir.path();
        fs::create_dir_all(d.join("bin")).unwrap();
        fs::create_dir_all(d.join("lib/backports")).unwrap();
        let exe = d.join("bin/moonlight");
        fs::write(&exe, b"x").unwrap();

        let paths = Component::<()>::rpath(
            &[
                // Does not exist -> dropped.
                String::from("$ORIGIN/lib/backports"),
                String::from("$ORIGIN/../lib/backports"),
                String::from("$ORIGIN"),
            ],
            &exe,
        );

        let backports = d.join("lib/backports").canonicalize().unwrap();
        let bin = d.join("bin").canonicalize().unwrap();
        assert_eq!(paths, vec![backports, bin], "got {paths:?}");
    }

    #[test]
    fn rpath_rejects_filesystem_root() {
        let dir = tempfile::TempDir::new().unwrap();
        let exe = dir.path().join("app");
        fs::write(&exe, b"x").unwrap();
        let root = dir
            .path()
            .canonicalize()
            .unwrap()
            .ancestors()
            .last()
            .unwrap()
            .to_path_buf();

        let paths = Component::<()>::rpath(&[root.to_string_lossy().into_owned()], &exe);

        assert!(paths.is_empty(), "expected no rpath dirs, got {paths:?}");
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
