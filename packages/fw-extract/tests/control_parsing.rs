//! The opkg control parsing this extractor depends on.
//!
//! `extractor.rs` reads every `*.control` file in a firmware image to learn
//! which package owns which library. No firmware image is small enough to ship
//! as a fixture, so nothing else here exercises `debian-control` or
//! `debversion`. These tests pin the exact calls that code makes, so a bump of
//! either crate cannot change them without a failure.

use std::fs;

use debian_control::Control;
use debversion::AsVersion;

fn control(body: &str) -> Control {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("pkg.control");
    fs::write(&path, body).unwrap();
    Control::from_file(&path).unwrap()
}

#[test]
fn reads_the_package_name_and_version() {
    let ctrl = control("Package: webos-base\nVersion: 1:2.3.4-5\nArchitecture: armv7ahf\n");
    let bin = ctrl.binaries().next().expect("one binary stanza");

    assert_eq!(bin.name().unwrap(), "webos-base");
    assert_eq!(bin.as_deb822().get("Version").unwrap(), "1:2.3.4-5");
}

#[test]
fn splits_a_version_into_epoch_upstream_and_revision() {
    let version = "1:2.3.4-5".into_version().unwrap();

    assert_eq!(version.epoch, Some(1));
    assert_eq!(version.upstream_version, "2.3.4");
    assert_eq!(version.debian_revision.as_deref(), Some("5"));
}

#[test]
fn a_bare_version_has_no_epoch_or_revision() {
    let version = "2.3.4".into_version().unwrap();

    assert_eq!(version.epoch, None);
    assert_eq!(version.upstream_version, "2.3.4");
    assert_eq!(version.debian_revision, None);
}

/// The extractor falls back to treating the raw string as the upstream version
/// when it does not parse. Keep a case that actually fails to parse, so that
/// fallback stays reachable.
#[test]
fn rejects_a_version_it_cannot_parse() {
    assert!("not a version".into_version().is_err());
}
