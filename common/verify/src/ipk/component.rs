use std::collections::HashSet;

use bin_lib::{BinaryInfo, LibraryInfo, LibraryPriority};
use ipk_lib::Component;

use crate::bin::binary::recursive_resolve_symbols;
use crate::ipk::{ComponentBinVerifyResult, ComponentVerifyResult};
use crate::{bin::BinVerifyResult, Verify, VerifyResult};

trait ComponentImpl {
    fn resolve_lib<F>(&self, name: &str, find_library: &F) -> Option<LibraryInfo>
    where
        F: Fn(&str) -> Option<LibraryInfo>;

    fn verify_bin<F>(&self, bin: &BinaryInfo, find_library: &F) -> BinVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>;

    fn resolve_in_global_scope<F>(&self, result: &mut BinVerifyResult, find_library: &F)
    where
        F: Fn(&str) -> Option<LibraryInfo>;
}

impl VerifyResult for ComponentVerifyResult {
    fn is_good(&self) -> bool {
        if let ComponentBinVerifyResult::Failed { .. } = &self.exe {
            return false;
        }
        for (required, result) in &self.libs {
            if !required {
                continue;
            }
            if let ComponentBinVerifyResult::Failed { .. } = result {
                return false;
            }
        }
        // A definitive non-native incompatibility (e.g. app ES level exceeds the
        // firmware's web engine) also fails the component; Unknown does not.
        if let Some(detection) = &self.detection {
            if detection.is_incompatible() {
                return false;
            }
        }
        return true;
    }
}

impl<T> ComponentImpl for Component<T> {
    /// Resolve a needed library by name the same way the dynamic loader would
    /// for this component: a library bundled on the rpath takes precedence,
    /// otherwise the firmware (system) copy is preferred over a non-rpath
    /// bundled copy.
    fn resolve_lib<F>(&self, name: &str, find_library: &F) -> Option<LibraryInfo>
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        if let Some(lib) = self.find_lib(name) {
            if lib.priority == LibraryPriority::Rpath {
                return Some(lib.clone());
            }
            if let Some(sys) = find_library(name) {
                return Some(sys);
            }
            return Some(lib.clone());
        }
        return find_library(name);
    }

    fn verify_bin<F>(&self, bin: &BinaryInfo, find_library: &F) -> BinVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        return bin.verify(&|name| self.resolve_lib(name, find_library));
    }

    /// Strike off undefined symbols that are satisfied by the executable's
    /// global symbol scope.
    ///
    /// The dynamic loader places the executable and every library in its
    /// dependency closure into a single global scope, and resolves each loaded
    /// object's undefined symbols against that whole scope. A library's imports
    /// can therefore be satisfied by a *sibling* library the executable also
    /// loads, even without a direct `DT_NEEDED` link between the two — for
    /// example a `libEGL.so.1` shim whose `gl*` imports are provided by the
    /// sibling `libGLESv2.so.2`. Verifying each library only against its own
    /// `DT_NEEDED` chain misses this and produces false "undefined symbol"
    /// reports, so resolve whatever is left against the global scope.
    fn resolve_in_global_scope<F>(&self, result: &mut BinVerifyResult, find_library: &F)
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        let Some(exe) = &self.exe else {
            return;
        };
        let resolver = |name: &str| self.resolve_lib(name, find_library);
        let mut visited: HashSet<String> = HashSet::new();
        for needed in &exe.needed {
            if result.undefined_sym.is_empty() && result.undefined_sym_lazy.is_empty() {
                break;
            }
            if !visited.insert(needed.clone()) {
                continue;
            }
            let Some(lib) = resolver(needed) else {
                continue;
            };
            recursive_resolve_symbols(
                &lib,
                &mut result.undefined_sym,
                &mut result.undefined_sym_lazy,
                &mut visited,
                &resolver,
            );
        }
    }
}

impl<T> Verify<ComponentVerifyResult> for Component<T> {
    fn verify<F>(&self, find_library: &F) -> ComponentVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        let Some(exe) = &self.exe else {
            return ComponentVerifyResult {
                id: self.id.clone(),
                exe: ComponentBinVerifyResult::Skipped {
                    name: String::new(),
                },
                libs: Vec::new(),
                detection: None,
                bundled: Vec::new(),
            };
        };
        let bin = self.verify_bin(exe, find_library);
        let mut libs: Vec<(bool, ComponentBinVerifyResult)> = self
            .libs
            .iter()
            .map(|lib| {
                let required = self.is_required(lib);
                // System library has higher precedence
                if lib.priority != LibraryPriority::Rpath
                    && find_library(&lib.name).is_some() {
                        return (
                            required,
                            ComponentBinVerifyResult::Skipped {
                                name: lib.name.clone(),
                            },
                        );
                    }
                let mut verify_result = self.verify_bin(
                    &BinaryInfo {
                        name: lib.name.clone(),
                        rpath: Vec::new(),
                        needed: lib.needed.clone(),
                        undefined: lib.undefined.clone(),
                        undefined_lazy: lib.undefined_lazy.clone(),
                    },
                    find_library,
                );
                // A bundled library's imports may be provided by a sibling
                // library co-loaded by the executable, not just by its own
                // DT_NEEDED chain. Resolve the leftovers against that scope.
                self.resolve_in_global_scope(&mut verify_result, find_library);
                (required, verify_result.into())
            })
            .collect();
        libs.sort_by(|(required_a, lib_a), (required_b, lib_b)| {
            let required_cmp = required_a.cmp(required_b);
            if !required_cmp.is_eq() {
                return required_cmp.reverse();
            }
            return lib_a.name().cmp(lib_b.name());
        });
        return ComponentVerifyResult {
            id: self.id.clone(),
            exe: bin.into(),
            libs,
            // Filled in by Package::verify_for_firmware for non-native components.
            detection: None,
            bundled: Vec::new(),
        };
    }
}

/// A binary's verdict: fails on anything that stops it from loading, warns when
/// only a lazily-bound import is missing, otherwise passes.
impl From<BinVerifyResult> for ComponentBinVerifyResult {
    fn from(result: BinVerifyResult) -> Self {
        if !result.is_good() {
            return ComponentBinVerifyResult::Failed(result);
        }
        if result.has_warnings() {
            return ComponentBinVerifyResult::Warned(result);
        }
        return ComponentBinVerifyResult::Ok { name: result.name };
    }
}

impl ComponentBinVerifyResult {
    pub fn name(&self) -> &str {
        return match self {
            ComponentBinVerifyResult::Skipped { name } | ComponentBinVerifyResult::Ok { name } => name,
            ComponentBinVerifyResult::Warned(result) | ComponentBinVerifyResult::Failed(result) => {
                &result.name
            }
        };
    }
}
