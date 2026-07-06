//! Node.js service runtime detection from a bundled `package.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use semver::VersionReq;
use serde::Deserialize;

use crate::ServiceRuntimeDetection;

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    engines: Engines,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
struct Engines {
    #[serde(default)]
    node: Option<String>,
}

/// Read `<dir>/package.json` and extract the declared Node.js requirement,
/// dependencies and entry point. Returns an empty detection when there is no
/// parseable `package.json` (e.g. a QML or non-JS service).
pub fn detect_service_runtime(dir: &Path) -> ServiceRuntimeDetection {
    let text = fs::read_to_string(dir.join("package.json")).unwrap_or_default();
    let pkg: PackageJson = serde_json::from_str(&text).unwrap_or_default();

    // `engines.node` is a semver range like ">=12.0.0"; parse best-effort.
    let declared_node = pkg
        .engines
        .node
        .as_deref()
        .and_then(|s| VersionReq::parse(s).ok());

    ServiceRuntimeDetection {
        declared_node,
        dependencies: pkg.dependencies.into_iter().collect(),
        main: pkg.main,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_pkg(body: &str) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let mut f = fs::File::create(dir.path().join("package.json")).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn parses_engines_and_deps() {
        let dir = write_pkg(
            r#"{ "main": "service.js", "engines": { "node": ">=12.0.0" },
                "dependencies": { "express": "^4.18.0", "lodash": "4.17.21" } }"#,
        );
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.main.as_deref(), Some("service.js"));
        assert!(d.declared_node.unwrap().matches(&semver::Version::parse("14.0.0").unwrap()));
        assert_eq!(d.dependencies.len(), 2);
        assert_eq!(d.dependencies[0].0, "express");
    }

    #[test]
    fn no_engines_declared() {
        let dir = write_pkg(r#"{ "main": "index.js" }"#);
        let d = detect_service_runtime(dir.path());
        assert!(d.declared_node.is_none());
        assert!(d.dependencies.is_empty());
    }

    #[test]
    fn missing_package_json_is_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = detect_service_runtime(dir.path());
        assert!(d.declared_node.is_none());
        assert!(d.main.is_none());
    }
}
