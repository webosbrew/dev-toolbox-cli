use bin_lib::{BinaryInfo, LibraryInfo, LibraryPriority};
use ipk_lib::Component;

use crate::ipk::{ComponentBinVerifyResult, ComponentVerifyResult};
use crate::{bin::BinVerifyResult, Verify, VerifyResult};

trait ComponentImpl {
    fn verify_bin<F>(&self, bin: &BinaryInfo, find_library: &F) -> BinVerifyResult
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
        return true;
    }
}

impl<T> ComponentImpl for Component<T> {
    fn verify_bin<F>(&self, bin: &BinaryInfo, find_library: &F) -> BinVerifyResult
    where
        F: Fn(&str) -> Option<LibraryInfo>,
    {
        return bin.verify(&|name| {
            let lib = self.find_lib(name);
            return if let Some(lib) = lib {
                if lib.priority == LibraryPriority::Rpath {
                    return Some(lib.clone());
                }

                if let Some(sys) = find_library(name) {
                    return Some(sys.clone());
                }
                Some(lib.clone())
            } else {
                find_library(name)
            };
        });
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
                libs: Default::default(),
            };
        };
        let bin = self.verify_bin(exe, find_library);
        let mut libs: Vec<(bool, ComponentBinVerifyResult)> = self
            .libs
            .iter()
            .map(|lib| {
                let required = self.is_required(lib);
                // System library has higher precedence
                if lib.priority != LibraryPriority::Rpath {
                    if find_library(&lib.name).is_some() {
                        return (
                            required,
                            ComponentBinVerifyResult::Skipped {
                                name: lib.name.clone(),
                            },
                        );
                    }
                }
                let verify_result = self.verify_bin(
                    &BinaryInfo {
                        name: lib.name.clone(),
                        rpath: Default::default(),
                        needed: lib.needed.clone(),
                        undefined: lib.undefined.clone(),
                    },
                    find_library,
                );
                (
                    required,
                    if verify_result.is_good() {
                        ComponentBinVerifyResult::Ok {
                            name: verify_result.name,
                        }
                    } else {
                        ComponentBinVerifyResult::Failed(verify_result)
                    },
                )
            })
            .collect();
        libs.sort_by(|(required_a, lib_a), (required_b, lib_b)| {
            let required_cmp = required_a.cmp(required_b);
            if !required_cmp.is_eq() {
                return required_cmp.reverse();
            }
            return lib_a.name().cmp(&lib_b.name());
        });
        return ComponentVerifyResult {
            id: self.id.clone(),
            exe: if bin.is_good() {
                ComponentBinVerifyResult::Ok { name: bin.name }
            } else {
                ComponentBinVerifyResult::Failed(bin)
            },
            libs,
        };
    }
}

impl ComponentBinVerifyResult {
    pub fn name(&self) -> &str {
        return match self {
            ComponentBinVerifyResult::Skipped { name } => name,
            ComponentBinVerifyResult::Ok { name } => name,
            ComponentBinVerifyResult::Failed(result) => &result.name,
        };
    }
}
