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

/// Inspect a service directory: read `package.json` for the entry point, then
/// analyze the code the firmware's Node.js actually runs for its ES language
/// level (checked against that Node.js) and runtime-API usage.
///
/// The scan starts at `main` and follows only the static, relative module graph
/// (`require`/`import`) — not every `.js` in the tree. A service often bundles
/// vendored code (`node_modules`, or a server it spawns on its own bundled Node)
/// whose ES level says nothing about the firmware Node; grading that would
/// wrongly fail packages that ship a newer Node themselves.
///
/// Note: `engines.node` is deliberately NOT read — webOS services don't set it
/// reliably. The ES level, by contrast, is derived from the actual code and so
/// is trustworthy.
pub fn detect_service_runtime(dir: &Path) -> ServiceRuntimeDetection {
    let text = fs::read_to_string(dir.join("package.json")).unwrap_or_default();
    let pkg: PackageJson = serde_json::from_str(&text).unwrap_or_default();

    let mut sources: Vec<(String, String)> = Vec::new();
    js::collect_service_js(dir, pkg.main.as_deref(), &mut sources);
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
    use crate::EsLevel;
    use std::path::Path;

    fn write(dir: &Path, rel: &str, body: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }

    fn write_pkg(body: &str) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        write(dir.path(), "package.json", body);
        dir
    }

    #[test]
    fn parses_main() {
        let dir = write_pkg(r#"{ "main": "service.js", "name": "com.example.app.service" }"#);
        write(dir.path(), "service.js", "var x = 1;");
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.main.as_deref(), Some("service.js"));
    }

    #[test]
    fn missing_package_json_is_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let d = detect_service_runtime(dir.path());
        assert!(d.main.is_none());
        assert!(d.es_level.is_none());
    }

    #[test]
    fn grades_only_the_entry_graph_not_the_whole_tree() {
        // The entry point is plain ES5 and pulls in nothing; a vendored server
        // and node_modules ship modern syntax but never run on the firmware Node.
        let dir = write_pkg(r#"{ "main": "service.js" }"#);
        write(dir.path(), "service.js", "var x = 1; function f() { return x; }");
        write(dir.path(), "server/index.js", "const y = a?.b ?? c;"); // ES2020, spawned separately
        write(dir.path(), "node_modules/dep/index.js", "const z = () => 2;"); // ES2015, vendored
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.es_level, Some(EsLevel::Es5));
    }

    #[test]
    fn follows_relative_require() {
        let dir = write_pkg(r#"{ "main": "service.js" }"#);
        write(dir.path(), "service.js", "var helper = require('./lib/helper');");
        write(dir.path(), "lib/helper.js", "const v = a ?? b;"); // ES2020
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.es_level, Some(EsLevel::Es2020));
    }

    #[test]
    fn follows_relative_import_and_export_from() {
        let dir = write_pkg(r#"{ "main": "service.js" }"#);
        write(
            dir.path(),
            "service.js",
            "import x from './a.js'; export { y } from './b.js';",
        );
        write(dir.path(), "a.js", "var a = 1;");
        write(dir.path(), "b.js", "class Widget {}"); // ES2015
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.es_level, Some(EsLevel::Es2015));
    }

    #[test]
    fn does_not_follow_bare_specifiers() {
        // `require('modern-dep')` resolves into node_modules — not graded.
        let dir = write_pkg(r#"{ "main": "service.js" }"#);
        write(dir.path(), "service.js", "var d = require('modern-dep');");
        write(dir.path(), "node_modules/modern-dep/index.js", "const z = a?.b;"); // ES2020
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.es_level, Some(EsLevel::Es5));
    }

    #[test]
    fn defaults_entry_to_index_js() {
        let dir = write_pkg(r#"{ "name": "com.example.app.service" }"#);
        write(dir.path(), "index.js", "const v = a ?? b;"); // ES2020
        let d = detect_service_runtime(dir.path());
        assert_eq!(d.es_level, Some(EsLevel::Es2020));
    }
}
