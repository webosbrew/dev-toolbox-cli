use std::collections::HashSet;

use bin_lib::{BinaryInfo, LibraryInfo};
use fw_lib::Firmware;
use ipk_lib::Component;

use crate::bin::binary::recursive_resolve_symbols;
use crate::ipk::{ComponentBinVerifyResult, ComponentVerifyResult};
use crate::{bin::BinVerifyResult, VerifyResult, VerifyWithFirmware};

trait ComponentImpl {
    fn verify_bin(&self, bin: &BinaryInfo, firmware: &Firmware) -> BinVerifyResult;
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
    fn verify_bin(&self, bin: &BinaryInfo, firmware: &Firmware) -> BinVerifyResult {
        let mut result = bin.verify(firmware);
        let mut visited_libs: HashSet<String> = Default::default();
        result.missing_lib.retain_mut(|lib| {
            if let Some(lib) = self.find_lib(lib) {
                let find_library = |name: &str| -> Option<LibraryInfo> {
                    return firmware
                        .find_library(name)
                        .or_else(|| self.find_lib(name).cloned());
                };

                recursive_resolve_symbols(
                    &lib,
                    &mut result.undefined_sym,
                    &mut visited_libs,
                    &find_library,
                );
                return false;
            }
            return true;
        });
        return result;
    }
}

impl<T> VerifyWithFirmware<ComponentVerifyResult> for Component<T> {
    fn verify(&self, firmware: &Firmware) -> ComponentVerifyResult {
        let exe = if let Some(bin) = &self.exe {
            self.verify_bin(bin, firmware)
        } else {
            return ComponentVerifyResult {
                id: self.id.clone(),
                exe: ComponentBinVerifyResult::Skipped {
                    name: String::new(),
                },
                libs: Default::default(),
            };
        };
        let mut libs: Vec<(bool, ComponentBinVerifyResult)> = self
            .libs
            .iter()
            .map(|lib| {
                let required = self.is_required(lib);
                // System library has higher precedence
                if let Some(_) = firmware.find_library(&lib.name) {
                    return (
                        required,
                        ComponentBinVerifyResult::Skipped {
                            name: lib.name.clone(),
                        },
                    );
                }
                let verify_result = self.verify_bin(
                    &BinaryInfo {
                        name: lib.name.clone(),
                        rpath: Default::default(),
                        needed: lib.needed.clone(),
                        undefined: lib.undefined.clone(),
                    },
                    firmware,
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
            exe: if exe.is_good() {
                ComponentBinVerifyResult::Ok { name: exe.name }
            } else {
                ComponentBinVerifyResult::Failed(exe)
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
