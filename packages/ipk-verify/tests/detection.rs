//! Tests for non-native technology compatibility verdicts.
//!
//! These exercise `Package::verify_for_firmware` end-to-end from parsed
//! detection facts to a per-firmware `CompatVerdict` and the `is_good` exit
//! signal, without needing a real `.ipk` on disk.

use bin_lib::LibraryInfo;
use fw_lib::WebEngine;
use ipk_lib::{AppInfo, Component, Package, ServiceInfo};
use semver::{Version, VersionReq};
use verify_lib::ipk::{CompatVerdict, VerifyForFirmware};
use verify_lib::VerifyResult;
use webdetect_lib::{
    EsLevel, FrameworkInfo, FrameworkKind, ServiceRuntimeDetection, WebAppDetection,
};

fn web_app(es: EsLevel) -> Component<AppInfo> {
    Component {
        id: "com.example.app".to_string(),
        info: AppInfo {
            id: "com.example.app".to_string(),
            version: "1.0.0".to_string(),
            r#type: "web".to_string(),
            title: "Example".to_string(),
            app_description: None,
            main: "index.html".to_string(),
            web: Some(WebAppDetection {
                framework: Some(FrameworkInfo::new(
                    FrameworkKind::React,
                    Version::parse("18.2.0").ok(),
                )),
                also_present: vec![],
                webostvjs: None,
                es_level: Some(es),
                es_features: vec![],
                remote_resources: vec![],
            }),
        },
        exe: None,
        libs: vec![],
    }
}

fn node_service(id: &str, req: &str) -> Component<ServiceInfo> {
    Component {
        id: id.to_string(),
        info: ServiceInfo {
            id: id.to_string(),
            engine: Some("node".to_string()),
            executable: None,
            runtime: Some(ServiceRuntimeDetection {
                declared_node: VersionReq::parse(req).ok(),
                dependencies: vec![],
                main: None,
            }),
        },
        exe: None,
        libs: vec![],
    }
}

fn package(app: Component<AppInfo>, services: Vec<Component<ServiceInfo>>) -> Package {
    Package {
        id: "com.example.app".to_string(),
        installed_size: None,
        app,
        services,
    }
}

fn no_libs(_: &str) -> Option<LibraryInfo> {
    None
}

#[test]
fn web_app_es_level_checked_against_engine() {
    let pkg = package(web_app(EsLevel::Es2017), vec![]);

    // Chromium 120 supports ES2017 → OK, component is good.
    let r = pkg.verify_for_firmware(&no_libs, None, Some(&WebEngine::Chromium(Version::new(120, 0, 0))));
    assert_eq!(r.app.detection.as_ref().unwrap().verdict(), &CompatVerdict::Ok);
    assert!(r.app.is_good());

    // Chromium 53 (webOS 4) predates async/await → FAIL, component not good.
    let r = pkg.verify_for_firmware(&no_libs, None, Some(&WebEngine::Chromium(Version::new(53, 0, 2785))));
    assert!(matches!(
        r.app.detection.as_ref().unwrap().verdict(),
        CompatVerdict::Fail { .. }
    ));
    assert!(!r.app.is_good());

    // Legacy WebKit engine → FAIL.
    let r = pkg.verify_for_firmware(&no_libs, None, Some(&WebEngine::WebKit(Version::new(537, 41, 0))));
    assert!(matches!(
        r.app.detection.as_ref().unwrap().verdict(),
        CompatVerdict::Fail { .. }
    ));

    // Firmware with no known web engine → Unknown, which must NOT fail the build.
    let r = pkg.verify_for_firmware(&no_libs, None, None);
    assert_eq!(r.app.detection.as_ref().unwrap().verdict(), &CompatVerdict::Unknown);
    assert!(r.app.is_good());
}

#[test]
fn multiple_services_each_get_their_own_node_verdict() {
    let pkg = package(
        web_app(EsLevel::Es5),
        vec![
            node_service("com.example.app.svc1", ">=12.0.0"),
            node_service("com.example.app.svc2", ">=99.0.0"),
        ],
    );

    // webOS 10.2 ships Node 16.20.2.
    let r = pkg.verify_for_firmware(
        &no_libs,
        Some(&Version::new(16, 20, 2)),
        Some(&WebEngine::Chromium(Version::new(120, 0, 0))),
    );
    assert_eq!(r.services.len(), 2);
    assert_eq!(r.services[0].detection.as_ref().unwrap().verdict(), &CompatVerdict::Ok);
    assert!(r.services[0].is_good());
    assert!(matches!(
        r.services[1].detection.as_ref().unwrap().verdict(),
        CompatVerdict::Fail { .. }
    ));
    assert!(!r.services[1].is_good());

    // No Node package on the firmware → Unknown for both, never a fail.
    let r = pkg.verify_for_firmware(&no_libs, None, None);
    assert_eq!(r.services[0].detection.as_ref().unwrap().verdict(), &CompatVerdict::Unknown);
    assert_eq!(r.services[1].detection.as_ref().unwrap().verdict(), &CompatVerdict::Unknown);
    assert!(r.services[1].is_good());
}
