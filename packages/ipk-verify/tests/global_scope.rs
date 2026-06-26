//! Regression tests for symbol resolution across the executable's global scope.
//!
//! The dynamic loader resolves every loaded object's undefined symbols against
//! a single global scope made up of the executable and its whole dependency
//! closure. A bundled library's imports can therefore be satisfied by a sibling
//! library the executable also loads, even with no direct `DT_NEEDED` link
//! between them (e.g. a `libEGL.so.1` shim whose `gl*` imports live in the
//! sibling `libGLESv2.so.2`). These tests pin that behaviour.

use bin_lib::{BinaryInfo, LibraryInfo, LibraryPriority};
use ipk_lib::Component;
use verify_lib::ipk::ComponentBinVerifyResult;
use verify_lib::Verify;

fn bundled_lib(name: &str, needed: &[&str], symbols: &[&str], undefined: &[&str]) -> LibraryInfo {
    let mut symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
    symbols.sort_unstable();
    LibraryInfo {
        name: name.to_string(),
        package: None,
        needed: needed.iter().map(|s| s.to_string()).collect(),
        symbols,
        names: vec![name.to_string()],
        undefined: undefined.iter().map(|s| s.to_string()).collect(),
        rpath: vec![],
        priority: LibraryPriority::Rpath,
    }
}

fn component(exe_needed: &[&str], libs: Vec<LibraryInfo>) -> Component<()> {
    Component {
        id: "test".to_string(),
        info: (),
        exe: Some(BinaryInfo {
            name: "app".to_string(),
            rpath: vec![],
            needed: exe_needed.iter().map(|s| s.to_string()).collect(),
            undefined: vec![],
        }),
        libs,
    }
}

fn lib_result<'a>(
    result: &'a verify_lib::ipk::ComponentVerifyResult,
    name: &str,
) -> &'a ComponentBinVerifyResult {
    &result
        .libs
        .iter()
        .find(|(_, lib)| lib.name() == name)
        .unwrap_or_else(|| panic!("no result for {name}"))
        .1
}

/// A `gl*` symbol imported by `libEGL.so.1` but only defined by the sibling
/// `libGLESv2.so.2` (not in libEGL's `DT_NEEDED`) must not be reported as
/// undefined. This is the exact false positive from apps-repo PR #190.
#[test]
fn sibling_library_satisfies_undefined_symbol() {
    // libGLESv2 exports the versioned symbol; libEGL imports it unversioned.
    let libegl = bundled_lib("libEGL.so.1", &[], &["eglGetDisplay"], &["glActiveTexture"]);
    let libgles = bundled_lib("libGLESv2.so.2", &[], &["glActiveTexture@GLES_3_2"], &[]);
    let component = component(&["libEGL.so.1", "libGLESv2.so.2"], vec![libegl, libgles]);

    // No firmware libraries available.
    let result = component.verify(&|_name| None);

    assert!(
        matches!(lib_result(&result, "libEGL.so.1"), ComponentBinVerifyResult::Ok { .. }),
        "libEGL.so.1 should pass: gl* provided by sibling libGLESv2.so.2; got {:?}",
        lib_result(&result, "libEGL.so.1")
    );
    assert!(matches!(
        lib_result(&result, "libGLESv2.so.2"),
        ComponentBinVerifyResult::Ok { .. }
    ));
}

/// A symbol that no library in the global scope provides must still be reported
/// as undefined — the global-scope resolution must not mask genuine misses.
#[test]
fn truly_missing_symbol_still_fails() {
    let libegl = bundled_lib("libEGL.so.1", &[], &["eglGetDisplay"], &["someMissingSymbol"]);
    let libgles = bundled_lib("libGLESv2.so.2", &[], &["glActiveTexture@GLES_3_2"], &[]);
    let component = component(&["libEGL.so.1", "libGLESv2.so.2"], vec![libegl, libgles]);

    let result = component.verify(&|_name| None);

    match lib_result(&result, "libEGL.so.1") {
        ComponentBinVerifyResult::Failed(r) => {
            assert!(r.undefined_sym.iter().any(|s| s == "someMissingSymbol"));
        }
        other => panic!("libEGL.so.1 should fail on the missing symbol, got {other:?}"),
    }
}
