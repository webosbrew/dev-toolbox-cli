//! An undefined symbol only warns when the loader binds it lazily.
//!
//! A function called through the PLT gets its relocation resolved on the first
//! call, so a missing definition does not stop the binary from loading — the
//! process only aborts if that code path runs. RetroArch, for instance, imports
//! GLES3 entry points that a GLES2-only firmware lacks, and never calls them
//! unless it gets a GLES3 context. Anything resolved at load time still fails.

use bin_lib::{BinaryInfo, LibraryInfo, LibraryPriority};
use ipk_lib::Component;
use verify_lib::ipk::ComponentBinVerifyResult;
use verify_lib::{Verify, VerifyResult};

fn lib(name: &str, symbols: &[&str]) -> LibraryInfo {
    let mut symbols: Vec<String> = symbols.iter().map(|s| s.to_string()).collect();
    symbols.sort_unstable();
    LibraryInfo {
        name: name.to_string(),
        package: None,
        needed: vec![],
        symbols,
        names: vec![name.to_string()],
        undefined: vec![],
        undefined_lazy: vec![],
        rpath: vec![],
        priority: LibraryPriority::Rpath,
    }
}

/// An app whose executable imports `eager` at load time and `lazy` through the
/// PLT, with `libs` bundled alongside it.
fn app(eager: &[&str], lazy: &[&str], libs: Vec<LibraryInfo>) -> Component<()> {
    Component {
        id: "test".to_string(),
        info: (),
        exe: Some(BinaryInfo {
            name: "app".to_string(),
            rpath: vec![],
            needed: libs.iter().map(|l| l.name.clone()).collect(),
            undefined: eager.iter().map(|s| s.to_string()).collect(),
            undefined_lazy: lazy.iter().map(|s| s.to_string()).collect(),
        }),
        libs,
    }
}

#[test]
fn missing_lazy_symbol_only_warns() {
    let result = app(&[], &["glTexStorage2D"], vec![lib("libGLESv2.so.2", &[])]).verify(&|_| None);

    match &result.exe {
        ComponentBinVerifyResult::Warned(bin) => {
            assert_eq!(bin.undefined_sym_lazy, vec!["glTexStorage2D"]);
            assert!(bin.undefined_sym.is_empty());
        }
        other => panic!("expected a warning, got {other:?}"),
    }
    assert!(
        result.is_good(),
        "a lazily-bound import must not fail the component"
    );
}

#[test]
fn missing_eager_symbol_still_fails() {
    let result = app(&["someDataSymbol"], &[], vec![lib("libGLESv2.so.2", &[])]).verify(&|_| None);

    match &result.exe {
        ComponentBinVerifyResult::Failed(bin) => {
            assert_eq!(bin.undefined_sym, vec!["someDataSymbol"]);
        }
        other => panic!("expected a failure, got {other:?}"),
    }
    assert!(!result.is_good());
}

#[test]
fn resolved_lazy_symbol_passes() {
    let libgles = lib("libGLESv2.so.2", &["glTexStorage2D@GLES_3_2"]);
    let result = app(&[], &["glTexStorage2D"], vec![libgles]).verify(&|_| None);

    assert!(
        matches!(&result.exe, ComponentBinVerifyResult::Ok { .. }),
        "the bundled library defines it, so there is nothing to warn about; got {:?}",
        result.exe
    );
}

/// A failure outranks a warning, and the report still lists both.
#[test]
fn eager_and_lazy_together_fail() {
    let result =
        app(&["someDataSymbol"], &["glTexStorage2D"], vec![lib("libGLESv2.so.2", &[])])
            .verify(&|_| None);

    match &result.exe {
        ComponentBinVerifyResult::Failed(bin) => {
            assert_eq!(bin.undefined_sym, vec!["someDataSymbol"]);
            assert_eq!(bin.undefined_sym_lazy, vec!["glTexStorage2D"]);
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}
