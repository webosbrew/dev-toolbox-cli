//! Node.js service runtime detection from a bundled `package.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::ServiceRuntimeDetection;

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    main: Option<String>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}

/// Read `<dir>/package.json` and extract the service's dependencies and entry
/// point. Returns an empty detection when there is no parseable `package.json`
/// (e.g. a QML or non-JS service).
///
/// Note: `engines.node` is deliberately NOT read — webOS services don't set it
/// reliably, so it cannot be trusted as a runtime requirement.
pub fn detect_service_runtime(dir: &Path) -> ServiceRuntimeDetection {
    let text = fs::read_to_string(dir.join("package.json")).unwrap_or_default();
    let pkg: PackageJson = serde_json::from_str(&text).unwrap_or_default();

    ServiceRuntimeDetection {
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
    fn parses_main_and_deps() {
        let dir = write_pkg(
            r#"{ "main": "service.js", "engines": { "node": ">=12.0.0" },
                "dependencies": { "express": "^4.18.0", "lodash": "4.17.21" } }"#,
        );
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.main.as_deref(), Some("service.js"));
        assert_eq!(d.dependencies.len(), 2);
        assert_eq!(d.dependencies[0].0, "express");
    }

    #[test]
    fn missing_package_json_is_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = detect_service_runtime(dir.path());
        assert!(d.dependencies.is_empty());
        assert!(d.main.is_none());
    }
}
