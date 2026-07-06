//! Node.js service runtime detection from a bundled `package.json`.

use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::js;
use crate::ServiceRuntimeDetection;

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    main: Option<String>,
}

/// Inspect a service directory: read `package.json` for the entry point, and
/// analyze the service's own `.js` code for its ES language level
/// (checked against the firmware's Node.js) and runtime-API usage.
///
/// Note: `engines.node` is deliberately NOT read — webOS services don't set it
/// reliably. The ES level, by contrast, is derived from the actual code and so
/// is trustworthy.
pub fn detect_service_runtime(dir: &Path) -> ServiceRuntimeDetection {
    let text = fs::read_to_string(dir.join("package.json")).unwrap_or_default();
    let pkg: PackageJson = serde_json::from_str(&text).unwrap_or_default();

    let mut sources: Vec<(String, String)> = Vec::new();
    js::collect_js(dir, 0, &mut sources);
    let analysis = js::analyze_js(&sources, false);

    ServiceRuntimeDetection {
        main: pkg.main,
        es_level: analysis.es_level,
        es_features: analysis.es_features,
        es_apis: analysis.es_apis,
        polyfills: js::detect_polyfills(&sources),
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
    fn parses_main() {
        let dir = write_pkg(
            r#"{ "main": "service.js", "name": "com.example.app.service" }"#,
        );
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.main.as_deref(), Some("service.js"));
    }

    #[test]
    fn missing_package_json_is_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = detect_service_runtime(dir.path());
        assert!(d.main.is_none());
    }
}
